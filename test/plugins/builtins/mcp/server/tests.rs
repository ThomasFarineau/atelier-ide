use super::*;

/// One request to the server, as an MCP client would send it: the status and the body.
fn post(port: u16, token: Option<&str>, extra: &str, body: &str) -> (u16, String) {
    let (status, _, body) = post_with_headers(port, token, extra, body);
    (status, body)
}

/// The same, with the headers of the answer.
fn post_with_headers(
    port: u16,
    token: Option<&str>,
    extra: &str,
    body: &str,
) -> (u16, String, String) {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    write!(
            stream,
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}{extra}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    let status = reply
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let (head, body) = reply
        .split_once("\r\n\r\n")
        .map(|(h, b)| (h.to_string(), b.to_string()))
        .unwrap_or_default();
    (status, head, body)
}

fn running() -> (Server, u16) {
    let server = start(0, "secret".into(), Arc::new(|| {})).unwrap();
    let port = server.port;
    (server, port)
}

#[test]
fn a_client_initializes_lists_the_tools_and_calls_one() {
    let (server, port) = running();
    let (status, body) = post(
        port,
        Some("secret"),
        "",
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
    );
    assert_eq!(status, 200);
    let answer: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        answer["result"]["protocolVersion"], "2025-03-26",
        "the version the client asked for"
    );
    assert_eq!(answer["result"]["serverInfo"]["name"], "atelier");
    let (status, body) = post(
        port,
        Some("secret"),
        "",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    );
    assert_eq!(
        (status, body.as_str()),
        (202, ""),
        "a notification is not answered"
    );
    let (_, body) = post(
        port,
        Some("secret"),
        "",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    );
    let tools: Value = serde_json::from_str(&body).unwrap();
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        names.contains(&"get_problems")
            && names.contains(&"open_file")
            && names.contains(&"search_in_project"),
        "{names:?}"
    );
    // A tool of the editor goes through the channel and is answered by the interface thread.
    let answering = std::thread::spawn(move || {
        let call = server.calls.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(call.tool, "get_open_files");
        call.reply.send(Ok("[]".into())).unwrap();
    });
    let (_, body) = post(
        port,
        Some("secret"),
        "",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_open_files","arguments":{}}}"#,
    );
    answering.join().unwrap();
    let answer: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(answer["result"]["content"][0]["text"], "[]");
    assert_eq!(answer["result"]["isError"], false);
}

#[test]
fn unknown_methods_and_tools_get_json_rpc_errors() {
    let (_server, port) = running();
    let (_, body) = post(
        port,
        Some("secret"),
        "",
        r#"{"jsonrpc":"2.0","id":7,"method":"resources/list"}"#,
    );
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["error"]["code"],
        -32601
    );
    let (_, body) = post(
        port,
        Some("secret"),
        "",
        r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"format_disk"}}"#,
    );
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["error"]["code"],
        -32602
    );
    let (status, _) = post(port, Some("secret"), "", "pas du json");
    assert_eq!(status, 400);
}

#[test]
fn without_the_token_or_from_a_web_page_nothing_is_answered() {
    let (_server, port) = running();
    let ping = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    assert_eq!(post(port, None, "", ping).0, 401);
    assert_eq!(post(port, Some("wrong"), "", ping).0, 401);
    assert_eq!(
        post(
            port,
            Some("secret"),
            "Origin: https://evil.example\r\n",
            ping
        )
        .0,
        403,
        "a page of the browser"
    );
    assert_eq!(post(port, Some("secret"), "", ping).0, 200);
}

#[test]
fn a_connection_is_a_session_that_records_its_calls_and_keeps_its_notes() {
    let (server, port) = running();
    let dir = std::env::temp_dir().join(format!("atelier-mcp-session-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    server.store.lock().unwrap().set_root(&dir);
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"claude-code","version":"2.1"}}}"#;
    let (_, head, _) = post_with_headers(port, Some("secret"), "", init);
    let id = head
        .lines()
        .find_map(|l| l.strip_prefix("Mcp-Session-Id: "))
        .expect("the client is given its session")
        .trim()
        .to_string();
    let with_session = format!("Mcp-Session-Id: {id}\r\n");
    let note = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"session_note","arguments":{"kind":"goal","text":"Réparer le build"}}}"#;
    let (_, body) = post(port, Some("secret"), &with_session, note);
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["result"]["isError"],
        false,
        "{body}"
    );
    let search = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_in_project","arguments":{"query":"fn main"}}}"#;
    post(port, Some("secret"), &with_session, search);
    let context = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_session_context","arguments":{}}}"#;
    let (_, body) = post(port, Some("secret"), &with_session, context);
    let text = serde_json::from_str::<Value>(&body).unwrap()["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        text.contains("Réparer le build") && text.contains("claude-code"),
        "{text}"
    );
    {
        let store = server.store.lock().unwrap();
        assert_eq!(store.sessions.len(), 1, "one connection, one session");
        let tools: Vec<&str> = store.sessions[0]
            .events
            .iter()
            .map(|e| e.tool.as_str())
            .collect();
        assert_eq!(
            tools,
            ["session_note", "search_in_project", "get_session_context"]
        );
        assert_eq!(store.sessions[0].title.as_deref(), Some("Réparer le build"));
    }
    // Closing the connection ends the session.
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    write!(stream, "DELETE /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer secret\r\n{with_session}\r\n").unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(server.store.lock().unwrap().sessions[0].ended.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_commit_commands_are_offered_as_prompts_and_the_commit_tools_are_listed() {
    let (_server, port) = running();
    let (_, body) = post(
        port,
        Some("secret"),
        "",
        r#"{"jsonrpc":"2.0","id":1,"method":"prompts/list"}"#,
    );
    let list: Value = serde_json::from_str(&body).unwrap();
    let names: Vec<&str> = list["result"]["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    assert_eq!(
        names,
        ["commit", "commit-all", "commit-by-context", "rework-commit"]
    );
    let (_, body) = post(
        port,
        Some("secret"),
        "",
        r#"{"jsonrpc":"2.0","id":5,"method":"prompts/get","params":{"name":"rework-commit","arguments":{"range":"last 10 commits"}}}"#,
    );
    let rework: Value = serde_json::from_str(&body).unwrap();
    assert!(
        rework["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("last 10 commits")
    );
    let (_, body) = post(
        port,
        Some("secret"),
        "",
        r#"{"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":"commit","arguments":{"mode":"context"}}}"#,
    );
    let got: Value = serde_json::from_str(&body).unwrap();
    let text = got["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(
        text.contains("“context” mode") && text.contains("Co-authored-by"),
        "{text}"
    );
    let (_, body) = post(
        port,
        Some("secret"),
        "",
        r#"{"jsonrpc":"2.0","id":3,"method":"prompts/get","params":{"name":"nope"}}"#,
    );
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["error"]["code"],
        -32602
    );
    let (_, body) = post(
        port,
        Some("secret"),
        "",
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#,
    );
    let tools: Value = serde_json::from_str(&body).unwrap();
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        [
            "git_commit_plan",
            "git_diff",
            "git_commit",
            "git_rework_plan",
            "git_show",
            "git_rework_apply"
        ]
        .iter()
        .all(|n| names.contains(n)),
        "{names:?}"
    );
}
