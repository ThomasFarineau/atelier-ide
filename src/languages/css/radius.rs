//! `border-radius`: its four corners, read from the text for the swatch and the editor of
//! corners, written back in the shortest form CSS has for them (`8px`, `8px 4px`…).

use super::shadow::Length;
use std::ops::Range;

/// The corners in the order of CSS: top left, top right, bottom right, bottom left.
#[derive(Clone, PartialEq, Debug)]
pub struct Radius {
    pub corners: [Length; 4],
}

impl Radius {
    /// `8px`, `8px 4px`, `8px 4px 2px`, `8px 4px 2px 1px`; not the elliptic corners (`/`).
    pub fn parse(value: &str) -> Option<Self> {
        if value.contains('/') {
            return None;
        }
        let values: Vec<Length> = value
            .split_whitespace()
            .map(Length::parse)
            .collect::<Option<_>>()?;
        let corners = match values.as_slice() {
            [a] => [a.clone(), a.clone(), a.clone(), a.clone()],
            [a, b] => [a.clone(), b.clone(), a.clone(), b.clone()],
            [a, b, c] => [a.clone(), b.clone(), c.clone(), b.clone()],
            [a, b, c, d] => [a.clone(), b.clone(), c.clone(), d.clone()],
            _ => return None,
        };
        Some(Self { corners })
    }

    pub fn write(&self) -> String {
        let [a, b, c, d] = &self.corners;
        let parts: Vec<&Length> = if a == b && b == c && c == d {
            vec![a]
        } else if a == c && b == d {
            vec![a, b]
        } else if b == d {
            vec![a, b, c]
        } else {
            vec![a, b, c, d]
        };
        parts
            .iter()
            .map(|l| l.write())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The radius of a corner in pixels, for a box `width` by `height`: a percentage is one of
    /// its size.
    pub fn pixels(&self, corner: usize, width: f32, height: f32) -> f32 {
        let length = &self.corners[corner];
        if length.unit == "%" {
            length.value / 100.0 * width.min(height)
        } else {
            length.pixels()
        }
    }
}

/// The values of `border-radius` (and `borderRadius` in JavaScript) in a text.
pub fn find(text: &str) -> Vec<(Range<usize>, Radius)> {
    super::properties::values(text, &["border-radius", "borderRadius"])
        .into_iter()
        .filter_map(|(range, _)| Radius::parse(&text[range.clone()]).map(|r| (range, r)))
        .collect()
}

#[cfg(test)]
#[path = "../../../test/languages/css/radius/tests.rs"]
mod tests;
