use super::*;

fn release(tag: &str, files: &[&str]) -> Release {
    Release {
        tag_name: tag.into(),
        html_url: format!("https://example.org/{tag}"),
        draft: false,
        prerelease: false,
        assets: files
            .iter()
            .map(|name| Asset {
                name: name.to_string(),
                browser_download_url: format!("https://example.org/{tag}/{name}"),
            })
            .collect(),
    }
}

#[test]
fn tags_of_plugins_are_read() {
    assert_eq!(
        plugin_tag("plugins/gltf-viewer/v0.3.0"),
        Some(("gltf-viewer", "0.3.0"))
    );
    assert_eq!(plugin_tag("app/v0.3.0"), None);
    assert_eq!(plugin_tag("plugins/x/0.3.0"), None);
    assert_eq!(plugin_tag("plugins//v1"), None);
    assert!(version_key("0.10.0") > version_key("0.9.1"));
    assert_eq!(version_key("v1.2.3-beta"), [1, 2, 3]);
}

#[test]
fn the_latest_release_of_each_plugin_with_its_files_is_kept() {
    let both = ["db.atelier-plugin", "plugin.json"];
    let mut drafted = release("plugins/db/v9.0.0", &both);
    drafted.draft = true;
    let releases = [
        release("app/v0.3.0", &["atelier.exe"]),
        release("plugins/db/v0.3.0", &both),
        release("plugins/db/v0.10.0", &both),
        drafted,
        // Built for the IDE 0.2 (no manifest of its own): not offered.
        release("plugins/viewer/v0.2.0", &["viewer.atelier-plugin"]),
        release(
            "plugins/viewer/v0.3.0",
            &["viewer.atelier-plugin", "plugin.json"],
        ),
    ];
    let latest = latest(&releases);
    let found: Vec<(&str, &str)> = latest
        .iter()
        .map(|(e, _)| (e.id.as_str(), e.version.as_str()))
        .collect();
    assert_eq!(found, [("db", "0.10.0"), ("viewer", "0.3.0")]);
    assert!(latest[0].0.download.ends_with("v0.10.0/db.atelier-plugin"));
    assert!(latest[0].1.ends_with("v0.10.0/plugin.json"));
}
