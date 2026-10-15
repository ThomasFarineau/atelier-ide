//! Spell checking of the prose in code: comments, strings and the text of Markdown files. The
//! dictionaries are Hunspell's (English and French, of LibreOffice, in `assets/dictionaries`); a
//! word is right when one of them knows it. The words the user adds are kept in
//! `dictionary.txt` of the data folder of Atelier, one per line.

use spellbook::Dictionary;
use std::{
    collections::HashSet,
    fs,
    ops::Range,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

/// Words shorter than this are not checked.
const MIN_LENGTH: usize = 3;
/// Texts bigger than this are not checked.
pub const MAX_TEXT: usize = 1_500_000;
/// Typos reported at most for a document.
const MAX_TYPOS: usize = 400;

pub struct Checker {
    dictionaries: Vec<Dictionary>,
    user: Mutex<HashSet<String>>,
    user_file: Option<PathBuf>,
}

impl Checker {
    /// A checker over dictionaries given as (affixes, words); `user_file` keeps the added words.
    pub fn new(dictionaries: &[(&str, &str)], user_file: Option<PathBuf>) -> Self {
        let dictionaries = dictionaries
            .iter()
            .filter_map(|(aff, dic)| Dictionary::new(aff, dic).ok())
            .collect();
        let user = user_file
            .as_ref()
            .and_then(|f| fs::read_to_string(f).ok())
            .map(|text| {
                text.lines()
                    .map(|l| l.trim().to_lowercase())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Self {
            dictionaries,
            user: Mutex::new(user),
            user_file,
        }
    }

    pub fn is_correct(&self, word: &str) -> bool {
        if self
            .user
            .lock()
            .is_ok_and(|user| user.contains(&word.to_lowercase()))
        {
            return true;
        }
        self.dictionaries.iter().any(|d| d.check(word))
    }

    /// The words that look like `word`, the best first.
    pub fn suggestions(&self, word: &str) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for dictionary in &self.dictionaries {
            let mut list = Vec::new();
            dictionary.suggest(word, &mut list);
            for suggestion in list.into_iter().take(4) {
                if !found.contains(&suggestion) {
                    found.push(suggestion);
                }
            }
        }
        found.truncate(6);
        found
    }

    /// Adds a word to the dictionary of the user.
    pub fn add(&self, word: &str) {
        let word = word.trim().to_lowercase();
        if word.is_empty() {
            return;
        }
        let Ok(mut user) = self.user.lock() else {
            return;
        };
        if user.insert(word)
            && let Some(file) = &self.user_file
        {
            let mut words: Vec<&String> = user.iter().collect();
            words.sort();
            let text: String = words.iter().map(|w| format!("{w}\n")).collect();
            if let Some(dir) = file.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(file, text);
        }
    }
}

/// A dictionary file of `assets/dictionaries`, in the program.
macro_rules! embedded {
    ($path:literal) => {
        include_str!($path)
    };
}

static CHECKER: OnceLock<Arc<Checker>> = OnceLock::new();
static LOADING: AtomicBool = AtomicBool::new(false);

/// The checker once its dictionaries are read; the reading starts at the first call, on a thread
/// of its own, and the answer is None until it is done.
pub fn checker() -> Option<Arc<Checker>> {
    if let Some(checker) = CHECKER.get() {
        return Some(checker.clone());
    }
    if !LOADING.swap(true, Ordering::SeqCst) {
        std::thread::spawn(|| {
            let checker = Checker::new(
                &[
                    (
                        embedded!("../assets/dictionaries/en_US.aff"),
                        embedded!("../assets/dictionaries/en_US.dic"),
                    ),
                    (
                        embedded!("../assets/dictionaries/fr_FR.aff"),
                        embedded!("../assets/dictionaries/fr_FR.dic"),
                    ),
                ],
                Some(crate::cli::data_dir().join("dictionary.txt")),
            );
            let _ = CHECKER.set(Arc::new(checker));
        });
    }
    None
}

/// Whether the prose of a language can be checked.
pub fn supports(language: &str) -> bool {
    matches!(
        language,
        "rs" | "js" | "ts" | "jsx" | "tsx" | "java" | "c" | "cpp" | "h" | "cs" | "go" | "kt"
            | "swift" | "php" | "py" | "sh" | "rb" | "yaml" | "yml" | "toml" | "md" | "txt"
            | "rst" | "adoc"
    )
}

// ---- Which text is prose ---------------------------------------------------------------------

/// The ranges of `text` that are prose in a file of `language`: comments and strings, or the
/// whole text of a prose file.
pub fn prose(text: &str, language: &str) -> Vec<Range<usize>> {
    let (line_comment, block_comment, quotes): (Option<&str>, bool, &str) = match language {
        "rs" => (Some("//"), true, "\""),
        "js" | "ts" | "jsx" | "tsx" => (Some("//"), true, "\"'`"),
        "java" | "c" | "cpp" | "h" | "cs" | "go" | "kt" | "swift" | "php" => {
            (Some("//"), true, "\"'")
        }
        "py" | "sh" | "rb" | "yaml" | "yml" | "toml" => (Some("#"), false, "\"'"),
        "md" | "txt" | "rst" | "adoc" => return prose_file(text),
        _ => return Vec::new(),
    };
    let bytes = text.as_bytes();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &text[i..];
        if let Some(prefix) = line_comment
            && rest.starts_with(prefix)
        {
            let end = rest.find('\n').map_or(text.len(), |n| i + n);
            ranges.push(i + prefix.len()..end);
            i = end;
        } else if block_comment && rest.starts_with("/*") {
            let end = rest[2..].find("*/").map_or(text.len(), |n| i + 2 + n);
            ranges.push(i + 2..end);
            i = (end + 2).min(text.len());
        } else if let Some(quote) = rest.chars().next().filter(|c| quotes.contains(*c)) {
            // A rust lifetime or char is no string: the quote must close on the line.
            let mut end = None;
            let mut escaped = false;
            for (n, c) in rest[quote.len_utf8()..].char_indices() {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == quote {
                    end = Some(i + quote.len_utf8() + n);
                    break;
                } else if c == '\n' && quote != '`' {
                    break;
                }
            }
            match end {
                Some(end) => {
                    ranges.push(i + quote.len_utf8()..end);
                    i = end + quote.len_utf8();
                }
                None => i += quote.len_utf8(),
            }
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    ranges
}

/// A prose file: all of it, except the fenced blocks of code.
fn prose_file(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut fenced = false;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            if !fenced {
                ranges.push(start..offset);
            }
            fenced = !fenced;
            start = offset + line.len();
        }
        offset += line.len();
    }
    if !fenced {
        ranges.push(start..text.len());
    }
    ranges.retain(|r| !r.is_empty());
    ranges
}

/// The words of a stretch of prose worth checking, with their ranges.
fn words(text: &str, range: Range<usize>) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let slice = &text[range.clone()];
    let mut start: Option<usize> = None;
    let chars: Vec<(usize, char)> = slice.char_indices().collect();
    let boundary = |c: char| !(c.is_alphabetic() || c == '\'' || c == '’');
    for (n, &(at, c)) in chars.iter().enumerate() {
        match (start, boundary(c)) {
            (None, false) => start = Some(n),
            (Some(first), true) => {
                push_word(slice, &chars, first, n, range.start, &mut out);
                start = None;
            }
            _ => {}
        }
        let _ = at;
    }
    if let Some(first) = start {
        push_word(slice, &chars, first, chars.len(), range.start, &mut out);
    }
    out
}

fn push_word(
    slice: &str,
    chars: &[(usize, char)],
    first: usize,
    end: usize,
    base: usize,
    out: &mut Vec<Range<usize>>,
) {
    // Quotes around the word are not part of it.
    let (mut first, mut end) = (first, end);
    while first < end && matches!(chars[first].1, '\'' | '’') {
        first += 1;
    }
    while end > first && matches!(chars[end - 1].1, '\'' | '’') {
        end -= 1;
    }
    if end - first < MIN_LENGTH {
        return;
    }
    let word: String = chars[first..end].iter().map(|(_, c)| *c).collect();
    // Names of the code, acronyms and words of other alphabets are not for the dictionary.
    let inner_capital = word
        .chars()
        .skip(1)
        .any(char::is_uppercase);
    if inner_capital {
        return;
    }
    let before = first.checked_sub(1).map(|i| chars[i].1);
    let after = chars.get(end).map(|(_, c)| *c);
    let glued = |c: Option<char>| c.is_some_and(|c| c.is_ascii_digit() || "_/\\@#$".contains(c));
    if glued(before) || glued(after) {
        return;
    }
    // A file name or an address: `name.ext`, `.ext`.
    let dotted_after = after == Some('.')
        && chars
            .get(end + 1)
            .is_some_and(|(_, c)| c.is_alphanumeric());
    if dotted_after || (before == Some('.') && first >= 2 && chars[first - 2].1.is_alphanumeric()) {
        return;
    }
    let start = chars[first].0;
    let stop = chars.get(end).map_or(slice.len(), |(at, _)| *at);
    out.push(base + start..base + stop);
}

/// The misspelled words of the prose of `text`.
pub fn typos(checker: &Checker, text: &str, language: &str) -> Vec<Range<usize>> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let mut found = Vec::new();
    for range in prose(text, language) {
        for word in words(text, range) {
            if !checker.is_correct(&text[word.clone()]) {
                found.push(word);
                if found.len() >= MAX_TYPOS {
                    return found;
                }
            }
        }
    }
    found
}

#[cfg(test)]
#[path = "../test/spelling/tests.rs"]
mod tests;
