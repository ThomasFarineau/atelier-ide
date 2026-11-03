//! The easing curves of CSS: `cubic-bezier(x1, y1, x2, y2)` and the keywords that stand for
//! one (`ease`, `ease-in`…) in a `transition` or an `animation`. Read for their swatch and the
//! editor of curves, written back as `cubic-bezier()`, or as the keyword they equal.

use std::ops::Range;

/// The two control points of the curve, the ends being (0, 0) and (1, 1).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Easing {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

/// The keywords of CSS and their curves.
pub const PRESETS: [(&str, Easing); 5] = [
    ("ease", Easing::new(0.25, 0.1, 0.25, 1.0)),
    ("ease-in", Easing::new(0.42, 0.0, 1.0, 1.0)),
    ("ease-out", Easing::new(0.0, 0.0, 0.58, 1.0)),
    ("ease-in-out", Easing::new(0.42, 0.0, 0.58, 1.0)),
    ("linear", Easing::new(0.0, 0.0, 1.0, 1.0)),
];

fn number(value: f32) -> String {
    let text = format!("{:.2}", (value * 100.0).round() / 100.0);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}

impl Easing {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    /// `cubic-bezier(…)` or a keyword.
    pub fn parse(text: &str) -> Option<Self> {
        let lower = text.trim().to_ascii_lowercase();
        if let Some((_, easing)) = PRESETS.iter().find(|(name, _)| *name == lower) {
            return Some(*easing);
        }
        let inside = lower.strip_prefix("cubic-bezier(")?.strip_suffix(')')?;
        let values: Vec<f32> = inside
            .split(',')
            .map(|v| v.trim().parse().ok())
            .collect::<Option<_>>()?;
        let [x1, y1, x2, y2] = values.as_slice() else {
            return None;
        };
        // The times must stay between 0 and 1.
        ((0.0..=1.0).contains(x1) && (0.0..=1.0).contains(x2))
            .then_some(Self::new(*x1, *y1, *x2, *y2))
    }

    /// The keyword of the curve, if it is one of them.
    pub fn keyword(&self) -> Option<&'static str> {
        let close = |a: f32, b: f32| (a - b).abs() < 0.005;
        PRESETS.iter().find_map(|(name, e)| {
            (close(e.x1, self.x1)
                && close(e.y1, self.y1)
                && close(e.x2, self.x2)
                && close(e.y2, self.y2))
            .then_some(*name)
        })
    }

    pub fn write(&self) -> String {
        format!(
            "cubic-bezier({}, {}, {}, {})",
            number(self.x1),
            number(self.y1),
            number(self.x2),
            number(self.y2)
        )
    }

    /// The point of the curve at parameter `t` (0 to 1): (time, progress).
    pub fn point(&self, t: f32) -> (f32, f32) {
        let u = 1.0 - t;
        let x = 3.0 * u * u * t * self.x1 + 3.0 * u * t * t * self.x2 + t * t * t;
        let y = 3.0 * u * u * t * self.y1 + 3.0 * u * t * t * self.y2 + t * t * t;
        (x, y)
    }

    /// The progress at `time` (0 to 1), as the browser animates it.
    pub fn progress(&self, time: f32) -> f32 {
        // The parameter whose time is `time`, by halving: the time grows with it.
        let (mut low, mut high) = (0.0f32, 1.0f32);
        for _ in 0..30 {
            let mid = (low + high) / 2.0;
            if self.point(mid).0 < time {
                low = mid;
            } else {
                high = mid;
            }
        }
        self.point((low + high) / 2.0).1
    }
}

/// The curves written in a text: every `cubic-bezier()`, and the keywords given to the
/// properties of timing (`transition`, `animation` and their `-timing-function`).
pub fn find(text: &str) -> Vec<(Range<usize>, Easing)> {
    let mut out = Vec::new();
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find("cubic-bezier(") {
        let start = from + found;
        let Some(close) = lower[start..].find(')').map(|c| start + c + 1) else {
            break;
        };
        if let Some(easing) = Easing::parse(&text[start..close]) {
            out.push((start..close, easing));
        }
        from = close;
    }
    // The keywords, in the values of the properties that take a timing.
    for property in ["transition", "animation"] {
        let mut from = 0;
        while let Some(found) = lower[from..].find(property) {
            let start = from + found;
            from = start + property.len();
            let before = lower[..start].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            // `transition:`, `transition-timing-function:`, but not `transition-delay`.
            let rest = &lower[from..];
            let name_end = rest.find(':').unwrap_or(0);
            let suffix = rest[..name_end].trim();
            if !(suffix.is_empty() || suffix == "-timing-function")
                || rest[..name_end].contains('\n')
            {
                continue;
            }
            let value_start = from + name_end + 1;
            let value_end = lower[value_start..]
                .find([';', '}', '\n', '"', '\''])
                .map_or(lower.len(), |e| value_start + e);
            let mut at = value_start;
            for word in lower[value_start..value_end].split([' ', ',', '\t']) {
                let word_start = at;
                at += word.len() + 1;
                if PRESETS.iter().any(|(name, _)| *name == word)
                    && let Some(easing) = Easing::parse(word)
                {
                    out.push((word_start..word_start + word.len(), easing));
                }
            }
        }
    }
    out.sort_by_key(|(r, _)| r.start);
    out.dedup_by_key(|(r, _)| r.start);
    out
}

#[cfg(test)]
#[path = "../../../test/languages/css/easing/tests.rs"]
mod tests;
