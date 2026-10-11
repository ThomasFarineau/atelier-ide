use super::*;
use std::path::{Path, PathBuf};

struct NoHost(PathBuf);
impl Host for NoHost {
    fn root(&self) -> &Path {
        &self.0
    }
    fn active_file(&self) -> Option<PathBuf> {
        None
    }
    fn open_file(&mut self, _: PathBuf) {}
    fn open_project(&mut self, _: PathBuf) {}
    fn save_files(&mut self) -> bool {
        true
    }
    fn reload_files(&mut self) {}
    fn show_tool_window(&mut self, _: &'static ToolWindow) {}
    fn notify(&mut self, _: String) {}
}

#[test]
fn parses_kubectl_columns() {
    let text = "NAME         READY   STATUS             RESTARTS      AGE   NOMINATED NODE\n\
                    web-1        1/1     Running            0             3d    <none>\n\
                    job-x        0/1     CrashLoopBackOff   5 (2m ago)    1h    <none>\n";
    let table = parse_table(text);
    assert_eq!(
        table.headers,
        [
            "NAME",
            "READY",
            "STATUS",
            "RESTARTS",
            "AGE",
            "NOMINATED NODE"
        ]
    );
    assert_eq!(table.rows[1][2], "CrashLoopBackOff");
    assert_eq!(table.rows[1][3], "5 (2m ago)");
    assert_eq!(table.rows[0][5], "<none>");
    assert_eq!(parse_table(""), Table::default());
}

#[test]
fn sorts_cells_by_column_type() {
    use std::cmp::Ordering::{Greater, Less};
    assert_eq!(compare_cells("AGE", "5m", "1h"), Less);
    assert_eq!(compare_cells("AGE", "2d3h", "2d"), Greater);
    assert_eq!(compare_cells("RESTARTS", "10 (2m ago)", "9"), Greater);
    assert_eq!(compare_cells("READY", "1/3", "2/2"), Less);
    assert_eq!(compare_cells("NAME", "api", "Web"), Less);
}

#[test]
fn secrets_are_edited_decoded_and_sent_as_string_data() {
    let doc = serde_json::json!({
        "kind": "Secret",
        "metadata": {"name": "db", "namespace": "dev", "managedFields": []},
        "data": {"user": "YWRtaW4=", "blob": "//79"}
    });
    let mut editor = Editor::new("secrets", doc).unwrap();
    assert_eq!(editor.entries, [("user".to_string(), "admin".to_string())]);
    assert_eq!(editor.binary, ["blob"]);
    editor.entries[0].1 = "root".into();
    editor.new_key = "port".into();
    editor.add_key();
    let out = editor.document();
    assert_eq!(out["stringData"]["user"], "root");
    assert_eq!(out["stringData"]["port"], "");
    assert_eq!(out["data"]["blob"], "//79");
    assert!(out["data"].get("user").is_none());
    assert!(out["metadata"].get("managedFields").is_none());
}

#[test]
fn config_maps_keep_binary_data_and_validate_keys() {
    let doc = serde_json::json!({
        "metadata": {"name": "cfg"}, "data": {"a": "1"}, "binaryData": {"b": "AAEC"}
    });
    let mut editor = Editor::new("configmaps", doc).unwrap();
    editor.new_key = "bad key".into();
    editor.add_key();
    assert!(editor.error.is_some());
    editor.new_key = "b".into();
    editor.add_key();
    assert!(editor.error.is_some(), "binary keys are taken");
    editor.entries.clear();
    assert_eq!(editor.document()["data"], serde_json::json!({}));
    assert_eq!(editor.document()["binaryData"]["b"], "AAEC");
}

#[test]
fn decodes_base64() {
    assert_eq!(b64_decode("aGVsbG8=").unwrap(), b"hello");
    assert_eq!(b64_decode("aGk").unwrap(), b"hi");
    assert!(b64_decode("a*b").is_none());
}

#[test]
fn row_identity_follows_the_namespace_column() {
    let mut plugin = K8sPlugin::default();
    let row = vec![
        "kube-system".to_string(),
        "dns".to_string(),
        "1/1".to_string(),
    ];
    plugin.all_namespaces = true;
    assert_eq!(
        plugin.identity(&row),
        Some(("kube-system".into(), "dns".into()))
    );
    plugin.all_namespaces = false;
    plugin.namespace = "dev".into();
    let row = vec!["dns".to_string()];
    assert_eq!(plugin.identity(&row), Some(("dev".into(), "dns".into())));
}

#[test]
#[allow(clippy::field_reassign_with_default)]
fn window_renders() {
    let ctx = egui::Context::default();
    ui::install(&ctx, ui::theme_by_index(0), 14.0);
    let mut host = NoHost(PathBuf::new());
    let mut plugin = K8sPlugin::default();
    plugin.table = parse_table("NAME   READY\nweb   1/1\n");
    plugin.meta_loaded = true;
    plugin.selected = Some((String::new(), "web".into()));
    plugin.detail = Some(Detail {
        title: "web.yaml".into(),
        text: "kind: Pod".into(),
        error: false,
    });
    plugin.detail = None;
    plugin.editor = Editor::new(
        "configmaps",
        serde_json::json!({"metadata": {"name": "cfg"}, "data": {"a": "1\n2"}}),
    )
    .ok();
    for _ in 0..2 {
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| plugin.tool_window("k8s.resources", ui, &mut host));
        });
    }
    assert!(plugin.editor.is_some());
    plugin.confirm = Some(Confirm {
        title: tr!("k8s.delete").into(),
        text: "?".into(),
        label: "x".into(),
        args: Vec::new(),
    });
    for _ in 0..2 {
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| plugin.tool_window("k8s.resources", ui, &mut host));
        });
    }
    assert!(plugin.confirm.is_some());
}
