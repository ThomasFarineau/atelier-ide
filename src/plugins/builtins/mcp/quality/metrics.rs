//! Writing snapshots under `.metrics/`.

use super::Out;
use std::path::Path;

/// Write `.metrics/<name>` and say so in the report.
pub fn write(root: &Path, name: &str, text: &str, out: &Out) {
    let path = root.join(".metrics").join(name);
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(&path, text));
    if saved.is_ok() {
        out.line(&format!("Wrote .metrics/{name}"));
    }
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/metrics/tests.rs"]
mod tests;
