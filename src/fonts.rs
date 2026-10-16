//! Fonts the user installs: from the catalog of Google Fonts or of Fontsource, from any address
//! (a TrueType or OpenType file, or the CSS of a font service with `@font-face` rules) or from
//! a file. They are kept in the `fonts` folder of the data folder of Atelier, a folder per
//! family with its regular and semibold files and a `font.json` that describes it.

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

/// The files of a family: those of its regular weight and those of its semibold one.
pub type Files = (Vec<Vec<u8>>, Vec<Vec<u8>>);
/// A catalog, or why it could not be read.
type Catalog = Result<Vec<Entry>, String>;

/// The largest file a font service may send.
const MAX_FILE: u64 = 64 * 1024 * 1024;

/// A catalog of fonts that can be installed by their name.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Provider {
    Google,
    Fontsource,
}

impl Provider {
    pub const ALL: [Self; 2] = [Self::Google, Self::Fontsource];

    pub fn name(self) -> &'static str {
        match self {
            Self::Google => "Google Fonts",
            Self::Fontsource => "Fontsource",
        }
    }
}

/// The kind of a font, as the catalogs sort them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    #[default]
    Sans,
    Serif,
    Monospace,
    Display,
    Handwriting,
}

impl Category {
    pub const ALL: [Self; 5] = [
        Self::Sans,
        Self::Serif,
        Self::Monospace,
        Self::Display,
        Self::Handwriting,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Sans => "Sans serif",
            Self::Serif => "Serif",
            Self::Monospace => "Monospace",
            Self::Display => "Display",
            Self::Handwriting => "Handwriting",
        }
    }

    /// The category as Google Fonts (`Sans Serif`) or Fontsource (`sans-serif`) writes it.
    fn parse(text: &str) -> Self {
        match text.to_ascii_lowercase().replace([' ', '-'], "").as_str() {
            "serif" => Self::Serif,
            "monospace" => Self::Monospace,
            "display" => Self::Display,
            "handwriting" => Self::Handwriting,
            _ => Self::Sans,
        }
    }
}

/// A font of a catalog.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub family: String,
    pub category: Category,
    /// The identifier of the font in the catalog (Fontsource), the family otherwise.
    id: String,
}

/// Where a font is installed from.
#[derive(Clone, Debug)]
pub enum Source {
    Catalog(Provider, Entry),
    /// A font file or a CSS of `@font-face` rules.
    Address(String),
    File(PathBuf),
}

/// A font installed by the user.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Installed {
    pub family: String,
    pub category: Category,
    /// Where it comes from, to be shown.
    pub origin: String,
    /// The files of the regular and the semibold weights, in their folder; several when the
    /// service cuts the font by alphabet.
    regular: Vec<String>,
    semibold: Vec<String>,
}

fn store_dir() -> PathBuf {
    crate::cli::data_dir().join("fonts")
}

/// The folder of a family: its name without the characters a file name cannot have.
fn family_dir(root: &Path, family: &str) -> PathBuf {
    let name: String = family
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    root.join(name.trim())
}

/// The installed fonts, sorted by name.
pub fn installed() -> Vec<Installed> {
    installed_in(&store_dir())
}

fn installed_in(root: &Path) -> Vec<Installed> {
    let mut fonts: Vec<Installed> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| fs::read_to_string(e.path().join("font.json")).ok())
        .filter_map(|text| serde_json::from_str(&text).ok())
        .collect();
    fonts.sort_by_key(|f| f.family.to_lowercase());
    fonts
}

/// The files of an installed family: (regular, semibold), semibold empty when it has none.
pub fn load(family: &str) -> Option<Files> {
    let dir = family_dir(&store_dir(), family);
    let font: Installed =
        serde_json::from_str(&fs::read_to_string(dir.join("font.json")).ok()?).ok()?;
    let read = |files: &[String]| -> Vec<Vec<u8>> {
        files
            .iter()
            .filter_map(|f| fs::read(dir.join(f)).ok())
            .collect()
    };
    let regular = read(&font.regular);
    (!regular.is_empty()).then(|| (regular, read(&font.semibold)))
}

pub fn uninstall(family: &str) -> Result<()> {
    let dir = family_dir(&store_dir(), family);
    fs::remove_dir_all(&dir).with_context(|| format!("{}", dir.display()))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
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

fn get_text(agent: &ureq::Agent, url: &str) -> Result<String> {
    Ok(String::from_utf8_lossy(&get(agent, url)?).into_owned())
}

/// The fonts of a catalog.
pub fn catalog(provider: Provider) -> Result<Vec<Entry>> {
    let agent = agent();
    let mut entries = match provider {
        Provider::Google => {
            #[derive(Deserialize)]
            struct Family {
                family: String,
                category: String,
            }
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Metadata {
                family_metadata_list: Vec<Family>,
            }
            let text = get_text(&agent, "https://fonts.google.com/metadata/fonts")?;
            // The answer may start with a line that keeps it from being run as a script.
            let json = text.trim_start_matches(")]}'").trim_start();
            let metadata: Metadata = serde_json::from_str(json)?;
            metadata
                .family_metadata_list
                .into_iter()
                .map(|f| Entry {
                    category: Category::parse(&f.category),
                    id: f.family.clone(),
                    family: f.family,
                })
                .collect::<Vec<_>>()
        }
        Provider::Fontsource => {
            #[derive(Deserialize)]
            struct Font {
                id: String,
                family: String,
                category: String,
            }
            let text = get_text(&agent, "https://api.fontsource.org/v1/fonts")?;
            let fonts: Vec<Font> = serde_json::from_str(&text)?;
            fonts
                .into_iter()
                .map(|f| Entry {
                    category: Category::parse(&f.category),
                    family: f.family,
                    id: f.id,
                })
                .collect()
        }
    };
    entries.sort_by_key(|e| e.family.to_lowercase());
    Ok(entries)
}

/// A font file, checked: TrueType or OpenType (egui reads neither WOFF nor WOFF2).
fn check_font(bytes: &[u8], from: &str) -> Result<()> {
    match bytes.get(..4) {
        Some([0, 1, 0, 0] | b"OTTO" | b"true" | b"ttcf") => Ok(()),
        Some(b"wOFF" | b"wOF2") => {
            bail!("{from}: WOFF fonts are not supported, a TTF or OTF is needed")
        }
        _ => bail!("{from}: not a TrueType or OpenType font"),
    }
}

/// The family written in a font file and whether its letters all have the same width.
fn describe(bytes: &[u8]) -> Option<(String, bool)> {
    let face = ttf_parser::Face::parse(bytes, 0).ok()?;
    let names = face.names();
    let name = |id| {
        names
            .into_iter()
            .filter(|n| n.name_id == id && n.is_unicode())
            .find_map(|n| n.to_string())
    };
    let family = name(ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
        .or_else(|| name(ttf_parser::name_id::FAMILY))?;
    Some((family, face.is_monospaced()))
}

/// A rule of a CSS of fonts: its family, weight, style and TrueType or OpenType address.
#[derive(Debug, PartialEq)]
struct FontFace {
    family: String,
    weight: u16,
    italic: bool,
    url: Option<String>,
}

/// The `@font-face` rules of a CSS, with the address of their TrueType or OpenType file
/// (None when it only offers WOFF).
fn font_faces(css: &str, base: &str) -> Vec<FontFace> {
    css.split("@font-face")
        .skip(1)
        .filter_map(|rule| {
            let body = &rule[rule.find('{')? + 1..rule.find('}')?];
            let property = |name: &str| {
                body.split(';').find_map(|p| {
                    let (key, value) = p.split_once(':')?;
                    (key.trim().eq_ignore_ascii_case(name)).then(|| value.trim().to_string())
                })
            };
            let family = property("font-family")?
                .trim_matches(['"', '\''])
                .to_string();
            let weight = property("font-weight")
                .and_then(|w| w.split_whitespace().next()?.parse().ok())
                .unwrap_or(400);
            let italic = property("font-style").is_some_and(|s| s.contains("italic"));
            let url = property("src").and_then(|src| {
                src.split("url(").skip(1).find_map(|part| {
                    let (address, rest) = part.split_once(')')?;
                    let address = address.trim().trim_matches(['"', '\'']);
                    let format = rest.split(',').next().unwrap_or("").to_ascii_lowercase();
                    let lower = address.to_ascii_lowercase();
                    let usable = format.contains("truetype")
                        || format.contains("opentype")
                        || ((format.trim().is_empty() || !format.contains("format"))
                            && (lower.ends_with(".ttf") || lower.ends_with(".otf")));
                    usable.then(|| absolute(address, base))
                })
            });
            Some(FontFace {
                family,
                weight,
                italic,
                url,
            })
        })
        .collect()
}

/// `address` made absolute from the address of the CSS it is written in.
fn absolute(address: &str, base: &str) -> String {
    if address.contains("://") {
        address.to_string()
    } else if let Some(rest) = address.strip_prefix("//") {
        format!("https://{rest}")
    } else if address.starts_with('/') {
        let origin_end = base
            .find("://")
            .and_then(|i| base[i + 3..].find('/').map(|j| i + 3 + j))
            .unwrap_or(base.len());
        format!("{}{address}", &base[..origin_end])
    } else {
        let dir_end = base.rfind('/').map_or(base.len(), |i| i + 1);
        format!("{}{address}", &base[..dir_end])
    }
}

/// From the rules of a family: the files of its regular weight and of its semibold one (the
/// closest weight above the regular one).
fn pick_weights(faces: &[FontFace]) -> (Vec<String>, Vec<String>) {
    let upright: Vec<&FontFace> = faces
        .iter()
        .filter(|f| !f.italic && f.url.is_some())
        .collect();
    let closest = |target: u16, above: u16| {
        upright
            .iter()
            .map(|f| f.weight)
            .filter(|w| *w >= above)
            .min_by_key(|w| w.abs_diff(target))
    };
    let urls = |weight: Option<u16>| -> Vec<String> {
        upright
            .iter()
            .filter(|f| Some(f.weight) == weight)
            .filter_map(|f| f.url.clone())
            .collect()
    };
    let regular = closest(400, 0);
    let semibold = regular.and_then(|r| closest(600, r + 1));
    (urls(regular), urls(semibold))
}

/// Downloads and installs a font. Returns what was installed.
pub fn install(source: &Source) -> Result<Installed> {
    install_in(&store_dir(), source)
}

fn install_in(root: &Path, source: &Source) -> Result<Installed> {
    let agent = agent();
    let (family, category, origin, (regular, semibold)): (String, Option<Category>, String, Files) =
        match source {
            Source::Catalog(Provider::Google, entry) => {
                let name = entry.family.replace(' ', "+");
                let css = get_text(
                    &agent,
                    &format!("https://fonts.googleapis.com/css2?family={name}:wght@400;600"),
                )
                .or_else(|_| {
                    // A family without these weights: all of its styles.
                    get_text(
                        &agent,
                        &format!("https://fonts.googleapis.com/css2?family={name}"),
                    )
                })?;
                let (regular, semibold) = pick_weights(&font_faces(&css, ""));
                let download = |urls: Vec<String>| -> Result<Vec<Vec<u8>>> {
                    urls.iter().map(|u| get(&agent, u)).collect()
                };
                (
                    entry.family.clone(),
                    Some(entry.category),
                    Provider::Google.name().into(),
                    (download(regular)?, download(semibold)?),
                )
            }
            Source::Catalog(Provider::Fontsource, entry) => {
                let text = get_text(
                    &agent,
                    &format!("https://api.fontsource.org/v1/fonts/{}", entry.id),
                )?;
                let font: serde_json::Value = serde_json::from_str(&text)?;
                let variants = font["variants"]
                    .as_object()
                    .ok_or_else(|| anyhow!("{}: no files", entry.family))?;
                // A rule per weight and alphabet, as a CSS would give them.
                let faces: Vec<FontFace> = variants
                    .iter()
                    .flat_map(|(weight, styles)| {
                        styles["normal"].as_object().into_iter().flatten().map(
                            move |(_, subset)| FontFace {
                                family: String::new(),
                                weight: weight.parse().unwrap_or(400),
                                italic: false,
                                url: subset["url"]["ttf"].as_str().map(str::to_string),
                            },
                        )
                    })
                    .collect();
                let (regular, semibold) = pick_weights(&faces);
                let download = |urls: Vec<String>| -> Result<Vec<Vec<u8>>> {
                    urls.iter().map(|u| get(&agent, u)).collect()
                };
                (
                    entry.family.clone(),
                    Some(entry.category),
                    Provider::Fontsource.name().into(),
                    (download(regular)?, download(semibold)?),
                )
            }
            Source::Address(url) => {
                let bytes = get(&agent, url)?;
                if check_font(&bytes, url).is_ok() {
                    let family = describe(&bytes)
                        .map(|d| d.0)
                        .unwrap_or_else(|| file_stem(url));
                    (family, None, url.clone(), (vec![bytes], Vec::new()))
                } else {
                    // A CSS: the first family it describes.
                    let css = String::from_utf8_lossy(&bytes);
                    let faces = font_faces(&css, url);
                    let family = faces
                        .first()
                        .map(|f| f.family.clone())
                        .ok_or_else(|| anyhow!("{url}: neither a font nor a CSS of fonts"))?;
                    let faces: Vec<FontFace> =
                        faces.into_iter().filter(|f| f.family == family).collect();
                    if faces.iter().all(|f| f.url.is_none()) {
                        bail!("{url}: this service only gives WOFF fonts, a TTF or OTF is needed");
                    }
                    let (regular, semibold) = pick_weights(&faces);
                    let download = |urls: Vec<String>| -> Result<Vec<Vec<u8>>> {
                        urls.iter().map(|u| get(&agent, u)).collect()
                    };
                    (
                        family,
                        None,
                        url.clone(),
                        (download(regular)?, download(semibold)?),
                    )
                }
            }
            Source::File(path) => {
                let bytes = fs::read(path).with_context(|| format!("{}", path.display()))?;
                let from = path.display().to_string();
                check_font(&bytes, &from)?;
                let family = describe(&bytes)
                    .map(|d| d.0)
                    .unwrap_or_else(|| file_stem(&from));
                (family, None, from, (vec![bytes], Vec::new()))
            }
        };
    if regular.is_empty() {
        bail!("{family}: no TrueType or OpenType file to install");
    }
    for (index, bytes) in regular.iter().chain(&semibold).enumerate() {
        check_font(bytes, &format!("{family} ({})", index + 1))?;
    }
    // A font of an address or a file says itself whether it is monospaced.
    let category = category.unwrap_or_else(|| match describe(&regular[0]) {
        Some((_, true)) => Category::Monospace,
        _ => Category::Sans,
    });
    let dir = family_dir(root, &family);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    let write = |weight: &str, files: &[Vec<u8>]| -> Result<Vec<String>> {
        files
            .iter()
            .enumerate()
            .map(|(index, bytes)| {
                let name = format!("{weight}-{index}.ttf");
                fs::write(dir.join(&name), bytes)?;
                Ok(name)
            })
            .collect()
    };
    let installed = Installed {
        regular: write("regular", &regular)?,
        semibold: write("semibold", &semibold)?,
        family,
        category,
        origin,
    };
    fs::write(
        dir.join("font.json"),
        serde_json::to_string_pretty(&installed)?,
    )?;
    Ok(installed)
}

/// The name of the file of an address or a path, without its extension.
fn file_stem(path: &str) -> String {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .split(['?', '#'])
        .next()
        .unwrap_or(path);
    Path::new(name)
        .file_stem()
        .map_or_else(|| name.to_string(), |s| s.to_string_lossy().into_owned())
}

/// What the Fonts page waits for.
#[derive(Default)]
pub struct Store {
    /// The catalogs: loading (None) or loaded.
    catalogs: Vec<(Provider, Option<Catalog>)>,
    catalog_rx: Vec<(Provider, mpsc::Receiver<Catalog>)>,
    /// The fonts being installed.
    installing: Vec<(String, mpsc::Receiver<Result<Installed, String>>)>,
    pub installed: Option<Vec<Installed>>,
    /// The last error or success to show.
    pub message: Option<(String, bool)>,
}

impl Store {
    /// Takes the answers that arrived. Returns true when something changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        self.catalog_rx
            .retain(|(provider, rx)| match rx.try_recv() {
                Ok(result) => {
                    if let Some(slot) = self.catalogs.iter_mut().find(|(p, _)| p == provider) {
                        slot.1 = Some(result);
                    }
                    changed = true;
                    false
                }
                Err(mpsc::TryRecvError::Empty) => true,
                Err(mpsc::TryRecvError::Disconnected) => false,
            });
        let mut done = Vec::new();
        self.installing.retain(|(label, rx)| match rx.try_recv() {
            Ok(result) => {
                done.push((label.clone(), result));
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => false,
        });
        for (_, result) in done {
            changed = true;
            self.message = Some(match result {
                Ok(font) => (font.family, true),
                Err(e) => (e, false),
            });
            self.installed = Some(installed());
        }
        changed
    }

    pub fn busy(&self) -> bool {
        !self.catalog_rx.is_empty() || !self.installing.is_empty()
    }

    pub fn installed(&mut self) -> &[Installed] {
        self.installed.get_or_insert_with(installed)
    }

    /// The catalog of a provider, asked for the first time it is needed.
    pub fn catalog(&mut self, provider: Provider) -> Option<&Catalog> {
        if !self.catalogs.iter().any(|(p, _)| *p == provider) {
            self.catalogs.push((provider, None));
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let _ = tx.send(catalog(provider).map_err(|e| format!("{e:#}")));
            });
            self.catalog_rx.push((provider, rx));
        }
        self.catalogs
            .iter()
            .find(|(p, _)| *p == provider)
            .and_then(|(_, c)| c.as_ref())
    }

    /// Forgets a catalog that failed, to ask again.
    pub fn retry(&mut self, provider: Provider) {
        self.catalogs.retain(|(p, _)| *p != provider);
    }

    pub fn is_installing(&self, label: &str) -> bool {
        self.installing.iter().any(|(l, _)| l == label)
    }

    pub fn install(&mut self, label: String, source: Source) {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(install(&source).map_err(|e| format!("{e:#}")));
        });
        self.installing.push((label, rx));
    }

    pub fn uninstall(&mut self, family: &str) {
        if let Err(e) = uninstall(family) {
            self.message = Some((format!("{e:#}"), false));
        }
        self.installed = Some(installed());
    }
}

#[cfg(test)]
#[path = "../test/fonts/tests.rs"]
mod tests;
