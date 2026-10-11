use super::*;

#[test]
fn dates_are_read_with_their_offset() {
    assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_time("1970-01-02T00:00:01.5Z"), Some(DAY_MS + 1500));
    assert_eq!(
        parse_time("2026-10-10T13:35:27.240Z"),
        Some(1_791_639_327_240)
    );
    assert_eq!(
        parse_time("2026-10-10T15:35:27.240+02:00"),
        parse_time("2026-10-10T13:35:27.240Z")
    );
    assert_eq!(parse_time("2024-02-29T00:00:00Z"), Some(1_709_164_800_000));
    assert_eq!(parse_time("hier"), None);
}

#[test]
fn claude_answers_are_counted_once() {
    let answer = |id: &str, at: &str| {
        format!(
            r#"{{"cwd":"C:\\p","timestamp":"{at}","message":{{"model":"claude-opus-5-5","id":"{id}","usage":{{"input_tokens":2,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":50}}}}}}"#
        )
    };
    let text = [
        r#"{"type":"user","cwd":"C:\\p","message":{"role":"user","content":"x"}}"#.to_string(),
        answer("a", "2026-10-10T10:00:00Z"),
        answer("a", "2026-10-10T10:00:00Z"),
        answer("b", "2026-10-10T10:01:00Z"),
        r#"{"timestamp":"2026-10-10T10:02:00Z","message":{"model":"<synthetic>","id":"c","usage":{"input_tokens":0,"output_tokens":0}}}"#.to_string(),
    ]
    .join("\n");
    let usage = claude_file(text.as_bytes());
    assert_eq!(usage.cwd.as_deref(), Some("C:\\p"));
    assert_eq!(
        usage.records.len(),
        2,
        "one record per answer, none for synthetic ones"
    );
    assert_eq!(
        usage.records[0].tokens,
        Tokens {
            input: 102,
            cached: 1000,
            output: 50
        }
    );
}

#[test]
fn codex_turns_and_limits_are_read() {
    let text = r#"{"timestamp":"2026-10-10T13:35:22.978Z","type":"session_meta","payload":{"cwd":"C:\\p"}}
{"timestamp":"2026-10-10T13:35:23.870Z","type":"turn_context","payload":{"model":"gpt-5-codex"}}
{"timestamp":"2026-10-10T13:35:24Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":1.0,"window_minutes":300,"resets_at":1791657272}}}}
{"timestamp":"2026-10-10T13:35:27.240Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":7967,"cached_input_tokens":4864,"output_tokens":137}},"rate_limits":{"primary":{"used_percent":2.0,"window_minutes":300,"resets_at":1791657272},"secondary":{"used_percent":17.0,"window_minutes":10080,"resets_at":1791961180},"plan_type":"plus"}}}
"#;
    let usage = codex_file(text.as_bytes());
    assert_eq!(usage.cwd.as_deref(), Some("C:\\p"));
    assert_eq!(usage.records.len(), 1);
    assert_eq!(usage.records[0].model, "gpt-5-codex");
    assert_eq!(
        usage.records[0].tokens,
        Tokens {
            input: 3103,
            cached: 4864,
            output: 137
        }
    );
    let (_, limits, plan) = usage.limits.unwrap();
    assert_eq!(limits.len(), 2);
    assert_eq!(limits[1].window_minutes, 10080);
    assert_eq!(limits[1].used_percent, 17.0);
    assert_eq!(plan.as_deref(), Some("plus"));
}

#[test]
fn gemini_messages_are_read() {
    let text = r#"{"sessionId":"s","messages":[
        {"id":"1","timestamp":"2026-10-10T10:00:00Z","type":"user","content":"x"},
        {"id":"2","timestamp":"2026-10-10T10:00:05Z","type":"gemini","model":"gemini-2.5-pro","tokens":{"input":1000,"output":20,"cached":400,"thoughts":30,"tool":0,"total":1050}}
    ]}"#;
    let usage = gemini_file(text);
    assert_eq!(usage.records.len(), 1);
    assert_eq!(
        usage.records[0].tokens,
        Tokens {
            input: 600,
            cached: 400,
            output: 50
        }
    );
    assert_eq!(gemini_project_hash(Path::new("/p")).len(), 64);
}

#[test]
fn the_summary_splits_by_window_project_and_model() {
    let now = parse_time("2026-10-10T12:00:00Z").unwrap();
    let record = |hours_ago: u64, model: &str, id: Option<&str>| Record {
        at: now - hours_ago * 3600 * 1000,
        model: model.into(),
        tokens: Tokens {
            input: 10,
            cached: 0,
            output: 0,
        },
        id: id.map(Into::into),
    };
    let here = Arc::new(FileUsage {
        cwd: Some("C:\\Users\\me\\p\\".into()),
        records: vec![record(1, "a", Some("x")), record(48, "b", None)],
        limits: Some((now - 1000, Vec::new(), None)),
    });
    let elsewhere = Arc::new(FileUsage {
        cwd: Some("C:\\other".into()),
        // A resumed session repeats the answer `x`; the last is too old.
        records: vec![
            record(1, "a", Some("x")),
            record(200, "a", None),
            record(24 * 40, "a", None),
        ],
        limits: Some((now, Vec::new(), Some("pro".into()))),
    });
    let usage = summarize(&[here, elsewhere], Path::new("c:/users/me/p"), None, now);
    assert_eq!(usage.day.total(), 10);
    assert_eq!(usage.week.total(), 20);
    assert_eq!(usage.month.total(), 30);
    assert_eq!(
        usage.project.total(),
        20,
        "the folder matches whatever its case and slashes"
    );
    assert_eq!(usage.answers, 3);
    assert_eq!(usage.sessions, 2);
    assert_eq!(usage.models, [("a".to_string(), 20), ("b".to_string(), 10)]);
    assert_eq!(usage.last, Some(now - 3600 * 1000));
    assert_eq!(
        usage.limits.unwrap().2.as_deref(),
        Some("pro"),
        "the latest limits"
    );
}

#[test]
fn versions_and_counts_are_short() {
    assert_eq!(
        clean_version("2.1.296 (Claude Code)").as_deref(),
        Some("2.1.296")
    );
    assert_eq!(
        clean_version("codex-cli 0.158.0").as_deref(),
        Some("0.158.0")
    );
    assert_eq!(clean_version("Gemini"), None);
    assert_eq!(short_count(999), "999");
    assert_eq!(short_count(1_234_567), "1,2 M", "French in the tests");
    assert_eq!(short_count(45_000), "45 k");
    assert_eq!(short_count(2_000), "2 k");
}
