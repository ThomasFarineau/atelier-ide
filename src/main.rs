#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod dev;
mod icons;
mod plugins;
mod project;
mod terminal;

fn main() -> eframe::Result {
    if std::env::args_os().any(|a| a == dev::FLAG) {
        if let Err(e) = dev::supervise() {
            eprintln!("[dev] {e:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Forge IDE")
            .with_inner_size([1440.0, 920.0])
            .with_min_inner_size([900.0, 600.0])
            // The app draws its own title bar (see `Ide::toolbar`).
            .with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "Forge IDE",
        options,
        Box::new(|cc| Ok(Box::new(app::Ide::new(cc)))),
    )
}
