use super::*;
use crate::languages::{DocRef, Provider, Severity};
use std::time::{Duration, Instant};

fn wait<T>(mut ask: impl FnMut() -> Option<T>, seconds: u64) -> Option<T> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if let Some(value) = ask() {
            return Some(value);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    None
}

/// With the real server: `cargo test -- --ignored jdtls` (a JDK 21 or later; jdtls is
/// downloaded into the IDE's folder when it is not there).
#[test]
#[ignore]
fn jdtls_answers_through_the_provider() {
    if server::launcher(&server::install_dir()).is_none() {
        server::download().expect("jdtls installed");
    }
    let dir = std::env::temp_dir().join(format!("atelier-jdtls-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let main = "public class Main {\n    static int twice(int x) { return x * 2; }\n\n    public static void main(String[] args) {\n        String name = \"a\";\n        int n = twice(3);\n        name.le\n        int broken = \"x\";\n    }\n}\n";
    let path = dir.join("Main.java");
    std::fs::write(&path, main).unwrap();
    let mut provider = provider(&dir, Arc::new(|| {}));
    let doc = DocRef {
        key: 1,
        path: Some(path.clone()),
        language: "java".into(),
    };
    let ready = wait(
        || {
            provider.update(&doc, main, 1);
            provider.target(1).map(|_| ())
        },
        180,
    );
    assert!(ready.is_some(), "status: {}", provider.status());
    let dot = main.find("name.le").unwrap() + "name.le".len();
    let completion = wait(
        || {
            let answer = provider.complete(&doc, main, dot, true);
            wait(|| answer.take(), 30)
                .flatten()
                .filter(|c| c.items.iter().any(|i| i.label.starts_with("length")))
        },
        240,
    )
    .expect("the members of String");
    assert_eq!(completion.from, dot - 2);
    let call = main.find("twice(3)").unwrap() + 1;
    let places = wait(
        || {
            let answer = provider.definition(&doc, main, call);
            wait(|| answer.take(), 30).filter(|p| !p.is_empty())
        },
        120,
    )
    .expect("the declaration of twice");
    assert_eq!(places[0].line, 1, "{places:?}");
    let diagnostics = wait(
        || {
            let found = provider.diagnostics(1);
            found
                .iter()
                .any(|d| d.source != tr!("syntax") && d.severity == Severity::Error)
                .then(|| found.clone())
        },
        240,
    )
    .expect("errors from jdtls");
    assert!(!diagnostics.is_empty());
    let messy = "public class Main{static int twice(int x){return x*2;}}\n";
    provider.update(&doc, messy, 2);
    let edits = wait(
        || {
            provider.update(&doc, messy, 2);
            let answer = provider.format(&doc, messy);
            wait(|| answer.take(), 30)
                .and_then(Result::ok)
                .filter(|e| !e.is_empty())
        },
        120,
    )
    .expect("formatting edits");
    let formatted = crate::languages::format::apply(messy, &edits);
    assert!(
        formatted.contains("static int twice(int x) {"),
        "{formatted}"
    );
    drop(provider);
    let _ = std::fs::remove_dir_all(server::data_dir(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}
