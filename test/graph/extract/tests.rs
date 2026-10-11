use super::*;

fn names(facts: &Facts) -> Vec<(&str, &str)> {
    facts
        .symbols
        .iter()
        .map(|s| (s.kind.as_str(), s.name.as_str()))
        .collect()
}

fn calls(facts: &Facts, kind: RefKind) -> Vec<(Option<&str>, &str)> {
    facts
        .refs
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| {
            (
                r.from.map(|f| facts.symbols[f].name.as_str()),
                r.name.as_str(),
            )
        })
        .collect()
}

#[test]
fn javascript_gives_its_definitions_imports_and_calls() {
    let facts = extract(
        "js",
        "import { add } from './math.js';\nconst fs = require('fs');\nexport { x } from './x';\n\
             function compute(a) { return add(a, helper(1)); }\n\
             class Counter extends Base {\n  increment() { this.total = add(this.total, 1); }\n}\n\
             const run = () => new Counter();\n",
    );
    assert_eq!(
        names(&facts),
        [
            ("function", "compute"),
            ("class", "Counter"),
            ("method", "Counter::increment"),
            ("function", "run")
        ]
    );
    assert_eq!(facts.imports, ["./math.js", "fs", "./x"]);
    let all = calls(&facts, RefKind::Calls);
    assert!(
        all.contains(&(Some("compute"), "add")) && all.contains(&(Some("compute"), "helper")),
        "{all:?}"
    );
    assert!(
        all.contains(&(Some("Counter::increment"), "add")),
        "{all:?}"
    );
    assert_eq!(calls(&facts, RefKind::Extends), [(Some("Counter"), "Base")]);
    assert_eq!(
        calls(&facts, RefKind::Instantiates),
        [(Some("run"), "Counter")]
    );
}

#[test]
fn typescript_adds_interfaces_types_enums_and_implements() {
    let facts = extract(
        "ts",
        "interface Shape extends Named { area(): number }\ntype Id = string;\nenum Color { Red }\n\
             export class Circle implements Shape { area() { return 1; } }\n",
    );
    let kinds = names(&facts);
    for expected in [
        ("interface", "Shape"),
        ("type", "Id"),
        ("enum", "Color"),
        ("class", "Circle"),
        ("method", "Circle::area"),
    ] {
        assert!(kinds.contains(&expected), "{kinds:?}");
    }
    assert_eq!(
        calls(&facts, RefKind::Implements),
        [(Some("Circle"), "Shape")]
    );
    assert_eq!(calls(&facts, RefKind::Extends), [(Some("Shape"), "Named")]);
}

#[test]
fn rust_gives_items_methods_uses_modules_and_calls() {
    let facts = extract(
        "rs",
        "mod server;\nmod inline { pub fn x() {} }\nuse crate::graph::{build, query::Hit};\nuse std::fs;\n\
             pub struct Store { n: u32 }\nimpl Store {\n    pub fn open() -> Self { Self::load() }\n    fn load() -> Self { helper() }\n}\n\
             trait Named { fn name(&self); }\nimpl Named for Store { fn name(&self) { self.open(); } }\n\
             fn helper() { Store::open(); }\n",
    );
    let kinds = names(&facts);
    for expected in [
        ("module", "server"),
        ("struct", "Store"),
        ("method", "Store::open"),
        ("method", "Store::load"),
        ("trait", "Named"),
        ("function", "helper"),
    ] {
        assert!(kinds.contains(&expected), "{kinds:?}");
    }
    assert_eq!(
        facts.imports,
        [
            "mod:server",
            "crate::graph::build",
            "crate::graph::query::Hit",
            "std::fs"
        ]
    );
    assert_eq!(
        calls(&facts, RefKind::Implements),
        [(Some("Store"), "Named")]
    );
    let all = calls(&facts, RefKind::Calls);
    assert!(all.contains(&(Some("Store::load"), "helper")), "{all:?}");
    assert!(all.contains(&(Some("helper"), "open")), "{all:?}");
    assert_eq!(
        facts
            .refs
            .iter()
            .find(|r| r.name == "open" && r.from.is_some_and(|f| facts.symbols[f].name == "helper"))
            .and_then(|r| r.qualifier.as_deref()),
        Some("Store")
    );
}

#[test]
fn markdown_gives_headings_and_relative_links_and_ignores_code() {
    let facts = extract(
        "md",
        "# Titre\n\nVoir [le guide](docs/guide.md#start) et [le site](https://x.org).\n\n```\n# pas un titre\n[x](nope.md)\n```\n\n## Suite\n",
    );
    assert_eq!(names(&facts), [("heading", "Titre"), ("heading", "Suite")]);
    assert_eq!(facts.imports, ["docs/guide.md"]);
}

#[test]
fn broken_code_still_gives_what_can_be_read() {
    let facts = extract("js", "function ok() {}\nfunction broken( {\nclass B {}\n");
    assert!(names(&facts).contains(&("function", "ok")));
    assert_eq!(language_of("TSX"), Some("tsx"));
    assert_eq!(language_of("png"), None);
}
