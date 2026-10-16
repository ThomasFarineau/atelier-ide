use super::*;
use crate::languages::Provider;

/// A provider that answers with a fixed list, fixed places and one problem.
struct Fixed(
    Vec<CompletionItem>,
    Vec<Location>,
    Vec<Location>,
    Vec<QuickFix>,
    Vec<Symbol>,
);

impl Provider for Fixed {
    fn handles(&self, language: &str) -> bool {
        language == "js"
    }
    fn update(&mut self, _: &DocRef, _: &str, _: u64) {}
    fn close(&mut self, _: u64) {}
    fn diagnostics(&mut self, _: u64) -> Arc<Vec<Diagnostic>> {
        Arc::new(vec![Diagnostic {
            range: 0..3,
            severity: Severity::Error,
            message: "bad".into(),
            source: "test".into(),
        }])
    }
    fn complete(
        &mut self,
        _: &DocRef,
        text: &str,
        offset: usize,
        _: bool,
    ) -> Oneshot<Option<Completion>> {
        let from = text[..offset]
            .rfind(|c: char| !is_word(c))
            .map_or(0, |i| i + 1);
        Oneshot::ready(Some(Completion {
            from,
            items: self.0.clone(),
        }))
    }
    fn hover(&mut self, _: &DocRef, _: &str, _: usize) -> Oneshot<Option<Hover>> {
        Oneshot::ready(None)
    }
    fn definition(&mut self, _: &DocRef, _: &str, _: usize) -> Oneshot<Vec<Location>> {
        Oneshot::ready(self.1.clone())
    }
    fn references(&mut self, _: &DocRef, _: &str, _: usize) -> Oneshot<Vec<Location>> {
        Oneshot::ready(self.2.clone())
    }
    fn quick_fixes(&mut self, _: &DocRef, _: &str, _: usize) -> Oneshot<Vec<QuickFix>> {
        Oneshot::ready(self.3.clone())
    }
    fn symbols(&mut self, _: &DocRef, _: &str) -> Oneshot<Vec<Symbol>> {
        Oneshot::ready(self.4.clone())
    }
    fn highlights(&mut self, _: &DocRef, _: &str, _: usize) -> Oneshot<Vec<Range<usize>>> {
        Oneshot::ready(vec![0..3, 10..13])
    }
}

fn setup(items: Vec<CompletionItem>) -> (Service, DocRef) {
    setup_places(items, Vec::new(), Vec::new())
}

fn setup_places(
    items: Vec<CompletionItem>,
    definition: Vec<Location>,
    references: Vec<Location>,
) -> (Service, DocRef) {
    let service = Service::default();
    service.add(Box::new(Fixed(
        items,
        definition,
        references,
        Vec::new(),
        Vec::new(),
    )));
    (
        service,
        DocRef {
            key: 1,
            path: None,
            language: "js".into(),
        },
    )
}

fn setup_extras(fixes: Vec<QuickFix>, symbols: Vec<Symbol>) -> (Service, DocRef) {
    let service = Service::default();
    service.add(Box::new(Fixed(
        Vec::new(),
        Vec::new(),
        Vec::new(),
        fixes,
        symbols,
    )));
    (
        service,
        DocRef {
            key: 1,
            path: None,
            language: "js".into(),
        },
    )
}

fn place(line: usize, column: usize, end: usize) -> Location {
    Location {
        path: None,
        line,
        column,
        end_line: line,
        end_column: end,
        preview: format!("line {line}"),
    }
}

fn frame(state: &mut EditorState, service: &Service, doc: &DocRef, text: &str) {
    let ctx = egui::Context::default();
    let options = Options {
        language: "js",
        font: FontId::monospace(14.0),
        theme: crate::ui::theme_by_index(0),
        marks: &[],
        annotations: None,
        gutter_commands: &[],
        assist: Some(Assist {
            service,
            doc: doc.clone(),
        }),
        search: &[],
        read_only: false,
        inlay_hints: false,
        sticky_lines: false,
        breadcrumbs: false,
        bookmarks: &[],
        spellcheck: false,
    };
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| state.assist_frame(ui, text, &options));
    });
}

fn state_for(text: &str) -> EditorState {
    let mut state = EditorState::default();
    state.sync(text);
    state.cursor = text.len();
    state.anchor = text.len();
    state
}

#[test]
fn typing_opens_the_popup_filters_it_and_enter_inserts() {
    let items = vec![
        CompletionItem::new("console", ItemKind::Class),
        CompletionItem::new("const", ItemKind::Keyword).rank(70),
        CompletionItem::new("continue", ItemKind::Keyword).rank(70),
        CompletionItem::new("alert", ItemKind::Function),
    ];
    let (service, doc) = setup(items);
    let mut text = "co".to_string();
    let mut state = state_for(&text);
    state.assist_after_edit(&text, Some('o'), false);
    assert_eq!(
        state.assist.want,
        Some(false),
        "a letter typed after one starts completion"
    );
    frame(&mut state, &service, &doc, &text);
    let popup = state.assist.popup.as_ref().expect("the popup opens");
    let shown: Vec<&str> = popup
        .filtered
        .iter()
        .map(|(i, _)| popup.items[*i].label.as_str())
        .collect();
    assert_eq!(
        shown,
        ["console", "const", "continue"],
        "alert does not match, best first"
    );
    // One more letter narrows the list.
    text.push('n');
    state.cursor = text.len();
    state.assist_after_edit(&text, Some('n'), false);
    assert_eq!(state.assist.popup.as_ref().unwrap().filtered.len(), 3);
    text.push('s');
    state.cursor = text.len();
    state.assist_after_edit(&text, Some('s'), false);
    let popup = state.assist.popup.as_ref().unwrap();
    assert_eq!(popup.filtered.len(), 2, "console and const");
    // Down then Enter takes the second one.
    assert_eq!(
        state.assist_key(&mut text, Key::ArrowDown, Modifiers::NONE),
        Some(false)
    );
    assert_eq!(
        state.assist_key(&mut text, Key::Enter, Modifiers::NONE),
        Some(true)
    );
    assert_eq!(text, "const");
    assert_eq!(state.cursor, 5);
    assert!(state.assist.popup.is_none());
    // Leaving the word closes nothing that is not open; a space after a word does not reopen it.
    text.push(' ');
    state.cursor = text.len();
    state.assist_after_edit(&text, Some(' '), false);
    assert!(state.assist.want.is_none() && state.assist.popup.is_none());
}

#[test]
fn items_place_the_caret_and_keep_the_indentation() {
    let snippet = CompletionItem::new("if", ItemKind::Snippet).insert("if () {\n    \n}", Some(4));
    let call = CompletionItem::new("log", ItemKind::Method).insert("log()", Some(4));
    let (service, doc) = setup(vec![snippet, call]);

    let mut text = "function f() {\n    if".to_string();
    let mut state = state_for(&text);
    state.assist.want = Some(true);
    frame(&mut state, &service, &doc, &text);
    assert_eq!(
        state.assist_key(&mut text, Key::Tab, Modifiers::NONE),
        Some(true)
    );
    assert_eq!(text, "function f() {\n    if () {\n        \n    }");
    assert_eq!(
        &text[state.cursor - 4..state.cursor],
        "if (",
        "the caret is between the parentheses"
    );

    let mut text = "console.lo".to_string();
    let mut state = state_for(&text);
    state.assist.want = Some(true);
    frame(&mut state, &service, &doc, &text);
    assert_eq!(
        state.assist_key(&mut text, Key::Enter, Modifiers::NONE),
        Some(true)
    );
    assert_eq!(text, "console.log()");
    assert_eq!(state.cursor, "console.log(".len());
}

#[test]
fn tab_replaces_the_rest_of_the_word_and_escape_closes() {
    let (service, doc) = setup(vec![CompletionItem::new("length", ItemKind::Property)]);
    let mut text = "arr.le_old".to_string();
    let mut state = state_for(&text);
    state.cursor = "arr.le".len();
    state.anchor = state.cursor;
    state.assist.want = Some(true);
    frame(&mut state, &service, &doc, &text);
    assert!(state.assist.popup.is_some());
    assert_eq!(
        state.assist_key(&mut text, Key::Tab, Modifiers::NONE),
        Some(true)
    );
    assert_eq!(text, "arr.length");

    let mut text = "arr.le".to_string();
    let mut state = state_for(&text);
    state.assist.want = Some(true);
    frame(&mut state, &service, &doc, &text);
    assert_eq!(
        state.assist_key(&mut text, Key::Escape, Modifiers::NONE),
        Some(false)
    );
    assert!(state.assist.popup.is_none());
    assert_eq!(text, "arr.le");
}

#[test]
fn going_to_a_declaration_jumps_to_the_only_place() {
    let (service, doc) = setup_places(Vec::new(), vec![place(0, 9, 12)], Vec::new());
    let text = "function add() {}\nadd();\n";
    let mut state = state_for(text);
    state.cursor = text.find("add();").unwrap() + 1;
    state.assist_goto(state.cursor);
    frame(&mut state, &service, &doc, text);
    assert_eq!(state.assist.navigate, Some(place(0, 9, 12)));
    assert!(state.assist.chooser.is_none());
}

#[test]
fn a_declaration_leads_to_its_usages_and_several_places_are_chosen_from() {
    // The definition is the word itself: the usages are asked for.
    let text = "function add() {}\nadd();\nadd();\n";
    let (service, doc) = setup_places(
        Vec::new(),
        vec![place(0, 9, 12)],
        vec![place(1, 0, 3), place(2, 0, 3)],
    );
    let mut state = state_for(text);
    state.cursor = 10;
    state.assist_goto(10);
    frame(&mut state, &service, &doc, text);
    frame(&mut state, &service, &doc, text);
    assert!(state.assist.navigate.is_none());
    let chooser = state
        .assist
        .chooser
        .as_ref()
        .expect("two usages to choose from");
    assert_eq!(chooser.title, tr!("usages"));
    assert_eq!(chooser.entries.len(), 2);
    // Down and Enter go to the second.
    let mut text = text.to_string();
    assert_eq!(
        state.assist_key(&mut text, Key::ArrowDown, Modifiers::NONE),
        Some(false)
    );
    assert_eq!(
        state.assist_key(&mut text, Key::Enter, Modifiers::NONE),
        Some(false)
    );
    assert_eq!(state.assist.navigate, Some(place(2, 0, 3)));
    assert!(state.assist.chooser.is_none());
}

#[test]
fn alt_enter_offers_actions_and_applies_the_chosen_one() {
    use crate::languages::TextEdit;
    let fix = QuickFix {
        title: "Importer « helper »".into(),
        edits: vec![TextEdit {
            range: 0..0,
            new_text: "import { helper } from './u.js';\n".into(),
        }],
    };
    let other = QuickFix {
        title: "Autre".into(),
        edits: vec![TextEdit {
            range: 0..0,
            new_text: "x".into(),
        }],
    };
    let (service, doc) = setup_extras(vec![fix, other], Vec::new());
    let mut text = "helper();\n".to_string();
    let mut state = state_for(&text);
    state.cursor = 3;
    state.anchor = 3;
    assert_eq!(
        state.assist_key(&mut text, Key::Enter, Modifiers::ALT),
        Some(false)
    );
    frame(&mut state, &service, &doc, &text);
    let chooser = state
        .assist
        .chooser
        .as_ref()
        .expect("the actions are listed");
    assert_eq!(chooser.title, tr!("quick_fixes"));
    // The two fixes, then the actions of the IDE (refactor, surround).
    assert_eq!(chooser.entries.len(), 4);
    assert_eq!(
        state.assist_key(&mut text, Key::Enter, Modifiers::NONE),
        Some(true)
    );
    assert_eq!(text, "import { helper } from './u.js';\nhelper();\n");
    assert_eq!(
        state.cursor,
        3 + "import { helper } from './u.js';\n".len(),
        "the caret stays on the same code"
    );
}

#[test]
fn the_file_structure_is_filtered_by_typing_and_goes_to_the_symbol() {
    let text = "function alpha() {}\nclass Beta {\n  gamma() {}\n}\n";
    let symbols = vec![
        Symbol {
            name: "alpha".into(),
            kind: ItemKind::Function,
            detail: "()".into(),
            range: 9..14,
            depth: 0,
        },
        Symbol {
            name: "Beta".into(),
            kind: ItemKind::Class,
            detail: String::new(),
            range: 26..30,
            depth: 0,
        },
        Symbol {
            name: "gamma".into(),
            kind: ItemKind::Method,
            detail: "()".into(),
            range: 35..40,
            depth: 1,
        },
    ];
    let (service, doc) = setup_extras(Vec::new(), symbols);
    let mut state = state_for(text);
    state.cursor = 36;
    state.anchor = 36;
    assert_eq!(
        state.assist_key(&mut text.to_string(), Key::F12, Modifiers::COMMAND),
        Some(false)
    );
    frame(&mut state, &service, &doc, text);
    let chooser = state.assist.chooser.as_ref().expect("the structure opens");
    assert_eq!(chooser.shown.len(), 3);
    assert_eq!(
        chooser.selected, 2,
        "the symbol the caret is in comes pre-selected"
    );
    // Typing filters, in no document: the text stays as it was.
    assert!(state.assist_text("be"));
    let chooser = state.assist.chooser.as_ref().unwrap();
    assert_eq!(chooser.shown.len(), 1);
    assert_eq!(chooser.entries[chooser.shown[0]].label, "Beta");
    let mut owned = text.to_string();
    assert_eq!(
        state.assist_key(&mut owned, Key::Backspace, Modifiers::NONE),
        Some(false)
    );
    assert_eq!(
        state.assist.chooser.as_ref().unwrap().shown.len(),
        1,
        "one letter left: still `b`"
    );
    assert_eq!(
        state.assist_key(&mut owned, Key::Enter, Modifiers::NONE),
        Some(false)
    );
    let target = state.assist.navigate.clone().expect("a place to go to");
    assert_eq!(target.range_in(text), 26..30);
    assert_eq!(target.preview, "class Beta {");
    assert!(!state.assist_text("x"), "no list is open any more");
}

#[test]
fn the_name_under_the_caret_is_highlighted_after_a_pause() {
    let (service, doc) = setup(Vec::new());
    let text = "abc = 1;\nabc;\n";
    let mut state = state_for(text);
    state.cursor = 1;
    state.anchor = 1;
    frame(&mut state, &service, &doc, text);
    assert!(
        state.assist.usages.ranges.is_empty(),
        "not before the pause"
    );
    std::thread::sleep(Duration::from_millis(200));
    frame(&mut state, &service, &doc, text);
    frame(&mut state, &service, &doc, text);
    assert_eq!(state.assist.usages.ranges, [0..3, 10..13]);
    // An edit forgets them.
    state.assist_after_edit(text, Some('x'), false);
    assert!(state.assist.usages.ranges.is_empty());
}

#[test]
fn brackets_are_paired_around_the_caret() {
    let text = "f(a[1], {b: (2)})";
    let mut state = state_for(text);
    let pair = |state: &mut EditorState, at: usize| {
        state.cursor = at;
        state.matching_bracket(text)
    };
    assert_eq!(pair(&mut state, 2), Some((1, 16)), "after the opening one");
    assert_eq!(pair(&mut state, 17), Some((1, 16)), "after the closing one");
    assert_eq!(
        pair(&mut state, 5),
        Some((3, 5)),
        "before the closing bracket of `a[1]`"
    );
    assert_eq!(pair(&mut state, 0), None, "no bracket next to the caret");
}

#[test]
fn a_usage_can_be_clicked_in_the_list() {
    let text = "function add() {}
add();
add();
add();
";
    let (service, doc) = setup_places(
        Vec::new(),
        vec![place(0, 9, 12)],
        vec![place(1, 0, 3), place(2, 0, 3), place(3, 0, 3)],
    );
    let mut state = state_for(text);
    state.cursor = 10;
    state.assist_goto(10);
    for _ in 0..3 {
        frame(&mut state, &service, &doc, text);
    }
    let chooser = state
        .assist
        .chooser
        .as_mut()
        .expect("three usages to choose from");
    let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(560.0, 140.0));
    chooser.rect = Some(rect);
    let mut owned = text.to_string();
    // The second row: below the title, one row down.
    let click = pos2(150.0, rect.top() + 26.0 + 4.0 + ROW_HEIGHT * 1.5);
    assert_eq!(state.assist_click(&mut owned, click), Some(false));
    assert_eq!(state.assist.navigate, Some(place(2, 0, 3)));
    assert!(state.assist.chooser.is_none());
}

#[test]
fn the_wheel_scrolls_a_long_list() {
    let many: Vec<Location> = (0..30).map(|i| place(i, 0, 3)).collect();
    let mut chooser = Chooser::new(tr!("usages"), place_entries(many), false);
    scroll_chooser(&mut chooser, -3.0);
    assert_eq!(chooser.scroll, 3);
    scroll_chooser(&mut chooser, 100.0);
    assert_eq!(chooser.scroll, 0);
    scroll_chooser(&mut chooser, -1000.0);
    assert_eq!(chooser.scroll, 30 - ROWS);
}

/// A provider that renames `old` to whatever is asked, in this file and in `other.js`.
struct Renamer;

impl Provider for Renamer {
    fn handles(&self, language: &str) -> bool {
        language == "js"
    }
    fn update(&mut self, _: &DocRef, _: &str, _: u64) {}
    fn close(&mut self, _: u64) {}
    fn diagnostics(&mut self, _: u64) -> Arc<Vec<Diagnostic>> {
        Arc::default()
    }
    fn complete(&mut self, _: &DocRef, _: &str, _: usize, _: bool) -> Oneshot<Option<Completion>> {
        Oneshot::ready(None)
    }
    fn hover(&mut self, _: &DocRef, _: &str, _: usize) -> Oneshot<Option<Hover>> {
        Oneshot::ready(None)
    }
    fn rename(
        &mut self,
        _: &DocRef,
        text: &str,
        offset: usize,
        new_name: &str,
    ) -> Oneshot<Result<Vec<FileEdit>, String>> {
        if new_name == "taken" {
            return Oneshot::ready(Err("« taken » existe déjà ici".into()));
        }
        let start = text[..offset]
            .rfind(|c: char| !c.is_alphanumeric())
            .map_or(0, |i| i + 1);
        let old = start
            ..text[start..]
                .find(|c: char| !c.is_alphanumeric())
                .map_or(text.len(), |i| start + i);
        Oneshot::ready(Ok(vec![
            FileEdit {
                path: None,
                edits: vec![TextEdit {
                    range: old,
                    new_text: new_name.into(),
                }],
            },
            FileEdit {
                path: Some("other.js".into()),
                edits: vec![TextEdit {
                    range: 0..3,
                    new_text: new_name.into(),
                }],
            },
        ]))
    }
}

#[test]
fn renaming_asks_for_a_name_and_returns_the_changes_of_every_file() {
    let service = Service::default();
    service.add(Box::new(Renamer));
    let doc = DocRef {
        key: 1,
        path: None,
        language: "js".into(),
    };
    let mut text = "old + old".to_string();
    let mut state = state_for(&text);
    state.cursor = 1;
    state.anchor = 1;
    // Shift+F6 opens the field with the name in it.
    assert_eq!(
        state.assist_key(&mut text, Key::F6, Modifiers::SHIFT),
        Some(false)
    );
    assert_eq!(state.assist.rename.as_ref().unwrap().name, "old");
    // A refused name keeps the field open with the reason.
    state.assist.rename.as_mut().unwrap().name = "taken".into();
    state.assist.rename.as_mut().unwrap().submit = true;
    frame(&mut state, &service, &doc, &text);
    assert_eq!(
        state.assist.rename.as_ref().unwrap().error.as_deref(),
        Some("« taken » existe déjà ici")
    );
    // A good one closes it and hands the changes over.
    state.assist.rename.as_mut().unwrap().name = "fresh".into();
    state.assist.rename.as_mut().unwrap().submit = true;
    frame(&mut state, &service, &doc, &text);
    assert!(state.assist.rename.is_none());
    let edits = state.assist.apply.take().expect("changes to this document");
    state.apply_edits(&mut text, edits);
    assert_eq!(text, "fresh + old");
    assert_eq!(state.assist.file_edits.len(), 1);
    assert_eq!(
        state.assist.file_edits[0].0,
        std::path::PathBuf::from("other.js")
    );
    // One undo brings everything back.
    assert!(state.undo(&mut text));
    assert_eq!(text, "old + old");
}

#[test]
fn nothing_to_go_to_does_nothing() {
    let (service, doc) = setup_places(Vec::new(), Vec::new(), Vec::new());
    let text = "foo();\n";
    let mut state = state_for(text);
    state.assist_goto(1);
    frame(&mut state, &service, &doc, text);
    frame(&mut state, &service, &doc, text);
    assert!(
        state.assist.navigate.is_none()
            && state.assist.chooser.is_none()
            && state.assist.job.is_none()
    );
}

#[test]
fn diagnostics_are_counted_for_the_status_bar() {
    let (service, doc) = setup(Vec::new());
    let mut state = state_for("bad");
    frame(&mut state, &service, &doc, "bad");
    assert_eq!(state.assist.counts(), (1, 0));
}
