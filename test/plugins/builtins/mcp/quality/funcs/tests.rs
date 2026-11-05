use super::*;
use crate::plugins::builtins::mcp::quality::lang::parse;
use crate::plugins::builtins::mcp::quality::scratch;

fn funcs(lang: Lang, src: &str) -> Vec<(String, String, usize)> {
    let tree = parse(lang, src).unwrap();
    functions(&tree, src, lang, "m")
        .iter()
        .map(|f| (f.name.clone(), f.namespace.clone(), complexity(f.node)))
        .collect()
}

#[test]
fn script_functions_classes_and_arrows() {
    let src = "function a(x){ if(x && x>1){return 1} return x ? 2 : 3 }\nconst b = (y) => y ?? 1;\nclass C { m(z){ for(;;){} } }";
    assert_eq!(
        funcs(Lang::Js, src),
        vec![
            ("a".into(), "m".into(), 4),
            ("b".into(), "m".into(), 2),
            ("m".into(), "m.C".into(), 2),
        ]
    );
}

#[test]
fn rust_functions_impls_and_skipped_tests() {
    let src = "fn a(x: i32) -> i32 { match x { 1 => 1, _ => 2 } }\nstruct S;\nimpl S { fn m(&self) -> Option<i32> { let v = Some(1)?; Some(v) } }\n#[cfg(test)] mod tests { fn t() {} }\n#[test] fn u() {}";
    assert_eq!(
        funcs(Lang::Rust, src),
        vec![("a".into(), "m".into(), 3), ("m".into(), "m::S".into(), 2)]
    );
}

#[test]
fn nested_functions_belong_to_their_parent() {
    let found = funcs(Lang::Ts, "function a(){ function b(){ if (1) {} } }");
    assert_eq!(found, vec![("a".into(), "m".into(), 2)]);
}

#[test]
fn rust_traits_and_nested_modules_get_their_namespace() {
    let src = "trait T { fn d(&self) {} }\nmod inner { pub fn g() {} mod deep { fn h() {} } }";
    assert_eq!(
        funcs(Lang::Rust, src),
        vec![
            ("d".into(), "m::T".into(), 1),
            ("g".into(), "m::inner".into(), 1),
            ("h".into(), "m::inner::deep".into(), 1),
        ]
    );
}

#[test]
fn script_module_drops_leading_src() {
    let root = Path::new("/p");
    assert_eq!(
        module_name(root, Path::new("/p/src/a/b.ts"), Lang::Ts),
        "a.b"
    );
    assert_eq!(
        module_name(root, Path::new("/p/lib/b.js"), Lang::Js),
        "lib.b"
    );
}

#[test]
fn rust_module_uses_crate_name_and_path() {
    let dir = scratch();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/a")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"my-crate\"\n").unwrap();
    let name = |f: &str| module_name(root, &root.join(f), Lang::Rust);
    assert_eq!(name("src/lib.rs"), "my_crate");
    assert_eq!(name("src/a/mod.rs"), "my_crate::a");
    assert_eq!(name("src/a/b.rs"), "my_crate::a::b");
}

#[test]
fn rust_file_outside_a_crate_uses_its_stem() {
    let dir = scratch();
    assert_eq!(
        module_name(dir.path(), &dir.path().join("x.rs"), Lang::Rust),
        "x"
    );
}

#[test]
fn crate_name_falls_back_to_directory() {
    let dir = scratch();
    std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let expected = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert_eq!(
        module_name(dir.path(), &dir.path().join("src/a.rs"), Lang::Rust),
        format!("{expected}::a")
    );
}

#[test]
fn lines_are_one_based() {
    let src = "
function a(){
}
";
    let tree = parse(Lang::Js, src).unwrap();
    let f = &functions(&tree, src, Lang::Js, "m")[0];
    assert_eq!((f.start_line, f.end_line), (2, 3));
}

#[test]
fn test_modules_are_skipped_by_name_or_by_attribute() {
    assert!(funcs(Lang::Rust, "mod tests { fn t() {} }").is_empty());
    assert!(funcs(Lang::Rust, "#[cfg(test)] mod other { fn t() {} }").is_empty());
    assert_eq!(funcs(Lang::Rust, "mod other { fn t() {} }").len(), 1);
}

#[test]
fn a_lone_src_file_keeps_its_name() {
    let root = Path::new("/p");
    assert_eq!(module_name(root, Path::new("/p/src.ts"), Lang::Ts), "src");
    assert_eq!(
        module_name(root, Path::new("/p/src/src.ts"), Lang::Ts),
        "src"
    );
}
