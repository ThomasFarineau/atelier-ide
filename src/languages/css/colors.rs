//! The colors written in a style sheet: where they are and which color they are, for the
//! swatches of the editor, and how to write a color picked for one, in the notation it had.

use super::lexer::{self, Dialect, Kind, Role};
use eframe::egui::Color32;

/// A color as CSS writes it: red, green, blue and alpha, not premultiplied.
pub type Rgba = [u8; 4];
use std::ops::Range;

/// How a color is written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Notation {
    Hex,
    Rgb,
    Hsl,
    /// A name (`red`): written as hexadecimal once changed.
    Named,
}

/// A color of the text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Swatch {
    /// The bytes of the whole color: `#fff`, `rgb(1, 2, 3)`.
    pub range: Range<usize>,
    pub rgba: Rgba,
    pub notation: Notation,
}

impl Swatch {
    /// The color to paint.
    pub fn color(&self) -> Color32 {
        let [r, g, b, a] = self.rgba;
        Color32::from_rgba_unmultiplied(r, g, b, a)
    }
}

/// The names of colors and their value.
const NAMED: &[(&str, u32)] = &[
    ("aqua", 0x00ffff),
    ("beige", 0xf5f5dc),
    ("black", 0x000000),
    ("blue", 0x0000ff),
    ("brown", 0xa52a2a),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkred", 0x8b0000),
    ("fuchsia", 0xff00ff),
    ("gold", 0xffd700),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("grey", 0x808080),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lightblue", 0xadd8e6),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lime", 0x00ff00),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("navy", 0x000080),
    ("olive", 0x808000),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("pink", 0xffc0cb),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("salmon", 0xfa8072),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
];

/// Past this size, a file shows no swatch.
const MAX_TEXT: usize = 1_000_000;

pub fn find(text: &str, dialect: Dialect) -> Vec<Swatch> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let (tokens, _) = lexer::tokens(text, dialect);
    let roles = lexer::roles(text, &tokens, dialect);
    let mut out = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let word = &text[token.range.clone()];
        match token.kind {
            Kind::Hash if roles[i] == Role::Number => {
                if let Some(rgba) = parse_hex(word) {
                    out.push(Swatch {
                        range: token.range.clone(),
                        rgba,
                        notation: Notation::Hex,
                    });
                }
            }
            Kind::Function => {
                let name = word.to_ascii_lowercase();
                let notation = match name.as_str() {
                    "rgb" | "rgba" => Notation::Rgb,
                    "hsl" | "hsla" => Notation::Hsl,
                    _ => continue,
                };
                let open = token.range.end;
                let Some(close) = text[open..].find(')').map(|c| open + c) else {
                    continue;
                };
                let inside = &text[open + 1..close];
                // A variable inside: the color is not known here.
                if inside.contains(['$', '@']) || inside.contains("var(") || inside.contains('\n') {
                    continue;
                }
                let rgba = if notation == Notation::Rgb {
                    parse_rgb(inside)
                } else {
                    parse_hsl(inside)
                };
                if let Some(rgba) = rgba {
                    out.push(Swatch {
                        range: token.range.start..close + 1,
                        rgba,
                        notation,
                    });
                }
            }
            Kind::Ident if roles[i] == Role::Text => {
                let lower = word.to_ascii_lowercase();
                if let Some((_, rgb)) = NAMED.iter().find(|(n, _)| *n == lower) {
                    out.push(Swatch {
                        range: token.range.clone(),
                        rgba: from_rgb(*rgb),
                        notation: Notation::Named,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

fn from_rgb(rgb: u32) -> Rgba {
    [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255]
}

pub fn parse_hex(hash: &str) -> Option<Rgba> {
    if !lexer::is_color(hash) {
        return None;
    }
    let digits = &hash[1..];
    let nibble = |i: usize| {
        u8::from_str_radix(&digits[i..i + 1], 16)
            .ok()
            .map(|n| n * 17)
    };
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    let (r, g, b, a) = match digits.len() {
        3 => (nibble(0)?, nibble(1)?, nibble(2)?, 255),
        4 => (nibble(0)?, nibble(1)?, nibble(2)?, nibble(3)?),
        6 => (byte(0)?, byte(2)?, byte(4)?, 255),
        8 => (byte(0)?, byte(2)?, byte(4)?, byte(6)?),
        _ => return None,
    };
    Some([r, g, b, a])
}

/// The arguments of a color function, separated by commas, spaces or `/`.
fn arguments(inside: &str) -> Vec<&str> {
    inside
        .split([',', ' ', '/', '\t'])
        .filter(|a| !a.is_empty())
        .collect()
}

/// An alpha: `0.5` or `50%`.
fn alpha(value: &str) -> Option<u8> {
    let a = match value.strip_suffix('%') {
        Some(p) => p.parse::<f32>().ok()? / 100.0,
        None => value.parse::<f32>().ok()?,
    };
    Some((a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn parse_rgb(inside: &str) -> Option<Rgba> {
    let args = arguments(inside);
    if !(3..=4).contains(&args.len()) {
        return None;
    }
    let channel = |value: &str| -> Option<u8> {
        let v = match value.strip_suffix('%') {
            Some(p) => p.parse::<f32>().ok()? * 2.55,
            None => value.parse::<f32>().ok()?,
        };
        Some(v.clamp(0.0, 255.0).round() as u8)
    };
    let a = match args.get(3) {
        Some(v) => alpha(v)?,
        None => 255,
    };
    Some([channel(args[0])?, channel(args[1])?, channel(args[2])?, a])
}

fn parse_hsl(inside: &str) -> Option<Rgba> {
    let args = arguments(inside);
    if !(3..=4).contains(&args.len()) {
        return None;
    }
    let hue = args[0].trim_end_matches("deg").parse::<f32>().ok()?;
    let percent = |v: &str| {
        v.trim_end_matches('%')
            .parse::<f32>()
            .ok()
            .map(|p| p / 100.0)
    };
    let (r, g, b) = hsl_to_rgb(hue, percent(args[1])?, percent(args[2])?);
    let a = match args.get(3) {
        Some(v) => alpha(v)?,
        None => 255,
    };
    Some([r, g, b, a])
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> (u8, u8, u8) {
    let (s, l) = (saturation.clamp(0.0, 1.0), lightness.clamp(0.0, 1.0));
    let h = hue.rem_euclid(360.0) / 60.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let byte = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (byte(r), byte(g), byte(b))
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, l)
}

/// An alpha written as CSS writes it: `0.5`, without needless decimals.
fn alpha_text(a: u8) -> String {
    let value = (a as f32 / 255.0 * 100.0).round() / 100.0;
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `color` written in `notation`.
pub fn write(rgba: Rgba, notation: Notation) -> String {
    let [r, g, b, a] = rgba;
    match notation {
        Notation::Hex | Notation::Named => {
            if a == 255 {
                format!("#{r:02x}{g:02x}{b:02x}")
            } else {
                format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
            }
        }
        Notation::Rgb if a == 255 => format!("rgb({r}, {g}, {b})"),
        Notation::Rgb => format!("rgba({r}, {g}, {b}, {})", alpha_text(a)),
        Notation::Hsl => {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            let (h, s, l) = (h.round(), (s * 100.0).round(), (l * 100.0).round());
            if a == 255 {
                format!("hsl({h}, {s}%, {l}%)")
            } else {
                format!("hsla({h}, {s}%, {l}%, {})", alpha_text(a))
            }
        }
    }
}
