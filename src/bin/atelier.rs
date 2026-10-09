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
const EXE: &str = if cfg!(windows) { "atelier-ide.exe" } else { "atelier-ide" };
const BOOTSTRAP: &str = if cfg!(windows) { "atelier.exe" } else { "atelier" };
/// The check must never hold the start of the IDE for long.
const CHECK_TIMEOUT: Duration = Duration::from_secs(4);
/// With a path to open, the check for an update waits at least this long since the last one.
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(900);
const MAX_ARCHIVE: u64 = 1 << 30;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    let dir = install_dir();
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--install-cli") {
        let message = match install_cli(&dir) {
            Ok(message) => message,
            Err(e) => format!("Impossible d'installer la commande : {e}"),
        };
        eprintln!("{message}");
        rfd::MessageDialog::new().set_title("Atelier").set_description(message).show();
        return;
    }
    let mut log = String::new();
    let app_present = dir.join(APP_DIR).join(EXE).exists();
    let skip = env::var_os("ATELIER_NO_UPDATE").is_some();
    if !skip && (args.is_empty() || !app_present || check_is_due(&dir)) {
        let _ = fs::write(dir.join("last_check"), now().to_string());
        match update(&dir) {
            Ok(message) => log.push_str(&format!("{message}\n")),
            Err(e) => log.push_str(&format!("mise à jour impossible : {e}\n")),
        }
    }
    let _ = fs::write(dir.join("bootstrap.log"), &log);
    let app = dir.join(APP_DIR).join(EXE);
    if !app.exists() {
        fail(&format!("Atelier IDE n'est pas installé et n'a pas pu être téléchargé.\n\n{log}"));
    }
    if let Err(e) = Command::new(&app).args(args).spawn() {
        fail(&format!("Impossible de lancer {} : {e}", app.display()));
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
    rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("Atelier IDE").set_description(message).show();
    std::process::exit(1);
}

/// Numbers of a release tag: `v1.2.3` is [1, 2, 3]; a suffix such as `-beta` is ignored.
fn version(tag: &str) -> Vec<u64> {
    tag.trim().trim_start_matches(['v', 'V']).split('-').next().unwrap_or("").split('.').map(|n| n.parse().unwrap_or(0)).collect()
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
    ureq::Agent::config_builder().timeout_global(Some(timeout)).build().into()
}

/// A GET with the headers GitHub wants, and the token when there is one.
fn get(agent: &ureq::Agent, url: &str, accept: &str) -> Result<Vec<u8>> {
    let mut request = agent.get(url).header("User-Agent", "atelier-bootstrap").header("Accept", accept);
    if let Ok(token) = env::var("ATELIER_GITHUB_TOKEN") {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let mut response = request.call()?;
    Ok(response.body_mut().with_config().limit(MAX_ARCHIVE).read_to_vec()?)
}

/// Checks for a newer release and installs it. What happened, for the log.
fn update(dir: &Path) -> Result<String> {
    cleanup(dir);
    let installed = fs::read_to_string(dir.join(APP_DIR).join("VERSION")).ok().map(|v| v.trim().to_string());
    let present = dir.join(APP_DIR).join(EXE).exists();
    let repo = env::var("ATELIER_REPO").unwrap_or_else(|_| REPO.to_string());
    let release: Value = serde_json::from_slice(&get(
        &agent(CHECK_TIMEOUT),
        &format!("https://api.github.com/repos/{repo}/releases/latest"),
        "application/vnd.github+json",
    )?)?;
    let tag = release["tag_name"].as_str().ok_or("réponse de GitHub sans version")?.to_string();
    if present && installed.as_deref().is_some_and(|v| !newer(&tag, v)) {
        return Ok(format!("à jour ({tag})"));
    }
    let name = archive_name(env::consts::OS, env::consts::ARCH);
    let assets = release["assets"].as_array().ok_or("release sans fichiers")?;
    let find = |wanted: &str| assets.iter().find(|a| a["name"] == wanted);
    let asset = find(&name).ok_or_else(|| format!("la release {tag} n'a pas de {name}"))?;
    let sums = find("SHA256SUMS").ok_or_else(|| format!("la release {tag} n'a pas de SHA256SUMS"))?;
    // A private repository serves its files through the API address, a public one directly.
    let address = |asset: &Value| -> String {
        let key = if env::var_os("ATELIER_GITHUB_TOKEN").is_some() { "url" } else { "browser_download_url" };
        asset[key].as_str().unwrap_or_default().to_string()
    };
    let slow = agent(DOWNLOAD_TIMEOUT);
    let sums = String::from_utf8(get(&slow, &address(sums), "application/octet-stream")?)?;
    let expected = checksum_of(&sums, &name).ok_or_else(|| format!("{name} absent de SHA256SUMS"))?;
    let archive = get(&slow, &address(asset), "application/octet-stream")?;
    let actual = hex(&Sha256::digest(&archive));
    if actual != expected {
        return Err(format!("somme de contrôle incorrecte pour {name}").into());
    }
    install(dir, &name, &archive, &tag)?;
    Ok(match installed {
        Some(old) => format!("mis à jour : {old} → {tag}"),
        None => {
            // The first install also makes `atelier` a command.
            let cli = install_cli(dir).unwrap_or_else(|e| format!("commande non installée : {e}"));
            format!("installé : {tag} ({cli})")
        }
    })
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Whether the last look for an update is old enough to make another one.
fn check_is_due(dir: &Path) -> bool {
    let last: u64 = fs::read_to_string(dir.join("last_check")).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
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
        let status = Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).status()?;
        if !status.success() {
            return Err("PowerShell n'a pas pu modifier le PATH".into());
        }
        Ok(format!("La commande « atelier » est disponible dans les nouveaux terminaux ({}).", dir.display()))
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
        let home = env::var_os("HOME").map(PathBuf::from).ok_or("HOME inconnu")?;
        let bin = home.join(".local").join("bin");
        fs::create_dir_all(&bin)?;
        let link = bin.join("atelier");
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&me, &link)?;
        let on_path = env::var_os("PATH").is_some_and(|p| env::split_paths(&p).any(|d| d == bin));
        Ok(if on_path {
            format!("La commande « atelier » est installée : {}", link.display())
        } else {
            format!("Installée : {}. Ajoutez {} au PATH pour l'utiliser.", link.display(), bin.display())
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
        if staging.join(EXE).exists() { Ok(()) } else { Err(format!("{name} ne contient pas {EXE}").into()) }
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
            let Some(relative) = entry.enclosed_name() else { continue };
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
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("atelier-bootstrap-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *data).unwrap();
        }
        let tar = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        gz.finish().unwrap()
    }

    fn zipped(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(newer("v0.10.0", "v0.9.3"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(newer("v1.2", "v1.1.9"));
        assert!(!newer("v1.2.0", "1.2"), "the same release");
        assert!(!newer("v0.0.1", "v0.0.2"));
        assert!(newer("v2.0.0-beta", "v1.9.0"), "a suffix is ignored");
    }

    #[test]
    fn archives_and_checksums_are_found_by_name() {
        assert_eq!(archive_name("windows", "x86_64"), "atelier-ide-windows-x86_64.zip");
        assert_eq!(archive_name("macos", "aarch64"), "atelier-ide-macos-aarch64.tar.gz");
        let sums = "AABB  atelier-ide-linux-x86_64.tar.gz\nccdd *atelier-ide-windows-x86_64.zip\n";
        assert_eq!(checksum_of(sums, "atelier-ide-linux-x86_64.tar.gz").as_deref(), Some("aabb"));
        assert_eq!(checksum_of(sums, "atelier-ide-windows-x86_64.zip").as_deref(), Some("ccdd"));
        assert_eq!(checksum_of(sums, "other.zip"), None);
        assert_eq!(hex(&Sha256::digest(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn an_update_replaces_the_app_whole_and_a_broken_one_changes_nothing() {
        for (name, archive) in [
            ("atelier-ide-linux-x86_64.tar.gz", tar_gz(&[(EXE, b"new ide"), ("extra.txt", b"x")])),
            ("atelier-ide-windows-x86_64.zip", zipped(&[(EXE, b"new ide"), ("extra.txt", b"x")])),
        ] {
            let dir = scratch("install");
            fs::create_dir_all(dir.join(APP_DIR)).unwrap();
            fs::write(dir.join(APP_DIR).join(EXE), "old ide").unwrap();
            fs::write(dir.join(APP_DIR).join("stale.txt"), "gone").unwrap();
            fs::write(dir.join(APP_DIR).join("VERSION"), "v0.0.1").unwrap();
            install(&dir, name, &archive, "v0.0.2").unwrap();
            assert_eq!(fs::read_to_string(dir.join(APP_DIR).join(EXE)).unwrap(), "new ide");
            assert_eq!(fs::read_to_string(dir.join(APP_DIR).join("VERSION")).unwrap(), "v0.0.2");
            assert!(!dir.join(APP_DIR).join("stale.txt").exists(), "the old files go with the old version");
            assert!(!dir.join("app.new").exists() && !dir.join("app.old").exists());
            // An archive without the IDE is refused and the installation stays.
            let broken = if name.ends_with(".zip") { zipped(&[("readme.txt", b"?")]) } else { tar_gz(&[("readme.txt", b"?")]) };
            assert!(install(&dir, name, &broken, "v0.0.3").is_err());
            assert_eq!(fs::read_to_string(dir.join(APP_DIR).join("VERSION")).unwrap(), "v0.0.2");
            assert_eq!(fs::read_to_string(dir.join(APP_DIR).join(EXE)).unwrap(), "new ide");
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_first_install_needs_no_previous_app() {
        let dir = scratch("first");
        install(&dir, "atelier-ide-linux-x86_64.tar.gz", &tar_gz(&[(EXE, b"ide")]), "v1.0.0").unwrap();
        assert_eq!(fs::read_to_string(dir.join(APP_DIR).join(EXE)).unwrap(), "ide");
        let _ = fs::remove_dir_all(&dir);
    }
}
