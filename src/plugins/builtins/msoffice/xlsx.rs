//! Excel workbooks (`.xlsx`): cell values, number and date formats, fills, fonts, column widths,
//! drawn as a grid with one tab per sheet, and exported to PDF. Formulas show the value Excel
//! stored with the file; nothing is recalculated.

use super::{
    INK, MAX_FILE, Node, hex_color, local, lum, parse_xml,
    pdf::{Font, Pdf, fit, text_width},
    read_part,
};
use crate::ui;
use anyhow::{Context as _, Result, anyhow, bail};
use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, Stroke, StrokeKind, vec2};
use egui_extras::{Column, TableBuilder};
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use std::{collections::HashMap, fs, path::Path};

const MAX_CELLS: usize = 3_000_000;
const MAX_COLS: u32 = 1024;
const DEFAULT_WIDTH: f32 = 64.0;
const GRID: Color32 = Color32::from_rgb(0xD4, 0xD4, 0xD4);
const HEADER_FILL: Color32 = Color32::from_rgb(0xF0, 0xF0, 0xF0);

pub fn is_sheet(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "xlsx" | "xlsm" | "xltx" | "xltm" | "xls"
        )
    })
}

// ---- Number formats ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Lit(char),
    /// A character of the format code itself.
    Code(char),
}

#[derive(Clone, Debug, PartialEq, Default)]
enum Section {
    #[default]
    General,
    Text,
    Number {
        prefix: String,
        suffix: String,
        decimals: usize,
        int_min: usize,
        thousands: bool,
        percent: bool,
    },
    Date(Vec<Piece>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NumFmt {
    positive: Section,
    /// Section used for negative numbers, if the format has one.
    negative: Option<Section>,
}

fn builtin_code(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 | 37 | 38 => "#,##0",
        4 | 39..=44 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        14 => "dd/mm/yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "dd/mm/yyyy h:mm",
        45 => "mm:ss",
        46 => "h:mm:ss",
        47 => "mm:ss.0",
        49 => "@",
        _ => return None,
    })
}

/// Splits a format code at its `;` outside quotes.
fn sections_of(code: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (i, c) in code.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' if !quoted => {
                out.push(&code[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&code[start..]);
    out
}

fn pieces_of(section: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut chars = section.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                    out.push(Piece::Lit(q));
                }
            }
            '\\' => out.extend(chars.next().map(Piece::Lit)),
            '_' => {
                chars.next();
                out.push(Piece::Lit(' '));
            }
            '*' => {
                chars.next();
            }
            '[' => {
                let inner: String = chars.by_ref().take_while(|c| *c != ']').collect();
                if let Some(currency) = inner.strip_prefix('$') {
                    out.extend(
                        currency
                            .split('-')
                            .next()
                            .unwrap_or("")
                            .chars()
                            .map(Piece::Lit),
                    );
                } else if inner.starts_with(['h', 'm', 's', 'H', 'M', 'S']) {
                    out.extend(inner.chars().take(1).map(Piece::Code));
                }
            }
            c => out.push(Piece::Code(c)),
        }
    }
    out
}

fn parse_section(text: &str) -> Section {
    let pieces = pieces_of(text);
    let code: String = pieces
        .iter()
        .filter_map(|p| match p {
            Piece::Code(c) => Some(c.to_ascii_lowercase()),
            Piece::Lit(_) => None,
        })
        .collect();
    if code.is_empty() && !pieces.iter().any(|p| matches!(p, Piece::Lit(_))) || code == "general" {
        return Section::General;
    }
    if code == "@" {
        return Section::Text;
    }
    if code.contains("general")
        || code.contains('e') && code.contains(['+', '-']) && code.contains('0')
    {
        return Section::General;
    }
    if code.contains(['y', 'd', 'h', 's']) || (code.contains('m') && !code.contains(['0', '#'])) {
        return Section::Date(pieces);
    }
    let is_digit = |p: &Piece| matches!(p, Piece::Code('0' | '#' | '?'));
    let (Some(first), Some(last)) = (
        pieces.iter().position(is_digit),
        pieces.iter().rposition(is_digit),
    ) else {
        // No digit placeholder: the text of the format is all there is to show.
        return Section::General;
    };
    let text_of = |range: &[Piece]| -> String {
        range
            .iter()
            .map(|p| match p {
                Piece::Lit(c) | Piece::Code(c) => *c,
            })
            .collect()
    };
    let run = &pieces[first..=last];
    let point = run.iter().position(|p| *p == Piece::Code('.'));
    let int_part = &run[..point.unwrap_or(run.len())];
    Section::Number {
        prefix: text_of(&pieces[..first]),
        suffix: text_of(&pieces[last + 1..]),
        decimals: point.map_or(0, |i| run[i..].iter().filter(|p| is_digit(p)).count()),
        int_min: int_part.iter().filter(|p| **p == Piece::Code('0')).count(),
        thousands: int_part.contains(&Piece::Code(',')),
        percent: pieces.contains(&Piece::Code('%')),
    }
}

pub fn parse_format(code: &str) -> NumFmt {
    let sections = sections_of(code);
    NumFmt {
        positive: parse_section(sections[0]),
        negative: sections.get(1).map(|s| parse_section(s)),
    }
}

fn group_thousands(digits: &str) -> String {
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('\u{a0}');
        }
        out.push(c);
    }
    out
}

/// Shortest text of a number, as Excel's General format: up to 15 significant digits.
fn general(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let magnitude = v.abs().log10().floor() as i32;
    if !(-9..15).contains(&magnitude) {
        return format!("{v:E}").replace('.', ",");
    }
    let decimals = (14 - magnitude).clamp(0, 20) as usize;
    let text = format!("{v:.decimals$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    };
    text.replace('.', ",")
}

const MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];
const MONTHS_SHORT: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];
const DAYS: [&str; 7] = [
    "dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi",
];
const DAYS_SHORT: [&str; 7] = ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."];

/// Year, month, day of a day count since 1970-01-01.
fn civil(days: i64) -> (i64, usize, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as usize;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn format_date(pieces: &[Piece], serial: f64) -> String {
    let mut day = serial.floor() as i64;
    let mut seconds = ((serial - serial.floor()) * 86_400.0).round() as i64;
    if seconds >= 86_400 {
        day += 1;
        seconds = 0;
    }
    // Excel counts a 29 February 1900 that never existed.
    let epoch_days = if day >= 61 {
        day - 25_569
    } else {
        day - 25_568
    };
    let (year, month, date) = civil(epoch_days);
    let weekday = (epoch_days + 4).rem_euclid(7) as usize;
    let (hour, minute, second) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    let letters: Vec<(usize, char)> = pieces
        .iter()
        .enumerate()
        .filter_map(|(i, p)| match p {
            Piece::Code(c) if c.is_ascii_alphabetic() => Some((i, c.to_ascii_lowercase())),
            _ => None,
        })
        .collect();
    let twelve = letters.iter().any(|(_, c)| *c == 'a') && {
        let word: String = letters.iter().map(|(_, c)| *c).collect();
        word.contains("am") || word.contains("a/p")
    };
    let mut out = String::new();
    let mut i = 0;
    let mut previous: Option<char> = None;
    while i < pieces.len() {
        match &pieces[i] {
            Piece::Lit(c) => {
                out.push(*c);
                i += 1;
            }
            Piece::Code(c) => {
                let lower = c.to_ascii_lowercase();
                if !lower.is_ascii_alphabetic() {
                    // Fraction digits of seconds are not shown.
                    if lower != '0' {
                        out.push(*c);
                    }
                    i += 1;
                    continue;
                }
                if lower == 'a' {
                    let rest: String = pieces[i..]
                        .iter()
                        .take(5)
                        .map(|p| match p {
                            Piece::Code(c) | Piece::Lit(c) => c.to_ascii_lowercase(),
                        })
                        .collect();
                    if rest == "am/pm" {
                        out.push_str(if hour < 12 { "AM" } else { "PM" });
                        i += 5;
                        continue;
                    }
                    if rest.starts_with("a/p") {
                        out.push(if hour < 12 { 'A' } else { 'P' });
                        i += 3;
                        continue;
                    }
                }
                let mut n = 1;
                while matches!(pieces.get(i + n), Some(Piece::Code(c)) if c.to_ascii_lowercase() == lower)
                {
                    n += 1;
                }
                let next_letter = letters.iter().find(|(at, _)| *at >= i + n).map(|(_, c)| *c);
                match lower {
                    'y' => {
                        if n >= 3 {
                            out += &format!("{year:04}");
                        } else {
                            out += &format!("{:02}", year.rem_euclid(100));
                        }
                    }
                    'd' => match n {
                        1 => out += &date.to_string(),
                        2 => out += &format!("{date:02}"),
                        3 => out += DAYS_SHORT[weekday],
                        _ => out += DAYS[weekday],
                    },
                    'h' => {
                        let h = if twelve { (hour + 11) % 12 + 1 } else { hour };
                        out += &if n >= 2 {
                            format!("{h:02}")
                        } else {
                            h.to_string()
                        };
                    }
                    's' => {
                        out += &if n >= 2 {
                            format!("{second:02}")
                        } else {
                            second.to_string()
                        }
                    }
                    'm' => {
                        if previous == Some('h') || next_letter == Some('s') {
                            out += &if n >= 2 {
                                format!("{minute:02}")
                            } else {
                                minute.to_string()
                            };
                        } else {
                            match n {
                                1 => out += &month.to_string(),
                                2 => out += &format!("{month:02}"),
                                3 => out += MONTHS_SHORT[month - 1],
                                4 => out += MONTHS[month - 1],
                                _ => out.extend(MONTHS[month - 1].chars().next()),
                            }
                        }
                    }
                    _ => {}
                }
                previous = Some(lower);
                i += n;
            }
        }
    }
    out
}

fn format_section(section: &Section, v: f64, signed: bool) -> String {
    match section {
        Section::General | Section::Text => general(v),
        Section::Date(pieces) => format_date(pieces, v),
        Section::Number {
            prefix,
            suffix,
            decimals,
            int_min,
            thousands,
            percent,
        } => {
            let scaled = v.abs() * if *percent { 100.0 } else { 1.0 };
            let text = format!("{scaled:.decimals$}");
            let (int, frac) = text.split_once('.').unwrap_or((&text, ""));
            let mut int = int.to_string();
            while int.len() < *int_min {
                int.insert(0, '0');
            }
            if *thousands {
                int = group_thousands(&int);
            }
            let sign = if signed && v < 0.0 && text.chars().any(|c| c.is_ascii_digit() && c != '0')
            {
                "-"
            } else {
                ""
            };
            let frac = if frac.is_empty() {
                String::new()
            } else {
                format!(",{frac}")
            };
            format!("{sign}{prefix}{int}{frac}{suffix}")
        }
    }
}

pub fn format_value(fmt: &NumFmt, v: f64) -> String {
    match (&fmt.negative, v < 0.0) {
        (Some(negative), true) => format_section(negative, -v, false),
        _ => format_section(&fmt.positive, v, true),
    }
}

// ---- Model ------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct CellStyle {
    pub bold: bool,
    pub italic: bool,
    pub fill: Option<Color32>,
    pub color: Option<Color32>,
    pub align: Option<Align>,
    fmt: NumFmt,
}

pub struct Cell {
    pub text: String,
    pub align: Align,
    pub style: usize,
}

#[derive(Default)]
pub struct Sheet {
    pub name: String,
    pub cells: HashMap<(u32, u32), Cell>,
    pub rows: u32,
    pub cols: u32,
    /// Width of each column in pixels, 0 for a hidden one.
    pub widths: Vec<f32>,
    pub truncated: bool,
}

pub struct Workbook {
    pub sheets: Vec<Sheet>,
    pub styles: Vec<CellStyle>,
}

impl Workbook {
    pub fn style(&self, cell: &Cell) -> &CellStyle {
        &self.styles[cell.style.min(self.styles.len() - 1)]
    }
    pub fn cell_count(&self) -> usize {
        self.sheets.iter().map(|s| s.cells.len()).sum()
    }
}

/// Column letters to a 0-based index, and the 0-based row of a reference such as `AB12`.
fn cell_ref(text: &str) -> Option<(u32, u32)> {
    let letters = text.chars().take_while(char::is_ascii_alphabetic).count();
    let (col, row) = text.split_at(letters);
    if col.is_empty() || col.len() > 3 {
        return None;
    }
    let col = col.bytes().fold(0u32, |acc, b| {
        acc * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1)
    });
    let row: u32 = row.parse().ok()?;
    Some((row.checked_sub(1)?, col - 1))
}

pub fn column_name(mut index: u32) -> String {
    let mut name = String::new();
    loop {
        name.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            return name;
        }
        index = index / 26 - 1;
    }
}

fn attrs(e: &BytesStart) -> Vec<(String, String)> {
    e.attributes()
        .flatten()
        .filter_map(|a| Some((local(a.key.as_ref()), a.unescape_value().ok()?.into_owned())))
        .collect()
}

fn get<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn argb(text: &str) -> Option<Color32> {
    text.get(text.len().checked_sub(6)?..).and_then(hex_color)
}

fn theme_colors(root: &Node) -> Vec<Color32> {
    let Some(scheme) = root.find("clrScheme") else {
        return Vec::new();
    };
    let color = |name: &str| {
        let node = scheme.child(name)?.children.first()?;
        node.attr("val")
            .filter(|_| node.name == "srgbClr")
            .or_else(|| node.attr("lastClr"))
            .and_then(hex_color)
    };
    // The order Excel numbers theme colors in.
    [
        "lt1", "dk1", "lt2", "dk2", "accent1", "accent2", "accent3", "accent4", "accent5",
        "accent6", "hlink", "folHlink",
    ]
    .iter()
    .map(|n| color(n).unwrap_or(Color32::BLACK))
    .collect()
}

fn excel_color(node: &Node, theme: &[Color32]) -> Option<Color32> {
    if let Some(rgb) = node.attr("rgb") {
        return argb(rgb);
    }
    if let Some(index) = node.attr("theme").and_then(|t| t.parse::<usize>().ok()) {
        let base = *theme.get(index)?;
        let tint: f32 = node
            .attr("tint")
            .and_then(|t| t.parse().ok())
            .unwrap_or(0.0);
        return Some(if tint < 0.0 {
            lum(base, 1.0 + tint, 0.0)
        } else if tint > 0.0 {
            lum(base, 1.0 - tint, tint)
        } else {
            base
        });
    }
    match node.attr("indexed")?.parse::<u32>().ok()? {
        8 => Some(Color32::BLACK),
        9 => Some(Color32::WHITE),
        10 => Some(Color32::RED),
        11 => Some(Color32::from_rgb(0, 255, 0)),
        12 => Some(Color32::BLUE),
        13 => Some(Color32::YELLOW),
        _ => None,
    }
}

fn read_styles(root: &Node, theme: &[Color32]) -> Vec<CellStyle> {
    let mut codes: HashMap<u32, String> = HashMap::new();
    if let Some(formats) = root.child("numFmts") {
        for f in formats.children.iter().filter(|c| c.name == "numFmt") {
            if let (Some(id), Some(code)) = (
                f.attr("numFmtId").and_then(|i| i.parse().ok()),
                f.attr("formatCode"),
            ) {
                codes.insert(id, code.to_string());
            }
        }
    }
    let fonts: Vec<(bool, bool, Option<Color32>)> = root
        .child("fonts")
        .map(|f| {
            f.children
                .iter()
                .filter(|c| c.name == "font")
                .map(|font| {
                    let on =
                        |name: &str| font.child(name).is_some_and(|n| n.attr("val") != Some("0"));
                    (
                        on("b"),
                        on("i"),
                        font.child("color").and_then(|c| excel_color(c, theme)),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let fills: Vec<Option<Color32>> = root
        .child("fills")
        .map(|f| {
            f.children
                .iter()
                .filter(|c| c.name == "fill")
                .map(|fill| {
                    let pattern = fill.child("patternFill")?;
                    if pattern.attr("patternType") == Some("none") {
                        return None;
                    }
                    pattern.child("fgColor").and_then(|c| excel_color(c, theme))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut styles: Vec<CellStyle> = root
        .child("cellXfs")
        .map(|xfs| {
            xfs.children
                .iter()
                .filter(|c| c.name == "xf")
                .map(|xf| {
                    let index = |key: &str| {
                        xf.attr(key)
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0)
                    };
                    let id: u32 = xf
                        .attr("numFmtId")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let code = codes
                        .get(&id)
                        .map(String::as_str)
                        .or_else(|| builtin_code(id));
                    let (bold, italic, color) =
                        fonts.get(index("fontId")).copied().unwrap_or_default();
                    CellStyle {
                        bold,
                        italic,
                        fill: fills.get(index("fillId")).copied().flatten(),
                        // Black is the default: only other colors are worth keeping.
                        color: color.filter(|c| *c != Color32::BLACK),
                        align: match xf.child("alignment").and_then(|a| a.attr("horizontal")) {
                            Some("left") => Some(Align::Min),
                            Some("center" | "centerContinuous") => Some(Align::Center),
                            Some("right") => Some(Align::Max),
                            _ => None,
                        },
                        fmt: code.map(parse_format).unwrap_or_default(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    if styles.is_empty() {
        styles.push(CellStyle::default());
    }
    styles
}

fn read_shared_strings(bytes: &[u8]) -> Result<Vec<String>> {
    let mut reader = Reader::from_reader(bytes);
    let mut buf = Vec::new();
    let mut strings = Vec::new();
    let (mut current, mut in_t, mut phonetic) = (String::new(), false, 0usize);
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => match local(e.name().as_ref()).as_str() {
                "si" => current.clear(),
                "t" => in_t = true,
                "rPh" => phonetic += 1,
                _ => {}
            },
            Event::Empty(e) if local(e.name().as_ref()) == "si" => strings.push(String::new()),
            Event::End(e) => match local(e.name().as_ref()).as_str() {
                "si" => strings.push(std::mem::take(&mut current)),
                "t" => in_t = false,
                "rPh" => phonetic = phonetic.saturating_sub(1),
                _ => {}
            },
            Event::Text(t) if in_t && phonetic == 0 => current.push_str(&t.unescape()?),
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(strings)
}

fn parse_sheet(
    name: String,
    bytes: &[u8],
    strings: &[String],
    styles: &[CellStyle],
) -> Result<Sheet> {
    let mut sheet = Sheet {
        name,
        ..Sheet::default()
    };
    let mut reader = Reader::from_reader(bytes);
    let mut buf = Vec::new();
    let mut row: i64 = -1;
    let mut next_col = 0u32;
    // (position, type, style, value, inline text) of the cell being read.
    let mut cell: Option<((u32, u32), String, usize, String, String)> = None;
    let (mut in_v, mut in_t, mut phonetic) = (false, false, 0usize);
    let mut column_defs: Vec<(u32, u32, f32, bool)> = Vec::new();

    let finish = |sheet: &mut Sheet, cell: ((u32, u32), String, usize, String, String)| {
        let ((r, c), kind, style_index, value, inline) = cell;
        if c >= MAX_COLS {
            return;
        }
        let style = styles.get(style_index).or(styles.first());
        let mut align = Align::Min;
        let text = match kind.as_str() {
            "s" => value
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|i| strings.get(i))
                .cloned()
                .unwrap_or_default(),
            "inlineStr" => {
                if inline.is_empty() {
                    value
                } else {
                    inline
                }
            }
            "str" | "d" => value,
            "e" => {
                align = Align::Center;
                value
            }
            "b" => {
                align = Align::Center;
                if value.trim() == "1" {
                    "VRAI".into()
                } else {
                    "FAUX".into()
                }
            }
            _ => match value.trim().parse::<f64>() {
                Ok(v) => {
                    align = Align::Max;
                    style.map_or_else(|| general(v), |s| format_value(&s.fmt, v))
                }
                Err(_) => value,
            },
        };
        if let Some(forced) = style.and_then(|s| s.align) {
            align = forced;
        }
        if text.is_empty() && style.is_none_or(|s| s.fill.is_none()) {
            return;
        }
        if sheet.cells.len() >= MAX_CELLS {
            sheet.truncated = true;
            return;
        }
        sheet.rows = sheet.rows.max(r + 1);
        sheet.cols = sheet.cols.max(c + 1);
        sheet.cells.insert(
            (r, c),
            Cell {
                text,
                align,
                style: style_index,
            },
        );
    };

    loop {
        let event = reader.read_event_into(&mut buf)?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(event, Event::Empty(_));
                let a = attrs(e);
                match local(e.name().as_ref()).as_str() {
                    "row" => {
                        row = get(&a, "r")
                            .and_then(|r| r.parse::<i64>().ok())
                            .map_or(row + 1, |r| r - 1);
                        next_col = 0;
                    }
                    "c" => {
                        let at = get(&a, "r")
                            .and_then(cell_ref)
                            .unwrap_or((row.max(0) as u32, next_col));
                        next_col = at.1 + 1;
                        let item = (
                            at,
                            get(&a, "t").unwrap_or("n").to_string(),
                            get(&a, "s").and_then(|s| s.parse().ok()).unwrap_or(0),
                            String::new(),
                            String::new(),
                        );
                        if empty {
                            finish(&mut sheet, item);
                        } else {
                            cell = Some(item);
                        }
                    }
                    "v" if !empty => in_v = true,
                    "t" if !empty && cell.is_some() && phonetic == 0 => in_t = true,
                    "rPh" if !empty => phonetic += 1,
                    "col" => {
                        let min = get(&a, "min").and_then(|v| v.parse::<u32>().ok());
                        let max = get(&a, "max").and_then(|v| v.parse::<u32>().ok());
                        if let (Some(min), Some(max)) = (min, max) {
                            let width = get(&a, "width").and_then(|w| w.parse::<f32>().ok());
                            column_defs.push((
                                min.saturating_sub(1),
                                max.saturating_sub(1).min(MAX_COLS - 1),
                                width.map_or(DEFAULT_WIDTH, |w| w * 7.0 + 5.0),
                                matches!(get(&a, "hidden"), Some("1" | "true")),
                            ));
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => match local(e.name().as_ref()).as_str() {
                "c" => {
                    if let Some(item) = cell.take() {
                        finish(&mut sheet, item);
                    }
                }
                "v" => in_v = false,
                "t" => in_t = false,
                "rPh" => phonetic = phonetic.saturating_sub(1),
                _ => {}
            },
            Event::Text(t) => {
                if let Some(item) = cell.as_mut() {
                    if in_v {
                        item.3.push_str(&t.unescape()?);
                    } else if in_t {
                        item.4.push_str(&t.unescape()?);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    sheet.widths = vec![DEFAULT_WIDTH; sheet.cols as usize];
    for (min, max, width, hidden) in column_defs {
        for c in min..=max.min(sheet.cols.saturating_sub(1)) {
            if let Some(w) = sheet.widths.get_mut(c as usize) {
                *w = if hidden { 0.0 } else { width };
            }
        }
    }
    Ok(sheet)
}

pub fn load(path: &Path) -> Result<Workbook> {
    if fs::metadata(path)?.len() > MAX_FILE {
        bail!("Classeur trop volumineux (limite : 128 Mo)");
    }
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xls"))
    {
        bail!(
            "Le format binaire .xls n'est pas pris en charge : enregistrez le classeur au format .xlsx"
        );
    }
    let file = fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|_| {
        anyhow!(
            "Ce fichier n'est pas un classeur Excel valide (ou il est protégé par un mot de passe)"
        )
    })?;
    let workbook = read_part(&mut zip, "xl/workbook.xml")?.ok_or_else(|| {
        anyhow!("Ce fichier n'est pas un classeur Excel : xl/workbook.xml est absent")
    })?;
    let workbook = parse_xml(&workbook).context("xl/workbook.xml")?;
    let mut targets: HashMap<String, String> = HashMap::new();
    if let Some(bytes) = read_part(&mut zip, "xl/_rels/workbook.xml.rels")? {
        let rels = parse_xml(&bytes).context("relations du classeur")?;
        for rel in rels.children.iter().filter(|c| c.name == "Relationship") {
            if let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target")) {
                let full = match target.strip_prefix('/') {
                    Some(absolute) => absolute.to_string(),
                    None => format!("xl/{target}"),
                };
                targets.insert(id.to_string(), full);
            }
        }
    }
    let theme = read_part(&mut zip, "xl/theme/theme1.xml")?
        .and_then(|b| parse_xml(&b).ok())
        .map(|r| theme_colors(&r))
        .unwrap_or_default();
    let styles = read_part(&mut zip, "xl/styles.xml")?
        .and_then(|b| parse_xml(&b).ok())
        .map(|r| read_styles(&r, &theme))
        .unwrap_or_else(|| vec![CellStyle::default()]);
    let strings = match read_part(&mut zip, "xl/sharedStrings.xml")? {
        Some(bytes) => read_shared_strings(&bytes).unwrap_or_default(),
        None => Vec::new(),
    };
    let mut sheets = Vec::new();
    let listed = workbook
        .child("sheets")
        .map(|s| s.children.as_slice())
        .unwrap_or_default();
    for entry in listed.iter().filter(|c| c.name == "sheet") {
        if matches!(entry.attr("state"), Some("hidden" | "veryHidden")) {
            continue;
        }
        let name = entry.attr("name").unwrap_or("Feuille").to_string();
        let Some(target) = entry.attr("id").and_then(|id| targets.get(id)) else {
            continue;
        };
        let Some(bytes) = read_part(&mut zip, target)? else {
            continue;
        };
        sheets.push(
            parse_sheet(name.clone(), &bytes, &strings, &styles)
                .with_context(|| format!("feuille « {name} »"))?,
        );
    }
    if sheets.is_empty() {
        bail!("Ce classeur ne contient aucune feuille visible");
    }
    Ok(Workbook { sheets, styles })
}

// ---- Viewer -----------------------------------------------------------------------------

#[derive(Default)]
pub struct SheetView {
    pub active: usize,
}

fn paint_cell(ui: &mut egui::Ui, fill: Color32, text: RichText, align: Align) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter()
        .rect_stroke(rect, 0.0, Stroke::new(1.0_f32, GRID), StrokeKind::Inside);
    let layout = match align {
        Align::Min => Layout::left_to_right(Align::Center),
        Align::Max => Layout::right_to_left(Align::Center),
        Align::Center => Layout::centered_and_justified(egui::Direction::LeftToRight),
    };
    ui.with_layout(layout, |ui| {
        if align != Align::Center {
            ui.add_space(4.0);
        }
        ui.add(egui::Label::new(text).truncate());
    });
}

pub fn show(ui: &mut egui::Ui, wb: &Workbook, state: &mut SheetView, path: &Path, zoom: f32) {
    let t = ui::theme(ui.ctx());
    state.active = state.active.min(wb.sheets.len() - 1);
    egui::ScrollArea::horizontal()
        .id_salt(("xlsx_tabs", path))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                for (i, sheet) in wb.sheets.iter().enumerate() {
                    if ui
                        .selectable_label(i == state.active, &sheet.name)
                        .clicked()
                    {
                        state.active = i;
                    }
                }
            });
        });
    ui.add_space(4.0);
    let sheet = &wb.sheets[state.active];
    if sheet.rows == 0 {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| ui.label(RichText::new("Feuille vide").color(t.text_secondary)));
        return;
    }
    if sheet.truncated {
        ui.label(
            RichText::new("Feuille tronquée : trop de cellules pour tout afficher")
                .small()
                .color(t.error),
        );
    }
    let columns: Vec<u32> = (0..sheet.cols)
        .filter(|c| sheet.widths[*c as usize] > 0.0)
        .collect();
    let row_h = 20.0 * zoom;
    let size = 13.0 * zoom;
    ui.push_id(("xlsx_grid", path, state.active), |ui| {
        egui::ScrollArea::horizontal()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let mut table = TableBuilder::new(ui)
                    .resizable(true)
                    .vscroll(true)
                    .auto_shrink([false, false])
                    .column(Column::exact(48.0 * zoom));
                for c in &columns {
                    table = table.column(
                        Column::initial(sheet.widths[*c as usize] * zoom)
                            .at_least(24.0)
                            .clip(true),
                    );
                }
                table
                    .header(row_h, |mut header| {
                        header.col(|ui| paint_cell(ui, HEADER_FILL, RichText::new(""), Align::Min));
                        for c in &columns {
                            header.col(|ui| {
                                let text = RichText::new(column_name(*c))
                                    .font(ui::semibold(size))
                                    .color(INK);
                                paint_cell(ui, HEADER_FILL, text, Align::Center);
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(row_h, sheet.rows as usize, |mut row| {
                            let r = row.index() as u32;
                            row.col(|ui| {
                                let text = RichText::new((r + 1).to_string())
                                    .size(size * 0.9)
                                    .color(INK);
                                paint_cell(ui, HEADER_FILL, text, Align::Center);
                            });
                            for c in &columns {
                                row.col(|ui| match sheet.cells.get(&(r, *c)) {
                                    Some(cell) => {
                                        let style = wb.style(cell);
                                        let mut text =
                                            RichText::new(cell.text.lines().next().unwrap_or(""))
                                                .font(if style.bold {
                                                    ui::semibold(size)
                                                } else {
                                                    FontId::proportional(size)
                                                })
                                                .color(style.color.unwrap_or(INK));
                                        if style.italic {
                                            text = text.italics();
                                        }
                                        paint_cell(
                                            ui,
                                            style.fill.unwrap_or(Color32::WHITE),
                                            text,
                                            cell.align,
                                        );
                                    }
                                    None => paint_cell(
                                        ui,
                                        Color32::WHITE,
                                        RichText::new(""),
                                        Align::Min,
                                    ),
                                });
                            }
                        });
                    });
            });
    });
}

// ---- PDF --------------------------------------------------------------------------------

const PAGE_W: f32 = 842.0;
const PAGE_H: f32 = 595.0;
const MARGIN: f32 = 32.0;
const ROW_H: f32 = 14.0;
const FONT_SIZE: f32 = 8.5;
const MAX_PAGES: usize = 2000;

pub fn export_pdf(wb: &Workbook, target: &Path) -> Result<()> {
    let mut pdf = Pdf::default();
    let usable_w = PAGE_W - 2.0 * MARGIN;
    let title_h = 18.0;
    let rows_per_page = (((PAGE_H - 2.0 * MARGIN - title_h) / ROW_H) as usize)
        .saturating_sub(1)
        .max(1);
    let number_w = 28.0;
    'sheets: for sheet in &wb.sheets {
        let columns: Vec<(u32, f32)> = (0..sheet.cols)
            .filter(|c| sheet.widths[*c as usize] > 0.0)
            .map(|c| {
                (
                    c,
                    (sheet.widths[c as usize] * 0.75).clamp(16.0, usable_w - number_w),
                )
            })
            .collect();
        // Columns that fit side by side on one page.
        let mut bands: Vec<Vec<(u32, f32)>> = Vec::new();
        let mut used = number_w;
        for column in columns {
            if bands.is_empty() || used + column.1 > usable_w {
                bands.push(Vec::new());
                used = number_w;
            }
            used += column.1;
            bands.last_mut().expect("band").push(column);
        }
        if bands.is_empty() {
            bands.push(Vec::new());
        }
        let total_bands = bands.len();
        for (b, band) in bands.iter().enumerate() {
            let mut first = 0u32;
            loop {
                if pdf.page_count() >= MAX_PAGES {
                    break 'sheets;
                }
                pdf.page(PAGE_W, PAGE_H);
                let label = if total_bands > 1 {
                    format!("{} (colonnes {}/{})", sheet.name, b + 1, total_bands)
                } else {
                    sheet.name.clone()
                };
                pdf.text(
                    MARGIN,
                    MARGIN + 10.0,
                    Font::Bold,
                    11.0,
                    INK,
                    &fit(Font::Bold, 11.0, &label, usable_w),
                );
                let mut y = MARGIN + title_h;
                let width: f32 = number_w + band.iter().map(|c| c.1).sum::<f32>();
                // Header with the column letters.
                pdf.fill_rect(MARGIN, y, width, ROW_H, HEADER_FILL);
                let mut x = MARGIN + number_w;
                for (c, w) in band {
                    let name = column_name(*c);
                    let at = x + (w - text_width(Font::Bold, FONT_SIZE, &name)) / 2.0;
                    pdf.text(at, y + ROW_H - 4.0, Font::Bold, FONT_SIZE, INK, &name);
                    x += w;
                }
                pdf.stroke_rect(MARGIN, y, width, ROW_H, GRID, 0.5);
                y += ROW_H;
                let last = (first + rows_per_page as u32).min(sheet.rows);
                for r in first..last {
                    pdf.fill_rect(MARGIN, y, number_w, ROW_H, HEADER_FILL);
                    let number = (r + 1).to_string();
                    let at =
                        MARGIN + (number_w - text_width(Font::Regular, FONT_SIZE, &number)) / 2.0;
                    pdf.text(at, y + ROW_H - 4.0, Font::Regular, FONT_SIZE, INK, &number);
                    pdf.stroke_rect(MARGIN, y, number_w, ROW_H, GRID, 0.5);
                    let mut x = MARGIN + number_w;
                    for (c, w) in band {
                        if let Some(cell) = sheet.cells.get(&(r, *c)) {
                            let style = wb.style(cell);
                            if let Some(fill) = style.fill {
                                pdf.fill_rect(x, y, *w, ROW_H, fill);
                            }
                            let font = Font::of(style.bold, style.italic);
                            let text = fit(
                                font,
                                FONT_SIZE,
                                cell.text.lines().next().unwrap_or(""),
                                w - 6.0,
                            );
                            let text_w = text_width(font, FONT_SIZE, &text);
                            let at = match cell.align {
                                Align::Min => x + 3.0,
                                Align::Center => x + (w - text_w) / 2.0,
                                Align::Max => x + w - 3.0 - text_w,
                            };
                            pdf.text(
                                at,
                                y + ROW_H - 4.0,
                                font,
                                FONT_SIZE,
                                style.color.unwrap_or(INK),
                                &text,
                            );
                        }
                        pdf.stroke_rect(x, y, *w, ROW_H, GRID, 0.5);
                        x += w;
                    }
                    y += ROW_H;
                }
                first = last;
                if first >= sheet.rows {
                    break;
                }
            }
        }
    }
    if pdf.page_count() == 0 {
        pdf.page(PAGE_W, PAGE_H);
    }
    fs::write(target, pdf.finish())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const WORKBOOK: &str = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Ventes" sheetId="1" r:id="rId1"/><sheet name="Cachée" sheetId="2" state="hidden" r:id="rId2"/></sheets></workbook>"#;

    const RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="x/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="x/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#;

    const STRINGS: &str = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<si><t>Produit</t></si><si><r><t>Prix </t></r><r><t>HT</t></r></si><si><t>Café &amp; thé</t></si></sst>"#;

    const STYLES: &str = r##"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<numFmts count="1"><numFmt numFmtId="164" formatCode="#,##0.00\ &quot;€&quot;"/></numFmts>
<fonts count="2"><font><sz val="11"/></font><font><b/><color rgb="FFFF0000"/></font></fonts>
<fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>
<fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/></patternFill></fill></fills>
<cellXfs count="4">
<xf numFmtId="0" fontId="0" fillId="0"/>
<xf numFmtId="164" fontId="0" fillId="0"/>
<xf numFmtId="14" fontId="0" fillId="0"/>
<xf numFmtId="0" fontId="1" fillId="2"><alignment horizontal="center"/></xf>
</cellXfs></styleSheet>"##;

    const SHEET: &str = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<cols><col min="1" max="1" width="20" customWidth="1"/><col min="3" max="3" width="10" hidden="1"/></cols>
<sheetData>
<row r="1"><c r="A1" t="s" s="3"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="inlineStr"><is><t>caché</t></is></c></row>
<row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2" s="1"><v>1234.5</v></c><c r="D2" s="2"><v>45000</v></c></row>
<row r="3"><c r="A3" t="b"><v>1</v></c><c r="B3"><f>B2*2</f><v>2469</v></c><c r="D3" t="e"><v>#DIV/0!</v></c></row>
</sheetData></worksheet>"#;

    fn sample(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("atelier-xlsx-{}-{name}.xlsx", std::process::id()));
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in [
            ("xl/workbook.xml", WORKBOOK),
            ("xl/_rels/workbook.xml.rels", RELS),
            ("xl/sharedStrings.xml", STRINGS),
            ("xl/styles.xml", STYLES),
            ("xl/worksheets/sheet1.xml", SHEET),
            ("xl/worksheets/sheet2.xml", "<worksheet/>"),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(data.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn reads_values_styles_and_widths() {
        let path = sample("read");
        let wb = load(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(wb.sheets.len(), 1, "the hidden sheet is skipped");
        let sheet = &wb.sheets[0];
        assert_eq!((sheet.rows, sheet.cols), (3, 4));
        let text = |r, c| sheet.cells[&(r, c)].text.as_str();
        assert_eq!(text(0, 0), "Produit");
        assert_eq!(text(0, 1), "Prix HT");
        assert_eq!(text(0, 2), "caché");
        assert_eq!(text(1, 0), "Café & thé");
        assert_eq!(text(1, 1), "1\u{a0}234,50 €");
        assert_eq!(text(1, 3), "15/03/2023");
        assert_eq!(text(2, 0), "VRAI");
        assert_eq!(text(2, 1), "2469");
        assert_eq!(text(2, 3), "#DIV/0!");
        let header = wb.style(&sheet.cells[&(0, 0)]);
        assert!(header.bold);
        assert_eq!(header.fill, Some(Color32::from_rgb(255, 255, 0)));
        assert_eq!(header.color, Some(Color32::from_rgb(255, 0, 0)));
        assert_eq!(sheet.cells[&(0, 0)].align, Align::Center);
        assert_eq!(sheet.cells[&(1, 1)].align, Align::Max);
        assert_eq!(sheet.widths, [145.0, 64.0, 0.0, 64.0]);
    }

    #[test]
    fn formats_numbers_and_dates() {
        let f = |code: &str, v: f64| format_value(&parse_format(code), v);
        assert_eq!(f("General", 0.1 + 0.2), "0,3");
        assert_eq!(f("General", 42.0), "42");
        assert_eq!(f("0.00", 3.14159), "3,14");
        assert_eq!(f("0%", 0.256), "26%");
        assert_eq!(f("0.0%", 0.256), "25,6%");
        assert_eq!(f("#,##0", 1234567.0), "1\u{a0}234\u{a0}567");
        assert_eq!(f("000", 7.0), "007");
        assert_eq!(f("#,##0.00;(#,##0.00)", -5.5), "(5,50)");
        assert_eq!(f("0.00", -5.5), "-5,50");
        assert_eq!(f("[$€-40C] #,##0", 12.0), "€ 12");
        assert_eq!(f("yyyy-mm-dd", 45000.0), "2023-03-15");
        assert_eq!(f("dd mmmm yyyy", 45000.0), "15 mars 2023");
        assert_eq!(f("dddd d mmm", 45000.0), "mercredi 15 mars");
        assert_eq!(f("h:mm", 0.75), "18:00");
        assert_eq!(f("h:mm:ss AM/PM", 0.75), "6:00:00 PM");
        assert_eq!(f("dd/mm/yyyy hh:mm", 45000.5), "15/03/2023 12:00");
        assert_eq!(f("@", 5.0), "5");
        assert_eq!(cell_ref("AB12"), Some((11, 27)));
        assert_eq!(column_name(27), "AB");
    }

    #[test]
    fn rejects_other_files_and_exports_a_pdf() {
        let text =
            std::env::temp_dir().join(format!("atelier-xlsx-{}-text.xlsx", std::process::id()));
        fs::write(&text, "not a zip").unwrap();
        let error = format!("{:#}", load(&text).err().unwrap());
        let _ = fs::remove_file(&text);
        assert!(error.contains("pas un classeur"), "{error}");

        let path = sample("pdf");
        let wb = load(&path).unwrap();
        let _ = fs::remove_file(&path);
        let target = std::env::temp_dir().join(format!("atelier-xlsx-{}.pdf", std::process::id()));
        export_pdf(&wb, &target).unwrap();
        let bytes = fs::read(&target).unwrap();
        let _ = fs::remove_file(&target);
        assert!(bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn the_grid_renders() {
        let path = sample("render");
        let wb = load(&path).unwrap();
        let _ = fs::remove_file(&path);
        let ctx = egui::Context::default();
        ui::install(&ctx, ui::theme_by_index(0), 14.0);
        let mut state = SheetView::default();
        for _ in 0..2 {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    show(ui, &wb, &mut state, Path::new("a.xlsx"), 1.0)
                });
            });
        }
    }
}
