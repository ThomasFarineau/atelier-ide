//! MCP built-in plugin: the IDE is a Model Context Protocol server, so that an AI started in one
//! of its terminals can use its tools (open files, problems, project search…).
//!
//! The server listens on the loopback interface behind a secret that is kept between runs
//! (`mcp.json` in the data folder). The terminals of the IDE define `ATELIER_MCP_URL` and
//! `ATELIER_MCP_TOKEN`; the configuration files [`agents`] writes refer to those, so they never
//! hold the secret. When the user starts a known AI in a terminal, the IDE offers to connect it
//! (see `Ide::offer_mcp`).

pub mod agents;
mod commits;
mod server;
mod sessions;
mod tools;

pub use sessions::clock;

use crate::{
    plugins::builtin::{BuiltinPlugin, Command, Dock, Host, ToolWindow},
    ui,
};
use eframe::egui::{self, Color32, FontId, RichText};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    hash::{BuildHasher, Hasher},
    sync::{Arc, Mutex},
};

pub static TOOL_WINDOWS: [ToolWindow; 1] = [ToolWindow {
    id: "mcp.sessions",
    title: "Sessions IA",
    icon: "mcp/icon",
    dock: Dock::Bottom,
    shortcut: "Alt+7",
}];

/// The address and secret of the running server, for the terminals started afterwards.
static CONNECTION: Mutex<Option<(String, String)>> = Mutex::new(None);

/// The address of the running server.
pub fn address() -> Option<String> {
    CONNECTION.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|(url, _)| url.clone())
}

/// The variables a terminal of the IDE defines so that an AI can find the server.
pub fn env() -> Vec<(&'static str, String)> {
    match &*CONNECTION.lock().unwrap_or_else(|e| e.into_inner()) {
        Some((url, token)) => vec![(agents::URL_VAR, url.clone()), (agents::TOKEN_VAR, token.clone())],
        None => Vec::new(),
    }
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    token: String,
    port: u16,
}

fn saved_path() -> std::path::PathBuf {
    crate::cli::data_dir().join("mcp.json")
}

/// The secret and port of the previous run, or new ones.
fn load() -> Saved {
    let mut saved: Saved = fs::read(saved_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if saved.token.len() < 32 {
        let random = || std::hash::RandomState::new().build_hasher().finish();
        saved.token = format!("{:016x}{:016x}{:016x}", random(), random(), random());
    }
    saved
}

fn save(saved: &Saved) {
    let _ = fs::create_dir_all(crate::cli::data_dir());
    if let Ok(json) = serde_json::to_vec(saved) {
        let _ = fs::write(saved_path(), json);
    }
}

#[derive(Default)]
pub struct McpPlugin {
    server: Option<server::Server>,
    error: Option<String>,
    /// Tool calls answered since the start, for the status bar tooltip.
    served: usize,
    /// The session open in the Sessions window.
    selected: Option<String>,
}

/// What the user asked of a session in the window.
enum Action {
    Resume(agents::Agent, String),
    Copy(String),
    Delete(String),
}

/// What an AI is told to take a session up: it reads the context the IDE wrote, which holds the
/// notes of the session (no quote or line break: the same line works in every shell).
fn resume_prompt(id: &str, file: &str) -> String {
    format!("Reprends la session Atelier {id} : lis {file} (ou appelle get_session_context) puis continue le travail.")
}

/// The command line that starts `agent` with that first request.
fn resume_command(agent: agents::Agent, prompt: &str) -> String {
    match agent {
        agents::Agent::Claude => format!("claude \"{prompt}\""),
        agents::Agent::Codex => format!("codex \"{prompt}\""),
        agents::Agent::Gemini => format!("gemini -i \"{prompt}\""),
    }
}

impl McpPlugin {
    fn start(&mut self, ctx: &egui::Context) {
        let mut saved = load();
        let ctx = ctx.clone();
        match server::start(saved.port, saved.token.clone(), Arc::new(move || ctx.request_repaint())) {
            Ok(server) => {
                // The port is kept for the next run unless another window of the IDE has it.
                if saved.port == 0 || saved.port == server.port {
                    saved.port = server.port;
                    save(&saved);
                }
                *CONNECTION.lock().unwrap_or_else(|e| e.into_inner()) = Some((format!("http://127.0.0.1:{}/mcp", server.port), saved.token));
                self.server = Some(server);
            }
            Err(e) => self.error = Some(format!("Serveur MCP indisponible : {e}")),
        }
    }

    fn address(&self) -> Option<String> {
        self.server.as_ref().map(|s| format!("http://127.0.0.1:{}/mcp", s.port))
    }

    /// Starts `agent` in a terminal of its own, on the context of a session: the IDE keeps the
    /// context, so any AI can take it up, not only the one that began it.
    fn resume(&mut self, agent: agents::Agent, id: &str, host: &mut dyn Host) {
        let Some(server) = &self.server else { return };
        let exported = server.store.lock().unwrap_or_else(|e| e.into_inner()).export(id);
        let file = match exported {
            Ok(file) => file,
            Err(e) => return host.notify(format!("Erreur : {e}")),
        };
        // The AI has to find the server to leave notes in the session it starts.
        if let Err(e) = agents::connect(agent, host.root(), self.address().as_deref()) {
            return host.notify(format!("Erreur : {e}"));
        }
        host.run_in_new_terminal(resume_command(agent, &resume_prompt(id, &file)));
        host.notify(format!("Session reprise avec {}", agent.name()));
    }
}

impl BuiltinPlugin for McpPlugin {
    fn id(&self) -> &'static str {
        "mcp"
    }
    fn name(&self) -> &'static str {
        "Atelier Super Intelligence Tools"
    }
    fn description(&self) -> &'static str {
        "Les outils d'Atelier pour les IA, par un serveur MCP : une IA lancée dans un terminal d'Atelier peut s'y connecter pour utiliser ses outils (fichiers ouverts, éditeur actif, problèmes, recherche dans le projet, ouverture d'un fichier). L'IDE propose la connexion quand on lance Claude Code, Gemini CLI ou Codex."
    }
    fn icon(&self) -> &'static str {
        "mcp/icon"
    }

    fn update(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        if self.server.is_none() && self.error.is_none() {
            self.start(ctx);
        }
        let Some(server) = &self.server else { return };
        *server.root.lock().unwrap_or_else(|e| e.into_inner()) = host.root().to_path_buf();
        server.store.lock().unwrap_or_else(|e| e.into_inner()).set_root(host.root());
        for call in server.calls.try_iter() {
            let outcome = tools::run(host, &call.tool, &call.args);
            let _ = call.reply.send(outcome);
            self.served += 1;
        }
    }

    fn tool_windows(&self) -> &'static [ToolWindow] {
        &TOOL_WINDOWS
    }

    fn shortcuts(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::Num7)) {
            host.show_tool_window(&TOOL_WINDOWS[0]);
        }
    }

    fn tool_window(&mut self, id: &str, ui: &mut egui::Ui, host: &mut dyn Host) {
        if id != TOOL_WINDOWS[0].id {
            return;
        }
        let t = ui::theme(ui.ctx());
        let Some(server) = &self.server else {
            ui.label(RichText::new(self.error.clone().unwrap_or_else(|| "Serveur MCP en cours de démarrage".into())).color(t.text_secondary));
            return;
        };
        let sessions = server.store.lock().unwrap_or_else(|e| e.into_inner()).sessions.clone();
        if sessions.is_empty() {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Aucune session d'IA dans ce projet").color(t.text_secondary));
                ui.label(RichText::new("Lancez claude, codex ou gemini dans un terminal d'Atelier : chaque connexion devient une session.").small().color(t.text_secondary));
            });
            return;
        }
        // Debug builds: a screenshot can ask for a session to be open.
        if cfg!(debug_assertions) && self.selected.is_none() && std::env::var_os("ATELIER_SHOT_SESSION_OPEN").is_some() {
            self.selected = sessions.last().map(|s| s.id.clone());
        }
        let at = sessions::now();
        let mut action: Option<Action> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for session in &sessions {
                let open = self.selected.as_deref() == Some(session.id.as_str());
                let (rect, response) = ui::row(ui, open);
                let live = session.active(at);
                ui.painter().circle_filled(egui::pos2(rect.left() + 14.0, rect.center().y), 4.0, if live { Color32::from_rgb(0x5F, 0xB8, 0x65) } else { t.text_secondary });
                let end = ui::paint_text(ui, rect, rect.left() + 28.0, &session.display_title(), t.text, ui::semibold(12.5));
                let hint = format!("{} · {} · {} appel(s){}", session.client, sessions::clock(session.started), session.events.len(), if live { " · en cours" } else { "" });
                ui::paint_text(ui, rect, end + 12.0, &hint, t.text_secondary, FontId::proportional(12.0));
                if response.clicked() {
                    self.selected = if open { None } else { Some(session.id.clone()) };
                }
                if !open {
                    continue;
                }
                egui::Frame::new().inner_margin(egui::Margin { left: 28, right: 12, top: 6, bottom: 10 }).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    if session.notes.is_empty() {
                        ui.label(RichText::new("Pas de note : l'IA n'a pas laissé de contexte (session_note). La reprise ne s'appuiera que sur ses appels.").small().color(t.text_secondary));
                    }
                    for note in &session.notes {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(match note.kind.as_str() {
                                "goal" => "Objectif",
                                "decision" => "Décision",
                                "todo" => "À faire",
                                "progress" => "Avancement",
                                _ => "Résumé",
                            }).font(ui::semibold(12.0)).color(t.accent));
                            ui.label(RichText::new(&note.text).size(12.5));
                        });
                    }
                    for event in session.events.iter().rev().take(6) {
                        ui.label(RichText::new(format!("{} {} {}", sessions::clock(event.at), event.tool, event.summary)).small().color(if event.ok { t.text_secondary } else { t.error }));
                    }
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Reprendre avec").color(t.text_secondary));
                        for agent in [agents::Agent::Claude, agents::Agent::Codex, agents::Agent::Gemini] {
                            if ui::button(ui, agent.name()).clicked() {
                                action = Some(Action::Resume(agent, session.id.clone()));
                            }
                        }
                        ui.separator();
                        if ui::button(ui, "Copier le contexte").clicked() {
                            action = Some(Action::Copy(session.id.clone()));
                        }
                        if ui::button(ui, "Supprimer").clicked() {
                            action = Some(Action::Delete(session.id.clone()));
                        }
                    });
                });
            }
        });
        match action {
            Some(Action::Resume(agent, id)) => self.resume(agent, &id, host),
            Some(Action::Copy(id)) => {
                let text = self.server.as_ref().and_then(|s| s.store.lock().unwrap_or_else(|e| e.into_inner()).markdown(&id));
                if let Some(text) = text {
                    ui.ctx().copy_text(text);
                    host.notify("Contexte de la session copié".into());
                }
            }
            Some(Action::Delete(id)) => {
                if let Some(server) = &self.server {
                    server.store.lock().unwrap_or_else(|e| e.into_inner()).delete(&id);
                }
                self.selected = None;
            }
            None => {}
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, _host: &mut dyn Host) {
        if let Some(address) = self.address() {
            ui::status_text(ui, "MCP").on_hover_text(format!("Serveur MCP d'Atelier : {address}\n{} appel(s) d'outil depuis le démarrage", self.served));
        }
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("mcp.claude", "MCP : connecter Claude Code à ce projet"),
            ("mcp.gemini", "MCP : connecter Gemini CLI à ce projet"),
            ("mcp.codex", "MCP : connecter Codex à ce projet"),
            ("mcp.status", "MCP : afficher l'adresse du serveur"),
        ]
        .into_iter()
        .map(|(id, label)| Command { id, label: label.into() })
        .collect()
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        let agent = match id {
            "mcp.claude" => agents::Agent::Claude,
            "mcp.gemini" => agents::Agent::Gemini,
            "mcp.codex" => agents::Agent::Codex,
            _ => {
                let message = match (&self.error, self.address()) {
                    (Some(error), _) => error.clone(),
                    (None, Some(address)) => format!("{address} · les terminaux d'Atelier définissent {} et {}", agents::URL_VAR, agents::TOKEN_VAR),
                    (None, None) => "Serveur MCP en cours de démarrage".into(),
                };
                host.notify(message);
                return;
            }
        };
        match agents::connect(agent, host.root(), self.address().as_deref()) {
            Ok(files) => {
                let names: Vec<String> = files.iter().filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
                host.notify(format!("{} connecté au MCP d'Atelier ({}) · relancez-le s'il tourne déjà", agent.name(), names.join(", ")));
            }
            Err(e) => host.notify(format!("Erreur : {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_ai_is_started_on_the_same_context_with_one_line_every_shell_accepts() {
        let prompt = resume_prompt("0123abcd", ".atelier/sessions/0123abcd.md");
        assert!(!prompt.contains(['"', '\n', '\'', '`', '$']), "{prompt}");
        assert!(prompt.contains(".atelier/sessions/0123abcd.md"));
        for (agent, start) in [
            (agents::Agent::Claude, "claude \""),
            (agents::Agent::Codex, "codex \""),
            (agents::Agent::Gemini, "gemini -i \""),
        ] {
            let command = resume_command(agent, &prompt);
            assert!(command.starts_with(start) && command.ends_with('"') && !command.contains('\n'), "{command}");
            assert_eq!(agents::detect(&command), Some(agent), "the IDE would recognize it as that AI");
        }
    }
}
