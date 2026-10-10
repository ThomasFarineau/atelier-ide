//! The translations of a project, from a source chosen by the user: the files of the reference
//! language and, found from them, those of the other languages. They are read as lists of keys,
//! checked against the source and written back key by key.
//!
//! The source is one of:
//! - a file. The other languages are found next to it, the language being in the name of a file
//!   or of a folder: `locales/fr_FR.json` and `locales/en.json` (one file per language),
//!   `locales/fr/common.json` and `locales/en/common.json` (one folder per language),
//!   `messages.properties` and `messages_fr.properties` (Java bundles, the file without a suffix
//!   being the default language);
//! - the folder of a language (`locales/fr`): every file in it is a namespace, and its keys are
//!   `namespace:key` (`common:menu.open`), as with i18next. The other languages are the folders
//!   next to it;
//! - a regular expression on the paths of the project's files, relative to it with `/`, whose
//!   group `lang` is the language and the optional group `ns` the namespace:
//!   `src/i18n/(?<lang>[a-z]{2})\.json`, `app/(?<ns>\w+)/lang/(?<lang>\w+)\.properties`. The
//!   reference language is then named too.
//!
//! JSON files may be flat (`"menu.open": "…"`) or nested (`{"menu": {"open": "…"}}`): the keys
//! are the dotted paths of the strings, and a new key goes where its parent is.

use regex::Regex;
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::{MAIN_SEPARATOR_STR, Path, PathBuf},
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

/// What the user chose as the source of the translations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The file of the reference language.
    File(PathBuf),
    /// The folder of the reference language, each file in it a namespace.
    Folder(PathBuf),
    /// A regular expression on the paths of the files of `root`, relative to it.
    Pattern {
        root: PathBuf,
        pattern: String,
        /// The reference language; empty: English if there is, else the first one.
        language: String,
    },
}

impl Source {
    /// A file or a folder, as it is on the disk.
    pub fn from_path(path: &Path) -> Source {
        if path.is_dir() {
            Source::Folder(path.to_path_buf())
        } else {
            Source::File(path.to_path_buf())
        }
    }

    /// How it is shown: its path relative to `root`, or its pattern.
    pub fn label(&self, root: &Path) -> String {
        let relative = |path: &Path| {
            path.strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        };
        match self {
            Source::File(path) => relative(path),
            Source::Folder(path) => format!("{}/", relative(path)),
            Source::Pattern { pattern, .. } => format!("/{pattern}/"),
        }
    }
}

/// One file of a language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// Empty when the catalog has no namespaces.
    pub namespace: String,
    pub path: PathBuf,
    pub format: Format,
}

/// The files of one language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocaleFile {
    pub language: String,
    pub parts: Vec<Part>,
}

impl LocaleFile {
    /// Its first file, to show or open.
    pub fn path(&self) -> &Path {
        self.parts.first().map_or(Path::new(""), |p| &p.path)
    }
}

/// Where the file of a namespace is for any language: `before` + language + `after`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Template {
    namespace: String,
    format: Format,
    before: String,
    after: String,
}

impl Template {
    fn path(&self, language: &str) -> PathBuf {
        PathBuf::from(format!("{}{language}{}", self.before, self.after))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub source: LocaleFile,
    /// The other languages, sorted.
    pub others: Vec<LocaleFile>,
    /// Whether the keys start with their namespace (`common:menu.open`).
    pub namespaced: bool,
    templates: Vec<Template>,
}

impl Catalog {
    /// The source first, then the other languages.
    pub fn files(&self) -> impl Iterator<Item = &LocaleFile> {
        std::iter::once(&self.source).chain(&self.others)
    }

    pub fn file(&self, language: &str) -> Option<&LocaleFile> {
        self.files().find(|f| f.language == language)
    }

    /// The first file a language has, or would have.
    #[cfg(test)]
    pub fn path_for(&self, language: &str) -> PathBuf {
        self.templates
            .first()
            .map_or_else(PathBuf::new, |t| t.path(language))
    }

    /// Every file of the catalog.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.files()
            .flat_map(|f| f.parts.iter().map(|p| p.path.as_path()))
    }

    /// The folders a new language would appear in, to watch.
    pub fn folders(&self) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = self
            .templates
            .iter()
            .filter_map(|t| {
                // The folder holding the file or folder named after the language.
                let marked = t.path("\u{1}");
                let mut path = marked.as_path();
                while !path.file_name()?.to_string_lossy().contains('\u{1}') {
                    path = path.parent()?;
                }
                path.parent().map(Path::to_path_buf)
            })
            .collect();
        folders.dedup();
        folders
    }

    /// The namespace of a key and the key in its file.
    fn split<'a>(&self, key: &'a str) -> Result<(&'a str, &'a str), String> {
        if !self.namespaced {
            return Ok(("", key));
        }
        key.split_once(':')
            .ok_or_else(|| trf!("translations.key_needs_namespace", key = key))
    }

    /// The file a key of a language is, or would be, in: its path, format and key in it.
    pub fn locate(&self, language: &str, key: &str) -> Result<(PathBuf, Format, String), String> {
        let (namespace, inner) = self.split(key)?;
        let part = self
            .file(language)
            .and_then(|f| f.parts.iter().find(|p| p.namespace == namespace));
        if let Some(part) = part {
            return Ok((part.path.clone(), part.format, inner.to_string()));
        }
        let template = self
            .templates
            .iter()
            .find(|t| t.namespace == namespace)
            .ok_or_else(|| trf!("translations.unknown_namespace", namespace = namespace))?;
        Ok((template.path(language), template.format, inner.to_string()))
    }

    /// Every key and text of a language, its namespaces after one another.
    pub fn read(&self, file: &LocaleFile) -> Result<Vec<(String, String)>, String> {
        let mut out = Vec::new();
        for part in &file.parts {
            for (key, value) in read(&part.path, part.format)? {
                let key = if self.namespaced {
                    format!("{}:{key}", part.namespace)
                } else {
                    key
                };
                out.push((key, value));
            }
        }
        Ok(out)
    }

    /// Writes the text of a key in a language.
    pub fn set(&self, language: &str, key: &str, text: &str) -> Result<(), String> {
        let (path, format, inner) = self.locate(language, key)?;
        set_value(&path, format, &inner, text)
    }

    /// Removes a key from every language.
    pub fn remove(&self, key: &str) -> Result<(), String> {
        let (namespace, inner) = self.split(key)?;
        for file in self.files() {
            for part in file.parts.iter().filter(|p| p.namespace == namespace) {
                remove_key(&part.path, part.format, inner)?;
            }
        }
        Ok(())
    }
}

fn format_of(path: &Path) -> Option<Format> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => Some(Format::Json),
        Some("properties") => Some(Format::Properties),
        _ => None,
    }
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    found.sort();
    found
}

/// `dir` and a separator, to put a name after.
fn prefix(dir: &Path) -> String {
    let dir = dir.to_string_lossy();
    if dir.ends_with(['/', '\\']) || dir.is_empty() {
        dir.into_owned()
    } else {
        format!("{dir}{MAIN_SEPARATOR_STR}")
    }
}

fn single(language: String, path: PathBuf, format: Format) -> LocaleFile {
    LocaleFile {
        language,
        parts: vec![Part {
            namespace: String::new(),
            path,
            format,
        }],
    }
}

/// The catalog of a source: its files, and those of the other languages.
pub fn discover(source: &Source) -> Result<Catalog, String> {
    let mut catalog = match source {
        Source::File(path) => discover_file(path)?,
        Source::Folder(path) => discover_folder(path)?,
        Source::Pattern {
            root,
            pattern,
            language,
        } => discover_pattern(root, pattern, language)?,
    };
    catalog.others.sort_by(|a, b| a.language.cmp(&b.language));
    Ok(catalog)
}

fn discover_file(source: &Path) -> Result<Catalog, String> {
    if !source.is_file() {
        return Err(trf!("translations.source_missing", path = source.display()));
    }
    let format = format_of(source).ok_or(tr!("translations.unsupported_format"))?;
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
    let template = |before: String, after: String| Template {
        namespace: String::new(),
        format,
        before,
        after,
    };
    let (template, language, others) = if format == Format::Properties {
        // `messages_fr_FR.properties`: the base is what comes before the language.
        let (base, language) = match stem.split_once('_') {
            Some((base, rest)) if is_locale(rest) => (base.to_string(), rest.to_string()),
            _ => (stem.clone(), "default".to_string()),
        };
        let others = sorted_entries(&dir)
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
                Some(single(language, p, format))
            })
            .collect();
        let template = template(format!("{}{base}_", prefix(&dir)), ".properties".into());
        (template, language, others)
    } else if is_locale(&stem) {
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("json")
            .to_string();
        let others = sorted_entries(&dir)
            .into_iter()
            .filter(|p| {
                p != source
                    && p.extension()
                        .is_some_and(|e| e.to_string_lossy() == extension)
            })
            .filter_map(|p| {
                let language = p.file_stem()?.to_str()?.to_string();
                is_locale(&language).then(|| single(language, p, format))
            })
            .collect();
        (
            template(prefix(&dir), format!(".{extension}")),
            stem,
            others,
        )
    } else {
        let language = dir
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| is_locale(n))
            .ok_or_else(|| tr!("translations.no_language_in_path").to_string())?
            .to_string();
        let parent = dir.parent().unwrap_or(Path::new(".")).to_path_buf();
        let others = sorted_entries(&parent)
            .into_iter()
            .filter(|p| p.is_dir() && *p != dir)
            .filter_map(|p| {
                let language = p.file_name()?.to_str()?.to_string();
                let file = p.join(&file_name);
                (is_locale(&language) && file.is_file()).then(|| single(language, file, format))
            })
            .collect();
        let after = format!("{MAIN_SEPARATOR_STR}{file_name}");
        (template(prefix(&parent), after), language, others)
    };
    Ok(Catalog {
        source: single(language, source.to_path_buf(), format),
        others,
        namespaced: false,
        templates: vec![template],
    })
}

/// The translation files under `dir`, with their path relative to it without the extension.
fn namespaces(dir: &Path) -> Vec<Part> {
    fn walk(base: &Path, dir: &Path, depth: usize, out: &mut Vec<Part>) {
        for path in sorted_entries(dir) {
            if path.is_dir() {
                if depth > 0 {
                    walk(base, &path, depth - 1, out);
                }
            } else if let Some(format) = format_of(&path) {
                let relative = path.strip_prefix(base).unwrap_or(&path).with_extension("");
                out.push(Part {
                    namespace: relative.to_string_lossy().replace('\\', "/"),
                    path,
                    format,
                });
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, 4, &mut out);
    out
}

fn discover_folder(source: &Path) -> Result<Catalog, String> {
    if !source.is_dir() {
        return Err(trf!("translations.source_missing", path = source.display()));
    }
    let language = source
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| is_locale(n))
        .ok_or_else(|| tr!("translations.folder_not_language").to_string())?
        .to_string();
    let parts = namespaces(source);
    if parts.is_empty() {
        return Err(tr!("translations.folder_empty").into());
    }
    let parent = source.parent().unwrap_or(Path::new(".")).to_path_buf();
    let templates = parts
        .iter()
        .map(|part| {
            let relative = part.path.strip_prefix(source).unwrap_or(&part.path);
            Template {
                namespace: part.namespace.clone(),
                format: part.format,
                before: prefix(&parent),
                after: format!("{MAIN_SEPARATOR_STR}{}", relative.to_string_lossy()),
            }
        })
        .collect();
    let others = sorted_entries(&parent)
        .into_iter()
        .filter(|p| p.is_dir() && p != source)
        .filter_map(|p| {
            let language = p.file_name()?.to_str()?.to_string();
            is_locale(&language).then(|| LocaleFile {
                language,
                parts: namespaces(&p),
            })
        })
        .collect();
    Ok(Catalog {
        source: LocaleFile { language, parts },
        others,
        namespaced: true,
        templates,
    })
}

/// The files of the project, relative to it with `/`, but those of tools and builds.
fn project_files(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
        for path in sorted_entries(dir) {
            if out.len() >= 50_000 {
                return;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                let skipped = name.starts_with('.')
                    || matches!(name, "node_modules" | "target" | "build" | "dist" | "out");
                if depth > 0 && !skipped {
                    walk(root, &path, depth - 1, out);
                }
            } else if format_of(&path).is_some()
                && let Ok(relative) = path.strip_prefix(root)
            {
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, 12, &mut out);
    out
}

/// The pattern as a whole-path regular expression; it needs a group `lang`.
pub fn compile_pattern(pattern: &str) -> Result<Regex, String> {
    let regex = Regex::new(&format!("^(?:{pattern})$"))
        .map_err(|e| trf!("translations.invalid_pattern", e = e))?;
    if !regex.capture_names().flatten().any(|n| n == "lang") {
        return Err(tr!("translations.pattern_needs_lang").into());
    }
    Ok(regex)
}

fn discover_pattern(root: &Path, pattern: &str, language: &str) -> Result<Catalog, String> {
    let regex = compile_pattern(pattern)?;
    let has_namespace = regex.capture_names().flatten().any(|n| n == "ns");
    // language → (namespace, relative path, span of the language in it).
    let mut found: BTreeMap<String, Vec<(String, String, std::ops::Range<usize>)>> =
        BTreeMap::new();
    for relative in project_files(root) {
        let Some(captures) = regex.captures(&relative) else {
            continue;
        };
        let Some(lang) = captures.name("lang").filter(|m| !m.as_str().is_empty()) else {
            continue;
        };
        let namespace = captures.name("ns").map_or("", |m| m.as_str()).to_string();
        let span = lang.range();
        found.entry(lang.as_str().to_string()).or_default().push((
            namespace,
            relative.clone(),
            span,
        ));
    }
    if found.is_empty() {
        return Err(trf!(
            "translations.pattern_matches_nothing",
            pattern = pattern
        ));
    }
    // Without a group `ns`, several files of one language are told apart by their path.
    let several = found.values().any(|files| files.len() > 1);
    let namespaced = has_namespace || several;
    let reference = if !language.trim().is_empty() {
        language.trim().to_string()
    } else {
        ["en", "en_US", "en-US", "en_GB", "en-GB"]
            .iter()
            .find(|l| found.contains_key(**l))
            .map_or_else(
                || found.keys().next().cloned().unwrap_or_default(),
                |l| l.to_string(),
            )
    };
    let root_prefix = prefix(root);
    let namespace_of = |namespace: &str, relative: &str, span: &std::ops::Range<usize>| {
        if has_namespace || !several {
            namespace.to_string()
        } else {
            format!(
                "{}{{lang}}{}",
                &relative[..span.start],
                &relative[span.end..]
            )
        }
    };
    let mut languages: Vec<LocaleFile> = Vec::new();
    let mut templates = Vec::new();
    for (lang, files) in &found {
        let mut parts = Vec::new();
        for (namespace, relative, span) in files {
            let path = root.join(relative);
            let format = format_of(&path).unwrap_or(Format::Json);
            let namespace = namespace_of(namespace, relative, span);
            if *lang == reference {
                templates.push(Template {
                    namespace: namespace.clone(),
                    format,
                    before: format!("{root_prefix}{}", &relative[..span.start]),
                    after: relative[span.end..].to_string(),
                });
            }
            parts.push(Part {
                namespace,
                path,
                format,
            });
        }
        languages.push(LocaleFile {
            language: lang.clone(),
            parts,
        });
    }
    let index = languages
        .iter()
        .position(|f| f.language == reference)
        .ok_or_else(|| trf!("translations.unknown_language", language = reference))?;
    let source = languages.remove(index);
    Ok(Catalog {
        source,
        others: languages,
        namespaced,
        templates,
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
    let source = catalog.read(&catalog.source)?;
    let target = catalog.read(file)?;
    let (translated, problems) = check_entries(&source, &target);
    Ok(Report {
        language: language.to_string(),
        path: file.path().to_path_buf(),
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
    // The file of a namespace a language did not have yet.
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
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

/// Creates the files of a new language, with the keys of the source; the files written.
pub fn add_language(catalog: &Catalog, language: &str, fill: Fill) -> Result<Vec<PathBuf>, String> {
    let language = language.trim();
    if !is_locale(language) {
        return Err(trf!("translations.invalid_language", language = language));
    }
    if catalog.file(language).is_some() {
        return Err(trf!("translations.language_exists", language = language));
    }
    let mut written = Vec::new();
    for template in &catalog.templates {
        let Some(part) = catalog
            .source
            .parts
            .iter()
            .find(|p| p.namespace == template.namespace)
        else {
            continue;
        };
        let path = template.path(language);
        new_file(&part.path, &path, part.format, fill)?;
        written.push(path);
    }
    Ok(written)
}

/// Writes at `path` the keys of `source`, without their texts or with them.
fn new_file(source: &Path, path: &Path, format: Format, fill: Fill) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let source = std::fs::read_to_string(source).map_err(|e| e.to_string())?;
    let text = match format {
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
    std::fs::write(path, text).map_err(|e| e.to_string())
}

// ---- Configuration ------------------------------------------------------------------------

/// The source chosen for a project: `{"source": "locales/fr.json"}` (a file or a folder,
/// relative to the project) or `{"pattern": "…", "language": "fr"}`.
pub fn source_of(root: &Path) -> Option<Source> {
    let text = std::fs::read_to_string(root.join(CONFIG)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    if let Some(pattern) = value["pattern"].as_str() {
        return Some(Source::Pattern {
            root: root.to_path_buf(),
            pattern: pattern.to_string(),
            language: value["language"].as_str().unwrap_or("").to_string(),
        });
    }
    Some(Source::from_path(&root.join(value["source"].as_str()?)))
}

/// Keeps `source` as the one of the project, once it is known to be one; its catalog.
pub fn set_source(root: &Path, source: &Source) -> Result<Catalog, String> {
    let catalog = discover(source)?;
    let path = root.join(CONFIG);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut value: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Map::new()));
    if let Some(object) = value.as_object_mut() {
        for key in ["source", "pattern", "language"] {
            object.remove(key);
        }
    }
    match source {
        Source::File(file) | Source::Folder(file) => {
            let relative = file.strip_prefix(root).unwrap_or(file);
            value["source"] = Value::String(relative.to_string_lossy().replace('\\', "/"));
        }
        Source::Pattern {
            pattern, language, ..
        } => {
            value["pattern"] = Value::String(pattern.clone());
            if !language.trim().is_empty() {
                value["language"] = Value::String(language.trim().to_string());
            }
        }
    }
    std::fs::write(&path, to_json(&value, "  ", true)?).map_err(|e| e.to_string())?;
    Ok(catalog)
}

/// Files that look like translations, to offer as the source: in a folder of languages, or
/// named after one; a folder of a language with several files is offered whole.
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
        let language_folder = dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(is_locale);
        if language_folder && namespaces(dir).len() > 1 {
            out.push(dir.to_path_buf());
            return;
        }
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
