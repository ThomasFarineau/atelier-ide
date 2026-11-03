//! The plugins one can install from the IDE (Plugins, Available): the releases
//! `plugins/<id>/vX.Y.Z` of atelier-ide-org/releases, the latest of each plugin. Each release has
//! the file Atelier installs (`<id>.atelier-plugin`) and its manifest (`plugin.json`), which
//! says what the plugin is and which version of the plugin interface it was built for: only
//! those this IDE loads are offered.

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Duration,
};

const RELEASES: &str =
    "https://api.github.com/repos/atelier-ide-org/releases/releases?per_page=100";
/// Largest file downloaded: a plugin, with the library of each system.
const MAX_FILE: u64 = 512 * 1024 * 1024;

/// A plugin of the catalog.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// The `.atelier-plugin` file.
    pub download: String,
    /// The page of the release.
    pub page: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct Manifest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    abi: Option<u32>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into()
}

fn get(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>> {
    let mut response = agent
        .get(url)
        .header("User-Agent", "atelier-ide")
        .call()
        .with_context(|| url.to_string())?;
    Ok(response
        .body_mut()
        .with_config()
        .limit(MAX_FILE)
        .read_to_vec()?)
}

/// The numbers of a version (`v1.2.3` is 1, 2, 3), to compare them.
pub fn version_key(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map_while(|part| part.parse().ok())
        .collect()
}

/// The plugin and version of a release of the catalog (`plugins/<id>/vX.Y.Z`).
pub fn plugin_tag(tag: &str) -> Option<(&str, &str)> {
    let rest = tag.strip_prefix("plugins/")?;
    let (id, version) = rest.split_once('/')?;
    (!id.is_empty() && version.starts_with('v') && version.len() > 1)
        .then(|| (id, version.trim_start_matches('v')))
}

/// The latest release of each plugin, read from the JSON of the releases. Returns them with
/// the address of their manifest, sorted by identifier.
fn latest(releases: &[Release]) -> Vec<(Entry, String)> {
    let mut best: Vec<(Entry, String)> = Vec::new();
    for release in releases.iter().filter(|r| !r.draft && !r.prerelease) {
        let Some((id, version)) = plugin_tag(&release.tag_name) else {
            continue;
        };
        let asset = |name: &str| {
            release
                .assets
                .iter()
                .find(|a| a.name == name)
                .map(|a| a.browser_download_url.clone())
        };
        let (Some(download), Some(manifest)) = (
            asset(&format!("{id}.{}", super::PACKAGE_EXTENSION)),
            asset("plugin.json"),
        ) else {
            continue;
        };
        let entry = Entry {
            id: id.to_string(),
            name: id.to_string(),
            version: version.to_string(),
            description: String::new(),
            download,
            page: release.html_url.clone(),
        };
        match best.iter_mut().find(|(e, _)| e.id == id) {
            Some(kept) if version_key(&kept.0.version) >= version_key(version) => {}
            Some(kept) => *kept = (entry, manifest),
            None => best.push((entry, manifest)),
        }
    }
    best.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    best
}

/// The plugins this IDE can install, with their manifests read.
pub fn fetch() -> Result<Vec<Entry>> {
    let agent = agent();
    let releases: Vec<Release> =
        serde_json::from_slice(&get(&agent, RELEASES)?).context("releases")?;
    let mut entries = Vec::new();
    for (mut entry, manifest) in latest(&releases) {
        let Ok(manifest) = get(&agent, &manifest)
            .and_then(|bytes| serde_json::from_slice::<Manifest>(&bytes).map_err(Into::into))
        else {
            continue;
        };
        if manifest.abi != Some(atelier_plugin_api::abi::VERSION) {
            continue;
        }
        if !manifest.name.is_empty() {
            entry.name = manifest.name;
        }
        entry.description = manifest.description;
        entries.push(entry);
    }
    Ok(entries)
}

/// Downloads the file of a plugin, into a folder of its own in the temporary folder.
pub fn download(entry: &Entry) -> Result<PathBuf> {
    let bytes = get(&agent(), &entry.download)?;
    if bytes.len() < 4 || &bytes[..2] != b"PK" {
        bail!("{}: not a plugin file", entry.download);
    }
    let dir = std::env::temp_dir().join(format!("atelier-plugin-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{}.{}", entry.id, super::PACKAGE_EXTENSION));
    std::fs::write(&file, bytes)?;
    Ok(file)
}

/// What the Available tab waits for: the catalog, and the plugins being downloaded.
#[derive(Default)]
pub struct Store {
    entries: Option<Result<Vec<Entry>, String>>,
    loading: Option<Receiver<Result<Vec<Entry>, String>>>,
    downloading: Vec<(String, Receiver<Result<PathBuf, String>>)>,
}

impl Store {
    /// The catalog, asked for the first time it is needed. None while it loads.
    pub fn entries(&mut self) -> Option<&Result<Vec<Entry>, String>> {
        if self.entries.is_none() && self.loading.is_none() {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let _ = tx.send(fetch().map_err(|e| format!("{e:#}")));
            });
            self.loading = Some(rx);
        }
        if let Some(rx) = &self.loading {
            match rx.try_recv() {
                Ok(result) => {
                    self.entries = Some(result);
                    self.loading = None;
                }
                Err(TryRecvError::Disconnected) => self.loading = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        self.entries.as_ref()
    }

    /// Asks for the catalog again.
    pub fn refresh(&mut self) {
        self.entries = None;
        self.loading = None;
    }

    pub fn busy(&self) -> bool {
        self.loading.is_some() || !self.downloading.is_empty()
    }

    pub fn is_downloading(&self, id: &str) -> bool {
        self.downloading.iter().any(|(i, _)| i == id)
    }

    pub fn download(&mut self, entry: &Entry) {
        let (tx, rx) = mpsc::channel();
        let entry = entry.clone();
        let id = entry.id.clone();
        thread::spawn(move || {
            let _ = tx.send(download(&entry).map_err(|e| format!("{e:#}")));
        });
        self.downloading.push((id, rx));
    }

    /// The downloads that ended: the file of each plugin, or what failed.
    pub fn downloaded(&mut self) -> Vec<(String, Result<PathBuf, String>)> {
        let mut done = Vec::new();
        self.downloading.retain(|(id, rx)| match rx.try_recv() {
            Ok(result) => {
                done.push((id.clone(), result));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        done
    }
}

#[cfg(test)]
#[path = "../../test/plugins/catalog/tests.rs"]
mod tests;
