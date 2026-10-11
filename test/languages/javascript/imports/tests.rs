use super::*;

fn apply(source: &str, fix: &QuickFix) -> String {
    let mut text = source.to_string();
    let mut edits = fix.edits.clone();
    edits.sort_by_key(|e| std::cmp::Reverse(e.range.start));
    for edit in edits {
        text.replace_range(edit.range, &edit.new_text);
    }
    text
}

#[test]
fn exports_are_read_from_es_modules_and_commonjs() {
    let es = exports_of(
        "export function a() {}\nexport const b = 1, { c, d: e } = o;\nexport class F {}\nconst g = 1;\nexport { g, g as h };\nexport default function main() {}\n",
    );
    assert_eq!(es.named, ["F", "a", "b", "c", "e", "g", "h"]);
    assert_eq!(es.default, Some(Some("main".into())));
    let cjs = exports_of(
        "function f() {}\nmodule.exports = { f, other: 1, method() {} };\nexports.extra = 2;\nmodule.exports.more = 3;\n",
    );
    assert_eq!(cjs.named, ["extra", "f", "method", "more", "other"]);
    let single = exports_of("class Tool {}\nmodule.exports = Tool;\n");
    assert_eq!(single.default, Some(Some("Tool".into())));
    assert_eq!(exports_of("const x = 1;\n"), Exports::default());
}

#[test]
fn specifiers_are_relative_to_the_importing_file() {
    let from = Path::new("/p/src/ui");
    assert_eq!(
        relative_specifier(from, Path::new("/p/src/ui/button.js"), false).as_deref(),
        Some("./button.js")
    );
    assert_eq!(
        relative_specifier(from, Path::new("/p/src/util/format.js"), false).as_deref(),
        Some("../util/format.js")
    );
    assert_eq!(
        relative_specifier(from, Path::new("/p/src/util/format.js"), true).as_deref(),
        Some("../util/format")
    );
    assert_eq!(camel_case("date-utils"), "dateUtils");
}

#[test]
fn an_undeclared_name_gets_imports_from_the_project_node_and_dependencies() {
    let root = std::env::temp_dir().join(format!("atelier-js-imports-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::create_dir_all(root.join("node_modules").join("hidden")).unwrap();
    std::fs::write(
        root.join("lib").join("util.js"),
        "export function helper() {}\nexport default function main() {}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("lib").join("format.js"),
        "module.exports = { pretty };\n",
    )
    .unwrap();
    std::fs::write(
        root.join("node_modules").join("hidden").join("x.js"),
        "export function helper() {}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"express":"4","left-pad":"1"}}"#,
    )
    .unwrap();
    let file = root.join("main.js");

    // ES module with an import already: the new one goes after it.
    let source = "import a from './a.js';\n\nhelper();\n";
    let fixes = import_fixes(source, source.find("helper").unwrap(), Some(&file), &root);
    assert_eq!(fixes.len(), 1, "{fixes:?}");
    assert_eq!(
        fixes[0].title,
        "Importer « helper » depuis « ./lib/util.js »"
    );
    assert_eq!(
        apply(source, &fixes[0]),
        "import a from './a.js';\nimport { helper } from './lib/util.js';\n\nhelper();\n"
    );

    // The default export, by the name it was declared with.
    let source = "main();\n";
    let fixes = import_fixes(source, 1, Some(&file), &root);
    assert_eq!(
        apply(source, &fixes[0]),
        "import main from './lib/util.js';\nmain();\n"
    );

    // CommonJS style.
    let source = "const fs2 = require('fs');\npretty();\n";
    let fixes = import_fixes(source, source.find("pretty").unwrap(), Some(&file), &root);
    assert_eq!(
        apply(source, &fixes[0]),
        "const fs2 = require('fs');\nconst { pretty } = require('./lib/format.js');\npretty();\n"
    );

    // Node modules and dependencies.
    let source =
        "import x from './x.js';\nreadFileSync('a');\npath.join();\nexpress();\nleftPad();\n";
    let titles = |needle: &str| -> Vec<String> {
        import_fixes(source, source.find(needle).unwrap() + 1, Some(&file), &root)
            .into_iter()
            .map(|f| f.title)
            .collect()
    };
    assert_eq!(
        titles("readFileSync"),
        ["Importer « readFileSync » depuis « fs »"]
    );
    assert_eq!(titles("path.join"), ["Importer « path » depuis « path »"]);
    assert_eq!(
        titles("express"),
        ["Importer « express » depuis « express »"]
    );
    assert_eq!(
        titles("leftPad"),
        ["Importer « leftPad » depuis « left-pad »"]
    );

    // Names that exist are left alone, and so are comments and members.
    let source = "const known = 1;\nknown;\nconsole.log(1);\nobj.helper;\n";
    for needle in ["known;", "console", "helper"] {
        assert!(
            import_fixes(source, source.find(needle).unwrap() + 1, Some(&file), &root).is_empty(),
            "{needle}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_name_is_added_to_the_existing_import_of_the_module() {
    let root = std::env::temp_dir().join(format!("atelier-js-imports2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("util.js"),
        "export const one = 1;\nexport const two = 2;\n",
    )
    .unwrap();
    let file = root.join("main.js");
    let source = "import { one } from './util.js';\ntwo;\n";
    let fixes = import_fixes(source, source.find("two").unwrap(), Some(&file), &root);
    assert_eq!(
        fixes[0].title,
        "Ajouter « two » à l'import de « ./util.js »"
    );
    assert_eq!(
        apply(source, &fixes[0]),
        "import { one, two } from './util.js';\ntwo;\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}
