//! `filter` (and `backdrop-filter`) and `transform`: lists of functions (`blur(4px)
//! brightness(1.2)`, `translate(10px, 0) rotate(45deg)`), read for their swatch and their
//! editor, written back once edited. A function the editor does not know (`drop-shadow()`,
//! `matrix()`, the 3D ones) is kept as it was written.

use super::colors::Rgba;
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EffectKind {
    Filter,
    Transform,
}

/// A number and its unit, as written: `4px`, `1.2`, `45deg`, `50%`.
#[derive(Clone, PartialEq, Debug)]
pub struct Amount {
    pub value: f32,
    pub unit: String,
}

impl Amount {
    pub fn new(value: f32, unit: &str) -> Self {
        Self {
            value,
            unit: unit.into(),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        let split = text
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))
            .unwrap_or(text.len());
        let value: f32 = text[..split].parse().ok()?;
        let unit = text[split..].to_ascii_lowercase();
        let known = matches!(
            unit.as_str(),
            "" | "px" | "%" | "em" | "rem" | "deg" | "turn" | "rad" | "grad" | "vw" | "vh"
        );
        known.then_some(Self { value, unit })
    }

    pub fn write(&self) -> String {
        let text = format!("{:.2}", (self.value * 100.0).round() / 100.0);
        let number = text.trim_end_matches('0').trim_end_matches('.');
        let number = if number == "-0" { "0" } else { number };
        format!("{number}{}", self.unit)
    }

    /// An angle in degrees.
    pub fn degrees(&self) -> f32 {
        match self.unit.as_str() {
            "turn" => self.value * 360.0,
            "rad" => self.value.to_degrees(),
            "grad" => self.value * 0.9,
            _ => self.value,
        }
    }

    /// A factor: `50%` is 0.5, `1.5` is 1.5.
    pub fn factor(&self) -> f32 {
        if self.unit == "%" {
            self.value / 100.0
        } else {
            self.value
        }
    }

    /// A length in pixels (an `em` counted as 16).
    pub fn pixels(&self) -> f32 {
        match self.unit.as_str() {
            "em" | "rem" => self.value * 16.0,
            _ => self.value,
        }
    }
}

/// One function of the list.
#[derive(Clone, PartialEq, Debug)]
pub struct Function {
    /// As written (`translateX`, `hue-rotate`).
    pub name: String,
    pub args: Vec<Amount>,
    /// A function the editor does not change: its whole text.
    pub raw: Option<String>,
}

impl Function {
    fn write(&self) -> String {
        match &self.raw {
            Some(raw) => raw.clone(),
            None => format!(
                "{}({})",
                self.name,
                self.args
                    .iter()
                    .map(Amount::write)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// What the editor knows of a function: the name of each argument, the range of its slider,
/// and the value a new one starts with.
pub struct Spec {
    pub name: &'static str,
    pub args: &'static [(&'static str, f32, f32)],
    pub unit: &'static str,
    pub default: f32,
}

pub const FILTERS: &[Spec] = &[
    Spec {
        name: "blur",
        args: &[("", 0.0, 40.0)],
        unit: "px",
        default: 4.0,
    },
    Spec {
        name: "brightness",
        args: &[("", 0.0, 3.0)],
        unit: "",
        default: 1.2,
    },
    Spec {
        name: "contrast",
        args: &[("", 0.0, 3.0)],
        unit: "",
        default: 1.2,
    },
    Spec {
        name: "grayscale",
        args: &[("", 0.0, 1.0)],
        unit: "",
        default: 1.0,
    },
    Spec {
        name: "hue-rotate",
        args: &[("", -180.0, 180.0)],
        unit: "deg",
        default: 90.0,
    },
    Spec {
        name: "invert",
        args: &[("", 0.0, 1.0)],
        unit: "",
        default: 1.0,
    },
    Spec {
        name: "opacity",
        args: &[("", 0.0, 1.0)],
        unit: "",
        default: 0.5,
    },
    Spec {
        name: "saturate",
        args: &[("", 0.0, 3.0)],
        unit: "",
        default: 2.0,
    },
    Spec {
        name: "sepia",
        args: &[("", 0.0, 1.0)],
        unit: "",
        default: 1.0,
    },
];

pub const TRANSFORMS: &[Spec] = &[
    Spec {
        name: "translate",
        args: &[("x", -200.0, 200.0), ("y", -200.0, 200.0)],
        unit: "px",
        default: 10.0,
    },
    Spec {
        name: "translateX",
        args: &[("x", -200.0, 200.0)],
        unit: "px",
        default: 10.0,
    },
    Spec {
        name: "translateY",
        args: &[("y", -200.0, 200.0)],
        unit: "px",
        default: 10.0,
    },
    Spec {
        name: "rotate",
        args: &[("", -360.0, 360.0)],
        unit: "deg",
        default: 15.0,
    },
    Spec {
        name: "scale",
        args: &[("x", 0.0, 3.0), ("y", 0.0, 3.0)],
        unit: "",
        default: 1.2,
    },
    Spec {
        name: "scaleX",
        args: &[("x", 0.0, 3.0)],
        unit: "",
        default: 1.2,
    },
    Spec {
        name: "scaleY",
        args: &[("y", 0.0, 3.0)],
        unit: "",
        default: 1.2,
    },
    Spec {
        name: "skew",
        args: &[("x", -80.0, 80.0), ("y", -80.0, 80.0)],
        unit: "deg",
        default: 10.0,
    },
    Spec {
        name: "skewX",
        args: &[("x", -80.0, 80.0)],
        unit: "deg",
        default: 10.0,
    },
    Spec {
        name: "skewY",
        args: &[("y", -80.0, 80.0)],
        unit: "deg",
        default: 10.0,
    },
];

impl EffectKind {
    pub fn specs(self) -> &'static [Spec] {
        match self {
            Self::Filter => FILTERS,
            Self::Transform => TRANSFORMS,
        }
    }
}

/// The spec of a function, by its name (`rotateZ` is a `rotate`).
pub fn spec(kind: EffectKind, name: &str) -> Option<&'static Spec> {
    let name = if name.eq_ignore_ascii_case("rotateZ") {
        "rotate"
    } else {
        name
    };
    kind.specs()
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
}

#[derive(Clone, PartialEq, Debug)]
pub struct Effects {
    pub kind: EffectKind,
    pub functions: Vec<Function>,
}

impl Effects {
    pub fn parse(value: &str, kind: EffectKind) -> Option<Self> {
        let mut functions = Vec::new();
        let mut rest = value.trim();
        while !rest.is_empty() {
            let open = rest.find('(')?;
            let name = rest[..open].trim();
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return None;
            }
            // The closing parenthesis, those inside (`drop-shadow(0 0 4px rgb(0 0 0))`) skipped.
            let mut depth = 0;
            let mut close = None;
            for (i, c) in rest[open..].char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(open + i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let close = close?;
            let inside = &rest[open + 1..close];
            let args: Option<Vec<Amount>> = inside
                .split([',', ' '])
                .filter(|a| !a.is_empty())
                .map(Amount::parse)
                .collect();
            let known = spec(kind, name).filter(|s| {
                args.as_ref()
                    .is_some_and(|a| !a.is_empty() && a.len() <= s.args.len())
            });
            functions.push(match (known, args) {
                (Some(_), Some(args)) => Function {
                    name: name.to_string(),
                    args,
                    raw: None,
                },
                _ => Function {
                    name: name.to_string(),
                    args: Vec::new(),
                    raw: Some(rest[..=close].to_string()),
                },
            });
            rest = rest[close + 1..].trim_start();
        }
        // Something the editor can change.
        functions
            .iter()
            .any(|f| f.raw.is_none())
            .then_some(Self { kind, functions })
    }

    /// The list written back; `none` once every function is taken away.
    pub fn write(&self) -> String {
        if self.functions.is_empty() {
            return "none".into();
        }
        self.functions
            .iter()
            .map(Function::write)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A function as the editor adds it.
    pub fn add(&mut self, spec: &Spec) {
        let args = match spec.name {
            "translate" | "skew" => vec![
                Amount::new(spec.default, spec.unit),
                Amount::new(0.0, spec.unit),
            ],
            _ => vec![Amount::new(spec.default, spec.unit)],
        };
        self.functions.push(Function {
            name: spec.name.into(),
            args,
            raw: None,
        });
    }

    /// The color a filter gives to `color` (its blur and shadows left out).
    pub fn filter(&self, color: Rgba) -> Rgba {
        let mut c = [
            color[0] as f32 / 255.0,
            color[1] as f32 / 255.0,
            color[2] as f32 / 255.0,
        ];
        let mut alpha = color[3] as f32 / 255.0;
        for function in self.functions.iter().filter(|f| f.raw.is_none()) {
            let amount = function.args.first();
            let a = amount.map_or(1.0, Amount::factor);
            match function.name.to_ascii_lowercase().as_str() {
                "brightness" => c = c.map(|v| v * a),
                "contrast" => c = c.map(|v| (v - 0.5) * a + 0.5),
                "invert" => c = c.map(|v| v + (1.0 - 2.0 * v) * a.min(1.0)),
                "opacity" => alpha *= a.min(1.0),
                "grayscale" => c = matrix(c, &grayscale(1.0 - a.min(1.0))),
                "sepia" => c = matrix(c, &sepia(1.0 - a.min(1.0))),
                "saturate" => c = matrix(c, &saturate(a)),
                "hue-rotate" => {
                    let degrees = amount.map_or(0.0, Amount::degrees);
                    c = matrix(c, &hue_rotate(degrees));
                }
                _ => {}
            }
            c = c.map(|v| v.clamp(0.0, 1.0));
        }
        let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
        [byte(c[0]), byte(c[1]), byte(c[2]), byte(alpha)]
    }

    /// The blur of a filter, in pixels.
    pub fn blur(&self) -> f32 {
        self.functions
            .iter()
            .filter(|f| f.raw.is_none() && f.name.eq_ignore_ascii_case("blur"))
            .filter_map(|f| f.args.first())
            .map(Amount::pixels)
            .sum()
    }

    /// Where a transform puts the point (`x`, `y`) of a box, from its center: the functions
    /// apply from the last to the first, as CSS does.
    pub fn transform(&self, x: f32, y: f32) -> (f32, f32) {
        let (mut x, mut y) = (x, y);
        for function in self.functions.iter().rev().filter(|f| f.raw.is_none()) {
            let arg = |i: usize| function.args.get(i);
            let name = function.name.to_ascii_lowercase();
            match name.as_str() {
                "translate" | "translatex" | "translatey" => {
                    let (dx, dy) = match name.as_str() {
                        "translatex" => (arg(0).map_or(0.0, Amount::pixels), 0.0),
                        "translatey" => (0.0, arg(0).map_or(0.0, Amount::pixels)),
                        _ => (
                            arg(0).map_or(0.0, Amount::pixels),
                            arg(1).map_or(0.0, Amount::pixels),
                        ),
                    };
                    x += dx;
                    y += dy;
                }
                "rotate" | "rotatez" => {
                    let (sin, cos) = arg(0).map_or(0.0, Amount::degrees).to_radians().sin_cos();
                    (x, y) = (x * cos - y * sin, x * sin + y * cos);
                }
                "scale" | "scalex" | "scaley" => {
                    let sx = arg(0).map_or(1.0, Amount::factor);
                    let (sx, sy) = match name.as_str() {
                        "scalex" => (sx, 1.0),
                        "scaley" => (1.0, sx),
                        _ => (sx, arg(1).map_or(sx, Amount::factor)),
                    };
                    x *= sx;
                    y *= sy;
                }
                "skew" | "skewx" | "skewy" => {
                    let ax = arg(0).map_or(0.0, Amount::degrees).to_radians().tan();
                    let (ax, ay) = match name.as_str() {
                        "skewx" => (ax, 0.0),
                        "skewy" => (0.0, ax),
                        _ => (ax, arg(1).map_or(0.0, Amount::degrees).to_radians().tan()),
                    };
                    (x, y) = (x + y * ax, y + x * ay);
                }
                _ => {}
            }
        }
        (x, y)
    }
}

fn matrix(c: [f32; 3], m: &[f32; 9]) -> [f32; 3] {
    [
        m[0] * c[0] + m[1] * c[1] + m[2] * c[2],
        m[3] * c[0] + m[4] * c[1] + m[5] * c[2],
        m[6] * c[0] + m[7] * c[1] + m[8] * c[2],
    ]
}

/// The matrices of the Filter Effects specification; `s` is 1 minus the amount.
#[rustfmt::skip]
fn grayscale(s: f32) -> [f32; 9] {
    [
        0.2126 + 0.7874 * s, 0.7152 - 0.7152 * s, 0.0722 - 0.0722 * s,
        0.2126 - 0.2126 * s, 0.7152 + 0.2848 * s, 0.0722 - 0.0722 * s,
        0.2126 - 0.2126 * s, 0.7152 - 0.7152 * s, 0.0722 + 0.9278 * s,
    ]
}

#[rustfmt::skip]
fn sepia(s: f32) -> [f32; 9] {
    [
        0.393 + 0.607 * s, 0.769 - 0.769 * s, 0.189 - 0.189 * s,
        0.349 - 0.349 * s, 0.686 + 0.314 * s, 0.168 - 0.168 * s,
        0.272 - 0.272 * s, 0.534 - 0.534 * s, 0.131 + 0.869 * s,
    ]
}

#[rustfmt::skip]
fn saturate(s: f32) -> [f32; 9] {
    [
        0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s,
        0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s,
        0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s,
    ]
}

fn hue_rotate(degrees: f32) -> [f32; 9] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    [
        0.213 + cos * 0.787 - sin * 0.213,
        0.715 - cos * 0.715 - sin * 0.715,
        0.072 - cos * 0.072 + sin * 0.928,
        0.213 - cos * 0.213 + sin * 0.143,
        0.715 + cos * 0.285 + sin * 0.140,
        0.072 - cos * 0.072 - sin * 0.283,
        0.213 - cos * 0.213 - sin * 0.787,
        0.715 - cos * 0.715 + sin * 0.715,
        0.072 + cos * 0.928 + sin * 0.072,
    ]
}

/// The filters and transforms written in a text: `filter`, `backdrop-filter` and `transform`
/// in CSS, `filter`, `backdropFilter` and `transform` in JavaScript.
pub fn find(text: &str) -> Vec<(Range<usize>, Effects)> {
    const NAMES: [(&str, EffectKind); 5] = [
        ("filter", EffectKind::Filter),
        ("backdrop-filter", EffectKind::Filter),
        ("backdropFilter", EffectKind::Filter),
        ("-webkit-backdrop-filter", EffectKind::Filter),
        ("transform", EffectKind::Transform),
    ];
    let names: Vec<&str> = NAMES.iter().map(|(n, _)| *n).collect();
    super::properties::values(text, &names)
        .into_iter()
        .filter_map(|(range, name)| {
            Effects::parse(&text[range.clone()], NAMES[name].1).map(|e| (range, e))
        })
        .collect()
}

#[cfg(test)]
#[path = "../../../test/languages/css/effects/tests.rs"]
mod tests;
