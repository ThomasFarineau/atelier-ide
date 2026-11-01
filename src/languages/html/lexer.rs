//! The tokens and the tags of an HTML document, read the way a browser reads them: a `<` that
//! does not start a tag is text, the content of `<script>` and `<style>` is not markup (it
//! ends at its closing tag only), and nothing stops the reading, an unclosed tag or comment
//! included.

use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    Comment,
    /// `<!DOCTYPE html>`, `<![CDATA[…]]>`, `<?xml …?>`.
    Declaration,
    /// `<` or `</`.
    TagOpen,
    TagName,
    /// `>` or `/>`.
    TagClose,
    AttrName,
    Equals,
    /// The value of an attribute, its quotes included.
    AttrValue,
    /// `&amp;`, `&#160;`.
    Entity,
    /// The content of `<script>`, `<style>`, `<textarea>` or `<title>`.
    RawText,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attr {
    /// The name in lower case.
    pub name: String,
    pub name_range: Range<usize>,
    /// The value without its quotes; None for a boolean attribute (`disabled`).
    pub value: Option<Range<usize>>,
    pub quoted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    /// The name in lower case.
    pub name: String,
    pub name_range: Range<usize>,
    /// From its `<` to after its `>` (or to where the reading stopped).
    pub range: Range<usize>,
    pub closing: bool,
    /// Ends with `/>`.
    pub self_closing: bool,
    /// Its `>` was found.
    pub complete: bool,
    pub attrs: Vec<Attr>,
}

impl Tag {
    pub fn attr(&self, name: &str) -> Option<&Attr> {
        self.attrs.iter().find(|a| a.name == name)
    }

    /// The value of an attribute, in `text`.
    pub fn value<'a>(&self, text: &'a str, name: &str) -> Option<&'a str> {
        self.attr(name)?.value.clone().map(|r| &text[r])
    }
}

/// The text inside an element whose content is not markup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raw {
    /// Its opening tag, in [`Scan::tags`].
    pub tag: usize,
    pub content: Range<usize>,
}

/// Something left open at the end of the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unclosed {
    Comment,
    Tag,
    Quote,
}

#[derive(Default, Debug)]
pub struct Scan {
    pub tokens: Vec<Token>,
    pub tags: Vec<Tag>,
    pub raw: Vec<Raw>,
    pub unclosed: Vec<(Range<usize>, Unclosed)>,
}

/// The elements whose content runs to their closing tag without being markup.
pub fn is_raw(name: &str) -> bool {
    matches!(name, "script" | "style" | "textarea" | "title")
}

fn is_name_char(b: u8) -> bool {
    !matches!(
        b,
        b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'/' | b'>' | b'<'
    )
}

fn is_attr_name_char(b: u8) -> bool {
    !matches!(
        b,
        b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'/' | b'>' | b'=' | b'"' | b'\''
    )
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

/// Where `needle` (ASCII, lower case) is next found from `from`, ignoring case.
fn find_ignore_case(text: &str, from: usize, needle: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let n = needle.as_bytes();
    (from..bytes.len().saturating_sub(n.len() - 1))
        .find(|&i| bytes[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// The length of the entity at `at` (`&amp;`, `&#38;`, `&#x26;`), 0 when it is not one.
fn entity_len(bytes: &[u8], at: usize) -> usize {
    let mut i = at + 1;
    if bytes.get(i) == Some(&b'#') {
        i += 1;
        let hex = matches!(bytes.get(i), Some(b'x' | b'X'));
        if hex {
            i += 1;
        }
        let digits = i;
        while bytes.get(i).is_some_and(|b| {
            if hex {
                b.is_ascii_hexdigit()
            } else {
                b.is_ascii_digit()
            }
        }) {
            i += 1;
        }
        if i == digits {
            return 0;
        }
    } else {
        let letters = i;
        while bytes.get(i).is_some_and(|b| b.is_ascii_alphanumeric()) {
            i += 1;
        }
        if i == letters {
            return 0;
        }
    }
    if bytes.get(i) == Some(&b';') {
        i += 1;
    }
    i - at
}

/// Reads the whole document.
pub fn scan(text: &str) -> Scan {
    let bytes = text.as_bytes();
    let mut scan = Scan::default();
    let mut i = 0;
    let mut text_start = 0;
    let flush_text = |scan: &mut Scan, from: usize, to: usize| {
        if to <= from {
            return;
        }
        // Entities inside the text get tokens of their own.
        let mut start = from;
        let mut j = from;
        while j < to {
            if bytes[j] == b'&' {
                let len = entity_len(bytes, j).min(to - j);
                if len > 0 {
                    if j > start {
                        scan.tokens.push(Token {
                            kind: Kind::Text,
                            range: start..j,
                        });
                    }
                    scan.tokens.push(Token {
                        kind: Kind::Entity,
                        range: j..j + len,
                    });
                    j += len;
                    start = j;
                    continue;
                }
            }
            j += 1;
        }
        if to > start {
            scan.tokens.push(Token {
                kind: Kind::Text,
                range: start..to,
            });
        }
    };
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &bytes[i..];
        // Comments, declarations, CDATA, processing instructions.
        if rest.starts_with(b"<!--") {
            flush_text(&mut scan, text_start, i);
            let end = text[i + 4..].find("-->").map(|e| i + 4 + e + 3);
            let stop = end.unwrap_or(bytes.len());
            if end.is_none() {
                scan.unclosed.push((i..i + 4, Unclosed::Comment));
            }
            scan.tokens.push(Token {
                kind: Kind::Comment,
                range: i..stop,
            });
            i = stop;
            text_start = i;
            continue;
        }
        if rest.starts_with(b"<![CDATA[") {
            flush_text(&mut scan, text_start, i);
            let stop = text[i..].find("]]>").map_or(bytes.len(), |e| i + e + 3);
            scan.tokens.push(Token {
                kind: Kind::Declaration,
                range: i..stop,
            });
            i = stop;
            text_start = i;
            continue;
        }
        if rest.starts_with(b"<!") || rest.starts_with(b"<?") {
            flush_text(&mut scan, text_start, i);
            let stop = text[i..].find('>').map_or(bytes.len(), |e| i + e + 1);
            scan.tokens.push(Token {
                kind: Kind::Declaration,
                range: i..stop,
            });
            i = stop;
            text_start = i;
            continue;
        }
        let closing = rest.starts_with(b"</");
        let name_start = i + if closing { 2 } else { 1 };
        // A `<` that is not followed by a letter is text (`a < b`).
        if !bytes.get(name_start).is_some_and(u8::is_ascii_alphabetic) {
            i += 1;
            continue;
        }
        flush_text(&mut scan, text_start, i);
        let tag_start = i;
        scan.tokens.push(Token {
            kind: Kind::TagOpen,
            range: i..name_start,
        });
        let mut j = name_start;
        while j < bytes.len() && is_name_char(bytes[j]) {
            j += 1;
        }
        let name = text[name_start..j].to_ascii_lowercase();
        scan.tokens.push(Token {
            kind: Kind::TagName,
            range: name_start..j,
        });
        let mut tag = Tag {
            name,
            name_range: name_start..j,
            range: tag_start..j,
            closing,
            self_closing: false,
            complete: false,
            attrs: Vec::new(),
        };
        // The attributes, up to `>` or `/>`.
        loop {
            while j < bytes.len() && is_space(bytes[j]) {
                j += 1;
            }
            if j >= bytes.len() {
                scan.unclosed.push((tag.name_range.clone(), Unclosed::Tag));
                break;
            }
            match bytes[j] {
                b'>' => {
                    scan.tokens.push(Token {
                        kind: Kind::TagClose,
                        range: j..j + 1,
                    });
                    j += 1;
                    tag.complete = true;
                    break;
                }
                b'/' if bytes.get(j + 1) == Some(&b'>') => {
                    scan.tokens.push(Token {
                        kind: Kind::TagClose,
                        range: j..j + 2,
                    });
                    j += 2;
                    tag.complete = true;
                    tag.self_closing = true;
                    break;
                }
                b'/' => {
                    j += 1;
                    continue;
                }
                // A tag that a new one interrupts (`<div <p>`): it ends here.
                b'<' => {
                    scan.unclosed.push((tag.name_range.clone(), Unclosed::Tag));
                    break;
                }
                _ => {}
            }
            let attr_start = j;
            j += 1;
            while j < bytes.len() && is_attr_name_char(bytes[j]) {
                j += 1;
            }
            let mut attr = Attr {
                name: text[attr_start..j].to_ascii_lowercase(),
                name_range: attr_start..j,
                value: None,
                quoted: false,
            };
            scan.tokens.push(Token {
                kind: Kind::AttrName,
                range: attr_start..j,
            });
            let mut k = j;
            while k < bytes.len() && is_space(bytes[k]) {
                k += 1;
            }
            if bytes.get(k) == Some(&b'=') {
                scan.tokens.push(Token {
                    kind: Kind::Equals,
                    range: k..k + 1,
                });
                k += 1;
                while k < bytes.len() && is_space(bytes[k]) {
                    k += 1;
                }
                match bytes.get(k) {
                    Some(&quote @ (b'"' | b'\'')) => {
                        let close = bytes[k + 1..].iter().position(|&b| b == quote);
                        let end = close.map_or(bytes.len(), |c| k + 1 + c);
                        if close.is_none() {
                            scan.unclosed.push((k..k + 1, Unclosed::Quote));
                        }
                        attr.value = Some(k + 1..end);
                        attr.quoted = true;
                        let stop = (end + 1).min(bytes.len());
                        scan.tokens.push(Token {
                            kind: Kind::AttrValue,
                            range: k..stop,
                        });
                        k = stop;
                    }
                    Some(_) => {
                        let from = k;
                        while k < bytes.len() && !is_space(bytes[k]) && bytes[k] != b'>' {
                            k += 1;
                        }
                        attr.value = Some(from..k);
                        scan.tokens.push(Token {
                            kind: Kind::AttrValue,
                            range: from..k,
                        });
                    }
                    None => {}
                }
                j = k;
            }
            tag.attrs.push(attr);
        }
        tag.range.end = j;
        let raw = !tag.closing && !tag.self_closing && tag.complete && is_raw(&tag.name);
        let name = tag.name.clone();
        scan.tags.push(tag);
        i = j;
        text_start = i;
        if raw {
            // Up to `</name`, whatever is in between.
            let end = find_ignore_case(text, i, &format!("</{name}")).unwrap_or(bytes.len());
            if matches!(name.as_str(), "script" | "style") {
                if end > i {
                    scan.tokens.push(Token {
                        kind: Kind::RawText,
                        range: i..end,
                    });
                }
            } else {
                // `<title>` and `<textarea>` have no tags but do have entities.
                flush_text(&mut scan, i, end);
            }
            scan.raw.push(Raw {
                tag: scan.tags.len() - 1,
                content: i..end,
            });
            i = end;
            text_start = i;
        }
    }
    flush_text(&mut scan, text_start, bytes.len());
    scan
}

/// The token at `offset`: the one that holds it, or the one that ends there.
pub fn token_at(tokens: &[Token], offset: usize) -> Option<&Token> {
    let index = tokens.partition_point(|t| t.range.end < offset);
    let token = tokens.get(index)?;
    (token.range.start <= offset).then_some(token)
}

/// The tag whose brackets hold `offset` (its `<` excluded).
pub fn tag_at(tags: &[Tag], offset: usize) -> Option<(usize, &Tag)> {
    let index = tags.partition_point(|t| t.range.end < offset);
    let tag = tags.get(index)?;
    let inside = tag.range.start < offset
        && (offset < tag.range.end || (!tag.complete && offset <= tag.range.end));
    inside.then_some((index, tag))
}
