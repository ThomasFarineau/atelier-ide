//! Runtimes installed in the project itself: Python (python-build-standalone), JDKs (Eclipse
//! Temurin) and Node.js, each in `.atelier/runtimes/<kind>/<version>`. The version the project
//! uses is in `.atelier/config.json` (`runtimes`), which can be versioned; the files of the
//! runtimes stay out of version control. The files other tools pin a version in
//! (`.python-version`, `.java-version`, `.nvmrc`…) say which version to offer.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    Python,
    Jdk,
    Node,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Python, Kind::Jdk, Kind::Node];

    pub fn id(self) -> &'static str {
        match self {
            Kind::Python => "python",
            Kind::Jdk => "jdk",
            Kind::Node => "node",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Python => "Python",
            Kind::Jdk => "JDK",
            Kind::Node => "Node.js",
        }
    }

    /// The files other tools pin the version in, the first found wins.
    pub fn pin_files(self) -> &'static [&'static str] {
        match self {
            Kind::Python => &[".python-version"],
            Kind::Jdk => &[".java-version", ".sdkmanrc"],
            Kind::Node => &[".nvmrc", ".node-version"],
        }
    }
}

/// What can be installed: a version, and where to get it when the catalog says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    /// `3.12.7`, `22.11.0`, or the feature release of a JDK (`21`).
    pub version: String,
    pub lts: bool,
    pub url: Option<String>,
    pub sha256: Option<String>,
}

/// Where an installation is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Resolving,
    Downloading,
    Extracting,
}

pub fn dir(root: &Path) -> PathBuf {
    crate::atelier::dir(root).join("runtimes")
}

pub fn kind_dir(root: &Path, kind: Kind) -> PathBuf {
    dir(root).join(kind.id())
}

/// `3.12.10` after `3.12.9`: the numbers of a version, in order.
pub fn version_key(version: &str) -> Vec<u64> {
    version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect()
}

/// The versions installed in the project, the newest first.
pub fn installed(root: &Path, kind: Kind) -> Vec<String> {
    let Ok(entries) = fs::read_dir(kind_dir(root, kind)) else {
        return Vec::new();
    };
    let mut versions: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    versions.sort_by_key(|v| std::cmp::Reverse(version_key(v)));
    versions
}

/// The folder a runtime is used from: the one of a JDK of macOS is `Contents/Home`.
pub fn home(root: &Path, kind: Kind, version: &str) -> PathBuf {
    let dir = kind_dir(root, kind).join(version);
    let mac = dir.join("Contents").join("Home");
    if kind == Kind::Jdk && mac.is_dir() {
        return mac;
    }
    dir
}

/// The folders of the programs of a runtime, to put first on the PATH.
pub fn bin_dirs(kind: Kind, home: &Path) -> Vec<PathBuf> {
    match kind {
        Kind::Python if cfg!(windows) => vec![home.to_path_buf(), home.join("Scripts")],
        Kind::Node if cfg!(windows) => vec![home.to_path_buf()],
        _ => vec![home.join("bin")],
    }
}

/// The main program of a runtime: `python`, `java`, `node`.
pub fn executable(kind: Kind, home: &Path) -> PathBuf {
    match (kind, cfg!(windows)) {
        (Kind::Python, true) => home.join("python.exe"),
        (Kind::Python, false) => home.join("bin").join("python3"),
        (Kind::Jdk, true) => home.join("bin").join("java.exe"),
        (Kind::Jdk, false) => home.join("bin").join("java"),
        (Kind::Node, true) => home.join("node.exe"),
        (Kind::Node, false) => home.join("bin").join("node"),
    }
}

/// The version the project uses, when it is installed.
pub fn active(root: &Path, kind: Kind) -> Option<String> {
    crate::atelier::setting(root, "runtimes")[kind.id()]
        .as_str()
        .map(str::to_string)
        .filter(|v| executable(kind, &home(root, kind, v)).is_file())
}

/// Makes `version` the one the project uses; None goes back to the one of the system.
pub fn set_active(root: &Path, kind: Kind, version: Option<&str>) {
    let mut runtimes = crate::atelier::setting(root, "runtimes");
    if !runtimes.is_object() {
        runtimes = json!({});
    }
    match version {
        Some(version) => runtimes[kind.id()] = json!(version),
        None => {
            if let Some(map) = runtimes.as_object_mut() {
                map.remove(kind.id());
            }
        }
    }
    crate::atelier::set_setting(root, "runtimes", runtimes);
}

/// The program of the active runtime of `kind`.
pub fn program(root: &Path, kind: Kind) -> Option<PathBuf> {
    active(root, kind).map(|v| executable(kind, &home(root, kind, &v)))
}

/// The virtual environment of the project (`.venv` or `venv`), with its Python.
pub fn venv(root: &Path) -> Option<(PathBuf, PathBuf)> {
    [".venv", "venv"].iter().find_map(|name| {
        let dir = root.join(name);
        let python = if cfg!(windows) {
            dir.join("Scripts").join("python.exe")
        } else {
            dir.join("bin").join("python")
        };
        python.is_file().then_some((dir, python))
    })
}

/// The Python of the project: its virtual environment, else its runtime.
pub fn python(root: &Path) -> Option<PathBuf> {
    venv(root)
        .map(|(_, python)| python)
        .or_else(|| program(root, Kind::Python))
}

/// The version a pin file of the project asks for, and that file.
pub fn pinned(root: &Path, kind: Kind) -> Option<(&'static str, String)> {
    kind.pin_files().iter().find_map(|file| {
        let text = fs::read_to_string(root.join(file)).ok()?;
        parse_pin(file, &text).map(|v| (*file, v))
    })
}

/// The version in the text of a pin file: its first line, or the `java=` line of `.sdkmanrc`
/// (`21.0.2-tem` is 21.0.2).
pub fn parse_pin(file: &str, text: &str) -> Option<String> {
    let line = if file == ".sdkmanrc" {
        text.lines()
            .find_map(|l| l.trim().strip_prefix("java="))
            .map(|v| v.split('-').next().unwrap_or(v))
    } else {
        text.lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))
    }?;
    let version = line.trim().trim_start_matches('v');
    (!version.is_empty()).then(|| version.to_string())
}

/// Whether `version` is what a pin file asks for: `3.12` is any 3.12, `lts/*` any LTS.
pub fn satisfies(wanted: &str, version: &str, lts: bool) -> bool {
    let wanted = wanted.trim().trim_start_matches('v');
    let wanted = wanted
        .strip_prefix("jdk-")
        .or_else(|| wanted.strip_prefix("temurin-"))
        .unwrap_or(wanted);
    if wanted.starts_with("lts") {
        return lts;
    }
    if wanted.is_empty() {
        return false;
    }
    if matches!(wanted, "latest" | "node" | "stable" | "*") {
        return true;
    }
    let version = version.trim_start_matches('v');
    version == wanted
        || version
            .strip_prefix(wanted)
            .is_some_and(|rest| rest.starts_with(['.', '+', '-', '_', 'u']))
}

/// The environment of a terminal of the project: the active runtimes and the virtual
/// environment first on the PATH, `JAVA_HOME` and `VIRTUAL_ENV`.
pub fn environment(root: &Path) -> Vec<(String, OsString)> {
    let mut paths = Vec::new();
    let mut vars: Vec<(String, OsString)> = Vec::new();
    if let Some((dir, _)) = venv(root) {
        paths.push(if cfg!(windows) {
            dir.join("Scripts")
        } else {
            dir.join("bin")
        });
        vars.push(("VIRTUAL_ENV".into(), dir.into_os_string()));
    }
    for kind in Kind::ALL {
        let Some(version) = active(root, kind) else {
            continue;
        };
        let home = home(root, kind, &version);
        paths.extend(bin_dirs(kind, &home));
        if kind == Kind::Jdk {
            vars.push(("JAVA_HOME".into(), home.into_os_string()));
        }
    }
    if paths.is_empty() {
        return vars;
    }
    if let Some(current) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&current));
    }
    if let Ok(path) = std::env::join_paths(paths) {
        vars.push(("PATH".into(), path));
    }
    vars
}

// ---- Catalogs -----------------------------------------------------------------------------

const PYTHON_SUMS: &str =
    "https://github.com/astral-sh/python-build-standalone/releases/latest/download/SHA256SUMS";
const PYTHON_RELEASES: &str =
    "https://github.com/astral-sh/python-build-standalone/releases/download";
const NODE_DIST: &str = "https://nodejs.org/dist";
const ADOPTIUM: &str = "https://api.adoptium.net/v3";
/// The oldest Node.js major release offered.
const OLDEST_NODE: u64 = 18;

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

fn get_text(url: &str) -> Result<String, String> {
    agent(Duration::from_secs(60))
        .get(url)
        .header("User-Agent", "atelier-ide")
        .call()
        .map_err(|e| format!("{url}: {e}"))?
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| e.to_string())
}

/// The target of python-build-standalone for this system.
fn python_triple() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        _ => return None,
    })
}

/// The Pythons of a `SHA256SUMS` of python-build-standalone built for `triple`, newest first:
/// the full `install_only` archives of the stable releases.
pub fn parse_python_sums(sums: &str, triple: &str) -> Vec<Offer> {
    let suffix = format!("-{triple}-install_only.tar.gz");
    let mut offers: Vec<Offer> = sums
        .lines()
        .filter_map(|line| {
            let (hash, name) = line.split_once(char::is_whitespace)?;
            let name = name.trim().trim_start_matches('*');
            name.strip_suffix(&suffix)?;
            let (version, tag) = name.strip_prefix("cpython-")?.split_once('+')?;
            let tag = tag.split('-').next()?;
            if !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
                return None;
            }
            Some(Offer {
                version: version.to_string(),
                lts: false,
                url: Some(format!(
                    "{PYTHON_RELEASES}/{tag}/{}",
                    name.replace('+', "%2B")
                )),
                sha256: Some(hash.to_lowercase()),
            })
        })
        .collect();
    offers.sort_by_key(|o| std::cmp::Reverse(version_key(&o.version)));
    offers.dedup_by(|a, b| a.version == b.version);
    offers
}

/// The newest release of each major Node.js from `index.json`, newest first.
pub fn parse_node_index(index: &str) -> Vec<Offer> {
    let Ok(Value::Array(releases)) = serde_json::from_str::<Value>(index) else {
        return Vec::new();
    };
    let mut offers: Vec<Offer> = Vec::new();
    for release in &releases {
        let Some(version) = release["version"].as_str() else {
            continue;
        };
        let version = version.trim_start_matches('v');
        let major = version_key(version).first().copied().unwrap_or(0);
        if major < OLDEST_NODE
            || offers
                .iter()
                .any(|o| version_key(&o.version).first() == Some(&major))
        {
            continue;
        }
        offers.push(Offer {
            version: version.to_string(),
            lts: release["lts"].as_str().is_some(),
            url: None,
            sha256: None,
        });
    }
    offers.sort_by_key(|o| std::cmp::Reverse(version_key(&o.version)));
    offers
}

/// The feature releases of Temurin, newest first.
pub fn parse_jdk_releases(releases: &str) -> Vec<Offer> {
    let Ok(value) = serde_json::from_str::<Value>(releases) else {
        return Vec::new();
    };
    let lts: Vec<u64> = value["available_lts_releases"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default();
    let mut offers: Vec<Offer> = value["available_releases"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_u64).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|feature| Offer {
            version: feature.to_string(),
            lts: lts.contains(&feature),
            url: None,
            sha256: None,
        })
        .collect();
    offers.sort_by_key(|o| std::cmp::Reverse(version_key(&o.version)));
    offers
}

/// What can be installed on this system.
pub fn catalog(kind: Kind) -> Result<Vec<Offer>, String> {
    match kind {
        Kind::Python => {
            let triple = python_triple().ok_or(tr!("runtimes.unsupported_system"))?;
            Ok(parse_python_sums(&get_text(PYTHON_SUMS)?, triple))
        }
        Kind::Node => Ok(parse_node_index(&get_text(&format!(
            "{NODE_DIST}/index.json"
        ))?)),
        Kind::Jdk => Ok(parse_jdk_releases(&get_text(&format!(
            "{ADOPTIUM}/info/available_releases"
        ))?)),
    }
}

/// An archive to download, and the version it installs.
struct Package {
    version: String,
    url: String,
    name: String,
    sha256: Option<String>,
}

fn node_platform() -> Option<(&'static str, &'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "windows" => "win",
        "linux" => "linux",
        "macos" => "darwin",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return None,
    };
    Some((os, arch, if os == "win" { "zip" } else { "tar.gz" }))
}

/// The name of a release of Temurin as a version: `jdk-21.0.5+11` is `21.0.5+11`.
pub fn jdk_version(release_name: &str) -> String {
    release_name
        .trim_start_matches("jdk-")
        .trim_start_matches("jdk")
        .to_string()
}

fn resolve(kind: Kind, offer: &Offer) -> Result<Package, String> {
    match kind {
        Kind::Python => {
            let url = offer
                .url
                .clone()
                .ok_or(tr!("runtimes.unsupported_system"))?;
            Ok(Package {
                version: offer.version.clone(),
                name: url.rsplit('/').next().unwrap_or("python.tar.gz").into(),
                url,
                sha256: offer.sha256.clone(),
            })
        }
        Kind::Node => {
            let (os, arch, ext) = node_platform().ok_or(tr!("runtimes.unsupported_system"))?;
            let version = &offer.version;
            let name = format!("node-v{version}-{os}-{arch}.{ext}");
            let sums = get_text(&format!("{NODE_DIST}/v{version}/SHASUMS256.txt"))?;
            let sha256 = sums.lines().find_map(|line| {
                let (hash, file) = line.split_once(char::is_whitespace)?;
                (file.trim() == name).then(|| hash.to_lowercase())
            });
            if sha256.is_none() {
                return Err(tr!("runtimes.unsupported_system").into());
            }
            Ok(Package {
                version: version.clone(),
                url: format!("{NODE_DIST}/v{version}/{name}"),
                name,
                sha256,
            })
        }
        Kind::Jdk => {
            let os = match std::env::consts::OS {
                "windows" => "windows",
                "linux" => "linux",
                "macos" => "mac",
                _ => return Err(tr!("runtimes.unsupported_system").into()),
            };
            let arch = match std::env::consts::ARCH {
                "x86_64" => "x64",
                "aarch64" => "aarch64",
                _ => return Err(tr!("runtimes.unsupported_system").into()),
            };
            let feature = version_key(&offer.version)
                .first()
                .copied()
                .unwrap_or_default();
            let text = get_text(&format!(
                "{ADOPTIUM}/assets/latest/{feature}/hotspot?architecture={arch}&image_type=jdk&os={os}&vendor=eclipse"
            ))?;
            let assets: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let asset = assets
                .as_array()
                .and_then(|a| a.first())
                .ok_or(tr!("runtimes.unsupported_system"))?;
            let package = &asset["binary"]["package"];
            Ok(Package {
                version: jdk_version(asset["release_name"].as_str().unwrap_or(&offer.version)),
                url: package["link"].as_str().unwrap_or_default().to_string(),
                name: package["name"].as_str().unwrap_or("jdk.zip").to_string(),
                sha256: package["checksum"].as_str().map(str::to_lowercase),
            })
        }
    }
}

/// Downloads `url` into `file`, telling `progress` the bytes so far and the size. Returns the
/// SHA-256 of what came.
fn download(url: &str, file: &Path, progress: &dyn Fn(Step, u64, u64)) -> Result<String, String> {
    let mut response = agent(Duration::from_secs(3600))
        .get(url)
        .header("User-Agent", "atelier-ide")
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(4 * 1024 * 1024 * 1024)
        .reader();
    let mut out = File::create(file).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0; 256 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&chunk[..n]);
        out.write_all(&chunk[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        progress(Step::Downloading, done, total);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
    let file = BufReader::new(File::open(archive).map_err(|e| e.to_string())?);
    let name = archive.to_string_lossy();
    if name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
            // Names that climb out of the folder are skipped.
            let Some(relative) = entry.enclosed_name() else {
                continue;
            };
            let target = into.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&target).map_err(|e| e.to_string())?;
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = File::create(&target).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
            }
        }
        Ok(())
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        tar.set_preserve_permissions(true);
        tar.unpack(into).map_err(|e| e.to_string())
    }
}

/// The folder an archive put everything in, when it has one (`node-v22.11.0-win-x64`).
fn content_root(dir: &Path) -> PathBuf {
    let entries: Vec<PathBuf> = fs::read_dir(dir)
        .map(|e| e.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    match entries.as_slice() {
        [only] if only.is_dir() => only.clone(),
        _ => dir.to_path_buf(),
    }
}

/// Installs the runtime of `offer` in the project and returns its version.
pub fn install(
    root: &Path,
    kind: Kind,
    offer: &Offer,
    progress: &dyn Fn(Step, u64, u64),
) -> Result<String, String> {
    progress(Step::Resolving, 0, 0);
    let package = resolve(kind, offer)?;
    let base = kind_dir(root, kind);
    let downloads = base.join(".download");
    fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    crate::atelier::ensure_ignored(root, "runtimes/");
    let archive = downloads.join(&package.name);
    let result = (|| {
        let sha256 = download(&package.url, &archive, progress)?;
        if let Some(expected) = &package.sha256
            && expected != &sha256
        {
            return Err(trf!("runtimes.checksum_mismatch", file = package.name));
        }
        progress(Step::Extracting, 0, 0);
        let staging = base.join(format!(".staging-{}", package.version));
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        unpack(&archive, &staging)?;
        let target = base.join(&package.version);
        let _ = fs::remove_dir_all(&target);
        fs::rename(content_root(&staging), &target).map_err(|e| e.to_string())?;
        let _ = fs::remove_dir_all(&staging);
        Ok(package.version.clone())
    })();
    let _ = fs::remove_file(&archive);
    let _ = fs::remove_dir(&downloads);
    result
}

/// Removes an installed version; the project goes back to the system one if it used it.
pub fn uninstall(root: &Path, kind: Kind, version: &str) -> Result<(), String> {
    if active(root, kind).as_deref() == Some(version) {
        set_active(root, kind, None);
    }
    fs::remove_dir_all(kind_dir(root, kind).join(version)).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/runtimes/install/tests.rs"]
mod tests;
