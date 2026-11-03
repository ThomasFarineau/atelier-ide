use super::*;

#[test]
fn fuzzy_matching_prefers_prefixes_and_word_starts() {
    let score = |q, l| fuzzy_score(q, l);
    assert!(score("con", "console").unwrap() > score("con", "decon").unwrap_or(i32::MIN));
    assert!(score("gEBI", "getElementById").is_some());
    assert!(score("gebi", "getElementById").is_some());
    assert!(score("xyz", "console").is_none());
    assert!(score("log", "console.log").is_some());
    assert_eq!(score("", "anything"), Some(0));
    assert!(score("fe", "forEach").unwrap() > score("fe", "buffer").unwrap());
}

#[test]
fn oneshot_is_read_once() {
    let slot = Oneshot::pending();
    assert!(slot.take().is_none());
    slot.clone().fulfill(3);
    assert_eq!(slot.take(), Some(3));
    assert_eq!(slot.take(), None);
    assert_eq!(Oneshot::ready("x").take(), Some("x"));
}

/// What a fake provider was given: (language, text) of each update, and the keys closed.
type Seen = Arc<Mutex<(Vec<(String, String)>, Vec<u64>)>>;

/// A provider that tells what it was given, for the routing of pages.
struct Fake {
    language: &'static str,
    seen: Seen,
    /// Where its one problem is.
    problem: Range<usize>,
}

impl Provider for Fake {
    fn handles(&self, language: &str) -> bool {
        language == self.language
    }
    fn update(&mut self, doc: &DocRef, text: &str, _version: u64) {
        let mut seen = self.seen.lock().unwrap();
        seen.0.push((doc.language.clone(), text.to_string()));
    }
    fn close(&mut self, key: u64) {
        self.seen.lock().unwrap().1.push(key);
    }
    fn diagnostics(&mut self, _key: u64) -> Arc<Vec<Diagnostic>> {
        Arc::new(vec![Diagnostic {
            range: self.problem.clone(),
            severity: Severity::Error,
            message: self.language.into(),
            source: String::new(),
        }])
    }
    fn complete(
        &mut self,
        doc: &DocRef,
        _text: &str,
        offset: usize,
        _explicit: bool,
    ) -> Oneshot<Option<Completion>> {
        Oneshot::ready(Some(Completion {
            from: offset,
            items: vec![CompletionItem::new(doc.language.clone(), ItemKind::Text)],
        }))
    }
    fn hover(&mut self, _doc: &DocRef, _text: &str, _offset: usize) -> Oneshot<Option<Hover>> {
        Oneshot::ready(None)
    }
    fn definition(&mut self, doc: &DocRef, text: &str, _offset: usize) -> Oneshot<Vec<Location>> {
        Oneshot::ready(vec![Location::in_text(doc.path.clone(), text, 0..1)])
    }
}

fn wait<T>(answer: Oneshot<T>) -> T {
    let started = std::time::Instant::now();
    loop {
        if let Some(value) = answer.take() {
            return value;
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn the_code_of_a_page_goes_to_the_provider_of_its_language() {
    let text = "<p>hi</p>\n<script>\nlet a = 1;\n</script>\n";
    let script = text.find("let").unwrap();
    let html_seen = Arc::default();
    let js_seen: Seen = Arc::default();
    let service = Service::default();
    service.add(Box::new(Fake {
        language: "html",
        seen: html_seen,
        problem: 0..1,
    }));
    service.add(Box::new(Fake {
        language: "js",
        seen: js_seen.clone(),
        // One problem in the script, one where the markup was.
        problem: script..script + 3,
    }));
    let doc = DocRef {
        key: 1,
        path: Some(PathBuf::from("site/index.html")),
        language: "html".into(),
    };
    service.update(&doc, text, 1);
    // The JavaScript provider got a document as long as the page, with the script only.
    {
        let seen = js_seen.lock().unwrap();
        let (language, code) = &seen.0[0];
        assert_eq!(language, "js");
        assert_eq!(code.len(), text.len());
        assert_eq!(&code[script..script + 10], "let a = 1;");
        assert!(code[..script].trim().is_empty());
    }
    let label = |offset: usize| {
        wait(service.complete(&doc, text, offset, true))
            .unwrap()
            .items[0]
            .label
            .clone()
    };
    assert_eq!(label(script + 2), "js");
    assert_eq!(label(1), "html");
    // A place in the document of the script is a place in the page.
    let found = wait(service.definition(&doc, text, script + 1));
    assert_eq!(found[0].path, None);
    // The problems of the page and of its script.
    let problems = service.diagnostics(&doc);
    let messages: Vec<&str> = problems.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(messages, ["html", "js"]);
    // Without a script, its document is closed.
    service.update(&doc, "<p>hi</p>", 2);
    assert_eq!(js_seen.lock().unwrap().1.len(), 1);
}
