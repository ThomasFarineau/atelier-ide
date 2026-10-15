//! Drawing and keyboard of the terminal: colored cells, cursor, selection, scrollback, as in a
//! real terminal emulator.

use crate::{
    terminal::Terminal,
    terminal_links::{self, Link},
    ui::Theme,
};
use eframe::egui::{
    self, Color32, Event, FontId, Key, Modifiers, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2,
    text::LayoutJob, vec2,
};
use std::path::Path;

pub(crate) const FONT_SIZE: f32 = 13.0;

const DARK: [u32; 16] = [
    0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666,
    0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
];
const LIGHT: [u32; 16] = [
    0x000000, 0xcd3131, 0x00a000, 0x949800, 0x0451a5, 0xbc05bc, 0x0598bc, 0x555555, 0x666666,
    0xcd3131, 0x14a014, 0xb5ba00, 0x0451a5, 0xbc05bc, 0x0598bc, 0xa5a5a5,
];

fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn indexed(i: u8, dark: bool) -> Color32 {
    match i {
        0..=15 => rgb(if dark { DARK } else { LIGHT }[i as usize]),
        16..=231 => {
            let n = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            Color32::from_rgb(level(n / 36), level(n / 6 % 6), level(n % 6))
        }
        _ => {
            let v = 8 + 10 * (i - 232);
            Color32::from_rgb(v, v, v)
        }
    }
}

fn color(c: vt100::Color, default: Color32, dark: bool, bold: bool) -> Color32 {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => indexed(if bold && i < 8 { i + 8 } else { i }, dark),
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Style {
    fg: Color32,
    bg: Option<Color32>,
    italic: bool,
    underline: bool,
}

fn style(cell: &vt100::Cell, t: &Theme) -> Style {
    let fg = color(cell.fgcolor(), t.text, t.dark, cell.bold());
    let bg = match cell.bgcolor() {
        vt100::Color::Default => None,
        c => Some(color(c, t.island, t.dark, false)),
    };
    let (fg, bg) = if cell.inverse() {
        (bg.unwrap_or(t.island), Some(fg))
    } else {
        (fg, bg)
    };
    Style {
        fg,
        bg,
        italic: cell.italic(),
        underline: cell.underline(),
    }
}

/// Bytes sent to the shell for a key, or None if it is not one.
fn key_bytes(key: Key, m: Modifiers, application: bool) -> Option<Vec<u8>> {
    let csi = |c: char| -> Vec<u8> {
        if application {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    let tilde = |n: u8| format!("\x1b[{n}~").into_bytes();
    Some(match key {
        Key::Enter => vec![b'\r'],
        Key::Backspace => vec![0x7f],
        Key::Tab if m.shift => b"\x1b[Z".to_vec(),
        Key::Tab => vec![b'\t'],
        Key::Escape => vec![0x1b],
        Key::ArrowUp => csi('A'),
        Key::ArrowDown => csi('B'),
        Key::ArrowRight => csi('C'),
        Key::ArrowLeft => csi('D'),
        Key::Home => csi('H'),
        Key::End => csi('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => b"\x1bOP".to_vec(),
        Key::F2 => b"\x1bOQ".to_vec(),
        Key::F3 => b"\x1bOR".to_vec(),
        Key::F4 => b"\x1bOS".to_vec(),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        // Alt + a letter or a digit: Escape then the character, as AIs read Alt+V (paste an
        // image) and shells Alt+B or Alt+F (a word back or forward).
        _ if m.alt && !m.ctrl => {
            let name = key.name();
            let c = name.chars().next().filter(|_| name.len() == 1)?;
            if !c.is_ascii_alphanumeric() {
                return None;
            }
            let c = if m.shift {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            };
            vec![0x1b, c as u8]
        }
        _ if m.ctrl && !m.alt => {
            let name = key.name();
            let letter = name.chars().next().filter(|_| name.len() == 1)?;
            if !letter.is_ascii_alphabetic() {
                return None;
            }
            vec![letter.to_ascii_lowercase() as u8 - b'a' + 1]
        }
        _ => return None,
    })
}

/// What `lines` turns of the wheel send to a program that took the screen, the pointer on
/// `cell` (row, column): wheel events in the encoding it asked for, else the arrow keys.
pub fn wheel_bytes(screen: &vt100::Screen, up: bool, lines: usize, cell: (u16, u16)) -> Vec<u8> {
    use vt100::{MouseProtocolEncoding, MouseProtocolMode};
    let mut out = Vec::new();
    if screen.mouse_protocol_mode() == MouseProtocolMode::None {
        let arrow: &[u8] = match (up, screen.application_cursor()) {
            (true, true) => b"OA",
            (true, false) => b"[A",
            (false, true) => b"OB",
            (false, false) => b"[B",
        };
        for _ in 0..lines {
            out.extend_from_slice(arrow);
        }
        return out;
    }
    let button: u32 = if up { 64 } else { 65 };
    let (row, col) = (u32::from(cell.0) + 1, u32::from(cell.1) + 1);
    for _ in 0..lines {
        match screen.mouse_protocol_encoding() {
            MouseProtocolEncoding::Sgr => {
                out.extend(format!("[<{button};{col};{row}M").into_bytes());
            }
            MouseProtocolEncoding::Utf8 => {
                out.extend_from_slice(b"[M");
                for value in [button, col, row] {
                    let c = char::from_u32(32 + value).unwrap_or(' ');
                    out.extend(c.to_string().into_bytes());
                }
            }
            MouseProtocolEncoding::Default => {
                out.extend_from_slice(b"[M");
                for value in [button, col, row] {
                    out.push((32 + value).min(255) as u8);
                }
            }
        }
    }
    out
}

/// Text of the selection, rows joined by line breaks, trailing spaces removed.
fn selected_text(screen: &vt100::Screen, from: (u16, u16), to: (u16, u16)) -> String {
    let (_, cols) = screen.size();
    let mut out = String::new();
    for row in from.0..=to.0 {
        let start = if row == from.0 { from.1 } else { 0 };
        let end = if row == to.0 {
            to.1
        } else {
            cols.saturating_sub(1)
        };
        let mut line = String::new();
        for col in start..=end {
            match screen.cell(row, col) {
                Some(cell) if cell.is_wide_continuation() => {}
                Some(cell) if cell.has_contents() => line.push_str(cell.contents()),
                _ => line.push(' '),
            }
        }
        out.push_str(line.trim_end());
        if row != to.0 {
            out.push('\n');
        }
    }
    out
}

/// The text of a row with one character per column (the second half of a wide character is a
/// space), so that a character of it is a column.
fn row_text(screen: &vt100::Screen, row: u16, cols: u16) -> String {
    (0..cols)
        .map(|col| match screen.cell(row, col) {
            Some(cell) if !cell.is_wide_continuation() && cell.has_contents() => {
                cell.contents().chars().next().unwrap_or(' ')
            }
            _ => ' ',
        })
        .collect()
}

/// A line of text that may go on over several rows, with the cell of each of its characters.
#[derive(Default)]
struct Line {
    text: String,
    cells: Vec<(u16, u16)>,
}

impl Line {
    fn push(&mut self, row: u16, chars: impl Iterator<Item = (usize, char)>) {
        for (col, c) in chars {
            self.text.push(c);
            self.cells.push((row, col as u16));
        }
    }
}

/// The line holding `row`. The terminal joins the rows it wrapped itself; `cut` also joins a row
/// that ends at the right edge to the text of the next one, past its indentation, as a program
/// that wraps its own text (an AI's answer) cuts a word too long for a row.
fn line_at(screen: &vt100::Screen, row: u16, rows: u16, cols: u16, cut: bool) -> Line {
    let texts: Vec<Vec<char>> = (0..rows)
        .map(|r| row_text(screen, r, cols).chars().collect())
        .collect();
    let end = |r: u16| texts[r as usize].iter().rposition(|c| *c != ' ');
    let indent = |r: u16| texts[r as usize].iter().position(|c| *c != ' ');
    let goes_on = |r: u16| {
        r + 1 < rows
            && (screen.row_wrapped(r)
                || cut && end(r).is_some_and(|e| e + 4 >= cols as usize) && indent(r + 1).is_some())
    };
    // Five rows each way: an address is rarely longer.
    let mut first = row;
    while first > 0 && row - first < 5 && goes_on(first - 1) {
        first -= 1;
    }
    let mut last = row;
    while last - row < 5 && goes_on(last) {
        last += 1;
    }
    let mut line = Line::default();
    for r in first..=last {
        let chars = &texts[r as usize];
        let from = if r > first && !screen.row_wrapped(r - 1) {
            indent(r).unwrap_or(0)
        } else {
            0
        };
        let to = if r < last && !screen.row_wrapped(r) {
            end(r).map_or(0, |e| e + 1)
        } else {
            chars.len()
        };
        line.push(
            r,
            chars[from..to]
                .iter()
                .copied()
                .enumerate()
                .map(|(i, c)| (from + i, c)),
        );
    }
    line
}

/// The columns a link spans on each of its rows.
type Spans = Vec<(u16, std::ops::Range<usize>)>;

/// The link at a cell of the screen, with the columns it spans on each of its rows.
fn link_at(
    screen: &vt100::Screen,
    (row, col): (u16, u16),
    rows: u16,
    cols: u16,
    root: &Path,
) -> Option<(Spans, Link)> {
    let find = |line: &Line| {
        let at = line.cells.iter().position(|cell| *cell == (row, col))?;
        terminal_links::find(&line.text, root, |p| p.is_file())
            .into_iter()
            .find(|(range, _)| range.contains(&at))
    };
    let wrapped = line_at(screen, row, rows, cols, false);
    let mut found = find(&wrapped).map(|link| (&wrapped, link));
    // A program cuts only an address longer than its rows: a shorter one ending at the edge is
    // followed by other words.
    let cut = line_at(screen, row, rows, cols, true);
    if let Some((range, link)) = find(&cut)
        && matches!(link, Link::Url(_))
        && range.len() + 6 > cols as usize
        && found
            .as_ref()
            .is_none_or(|(_, (r, _))| range.len() > r.len())
    {
        found = Some((&cut, (range, link)));
    }
    let (line, (range, link)) = found?;
    let mut spans = Spans::new();
    for &(r, c) in &line.cells[range] {
        let c = c as usize;
        match spans.last_mut() {
            Some((last, span)) if *last == r => span.end = c + 1,
            _ => spans.push((r, c..c + 1)),
        }
    }
    Some((spans, link))
}

/// Selection anchor and head, as (row, column) of the visible screen.
#[derive(Default, Clone, Copy)]
pub struct Selection {
    anchor: Option<(u16, u16)>,
    head: (u16, u16),
}

impl Selection {
    fn range(&self) -> Option<((u16, u16), (u16, u16))> {
        let anchor = self.anchor?;
        (anchor != self.head).then(|| (anchor.min(self.head), anchor.max(self.head)))
    }
}

/// Draws the terminal in the available space and handles the mouse and the keyboard. Paths
/// in its text are read from `root`.
pub fn show(
    ui: &mut Ui,
    term: &mut Terminal,
    selection: &mut Selection,
    t: &Theme,
    root: &Path,
) -> egui::Response {
    let font = FontId::monospace(FONT_SIZE);
    let (row_h, char_w) = ui.fonts_mut(|f| (f.row_height(&font).ceil(), f.glyph_width(&font, 'M')));
    let size = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    term.resize(
        (rect.height() / row_h) as u16,
        (rect.width() / char_w) as u16,
    );
    let (rows, cols) = term.screen_ref().size();
    // What a program copied (an AI that has its own selection) goes to the clipboard.
    if let Some(text) = term.take_copied() {
        ui.ctx().copy_text(text);
    }

    if response.clicked() || response.drag_started() || term.want_focus {
        term.want_focus = false;
        response.request_focus();
    }
    let focused = response.has_focus();
    term.focused = focused;

    // Mouse selection.
    let cell_at = |p: Pos2| -> (u16, u16) {
        let row = ((p.y - rect.top()) / row_h)
            .floor()
            .clamp(0.0, rows as f32 - 1.0) as u16;
        let col = ((p.x - rect.left()) / char_w)
            .floor()
            .clamp(0.0, cols as f32 - 1.0) as u16;
        (row, col)
    };
    // The wheel scrolls the history. A program that took the whole screen (an editor, a pager,
    // an AI) or asked for the mouse gets it instead, as wheel events or arrow keys.
    if response.hovered() {
        // Small steps add up until they make a line, in both directions.
        term.wheel += ui.input(|i| i.smooth_scroll_delta.y);
        let lines = (term.wheel / row_h).trunc();
        if lines != 0.0 {
            term.wheel -= lines * row_h;
            let screen = term.screen_ref();
            if screen.alternate_screen()
                || screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None
            {
                let cell = response.hover_pos().map_or((0, 0), cell_at);
                let bytes = wheel_bytes(screen, lines > 0.0, lines.abs() as usize, cell);
                let _ = term.write(&bytes);
            } else {
                term.scroll(lines as isize);
                *selection = Selection::default();
            }
        }
    }
    // Links: the one under the pointer is underlined, a click opens it.
    let hovered_link = response
        .hover_pos()
        .and_then(|p| link_at(term.screen_ref(), cell_at(p), rows, cols, root));
    if response.clicked()
        && let Some((_, link)) = &hovered_link
    {
        match link {
            Link::Url(url) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
            Link::File { path, line, column } => {
                term.open_file = Some((path.clone(), *line, *column));
            }
        }
    }
    let response = if hovered_link.is_some() {
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tr!("terminal.open_link"))
    } else {
        response
    };
    if let Some(p) = response.interact_pointer_pos() {
        if response.drag_started() || response.clicked() {
            // A drag is known once the pointer has moved: it starts where the button went down.
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
            *selection = Selection {
                anchor: Some(cell_at(origin)),
                head: cell_at(p),
            };
        } else if response.dragged() {
            selection.head = cell_at(p);
        }
    }

    // Keyboard.
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
        let events = ui.input(|i| i.events.clone());
        let application = term.screen_ref().application_cursor();
        let bracketed = term.screen_ref().bracketed_paste();
        let mut send: Vec<u8> = Vec::new();
        for event in events {
            match event {
                Event::Text(text) => {
                    if let Some(typed) = &mut term.typed {
                        typed.push_str(&text);
                    }
                    send.extend(text.into_bytes());
                }
                Event::Paste(text) => {
                    // A pasted line may hold several commands: not tracked.
                    term.typed = (!text.contains(['\n', '\r']))
                        .then(|| term.typed.clone().unwrap_or_default() + &text);
                    if bracketed {
                        send.extend(b"\x1b[200~");
                        send.extend(text.replace("\r\n", "\r").replace('\n', "\r").into_bytes());
                        send.extend(b"\x1b[201~");
                    } else {
                        send.extend(text.replace("\r\n", "\r").replace('\n', "\r").into_bytes());
                    }
                }
                // Ctrl+C copies a selection, otherwise it interrupts the program.
                Event::Copy => match selection.range() {
                    Some((from, to)) => {
                        let text = selected_text(term.screen_ref(), from, to);
                        ui.ctx().copy_text(text);
                        *selection = Selection::default();
                    }
                    None => {
                        term.typed = Some(String::new());
                        send.push(3)
                    }
                },
                Event::Cut => send.push(0x18),
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if modifiers.ctrl && key == Key::Backtick {
                        response.surrender_focus();
                    } else if modifiers.shift && matches!(key, Key::PageUp | Key::PageDown) {
                        let page = rows.saturating_sub(1) as isize;
                        term.scroll(if key == Key::PageUp { page } else { -page });
                    } else if modifiers.command && matches!(key, Key::C | Key::V | Key::X) {
                        // Handled through the Copy, Paste and Cut events.
                    } else if key == Key::Enter && !modifiers.shift && !modifiers.alt {
                        // Starting a known AI: the IDE offers to connect it first, and sends the
                        // Enter when the user has answered.
                        let agent = term
                            .typed
                            .as_deref()
                            .and_then(crate::plugins::builtins::mcp::agents::detect)
                            .filter(|_| term.ai_watch);
                        term.typed = Some(String::new());
                        match agent {
                            Some(agent) => {
                                term.pending_ai = Some(agent);
                                break;
                            }
                            None => send.push(b'\r'),
                        }
                    } else if let Some(bytes) = key_bytes(key, modifiers, application) {
                        match key {
                            Key::Backspace => {
                                if let Some(typed) = &mut term.typed {
                                    typed.pop();
                                }
                            }
                            // Moving in the line, history, completion, interrupting: the line is
                            // no longer what was typed.
                            Key::Escape
                            | Key::ArrowUp
                            | Key::ArrowDown
                            | Key::ArrowLeft
                            | Key::ArrowRight
                            | Key::Tab
                            | Key::Delete
                            | Key::Home
                            | Key::End => {
                                term.typed = None;
                            }
                            Key::C | Key::D | Key::U | Key::L if modifiers.ctrl => {
                                term.typed = Some(String::new())
                            }
                            _ if modifiers.ctrl || modifiers.alt => term.typed = None,
                            _ => {}
                        }
                        send.extend(bytes);
                    }
                }
                _ => {}
            }
        }
        if !send.is_empty() {
            term.scroll_to_bottom();
            *selection = Selection::default();
            let _ = term.write(&send);
        }
    }

    // Drawing.
    let painter = ui.painter_at(rect);
    let screen = term.screen_ref();
    let selected = selection.range();
    for row in 0..rows {
        let y = rect.top() + row as f32 * row_h;
        // Backgrounds first, then the selection, then the text, which stays readable over it.
        let mut texts = Vec::new();
        let mut col = 0;
        while col < cols {
            let Some(cell) = screen.cell(row, col) else {
                break;
            };
            let st = style(cell, t);
            let start = col;
            let mut text = String::new();
            while col < cols {
                let Some(cell) = screen.cell(row, col) else {
                    break;
                };
                if style(cell, t) != st {
                    break;
                }
                if !cell.is_wide_continuation() {
                    if cell.has_contents() {
                        text.push_str(cell.contents());
                    } else {
                        text.push(' ');
                    }
                }
                col += 1;
            }
            let x = rect.left() + start as f32 * char_w;
            if let Some(bg) = st.bg {
                let run = Rect::from_min_max(
                    pos2(x, y),
                    pos2(x + (col - start) as f32 * char_w, y + row_h),
                );
                painter.rect_filled(run, 0.0, bg);
            }
            if !text.trim().is_empty() || st.underline {
                texts.push((x, st, text));
            }
        }
        if let Some((from, to)) = selected
            && (from.0..=to.0).contains(&row)
        {
            let start = if row == from.0 { from.1 } else { 0 };
            let end = if row == to.0 { to.1 + 1 } else { cols };
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(rect.left() + start as f32 * char_w, y),
                    pos2(rect.left() + end as f32 * char_w, y + row_h),
                ),
                0.0,
                t.text_selection,
            );
        }
        for (x, st, text) in texts {
            let mut job = LayoutJob::default();
            job.append(
                &text,
                0.0,
                egui::TextFormat {
                    font_id: font.clone(),
                    color: st.fg,
                    italics: st.italic,
                    underline: if st.underline {
                        Stroke::new(1.0_f32, st.fg)
                    } else {
                        Stroke::NONE
                    },
                    ..Default::default()
                },
            );
            job.wrap.max_width = f32::INFINITY;
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            painter.galley(pos2(x, y + (row_h - galley.size().y) / 2.0), galley, st.fg);
        }
    }
    if let Some((spans, _)) = &hovered_link {
        for (row, range) in spans {
            let y = rect.top() + (*row as f32 + 1.0) * row_h - 1.5;
            painter.hline(
                rect.left() + range.start as f32 * char_w..=rect.left() + range.end as f32 * char_w,
                y,
                Stroke::new(1.0_f32, t.link),
            );
        }
    }
    if !screen.hide_cursor() && screen.scrollback() == 0 {
        let (row, col) = screen.cursor_position();
        let cursor = Rect::from_min_size(
            pos2(
                rect.left() + col as f32 * char_w,
                rect.top() + row as f32 * row_h,
            ),
            vec2(char_w, row_h),
        );
        if focused {
            painter.rect_filled(cursor, 0.0, t.text.gamma_multiply(0.7));
        } else {
            painter.rect_stroke(
                cursor,
                0.0,
                Stroke::new(1.0_f32, t.text_secondary),
                StrokeKind::Inside,
            );
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_found_under_the_pointer_whatever_the_colors() {
        let mut parser = vt100::Parser::new(4, 80, 0);
        // A colored address, as cargo and npm print them.
        parser.process(
            b"Serving at \x1b[36mhttp://localhost:8080/\x1b[0m now\r\n\xe6\x97\xa5 https://x.dev",
        );
        let screen = parser.screen();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let (spans, link) = link_at(screen, (0, 15), 4, 80, root).expect("a link");
        assert_eq!(spans, [(0, 11..33)]);
        assert_eq!(link, Link::Url("http://localhost:8080/".into()));
        assert!(link_at(screen, (0, 3), 4, 80, root).is_none());
        // After a wide character, columns and characters still agree.
        let (spans, _) =
            link_at(screen, (1, 4), 4, 80, root).expect("a link after a wide character");
        assert_eq!(spans[0].1.start, 3);
    }

    #[test]
    fn an_address_on_several_rows_is_one_link() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let url = format!("https://example.com/{}", "abcdefghij".repeat(5));
        // Wrapped by the terminal: the rows go on into each other.
        let mut parser = vt100::Parser::new(4, 40, 0);
        parser.process(format!("see {url} ok").as_bytes());
        let (spans, link) = link_at(parser.screen(), (1, 10), 4, 40, root).expect("a link");
        assert_eq!(link, Link::Url(url.clone()));
        assert_eq!(spans, [(0, 4..40), (1, 0..(4 + url.len() - 40))]);
        // Cut by a program that wraps its own text, as an AI does: the next rows are indented.
        let mut parser = vt100::Parser::new(4, 40, 0);
        let chars: Vec<char> = format!("❯ see {url} ok").chars().collect();
        let mut rows: Vec<String> = vec![chars[..38].iter().collect()];
        for piece in chars[38..].chunks(36) {
            rows.push(format!("  {}", piece.iter().collect::<String>()));
        }
        parser.process(rows.join("\r\n").as_bytes());
        let (spans, link) = link_at(parser.screen(), (1, 10), 4, 40, root).expect("a link");
        assert_eq!(link, Link::Url(url.clone()));
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0], (0, 6..38));
        // A short address at the end of a row is not joined to the words of the next one.
        let mut parser = vt100::Parser::new(4, 40, 0);
        parser.process(b"a few words then https://x.dev/abcdefgh\r\n  more words");
        let (_, link) = link_at(parser.screen(), (0, 20), 4, 40, root).expect("a link");
        assert_eq!(link, Link::Url("https://x.dev/abcdefgh".into()));
    }

    #[test]
    fn alt_and_a_letter_send_escape_then_the_letter() {
        let alt = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert_eq!(key_bytes(Key::V, alt, false), Some(b"\x1bv".to_vec()));
        let alt_shift = Modifiers { shift: true, ..alt };
        assert_eq!(key_bytes(Key::B, alt_shift, false), Some(b"\x1bB".to_vec()));
        // AltGr is Ctrl + Alt: the character comes as text.
        assert_eq!(
            key_bytes(Key::E, Modifiers { ctrl: true, ..alt }, false),
            None
        );
    }

    #[test]
    fn paths_of_the_project_are_links() {
        let mut parser = vt100::Parser::new(2, 80, 0);
        parser.process(b"error: --> src/main.rs:12:5");
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let (_, link) = link_at(parser.screen(), (0, 15), 2, 80, root).expect("a link");
        assert_eq!(
            link,
            Link::File {
                path: root.join("src/main.rs"),
                line: Some(12),
                column: Some(5)
            }
        );
    }
}
