use super::*;
use crate::plugins::builtins::mcp::quality::{Scratch, scratch};
use std::cell::RefCell;

const REPORT: &str = "SF:src/a.js\nDA:1,1\nend_of_record\n";

fn project(files: &[(&str, &str)]) -> Scratch {
    let dir = scratch();
    for (name, body) in files {
        let p = dir.path().join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    dir
}

/// Fake runner: records commands, and writes `report` when the command mentions `trigger`.
fn fake<'a>(
    log: &'a RefCell<Vec<String>>,
    trigger: &'a str,
    report: &'a str,
) -> impl Fn(&str, &Path) -> bool + 'a {
    move |cmd, dir| {
        log.borrow_mut().push(cmd.to_string());
        if cmd.contains(trigger) {
            let p = dir.join(report);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, REPORT).unwrap();
        }
        true
    }
}

fn pkg(json: &str) -> Value {
    serde_json::from_str(json).unwrap()
}

#[test]
fn coverage_script_wins_then_vitest_then_c8() {
    let cmd = |j: &str| node_coverage_command(&pkg(j));
    assert_eq!(
        cmd(r#"{"scripts":{"coverage":"x","test":"vitest"}}"#).unwrap(),
        "npm run coverage"
    );
    assert!(
        cmd(r#"{"devDependencies":{"vitest":"1"}}"#)
            .unwrap()
            .starts_with("npx vitest run --coverage")
    );
    assert_eq!(
        cmd(r#"{"scripts":{"test":"mocha"}}"#).unwrap(),
        "npx c8 --reporter=lcov npm test"
    );
    assert!(cmd("{}").is_err());
}

#[test]
fn test_command_prefers_the_test_script() {
    let dir = project(&[(
        "package.json",
        r#"{"scripts":{"test":"x"},"devDependencies":{"vitest":"1"}}"#,
    )]);
    assert_eq!(node_test_command(dir.path()).unwrap(), "npm test --silent");
    let dir = project(&[("package.json", r#"{"devDependencies":{"vitest":"1"}}"#)]);
    assert_eq!(node_test_command(dir.path()).unwrap(), "npx vitest run");
    let dir = project(&[("package.json", "{}")]);
    assert!(node_test_command(dir.path()).is_err());
    assert!(node_test_command(scratch().path()).is_err());
}

#[test]
fn malformed_package_json_is_reported() {
    let dir = project(&[("package.json", "{")]);
    assert!(
        package_json(dir.path())
            .unwrap_err()
            .starts_with("package.json:")
    );
}

#[test]
fn existing_reports_are_loaded() {
    let dir = project(&[("coverage/lcov.info", REPORT)]);
    assert!(
        load_existing(dir.path())
            .unwrap()
            .file(&dir.path().join("src/a.js"))
            .is_some()
    );
    assert!(load_existing(scratch().path()).is_none());
}

#[test]
fn script_coverage_installs_the_vitest_provider_once_needed() {
    let dir = project(&[("package.json", r#"{"devDependencies":{"vitest":"1"}}"#)]);
    let log = RefCell::new(vec![]);
    let cov = run_with(
        dir.path(),
        true,
        false,
        &fake(&log, "vitest run", "coverage/lcov.info"),
    )
    .unwrap();
    assert!(!cov.is_empty());
    let log = log.borrow();
    assert!(log[0].starts_with("npm install") && log[1].starts_with("npx vitest run --coverage"));
}

#[test]
fn provider_already_installed_is_not_reinstalled() {
    let dir = project(&[
        ("package.json", r#"{"scripts":{"coverage":"c"}}"#),
        ("node_modules/@vitest/coverage-v8/x", ""),
    ]);
    let log = RefCell::new(vec![]);
    run_with(
        dir.path(),
        true,
        false,
        &fake(&log, "npm run", "coverage/lcov.info"),
    )
    .unwrap();
    assert_eq!(*log.borrow(), vec!["npm run coverage"]);
}

#[test]
fn missing_script_report_is_an_error() {
    let dir = project(&[("package.json", r#"{"scripts":{"coverage":"c"}}"#)]);
    let log = RefCell::new(vec![]);
    let err = run_with(dir.path(), true, false, &fake(&log, "never", "x"))
        .err()
        .unwrap();
    assert!(err.contains("no coverage/lcov.info"));
}

#[test]
fn rust_falls_back_to_tarpaulin() {
    let dir = project(&[("Cargo.toml", "[package]\n")]);
    let log = RefCell::new(vec![]);
    run_with(
        dir.path(),
        false,
        true,
        &fake(&log, "tarpaulin", "target/lcov.info"),
    )
    .unwrap();
    assert!(
        log.borrow()[0].starts_with("cargo llvm-cov")
            && log.borrow()[1].starts_with("cargo tarpaulin")
    );
    let err = run_with(
        dir.path(),
        false,
        true,
        &fake(&RefCell::new(vec![]), "never", "x"),
    )
    .err()
    .unwrap();
    assert!(err.contains("cargo-llvm-cov"));
}

#[test]
fn nothing_to_run_without_manifests() {
    let dir = scratch();
    let log = RefCell::new(vec![]);
    assert!(
        run_with(dir.path(), true, true, &fake(&log, "x", "x"))
            .unwrap()
            .is_empty()
    );
    assert!(log.borrow().is_empty());
}

#[test]
fn kinds_tells_script_from_rust() {
    let files = [PathBuf::from("a.ts"), PathBuf::from("b.rs")];
    assert_eq!(kinds(&files), (true, true));
    assert_eq!(kinds(&files[1..]), (false, true));
    assert_eq!(kinds(&[]), (false, false));
}

#[test]
fn coverage_scripts_do_not_trigger_the_provider_install() {
    let dir = project(&[("package.json", r#"{"scripts":{"coverage":"c"}}"#)]);
    let log = RefCell::new(vec![]);
    run_with(
        dir.path(),
        true,
        false,
        &fake(&log, "npm run", "coverage/lcov.info"),
    )
    .unwrap();
    assert_eq!(*log.borrow(), vec!["npm run coverage"]);
}
