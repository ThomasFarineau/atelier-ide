use super::*;

#[test]
fn parses_quotes_line_breaks_and_empty_fields() {
    let rows = parse(
        "a,b,c\r\n\"x,1\",\"say \"\"hi\"\"\",\n\"multi\nline\",,z\n",
        ',',
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1], ["x,1", "say \"hi\"", ""]);
    assert_eq!(rows[2], ["multi\nline", "", "z"]);
    assert_eq!(parse("", ','), Vec::<Vec<String>>::new());
    assert_eq!(parse("a\n\nb", ','), [vec!["a"], vec!["b"]]);
    assert_eq!(parse("\u{feff}x;y", ';'), [vec!["x", "y"]]);
}

#[test]
fn detects_the_delimiter() {
    let p = Path::new("data.csv");
    assert_eq!(detect_delimiter(p, "a;b;c\n1;2;3"), ';');
    assert_eq!(detect_delimiter(p, "\"a,b\";c\n"), ';');
    assert_eq!(detect_delimiter(p, "a,b,c\n"), ',');
    assert_eq!(detect_delimiter(p, "single\n"), ',');
    assert_eq!(detect_delimiter(Path::new("x.TSV"), "a,b"), '\t');
    assert_eq!(detect_delimiter(p, "a\tb\n"), '\t');
}

#[test]
fn sorts_numbers_before_text_and_numerically() {
    assert_eq!(compare("9", "10"), Ordering::Less);
    assert_eq!(compare("2", "abc"), Ordering::Less);
    assert_eq!(compare("Banane", "cerise"), Ordering::Less);
    assert_eq!(column_name(0), "A");
    assert_eq!(column_name(26), "AA");
}

#[test]
fn rows_are_ordered_by_the_sorted_column() {
    let mut state = State::default();
    state.refresh(Path::new("a.csv"), "n,v\nx,10\ny,9\nz,100\n");
    assert_eq!(state.order, [1, 2, 3]);
    state.sort = Some((1, true));
    state.refresh(Path::new("a.csv"), "n,v\nx,10\ny,9\nz,100\n");
    assert_eq!(state.order, [2, 1, 3]);
    state.sort = Some((1, false));
    state.header = false;
    state.refresh(Path::new("a.csv"), "n,v\nx,10\ny,9\nz,100\n");
    assert_eq!(state.order, [0, 3, 1, 2]);
}

#[test]
fn csv_files_get_a_split_layout_and_the_table_renders() {
    let mut plugin = CsvPlugin::default();
    let path = Path::new("C:/projet/data.CSV");
    assert_eq!(plugin.editor_layout(path), Some(EditorLayout::Split));
    assert_eq!(plugin.editor_layout(Path::new("main.rs")), None);
    plugin.set_editor_layout(path, EditorLayout::Preview);
    assert_eq!(plugin.editor_layout(path), Some(EditorLayout::Preview));
    let ctx = egui::Context::default();
    ui::install(&ctx, ui::theme_by_index(0), 14.0);
    let mut text = "name,qty\napple,3\n\"pear, green\",5\n".to_string();
    for _ in 0..2 {
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                plugin.preview(ui, path, &mut text, None);
            });
        });
    }
    assert_eq!(plugin.states[path].order.len(), 2);
}
