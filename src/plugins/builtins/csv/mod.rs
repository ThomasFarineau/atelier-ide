//! CSV built-in plugin: table preview of delimiter-separated files (`.csv`, `.tsv`, `.psv`,
//! `.tab`) next to the source or instead of it, with sortable columns.

use crate::i18n::Text;
use crate::{
    plugins::builtin::{BuiltinPlugin, Command, EditorLayout, Host},
    ui,
};
use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};
use std::{
    cmp::Ordering,
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
};

const ROW_HEIGHT: f32 = 22.0;
const MAX_COLUMN_CHARS: usize = 40;
/// What the display order was computed for: (text version, header, sort column and direction).
type OrderKey = (u64, bool, Option<(usize, bool)>);

const DELIMITERS: [(Text, Option<char>); 5] = [
    (Text("csv.auto_2"), None),
    (Text("csv.comma_2"), Some(',')),
    (Text("csv.semicolon_2"), Some(';')),
    (Text("csv.tab_2"), Some('\t')),
    (Text("csv.pipe_2"), Some('|')),
];

pub fn is_csv(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "csv" | "tsv" | "psv" | "tab"
        )
    })
}

/// Delimiter of a file: fixed by some extensions, otherwise the most frequent candidate
/// outside quotes on the first line.
pub fn detect_delimiter(path: &Path, text: &str) -> char {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("tsv" | "tab") => return '\t',
        Some("psv") => return '|',
        _ => {}
    }
    let mut counts = [0usize; 4];
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '\n' if !quoted => break,
            ',' if !quoted => counts[0] += 1,
            ';' if !quoted => counts[1] += 1,
            '\t' if !quoted => counts[2] += 1,
            '|' if !quoted => counts[3] += 1,
            _ => {}
        }
    }
    let best = counts
        .iter()
        .enumerate()
        .max_by_key(|(i, n)| (**n, usize::MAX - i));
    match best {
        Some((i, n)) if *n > 0 => [',', ';', '\t', '|'][i],
        _ => ',',
    }
}

/// Splits `text` into records of fields: quoted fields may hold the delimiter, line breaks and
/// doubled quotes.
pub fn parse(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .chars()
        .peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => {
                quoted = true;
                started = true;
            }
            c if c == delimiter => {
                row.push(std::mem::take(&mut field));
                started = true;
            }
            '\r' => {}
            '\n' => {
                if started || !field.is_empty() || !row.is_empty() {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                started = false;
            }
            c => {
                field.push(c);
                started = true;
            }
        }
    }
    if started || !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// Numbers compare as numbers, everything else as text without case.
fn compare(a: &str, b: &str) -> Ordering {
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

fn column_name(mut index: usize) -> String {
    let mut name = String::new();
    loop {
        name.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            return name;
        }
        index = index / 26 - 1;
    }
}

struct Parsed {
    key: u64,
    delimiter: char,
    rows: Vec<Vec<String>>,
    columns: usize,
    /// Initial width of each column, from its widest cells.
    widths: Vec<f32>,
}

struct State {
    layout: EditorLayout,
    /// Forced delimiter, `None` to detect it.
    delimiter: Option<char>,
    /// The first record holds the column names.
    header: bool,
    /// Column and ascending order.
    sort: Option<(usize, bool)>,
    parsed: Option<Parsed>,
    /// Body rows (indices into `parsed.rows`) in display order.
    order: Vec<usize>,
    order_key: Option<OrderKey>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            layout: EditorLayout::default(),
            delimiter: None,
            header: true,
            sort: None,
            parsed: None,
            order: Vec::new(),
            order_key: None,
        }
    }
}

impl State {
    fn refresh(&mut self, path: &Path, text: &str) {
        let delimiter = self
            .delimiter
            .unwrap_or_else(|| detect_delimiter(path, text));
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        delimiter.hash(&mut hasher);
        let key = hasher.finish();
        if self.parsed.as_ref().is_none_or(|p| p.key != key) {
            let rows = parse(text, delimiter);
            let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
            let widths = (0..columns)
                .map(|c| {
                    let chars = rows
                        .iter()
                        .take(300)
                        .filter_map(|r| r.get(c))
                        .map(|cell| cell.lines().next().unwrap_or("").chars().count())
                        .max()
                        .unwrap_or(0);
                    chars.clamp(4, MAX_COLUMN_CHARS) as f32 * 7.5 + 20.0
                })
                .collect();
            self.parsed = Some(Parsed {
                key,
                delimiter,
                rows,
                columns,
                widths,
            });
        }
        let Some(parsed) = &self.parsed else { return };
        let order_key = (parsed.key, self.header, self.sort);
        if self.order_key != Some(order_key) {
            let start = usize::from(self.header && !parsed.rows.is_empty());
            let mut order: Vec<usize> = (start..parsed.rows.len()).collect();
            if let Some((col, asc)) = self.sort {
                let cell = |i: usize| parsed.rows[i].get(col).map_or("", String::as_str);
                order.sort_by(|&a, &b| {
                    let o = compare(cell(a), cell(b));
                    if asc { o } else { o.reverse() }
                });
            }
            self.order = order;
            self.order_key = Some(order_key);
        }
    }
}

#[derive(Default)]
pub struct CsvPlugin {
    states: HashMap<PathBuf, State>,
}

impl BuiltinPlugin for CsvPlugin {
    fn id(&self) -> &'static str {
        "csv"
    }
    fn name(&self) -> &'static str {
        "CSV / TSV"
    }
    fn description(&self) -> &'static str {
        tr!("csv.table_preview_csv_tsv_psv")
    }
    fn icon(&self) -> &'static str {
        "editorPreview"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("csv")
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("csv.text", tr!("csv.csv_editor_only")),
            ("csv.split", tr!("csv.csv_editor_table")),
            ("csv.preview", tr!("csv.csv_table_only")),
        ]
        .into_iter()
        .map(|(id, label)| Command {
            id,
            label: label.into(),
        })
        .collect()
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        let layout = match id {
            "csv.text" => EditorLayout::Text,
            "csv.split" => EditorLayout::Split,
            "csv.preview" => EditorLayout::Preview,
            _ => return,
        };
        match host.active_file().filter(|p| is_csv(p)) {
            Some(path) => self.set_editor_layout(&path, layout),
            None => host.notify(tr!("csv.open_csv_tsv_psv_file").into()),
        }
    }

    fn editor_layout(&self, path: &Path) -> Option<EditorLayout> {
        is_csv(path).then(|| {
            self.states
                .get(path)
                .map_or_else(EditorLayout::default, |s| s.layout)
        })
    }

    fn set_editor_layout(&mut self, path: &Path, layout: EditorLayout) {
        self.states.entry(path.to_path_buf()).or_default().layout = layout;
    }

    fn preview(&mut self, ui: &mut egui::Ui, path: &Path, text: &mut String, _scroll: Option<f32>) {
        let t = ui::theme(ui.ctx());
        let state = self.states.entry(path.to_path_buf()).or_default();
        state.refresh(path, text);

        // Options bar.
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            let mut changed =
                ui::checkbox(ui, &mut state.header, tr!("csv.first_row_as_header")).changed();
            ui.add_space(8.0);
            let detected = state.parsed.as_ref().map_or(',', |p| p.delimiter);
            let shown = match state.delimiter {
                None => trf!("csv.auto", detected = delimiter_name(detected)),
                Some(c) => delimiter_name(c).to_string(),
            };
            egui::ComboBox::from_id_salt(("csv_delimiter", path))
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for (label, value) in DELIMITERS {
                        if ui
                            .selectable_label(state.delimiter == value, label.get())
                            .clicked()
                        {
                            state.delimiter = value;
                            changed = true;
                        }
                    }
                });
            if changed {
                state.refresh(path, text);
            }
            if let Some(parsed) = &state.parsed {
                let body = parsed.rows.len().saturating_sub(usize::from(state.header));
                ui.label(
                    RichText::new(trf!(
                        "csv.rows_columns",
                        body = body,
                        columns = parsed.columns
                    ))
                    .small()
                    .color(t.text_secondary),
                );
            }
        });
        ui.add_space(4.0);

        let Some(parsed) = &state.parsed else { return };
        if parsed.rows.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(tr!("csv.empty_file")).color(t.text_secondary))
            });
            return;
        }
        let header_row = state.header.then(|| &parsed.rows[0]);
        let mut sort_clicked = None;
        let sort = state.sort;
        let order = &state.order;
        let rows = &parsed.rows;
        ui.push_id(("csv_table", path), |ui| {
            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut table = TableBuilder::new(ui)
                        .striped(true)
                        .resizable(true)
                        .vscroll(true)
                        .auto_shrink([false, false])
                        .column(Column::exact(52.0));
                    for width in &parsed.widths {
                        table = table.column(Column::initial(*width).at_least(36.0).clip(true));
                    }
                    table
                        .header(26.0, |mut header| {
                            header.col(|_| {});
                            for c in 0..parsed.columns {
                                header.col(|ui| {
                                    let name = header_row
                                        .and_then(|r| r.get(c))
                                        .filter(|n| !n.is_empty())
                                        .cloned()
                                        .unwrap_or_else(|| column_name(c));
                                    let arrow = match sort {
                                        Some((col, asc)) if col == c => {
                                            Some(if asc { "chevronUp" } else { "chevronDown" })
                                        }
                                        _ => None,
                                    };
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 2.0;
                                        let label = ui.add(
                                            egui::Label::new(
                                                RichText::new(name).font(ui::semibold(12.5)),
                                            )
                                            .truncate()
                                            .sense(egui::Sense::click()),
                                        );
                                        if let Some(icon) = arrow {
                                            ui.add(ui::icon(ui, icon));
                                        }
                                        if label
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            sort_clicked = Some(c);
                                        }
                                    });
                                });
                            }
                        })
                        .body(|body| {
                            body.rows(ROW_HEIGHT, order.len(), |mut row| {
                                let source = order[row.index()];
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(
                                            (source + 1 - usize::from(header_row.is_some()))
                                                .to_string(),
                                        )
                                        .small()
                                        .color(t.line_number),
                                    );
                                });
                                for c in 0..parsed.columns {
                                    row.col(|ui| {
                                        let cell = rows[source].get(c).map_or("", String::as_str);
                                        let first = cell.lines().next().unwrap_or("");
                                        let response = ui.add(egui::Label::new(first).truncate());
                                        if cell.len() > first.len() {
                                            response.on_hover_text(cell);
                                        }
                                    });
                                }
                            });
                        });
                });
        });
        if let Some(col) = sort_clicked {
            // Ascending, then descending, then the order of the file.
            state.sort = match state.sort {
                Some((c, true)) if c == col => Some((col, false)),
                Some((c, false)) if c == col => None,
                _ => Some((col, true)),
            };
            state.refresh(path, text);
        }
    }
}

fn delimiter_name(c: char) -> &'static str {
    match c {
        ',' => tr!("csv.comma"),
        ';' => tr!("csv.semicolon"),
        '\t' => tr!("csv.tab"),
        '|' => tr!("csv.pipe"),
        _ => tr!("csv.other"),
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/csv/mod/tests.rs"]
mod tests;
