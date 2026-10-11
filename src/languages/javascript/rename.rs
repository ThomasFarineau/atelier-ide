//! Renaming a name of JavaScript or TypeScript: every place of the document that is the same
//! name, and the files of the project that import it.

use super::{
    analysis::{Analysis, DeclKind, dialect},
    builtins,
    navigate::{highlights, resolve_module},
};
use crate::languages::{FileEdit, TextEdit};
use std::path::{Path, PathBuf};

const EXTENSIONS: [&str; 8] = ["js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts"];
const SKIPPED: [&str; 9] = [
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "out",
    "coverage",
    ".next",
    ".atelier",
];
const MAX_FILES: usize = 1500;

/// Whether `name` can be an identifier.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        && !builtins::KEYWORDS.contains(&name)
}

/// The edits that rename the name at `offset` to `new_name`: in the document (path None), and
/// in the files that import it.
pub fn rename(
    text: &str,
    offset: usize,
    new_name: &str,
    path: Option<&Path>,
    root: &Path,
) -> Result<Vec<FileEdit>, String> {
    if !valid_name(new_name) {
        return Err(trf!("javascript.not_valid_name", new_name = new_name));
    }
    let analysis = Analysis::new(text).ok_or(tr!("javascript.cannot_analyze"))?;
    let ranges = highlights(text, offset);
    if ranges.is_empty() {
        return Err(tr!("javascript.no_name_rename_here").into());
    }
    let old = &text[ranges[0].clone()];
    if old == new_name {
        return Err(tr!("javascript.name_did_not_change").into());
    }
    // A name that already exists in the same scope would be merged with this one.
    let leaf = analysis
        .root()
        .descendant_for_byte_range(ranges[0].start, ranges[0].start);
    let is_member = leaf.is_some_and(|n| n.kind() == "property_identifier");
    let declaration = (!is_member)
        .then(|| analysis.resolve(old, ranges[0].start))
        .flatten();
    if let Some(decl) = declaration {
        let scope = analysis
            .scopes
            .iter()
            .position(|s| s.decls.iter().any(|d| d.name_range == decl.name_range));
        if let Some(scope) = scope
            && analysis.scopes[scope]
                .decls
                .iter()
                .any(|d| d.name == new_name)
        {
            return Err(trf!("javascript.already_exists_here", new_name = new_name));
        }
    }
    let edits: Vec<TextEdit> = ranges
        .iter()
        .map(|range| {
            let shorthand = analysis
                .root()
                .descendant_for_byte_range(range.start, range.end)
                .is_some_and(|n| {
                    matches!(
                        n.kind(),
                        "shorthand_property_identifier" | "shorthand_property_identifier_pattern"
                    )
                });
            TextEdit {
                range: range.clone(),
                // `{ a }` becomes `{ a: b }`: the property keeps its name.
                new_text: if shorthand {
                    format!("{old}: {new_name}")
                } else {
                    new_name.to_string()
                },
            }
        })
        .collect();
    let mut plan = vec![FileEdit { path: None, edits }];
    // A name of the module scope is imported by other files.
    if let (Some(decl), Some(path)) = (declaration, path)
        && analysis
            .scopes
            .first()
            .is_some_and(|s| s.decls.iter().any(|d| d.name_range == decl.name_range))
        && decl.kind != DeclKind::Import
    {
        plan.extend(importers(old, new_name, path, root));
    }
    Ok(plan)
}

fn project_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !SKIPPED.contains(&name.as_str()) && !name.starts_with('.') {
                    stack.push(path);
                }
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| EXTENSIONS.contains(&e))
            {
                files.push(path);
                if files.len() >= MAX_FILES {
                    return files;
                }
            }
        }
    }
    files
}

/// The files that import `old` from `file`: the import and every use of what it brings in.
fn importers(old: &str, new_name: &str, file: &Path, root: &Path) -> Vec<FileEdit> {
    let mut out = Vec::new();
    let target = file.components().collect::<PathBuf>();
    for other in project_files(root) {
        if other.components().collect::<PathBuf>() == target {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&other) else {
            continue;
        };
        if !content.contains(old) || content.len() > 1_000_000 {
            continue;
        }
        let _dialect = super::analysis::use_dialect(super::analysis::Dialect::of(Some(&other), ""));
        let Some(analysis) = Analysis::new(&content) else {
            continue;
        };
        let Some(dir) = other.parent() else { continue };
        let mut edits: Vec<TextEdit> = Vec::new();
        for scope in analysis.scopes.iter().take(1) {
            for decl in &scope.decls {
                let Some((source, imported)) = decl.import.as_ref().filter(|(_, i)| i == old)
                else {
                    continue;
                };
                if resolve_module(dir, source).map(|p| p.components().collect::<PathBuf>())
                    != Some(target.clone())
                {
                    continue;
                }
                // `import { old }`: the name is the local one too. `import { old as other }`
                // keeps its local name, only the imported one changes.
                let aliased = decl.name != *imported;
                if aliased {
                    // The imported name sits before ` as `.
                    let line_start = decl.name_range.start.saturating_sub(old.len() + 4);
                    if let Some(at) = content[line_start..decl.name_range.start].find(old) {
                        let start = line_start + at;
                        edits.push(TextEdit {
                            range: start..start + old.len(),
                            new_text: new_name.to_string(),
                        });
                    }
                    continue;
                }
                for range in highlights(&content, decl.name_range.start) {
                    edits.push(TextEdit {
                        range,
                        new_text: new_name.to_string(),
                    });
                }
            }
        }
        edits.sort_by_key(|e| e.range.start);
        edits.dedup();
        if !edits.is_empty() {
            out.push(FileEdit {
                path: Some(other),
                edits,
            });
        }
    }
    let _ = dialect();
    out
}

#[cfg(test)]
#[path = "../../../test/languages/javascript/rename/tests.rs"]
mod tests;
