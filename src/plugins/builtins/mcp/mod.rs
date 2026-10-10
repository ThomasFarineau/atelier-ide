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
mod rework;
mod server;
mod sessions;
mod tools;
mod usage;

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
    title: crate::i18n::Text("mcp.ai_sessions"),
    icon: "mcp/icon",
    dock: Dock::Bottom,
    shortcut: "Alt+7",
}];

/// The address and secret of the running server, for the terminals started afterwards.
static CONNECTION: Mutex<Option<(String, String)>> = Mutex::new(None);

/// The address of the running server.
pub fn address() -> Option<String> {
    CONNECTION
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|(url, _)| url.clone())
}

/// The variables a terminal of the IDE defines so that an AI can find the server.
pub fn env() -> Vec<(&'static str, String)> {
    match &*CONNECTION.lock().unwrap_or_else(|e| e.into_inner()) {
        Some((url, token)) => vec![
            (agents::URL_VAR, url.clone()),
            (agents::TOKEN_VAR, token.clone()),
        ],
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
    let mut saved: Saved = fs::read(saved_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
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
    /// The tab of the window: the sessions, or the AIs installed.
    tab: Tab,
    usage: usage::Scanner,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Tab {
    #[default]
    Sessions,
    Usage,
}

/// The usage is read again after this long while its tab is shown.
const USAGE_REFRESH_MS: u64 = 60_000;

/// What the user asked of a session in the window.
enum Action {
    Resume(agents::Agent, String),
    Copy(String),
    Delete(String),
}

/// What an AI is told to take a session up: it reads the context the IDE wrote, which holds the
/// notes of the session (no quote or line break: the same line works in every shell).
fn resume_prompt(id: &str, file: &str) -> String {
    trf!("mcp.take_up_atelier_session_read", id = id, file = file)
}

/// The command line that installs `agent`.
fn install_command(agent: agents::Agent) -> &'static str {
    match agent {
        agents::Agent::Claude => "npm install -g @anthropic-ai/claude-code",
        agents::Agent::Codex => "npm install -g @openai/codex",
        agents::Agent::Gemini => "npm install -g @google/gemini-cli",
    }
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
        match server::start(
            saved.port,
            saved.token.clone(),
            Arc::new(move || ctx.request_repaint()),
        ) {
            Ok(server) => {
                // The port is kept for the next run unless another window of the IDE has it.
                if saved.port == 0 || saved.port == server.port {
                    saved.port = server.port;
                    save(&saved);
                }
                *CONNECTION.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((format!("http://127.0.0.1:{}/mcp", server.port), saved.token));
                self.server = Some(server);
            }
            Err(e) => self.error = Some(trf!("mcp.mcp_server_not_available", e = e)),
        }
    }

    fn address(&self) -> Option<String> {
        self.server
            .as_ref()
            .map(|s| format!("http://127.0.0.1:{}/mcp", s.port))
    }

    /// Starts `agent` in a terminal of its own, on the context of a session: the IDE keeps the
    /// context, so any AI can take it up, not only the one that began it.
    fn resume(&mut self, agent: agents::Agent, id: &str, host: &mut dyn Host) {
        let Some(server) = &self.server else { return };
        let exported = server
            .store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .export(id);
        let file = match exported {
            Ok(file) => file,
            Err(e) => return host.notify(trf!("mcp.error", e = e)),
        };
        // The AI has to find the server to leave notes in the session it starts.
        if let Err(e) = agents::connect(agent, host.root(), self.address().as_deref()) {
            return host.notify(trf!("mcp.error", e = e));
        }
        host.run_in_new_terminal(resume_command(agent, &resume_prompt(id, &file)));
        host.notify(trf!("mcp.session_taken_up", agent = agent.name()));
    }
}

impl McpPlugin {
    fn sessions_tab(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        let Some(server) = &self.server else {
            ui.label(
                RichText::new(
                    self.error
                        .clone()
                        .unwrap_or_else(|| tr!("mcp.mcp_server_starting").into()),
                )
                .color(t.text_secondary),
            );
            return;
        };
        let sessions = server
            .store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sessions
            .clone();
        if sessions.is_empty() {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(tr!("mcp.no_ai_sessions_project")).color(t.text_secondary));
                ui.label(
                    RichText::new(tr!("mcp.start_claude_codex_gemini_terminal"))
                        .small()
                        .color(t.text_secondary),
                );
            });
            return;
        }
        // Debug builds: a screenshot can ask for a session to be open.
        if cfg!(debug_assertions)
            && self.selected.is_none()
            && std::env::var_os("ATELIER_SHOT_SESSION_OPEN").is_some()
        {
            self.selected = sessions.last().map(|s| s.id.clone());
        }
        let at = sessions::now();
        let mut action: Option<Action> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for session in &sessions {
                    let open = self.selected.as_deref() == Some(session.id.as_str());
                    let (rect, response) = ui::row(ui, open);
                    let live = session.active(at);
                    ui.painter().circle_filled(
                        egui::pos2(rect.left() + 14.0, rect.center().y),
                        4.0,
                        if live {
                            Color32::from_rgb(0x5F, 0xB8, 0x65)
                        } else {
                            t.text_secondary
                        },
                    );
                    let end = ui::paint_text(
                        ui,
                        rect,
                        rect.left() + 28.0,
                        &session.display_title(),
                        t.text,
                        ui::semibold(12.5),
                    );
                    let hint = trf!(
                        "mcp.call_s",
                        client = session.client,
                        started = sessions::clock(session.started),
                        count = session.events.len(),
                        plural = if live { tr!("mcp.running") } else { "" }
                    );
                    ui::paint_text(
                        ui,
                        rect,
                        end + 12.0,
                        &hint,
                        t.text_secondary,
                        FontId::proportional(12.0),
                    );
                    if response.clicked() {
                        self.selected = if open { None } else { Some(session.id.clone()) };
                    }
                    if !open {
                        continue;
                    }
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: 28,
                            right: 12,
                            top: 6,
                            bottom: 10,
                        })
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            if session.notes.is_empty() {
                                ui.label(
                                    RichText::new(tr!("mcp.no_notes_ai_left_no"))
                                        .small()
                                        .color(t.text_secondary),
                                );
                            }
                            for note in &session.notes {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new(match note.kind.as_str() {
                                            "goal" => tr!("mcp.goal"),
                                            "decision" => tr!("mcp.decision"),
                                            "todo" => tr!("mcp.do"),
                                            "progress" => tr!("mcp.progress"),
                                            _ => tr!("mcp.summary"),
                                        })
                                        .font(ui::semibold(12.0))
                                        .color(t.accent),
                                    );
                                    ui.label(RichText::new(&note.text).size(12.5));
                                });
                            }
                            for event in session.events.iter().rev().take(6) {
                                ui.label(
                                    RichText::new(format!(
                                        "{} {} {}",
                                        sessions::clock(event.at),
                                        event.tool,
                                        event.summary
                                    ))
                                    .small()
                                    .color(if event.ok {
                                        t.text_secondary
                                    } else {
                                        t.error
                                    }),
                                );
                            }
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(tr!("mcp.take_up")).color(t.text_secondary));
                                for agent in [
                                    agents::Agent::Claude,
                                    agents::Agent::Codex,
                                    agents::Agent::Gemini,
                                ] {
                                    if ui::button(ui, agent.name()).clicked() {
                                        action = Some(Action::Resume(agent, session.id.clone()));
                                    }
                                }
                                ui.separator();
                                if ui::button(ui, tr!("mcp.copy_context")).clicked() {
                                    action = Some(Action::Copy(session.id.clone()));
                                }
                                if ui::button(ui, tr!("mcp.delete")).clicked() {
                                    action = Some(Action::Delete(session.id.clone()));
                                }
                            });
                        });
                }
            });
        match action {
            Some(Action::Resume(agent, id)) => self.resume(agent, &id, host),
            Some(Action::Copy(id)) => {
                let text = self.server.as_ref().and_then(|s| {
                    s.store
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .markdown(&id)
                });
                if let Some(text) = text {
                    ui.ctx().copy_text(text);
                    host.notify(tr!("mcp.session_context_copied").into());
                }
            }
            Some(Action::Delete(id)) => {
                if let Some(server) = &self.server {
                    server
                        .store
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .delete(&id);
                }
                self.selected = None;
            }
            None => {}
        }
    }

    /// Reads the usage of the AIs again, in the background.
    fn refresh_usage(&mut self, ctx: &egui::Context, host: &dyn Host) {
        let root = host.root().to_path_buf();
        let address = self.address();
        let ctx = ctx.clone();
        self.usage.refresh(
            &root,
            |agent| agents::is_connected(agent, &root, address.as_deref()),
            move || ctx.request_repaint(),
        );
    }

    /// The calls `agent` made to the IDE, from the sessions of the MCP server: (sessions, calls).
    fn calls_of(&self, agent: agents::Agent) -> (usize, usize) {
        let Some(server) = &self.server else {
            return (0, 0);
        };
        let store = server.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .sessions
            .iter()
            .filter(|s| s.client.to_lowercase().contains(agent.id()))
            .fold((0, 0), |(n, calls), s| (n + 1, calls + s.events.len()))
    }

    /// The AIs installed, whether the project is connected to the IDE for them, and what they
    /// used.
    fn usage_tab(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        self.usage.poll();
        let stale = self
            .usage
            .report
            .as_ref()
            .is_none_or(|(read, _)| sessions::now().saturating_sub(*read) > USAGE_REFRESH_MS);
        if stale && !self.usage.busy() {
            self.refresh_usage(ui.ctx(), host);
        }
        let Some((_, report)) = &self.usage.report else {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(tr!("mcp.reading_ai_usage")).color(t.text_secondary));
            });
            return;
        };
        let report = report.clone();
        let mut action: Option<(agents::Agent, bool)> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (agent, usage) in &report {
                    if let Some(wanted) = self.agent_card(ui, *agent, usage) {
                        action = Some((*agent, wanted));
                    }
                    ui.separator();
                }
                ui.label(
                    RichText::new(tr!("mcp.usage_local_only"))
                        .small()
                        .color(t.text_secondary),
                );
            });
        match action {
            Some((agent, true)) => {
                self.run_command(&format!("mcp.{}", agent.id()), host);
                self.refresh_usage(ui.ctx(), host);
            }
            Some((agent, false)) => host.run_in_new_terminal(install_command(agent).to_string()),
            None => {}
        }
    }

    /// One AI of the usage tab. `Some(true)` when the user asks to connect it to the IDE,
    /// `Some(false)` to install it.
    fn agent_card(
        &self,
        ui: &mut egui::Ui,
        agent: agents::Agent,
        usage: &usage::AgentUsage,
    ) -> Option<bool> {
        let t = ui::theme(ui.ctx());
        let secondary = |text: String| RichText::new(text).size(12.0).color(t.text_secondary);
        let installed = usage.program.is_some();
        let mut action = None;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            ui.painter().circle_filled(
                dot.center(),
                4.0,
                if installed {
                    Color32::from_rgb(0x5F, 0xB8, 0x65)
                } else {
                    t.text_disabled
                },
            );
            ui.label(RichText::new(agent.name()).font(ui::semibold(13.0)));
            match &usage.program {
                Some(program) => {
                    if let Some(version) = &usage.version {
                        ui.label(RichText::new(version).color(t.text_secondary));
                    }
                    ui.label(secondary(program.display().to_string()));
                }
                None => {
                    ui.label(secondary(tr!("mcp.not_installed").into()));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !installed {
                    if ui::button(ui, tr!("mcp.install"))
                        .on_hover_text(install_command(agent))
                        .clicked()
                    {
                        action = Some(false);
                    }
                } else if usage.connected {
                    ui.label(RichText::new(tr!("mcp.connected_to_ide")).color(t.accent));
                } else if ui::button(ui, tr!("mcp.connect")).clicked() {
                    action = Some(true);
                }
            });
        });
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 22,
                right: 12,
                top: 0,
                bottom: 6,
            })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                if usage.answers == 0 {
                    ui.label(secondary(tr!("mcp.no_usage_30_days").into()));
                } else {
                    let tokens = |label: &str, tokens: usage::Tokens| {
                        format!("{label} {}", usage::short_count(tokens.total()))
                    };
                    let windows = trf!(
                        "mcp.tokens_windows",
                        day = tokens(tr!("mcp.last_24_h"), usage.day),
                        week = tokens(tr!("mcp.last_7_days"), usage.week),
                        month = tokens(tr!("mcp.last_30_days"), usage.month)
                    );
                    ui.label(RichText::new(windows).size(12.5))
                        .on_hover_text(trf!(
                            "mcp.tokens_detail",
                            input = usage::short_count(usage.month.input),
                            cached = usage::short_count(usage.month.cached),
                            output = usage::short_count(usage.month.output)
                        ));
                    ui.label(secondary(trf!(
                        "mcp.project_usage",
                        tokens = usage::short_count(usage.project.total()),
                        sessions = usage.sessions,
                        answers = usage.answers
                    )));
                    let total: u64 = usage.models.iter().map(|(_, n)| n).sum();
                    let models: Vec<String> = usage
                        .models
                        .iter()
                        .take(4)
                        .map(|(model, n)| {
                            let share = (n * 100).checked_div(total).unwrap_or(0);
                            format!("{model} {share} %")
                        })
                        .collect();
                    ui.label(secondary(trf!(
                        "mcp.models_used",
                        models = models.join(" · ")
                    )));
                    if let Some(last) = usage.last {
                        ui.label(secondary(trf!("mcp.last_used", at = sessions::clock(last))));
                    }
                }
                if let Some((read, limits, plan)) = &usage.limits {
                    for limit in limits {
                        limit_row(ui, limit);
                    }
                    ui.label(secondary(trf!(
                        "mcp.limits_read",
                        plan = plan.as_deref().unwrap_or("?"),
                        at = sessions::clock(*read)
                    )));
                } else if usage.answers > 0 {
                    ui.label(secondary(tr!("mcp.limits_not_kept").into()));
                }
                let (count, calls) = self.calls_of(agent);
                if count > 0 {
                    ui.label(secondary(trf!(
                        "mcp.through_atelier",
                        sessions = count,
                        calls = calls
                    )));
                }
            });
        action
    }
}

/// A usage limit: its window, the share used and when it starts again.
fn limit_row(ui: &mut egui::Ui, limit: &usage::Limit) {
    let t = ui::theme(ui.ctx());
    ui.horizontal(|ui| {
        let window = match limit.window_minutes {
            10080 => tr!("mcp.limit_week").to_string(),
            m if m % 60 == 0 => trf!("mcp.limit_hours", hours = m / 60),
            m => trf!("mcp.limit_minutes", minutes = m),
        };
        ui.label(RichText::new(window).size(12.0));
        let used = (limit.used_percent / 100.0).clamp(0.0, 1.0) as f32;
        ui.add(
            egui::ProgressBar::new(used)
                .desired_width(140.0)
                .desired_height(8.0)
                .fill(if used >= 0.9 { t.error } else { t.accent }),
        );
        ui.label(RichText::new(format!("{:.0} %", limit.used_percent)).size(12.0));
        if let Some(reset) = limit.resets_at {
            ui.label(
                RichText::new(trf!("mcp.limit_resets", at = sessions::clock(reset * 1000)))
                    .size(12.0)
                    .color(t.text_secondary),
            );
        }
    });
}

impl BuiltinPlugin for McpPlugin {
    fn id(&self) -> &'static str {
        "mcp"
    }
    fn name(&self) -> &'static str {
        tr!("mcp.atelier_super_intelligence_tools")
    }
    fn description(&self) -> &'static str {
        tr!("mcp.tools_atelier_ais_through_mcp")
    }
    fn icon(&self) -> &'static str {
        "mcp/icon"
    }

    fn update(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        if self.server.is_none() && self.error.is_none() {
            self.start(ctx);
        }
        self.usage.poll();
        let Some(server) = &self.server else { return };
        *server.root.lock().unwrap_or_else(|e| e.into_inner()) = host.root().to_path_buf();
        server
            .store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_root(host.root());
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
        // Debug builds: a screenshot can ask for the usage tab.
        if cfg!(debug_assertions) && std::env::var_os("ATELIER_SHOT_AI_USAGE").is_some() {
            self.tab = Tab::Usage;
        }
        ui.horizontal(|ui| {
            for (tab, label) in [
                (Tab::Sessions, tr!("mcp.sessions_tab")),
                (Tab::Usage, tr!("mcp.installed_ais")),
            ] {
                if ui.selectable_label(self.tab == tab, label).clicked() {
                    self.tab = tab;
                }
            }
            if self.tab == Tab::Usage {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let tooltip = tr!("mcp.refresh");
                    if ui::icon_button_enabled(ui, "refresh", !self.usage.busy(), tooltip).clicked()
                    {
                        self.refresh_usage(ui.ctx(), host);
                    }
                    if self.usage.busy() {
                        ui.spinner();
                    }
                });
            }
        });
        ui.separator();
        match self.tab {
            Tab::Sessions => self.sessions_tab(ui, host),
            Tab::Usage => self.usage_tab(ui, host),
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, _host: &mut dyn Host) {
        if let Some(address) = self.address() {
            ui::status_text(ui, "MCP").on_hover_text(trf!(
                "mcp.atelier_mcp_server_tool_call",
                address = address,
                served = self.served
            ));
        }
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("mcp.claude", tr!("mcp.mcp_connect_claude_code_project")),
            ("mcp.gemini", tr!("mcp.mcp_connect_gemini_cli_project")),
            ("mcp.codex", tr!("mcp.mcp_connect_codex_project")),
            ("mcp.status", tr!("mcp.mcp_show_server_address")),
        ]
        .into_iter()
        .map(|(id, label)| Command {
            id,
            label: label.into(),
        })
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
                    (None, Some(address)) => trf!(
                        "mcp.terminals_atelier_set",
                        address = address,
                        value = agents::URL_VAR,
                        value2 = agents::TOKEN_VAR
                    ),
                    (None, None) => tr!("mcp.mcp_server_starting").into(),
                };
                host.notify(message);
                return;
            }
        };
        match agents::connect(agent, host.root(), self.address().as_deref()) {
            Ok(files) => {
                let names: Vec<String> = files
                    .iter()
                    .filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .collect();
                host.notify(trf!(
                    "mcp.connected_atelier_mcp_restart_if",
                    agent = agent.name(),
                    names = names.join(", ")
                ));
            }
            Err(e) => host.notify(trf!("mcp.error", e = e)),
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/mcp/mod/tests.rs"]
mod tests;
