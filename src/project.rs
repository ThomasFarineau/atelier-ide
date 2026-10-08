use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const MAX_FILE_SIZE: u64 = 4 * 1024 * 1024;

pub struct Document {
    pub path: Option<PathBuf>,
    pub text: String,
    saved: String,
    pub language: String,
}

impl Document {
    pub fn untitled() -> Self {
        Self {
            path: None,
            text: String::new(),
            saved: String::new(),
            language: "txt".into(),
        }
    }
    pub fn open(path: PathBuf) -> Result<Self> {
        if fs::metadata(&path)?.len() > MAX_FILE_SIZE {
            bail!("Fichier trop volumineux (limite : 4 Mo)");
        }
        let text = fs::read_to_string(&path).context("Le fichier doit être un texte UTF-8")?;
        if text.contains('\0') {
            bail!("Ce fichier semble être binaire");
        }
        let language = language(&path).into();
        Ok(Self {
            path: Some(path),
            saved: text.clone(),
            text,
            language,
        })
    }
    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }
    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Sans titre".into())
    }
    pub fn save(&mut self, path: PathBuf, force: bool) -> Result<()> {
        if !force && self.path.as_ref() == Some(&path) && path.exists() {
            let disk = fs::read_to_string(&path)?;
            if disk != self.saved {
                bail!(
                    "Le fichier a été modifié sur disque. Utilisez Enregistrer sous pour choisir une copie ou confirmer son remplacement."
                );
            }
        }
        fs::write(&path, &self.text)
            .with_context(|| format!("Impossible d'enregistrer {}", path.display()))?;
        self.language = language(&path).into();
        self.path = Some(path);
        self.saved = self.text.clone();
        Ok(())
    }
}

pub fn language(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "html",
        "css" => "css",
        "js" | "mjs" | "cjs" | "jsx" => "js",
        "ts" | "tsx" => "ts",
        "java" => "java",
        "py" => "py",
        "rs" => "rs",
        "json" => "json",
        "toml" => "toml",
        "md" => "md",
        _ => "txt",
    }
}

pub fn children(path: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = fs::read_dir(path)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            !matches!(
                p.file_name().and_then(|n| n.to_str()),
                Some(".git" | "target" | "node_modules" | ".venv" | "__pycache__")
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|p| {
        (
            !p.is_dir(),
            p.file_name().unwrap_or_default().to_ascii_lowercase(),
        )
    });
    Ok(entries)
}

/// Returns a path within the project, including protection against symlink escapes.
pub fn new_file_path(root: &Path, name: &str) -> Result<PathBuf> {
    let relative = Path::new(name.trim());
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        bail!("Utilisez un chemin relatif sans '..' (ex. src/main.rs)");
    }
    let target = root.join(relative);
    if target.exists() {
        bail!("Ce fichier existe déjà");
    }
    let mut ancestor = target.parent().context("Chemin invalide")?;
    while !ancestor.exists() {
        ancestor = ancestor.parent().context("Chemin invalide")?;
    }
    if !ancestor.canonicalize()?.starts_with(root.canonicalize()?) {
        bail!("Le chemin sort du projet");
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_extensions() {
        for (name, expected) in [
            ("index.HTML", "html"),
            ("a.css", "css"),
            ("a.mjs", "js"),
            ("a.tsx", "ts"),
            ("Main.java", "java"),
            ("a.py", "py"),
            ("lib.rs", "rs"),
        ] {
            assert_eq!(language(Path::new(name)), expected);
        }
    }
    #[test]
    fn refuses_parent_and_absolute_paths() {
        assert!(new_file_path(Path::new("."), "../escape.rs").is_err());
        assert!(new_file_path(Path::new("."), "/escape.rs").is_err());
        assert!(new_file_path(Path::new("."), "").is_err());
    }
    #[test]
    fn refuses_to_overwrite_external_changes() {
        let path = std::env::temp_dir().join(format!("forge-conflict-{}.rs", std::process::id()));
        fs::write(&path, "initial").unwrap();
        let mut doc = Document::open(path.clone()).unwrap();
        doc.text = "editor".into();
        fs::write(&path, "external").unwrap();
        assert!(doc.save(path.clone(), false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        fs::remove_file(path).unwrap();
    }
}
