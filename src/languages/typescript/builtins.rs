//! What TypeScript adds to JavaScript without an import: keywords, the primitive and library
//! types, and code templates.

use crate::languages::{CompletionItem, ItemKind};

pub const KEYWORDS: &[&str] = &[
    "abstract",
    "accessor",
    "as",
    "asserts",
    "declare",
    "enum",
    "global",
    "implements",
    "infer",
    "interface",
    "is",
    "keyof",
    "module",
    "namespace",
    "override",
    "private",
    "protected",
    "public",
    "readonly",
    "satisfies",
    "type",
    "unique",
];

/// The primitive types: (name, documentation).
pub const PRIMITIVES: &[(&str, &str)] = &[
    ("string", "typescript.type.string"),
    ("number", "typescript.type.number"),
    ("boolean", "typescript.type.boolean"),
    ("any", "typescript.type.any"),
    ("unknown", "typescript.type.unknown"),
    ("never", "typescript.type.never"),
    ("void", "typescript.type.void"),
    ("null", "typescript.type.null"),
    ("undefined", "typescript.type.undefined"),
    ("object", "typescript.type.object"),
    ("symbol", "typescript.type.symbol"),
    ("bigint", "typescript.type.bigint"),
];

/// Types of the standard library and utility types: (name, documentation).
pub const LIBRARY_TYPES: &[(&str, &str)] = &[
    ("Array", "typescript.type.Array"),
    ("ReadonlyArray", "typescript.type.ReadonlyArray"),
    ("Promise", "typescript.type.Promise"),
    ("Record", "typescript.type.Record"),
    ("Partial", "typescript.type.Partial"),
    ("Required", "typescript.type.Required"),
    ("Readonly", "typescript.type.Readonly"),
    ("Pick", "typescript.type.Pick"),
    ("Omit", "typescript.type.Omit"),
    ("Exclude", "typescript.type.Exclude"),
    ("Extract", "typescript.type.Extract"),
    ("NonNullable", "typescript.type.NonNullable"),
    ("ReturnType", "typescript.type.ReturnType"),
    ("Parameters", "typescript.type.Parameters"),
    ("InstanceType", "typescript.type.InstanceType"),
    ("Awaited", "typescript.type.Awaited"),
    ("Map", "typescript.type.Map"),
    ("Set", "typescript.type.Set"),
    ("WeakMap", "typescript.type.WeakMap"),
    ("WeakSet", "typescript.type.WeakSet"),
    ("Date", "typescript.type.Date"),
    ("RegExp", "typescript.type.RegExp"),
    ("Error", "typescript.type.Error"),
    ("Function", "typescript.type.Function"),
    ("Object", "typescript.type.Object"),
    ("Uppercase", "typescript.type.Uppercase"),
    ("Lowercase", "typescript.type.Lowercase"),
    ("Capitalize", "typescript.type.Capitalize"),
];

/// Code templates for TypeScript: (trigger, label, text, caret offset in the text, description).
pub const SNIPPETS: &[(&str, &str, &str, usize, &str)] = &[
    (
        "interface",
        "interface",
        "interface Name {\n    \n}",
        10,
        "typescript.snippet.interface",
    ),
    (
        "type",
        "type",
        "type Name = ;",
        12,
        "typescript.snippet.type",
    ),
    (
        "enum",
        "enum",
        "enum Name {\n    \n}",
        5,
        "typescript.snippet.enum",
    ),
    (
        "tclass",
        "tclass",
        "class Name {\n    constructor(private readonly value: string) {}\n}",
        6,
        "typescript.snippet.tclass",
    ),
    (
        "tfunc",
        "tfunc",
        "function name(param: string): void {\n    \n}",
        9,
        "typescript.snippet.tfunc",
    ),
    (
        "tarrow",
        "tarrow",
        "const name = (param: string): void => {\n    \n};",
        6,
        "typescript.snippet.tarrow",
    ),
    (
        "atry",
        "atry",
        "try {\n    \n} catch (error: unknown) {\n    \n}",
        11,
        "typescript.snippet.atry",
    ),
    (
        "generic",
        "generic",
        "function name<T>(value: T): T {\n    return value;\n}",
        9,
        "typescript.snippet.generic",
    ),
];

/// Keywords and templates of TypeScript, for the places where a value or a statement goes.
pub fn items() -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = KEYWORDS
        .iter()
        .map(|k| CompletionItem::new(*k, ItemKind::Keyword).rank(70))
        .collect();
    for (_, label, text, caret, doc) in SNIPPETS {
        items.push(
            CompletionItem::new(*label, ItemKind::Snippet)
                .detail(crate::i18n::t(doc))
                .insert(*text, Some(*caret))
                .rank(80),
        );
    }
    items
}

/// The types that exist without a declaration, for the places where a type goes.
pub fn type_items() -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = PRIMITIVES
        .iter()
        .map(|(name, doc)| {
            CompletionItem::new(*name, ItemKind::Keyword)
                .doc(crate::i18n::t(doc))
                .rank(40)
        })
        .collect();
    for (name, doc) in LIBRARY_TYPES {
        items.push(
            CompletionItem::new(*name, ItemKind::Interface)
                .doc(crate::i18n::t(doc))
                .rank(45),
        );
    }
    for keyword in ["typeof", "keyof", "readonly", "unique", "infer"] {
        items.push(CompletionItem::new(keyword, ItemKind::Keyword).rank(70));
    }
    items
}

/// The documentation of a primitive or library type.
pub fn type_doc(name: &str) -> Option<&'static str> {
    PRIMITIVES
        .iter()
        .chain(LIBRARY_TYPES.iter())
        .find(|(n, _)| *n == name)
        .map(|(_, doc)| crate::i18n::t(doc))
}

/// Whether the name is a type of the language or its library (no import needed).
pub fn is_library_type(name: &str) -> bool {
    type_doc(name).is_some()
}
