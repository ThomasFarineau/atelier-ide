#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod atelier;
mod cli;
mod dev;
mod editor;
mod features;
mod find;
mod graph;
mod highlight;
mod icon_set;
mod icons;
mod languages;
mod plugins;
mod project;
mod search;
mod stats;
mod tasks;
mod terminal;
mod terminal_layout;
mod terminal_ui;
mod ui;
mod wsl;

fn main() -> eframe::Result {
    if std::env::args_os().any(|a| a == dev::FLAG) {
        if let Err(e) = dev::supervise() {
            eprintln!("[dev] {e:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    // `atelier <folder|file>`: an IDE that is open takes it.
    if !cli::new_window()
        && std::env::var_os(dev::CHILD_ENV).is_none()
        && let Some(path) = cli::target()
        && cli::forward(&path)
    {
        return Ok(());
    }
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/logo/atelier.png")).ok();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Atelier IDE")
        .with_app_id("atelier-ide")
        .with_inner_size([1440.0, 920.0])
        .with_min_inner_size([900.0, 600.0])
        // The app draws its own title bar (see `Ide::toolbar`).
        .with_decorations(false);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Atelier IDE",
        options,
        Box::new(|cc| Ok(Box::new(app::Ide::new(cc)))),
    )
}
