//! GitHub, a sub-plugin of Git: the **Pull Request** window shows the pull request of the current
//! branch with its reviews and the conversations of its reviews, read with the GitHub CLI (`gh`);
//! a conversation opens its file at its line. It also brings the schemas of the workflows of
//! GitHub Actions and of the configuration of Dependabot.

pub mod pull;

use crate::i18n::Text;
use crate::languages::json::Schema;
use crate::plugins::builtin::{BuiltinPlugin, Command, Dock, Host, ToolWindow};
use crate::ui;
use eframe::egui::{self, RichText};
use pull::Pull;
use std::{
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
};

pub static TOOL_WINDOWS: [ToolWindow; 1] = [ToolWindow {
    id: "github.pull_request",
    title: Text("github.pull_request"),
    icon: "git/github/icon",
    dock: Dock::Right,
    shortcut: "",
}];

pub static SCHEMAS: [Schema; 2] = [
    Schema {
        name: "GitHub Actions workflow",
        files: &[".github/workflows/*.yml", ".github/workflows/*.yaml"],
        urls: &["https://json.schemastore.org/github-workflow.json"],
        text: include_str!("workflow.schema.json"),
    },
    Schema {
        name: "Dependabot",
        files: &[".github/dependabot.yml", ".github/dependabot.yaml"],
        urls: &["https://json.schemastore.org/dependabot-2.0.json"],
        text: include_str!("dependabot.schema.json"),
    },
];

/// Whether the project is on GitHub: a remote of its repository is there.
pub fn on_github(root: &Path) -> bool {
    std::fs::read_to_string(root.join(".git").join("config"))
        .is_ok_and(|config| config.contains("github.com"))
        || root.join(".github").is_dir()
}

type Fetch = JoinHandle<Result<Option<Pull>, String>>;

#[derive(Default)]
pub struct GitHubPlugin {
    root: PathBuf,
    github: bool,
    fetching: Option<Fetch>,
    pull: Option<Result<Option<Pull>, String>>,
}

impl GitHubPlugin {
    fn refresh(&mut self) {
        if self.fetching.is_none() {
            let root = self.root.clone();
            self.fetching = Some(thread::spawn(move || pull::fetch(&root)));
        }
    }

    fn view(&self, ui: &mut egui::Ui, pull: &Pull, open: &mut Option<(String, Option<usize>)>) {
        let t = ui::theme(ui.ctx());
        ui.label(
            RichText::new(format!("#{} {}", pull.number, pull.title)).font(ui::semibold(14.0)),
        );
        let state = match (pull.state.as_str(), pull.draft) {
            ("MERGED", _) => tr!("github.merged"),
            ("CLOSED", _) => tr!("github.closed"),
            (_, true) => tr!("github.draft"),
            _ => tr!("github.open"),
        };
        ui.label(
            RichText::new(format!(
                "{state} · {} → {} · {}",
                pull.head, pull.base, pull.author
            ))
            .small()
            .color(t.text_secondary),
        );
        let (decision, color) = review_state(&pull.decision, &t);
        if !decision.is_empty() {
            ui.label(RichText::new(decision).color(color));
        }
        ui.add_space(8.0);
        let written: Vec<_> = pull
            .reviews
            .iter()
            .filter(|r| !r.body.trim().is_empty() || r.state != "COMMENTED")
            .collect();
        ui::section(ui, &trf!("github.reviews", count = written.len()));
        if written.is_empty() {
            ui.label(RichText::new(tr!("github.no_reviews")).color(t.text_secondary));
        }
        for review in written {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&review.author).strong());
                let (state, color) = review_state(&review.state, &t);
                ui.label(RichText::new(state).color(color));
                ui.label(
                    RichText::new(day(&review.date))
                        .small()
                        .color(t.text_secondary),
                );
            });
            if !review.body.trim().is_empty() {
                ui.add(egui::Label::new(review.body.trim()).wrap());
            }
            ui.add_space(4.0);
        }
        ui.add_space(8.0);
        ui::section(
            ui,
            &trf!("github.conversations", count = pull.threads.len()),
        );
        if pull.threads.is_empty() {
            ui.label(RichText::new(tr!("github.no_comments")).color(t.text_secondary));
        }
        for thread in &pull.threads {
            let place = match thread.line {
                Some(line) => format!("{}:{line}", thread.path),
                None => format!("{} · {}", thread.path, tr!("github.outdated")),
            };
            let link = ui
                .link(RichText::new(place).color(t.link))
                .on_hover_text(tr!("github.open_file"));
            if link.clicked() {
                *open = Some((thread.path.clone(), thread.line));
            }
            for (i, comment) in thread.comments.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    if i > 0 {
                        ui.add_space(14.0);
                    }
                    ui.label(RichText::new(&comment.author).strong());
                    ui.label(
                        RichText::new(day(&comment.date))
                            .small()
                            .color(t.text_secondary),
                    );
                });
                ui.horizontal(|ui| {
                    if i > 0 {
                        ui.add_space(14.0);
                    }
                    ui.add(egui::Label::new(comment.body.trim()).wrap());
                });
            }
            ui.add_space(6.0);
        }
    }
}

/// The day of a date of GitHub (`2026-10-16T09:12:00Z`).
fn day(date: &str) -> &str {
    date.get(..10).unwrap_or(date)
}

/// The words and the color of the state of a review, or of the decision on a pull request.
fn review_state(state: &str, t: &ui::Theme) -> (&'static str, egui::Color32) {
    match state {
        "APPROVED" => (tr!("github.approved"), t.added),
        "CHANGES_REQUESTED" => (tr!("github.changes_requested"), t.deleted),
        "REVIEW_REQUIRED" => (tr!("github.review_required"), t.modified),
        "COMMENTED" => (tr!("github.commented"), t.text_secondary),
        "DISMISSED" => (tr!("github.dismissed"), t.text_disabled),
        "PENDING" => (tr!("github.pending"), t.text_secondary),
        _ => ("", t.text_secondary),
    }
}

impl BuiltinPlugin for GitHubPlugin {
    fn id(&self) -> &'static str {
        "github"
    }
    fn parent(&self) -> Option<&'static str> {
        Some("git")
    }
    fn name(&self) -> &'static str {
        tr!("plugin.github.name")
    }
    fn description(&self) -> &'static str {
        tr!("plugin.github.summary")
    }
    fn icon(&self) -> &'static str {
        crate::plugins::builtin::card_icon("github")
    }
    fn applies(&self) -> bool {
        self.github
    }
    fn tool_windows(&self) -> &'static [ToolWindow] {
        &TOOL_WINDOWS
    }
    fn json_schemas(&self) -> &'static [Schema] {
        &SCHEMAS
    }

    fn update(&mut self, _ctx: &egui::Context, host: &mut dyn Host) {
        if self.root != host.root() {
            self.root = host.root().to_path_buf();
            self.github = on_github(&self.root);
            self.pull = None;
            self.fetching = None;
        }
        if self.fetching.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(fetching) = self.fetching.take()
        {
            self.pull = Some(
                fetching
                    .join()
                    .unwrap_or_else(|_| Err(tr!("github.failed").into())),
            );
        }
    }

    fn commands(&self) -> Vec<Command> {
        vec![Command {
            id: "github.show",
            label: tr!("github.command_show").into(),
        }]
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        if id == "github.show" {
            self.refresh();
            host.show_tool_window(&TOOL_WINDOWS[0]);
        }
    }

    fn tool_window(&mut self, _id: &str, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        if self.pull.is_none() {
            self.refresh();
        }
        let url = match &self.pull {
            Some(Ok(Some(pull))) => Some(pull.url.clone()),
            _ => None,
        };
        ui.horizontal(|ui| {
            if ui::icon_button(ui, "refresh", tr!("github.refresh")).clicked() {
                self.refresh();
            }
            if let Some(url) = &url
                && ui::icon_button(ui, "openNewTab", tr!("github.open_browser")).clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
            if self.fetching.is_some() {
                ui.spinner();
            }
        });
        ui.add_space(4.0);
        let mut open = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match &self.pull {
                None => {}
                Some(Ok(None)) => {
                    ui.label(RichText::new(tr!("github.no_pull_request")).color(t.text_secondary));
                }
                Some(Err(error)) => {
                    ui.add(egui::Label::new(RichText::new(error).color(t.text_secondary)).wrap());
                }
                Some(Ok(Some(pull))) => self.view(ui, pull, &mut open),
            });
        if let Some((path, line)) = open {
            let path = self.root.join(path);
            match line {
                Some(line) => host.open_at(path, line.saturating_sub(1), 0, 0),
                None => host.open_file(path),
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/git/github/tests.rs"]
mod tests;
