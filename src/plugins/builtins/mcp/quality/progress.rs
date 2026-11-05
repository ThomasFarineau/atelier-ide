//! How long the steps of a run took.

use std::time::Duration;

/// `42s`, `3m05s` or `1h02m`.
pub fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m{:02}s", s / 60, s % 60),
        _ => format!("{}h{:02}m", s / 3600, s % 3600 / 60),
    }
}

/// One-line account of a finished parallel step.
pub fn summary(what: &str, jobs: usize, elapsed: Duration) -> String {
    format!("{what} on {jobs} thread(s) in {}", fmt_duration(elapsed))
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/progress/tests.rs"]
mod tests;
