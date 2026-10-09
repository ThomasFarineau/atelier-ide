//! The AI command lines the IDE knows how to connect to its MCP server, and the configuration
//! each one reads. The secret is never written in the files: it is read from `ATELIER_MCP_TOKEN`,
//! which the terminals of the IDE define. The address is read from `ATELIER_MCP_URL` too, where
//! the AI expands variables (Claude Code, Gemini CLI), so those files stay valid when the port
//! changes and can be committed. Codex does not, so its file holds the address of the moment and
//! is written again when it changes.

use serde_json::{Map, Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The server name in the configuration of the AI.
pub const SERVER: &str = "atelier";
pub const URL_VAR: &str = "ATELIER_MCP_URL";
pub const TOKEN_VAR: &str = "ATELIER_MCP_TOKEN";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Agent {
    Claude,
    Gemini,
    Codex,
}

impl Agent {
    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "Claude Code",
            Agent::Gemini => "Gemini CLI",
            Agent::Codex => "Codex",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Gemini => "gemini",
            Agent::Codex => "codex",
        }
    }

    /// The file of the project that declares its MCP servers.
    fn config(self, root: &Path) -> PathBuf {
        match self {
            Agent::Claude => root.join(".mcp.json"),
            Agent::Gemini => root.join(".gemini").join("settings.json"),
            Agent::Codex => root.join(".codex").join("config.toml"),
        }
    }

    /// What this AI is told about the server.
    fn entry(self) -> Value {
        let headers = json!({"Authorization": format!("Bearer ${{{TOKEN_VAR}}}")});
        let url = format!("${{{URL_VAR}}}");
        match self {
            Agent::Claude => json!({"type": "http", "url": url, "headers": headers}),
            Agent::Gemini => json!({"httpUrl": url, "headers": headers}),
            Agent::Codex => unreachable!("Codex is configured in TOML"),
        }
    }
}

/// The AI a line typed in a terminal starts, if it is one the IDE can connect.
pub fn detect(line: &str) -> Option<Agent> {
    let program = line.trim().split_whitespace().next()?;
    let name = program.rsplit(['/', '\\']).next()?.to_lowercase();
    let name = name
        .trim_end_matches(".exe")
        .trim_end_matches(".cmd")
        .trim_end_matches(".ps1");
    match name {
        "claude" => Some(Agent::Claude),
        "gemini" => Some(Agent::Gemini),
        "codex" => Some(Agent::Codex),
        _ => None,
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            format!(
                "{} n'est pas du JSON valide ({e}) : rien n'a été modifié",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("{} : {e}", path.display())),
    }
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{} : {e}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    text.push('\n');
    fs::write(path, text).map_err(|e| format!("{} : {e}", path.display()))
}

/// The object at `key` of `value`, made if it is missing.
fn object<'a>(value: &'a mut Value, key: &str) -> Result<&'a mut Map<String, Value>, String> {
    value
        .as_object_mut()
        .ok_or("le fichier de configuration n'est pas un objet JSON")?
        .entry(key)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("« {key} » n'est pas un objet JSON"))
}

/// Whether the project's configuration for `agent` declares the server (for Codex: at the
/// address `url` of the running one).
pub fn is_connected(agent: Agent, root: &Path, url: Option<&str>) -> bool {
    if agent == Agent::Codex {
        let doc = fs::read_to_string(agent.config(root))
            .ok()
            .and_then(|t| t.parse::<toml_edit::DocumentMut>().ok());
        let found = doc
            .as_ref()
            .and_then(|d| d.get("mcp_servers")?.get(SERVER)?.get("url")?.as_str());
        return found.is_some() && found == url;
    }
    read_json(&agent.config(root)).is_ok_and(|v| v["mcpServers"].get(SERVER).is_some())
}

/// Declares the server in `.codex/config.toml`, keeping the rest of the file, comments included.
fn connect_codex(config: &Path, url: &str) -> Result<(), String> {
    let text = match fs::read_to_string(config) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{} : {e}", config.display())),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| {
        format!(
            "{} n'est pas du TOML valide ({e}) : rien n'a été modifié",
            config.display()
        )
    })?;
    let servers = doc.entry("mcp_servers").or_insert_with(|| {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        toml_edit::Item::Table(table)
    });
    let servers = servers
        .as_table_mut()
        .ok_or("« mcp_servers » n'est pas une table TOML")?;
    let entry = servers
        .entry(SERVER)
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or("« mcp_servers.atelier » n'est pas une table TOML")?;
    entry["url"] = toml_edit::value(url);
    entry["bearer_token_env_var"] = toml_edit::value(TOKEN_VAR);
    if let Some(parent) = config.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{} : {e}", parent.display()))?;
    }
    fs::write(config, doc.to_string()).map_err(|e| format!("{} : {e}", config.display()))
}

/// Declares the server in the project's configuration for `agent`, keeping the rest of the
/// file. `url` is the address of the running server, which only Codex needs written. The files
/// written, for the message to the user.
pub fn connect(agent: Agent, root: &Path, url: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let config = agent.config(root);
    if agent == Agent::Codex {
        let url = url.ok_or("le serveur MCP n'est pas démarré")?;
        connect_codex(&config, url)?;
        return Ok(vec![config]);
    }
    let mut file = read_json(&config)?;
    object(&mut file, "mcpServers")?.insert(SERVER.into(), agent.entry());
    let mut written = vec![config.clone()];
    // Claude Code asks before it trusts a server of a project file: the user has just said yes.
    let approval =
        (agent == Agent::Claude).then(|| root.join(".claude").join("settings.local.json"));
    let approved = match &approval {
        Some(path) => {
            let mut settings = read_json(path)?;
            let list = settings
                .as_object_mut()
                .ok_or("settings.local.json n'est pas un objet JSON")?
                .entry("enabledMcpjsonServers")
                .or_insert_with(|| json!([]));
            let list = list
                .as_array_mut()
                .ok_or("« enabledMcpjsonServers » n'est pas une liste")?;
            if !list.iter().any(|s| s == SERVER) {
                list.push(json!(SERVER));
            }
            Some((path.clone(), settings))
        }
        None => None,
    };
    write_json(&config, &file)?;
    if let Some((path, settings)) = approved {
        write_json(&path, &settings)?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atelier-mcp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_command_lines_of_the_known_ais_are_recognized() {
        assert_eq!(detect("claude"), Some(Agent::Claude));
        assert_eq!(detect("  claude --resume  "), Some(Agent::Claude));
        assert_eq!(
            detect(r"C:\Users\moi\AppData\Roaming\npm\claude.cmd -p hi"),
            Some(Agent::Claude)
        );
        assert_eq!(detect("gemini"), Some(Agent::Gemini));
        assert_eq!(detect("codex exec fix"), Some(Agent::Codex));
        assert_eq!(detect("claudette"), None);
        assert_eq!(detect("echo claude"), None);
        assert_eq!(detect(""), None);
    }

    #[test]
    fn claude_gets_the_server_in_mcp_json_and_keeps_what_was_there() {
        let root = scratch("claude");
        fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"autre":{"command":"x"}}}"#,
        )
        .unwrap();
        assert!(!is_connected(Agent::Claude, &root, None));
        let written = connect(Agent::Claude, &root, None).unwrap();
        assert_eq!(written.len(), 2);
        assert!(is_connected(Agent::Claude, &root, None));
        let file: Value =
            serde_json::from_str(&fs::read_to_string(root.join(".mcp.json")).unwrap()).unwrap();
        assert_eq!(file["mcpServers"]["autre"]["command"], "x");
        assert_eq!(
            file["mcpServers"][SERVER]["url"], "${ATELIER_MCP_URL}",
            "no address or secret in the file"
        );
        assert_eq!(
            file["mcpServers"][SERVER]["headers"]["Authorization"],
            "Bearer ${ATELIER_MCP_TOKEN}"
        );
        let settings: Value = serde_json::from_str(
            &fs::read_to_string(root.join(".claude/settings.local.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["enabledMcpjsonServers"], json!([SERVER]));
        // Twice changes nothing more.
        connect(Agent::Claude, &root, None).unwrap();
        let settings: Value = serde_json::from_str(
            &fs::read_to_string(root.join(".claude/settings.local.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["enabledMcpjsonServers"], json!([SERVER]));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_gets_a_toml_entry_with_the_current_address_and_keeps_the_rest() {
        let root = scratch("codex");
        fs::create_dir_all(root.join(".codex")).unwrap();
        fs::write(
            root.join(".codex/config.toml"),
            "# mon réglage\nmodel = \"o3\"\n\n[mcp_servers.autre]\ncommand = \"npx\"\n",
        )
        .unwrap();
        let first = "http://127.0.0.1:4000/mcp";
        assert!(!is_connected(Agent::Codex, &root, Some(first)));
        connect(Agent::Codex, &root, Some(first)).unwrap();
        assert!(is_connected(Agent::Codex, &root, Some(first)));
        let text = fs::read_to_string(root.join(".codex/config.toml")).unwrap();
        assert!(
            text.contains("# mon réglage")
                && text.contains("model = \"o3\"")
                && text.contains("command = \"npx\""),
            "{text}"
        );
        assert!(
            text.contains("bearer_token_env_var = \"ATELIER_MCP_TOKEN\""),
            "the secret is not written: {text}"
        );
        // The port changed: the entry is stale until it is written again.
        let second = "http://127.0.0.1:5000/mcp";
        assert!(!is_connected(Agent::Codex, &root, Some(second)));
        connect(Agent::Codex, &root, Some(second)).unwrap();
        assert!(is_connected(Agent::Codex, &root, Some(second)));
        assert_eq!(
            fs::read_to_string(root.join(".codex/config.toml"))
                .unwrap()
                .matches("[mcp_servers.atelier]")
                .count(),
            1
        );
        assert!(
            connect(Agent::Codex, &root, None).is_err(),
            "no server, no address"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gemini_gets_its_own_settings_file_and_a_broken_file_is_left_alone() {
        let root = scratch("gemini");
        connect(Agent::Gemini, &root, None).unwrap();
        let file: Value =
            serde_json::from_str(&fs::read_to_string(root.join(".gemini/settings.json")).unwrap())
                .unwrap();
        assert_eq!(file["mcpServers"][SERVER]["httpUrl"], "${ATELIER_MCP_URL}");
        let broken = scratch("broken");
        fs::write(broken.join(".mcp.json"), "{ pas du json").unwrap();
        assert!(connect(Agent::Claude, &broken, None).is_err());
        assert_eq!(
            fs::read_to_string(broken.join(".mcp.json")).unwrap(),
            "{ pas du json",
            "untouched"
        );
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&broken);
    }
}
