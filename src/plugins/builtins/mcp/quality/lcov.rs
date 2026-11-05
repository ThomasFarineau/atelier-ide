//! LCOV coverage reports.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[derive(Default, Clone)]
pub struct FileCov {
    pub lines: BTreeMap<usize, u64>,
    pub branches: Vec<(usize, bool)>,
}

#[derive(Default)]
pub struct Coverage {
    files: HashMap<String, FileCov>,
}

/// Stable key for comparing paths from different sources.
pub fn key(path: &Path) -> String {
    let p = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s).to_string();
    if cfg!(windows) { s.to_lowercase() } else { s }
}

impl Coverage {
    pub fn parse(text: &str, base: &Path) -> Coverage {
        let mut cov = Coverage::default();
        let mut current: Option<String> = None;
        for line in text.lines() {
            let line = line.trim();
            if let Some(sf) = line.strip_prefix("SF:") {
                let k = key(&base.join(sf));
                cov.files.entry(k.clone()).or_default();
                current = Some(k);
            } else if let Some(da) = line.strip_prefix("DA:") {
                let mut it = da.split(',');
                if let (Some(l), Some(h), Some(k)) = (it.next(), it.next(), &current)
                    && let (Ok(l), Ok(h)) = (l.parse::<usize>(), h.parse::<u64>())
                    && let Some(file) = cov.files.get_mut(k)
                {
                    *file.lines.entry(l).or_insert(0) += h;
                }
            } else if let Some(br) = line.strip_prefix("BRDA:") {
                let f: Vec<&str> = br.split(',').collect();
                if let (Some(l), Some(t), Some(k)) = (f.first(), f.get(3), &current)
                    && let Ok(l) = l.parse::<usize>()
                    && let Some(file) = cov.files.get_mut(k)
                {
                    file.branches
                        .push((l, t.parse::<u64>().is_ok_and(|n| n > 0)));
                }
            } else if line == "end_of_record" {
                current = None;
            }
        }
        cov
    }

    pub fn merge(&mut self, other: Coverage) {
        self.files.extend(other.files);
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn file(&self, path: &Path) -> Option<&FileCov> {
        self.files.get(&key(path))
    }

    /// Percent (0-100) of a function exercised: branches if recorded, else lines.
    pub fn function_percent(&self, path: &Path, start: usize, end: usize) -> f64 {
        let Some(f) = self.file(path) else {
            return 0.0;
        };
        let branches: Vec<bool> = f
            .branches
            .iter()
            .filter(|(l, _)| (start..=end).contains(l))
            .map(|(_, t)| *t)
            .collect();
        if !branches.is_empty() {
            return percent(branches.iter().filter(|t| **t).count(), branches.len());
        }
        let lines: Vec<u64> = f.lines.range(start..=end).map(|(_, h)| *h).collect();
        if lines.is_empty() {
            return 0.0;
        }
        percent(lines.iter().filter(|h| **h > 0).count(), lines.len())
    }
}

fn percent(hit: usize, total: usize) -> f64 {
    hit as f64 * 100.0 / total as f64
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/lcov/tests.rs"]
mod tests;
