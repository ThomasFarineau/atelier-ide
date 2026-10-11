//! `rustfmt` on the text of the editor, for Reformat Code without the language server (and
//! when the server cannot). It reads the text on its standard input, from the folder of the
//! file so that the `rustfmt.toml` of the project applies, with the edition of the crate.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// The `Cargo.toml` of the crate a file belongs to.
fn manifest(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .map(|dir| dir.join("Cargo.toml"))
        .find(|m| m.is_file())
}

/// The edition of the crate of a file, as its manifest says (2015 when it says nothing, as
/// Cargo reads it).
pub fn edition(path: Option<&Path>) -> String {
    let Some(text) = path
        .and_then(manifest)
        .and_then(|m| std::fs::read_to_string(m).ok())
    else {
        return "2021".into();
    };
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return "2021".into();
    };
    doc.get("package")
        .and_then(|p| p.get("edition"))
        .and_then(|e| e.as_str())
        .unwrap_or("2015")
        .to_string()
}

/// The text formatted by `rustfmt`, or why it could not be.
pub fn rustfmt(text: &str, path: Option<&Path>) -> Result<String, String> {
    // The line endings of the document stay as they are, whatever the platform.
    let newlines = if text.contains("\r\n") {
        "newline_style=Windows"
    } else {
        "newline_style=Unix"
    };
    let mut command = Command::new("rustfmt");
    command
        .args(["--edition", &edition(path), "--config", newlines])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = path.and_then(Path::parent).filter(|d| d.is_dir()) {
        command.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            tr!("rust.rustfmt_not_found").to_string()
        } else {
            e.to_string()
        }
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        let input = text.as_bytes().to_vec();
        // A separate thread, so a large text cannot block while rustfmt fills its output.
        std::thread::spawn(move || stdin.write_all(&input));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if output.status.success() {
        return String::from_utf8(output.stdout).map_err(|e| e.to_string());
    }
    // rustfmt says where the code does not parse: the first lines are enough.
    let error = String::from_utf8_lossy(&output.stderr);
    let shown: Vec<&str> = error
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .collect();
    Err(trf!("rust.rustfmt_failed", error = shown.join(" ")))
}

#[cfg(test)]
#[path = "../../../test/languages/rust/format/tests.rs"]
mod tests;
