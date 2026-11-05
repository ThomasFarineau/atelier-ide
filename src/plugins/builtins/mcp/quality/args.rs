//! The options of a quality tool, as command-line words: the MCP arguments are turned into them
//! (see `super::argv`), so that each tool parses and checks its own.

use super::Out;
use std::collections::{HashMap, HashSet};

pub struct Args {
    flags: HashSet<String>,
    values: HashMap<String, String>,
    pub positional: Vec<String>,
}

impl Args {
    pub fn parse(argv: &[String], flags: &[&str], values: &[&str]) -> Result<Args, String> {
        let mut args = Args {
            flags: HashSet::new(),
            values: HashMap::new(),
            positional: Vec::new(),
        };
        let mut it = argv.iter();
        while let Some(a) = it.next() {
            let Some(name) = a.strip_prefix("--") else {
                args.positional.push(a.clone());
                continue;
            };
            let (name, inline) = match name.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (name, None),
            };
            if flags.contains(&name) {
                args.flags.insert(name.to_string());
            } else if values.contains(&name) {
                let v = inline
                    .or_else(|| it.next().cloned())
                    .ok_or(format!("--{name} needs a value"))?;
                args.values.insert(name.to_string(), v);
            } else {
                return Err(format!("unknown option --{name}"));
            }
        }
        Ok(args)
    }

    pub fn has(&self, flag: &str) -> bool {
        self.flags.contains(flag)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        match self.value(name) {
            None => Ok(default),
            Some(v) => v.parse().map_err(|_| format!("--{name}: bad number '{v}'")),
        }
    }
}

/// Turn a tool's result into its exit code, writing an error in its report.
pub fn exit(tool: &str, result: Result<i32, String>, out: &Out) -> i32 {
    result.unwrap_or_else(|e| {
        out.line(&format!("{tool}: {e}"));
        1
    })
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/args/tests.rs"]
mod tests;
