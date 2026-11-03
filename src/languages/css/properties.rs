//! Where the values of a few properties are written in a text: `name: value` in a sheet or a
//! `style` attribute, `camelName: 'value'` in JavaScript. For the swatches of the values that
//! have an editor of their own (shadows, radii, filters, transforms).

use std::ops::Range;

/// Where the value of a property ends: at `;`, `}`, the end of its line or its closing quote.
fn value_end(text: &str, from: usize, quote: Option<char>) -> usize {
    let mut depth = 0i32;
    for (i, c) in text[from..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ';' | '}' | '\n' if depth <= 0 => return from + i,
            c if Some(c) == quote => return from + i,
            '"' | '\'' if quote.is_none() && depth <= 0 => return from + i,
            _ => {}
        }
    }
    text.len()
}

/// The values of the properties `names`, with the index of their name, in the order of the
/// text. `!important` is left out of the value.
pub fn values(text: &str, names: &[&str]) -> Vec<(Range<usize>, usize)> {
    let mut out = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let mut from = 0;
        while let Some(found) = text[from..].find(name) {
            let start = from + found;
            from = start + name.len();
            // A whole name: not `my-box-shadow`, `backdrop-filter` for `filter`, `.filter(`.
            let before = text[..start].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '$')) {
                continue;
            }
            let after = text[from..].trim_start_matches([' ', '\t']);
            let Some(rest) = after.strip_prefix(':') else {
                continue;
            };
            let mut at = text.len() - rest.len();
            at += rest.len() - rest.trim_start_matches([' ', '\t']).len();
            // In JavaScript, the value is a string.
            let quote = text[at..]
                .chars()
                .next()
                .filter(|c| matches!(c, '"' | '\'' | '`'));
            if let Some(q) = quote {
                at += q.len_utf8();
            }
            let end = value_end(text, at, quote);
            let value = text[at..end].trim_end();
            let value = value
                .strip_suffix("!important")
                .map_or(value, str::trim_end);
            if !value.is_empty() {
                out.push((at..at + value.len(), index));
            }
        }
    }
    out.sort_by_key(|(r, _)| r.start);
    out
}
