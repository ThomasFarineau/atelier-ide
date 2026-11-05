//! Producing and loading LCOV coverage for a project.

use super::lang::Lang;
use super::lcov::Coverage;
use super::{Out, run as state, shell};
use std::time::Duration;

/// A coverage run that has not finished in this long is stopped.
const COVERAGE_LIMIT: Duration = Duration::from_secs(1800);
use serde_json::Value;
use std::path::{Path, PathBuf};

const REPORTS: &[&str] = &[
    "coverage/lcov.info",
    "lcov.info",
    "target/lcov.info",
    "target/llvm-cov/lcov.info",
];
const VITEST_PROVIDER: &str = "node_modules/@vitest/coverage-v8";

/// Runs a shell command in a directory; true when it succeeds.
pub type Exec<'a> = &'a dyn Fn(&str, &Path) -> bool;

fn load(paths: &[PathBuf], root: &Path) -> Coverage {
    let mut cov = Coverage::default();
    for text in paths.iter().filter_map(|p| std::fs::read_to_string(p).ok()) {
        cov.merge(Coverage::parse(&text, root));
    }
    cov
}

/// Coverage already on disk, if any.
pub fn load_existing(root: &Path) -> Option<Coverage> {
    let reports: Vec<PathBuf> = REPORTS
        .iter()
        .map(|r| root.join(r))
        .filter(|p| p.is_file())
        .collect();
    let cov = load(&reports, root);
    (!cov.is_empty()).then_some(cov)
}

fn package_json(root: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(root.join("package.json")).map_err(|_| "no package.json")?;
    serde_json::from_str(&text).map_err(|e| format!("package.json: {e}"))
}

fn uses_vitest(pkg: &Value) -> bool {
    pkg.to_string().contains("\"vitest")
}

fn has_script(pkg: &Value, name: &str) -> bool {
    pkg["scripts"][name].is_string()
}

/// Command that runs the project's tests (JS/TS side).
pub fn node_test_command(root: &Path) -> Result<String, String> {
    let pkg = package_json(root)?;
    if has_script(&pkg, "test") {
        Ok("npm test --silent".into())
    } else if uses_vitest(&pkg) {
        Ok("npx vitest run".into())
    } else {
        Err("no test script in package.json".into())
    }
}

fn node_coverage_command(pkg: &Value) -> Result<String, String> {
    if has_script(pkg, "coverage") {
        Ok("npm run coverage".into())
    } else if uses_vitest(pkg) {
        Ok("npx vitest run --coverage --coverage.reporter=lcov".into())
    } else if has_script(pkg, "test") {
        Ok("npx c8 --reporter=lcov npm test".into())
    } else {
        Err("no coverage or test script in package.json".into())
    }
}

fn run_report(root: &Path, report: &str, cmd: &str, exec: Exec) -> Option<Coverage> {
    let path = root.join(report);
    let _ = std::fs::remove_file(&path);
    exec(cmd, root);
    path.is_file().then(|| load(&[path], root))
}

/// Whether the files include (script, rust) sources.
pub fn kinds(files: &[PathBuf]) -> (bool, bool) {
    let langs: Vec<Lang> = files.iter().filter_map(|f| Lang::from_path(f)).collect();
    (
        langs.iter().any(|l| !l.is_rust()),
        langs.iter().any(|l| l.is_rust()),
    )
}

fn run_script(root: &Path, exec: Exec) -> Result<Coverage, String> {
    let pkg = package_json(root)?;
    let cmd = node_coverage_command(&pkg)?;
    if cmd.contains("--coverage") && !root.join(VITEST_PROVIDER).exists() {
        exec("npm install --no-save @vitest/coverage-v8", root);
    }
    run_report(root, "coverage/lcov.info", &cmd, exec)
        .ok_or("no coverage/lcov.info produced".into())
}

fn run_rust(root: &Path, exec: Exec) -> Result<Coverage, String> {
    let llvm = "cargo llvm-cov --lcov --output-path target/lcov.info";
    let tarpaulin = "cargo tarpaulin --out Lcov --output-dir target";
    run_report(root, "target/lcov.info", llvm, exec)
        .or_else(|| run_report(root, "target/lcov.info", tarpaulin, exec))
        .ok_or("no Rust coverage produced (install cargo-llvm-cov)".into())
}

/// Run the projects' coverage tools and load what they write.
/// The commands are named in the report; their output is not kept.
pub fn run(root: &Path, script: bool, rust: bool, out: &Out) -> Result<Coverage, String> {
    let exec = |cmd: &str, dir: &Path| {
        state::checkpoint();
        state::phase(&format!("coverage: {cmd}"));
        out.line(&format!("$ {cmd}"));
        shell::run_quiet(cmd, dir, COVERAGE_LIMIT, &[]) == shell::Outcome::Pass
    };
    run_with(root, script, rust, &exec)
}

pub fn run_with(root: &Path, script: bool, rust: bool, exec: Exec) -> Result<Coverage, String> {
    let mut cov = Coverage::default();
    if script && root.join("package.json").is_file() {
        cov.merge(run_script(root, exec)?);
    }
    if rust && root.join("Cargo.toml").is_file() {
        cov.merge(run_rust(root, exec)?);
    }
    Ok(cov)
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/coverage/tests.rs"]
mod tests;
