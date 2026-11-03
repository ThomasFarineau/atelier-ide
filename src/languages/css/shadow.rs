//! The shadows of CSS (`box-shadow`, `text-shadow`): read from the text for their swatch and
//! their editor, written back once edited. What the editor does not change keeps its text: the
//! unit of each length, the way the color is written.

use super::colors::{self, Notation, Rgba};
use std::ops::Range;

/// A length as written: `4px`, `0`, `0.5em`.
#[derive(Clone, PartialEq, Debug)]
pub struct Length {
    pub value: f32,
    pub unit: String,
}

impl Length {
    pub fn px(value: f32) -> Self {
        Self {
            value,
            unit: "px".into(),
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let split = text
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))
            .unwrap_or(text.len());
        let value: f32 = text[..split].parse().ok()?;
        let unit = text[split..].to_ascii_lowercase();
        let known = matches!(
            unit.as_str(),
            "" | "px" | "%" | "em" | "rem" | "pt" | "vw" | "vh" | "ch" | "ex" | "cm" | "mm" | "in"
        );
        // A number without a unit is a length only when it is 0.
        (known && (!unit.is_empty() || value == 0.0)).then_some(Self { value, unit })
    }

    /// The length in pixels, for the previews (an `em` counted as 16 pixels).
    pub fn pixels(&self) -> f32 {
        let factor = match self.unit.as_str() {
            "em" | "rem" => 16.0,
            "pt" => 4.0 / 3.0,
            "ch" | "ex" => 8.0,
            "cm" => 37.8,
            "mm" => 3.78,
            "in" => 96.0,
            _ => 1.0,
        };
        self.value * factor
    }

    /// A value set in the editor: in the unit it had, pixels for a bare `0`.
    pub fn set(&mut self, value: f32) {
        if self.unit.is_empty() && value != 0.0 {
            self.unit = "px".into();
        }
        // Pixels to the tenth, the larger units to the hundredth.
        let steps = if self.unit == "px" { 10.0 } else { 100.0 };
        self.value = (value * steps).round() / steps;
    }

    pub fn write(&self) -> String {
        let text = format!("{:.2}", self.value);
        let number = text.trim_end_matches('0').trim_end_matches('.');
        let number = if number == "-0" { "0" } else { number };
        if number == "0" {
            "0".into()
        } else {
            format!("{number}{}", self.unit)
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct ShadowColor {
    pub rgba: Rgba,
    pub notation: Notation,
    pub text: String,
}

/// One shadow of the list.
#[derive(Clone, PartialEq, Debug)]
pub struct Shadow {
    pub inset: bool,
    pub x: Length,
    pub y: Length,
    pub blur: Option<Length>,
    pub spread: Option<Length>,
    /// None: the color of the text (`currentColor`).
    pub color: Option<ShadowColor>,
    /// The color came before the lengths (`red 2px 2px`): it stays there.
    color_first: bool,
}

impl Shadow {
    pub fn new() -> Self {
        Self {
            inset: false,
            x: Length::px(0.0),
            y: Length::px(4.0),
            blur: Some(Length::px(12.0)),
            spread: None,
            color: Some(ShadowColor {
                rgba: [0, 0, 0, 64],
                notation: Notation::Rgb,
                text: colors::write([0, 0, 0, 64], Notation::Rgb),
            }),
            color_first: false,
        }
    }

    pub fn set_color(&mut self, rgba: Rgba, notation: Notation) {
        self.color = Some(ShadowColor {
            rgba,
            notation,
            text: colors::write(rgba, notation),
        });
    }

    /// The color to paint: black when the shadow takes the color of the text.
    pub fn rgba(&self) -> Rgba {
        self.color.as_ref().map_or([0, 0, 0, 255], |c| c.rgba)
    }

    fn parse(text: &str, allow_inset: bool) -> Option<Self> {
        let mut inset = false;
        let mut lengths: Vec<Length> = Vec::new();
        let mut color: Option<ShadowColor> = None;
        let mut color_first = false;
        for word in words(text) {
            if word.eq_ignore_ascii_case("inset") && allow_inset {
                inset = true;
            } else if let Some(length) = Length::parse(word) {
                lengths.push(length);
            } else if color.is_none() {
                let (rgba, notation) = parse_color(word)?;
                color_first = lengths.is_empty();
                color = Some(ShadowColor {
                    rgba,
                    notation,
                    text: word.to_string(),
                });
            } else {
                return None;
            }
        }
        let max = if allow_inset { 4 } else { 3 };
        if !(2..=max).contains(&lengths.len()) {
            return None;
        }
        let mut lengths = lengths.into_iter();
        Some(Self {
            inset,
            x: lengths.next()?,
            y: lengths.next()?,
            blur: lengths.next(),
            spread: lengths.next(),
            color,
            color_first,
        })
    }

    fn write(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.inset {
            parts.push("inset".into());
        }
        let color = self.color.as_ref().map(|c| c.text.clone());
        if self.color_first
            && let Some(color) = &color
        {
            parts.push(color.clone());
        }
        parts.push(self.x.write());
        parts.push(self.y.write());
        // A spread needs a blur before it.
        let blur = match (&self.blur, &self.spread) {
            (Some(blur), _) => Some(blur.write()),
            (None, Some(_)) => Some("0".into()),
            (None, None) => None,
        };
        parts.extend(blur);
        if let Some(spread) = &self.spread {
            parts.push(spread.write());
        }
        if !self.color_first {
            parts.extend(color);
        }
        parts.join(" ")
    }
}

impl Default for Shadow {
    fn default() -> Self {
        Self::new()
    }
}

/// The words of a shadow, a color function counted as one (`rgb(0 0 0 / 50%)`).
fn words(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(s) = start.take() {
                out.push(&text[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(&text[s..]);
    }
    out
}

fn parse_color(text: &str) -> Option<(Rgba, Notation)> {
    let lower = text.to_ascii_lowercase();
    if text.starts_with('#') {
        return colors::parse_hex(text).map(|c| (c, Notation::Hex));
    }
    if lower == "transparent" {
        return Some(([0, 0, 0, 0], Notation::Named));
    }
    if let Some(open) = lower.find('(')
        && lower.ends_with(')')
    {
        let inside = &text[open + 1..text.len() - 1];
        return match &lower[..open] {
            "rgb" | "rgba" => colors::parse_rgb(inside).map(|c| (c, Notation::Rgb)),
            "hsl" | "hsla" => colors::parse_hsl(inside).map(|c| (c, Notation::Hsl)),
            _ => None,
        };
    }
    colors::named(&lower).map(|c| (c, Notation::Named))
}

/// The value of a `box-shadow` or a `text-shadow`.
#[derive(Clone, PartialEq, Debug)]
pub struct Shadows {
    /// `text-shadow`: no spread, no inset.
    pub text: bool,
    pub list: Vec<Shadow>,
}

impl Shadows {
    pub fn parse(value: &str, text: bool) -> Option<Self> {
        let mut list = Vec::new();
        let mut depth = 0i32;
        let mut start = 0;
        let value = value.trim();
        for (i, c) in value.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                ',' if depth == 0 => {
                    list.push(Shadow::parse(value[start..i].trim(), !text)?);
                    start = i + 1;
                }
                _ => {}
            }
        }
        list.push(Shadow::parse(value[start..].trim(), !text)?);
        Some(Self { text, list })
    }

    pub fn write(&self) -> String {
        self.list
            .iter()
            .map(Shadow::write)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The shadows written in a text: the values of `box-shadow` and `text-shadow` in CSS (a
/// sheet, a `style` attribute), and of `boxShadow` and `textShadow` in JavaScript (`'…'`).
pub fn find(text: &str) -> Vec<(Range<usize>, Shadows)> {
    const NAMES: [&str; 5] = [
        "box-shadow",
        "-webkit-box-shadow",
        "text-shadow",
        "boxShadow",
        "textShadow",
    ];
    super::properties::values(text, &NAMES)
        .into_iter()
        .filter_map(|(range, name)| {
            let is_text = NAMES[name].starts_with("text");
            Shadows::parse(&text[range.clone()], is_text).map(|s| (range, s))
        })
        .collect()
}

#[cfg(test)]
#[path = "../../../test/languages/css/shadow/tests.rs"]
mod tests;
