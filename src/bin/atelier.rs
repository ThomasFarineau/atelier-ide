#![cfg_attr(windows, windows_subsystem = "windows")]
//! Atelier bootstrap: the small program users start. It keeps the IDE up to date, then runs it.
//!
//! Layout of an installation, next to this program:
//!
//! ```text
//! atelier(.exe)        this bootstrap
//! app/atelier-ide(.exe) the IDE, replaced as a whole by every update
//! app/VERSION        the release it comes from
//! bootstrap.log      what the last start did
//! ```
//!
//! At every start the bootstrap asks GitHub for the latest release. When it is newer than
//! `app/VERSION` (or the IDE is not there yet), it downloads the archive of this platform,
//! checks it against the release's `SHA256SUMS`, unpacks it next to `app/` and swaps the two.
//! The IDE is not running at that moment, which is why an update never meets a locked file on
//! Windows. Offline, or on any failure, the IDE that is installed starts as it is.
//!
//! The bootstrap is also the `atelier` command: `atelier <folder|file>` is passed to the IDE, which
//! opens it in the window that is already open or in a new one. `atelier --install-cli` puts the
//! command on the PATH (done once by itself after the first install). With a path to open, the
//! check for an update is made at most every six hours, so the command stays instant.
//!
//! `ATELIER_NO_UPDATE=1` skips the check, `ATELIER_REPO=owner/name` points to another repository and
//! `ATELIER_GITHUB_TOKEN` gives access to a private one.

// The languages of the IDE, in the language of the system: the settings of the IDE are not read here.
#[allow(dead_code)]
#[macro_use]
#[path = "../i18n.rs"]
mod i18n;
// The window shown while the bootstrap checks for an update, like the updaters of Discord or VS Code.
#[path = "atelier/splash.rs"]
mod splash;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsString,
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const REPO: &str = "ThomasFarineau/atelier-ide";
const APP_DIR: &str = "app";
const EXE: &str = if cfg!(windows) {
    "atelier-ide.exe"
} else {
    "atelier-ide"
};
const BOOTSTRAP: &str = if cfg!(windows) {
    "atelier.exe"
} else {
    "atelier"
};
/// The check must never hold the start of the IDE for long.
const CHECK_TIMEOUT: Duration = Duration::from_secs(4);
/// With a path to open, the check for an update waits at least this long since the last one.
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(900);
const MAX_ARCHIVE: u64 = 1 << 30;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Where an update stands, for the window of the bootstrap.
#[derive(Clone, Debug, PartialEq)]
enum Progress {
    Checking,
    Downloading {
        tag: String,
        done: u64,
        total: u64,
    },
    Installing {
        tag: String,
    },
    Starting,
    /// The IDE could not start; the message says why.
    Failed(String),
    /// The IDE is started: the window can close.
    Done,
}

fn main() {
    i18n::set(i18n::system());
    let dir = install_dir();
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--install-cli") {
        let message = match install_cli(&dir) {
            Ok(message) => message,
            Err(e) => trf!("cannot_install_command", e = e),
        };
        eprintln!("{message}");
        rfd::MessageDialog::new()
            .set_title("Atelier")
            .set_description(message)
            .show();
        return;
    }
    let app_present = dir.join(APP_DIR).join(EXE).exists();
    let skip = env::var_os("ATELIER_NO_UPDATE").is_some();
    let check = !skip && (args.is_empty() || !app_present || check_is_due(&dir));
    // Started from its shortcut, or with nothing installed yet, the bootstrap shows its window.
    // The `atelier <path>` command stays silent, so that it is instant.
    if check && (args.is_empty() || !app_present) {
        splash::run(dir, args);
        return;
    }
    if let Progress::Failed(message) = check_and_start(&dir, args, check, &|_| {}) {
        fail(&message);
    }
}

/// Checks for an update when `check` (telling `progress` how it goes), then starts the IDE. What
/// that came to: [`Progress::Done`], or [`Progress::Failed`].
fn check_and_start(
    dir: &Path,
    args: Vec<OsString>,
    check: bool,
    progress: &dyn Fn(Progress),
) -> Progress {
    let mut log = String::new();
    if check {
        let _ = fs::write(dir.join("last_check"), now().to_string());
        progress(Progress::Checking);
        match update(dir, progress) {
            Ok(message) => log.push_str(&format!("{message}\n")),
            Err(e) => log.push_str(&trf!("cannot_update", e = e)),
        }
    }
    let _ = fs::write(dir.join("bootstrap.log"), &log);
    let app = dir.join(APP_DIR).join(EXE);
    if !app.exists() {
        return Progress::Failed(trf!("atelier_ide_not_installed_could", log = log));
    }
    progress(Progress::Starting);
    let mut command = Command::new(&app);
    command.args(args);
    // The IDE is a process of its own: what started the bootstrap (a terminal, a job) may go.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        command.creation_flags(
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB,
        );
        if command.spawn().is_ok() {
            return Progress::Done;
        }
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    match command.spawn() {
        Ok(_) => Progress::Done,
        Err(e) => Progress::Failed(trf!("cannot_start_2", app = app.display(), e = e)),
    }
}

/// The folder of this program, where the installation lives.
fn install_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Atelier IDE")
        .set_description(message)
        .show();
    std::process::exit(1);
}

/// Numbers of a release tag: `v1.2.3` is [1, 2, 3]; a suffix such as `-beta` is ignored.
fn version(tag: &str) -> Vec<u64> {
    tag.trim()
        .trim_start_matches(['v', 'V'])
        .split('-')
        .next()
        .unwrap_or("")
        .split('.')
        .map(|n| n.parse().unwrap_or(0))
        .collect()
}

/// Whether release `candidate` is newer than `installed`.
fn newer(candidate: &str, installed: &str) -> bool {
    let (a, b) = (version(candidate), version(installed));
    let len = a.len().max(b.len());
    let pad = |mut v: Vec<u64>| {
        v.resize(len, 0);
        v
    };
    pad(a) > pad(b)
}

/// Name of the release archive for this platform.
fn archive_name(os: &str, arch: &str) -> String {
    let extension = if os == "windows" { "zip" } else { "tar.gz" };
    format!("atelier-ide-{os}-{arch}.{extension}")
}

/// The checksum of `name` in the text of a `SHA256SUMS` file.
fn checksum_of(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| hash.to_lowercase())
    })
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

/// A GET with the headers GitHub wants, and the token when there is one.
fn get(agent: &ureq::Agent, url: &str, accept: &str) -> Result<Vec<u8>> {
    get_with(agent, url, accept, &mut |_| {})
}

/// [`get`], telling `read` how many bytes have come so far.
fn get_with(
    agent: &ureq::Agent,
    url: &str,
    accept: &str,
    read: &mut dyn FnMut(u64),
) -> Result<Vec<u8>> {
    let mut request = agent
        .get(url)
        .header("User-Agent", "atelier-bootstrap")
        .header("Accept", accept);
    if let Ok(token) = env::var("ATELIER_GITHUB_TOKEN") {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let mut response = request.call()?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(MAX_ARCHIVE)
        .reader();
    let mut data = Vec::new();
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            return Ok(data);
        }
        data.extend_from_slice(&chunk[..n]);
        read(data.len() as u64);
    }
}

/// Checks for a newer release and installs it. What happened, for the log.
fn update(dir: &Path, progress: &dyn Fn(Progress)) -> Result<String> {
    cleanup(dir);
    let installed = fs::read_to_string(dir.join(APP_DIR).join("VERSION"))
        .ok()
        .map(|v| v.trim().to_string());
    let present = dir.join(APP_DIR).join(EXE).exists();
    let repo = env::var("ATELIER_REPO").unwrap_or_else(|_| REPO.to_string());
    let latest = get(
        &agent(CHECK_TIMEOUT),
        &format!("https://api.github.com/repos/{repo}/releases/latest"),
        "application/vnd.github+json",
    );
    // GitHub answers 404 both for a repository without a release and for a private one.
    if let Err(e) = &latest
        && let Some(ureq::Error::StatusCode(404)) = e.downcast_ref::<ureq::Error>()
    {
        return Err(trf!("no_release_found", repo = repo).into());
    }
    let release: Value = serde_json::from_slice(&latest?)?;
    let tag = release["tag_name"]
        .as_str()
        .ok_or(tr!("answer_github_without_version"))?
        .to_string();
    if present && installed.as_deref().is_some_and(|v| !newer(&tag, v)) {
        return Ok(trf!("up_date", tag = tag));
    }
    let name = archive_name(env::consts::OS, env::consts::ARCH);
    let assets = release["assets"]
        .as_array()
        .ok_or(tr!("release_without_files"))?;
    let find = |wanted: &str| assets.iter().find(|a| a["name"] == wanted);
    let asset = find(&name).ok_or_else(|| trf!("release_has_no", tag = tag, name = name))?;
    let sums = find("SHA256SUMS").ok_or_else(|| trf!("release_has_no_sha256sums", tag = tag))?;
    // A private repository serves its files through the API address, a public one directly.
    let address = |asset: &Value| -> String {
        let key = if env::var_os("ATELIER_GITHUB_TOKEN").is_some() {
            "url"
        } else {
            "browser_download_url"
        };
        asset[key].as_str().unwrap_or_default().to_string()
    };
    let slow = agent(DOWNLOAD_TIMEOUT);
    let sums = String::from_utf8(get(&slow, &address(sums), "application/octet-stream")?)?;
    let expected =
        checksum_of(&sums, &name).ok_or_else(|| trf!("missing_from_sha256sums", name = name))?;
    let total = asset["size"].as_u64().unwrap_or(0);
    let downloading = |done| {
        progress(Progress::Downloading {
            tag: tag.clone(),
            done,
            total,
        })
    };
    downloading(0);
    let archive = get_with(
        &slow,
        &address(asset),
        "application/octet-stream",
        &mut |done| downloading(done),
    )?;
    progress(Progress::Installing { tag: tag.clone() });
    let actual = hex(&Sha256::digest(&archive));
    if actual != expected {
        return Err(trf!("wrong_checksum", name = name).into());
    }
    install(dir, &name, &archive, &tag)?;
    Ok(match installed {
        Some(old) => trf!("updated", old = old, tag = tag),
        None => {
            // The first install also makes `atelier` a command.
            let cli = install_cli(dir).unwrap_or_else(|e| trf!("command_not_installed", e = e));
            trf!("installed_lower", tag = tag, cli = cli)
        }
    })
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Whether the last look for an update is old enough to make another one.
fn check_is_due(dir: &Path) -> bool {
    let last: u64 = fs::read_to_string(dir.join("last_check"))
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0);
    now().saturating_sub(last) >= CHECK_EVERY.as_secs()
}

/// Makes `atelier` a command of the terminal. What was done, for the user.
fn install_cli(dir: &Path) -> Result<String> {
    let me = env::current_exe()?;
    #[cfg(windows)]
    {
        // The command is this program under the name `atelier.exe`, in a folder of the PATH.
        let command = dir.join(BOOTSTRAP);
        if me != command {
            fs::copy(&me, &command)?;
        }
        let folder = dir.display().to_string().replace('\'', "''");
        let script = format!(
            "$p=[Environment]::GetEnvironmentVariable('Path','User'); \
             if (($p -split ';') -notcontains '{folder}') {{ \
             [Environment]::SetEnvironmentVariable('Path',($p.TrimEnd(';')+';{folder}'),'User') }}"
        );
        // Without this flag, PowerShell opens a console window over the IDE that is starting.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let status = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .status()?;
        if !status.success() {
            return Err(tr!("powershell_could_not_change_path").into());
        }
        Ok(trf!(
            "atelier_command_available_new_terminals",
            dir = dir.display()
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or(tr!("home_unknown"))?;
        let bin = home.join(".local").join("bin");
        fs::create_dir_all(&bin)?;
        let link = bin.join("atelier");
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&me, &link)?;
        let on_path = env::var_os("PATH").is_some_and(|p| env::split_paths(&p).any(|d| d == bin));
        Ok(if on_path {
            trf!("atelier_command_installed", link = link.display())
        } else {
            trf!(
                "installed_add_path_use",
                link = link.display(),
                bin = bin.display()
            )
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Leftovers of a previous update: the folder it replaced, the bootstrap it renamed.
fn cleanup(dir: &Path) {
    let _ = fs::remove_dir_all(dir.join("app.new"));
    let _ = fs::remove_dir_all(dir.join("app.old"));
    let _ = fs::remove_file(dir.join(format!("{BOOTSTRAP}.old")));
}

/// Unpacks `archive` and puts it in place of `app/`.
fn install(dir: &Path, name: &str, archive: &[u8], tag: &str) -> Result<()> {
    let staging = dir.join("app.new");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let unpacked = unpack(name, archive, &staging).and_then(|()| {
        if staging.join(EXE).exists() {
            Ok(())
        } else {
            Err(trf!("does_not_hold", name = name, EXE = EXE).into())
        }
    });
    if let Err(e) = unpacked {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }
    fs::write(staging.join("VERSION"), tag)?;
    // A new bootstrap in the archive replaces this one: a running program can be renamed.
    let fresh = staging.join(BOOTSTRAP);
    if fresh.exists() {
        let current = dir.join(BOOTSTRAP);
        let old = dir.join(format!("{BOOTSTRAP}.old"));
        let _ = fs::remove_file(&old);
        if fs::rename(&current, &old).is_ok() && fs::rename(&fresh, &current).is_err() {
            let _ = fs::rename(&old, &current);
        }
    }
    let app = dir.join(APP_DIR);
    let old = dir.join("app.old");
    let _ = fs::remove_dir_all(&old);
    if app.exists() {
        fs::rename(&app, &old)?;
    }
    if let Err(e) = fs::rename(&staging, &app) {
        // Back to what was there.
        let _ = fs::rename(&old, &app);
        return Err(e.into());
    }
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

fn unpack(name: &str, archive: &[u8], into: &Path) -> Result<()> {
    if name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(Cursor::new(archive))?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            // Names that climb out of the folder are skipped.
            let Some(relative) = entry.enclosed_name() else {
                continue;
            };
            let target = into.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&target)?;
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            fs::write(&target, data)?;
        }
    } else {
        tar::Archive::new(flate2::read::GzDecoder::new(Cursor::new(archive))).unpack(into)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for exe in [EXE, BOOTSTRAP] {
            let path = into.join(exe);
            if path.exists() {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../test/bin/atelier/tests.rs"]
mod tests;
