use super::*;

#[test]
fn icons_follow_the_theme_and_plugins_keep_their_own() {
    for name in ["git", "branch", "commit", "push", "vcsCommit", "update"] {
        let (light, dark) = (icon_source(name, false), icon_source(name, true));
        assert_ne!(light.uri(), dark.uri(), "{name}: one file per theme");
    }
    // Only a plain file: both themes share it.
    let plain = crate::icon_set::find("msoffice/icon").unwrap();
    assert!(plain.variants[1].is_none() && plain.variants[2].is_none());
    assert_eq!(
        crate::icon_set::source("msoffice/icon", true)
            .unwrap()
            .uri(),
        crate::icon_set::source("msoffice/icon", false)
            .unwrap()
            .uri()
    );
    // A name that exists nowhere falls back to a dot instead of failing.
    assert!(
        icon_source("nope", true)
            .uri()
            .unwrap()
            .contains("moreHorizontal")
    );
    assert!(
        icon_on_accent("project")
            .uri()
            .unwrap()
            .contains("ui-white")
    );
}

#[test]
fn themes_install_and_fonts_resolve() {
    let ctx = egui::Context::default();
    for index in 0..THEMES.len() {
        install(&ctx, theme_by_index(index), 14.0);
        assert_eq!(theme(&ctx), theme_by_index(index));
    }
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(RichText::new("Projet").font(semibold(13.0)));
            ui.monospace("fn main() {}");
        });
    });
}
