use super::*;

fn at(source: &str) -> (String, usize) {
    let offset = source.find('|').unwrap();
    (source.replacen('|', "", 1), offset)
}

fn apply(text: &str, edits: &[TextEdit]) -> String {
    let mut text = text.to_string();
    let mut sorted = edits.to_vec();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.range.start));
    for edit in sorted {
        text.replace_range(edit.range, &edit.new_text);
    }
    text
}

#[test]
fn a_local_name_is_renamed_everywhere_it_is_used() {
    let (text, offset) = at(
        "function ad|d(a) { return a; }\nadd(1);\nconst other = () => add(2);\nfunction f() { const add = 5; return add; }\n",
    );
    let plan = rename(&text, offset, "plus", None, Path::new(".")).unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(
        apply(&text, &plan[0].edits),
        "function plus(a) { return a; }\nplus(1);\nconst other = () => plus(2);\nfunction f() { const add = 5; return add; }\n"
    );
}

#[test]
fn invalid_or_clashing_names_are_refused() {
    let (text, offset) = at("const a|b = 1;\nconst cd = 2;\n");
    assert!(rename(&text, offset, "1x", None, Path::new(".")).is_err());
    assert!(rename(&text, offset, "class", None, Path::new(".")).is_err());
    assert!(rename(&text, offset, "ab", None, Path::new(".")).is_err());
    assert_eq!(
        rename(&text, offset, "cd", None, Path::new(".")).unwrap_err(),
        "« cd » existe déjà ici"
    );
    let (text, offset) = at("let 1|;\n");
    assert!(rename(&text, offset, "x", None, Path::new(".")).is_err());
}

#[test]
fn a_shorthand_property_keeps_its_name() {
    let (text, offset) =
        at("const na|me = 'x';\nconst user = { name };\nconst { name: n } = user;\n");
    let plan = rename(&text, offset, "title", None, Path::new(".")).unwrap();
    assert_eq!(
        apply(&text, &plan[0].edits),
        "const title = 'x';\nconst user = { name: title };\nconst { name: n } = user;\n"
    );
}

#[test]
fn the_files_that_import_a_name_follow() {
    let root = std::env::temp_dir().join(format!("atelier-rename-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("main.js"),
        "import { helper, other as o } from './lib.js';\nhelper();\nconst x = helper(1);\n",
    )
    .unwrap();
    std::fs::write(
        root.join("aliased.js"),
        "import { helper as h } from './lib.js';\nh();\n",
    )
    .unwrap();
    std::fs::write(root.join("unrelated.js"), "const helper = 1;\nhelper;\n").unwrap();
    let lib = root.join("lib.js");
    let (text, offset) = at("export function hel|per() {}\nexport const other = 1;\nhelper();\n");
    std::fs::write(&lib, &text).unwrap();
    let plan = rename(&text, offset, "assist", Some(&lib), &root).unwrap();
    assert_eq!(
        apply(&text, &plan[0].edits),
        "export function assist() {}\nexport const other = 1;\nassist();\n"
    );
    let by_file = |name: &str| {
        plan.iter()
            .find(|f| f.path.as_ref().is_some_and(|p| p.ends_with(name)))
    };
    let main = std::fs::read_to_string(root.join("main.js")).unwrap();
    assert_eq!(
        apply(&main, &by_file("main.js").expect("main imports it").edits),
        "import { assist, other as o } from './lib.js';\nassist();\nconst x = assist(1);\n"
    );
    let aliased = std::fs::read_to_string(root.join("aliased.js")).unwrap();
    assert_eq!(
        apply(&aliased, &by_file("aliased.js").unwrap().edits),
        "import { assist as h } from './lib.js';\nh();\n"
    );
    assert!(by_file("unrelated.js").is_none(), "another `helper`");
    let _ = std::fs::remove_dir_all(&root);
}

/// Renaming an imported name from a file that uses it renames it where it is declared and in
/// the other files that import it, as renaming it at its declaration does.
#[test]
fn renaming_an_import_renames_its_declaration_and_the_other_importers() {
    let root = std::env::temp_dir().join(format!("atelier-rename-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::write(
        root.join("lib/util.js"),
        "export function helper() {}
helper();
",
    )
    .unwrap();
    std::fs::write(
        root.join("other.js"),
        "import { helper } from './lib/util.js';
helper();
",
    )
    .unwrap();
    std::fs::write(
        root.join("aliased.js"),
        "import { helper as h } from './lib/util.js';
h();
",
    )
    .unwrap();
    let main = root.join("main.js");
    let (text, offset) = at("import { helper } from './lib/util.js';
hel|per();
");
    std::fs::write(&main, &text).unwrap();
    let plan = rename(&text, offset, "assist", Some(&main), &root).unwrap();
    assert_eq!(
        apply(&text, &plan[0].edits),
        "import { assist } from './lib/util.js';
assist();
"
    );
    let changed = |name: &str| {
        let file = plan
            .iter()
            .find(|f| f.path.as_ref().is_some_and(|p| p.ends_with(name)))
            .unwrap_or_else(|| panic!("{name} changes"));
        apply(
            &std::fs::read_to_string(root.join(name)).unwrap(),
            &file.edits,
        )
    };
    assert_eq!(
        changed("lib/util.js"),
        "export function assist() {}
assist();
"
    );
    assert_eq!(
        changed("other.js"),
        "import { assist } from './lib/util.js';
assist();
"
    );
    assert_eq!(
        changed("aliased.js"),
        "import { assist as h } from './lib/util.js';
h();
"
    );
    assert!(
        !plan
            .iter()
            .any(|f| f.path.as_ref().is_some_and(|p| p.ends_with("main.js"))),
        "main.js is the document itself"
    );
    // A name the declaring file already has would be merged with this one.
    std::fs::write(
        root.join("lib/util.js"),
        "export function helper() {}
export const assist = 1;
",
    )
    .unwrap();
    assert!(rename(&text, offset, "assist", Some(&main), &root).is_err());
    // An alias is local: only the document changes.
    let (aliased, at_alias) = at("import { helper as h } from './lib/util.js';
|h();
");
    let plan = rename(&aliased, at_alias, "g", Some(&main), &root).unwrap();
    assert_eq!(plan.len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}
