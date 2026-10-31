#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// First: its `tr!` and `trf!` macros are used by every module after it.
#[macro_use]
mod i18n;
mod app;
mod atelier;
mod cli;
mod crash;
mod dev;
mod editor;
mod features;
mod find;
mod fonts;
mod graph;
mod highlight;
mod icon_set;
mod icons;
mod languages;
mod plugins;
mod project;
mod search;
mod spelling;
mod stats;
mod tasks;
mod terminal;
mod terminal_layout;
mod terminal_links;
mod terminal_ui;
mod ui;
mod window;
mod wsl;

fn main() -> eframe::Result {
    i18n::set(i18n::system());
    crash::install();
    if std::env::args_os().any(|a| a == dev::FLAG) {
        if let Err(e) = dev::supervise() {
            eprintln!("[dev] {e:#}");
            std::process::exit(1);
        }
        crash::clean_exit();
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
        .with_inner_size(window::DEFAULT_SIZE)
        .with_min_inner_size([900.0, 600.0])
        // The app draws its own title bar (see `Ide::toolbar`).
        .with_decorations(false);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    // eframe puts back the place and size it kept from the last window, even with
    // `persist_window` off: the builder above is used as it is.
    let fresh = viewport.clone();
    let options = eframe::NativeOptions {
        viewport,
        window_builder: Some(Box::new(move |_restored| fresh)),
        // Every window opens centered on the monitor in use (see `window`), never where the
        // last one was closed: closed minimized, that was a tiny window in a corner.
        persist_window: false,
        centered: true,
        ..Default::default()
    };
    let result = eframe::run_native(
        "Atelier IDE",
        options,
        Box::new(|cc| Ok(Box::new(app::Ide::new(cc)))),
    );
    crash::clean_exit();
    result
}
