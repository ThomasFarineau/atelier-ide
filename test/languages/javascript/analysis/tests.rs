use super::*;

fn ty_at(text: &str, marker: &str, name: &str) -> (Analysis, Ty) {
    let analysis = Analysis::new(text).unwrap();
    let at = text.find(marker).unwrap();
    let ty = analysis.ty_of_name(name, at, 0);
    (analysis, ty)
}

#[test]
fn syntax_errors_are_reported_at_the_bad_token() {
    assert!(syntax_errors("const a = 1;\nfunction f(x) { return x * 2; }\n").is_empty());
    let errors = syntax_errors("const a = ;\nlet b = 2;\n");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!((8..=10).contains(&errors[0].range.start), "{errors:?}");
    let errors = syntax_errors("function f( {\n  return 1;\n}\n");
    assert!(!errors.is_empty());
    let errors = syntax_errors("if (x) {\n  foo(\n}\n");
    assert!(!errors.is_empty());
}

#[test]
fn scopes_hoist_var_and_functions_but_not_let() {
    let text = "function outer(p) {\n  var a = 1;\n  if (p) { let b = 2; var c = 3; }\n  return a;\n  //here\n}\nconst top = 1;\n";
    let analysis = Analysis::new(text).unwrap();
    let at = text.find("//here").unwrap();
    let names: Vec<&str> = analysis
        .visible(at)
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert!(
        names.contains(&"p") && names.contains(&"a") && names.contains(&"c"),
        "{names:?}"
    );
    assert!(
        names.contains(&"outer") && names.contains(&"top"),
        "{names:?}"
    );
    assert!(!names.contains(&"b"), "block scoped: {names:?}");
    let inside = text.find("let b").unwrap();
    assert!(analysis.visible(inside).iter().any(|d| d.name == "b"));
}

#[test]
fn destructuring_imports_and_catch_are_declared() {
    let text = "import def, { a, b as c } from './m.js';\nimport * as ns from 'x';\nconst { d, e: [f, ...g], h = 1 } = obj;\ntry {} catch (err) { //in\n}\n";
    let analysis = Analysis::new(text).unwrap();
    let names: Vec<&str> = analysis
        .visible(text.find("//in").unwrap())
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    for expected in ["def", "a", "c", "ns", "d", "f", "g", "h", "err"] {
        assert!(names.contains(&expected), "{expected} in {names:?}");
    }
    assert!(!names.contains(&"b") && !names.contains(&"e"));
    let c = analysis.resolve("c", 0).unwrap();
    assert_eq!(c.import, Some(("./m.js".into(), "b".into())));
}

#[test]
fn literal_types_flow_through_variables_and_calls() {
    let text = "const s = 'x';\nconst n = 1 + 2;\nconst list = [1, 2];\nconst up = s.toUpperCase();\nconst parts = s.split(',');\nconst m = new Map();\nfunction mk() { return { a: 1 }; }\nconst o = mk();\nlet z; //end\n";
    let at = |name| ty_at(text, "//end", name).1;
    assert_eq!(at("s"), Ty::Str);
    assert_eq!(at("n"), Ty::Num);
    assert_eq!(at("list"), Ty::ArrayOf(Box::new(Ty::Num)));
    assert_eq!(at("up"), Ty::Str);
    assert_eq!(at("parts"), Ty::Array);
    assert_eq!(at("m"), Ty::Builtin("Map"));
    assert!(matches!(at("o"), Ty::Object(_)));
    assert_eq!(at("z"), Ty::Unknown);
    assert_eq!(at("Math"), Ty::Static("Math"));
}

#[test]
fn classes_expose_methods_fields_constructor_properties_and_statics() {
    let text = "class Animal {\n  legs = 4;\n  static create() { return new Animal('x'); }\n  constructor(name) { this.name = 'n'; }\n  speak() {}\n}\nclass Dog extends Animal { bark() {} }\nconst d = new Dog();\n//end\n";
    let analysis = Analysis::new(text).unwrap();
    let at = text.find("//end").unwrap();
    let Ty::Instance(range) = analysis.ty_of_name("d", at, 0) else {
        panic!("a Dog instance");
    };
    let names: Vec<String> = analysis
        .props_of(&Ty::Instance(range), at)
        .into_iter()
        .map(|p| p.name)
        .collect();
    for expected in ["bark", "speak", "legs", "name", "hasOwnProperty"] {
        assert!(
            names.contains(&expected.to_string()),
            "{expected} in {names:?}"
        );
    }
    let Ty::Class(class) = analysis.ty_of_name("Animal", at, 0) else {
        panic!("a class");
    };
    let statics: Vec<String> = analysis
        .props_of(&Ty::Class(class), at)
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(
        statics.contains(&"create".to_string()) && !statics.contains(&"speak".to_string()),
        "{statics:?}"
    );
}

#[test]
fn jsdoc_types_and_comments_are_read() {
    let text = "/**\n * Adds.\n * @param {string} name - the name\n */\nfunction greet(name, other) { name; }\n";
    let analysis = Analysis::new(text).unwrap();
    let at = text.find("name;").unwrap();
    assert_eq!(analysis.ty_of_name("name", at, 0), Ty::Str);
    assert_eq!(analysis.ty_of_name("other", at, 0), Ty::Unknown);
    let decl = analysis.resolve("greet", at).unwrap();
    let doc = analysis.jsdoc_before(&decl.node).unwrap();
    assert!(doc.starts_with("Adds."), "{doc}");
}
