//! Finding, installing and starting jdtls (Eclipse JDT Language Server), the language server of
//! Java. The IDE installs it in its own folder (`~/.atelier/lsp/jdtls`) and starts it with a
//! Java 21 or later (`JAVA_HOME`, else the PATH); a `jdtls` on the PATH works as well. Each
//! project gets its own workspace folder, where jdtls keeps what it learned of the project.

use crate::languages::lsp::{
    Launch,
    provider::{Control, with_control},
};
use serde_json::json;
use std::{
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::Duration,
};

/// What the commands of the plugin ask of the provider, and what the provider tells back.
pub static CONTROL: Mutex<Control> = Mutex::new(Control::new());

const DOWNLOAD: &str =
    "https://download.eclipse.org/jdtls/snapshots/jdt-language-server-latest.tar.gz";
/// The oldest Java that runs jdtls.
pub const MIN_JAVA: u32 = 21;

fn control<R>(f: impl FnOnce(&mut Control) -> R) -> R {
    with_control(&CONTROL, f)
}

/// The folder the IDE installs language servers in (`ATELIER_LSP_DIR` overrides it, for tests).
fn managed_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ATELIER_LSP_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map_or_else(std::env::temp_dir, PathBuf::from);
    home.join(".atelier").join("lsp")
}

pub fn install_dir() -> PathBuf {
    managed_dir().join("jdtls")
}

fn quiet(program: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// The major version of `java -version`: `openjdk version "21.0.2"` is 21, `"1.8.0_202"` is 8.
pub fn major_version(output: &str) -> Option<u32> {
    let start = output.find('"')? + 1;
    let version = &output[start..start + output[start..].find('"')?];
    let mut parts = version.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

fn on_path(names: &[String]) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
}

fn executable(name: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            format!("{name}.exe"),
            format!("{name}.cmd"),
            format!("{name}.bat"),
        ]
    } else {
        vec![name.to_string()]
    }
}

/// A Java to run jdtls with, and its version: `JAVA_HOME`'s, else the one on the PATH.
pub fn java() -> Option<(PathBuf, u32)> {
    let from_home = std::env::var_os("JAVA_HOME")
        .map(|home| {
            PathBuf::from(home)
                .join("bin")
                .join(executable("java").remove(0))
        })
        .filter(|p| p.is_file());
    let program = from_home.or_else(|| on_path(&executable("java")))?;
    let output = quiet(&program).arg("-version").output().ok()?;
    // `java -version` writes on its error output.
    let text = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    Some((program, major_version(&text)?))
}

/// The launcher jar of an installation of jdtls.
pub(super) fn launcher(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir.join("plugins"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with("org.eclipse.equinox.launcher_") && n.ends_with(".jar")
            })
        })
}

/// The configuration folder of jdtls for this system.
fn configuration(dir: &Path) -> PathBuf {
    let os = if cfg!(windows) {
        "win"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    };
    let arm = cfg!(target_arch = "aarch64") && !cfg!(windows);
    let name = if arm {
        format!("config_{os}_arm")
    } else {
        format!("config_{os}")
    };
    let specific = dir.join(&name);
    if specific.is_dir() {
        specific
    } else {
        dir.join(format!("config_{os}"))
    }
}

/// Where jdtls keeps what it learned of a project: one folder per project.
pub(super) fn data_dir(root: &Path) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut hasher);
    let name = root
        .file_name()
        .map_or_else(|| "project".into(), |n| n.to_string_lossy().into_owned());
    managed_dir()
        .join("jdtls-data")
        .join(format!("{name}-{:016x}", hasher.finish()))
}

/// The arguments that start jdtls from its installation folder.
pub fn arguments(dir: &Path, launcher: &Path, root: &Path) -> Vec<String> {
    [
        "-Declipse.application=org.eclipse.jdt.ls.core.id1",
        "-Dosgi.bundles.defaultStartLevel=4",
        "-Declipse.product=org.eclipse.jdt.ls.core.product",
        "-Dlog.level=WARNING",
        "-Xmx1G",
        "--add-modules=ALL-SYSTEM",
        "--add-opens",
        "java.base/java.util=ALL-UNNAMED",
        "--add-opens",
        "java.base/java.lang=ALL-UNNAMED",
        "-jar",
    ]
    .iter()
    .map(|a| a.to_string())
    .chain([
        launcher.to_string_lossy().into_owned(),
        "-configuration".into(),
        configuration(dir).to_string_lossy().into_owned(),
        "-data".into(),
        data_dir(root).to_string_lossy().into_owned(),
    ])
    .collect()
}

/// How to start jdtls for a project: the IDE's installation with a recent enough Java, else a
/// `jdtls` on the PATH.
pub fn find(root: &Path) -> Option<Launch> {
    let dir = install_dir();
    if let Some(jar) = launcher(&dir)
        && let Some((java, version)) = java()
    {
        if version >= MIN_JAVA {
            return Some(Launch {
                program: java,
                args: arguments(&dir, &jar, root),
                options: json!({}),
            });
        }
        control(|c| {
            c.messages.push((
                trf!("java.java_too_old", version = version, min = MIN_JAVA),
                true,
            ))
        });
    }
    let wrapper = on_path(&executable("jdtls"))?;
    Some(Launch {
        program: wrapper,
        args: vec![
            "-data".into(),
            data_dir(root).to_string_lossy().into_owned(),
        ],
        options: json!({}),
    })
}

/// Downloads jdtls and unpacks it in the IDE's folder, in the background.
pub fn install() {
    let started = control(|c| !std::mem::replace(&mut c.installing, true));
    if !started {
        return;
    }
    thread::spawn(|| {
        let result = download();
        control(|c| {
            c.installing = false;
            match result {
                Ok(()) => {
                    c.restart_requested = true;
                    c.messages
                        .push((tr!("java.server_installed").into(), false));
                }
                Err(e) => c
                    .messages
                    .push((trf!("java.cannot_install_server", e = e), true)),
            }
        });
    });
}

pub(super) fn download() -> Result<(), String> {
    match java() {
        Some((_, version)) if version >= MIN_JAVA => {}
        Some((_, version)) => {
            return Err(trf!("java.java_too_old", version = version, min = MIN_JAVA));
        }
        None => return Err(trf!("java.java_missing", min = MIN_JAVA)),
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .into();
    let mut response = agent
        .get(DOWNLOAD)
        .header("User-Agent", "atelier-ide")
        .call()
        .map_err(|e| e.to_string())?;
    let archive = response
        .body_mut()
        .with_config()
        .limit(512 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    let target = install_dir();
    let staging = target.with_extension("download");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let mut tar_bytes = Vec::new();
    flate2::read::GzDecoder::new(&archive[..])
        .read_to_end(&mut tar_bytes)
        .map_err(|e| e.to_string())?;
    tar::Archive::new(&tar_bytes[..])
        .unpack(&staging)
        .map_err(|e| e.to_string())?;
    if launcher(&staging).is_none() {
        return Err(tr!("java.archive_without_launcher").into());
    }
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&staging, &target).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "../../../test/languages/java/server/tests.rs"]
mod tests;
