//! The translations of a project, from one file chosen as the source: the files of the other
//! languages are found next to it, read as lists of keys, checked against the source and
//! written back key by key.
//!
//! Three layouts are understood, the language being in the name of a file or of a folder:
//! - `locales/fr_FR.json`, `locales/en.json`: one file per language in a folder;
//! - `locales/fr/common.json`, `locales/en/common.json`: one folder per language;
//! - `messages.properties`, `messages_fr.properties`: Java bundles, the file without a suffix
//!   being the default language.
//!
//! JSON files may be flat (`"menu.open": "…"`) or nested (`{"menu": {"open": "…"}}`): the keys
//! are the dotted paths of the strings, and a new key goes where its parent is.

use regex::Regex;
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::LazyLock,
};

/// Where the choice of the source is kept, in the project.
pub const CONFIG: &str = ".atelier/i18n.json";

static LOCALE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z]{2,3}(?:[-_](?:[A-Z]{2}|[A-Za-z]{4}|\d{3}))?(?:[-_][A-Z]{2})?$")
        .expect("regex")
});
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\{\{\s*[\w.\-]+\s*\}\}|\$\{[\w.\-]+\}|\{[\w.\-]*\}|%(?:\d+\$)?[sdifu@]|%\{[\w.\-]+\}",
    )
    .expect("regex")
});

pub fn is_locale(name: &str) -> bool {
    LOCALE.is_match(name)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    Properties,
}

/// How the language is written in the paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Layout {
    /// `dir/<language>.json`
    Stem { dir: PathBuf, extension: String },
    /// `dir/<language>/<file>`
    Folder { dir: PathBuf, file: String },
    /// `dir/<base>_<language>.properties`, and `dir/<base>.properties` for the default language.
    Suffix { dir: PathBuf, base: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocaleFile {
    pub language: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub format: Format,
    pub layout: Layout,
    pub source: LocaleFile,
    /// The other languages, sorted.
    pub others: Vec<LocaleFile>,
}

impl Catalog {
    /// The source first, then the other languages.
    pub fn files(&self) -> impl Iterator<Item = &LocaleFile> {
        std::iter::once(&self.source).chain(&self.others)
    }

    pub fn file(&self, language: &str) -> Option<&LocaleFile> {
        self.files().find(|f| f.language == language)
    }

    /// The file a language has, or would have.
    pub fn path_for(&self, language: &str) -> PathBuf {
        match &self.layout {
            Layout::Stem { dir, extension } => dir.join(format!("{language}.{extension}")),
            Layout::Folder { dir, file } => dir.join(language).join(file),
            Layout::Suffix { dir, base } => dir.join(format!("{base}_{language}.properties")),
        }
    }
}

fn format_of(path: &Path) -> Result<Format, String> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => Ok(Format::Json),
        Some("properties") => Ok(Format::Properties),
        _ => Err(tr!("translations.unsupported_format").into()),
    }
}

/// The catalog a source file belongs to: its layout, and the files of the other languages.
pub fn discover(source: &Path) -> Result<Catalog, String> {
    if !source.is_file() {
        return Err(trf!("translations.source_missing", path = source.display()));
    }
    let format = format_of(source)?;
    let dir = source.parent().unwrap_or(Path::new(".")).to_path_buf();
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let file_name = source
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let list = |dir: &Path| -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        found.sort();
        found
    };
    let (layout, language, others) = if format == Format::Properties {
        // `messages_fr_FR.properties`: the base is what comes before the language.
        let (base, language) = match stem.split_once('_') {
            Some((base, rest)) if is_locale(rest) => (base.to_string(), rest.to_string()),
            _ => (stem.clone(), "default".to_string()),
        };
        let others = list(&dir)
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "properties") && p != source)
            .filter_map(|p| {
                let stem = p.file_stem()?.to_str()?;
                let language = if stem == base {
                    "default".to_string()
                } else {
                    stem.strip_prefix(&format!("{base}_"))
                        .filter(|l| is_locale(l))?
                        .to_string()
                };
                Some(LocaleFile { language, path: p })
            })
            .collect();
        (Layout::Suffix { dir, base }, language, others)
    } else if is_locale(&stem) {
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("json")
            .to_string();
        let others = list(&dir)
            .into_iter()
            .filter(|p| {
                p != source
                    && p.extension()
                        .is_some_and(|e| e.to_string_lossy() == extension)
            })
            .filter_map(|p| {
                let language = p.file_stem()?.to_str()?.to_string();
                is_locale(&language).then_some(LocaleFile { language, path: p })
            })
            .collect();
        (Layout::Stem { dir, extension }, stem, others)
    } else {
        let language = dir
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| is_locale(n))
            .ok_or_else(|| tr!("translations.no_language_in_path").to_string())?
            .to_string();
        let parent = dir.parent().unwrap_or(Path::new(".")).to_path_buf();
        let others = list(&parent)
            .into_iter()
            .filter(|p| p.is_dir() && *p != dir)
            .filter_map(|p| {
                let language = p.file_name()?.to_str()?.to_string();
                let file = p.join(&file_name);
                (is_locale(&language) && file.is_file()).then_some(LocaleFile {
                    language,
                    path: file,
                })
            })
            .collect();
        (
            Layout::Folder {
                dir: parent,
                file: file_name,
            },
            language,
            others,
        )
    };
    let mut others: Vec<LocaleFile> = others;
    others.sort_by(|a, b| a.language.cmp(&b.language));
    Ok(Catalog {
        format,
        layout,
        source: LocaleFile {
            language,
            path: source.to_path_buf(),
        },
        others,
    })
}

// ---- Reading ------------------------------------------------------------------------------

fn flatten(prefix: &str, value: &Value, out: &mut Vec<(String, String)>) {
    let key = |k: &str| {
        if prefix.is_empty() {
            k.to_string()
        } else {
            format!("{prefix}.{k}")
        }
    };
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                flatten(&key(k), v, out);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                flatten(&key(&i.to_string()), v, out);
            }
        }
        Value::String(s) => out.push((prefix.to_string(), s.clone())),
        Value::Null => out.push((prefix.to_string(), String::new())),
        other => out.push((prefix.to_string(), other.to_string())),
    }
}

/// The value of a `.properties` text: escapes resolved.
fn unescape_properties(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn escape_properties(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

/// The lines of a `.properties` file joined where they continue (`\` at the end).
fn properties_entries(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut logical = String::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if logical.is_empty() && (trimmed.is_empty() || trimmed.starts_with(['#', '!'])) {
            continue;
        }
        let continues = trimmed.ends_with('\\') && !trimmed.ends_with("\\\\");
        logical.push_str(if continues {
            &trimmed[..trimmed.len() - 1]
        } else {
            trimmed
        });
        if continues {
            continue;
        }
        let entry = std::mem::take(&mut logical);
        let split = entry.find(['=', ':']).unwrap_or(entry.len());
        let key = entry[..split].trim().to_string();
        let value = entry.get(split + 1..).unwrap_or("").trim_start();
        if !key.is_empty() {
            out.push((key, unescape_properties(value)));
        }
    }
    out
}

/// The keys and values of a file, in its order.
pub fn read(path: &Path, format: Format) -> Result<Vec<(String, String)>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    match format {
        Format::Json => {
            if text.trim().is_empty() {
                return Ok(Vec::new());
            }
            let value: Value =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut out = Vec::new();
            flatten("", &value, &mut out);
            Ok(out)
        }
        Format::Properties => Ok(properties_entries(&text)),
    }
}

/// The placeholders of a text (`{name}`, `{{count}}`, `%s`, `${x}`), sorted, to compare two
/// languages.
pub fn placeholders(text: &str) -> Vec<String> {
    let mut found: Vec<String> = PLACEHOLDER
        .find_iter(text)
        .map(|m| m.as_str().replace(' ', ""))
        .collect();
    found.sort();
    found
}

// ---- Checking -----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The source has the key, the language does not.
    Missing,
    /// The language has a key the source does not.
    Extra,
    Empty,
    /// The placeholders differ from the source's.
    Placeholders {
        source: Vec<String>,
        found: Vec<String>,
    },
    /// The same text as the source: maybe not translated.
    SameAsSource,
}

impl Problem {
    pub fn label(&self) -> String {
        match self {
            Problem::Missing => tr!("translations.problem_missing").into(),
            Problem::Extra => tr!("translations.problem_extra").into(),
            Problem::Empty => tr!("translations.problem_empty").into(),
            Problem::Placeholders { source, found } => trf!(
                "translations.problem_placeholders",
                source = source.join(" "),
                found = if found.is_empty() {
                    "—".to_string()
                } else {
                    found.join(" ")
                }
            ),
            Problem::SameAsSource => tr!("translations.problem_same").into(),
        }
    }

    /// The problems that make a translation wrong, not only doubtful.
    pub fn is_error(&self) -> bool {
        !matches!(self, Problem::SameAsSource)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub language: String,
    pub path: PathBuf,
    /// Keys of the source.
    pub total: usize,
    /// Keys of the source the language has, not empty.
    pub translated: usize,
    pub problems: Vec<(String, Problem)>,
}

impl Report {
    pub fn percent(&self) -> usize {
        (self.translated * 100)
            .checked_div(self.total)
            .unwrap_or(100)
    }

    pub fn errors(&self) -> usize {
        self.problems.iter().filter(|(_, p)| p.is_error()).count()
    }
}

/// A text worth translating: not a code, a number or a name that stays the same everywhere.
fn worth_translating(text: &str) -> bool {
    text.split_whitespace().count() > 1 && text.chars().filter(|c| c.is_alphabetic()).count() > 3
}

/// How a language compares to the source.
pub fn check_entries(
    source: &[(String, String)],
    target: &[(String, String)],
) -> (usize, Vec<(String, Problem)>) {
    let found: HashMap<&str, &str> = target
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let known: HashMap<&str, &str> = source
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let mut problems = Vec::new();
    let mut translated = 0;
    for (key, original) in source {
        match found.get(key.as_str()) {
            None => problems.push((key.clone(), Problem::Missing)),
            Some(value) if value.trim().is_empty() => {
                if !original.trim().is_empty() {
                    problems.push((key.clone(), Problem::Empty));
                }
            }
            Some(value) => {
                translated += 1;
                let (expected, have) = (placeholders(original), placeholders(value));
                if expected != have {
                    problems.push((
                        key.clone(),
                        Problem::Placeholders {
                            source: expected,
                            found: have,
                        },
                    ));
                } else if value == original && worth_translating(value) {
                    problems.push((key.clone(), Problem::SameAsSource));
                }
            }
        }
    }
    for (key, _) in target {
        if !known.contains_key(key.as_str()) {
            problems.push((key.clone(), Problem::Extra));
        }
    }
    (translated, problems)
}

pub fn check(catalog: &Catalog, language: &str) -> Result<Report, String> {
    let file = catalog
        .file(language)
        .ok_or_else(|| trf!("translations.unknown_language", language = language))?;
    let source = read(&catalog.source.path, catalog.format)?;
    let target = read(&file.path, catalog.format)?;
    let (translated, problems) = check_entries(&source, &target);
    Ok(Report {
        language: language.to_string(),
        path: file.path.clone(),
        total: source.len(),
        translated,
        problems,
    })
}

/// Every language but the source.
pub fn check_all(catalog: &Catalog) -> Vec<Result<Report, String>> {
    catalog
        .others
        .iter()
        .map(|f| check(catalog, &f.language))
        .collect()
}

// ---- Writing ------------------------------------------------------------------------------

/// The indentation of a JSON text: what its second line starts with, two spaces by default.
fn indentation(text: &str) -> String {
    text.lines()
        .nth(1)
        .map(|l| l[..l.len() - l.trim_start().len()].to_string())
        .filter(|i| !i.is_empty())
        .unwrap_or_else(|| "  ".to_string())
}

fn to_json(value: &Value, indent: &str, newline: bool) -> Result<String, String> {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    serde::Serialize::serialize(value, &mut serializer).map_err(|e| e.to_string())?;
    let mut text = String::from_utf8(out).map_err(|e| e.to_string())?;
    if newline {
        text.push('\n');
    }
    Ok(text)
}

/// Sets a dotted key in a JSON value: in the nested object that holds its parent when there is
/// one, else as a flat key of the deepest object found.
fn set_json(value: &mut Value, key: &str, text: &str) {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    let parts: Vec<&str> = key.split('.').collect();
    let mut node = value;
    let mut used = 0;
    while used + 1 < parts.len() {
        let next = node
            .as_object()
            .and_then(|m| m.get(parts[used]))
            .is_some_and(Value::is_object);
        if !next {
            break;
        }
        node = node
            .as_object_mut()
            .and_then(|m| m.get_mut(parts[used]))
            .expect("checked");
        used += 1;
    }
    let rest = parts[used..].join(".");
    if let Some(map) = node.as_object_mut() {
        map.insert(rest, Value::String(text.to_string()));
    }
}

/// Removes a dotted key from a JSON value, and the objects it leaves empty.
fn remove_json(value: &mut Value, key: &str) -> bool {
    let Some(map) = value.as_object_mut() else {
        return false;
    };
    if map.shift_remove(key).is_some() {
        return true;
    }
    if let Some((first, rest)) = key.split_once('.')
        && let Some(child) = map.get_mut(first)
        && remove_json(child, rest)
    {
        if child.as_object().is_some_and(Map::is_empty) {
            map.shift_remove(first);
        }
        return true;
    }
    false
}

/// Writes the value of a key in a file: replaced where it is, added at the end otherwise.
pub fn set_value(path: &Path, format: Format, key: &str, text: &str) -> Result<(), String> {
    let old = std::fs::read_to_string(path).unwrap_or_default();
    let new = match format {
        Format::Json => {
            let mut value: Value = if old.trim().is_empty() {
                Value::Object(Map::new())
            } else {
                serde_json::from_str(&old).map_err(|e| format!("{}: {e}", path.display()))?
            };
            set_json(&mut value, key, text);
            to_json(
                &value,
                &indentation(&old),
                old.is_empty() || old.ends_with('\n'),
            )?
        }
        Format::Properties => {
            let mut lines: Vec<String> = old.lines().map(str::to_string).collect();
            let entry = format!("{key}={}", escape_properties(text));
            let at = lines.iter().position(|l| {
                let t = l.trim_start();
                !t.starts_with(['#', '!'])
                    && t.find(['=', ':']).is_some_and(|i| t[..i].trim() == key)
            });
            match at {
                Some(i) => lines[i] = entry,
                None => lines.push(entry),
            }
            lines.join("\n") + "\n"
        }
    };
    std::fs::write(path, new).map_err(|e| format!("{}: {e}", path.display()))
}

/// Removes a key from a file.
pub fn remove_key(path: &Path, format: Format, key: &str) -> Result<(), String> {
    let old = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let new = match format {
        Format::Json => {
            let mut value: Value =
                serde_json::from_str(&old).map_err(|e| format!("{}: {e}", path.display()))?;
            remove_json(&mut value, key);
            to_json(&value, &indentation(&old), old.ends_with('\n'))?
        }
        Format::Properties => {
            let lines: Vec<&str> = old
                .lines()
                .filter(|l| {
                    let t = l.trim_start();
                    t.starts_with(['#', '!'])
                        || t.find(['=', ':']).is_none_or(|i| t[..i].trim() != key)
                })
                .collect();
            lines.join("\n") + "\n"
        }
    };
    std::fs::write(path, new).map_err(|e| format!("{}: {e}", path.display()))
}

/// What the strings of a new language start as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    /// Empty: the checks list them until they are translated.
    Empty,
    /// The text of the source, to translate in place.
    Source,
}

fn blank(value: &Value, fill: Fill) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), blank(v, fill)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(|v| blank(v, fill)).collect()),
        Value::String(_) if fill == Fill::Empty => Value::String(String::new()),
        other => other.clone(),
    }
}

/// Creates the file of a new language, with the keys of the source.
pub fn add_language(catalog: &Catalog, language: &str, fill: Fill) -> Result<PathBuf, String> {
    let language = language.trim();
    if !is_locale(language) {
        return Err(trf!("translations.invalid_language", language = language));
    }
    if catalog.file(language).is_some() {
        return Err(trf!("translations.language_exists", language = language));
    }
    let path = catalog.path_for(language);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let source = std::fs::read_to_string(&catalog.source.path).map_err(|e| e.to_string())?;
    let text = match catalog.format {
        Format::Json => {
            let value: Value = serde_json::from_str(&source).map_err(|e| e.to_string())?;
            to_json(&blank(&value, fill), &indentation(&source), true)?
        }
        Format::Properties => {
            let entries = properties_entries(&source);
            let mut out = String::new();
            for (key, value) in entries {
                let value = if fill == Fill::Empty {
                    String::new()
                } else {
                    value
                };
                out.push_str(&format!("{key}={}\n", escape_properties(&value)));
            }
            out
        }
    };
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

// ---- Configuration ------------------------------------------------------------------------

/// The source chosen for a project, relative to its root.
pub fn source_of(root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(root.join(CONFIG)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let source = root.join(value["source"].as_str()?);
    Some(source)
}

pub fn set_source(root: &Path, source: &Path) -> Result<(), String> {
    discover(source)?;
    let relative = source.strip_prefix(root).unwrap_or(source);
    let path = root.join(CONFIG);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut value: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| Value::Object(Map::new()));
    value["source"] = Value::String(relative.to_string_lossy().replace('\\', "/"));
    std::fs::write(&path, to_json(&value, "  ", true)?).map_err(|e| e.to_string())
}

/// Files that look like translations, to offer as the source: in a folder of languages, or
/// named after one.
pub fn candidates(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth == 0 || out.len() >= 40 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if !name.starts_with('.')
                    && !matches!(
                        name,
                        "node_modules" | "target" | "build" | "dist" | "out" | "vendor"
                    )
                {
                    walk(&path, depth - 1, out);
                }
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let in_locale_dir = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .is_some_and(is_locale);
            let looks = match path.extension().and_then(|e| e.to_str()) {
                Some("json") => is_locale(stem) || in_locale_dir,
                Some("properties") => stem.split_once('_').is_some_and(|(_, l)| is_locale(l)),
                _ => false,
            };
            if looks {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 6, &mut out);
    out
}

/// The text of a report, for the MCP tool and the notifications.
pub fn describe(report: &Report, source: &str) -> String {
    let mut out = trf!(
        "translations.report_line",
        language = report.language,
        percent = report.percent(),
        translated = report.translated,
        total = report.total,
        source = source
    );
    let mut by_kind: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for (key, problem) in &report.problems {
        by_kind.entry(problem.label()).or_default().push(key);
    }
    for (label, keys) in by_kind {
        out.push_str(&format!(
            "\n  {label} ({}): {}",
            keys.len(),
            keys.iter().take(50).copied().collect::<Vec<_>>().join(", ")
        ));
        if keys.len() > 50 {
            out.push_str(" …");
        }
    }
    out
}
