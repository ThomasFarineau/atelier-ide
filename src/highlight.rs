//! Syntax highlighting with the colors of IntelliJ's default Dark and Light editor schemes.
//!
//! Highlighting is incremental: the parser state is kept every few lines, so an edit only
//! recomputes from the edited line, and the work is spread over frames within a time budget.
//! A large file never blocks the interface; lines not reached yet show as plain text.

use eframe::egui::Color32;
use egui_extras::syntax_highlighting::SyntectSettings;
use std::{
    hash::{Hash, Hasher},
    ops::Range,
    str::FromStr,
    sync::OnceLock,
    time::{Duration, Instant},
};
use syntect::{
    highlighting::{
        Color, FontStyle, HighlightState, Highlighter, RangedHighlightIterator, ScopeSelectors,
        StyleModifier, Theme, ThemeItem, ThemeSettings,
    },
    parsing::{ParseState, ScopeStack, SyntaxReference},
};

const DARK_KEY: &str = "intellij-dark";
const LIGHT_KEY: &str = "intellij-light";
/// Lines between two saved parser states.
/// Longest line that is colored, in bytes.
const MAX_LINE: usize = 4000;
const CHECKPOINT: usize = 32;

struct Scheme {
    text: u32,
    background: u32,
    keyword: u32,
    string: u32,
    number: u32,
    comment: u32,
    doc: u32,
    function: u32,
    constant: u32,
    annotation: u32,
    tag: u32,
    attribute: u32,
    key: u32,
    escape: u32,
}

const DARK: Scheme = Scheme {
    text: 0xBCBEC4,
    background: 0x191A1C,
    keyword: 0xCF8E6D,
    string: 0x6AAB73,
    number: 0x2AACB8,
    comment: 0x7A7E85,
    doc: 0x5F826B,
    function: 0x56A8F5,
    constant: 0xC77DBB,
    annotation: 0xB3AE60,
    tag: 0xD5B778,
    attribute: 0xBABABA,
    key: 0xC77DBB,
    escape: 0xCF8E6D,
};

const LIGHT: Scheme = Scheme {
    text: 0x080808,
    background: 0xFFFFFF,
    keyword: 0x0033B3,
    string: 0x067D17,
    number: 0x1750EB,
    comment: 0x8C8C8C,
    doc: 0x8C8C8C,
    function: 0x00627A,
    constant: 0x871094,
    annotation: 0x9E880D,
    tag: 0x0033B3,
    attribute: 0x174AD4,
    key: 0x871094,
    escape: 0x0037A6,
};

fn color(rgb: u32) -> Color {
    Color {
        r: (rgb >> 16) as u8,
        g: (rgb >> 8) as u8,
        b: rgb as u8,
        a: 0xFF,
    }
}

fn theme(scheme: &Scheme) -> Theme {
    let bold = Some(FontStyle::BOLD);
    // Later rules win over earlier ones of the same specificity.
    let rules: &[(&str, u32, Option<FontStyle>)] = &[
        (
            "comment, punctuation.definition.comment",
            scheme.comment,
            None,
        ),
        (
            "comment.block.documentation, comment.line.documentation",
            scheme.doc,
            None,
        ),
        ("string, punctuation.definition.string", scheme.string, None),
        ("constant.character.escape", scheme.escape, None),
        ("constant.numeric", scheme.number, None),
        (
            "keyword, storage, storage.type, storage.modifier, constant.language, variable.language",
            scheme.keyword,
            None,
        ),
        (
            "keyword.operator, punctuation, meta.brace",
            scheme.text,
            None,
        ),
        (
            "keyword.operator.word, keyword.operator.new",
            scheme.keyword,
            None,
        ),
        (
            "entity.name.function, support.function, meta.function-call entity.name.function, variable.function, support.macro, entity.name.macro",
            scheme.function,
            None,
        ),
        (
            "variable.other.constant, constant.other, entity.name.constant, support.constant, variable.other.enummember",
            scheme.constant,
            None,
        ),
        (
            "meta.annotation, storage.type.annotation, meta.attribute, punctuation.definition.annotation, meta.decorator",
            scheme.annotation,
            None,
        ),
        (
            "entity.name.tag, entity.other.attribute-name.class.css, entity.other.attribute-name.id.css",
            scheme.tag,
            None,
        ),
        ("entity.other.attribute-name", scheme.attribute, None),
        (
            "support.type.property-name, meta.mapping.key string, entity.name.tag.yaml",
            scheme.key,
            None,
        ),
        ("markup.heading, markup.bold", scheme.keyword, bold),
        ("markup.raw, markup.inline.raw", scheme.string, None),
        ("markup.underline.link", scheme.function, None),
    ];
    Theme {
        name: Some("IntelliJ".into()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(color(scheme.text)),
            background: Some(color(scheme.background)),
            ..Default::default()
        },
        scopes: rules
            .iter()
            .map(|(scope, rgb, font_style)| ThemeItem {
                scope: ScopeSelectors::from_str(scope).expect("valid scope selector"),
                style: StyleModifier {
                    foreground: Some(color(*rgb)),
                    background: None,
                    font_style: *font_style,
                },
            })
            .collect(),
    }
}

fn settings() -> &'static SyntectSettings {
    static SETTINGS: OnceLock<SyntectSettings> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let mut settings = SyntectSettings::default();
        settings.ts.themes.insert(DARK_KEY.into(), theme(&DARK));
        settings.ts.themes.insert(LIGHT_KEY.into(), theme(&LIGHT));
        settings
    })
}

/// Syntax name or file extension understood by syntect for an IDE language.
fn syntax(language: &str) -> &str {
    match language {
        "ts" => "js",
        "txt" => "",
        other => other,
    }
}

/// Words of TypeScript that the JavaScript syntax leaves uncolored.
const TS_KEYWORDS: &[&str] = &[
    "interface",
    "enum",
    "implements",
    "readonly",
    "public",
    "private",
    "protected",
    "abstract",
    "namespace",
    "declare",
    "satisfies",
    "keyof",
    "infer",
    "override",
    "as",
];
const TS_TYPES: &[&str] = &[
    "string", "number", "boolean", "any", "unknown", "never", "void", "object", "symbol", "bigint",
];

/// Colors the TypeScript keywords and primitive types of a line that the JavaScript syntax
/// did not color (a word in a string or a comment already has a color).
fn typescript_overlay(line: &str, spans: &mut Vec<(Range<usize>, Color32)>, keyword: Color32) {
    let bytes = line.as_bytes();
    let mut added: Vec<(Range<usize>, Color32)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] == b'$') {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
        {
            i += 1;
        }
        let word = &line[start..i];
        let previous = line[..start].chars().next_back();
        // A property (`a.string`) or a key is not a keyword.
        if previous == Some('.') {
            continue;
        }
        let is_keyword = if word == "type" {
            // `type Name =` or `type Name<`, at the start of a statement.
            let before = line[..start].trim_end();
            let rest = line[i..].trim_start();
            let starts_statement = before.is_empty()
                || before.ends_with("export")
                || before.ends_with("declare")
                || before.ends_with(';')
                || before.ends_with('{')
                || before.ends_with('}');
            let named = rest
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$');
            starts_statement && named && rest.contains(['=', '<'])
        } else {
            TS_KEYWORDS.contains(&word) || TS_TYPES.contains(&word)
        };
        let covered = spans
            .iter()
            .any(|(range, _)| range.start <= start && i <= range.end);
        if is_keyword && !covered {
            added.push((start..i, keyword));
        }
    }
    if !added.is_empty() {
        spans.extend(added);
        spans.sort_by_key(|(range, _)| range.start);
    }
}

pub fn line_hash(line: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    line.hash(&mut hasher);
    hasher.finish()
}

/// Colored ranges of one line, with the hash of the text they were computed for.
pub struct LineSpans {
    hash: u64,
    pub spans: Vec<(Range<usize>, Color32)>,
}

/// Where the colors of a TypeScript document, taken from its syntax tree, stand.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TreeColors {
    /// Computed again at the next frame.
    Stale,
    Fresh,
    /// The tree could not color it: the editor syntax does.
    Unavailable,
}

fn palette(scheme: &Scheme) -> crate::languages::typescript::highlight::Palette {
    let c = |rgb: u32| Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    crate::languages::typescript::highlight::Palette {
        keyword: c(scheme.keyword),
        string: c(scheme.string),
        number: c(scheme.number),
        comment: c(scheme.comment),
        function: c(scheme.function),
        constant: c(scheme.constant),
        annotation: c(scheme.annotation),
        tag: c(scheme.tag),
        escape: c(scheme.escape),
        text: c(scheme.text),
    }
}

/// The colors of a style sheet (CSS, SCSS, Sass, Less), from its own lexer: syntect knows
/// none but CSS, and not its nesting.
fn style_colors(
    text: &str,
    language: &str,
    scheme: &Scheme,
) -> Option<Vec<crate::languages::typescript::highlight::LineColors>> {
    use crate::languages::css::lexer::{self, Role};
    if text.len() > crate::languages::css::analysis::MAX_TEXT {
        return None;
    }
    let c = |rgb: u32| Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    let color = |role: Role| match role {
        Role::Comment => c(scheme.comment),
        Role::String => c(scheme.string),
        Role::Number => c(scheme.number),
        Role::Keyword | Role::Interpolation => c(scheme.keyword),
        Role::Property => c(scheme.key),
        Role::Variable => c(scheme.constant),
        Role::Tag => c(scheme.tag),
        Role::Class | Role::Function => c(scheme.function),
        Role::Pseudo => c(scheme.annotation),
        Role::Attribute => c(scheme.attribute),
        Role::Text => c(scheme.text),
    };
    let lines = lexer::line_roles(text, lexer::Dialect::of(language));
    Some(
        lines
            .into_iter()
            .map(|spans| {
                spans
                    .into_iter()
                    .map(|(r, role)| (r, color(role)))
                    .collect()
            })
            .collect(),
    )
}

/// Highlighting of one document, computed line by line on demand.
pub struct Incremental {
    tree_colors: TreeColors,
    language: String,
    dark: bool,
    syntax: Option<&'static SyntaxReference>,
    highlighter: Highlighter<'static>,
    /// Parser state at the start of lines 0, CHECKPOINT, 2 × CHECKPOINT…
    checkpoints: Vec<(ParseState, HighlightState)>,
    /// State at the start of line `lines.len()`, the next one to compute.
    state: Option<(ParseState, HighlightState)>,
    lines: Vec<LineSpans>,
}

impl Incremental {
    pub fn new(language: &str, dark: bool) -> Self {
        let settings = settings();
        let name = syntax(language);
        let syntax = if name.is_empty() {
            None
        } else {
            settings
                .ps
                .find_syntax_by_extension(name)
                .or_else(|| settings.ps.find_syntax_by_name(name))
        };
        let theme = &settings.ts.themes[if dark { DARK_KEY } else { LIGHT_KEY }];
        let mut highlighter = Self {
            tree_colors: if language == "ts" || crate::languages::css::is_style(language) {
                TreeColors::Stale
            } else {
                TreeColors::Unavailable
            },
            language: language.into(),
            dark,
            syntax,
            highlighter: Highlighter::new(theme),
            checkpoints: Vec::new(),
            state: None,
            lines: Vec::new(),
        };
        highlighter.invalidate_from(0);
        highlighter
    }

    /// True when the highlighter was made for another language or theme.
    pub fn outdated(&self, language: &str, dark: bool) -> bool {
        self.language != language || self.dark != dark
    }

    /// Forgets lines from `line` on; they are computed again when needed.
    pub fn invalidate_from(&mut self, line: usize) {
        if self.tree_colors != TreeColors::Unavailable {
            self.tree_colors = TreeColors::Stale;
            self.lines.clear();
        }
        let Some(syntax) = self.syntax else { return };
        if line > self.lines.len() && self.state.is_some() {
            return;
        }
        let checkpoint = (line / CHECKPOINT).min(self.checkpoints.len().saturating_sub(1));
        self.checkpoints.truncate(checkpoint + 1);
        if self.checkpoints.is_empty() {
            self.checkpoints.push((
                ParseState::new(syntax),
                HighlightState::new(&self.highlighter, ScopeStack::new()),
            ));
        }
        self.lines.truncate(checkpoint * CHECKPOINT);
        self.state = self.checkpoints.last().cloned();
    }

    /// Colors of line `index`, if they were computed for this text.
    pub fn line(&mut self, index: usize, text: &str) -> Option<&LineSpans> {
        if self.lines.get(index)?.hash != line_hash(text) {
            // Changed without going through the editor (reload, preview…).
            self.invalidate_from(index);
            return None;
        }
        self.lines.get(index)
    }

    /// Computes lines until `until` (excluded) or until the time budget is spent.
    /// `line_text(i)` returns line `i` without its line break. Returns true when done.
    pub fn advance(
        &mut self,
        line_text: impl Fn(usize) -> Option<String>,
        until: usize,
        budget: Duration,
    ) -> bool {
        if self.syntax.is_none() && self.tree_colors == TreeColors::Unavailable {
            return true;
        }
        if self.tree_colors != TreeColors::Unavailable {
            if self.tree_colors == TreeColors::Stale {
                // The whole document at once: the tree needs all of it.
                let mut all: Vec<String> = Vec::new();
                while let Some(line) = line_text(all.len()) {
                    all.push(line);
                }
                let scheme = if self.dark { &DARK } else { &LIGHT };
                let colored = if self.language == "ts" {
                    crate::languages::typescript::highlight::colors(
                        &all.join("\n"),
                        &palette(scheme),
                    )
                } else {
                    style_colors(&all.join("\n"), &self.language, scheme)
                };
                match colored {
                    Some(per_line) if per_line.len() == all.len() => {
                        self.lines = per_line
                            .into_iter()
                            .zip(&all)
                            .map(|(spans, text)| LineSpans {
                                hash: line_hash(text),
                                // Minified lines are left uncolored, as with the editor syntax.
                                spans: if text.len() > MAX_LINE {
                                    Vec::new()
                                } else {
                                    spans
                                },
                            })
                            .collect();
                        self.tree_colors = TreeColors::Fresh;
                    }
                    _ => self.tree_colors = TreeColors::Unavailable,
                }
            }
            if self.tree_colors == TreeColors::Fresh {
                return true;
            }
        }
        let started = Instant::now();
        let set = &settings().ps;
        while self.lines.len() < until {
            let Some(text) = line_text(self.lines.len()) else {
                return true;
            };
            let Some((parse, highlight)) = &mut self.state else {
                return true;
            };
            // Minified lines are left uncolored: parsing them takes seconds.
            if text.len() > MAX_LINE {
                self.lines.push(LineSpans {
                    hash: line_hash(&text),
                    spans: Vec::new(),
                });
                continue;
            }
            // syntect expects the line break, as in the files it parses.
            let line = format!("{text}\n");
            let ops = parse.parse_line(&line, set).unwrap_or_default();
            let mut spans: Vec<(Range<usize>, Color32)> =
                RangedHighlightIterator::new(highlight, &ops, &line, &self.highlighter)
                    .filter(|(_, _, range)| range.start < text.len())
                    .map(|(style, _, range)| {
                        let c = style.foreground;
                        (
                            range.start..range.end.min(text.len()),
                            Color32::from_rgb(c.r, c.g, c.b),
                        )
                    })
                    .collect();
            // The tree could not color it: the words JavaScript does not know are added.
            if self.language == "ts" {
                let keyword = if self.dark {
                    DARK.keyword
                } else {
                    LIGHT.keyword
                };
                typescript_overlay(
                    &text,
                    &mut spans,
                    Color32::from_rgb((keyword >> 16) as u8, (keyword >> 8) as u8, keyword as u8),
                );
            }
            self.lines.push(LineSpans {
                hash: line_hash(&text),
                spans,
            });
            if self.lines.len().is_multiple_of(CHECKPOINT)
                && let Some(state) = &self.state
            {
                self.checkpoints.push(state.clone());
            }
            if started.elapsed() > budget {
                return self.lines.len() >= until;
            }
        }
        true
    }
}

#[cfg(test)]
#[path = "../test/highlight/tests.rs"]
mod tests;
