use super::*;
use crate::languages::ItemKind;

#[test]
fn hover_markdown_is_split_into_signature_and_text() {
    let markdown = "```typescript\nfunction add(a: number, b: number): number\n```\n\nAdds **two** [numbers](https://x.y).\n\n*@param* `a` — first";
    let (title, body) = split_hover(markdown);
    assert_eq!(title, "function add(a: number, b: number): number");
    assert!(body.starts_with("Adds two numbers."), "{body}");
    assert!(body.contains("@param a — first"), "{body}");
    let (title, body) = split_hover("Just a sentence.\nMore.");
    assert_eq!(
        (title.as_str(), body.as_str()),
        ("Just a sentence.", "More.")
    );
}

#[test]
fn server_completions_become_items_in_server_order() {
    let text = "const x = [];\nx.pu";
    let result = json!({"isIncomplete": false, "items": [
        {"label": "push", "kind": 2, "sortText": "1", "detail": "(...items: any[]): number", "documentation": {"kind": "markdown", "value": "Appends **items**."}},
        {"label": "length", "kind": 10, "sortText": "0"},
        {"label": "toString", "kind": 2, "sortText": "2", "insertText": "toString"},
    ]});
    let from = text.len() - 2;
    let completion = completion_from_server(&result, text, from).unwrap();
    let labels: Vec<&str> = completion.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["length", "push", "toString"], "sorted by sortText");
    assert_eq!(completion.from, from);
    let push = &completion.items[1];
    assert_eq!(push.insert, "push()");
    assert_eq!(push.caret, Some(5));
    assert_eq!(push.doc.as_deref(), Some("Appends items."));
    assert_eq!(completion.items[0].kind, ItemKind::Property);
    assert!(completion_from_server(&json!({"items": []}), text, from).is_none());
}

#[test]
fn the_provider_falls_back_to_the_local_analysis_without_a_server() {
    let mut provider = JavaScript::new(Path::new("."), Arc::new(|| {}));
    provider.server = Server::Missing;
    let doc = DocRef {
        key: 1,
        path: None,
        language: "js".into(),
    };
    let text = "const total = 5;\nconsole.log(tot";
    provider.update(&doc, text, 1);
    let completion = provider
        .complete(&doc, text, text.len(), false)
        .take()
        .unwrap()
        .unwrap();
    assert!(completion.items.iter().any(|i| i.label == "total"));
    let broken = "const a = ;\n";
    provider.update(&doc, broken, 2);
    assert_eq!(provider.diagnostics(1).len(), 1);
    assert_eq!(provider.status(), "JS : analyse locale");
    let info = provider
        .hover(&doc, "const total = 5;\ntotal;", 18)
        .take()
        .unwrap()
        .unwrap();
    assert_eq!(info.title, "const total: number");
}

fn wait_for(provider: &mut JavaScript, what: &str, mut done: impl FnMut(&mut JavaScript) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !done(provider) {
        assert!(
            std::time::Instant::now() < deadline,
            "timeout waiting for {what}: {}",
            provider.status()
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Talks to a real typescript-language-server when one is installed (here, or in
/// `ATELIER_LSP_DIR`); otherwise there is nothing to check.
#[test]
fn the_real_server_answers_completion_hover_and_diagnostics() {
    let root = std::env::temp_dir().join(format!("atelier-js-lsp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    if server::launch(&root).is_none() || !server::node_available() {
        return;
    }
    let file = root.join("a.js");
    let text = "/** Adds.
 * @param {number} a
 * @param {number} b
 */
function add(a, b) { return a + b; }
const greeting = 'x';
greeting.toUp;
add(1, 2);
let broken = ;
";
    std::fs::write(&file, text).unwrap();
    let mut provider = JavaScript::new(&root, Arc::new(|| {}));
    let doc = DocRef {
        key: 7,
        path: Some(file.clone()),
        language: "js".into(),
    };
    provider.update(&doc, text, 1);
    wait_for(&mut provider, "the server", |p| {
        p.client().is_some_and(|c| c.ready())
    });
    provider.update(&doc, text, 2);

    let offset = text.find("greeting.toUp").unwrap() + "greeting.toUp".len();
    let answer = provider.complete(&doc, text, offset, false);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let completion = loop {
        if let Some(result) = answer.take() {
            break result;
        }
        assert!(std::time::Instant::now() < deadline, "no completion");
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let completion = completion.expect("completion items");
    let upper = completion
        .items
        .iter()
        .find(|i| i.label == "toUpperCase")
        .unwrap_or_else(|| {
            panic!(
                "no toUpperCase in {} items: {:?}",
                completion.items.len(),
                completion
                    .items
                    .iter()
                    .take(20)
                    .map(|i| i.label.clone())
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(completion.from, offset - 4);
    assert_eq!(upper.insert, "toUpperCase()");

    let at = text.find("add(1, 2)").unwrap();
    let answer = provider.hover(&doc, text, at);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let info = loop {
        if let Some(result) = answer.take() {
            break result;
        }
        assert!(std::time::Instant::now() < deadline, "no hover");
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let info = info.expect("a hover");
    assert!(
        info.title.contains("add(a: number, b: number)"),
        "{}",
        info.title
    );

    wait_for(&mut provider, "diagnostics", |p| {
        !p.diagnostics(7).is_empty()
    });
    let diagnostics = provider.diagnostics(7);
    assert!(
        diagnostics.iter().any(|d| d.severity == Severity::Error),
        "{diagnostics:?}"
    );
    assert_eq!(provider.status(), "tsserver");
    drop(provider);
    let _ = std::fs::remove_dir_all(&root);
}

fn wait_answer<T>(answer: &Oneshot<T>, what: &str) -> T {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        if let Some(value) = answer.take() {
            return value;
        }
        assert!(std::time::Instant::now() < deadline, "no answer for {what}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Definition, usages, structure and import actions from a real server.
#[test]
fn the_real_server_navigates_and_offers_imports() {
    let root = std::env::temp_dir().join(format!("atelier-js-lsp-nav-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    if server::launch(&root).is_none() || !server::node_available() {
        return;
    }
    std::fs::write(root.join("jsconfig.json"), "{}").unwrap();
    std::fs::write(
        root.join("util.js"),
        "export function helper() { return 1; }
",
    )
    .unwrap();
    let file = root.join("main.js");
    let text = "function add(a, b) { return a + b; }
const total = add(1, 2);
console.log(total, add(3, 4));
helper();
";
    std::fs::write(&file, text).unwrap();
    let mut provider = JavaScript::new(&root, Arc::new(|| {}));
    let doc = DocRef {
        key: 9,
        path: Some(file.clone()),
        language: "js".into(),
    };
    provider.update(&doc, text, 1);
    wait_for(&mut provider, "the server", |p| {
        p.client().is_some_and(|c| c.ready())
    });
    provider.update(&doc, text, 2);

    // Definition of the first use of `add`.
    let at = text.find("add(1, 2)").unwrap() + 1;
    let found = wait_answer(&provider.definition(&doc, text, at), "definition");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].line, found[0].column), (0, 9));

    // Usages of `add`, from the declaration.
    let declared = text.find("add(a").unwrap() + 1;
    let usages = wait_answer(&provider.references(&doc, text, declared), "references");
    assert_eq!(
        usages.iter().map(|l| l.line).collect::<Vec<_>>(),
        [1, 2],
        "{usages:?}"
    );

    // The structure.
    let symbols = wait_answer(&provider.symbols(&doc, text), "symbols");
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "add" && s.kind == ItemKind::Function),
        "{symbols:?}"
    );
    assert!(symbols.iter().any(|s| s.name == "total"), "{symbols:?}");

    // The highlight of a name.
    let places = wait_answer(&provider.highlights(&doc, text, at), "highlights");
    assert_eq!(places.len(), 3, "the declaration and two uses: {places:?}");

    // Import `helper` from util.js.
    let at = text.find("helper();").unwrap() + 1;
    let fixes = wait_answer(&provider.quick_fixes(&doc, text, at), "quick fixes");
    assert!(
        fixes
            .iter()
            .any(|f| f.title.to_lowercase().contains("import") && f.title.contains("util")),
        "{fixes:?}"
    );
    drop(provider);
    let _ = std::fs::remove_dir_all(&root);
}
