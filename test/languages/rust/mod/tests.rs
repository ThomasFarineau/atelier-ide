use super::*;
use crate::languages::{DocRef, Provider, Severity};
use std::time::{Duration, Instant};

/// Polls a result until it comes, or gives up.
fn wait<T>(mut ask: impl FnMut() -> Option<T>, seconds: u64) -> Option<T> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if let Some(value) = ask() {
            return Some(value);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

/// Before the server answers, Reformat Code runs rustfmt itself.
#[test]
fn reformat_without_the_server_runs_rustfmt() {
    if format::rustfmt("", None).is_err() {
        return; // No rustfmt here.
    }
    let dir = std::env::temp_dir();
    let mut provider = provider(&dir, Arc::new(|| {}));
    let doc = DocRef {
        key: 7,
        path: None,
        language: "rs".into(),
    };
    let text = "fn main(){\nlet x=1;\n}\n";
    let answer = provider.format(&doc, text);
    let edits = wait(|| answer.take(), 20).expect("an answer").unwrap();
    assert_eq!(
        crate::languages::format::apply(text, &edits),
        "fn main() {\n    let x = 1;\n}\n"
    );
    let broken = provider.format(&doc, "fn main( {");
    assert!(wait(|| broken.take(), 20).expect("an answer").is_err());
}

/// With the real server: `cargo test -- --ignored rust_analyzer` (rust-analyzer installed).
#[test]
#[ignore]
fn rust_analyzer_answers_through_the_provider() {
    let dir = std::env::temp_dir().join(format!("atelier-ra-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/util.rs"),
        "pub fn twice(x: u32) -> u32 {\n    x * 2\n}\n",
    )
    .unwrap();
    let main = "mod util;\n\nfn main() {\n    let name = String::from(\"a\");\n    let n = util::twice(3);\n    name.le\n    let broken: u32 = \"x\";\n}\n";
    let path = dir.join("src/main.rs");
    std::fs::write(&path, main).unwrap();

    let mut provider = provider(&dir, Arc::new(|| {}));
    let doc = DocRef {
        key: 1,
        path: Some(path.clone()),
        language: "rs".into(),
    };
    // The server starts: found, launched, then indexing the crate.
    let ready = wait(
        || {
            provider.update(&doc, main, 1);
            provider.target(1).map(|_| ())
        },
        60,
    );
    assert!(ready.is_some(), "status: {}", provider.status());

    let dot = main.find("name.le").unwrap() + "name.le".len();
    let completion = wait(
        || {
            let answer = provider.complete(&doc, main, dot, true);
            wait(|| answer.take(), 20)
                .flatten()
                .filter(|c| c.items.iter().any(|i| i.label.starts_with("len")))
        },
        120,
    )
    .expect("the members of String");
    assert_eq!(completion.from, dot - 2);

    let twice = main.find("twice").unwrap() + 1;
    let places = wait(
        || {
            let answer = provider.definition(&doc, main, twice);
            wait(|| answer.take(), 20).filter(|p| !p.is_empty())
        },
        60,
    )
    .expect("the definition in util.rs");
    assert!(
        places[0]
            .path
            .as_deref()
            .is_some_and(|p| p.ends_with("util.rs")),
        "{places:?}"
    );

    let hover = wait(
        || {
            let answer = provider.hover(&doc, main, main.find("let n =").unwrap() + 4);
            wait(|| answer.take(), 20).flatten()
        },
        60,
    )
    .expect("a hover");
    assert!(hover.title.contains("u32"), "{hover:?}");

    let diagnostics = wait(
        || {
            let found = provider.diagnostics(1);
            found
                .iter()
                .any(|d| d.source != tr!("syntax"))
                .then(|| found.clone())
        },
        120,
    )
    .expect("diagnostics of the server");
    assert!(
        diagnostics.iter().any(|d| d.severity == Severity::Error),
        "{diagnostics:?}"
    );
    drop(provider);
    let _ = std::fs::remove_dir_all(&dir);
}
