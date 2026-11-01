use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    time::SystemTime,
};

/// Files up to this size are edited; bigger ones up to `LARGE_FILE_SIZE` open read-only.
pub const MAX_FILE_SIZE: u64 = 4 * 1024 * 1024;
/// The biggest file that opens at all.
pub const LARGE_FILE_SIZE: u64 = 64 * 1024 * 1024;

/// A file being read on a background thread.
pub struct Loading {
    rx: Receiver<Result<String>>,
    read: Arc<AtomicU64>,
    total: u64,
}

impl Loading {
    /// Share of the file read so far, from 0 to 1.
    pub fn progress(&self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        (self.read.load(Ordering::Relaxed) as f32 / self.total as f32).min(1.0)
    }
}

pub struct Document {
    /// Set while the file is read in the background; the text is empty until then.
    pub loading: Option<Loading>,
    /// Shown by a plugin (image viewer): never read as text, never saved.
    pub binary: bool,
    /// A file bigger than `MAX_FILE_SIZE`: it can be read and searched, not changed or saved.
    pub read_only: bool,
    /// Modification time of the file when it was last read or written.
    mtime: Option<SystemTime>,
    pub path: Option<PathBuf>,
    pub text: String,
    saved: String,
    pub language: String,
    /// Editor view state: line index, caret, undo history, highlighting.
    pub view: crate::editor::EditorState,
}

impl Document {
    pub fn untitled() -> Self {
        Self {
            loading: None,
            binary: false,
            read_only: false,
            mtime: None,
            path: None,
            text: String::new(),
            saved: String::new(),
            language: "txt".into(),
            view: Default::default(),
        }
    }
    pub fn open(path: PathBuf) -> Result<Self> {
        let text = read_text(&path)?;
        let large = text.len() as u64 > MAX_FILE_SIZE;
        let language = if large {
            "txt".into()
        } else {
            language(&path).into()
        };
        let mtime = modified(&path);
        Ok(Self {
            loading: None,
            binary: false,
            read_only: large,
            mtime,
            path: Some(path),
            saved: if large { String::new() } else { text.clone() },
            text,
            language,
            view: Default::default(),
        })
    }
    /// A file displayed by a plugin: nothing is read, the text stays empty.
    pub fn binary(path: PathBuf) -> Self {
        Self {
            binary: true,
            mtime: None,
            language: language(&path).into(),
            path: Some(path),
            ..Self::untitled()
        }
    }
    /// A document whose file is read on a background thread; see `poll`.
    pub fn open_async(path: PathBuf) -> Result<Self> {
        let total = fs::metadata(&path)?.len();
        if total > LARGE_FILE_SIZE {
            bail!(tr!("file_too_large_limit_4"));
        }
        let large = total > MAX_FILE_SIZE;
        let mtime = modified(&path);
        let read = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel();
        let (progress, file) = (read.clone(), path.clone());
        std::thread::spawn(move || {
            let _ = tx.send(read_chunked(&file, &progress));
        });
        Ok(Self {
            loading: Some(Loading { rx, read, total }),
            binary: false,
            read_only: large,
            mtime,
            // No highlighting nor language service on a file that big.
            language: if large {
                "txt".into()
            } else {
                language(&path).into()
            },
            path: Some(path),
            text: String::new(),
            saved: String::new(),
            view: Default::default(),
        })
    }
    /// Collects the result of the background read. Returns `Some(Err)` when it failed, in
    /// which case the document should be closed.
    pub fn poll(&mut self) -> Option<Result<()>> {
        let loading = self.loading.as_ref()?;
        let result = match loading.rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(anyhow::anyhow!(tr!("reading_interrupted"))),
        };
        self.loading = None;
        Some(result.map(|text| {
            // A read-only file is never compared with a saved copy.
            self.saved = if self.read_only {
                String::new()
            } else {
                text.clone()
            };
            self.text = text;
            self.view.invalidate();
        }))
    }
    /// The file was moved or renamed: the document follows it.
    pub fn relocate(&mut self, path: PathBuf) {
        self.language = language(&path).into();
        self.mtime = modified(&path);
        self.path = Some(path);
    }
    /// True when the file was changed on disk since it was last read or written.
    pub fn changed_on_disk(&self) -> bool {
        if self.binary || self.loading.is_some() {
            return false;
        }
        let Some(path) = &self.path else { return false };
        modified(path).is_some_and(|m| self.mtime != Some(m))
    }
    /// Reloads an unmodified document whose file changed on disk (checkout, pull, stash…).
    /// Returns true when the text changed.
    pub fn reload(&mut self) -> Result<bool> {
        let Some(path) = &self.path else {
            return Ok(false);
        };
        if self.binary || self.loading.is_some() || self.dirty() || !path.is_file() {
            return Ok(false);
        }
        let mtime = modified(path);
        let text = read_text(path)?;
        self.mtime = mtime;
        if text == self.saved {
            return Ok(false);
        }
        self.saved = if self.read_only {
            String::new()
        } else {
            text.clone()
        };
        self.text = text;
        self.view.invalidate();
        Ok(true)
    }
    pub fn dirty(&self) -> bool {
        !self.read_only && self.text != self.saved
    }
    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| tr!("untitled").into())
    }
    pub fn save(&mut self, path: PathBuf, force: bool) -> Result<()> {
        if self.binary {
            bail!(tr!("file_not_text"));
        }
        if self.read_only {
            bail!(tr!("file_read_only_too_large"));
        }
        if self.loading.is_some() {
            bail!(tr!("file_still_loading"));
        }
        if !force && self.path.as_ref() == Some(&path) && path.exists() {
            let disk = fs::read_to_string(&path)?;
            if disk != self.saved {
                bail!(tr!("file_was_changed_disk_use"));
            }
        }
        fs::write(&path, &self.text).with_context(|| trf!("cannot_save", path = path.display()))?;
        self.language = language(&path).into();
        self.mtime = modified(&path);
        self.path = Some(path);
        self.saved = self.text.clone();
        Ok(())
    }
}

/// The path without the Windows `\\?\` prefix of canonical paths, for display and for programs
/// that do not understand it.
pub fn simplify(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        // `\\?\UNC\server\share` is `\\server\share`, which is what programs and users know.
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(unc) => PathBuf::from(format!(r"\\{unc}")),
            None => PathBuf::from(rest),
        },
        None => path.to_path_buf(),
    }
}

pub fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn read_text(path: &Path) -> Result<String> {
    read_chunked(path, &AtomicU64::new(0))
}

/// Reads a text file, publishing the number of bytes read so far.
fn read_chunked(path: &Path, progress: &AtomicU64) -> Result<String> {
    let mut file = fs::File::open(path)?;
    if file.metadata()?.len() > LARGE_FILE_SIZE {
        bail!(tr!("file_too_large_limit_4"));
    }
    let mut bytes = Vec::new();
    let mut chunk = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        progress.store(bytes.len() as u64, Ordering::Relaxed);
    }
    let text = String::from_utf8(bytes).context(tr!("file_must_utf_8_text"))?;
    if text.contains(' ') {
        bail!(tr!("file_seems_binary"));
    }
    Ok(text)
}

pub fn language(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" | "xhtml" => "html",
        "pug" | "jade" => "pug",
        "css" => "css",
        "scss" => "scss",
        "sass" => "sass",
        "less" => "less",
        "js" | "mjs" | "cjs" | "jsx" => "js",
        "ts" | "tsx" => "ts",
        "java" => "java",
        "py" | "pyi" | "pyw" => "py",
        "rs" => "rs",
        "json" => "json",
        "toml" => "toml",
        "md" => "md",
        "yaml" | "yml" => "yaml",
        "xml" | "svg" => "xml",
        "sh" | "bash" | "zsh" => "sh",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "cs" => "cs",
        "go" => "go",
        "rb" => "rb",
        "php" => "php",
        "sql" => "sql",
        _ => "txt",
    }
}

/// Dependencies and build output: shown in the tree, dimmed, as IntelliJ does with its
/// excluded folders, and left out of searches.
pub fn is_library(name: &str) -> bool {
    matches!(name, "target" | "node_modules" | ".venv" | "__pycache__")
}

/// What the project tree shows in a folder: everything but the `.git` folder.
pub fn children(path: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = fs::read_dir(path)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().and_then(|n| n.to_str()) != Some(".git"))
        .collect::<Vec<_>>();
    entries.sort_by_key(|p| {
        (
            !p.is_dir(),
            p.file_name().unwrap_or_default().to_ascii_lowercase(),
        )
    });
    Ok(entries)
}

/// The folders and files that searches look at: the tree's, without dependencies and build output.
pub fn searchable_children(path: &Path) -> Result<Vec<PathBuf>> {
    Ok(children(path)?
        .into_iter()
        .filter(|p| {
            !p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_library)
        })
        .collect())
}

/// Returns a path within the project, including protection against symlink escapes.
pub fn new_file_path(root: &Path, name: &str) -> Result<PathBuf> {
    let relative = Path::new(name.trim());
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        bail!(tr!("use_relative_path_without_e"));
    }
    let target = root.join(relative);
    if target.exists() {
        bail!(tr!("file_already_exists"));
    }
    let mut ancestor = target.parent().context(tr!("invalid_path"))?;
    while !ancestor.exists() {
        ancestor = ancestor.parent().context(tr!("invalid_path"))?;
    }
    if !ancestor.canonicalize()?.starts_with(root.canonicalize()?) {
        bail!(tr!("path_leaves_project"));
    }
    Ok(target)
}

/// A file or folder name that stays in its folder: no separator, not `.` or `..`.
fn check_name(name: &str) -> Result<&str> {
    let name = name.trim();
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(_)), None) if !name.contains(['/', '\\']) => Ok(name),
        _ => bail!(tr!("invalid_name_no_folder_separator")),
    }
}

/// Renames `from` inside its folder. Returns the new path.
pub fn rename_path(from: &Path, name: &str) -> Result<PathBuf> {
    let name = check_name(name)?;
    let target = from.parent().context(tr!("invalid_path"))?.join(name);
    if target == from {
        return Ok(target);
    }
    // A name that only differs by case is the same entry on Windows: allow it.
    if target.exists()
        && !target
            .to_string_lossy()
            .eq_ignore_ascii_case(&from.to_string_lossy())
    {
        bail!(trf!("already_exists", name = name));
    }
    fs::rename(from, &target).with_context(|| trf!("cannot_rename", from = from.display()))?;
    Ok(target)
}

/// Moves `from` into the folder `dest`. Returns the new path.
pub fn move_into(from: &Path, dest: &Path) -> Result<PathBuf> {
    if !dest.is_dir() {
        bail!(tr!("destination_not_folder"));
    }
    if dest.starts_with(from) {
        bail!(tr!("cannot_move_folder_into_itself"));
    }
    let name = from.file_name().context(tr!("invalid_path"))?;
    let target = dest.join(name);
    if from.parent() == Some(dest) {
        return Ok(target);
    }
    if target.exists() {
        bail!(trf!("already_exists_folder", name = name.to_string_lossy()));
    }
    fs::rename(from, &target).with_context(|| trf!("cannot_move", from = from.display()))?;
    Ok(target)
}

/// Creates the folder `name` (relative to the project, missing parents included).
pub fn create_dir(root: &Path, name: &str) -> Result<PathBuf> {
    let path = new_file_path(root, name)?;
    fs::create_dir_all(&path)?;
    Ok(path)
}

/// Deletes a file, or a folder with everything in it.
pub fn delete_path(path: &Path) -> Result<()> {
    if path.is_dir() && !path.is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .with_context(|| trf!("cannot_delete", path = path.display()))
}

#[cfg(test)]
#[path = "../test/project/tests.rs"]
mod tests;
