//! Drawing and keyboard of the terminal: colored cells, cursor, selection, scrollback, as in a
//! real terminal emulator.

use crate::{terminal::Terminal, ui::Theme};
use eframe::egui::{
    self, Color32, Event, FontId, Key, Modifiers, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2,
    text::LayoutJob, vec2,
};

const FONT_SIZE: f32 = 13.0;

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

/// Text of the selection, rows joined by line breaks, trailing spaces removed.
fn selected_text(screen: &vt100::Screen, from: (u16, u16), to: (u16, u16)) -> String {
    let (_, cols) = screen.size();
    let mut out = String::new();
    for row in from.0..=to.0 {
        let start = if row == from.0 { from.1 } else { 0 };
        let end = if row == to.0 { to.1 } else { cols.saturating_sub(1) };
        let mut line = String::new();
        for col in start..=end {
            match screen.cell(row, col) {
                Some(cell) if cell.is_wide_continuation() => {}
                Some(cell) if cell.has_contents() => line.push_str(&cell.contents()),
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

/// Draws the terminal in the available space and handles the mouse and the keyboard.
pub fn show(ui: &mut Ui, term: &mut Terminal, selection: &mut Selection, t: &Theme) -> egui::Response {
    let font = FontId::monospace(FONT_SIZE);
    let (row_h, char_w) = ui.fonts_mut(|f| (f.row_height(&font).ceil(), f.glyph_width(&font, 'M')));
    let size = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    term.resize((rect.height() / row_h) as u16, (rect.width() / char_w) as u16);
    let (rows, cols) = term.screen_ref().size();

    if response.clicked() || response.drag_started() || term.want_focus {
        term.want_focus = false;
        response.request_focus();
    }
    let focused = response.has_focus();
    term.focused = focused;

    // Mouse wheel scrolls the history.
    if response.hovered() && !term.screen_ref().alternate_screen() {
        // Small steps add up until they make a line, in both directions.
        term.wheel += ui.input(|i| i.smooth_scroll_delta.y);
        let lines = (term.wheel / row_h).trunc();
        if lines != 0.0 {
            term.wheel -= lines * row_h;
            term.scroll(lines as isize);
            *selection = Selection::default();
        }
    }

    // Mouse selection.
    let cell_at = |p: Pos2| -> (u16, u16) {
        let row = ((p.y - rect.top()) / row_h).floor().clamp(0.0, rows as f32 - 1.0) as u16;
        let col = ((p.x - rect.left()) / char_w).floor().clamp(0.0, cols as f32 - 1.0) as u16;
        (row, col)
    };
    if let Some(p) = response.interact_pointer_pos() {
        if response.drag_started() || response.clicked() {
            *selection = Selection {
                anchor: Some(cell_at(p)),
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
                    term.typed = (!text.contains(['\n', '\r'])).then(|| term.typed.clone().unwrap_or_default() + &text);
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
                        let agent = term.typed.as_deref().and_then(crate::plugins::builtins::mcp::agents::detect).filter(|_| term.ai_watch);
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
                            Key::Escape | Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight | Key::Tab | Key::Delete | Key::Home | Key::End => {
                                term.typed = None;
                            }
                            Key::C | Key::D | Key::U | Key::L if modifiers.ctrl => term.typed = Some(String::new()),
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
    for row in 0..rows {
        let y = rect.top() + row as f32 * row_h;
        let mut col = 0;
        while col < cols {
            let Some(cell) = screen.cell(row, col) else { break };
            let st = style(cell, t);
            let start = col;
            let mut text = String::new();
            while col < cols {
                let Some(cell) = screen.cell(row, col) else { break };
                if style(cell, t) != st {
                    break;
                }
                if !cell.is_wide_continuation() {
                    if cell.has_contents() {
                        text.push_str(&cell.contents());
                    } else {
                        text.push(' ');
                    }
                }
                col += 1;
            }
            let x = rect.left() + start as f32 * char_w;
            if let Some(bg) = st.bg {
                let run = Rect::from_min_max(pos2(x, y), pos2(x + (col - start) as f32 * char_w, y + row_h));
                painter.rect_filled(run, 0.0, bg);
            }
            if text.trim().is_empty() && !st.underline {
                continue;
            }
            let mut job = LayoutJob::default();
            job.append(
                &text,
                0.0,
                egui::TextFormat {
                    font_id: font.clone(),
                    color: st.fg,
                    italics: st.italic,
                    underline: if st.underline { Stroke::new(1.0_f32, st.fg) } else { Stroke::NONE },
                    ..Default::default()
                },
            );
            job.wrap.max_width = f32::INFINITY;
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            painter.galley(pos2(x, y + (row_h - galley.size().y) / 2.0), galley, st.fg);
        }
    }
    if let Some((from, to)) = selection.range() {
        for row in from.0..=to.0 {
            let start = if row == from.0 { from.1 } else { 0 };
            let end = if row == to.0 { to.1 + 1 } else { cols };
            let y = rect.top() + row as f32 * row_h;
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(rect.left() + start as f32 * char_w, y),
                    pos2(rect.left() + end as f32 * char_w, y + row_h),
                ),
                0.0,
                t.text_selection,
            );
        }
    }
    if !screen.hide_cursor() && screen.scrollback() == 0 {
        let (row, col) = screen.cursor_position();
        let cursor = Rect::from_min_size(
            pos2(rect.left() + col as f32 * char_w, rect.top() + row as f32 * row_h),
            vec2(char_w, row_h),
        );
        if focused {
            painter.rect_filled(cursor, 0.0, t.text.gamma_multiply(0.7));
        } else {
            painter.rect_stroke(cursor, 0.0, Stroke::new(1.0_f32, t.text_secondary), StrokeKind::Inside);
        }
    }
    response
}
