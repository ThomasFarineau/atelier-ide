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
    let program = line.split_whitespace().next()?;
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
            trf!(
                "mcp.not_valid_json_nothing_was",
                path = path.display(),
                e = e
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
        .ok_or(tr!("mcp.configuration_file_not_json_object"))?
        .entry(key)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| trf!("mcp.not_json_object", key = key))
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
        trf!(
            "mcp.not_valid_toml_nothing_was",
            config = config.display(),
            e = e
        )
    })?;
    let servers = doc.entry("mcp_servers").or_insert_with(|| {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        toml_edit::Item::Table(table)
    });
    let servers = servers
        .as_table_mut()
        .ok_or(tr!("mcp.mcp_servers_not_toml_table"))?;
    let entry = servers
        .entry(SERVER)
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or(tr!("mcp.mcp_servers_atelier_not_toml"))?;
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
        let url = url.ok_or(tr!("mcp.mcp_server_not_started"))?;
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
                .ok_or(tr!("mcp.settings_local_json_not_json"))?
                .entry("enabledMcpjsonServers")
                .or_insert_with(|| json!([]));
            let list = list
                .as_array_mut()
                .ok_or(tr!("mcp.enabledmcpjsonservers_not_list"))?;
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
#[path = "../../../../test/plugins/builtins/mcp/agents/tests.rs"]
mod tests;
