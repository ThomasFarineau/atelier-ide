//! The gradients of CSS (`linear-gradient()`, `radial-gradient()`, `conic-gradient()` and
//! their `repeating-` forms): read from the text for the swatch and the editor of gradients,
//! the color at each point of them for the previews, and written back once edited.
//!
//! What the editor does not change is written back as it was read: a stop keeps the text of its
//! color and of its position until it is moved or picked again.

use super::colors::{self, Notation, Rgba};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Linear,
    Radial,
    Conic,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Radial => "radial",
            Self::Conic => "conic",
        }
    }
}

/// Where a stop is along the gradient.
#[derive(Clone, PartialEq, Debug)]
pub enum Position {
    /// A percentage, from 0 to 100 (and beyond).
    Percent(f32),
    /// Another length (`20px`, `2em`), kept as written: the previews place it as if it had no
    /// position.
    Other(String),
}

#[derive(Clone, PartialEq, Debug)]
pub struct Stop {
    pub color: Rgba,
    pub notation: Notation,
    /// The color as written; rewritten when the color changes.
    pub color_text: String,
    pub position: Option<Position>,
    /// A second position (`red 10% 20%`), kept as written.
    pub second: Option<String>,
}

impl Stop {
    pub fn new(color: Rgba, notation: Notation, position: f32) -> Self {
        Self {
            color,
            notation,
            color_text: colors::write(color, notation),
            position: Some(Position::Percent(position)),
            second: None,
        }
    }

    /// The color picked for the stop.
    pub fn set_color(&mut self, color: Rgba, notation: Notation) {
        self.color = color;
        self.notation = notation;
        self.color_text = colors::write(color, notation);
    }

    fn write(&self) -> String {
        let mut out = self.color_text.clone();
        match &self.position {
            Some(Position::Percent(p)) => out.push_str(&format!(" {}%", number(*p))),
            Some(Position::Other(text)) => {
                out.push(' ');
                out.push_str(text);
            }
            None => {}
        }
        if let Some(second) = &self.second {
            out.push(' ');
            out.push_str(second);
        }
        out
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Gradient {
    pub kind: Kind,
    pub repeating: bool,
    /// What comes before the stops: `90deg`, `to right`, `circle at center`, `from 45deg`.
    pub head: Option<String>,
    pub stops: Vec<Stop>,
}

/// A number written without needless decimals: `12.5`, `40`.
fn number(value: f32) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    let text = format!("{rounded:.1}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The arguments of a function, split at the commas outside parentheses.
fn split_arguments(inside: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in inside.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(inside[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(inside[start..].trim());
    out
}

/// A color as a stop writes it: hexadecimal, `rgb()`, `hsl()`, a name or `transparent`.
fn parse_color(text: &str) -> Option<(Rgba, Notation)> {
    if text.starts_with('#') {
        return colors::parse_hex(text).map(|c| (c, Notation::Hex));
    }
    let lower = text.to_ascii_lowercase();
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

/// A stop: `red`, `#fff 20%`, `rgb(0 0 0 / 50%) 10px 30px`.
fn parse_stop(argument: &str) -> Option<Stop> {
    // The color may hold spaces (`rgb(0 0 0)`): it ends at its closing parenthesis.
    let color_end = if let Some(open) = argument.find('(')
        && argument[..open].chars().all(|c| c.is_ascii_alphabetic())
    {
        argument.find(')')? + 1
    } else {
        argument.find(char::is_whitespace).unwrap_or(argument.len())
    };
    let color_text = &argument[..color_end];
    let (color, notation) = parse_color(color_text)?;
    let mut rest = argument[color_end..].split_whitespace();
    let position = rest.next().map(|p| match p.strip_suffix('%') {
        Some(n) => n
            .parse::<f32>()
            .map_or_else(|_| Position::Other(p.to_string()), Position::Percent),
        None => Position::Other(p.to_string()),
    });
    let second = rest.next().map(str::to_string);
    if rest.next().is_some() {
        return None;
    }
    Some(Stop {
        color,
        notation,
        color_text: color_text.to_string(),
        position,
        second,
    })
}

/// Reads a whole gradient, `linear-gradient(…)` included. None for what the editor cannot
/// show: a color it does not know (`var(--x)`, `currentColor`), an interpolation hint.
pub fn parse(text: &str) -> Option<Gradient> {
    let open = text.find('(')?;
    let name = text[..open].trim().to_ascii_lowercase();
    let (repeating, name) = match name.strip_prefix("repeating-") {
        Some(rest) => (true, rest),
        None => (false, name.as_str()),
    };
    let kind = match name {
        "linear-gradient" => Kind::Linear,
        "radial-gradient" => Kind::Radial,
        "conic-gradient" => Kind::Conic,
        _ => return None,
    };
    let inside = text[open + 1..].strip_suffix(')')?;
    let mut arguments = split_arguments(inside).into_iter().peekable();
    let head = match arguments.peek() {
        Some(first) if parse_stop(first).is_none() => arguments.next().map(str::to_string),
        _ => None,
    };
    let stops: Vec<Stop> = arguments.map(parse_stop).collect::<Option<_>>()?;
    if stops.len() < 2 {
        return None;
    }
    Some(Gradient {
        kind,
        repeating,
        head,
        stops,
    })
}

/// The gradients written in a text, with their ranges.
pub fn find(text: &str) -> Vec<(Range<usize>, Gradient)> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = lower[from..].find("-gradient(") {
        let name_end = from + found + "-gradient".len();
        // Back to the start of the name: `repeating-linear`.
        let start = lower[..name_end]
            .rfind(|c: char| !(c.is_ascii_alphabetic() || c == '-'))
            .map_or(0, |i| i + 1);
        // To the parenthesis that closes it.
        let mut depth = 0;
        let mut end = None;
        for (i, c) in text[name_end..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(name_end + i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { break };
        if let Some(gradient) = parse(&text[start..end]) {
            out.push((start..end, gradient));
        }
        from = end;
    }
    out
}

impl Gradient {
    pub fn write(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(head) = &self.head {
            parts.push(head.clone());
        }
        parts.extend(self.stops.iter().map(Stop::write));
        format!(
            "{}{}-gradient({})",
            if self.repeating { "repeating-" } else { "" },
            self.kind.name(),
            parts.join(", ")
        )
    }

    /// The direction of a linear gradient in degrees (0 to the top, clockwise), of a conic one
    /// where it starts; None when the head says it in a way the editor does not read.
    pub fn angle(&self) -> Option<f32> {
        let default = match self.kind {
            Kind::Linear => 180.0,
            Kind::Conic => 0.0,
            Kind::Radial => return None,
        };
        let Some(head) = self.head.as_deref() else {
            return Some(default);
        };
        let head = head.trim().to_ascii_lowercase();
        let value = match self.kind {
            Kind::Conic => head
                .strip_prefix("from ")?
                .split_whitespace()
                .next()?
                .to_string(),
            _ => head.clone(),
        };
        if let Some(sides) = value.strip_prefix("to ") {
            let mut words: Vec<&str> = sides.split_whitespace().collect();
            words.sort_unstable();
            return match words.as_slice() {
                ["top"] => Some(0.0),
                ["right"] => Some(90.0),
                ["bottom"] => Some(180.0),
                ["left"] => Some(270.0),
                ["right", "top"] => Some(45.0),
                ["bottom", "right"] => Some(135.0),
                ["bottom", "left"] => Some(225.0),
                ["left", "top"] => Some(315.0),
                _ => None,
            };
        }
        angle_value(&value)
    }

    /// Sets the direction (linear) or the start (conic), in degrees.
    pub fn set_angle(&mut self, degrees: f32) {
        let degrees = degrees.rem_euclid(360.0);
        match self.kind {
            Kind::Linear => self.head = Some(format!("{}deg", number(degrees))),
            Kind::Conic => {
                // `from 45deg at center`: the position stays.
                let at = self
                    .head
                    .as_deref()
                    .and_then(|h| h.find(" at ").map(|i| h[i..].to_string()))
                    .unwrap_or_default();
                self.head = Some(format!("from {}deg{at}", number(degrees)));
            }
            Kind::Radial => {}
        }
    }

    /// A radial gradient drawn as a circle rather than an ellipse.
    pub fn circle(&self) -> bool {
        self.head.as_deref().is_some_and(|h| {
            h.split_whitespace()
                .any(|w| w.eq_ignore_ascii_case("circle"))
        })
    }

    pub fn set_circle(&mut self, circle: bool) {
        if self.kind != Kind::Radial {
            return;
        }
        let (from, to) = if circle {
            ("ellipse", "circle")
        } else {
            ("circle", "ellipse")
        };
        let head = self.head.clone().unwrap_or_default();
        let mut words: Vec<String> = head.split_whitespace().map(str::to_string).collect();
        match words.iter().position(|w| w.eq_ignore_ascii_case(from)) {
            Some(i) => words[i] = to.into(),
            None if circle => words.insert(0, to.into()),
            None => {}
        }
        let head = words.join(" ");
        self.head = (!head.is_empty()).then_some(head);
    }

    /// The position of each stop from 0 to 1, as CSS places those that have none: the first at
    /// the start, the last at the end, the others evenly between their neighbors.
    pub fn positions(&self) -> Vec<f32> {
        let n = self.stops.len();
        let mut known: Vec<Option<f32>> = self
            .stops
            .iter()
            .map(|s| match s.position {
                Some(Position::Percent(p)) => Some(p / 100.0),
                _ => None,
            })
            .collect();
        if known[0].is_none() {
            known[0] = Some(0.0);
        }
        if known[n - 1].is_none() {
            known[n - 1] = Some(1.0);
        }
        // A stop is never before the one before it.
        let mut highest = f32::MIN;
        for value in known.iter_mut().flatten() {
            *value = value.max(highest);
            highest = *value;
        }
        let mut out = vec![0.0; n];
        let mut i = 0;
        while i < n {
            if let Some(value) = known[i] {
                out[i] = value;
                i += 1;
                continue;
            }
            let previous = out[i - 1];
            let next = (i..n).find(|&j| known[j].is_some()).unwrap_or(n - 1);
            let target = known[next].unwrap_or(1.0);
            let steps = (next - i + 1) as f32;
            for (k, j) in (i..next).enumerate() {
                out[j] = previous + (target - previous) * (k + 1) as f32 / steps;
            }
            i = next;
        }
        out
    }

    /// The color at `t` along the gradient (0 to 1 from the first stop to the last).
    pub fn sample(&self, t: f32) -> Rgba {
        let positions = self.positions();
        let first = positions[0];
        let last = *positions.last().unwrap_or(&1.0);
        let mut t = t;
        if self.repeating && last > first {
            t = first + (t - first).rem_euclid(last - first);
        }
        if t <= first {
            return self.stops[0].color;
        }
        for (i, window) in positions.windows(2).enumerate() {
            let (a, b) = (window[0], window[1]);
            if t <= b {
                let f = if b > a { (t - a) / (b - a) } else { 1.0 };
                return mix(self.stops[i].color, self.stops[i + 1].color, f);
            }
        }
        self.stops.last().map_or([0, 0, 0, 0], |s| s.color)
    }

    /// The color of the point (`x`, `y`) of a box `width` by `height`, from its top left.
    pub fn color_at(&self, x: f32, y: f32, width: f32, height: f32) -> Rgba {
        let (dx, dy) = (x - width / 2.0, y - height / 2.0);
        let t = match self.kind {
            Kind::Linear => {
                let a = self.angle().unwrap_or(180.0).to_radians();
                let (sin, cos) = a.sin_cos();
                let length = (width * sin).abs() + (height * cos).abs();
                (dx * sin - dy * cos) / length.max(1e-3) + 0.5
            }
            Kind::Radial => {
                let corner = (width * width + height * height).sqrt() / 2.0;
                if self.circle() {
                    (dx * dx + dy * dy).sqrt() / corner.max(1e-3)
                } else {
                    let (rx, ry) = (width / 2.0 * 2f32.sqrt(), height / 2.0 * 2f32.sqrt());
                    ((dx / rx.max(1e-3)).powi(2) + (dy / ry.max(1e-3)).powi(2)).sqrt()
                }
            }
            Kind::Conic => {
                let from = self.angle().unwrap_or(0.0);
                let a = dx.atan2(-dy).to_degrees();
                (a - from).rem_euclid(360.0) / 360.0
            }
        };
        self.sample(t)
    }
}

/// `45deg`, `0.25turn`, `1rad`, `50grad` in degrees.
fn angle_value(text: &str) -> Option<f32> {
    let text = text.trim();
    let (number, factor) = if let Some(n) = text.strip_suffix("deg") {
        (n, 1.0)
    } else if let Some(n) = text.strip_suffix("turn") {
        (n, 360.0)
    } else if let Some(n) = text.strip_suffix("grad") {
        (n, 0.9)
    } else if let Some(n) = text.strip_suffix("rad") {
        (n, 180.0 / std::f32::consts::PI)
    } else if text == "0" {
        ("0", 1.0)
    } else {
        return None;
    };
    number.parse::<f32>().ok().map(|n| n * factor)
}

/// Between two colors, as CSS mixes them: with their opacity, so that `transparent` does not
/// darken the other one.
pub fn mix(a: Rgba, b: Rgba, f: f32) -> Rgba {
    let f = f.clamp(0.0, 1.0);
    let (aa, ba) = (a[3] as f32 / 255.0, b[3] as f32 / 255.0);
    let alpha = aa + (ba - aa) * f;
    let mut out = [0u8; 4];
    for c in 0..3 {
        let premultiplied = a[c] as f32 * aa + (b[c] as f32 * ba - a[c] as f32 * aa) * f;
        out[c] = if alpha > 0.0 {
            (premultiplied / alpha).round().clamp(0.0, 255.0) as u8
        } else {
            (a[c] as f32 + (b[c] as f32 - a[c] as f32) * f).round() as u8
        };
    }
    out[3] = (alpha * 255.0).round() as u8;
    out
}

#[cfg(test)]
#[path = "../../../test/languages/css/gradient/tests.rs"]
mod tests;
