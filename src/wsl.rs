//! Projects that live in a WSL distribution, reached from Windows through
//! `\\wsl.localhost\<distro>` (or the older `\\wsl$\<distro>`). The IDE reads and writes their files
//! through that share and runs the terminal, git and the run commands inside the distribution.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslPath {
    pub distro: String,
    /// Absolute path inside the distribution, e.g. `/home/thomas/workspace`.
    pub linux: String,
}

/// The distribution and Linux path of a folder or file on a WSL share, with or without the
/// `\\?\UNC\` prefix of canonical paths. None for every other path.
pub fn parse(path: &Path) -> Option<WslPath> {
    let text = path.to_string_lossy();
    let rest = text
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| text.strip_prefix(r"\\"))?;
    let mut parts = rest.split('\\').filter(|p| !p.is_empty());
    let host = parts.next()?;
    if !host.eq_ignore_ascii_case("wsl.localhost") && !host.eq_ignore_ascii_case("wsl$") {
        return None;
    }
    let distro = parts.next()?.to_string();
    let linux: Vec<&str> = parts.collect();
    Some(WslPath {
        distro,
        linux: format!("/{}", linux.join("/")),
    })
}

/// The Windows path of a Linux path inside `distro`.
pub fn to_windows(distro: &str, linux: &str) -> PathBuf {
    let mut path = format!(r"\\wsl.localhost\{distro}");
    for part in linux.split('/').filter(|p| !p.is_empty()) {
        path.push('\\');
        path.push_str(part);
    }
    PathBuf::from(path)
}

/// A word quoted as a literal for a POSIX shell.
pub fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// A command line for a POSIX shell: every word is quoted as a literal.
pub fn shell_command(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(quote)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
#[path = "../test/wsl/tests.rs"]
mod tests;
