use super::*;

fn at(source: &str) -> (String, usize) {
    let offset = source.find('|').unwrap();
    (source.replacen('|', "", 1), offset)
}

fn lines(locations: &[Location]) -> Vec<usize> {
    locations.iter().map(|l| l.line).collect()
}

#[test]
fn a_name_leads_to_its_declaration_and_a_declaration_to_nothing() {
    let (text, offset) = at("function add(a, b) {\n  return a + b;\n}\nconst r = ad|d(1, 2);\n");
    let found = definition(&text, offset, None);
    assert_eq!(lines(&found), [0]);
    assert_eq!(found[0].column, "function ".len());
    assert_eq!(found[0].preview, "function add(a, b) {");
    // A parameter leads to the parameter list.
    let (text, offset) = at("function add(a, b) {\n  return a + |b;\n}\n");
    assert_eq!(lines(&definition(&text, offset, None)), [0]);
    // On the declaration itself there is nowhere to go.
    let (text, offset) = at("function ad|d() {}\n");
    assert!(definition(&text, offset, None).is_empty());
    // The innermost declaration wins.
    let (text, offset) = at("const x = 1;\nfunction f() {\n  const x = 2;\n  return |x;\n}\n");
    assert_eq!(lines(&definition(&text, offset, None)), [2]);
    let (text, offset) = at("console.lo|g(1);\n");
    assert!(
        definition(&text, offset, None).is_empty(),
        "built-ins have no source here"
    );
}

#[test]
fn every_use_of_a_function_leads_to_it() {
    let source = "function add(a, b) { return a + b; }
class C {
  run() { return add(1, 2); }
}
const f = () => add(3, 4);
console.log(add(5, 6), [1].map((x) => add(x, 1)));
setTimeout(function () { add(7, 8); });
add(9, 10);
";
    let mut count = 0;
    for (index, _) in source.match_indices("add(").skip(1) {
        let found = definition(source, index + 1, None);
        assert_eq!(found.len(), 1, "use at {index}: {found:?}");
        assert_eq!(found[0].line, 0);
        count += 1;
    }
    assert_eq!(count, 6);
    // And the declaration lists all of them.
    assert_eq!(
        references(source, source.find("add(a").unwrap() + 1).len(),
        6
    );
}

#[test]
fn members_lead_to_their_declaration() {
    let (text, offset) = at(
        "class A {\n  go() {}\n  constructor() { this.total = 0; }\n  run() { this.g|o(); this.total++; }\n}\nconst o = { first: 1 };\no.fi|rst;\n",
    );
    assert_eq!(lines(&definition(&text, offset, None)), [1]);
    let (text, offset) =
        at("class A {\n  constructor() { this.total = 0; }\n  run() { this.tot|al++; }\n}\n");
    assert_eq!(lines(&definition(&text, offset, None)), [1]);
    let (text, offset) = at("const o = { first: 1 };\no.fi|rst;\n");
    assert_eq!(lines(&definition(&text, offset, None)), [0]);
}

#[test]
fn imports_lead_into_the_module_file() {
    let dir = std::env::temp_dir().join(format!("atelier-js-nav-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(
        dir.join("lib").join("util.js"),
        "const hidden = 1;\n\nexport function helper() {}\nexport default class Tool {}\n",
    )
    .unwrap();
    std::fs::write(dir.join("lib").join("index.js"), "export const x = 1;\n").unwrap();
    let main = dir.join("main.js");

    let (text, offset) = at("import Tool, { helper } from './lib/util.js';\nhel|per();\n");
    let found = definition(&text, offset, Some(&main));
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].path.as_deref(),
        Some(dir.join("lib").join("util.js").as_path())
    );
    assert_eq!(found[0].line, 2);

    let (text, offset) = at("import Tool, { helper } from './lib/util.js';\nnew To|ol();\n");
    let found = definition(&text, offset, Some(&main));
    assert_eq!(found[0].line, 3, "the default export");

    let (text, offset) = at("import { x } from './lib';\n|x;\n");
    let found = definition(&text, offset, Some(&main));
    assert!(found[0].path.as_ref().unwrap().ends_with("index.js"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn typescript_specifiers_follow_js_names_and_tsconfig_paths() {
    let dir = std::env::temp_dir().join(format!("atelier-ts-paths-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src").join("lib")).unwrap();
    std::fs::write(
        dir.join("src").join("lib").join("math.ts"),
        "export const add = 1;\n",
    )
    .unwrap();
    std::fs::write(dir.join("src").join("util.tsx"), "export const x = 1;\n").unwrap();
    std::fs::write(
            dir.join("tsconfig.base.json"),
            "{\n  // shared\n  \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"@lib/*\": [\"src/lib/*\",], \"@util\": [\"src/util\"] } },\n}\n",
        )
        .unwrap();
    std::fs::write(
        dir.join("tsconfig.json"),
        "{ \"extends\": \"./tsconfig.base\" /* inherited */ }\n",
    )
    .unwrap();
    let from = dir.join("src");
    assert_eq!(
        resolve_module(&from, "./lib/math.js"),
        Some(dir.join("src").join("lib").join("math.ts"))
    );
    assert_eq!(
        resolve_module(&from, "./util.js"),
        Some(dir.join("src").join("util.tsx"))
    );
    assert_eq!(
        resolve_module(&from, "@lib/math"),
        Some(dir.join("src").join("lib").join("math.ts"))
    );
    assert_eq!(
        resolve_module(&from, "@util"),
        Some(dir.join("src").join("util.tsx"))
    );
    assert_eq!(resolve_module(&from, "@lib/missing"), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn packages_of_node_modules_are_followed() {
    let dir = std::env::temp_dir().join(format!("atelier-js-pkg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let modules = dir.join("node_modules");
    std::fs::create_dir_all(modules.join("tool").join("lib")).unwrap();
    std::fs::create_dir_all(modules.join("@scope").join("kit")).unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        modules.join("tool").join("package.json"),
        r#"{"main":"./lib/main.js"}"#,
    )
    .unwrap();
    std::fs::write(
        modules.join("tool").join("lib").join("main.js"),
        "export function run() {}\n",
    )
    .unwrap();
    std::fs::write(modules.join("tool").join("lib").join("extra.js"), "").unwrap();
    std::fs::write(
        modules.join("@scope").join("kit").join("package.json"),
        r#"{"exports":{".":{"import":"./esm.mjs","require":"./cjs.js"}}}"#,
    )
    .unwrap();
    std::fs::write(
        modules.join("@scope").join("kit").join("esm.mjs"),
        "export const kit = 1;\n",
    )
    .unwrap();
    let from = dir.join("src");
    assert_eq!(
        resolve_module(&from, "tool"),
        Some(modules.join("tool").join("lib").join("main.js"))
    );
    assert_eq!(
        resolve_module(&from, "tool/lib/extra"),
        Some(modules.join("tool").join("lib").join("extra.js"))
    );
    assert_eq!(
        resolve_module(&from, "@scope/kit"),
        Some(modules.join("@scope").join("kit").join("esm.mjs"))
    );
    assert_eq!(resolve_module(&from, "missing"), None);
    assert_eq!(resolve_module(&from, "node:fs"), None);

    let main = from.join("main.js");
    let (text, offset) = at("import { run } from 'tool';\n|run();\n");
    let found = definition(&text, offset, Some(&main));
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].path.as_deref(),
        Some(modules.join("tool").join("lib").join("main.js").as_path())
    );
    assert_eq!(found[0].line, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn references_list_the_uses_without_the_declaration() {
    let (text, offset) = at(
        "function ad|d(a) { return a; }\nadd(1);\nconst n = add(2) + add(3);\nfunction other() { const add = 5; return add; }\n",
    );
    let found = references(&text, offset);
    assert_eq!(
        lines(&found),
        [1, 2, 2],
        "the shadowing `add` is another name"
    );
    let (text, offset) = at("function f(a) {\n  return |a + a;\n}\n");
    assert_eq!(
        references(&text, offset).len(),
        2,
        "both uses, the declaration is left out"
    );
}

#[test]
fn references_of_a_member_follow_the_declaration() {
    let (text, offset) = at(
        "class A {\n  g|o() {}\n  run() { this.go(); }\n}\nnew A().go();\nconst o = { go() {} };\no.go();\n",
    );
    let found = references(&text, offset);
    assert_eq!(
        lines(&found),
        [2, 4],
        "the other `go` belongs to another object"
    );
}
