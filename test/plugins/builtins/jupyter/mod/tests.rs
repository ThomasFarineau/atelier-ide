use super::*;

#[test]
fn notebooks_open_as_a_preview() {
    let plugin = JupyterPlugin::default();
    assert_eq!(
        plugin.editor_layout(Path::new("a/b.ipynb")),
        Some(EditorLayout::Preview)
    );
    assert_eq!(plugin.editor_layout(Path::new("a/b.py")), None);
    assert_eq!(
        plugin.file_icon_for(Path::new("a.ipynb")),
        Some("jupyter/extensions/ipynb")
    );
}

/// Draws a notebook with every kind of output, and one that is not valid.
#[test]
fn a_notebook_renders() {
    let mut plugin = JupyterPlugin::default();
    let mut notebook = notebook::empty();
    let cells = notebook["cells"].as_array_mut().unwrap();
    cells[0]["source"] = notebook::lines("print('hi')\n1 / 0");
    cells[0]["outputs"] = serde_json::json!([
        {"output_type": "stream", "name": "stdout", "text": ["hi\n"]},
        {"output_type": "error", "ename": "ZeroDivisionError", "evalue": "division by zero",
         "traceback": ["\u{1b}[0;31mZeroDivisionError\u{1b}[0m: division by zero"]},
        {"output_type": "display_data", "metadata": {},
         "data": {"image/png": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==", "text/plain": "<Figure>"}},
        {"output_type": "execute_result", "execution_count": 2, "metadata": {},
         "data": {"text/markdown": "**bold**", "text/plain": "x"}}
    ]);
    cells.push(serde_json::json!({"cell_type": "markdown", "metadata": {}, "source": ["# Title\n", "text"]}));
    cells.push(serde_json::json!({"cell_type": "raw", "metadata": {}, "source": "raw"}));
    let mut text = notebook::to_text(&notebook);
    let before = text.clone();
    let path = Path::new("demo.ipynb");
    let ctx = egui::Context::default();
    for _ in 0..3 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                plugin.preview(ui, path, &mut text, None);
            });
        });
    }
    assert_eq!(text, before, "drawing does not change the notebook");
    let mut broken = "{".to_string();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            plugin.preview(ui, Path::new("broken.ipynb"), &mut broken, None);
        });
    });
    assert_eq!(broken, "{");
}
