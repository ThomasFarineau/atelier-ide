//! Live templates, postfix templates and Surround With templates, the ones of the IDE and the
//! ones of the user, in `templates.json` of the data folder of Atelier (edited in Settings ›
//! Editor › Live Templates):
//!
//! ```json
//! [
//!   {"abbreviation": "fori", "description": "for loop", "languages": ["js", "ts"],
//!    "text": "for (let i = 0; i < $END$; i++) {\n}"},
//!   {"abbreviation": "log", "description": "log it", "languages": ["js"],
//!    "text": "console.log($EXPR$);"},
//!   {"description": "try / catch", "languages": ["js"], "text": "try {\n$SELECTION$\n} catch (error) {\n}"}
//! ]
//! ```
//!
//! A template with `$SELECTION$` surrounds the selected code (Ctrl+Alt+T). A template with `$EXPR$`
//! is a postfix one: typed after an expression and a dot (`value.log`), it takes the expression.
//! The others are offered by the completion when their abbreviation is typed. `$END$` is where the
//! caret goes, `$NAME$` a name for what the expression holds. No languages means all of them.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Template {
    pub abbreviation: String,
    pub description: String,
    pub languages: Vec<String>,
    pub text: String,
}

impl Template {
    pub fn new(abbreviation: &str, description: &str, languages: &[&str], text: &str) -> Self {
        Self {
            abbreviation: abbreviation.into(),
            description: description.into(),
            languages: languages.iter().map(|l| l.to_string()).collect(),
            text: text.into(),
        }
    }

    /// The keys of the postfix templates of the IDE for a language, as typed: `.if`, `.log`…
    pub fn postfix_keys(language: &str) -> Vec<String> {
        postfix(language)
            .into_iter()
            .map(|t| format!(".{}", t.abbreviation))
            .collect()
    }

    /// Whether it is for a language as `project::language` names it (`rs`, `js`).
    pub fn is_for(&self, language: &str) -> bool {
        self.languages.is_empty() || self.languages.iter().any(|l| l == language)
    }

    /// It surrounds the selection rather than being typed.
    pub fn surrounds(&self) -> bool {
        self.text.contains("$SELECTION$")
    }

    /// It is typed after an expression and a dot.
    pub fn is_postfix(&self) -> bool {
        self.text.contains("$EXPR$")
    }

    /// The text to insert and where the caret goes in it, with the markers taken out.
    pub fn expand(&self, selection: &str) -> (String, Option<usize>) {
        finish(self.text.replace("$SELECTION$", selection))
    }

    /// The same for a postfix template: `expression` is what came before the dot, `name` what to
    /// call a variable that holds it.
    pub fn expand_postfix(&self, expression: &str, name: &str) -> (String, Option<usize>) {
        finish(
            self.text
                .replace("$EXPR$", expression)
                .replace("$NAME$", name),
        )
    }
}

/// The text without its `$END$`, and where that was.
fn finish(text: String) -> (String, Option<usize>) {
    match text.find("$END$") {
        Some(at) => (text.replacen("$END$", "", 1).replace("$END$", ""), Some(at)),
        None => (text, None),
    }
}

pub fn parse(text: &str) -> Vec<Template> {
    serde_json::from_str(text).unwrap_or_default()
}

pub fn load(path: &Path) -> Vec<Template> {
    fs::read_to_string(path)
        .map(|text| parse(&text))
        .unwrap_or_default()
}

fn file() -> PathBuf {
    crate::cli::data_dir().join("templates.json")
}

static USER: RwLock<Option<Arc<Vec<Template>>>> = RwLock::new(None);

/// The templates of the user, read once and again after `reload` or `save`.
pub fn user() -> Arc<Vec<Template>> {
    if let Some(list) = USER.read().ok().and_then(|g| g.clone()) {
        return list;
    }
    let list = Arc::new(load(&file()));
    if let Ok(mut guard) = USER.write() {
        *guard = Some(list.clone());
    }
    list
}

/// Reads the file again (it was written by something else, an import).
pub fn reload() {
    if let Ok(mut guard) = USER.write() {
        *guard = None;
    }
}

/// Keeps `list` as the templates of the user.
pub fn save(list: &[Template]) -> std::io::Result<()> {
    let path = file();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, serde_json::to_string_pretty(list)?)?;
    reload();
    Ok(())
}

/// The postfix templates of the IDE for a language.
pub fn postfix(language: &str) -> Vec<Template> {
    let t = |key: &str, description: &str, languages: &[&str], text: &str| {
        Template::new(key, description, languages, text)
    };
    match language {
        "js" | "ts" | "jsx" | "tsx" => vec![
            t("if", "if (expr) { }", &[], "if ($EXPR$) {\n    $END$\n}"),
            t("else", "if (!expr) { }", &[], "if (!$EXPR$) {\n    $END$\n}"),
            t("not", "!expr", &[], "!$EXPR$"),
            t("var", "const name = expr;", &[], "const $NAME$ = $EXPR$;"),
            t("const", "const name = expr;", &[], "const $NAME$ = $EXPR$;"),
            t("let", "let name = expr;", &[], "let $NAME$ = $EXPR$;"),
            t("for", "for (const item of expr) { }", &[], "for (const item of $EXPR$) {\n    $END$\n}"),
            t("return", "return expr;", &[], "return $EXPR$;"),
            t("log", "console.log(expr);", &[], "console.log($EXPR$);"),
            t("await", "await expr", &[], "await $EXPR$"),
            t("null", "if (expr == null) { }", &[], "if ($EXPR$ == null) {\n    $END$\n}"),
            t("nn", "if (expr != null) { }", &[], "if ($EXPR$ != null) {\n    $END$\n}"),
            t("par", "(expr)", &[], "($EXPR$)"),
            t("throw", "throw expr;", &[], "throw $EXPR$;"),
        ],
        "rs" => vec![
            t("if", "if expr { }", &[], "if $EXPR$ {\n    $END$\n}"),
            t("else", "if !expr { }", &[], "if !$EXPR$ {\n    $END$\n}"),
            t("not", "!expr", &[], "!$EXPR$"),
            t("let", "let name = expr;", &[], "let $NAME$ = $EXPR$;"),
            t("var", "let name = expr;", &[], "let $NAME$ = $EXPR$;"),
            t("match", "match expr { }", &[], "match $EXPR$ {\n    $END$\n}"),
            t("for", "for item in expr { }", &[], "for item in $EXPR$ {\n    $END$\n}"),
            t("while", "while expr { }", &[], "while $EXPR$ {\n    $END$\n}"),
            t("return", "return expr;", &[], "return $EXPR$;"),
            t("dbg", "dbg!(expr)", &[], "dbg!($EXPR$)"),
            t("println", "println!(\"{}\", expr);", &[], "println!(\"{}\", $EXPR$);"),
            t("ref", "&expr", &[], "&$EXPR$"),
            t("refm", "&mut expr", &[], "&mut $EXPR$"),
            t("some", "Some(expr)", &[], "Some($EXPR$)"),
            t("ok", "Ok(expr)", &[], "Ok($EXPR$)"),
            t("box", "Box::new(expr)", &[], "Box::new($EXPR$)"),
        ],
        "java" | "kt" | "cs" => vec![
            t("if", "if (expr) { }", &[], "if ($EXPR$) {\n    $END$\n}"),
            t("else", "if (!expr) { }", &[], "if (!$EXPR$) {\n    $END$\n}"),
            t("not", "!expr", &[], "!$EXPR$"),
            t("var", "var name = expr;", &[], "var $NAME$ = $EXPR$;"),
            t("for", "for (var item : expr) { }", &[], "for (var item : $EXPR$) {\n    $END$\n}"),
            t("return", "return expr;", &[], "return $EXPR$;"),
            t("sout", "System.out.println(expr);", &["java"], "System.out.println($EXPR$);"),
            t("null", "if (expr == null) { }", &[], "if ($EXPR$ == null) {\n    $END$\n}"),
            t("nn", "if (expr != null) { }", &[], "if ($EXPR$ != null) {\n    $END$\n}"),
            t("par", "(expr)", &[], "($EXPR$)"),
            t("throw", "throw expr;", &[], "throw $EXPR$;"),
        ],
        "py" => vec![
            t("if", "if expr:", &[], "if $EXPR$:\n    $END$"),
            t("else", "if not expr:", &[], "if not $EXPR$:\n    $END$"),
            t("not", "not expr", &[], "not $EXPR$"),
            t("var", "name = expr", &[], "$NAME$ = $EXPR$"),
            t("for", "for item in expr:", &[], "for item in $EXPR$:\n    $END$"),
            t("return", "return expr", &[], "return $EXPR$"),
            t("print", "print(expr)", &[], "print($EXPR$)"),
            t("len", "len(expr)", &[], "len($EXPR$)"),
            t("par", "(expr)", &[], "($EXPR$)"),
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[path = "../../test/languages/templates/tests.rs"]
mod tests;
