//! The graph of a project, made from what every file says about itself: files and what they
//! define as nodes, "contains", "imports", "calls", "extends"… as links, and the groups of files
//! that belong together (communities). A link is `EXTRACTED` when the code states it (an import,
//! a call to a name the file defines or imports) and `INFERRED` when it is a guess from a name
//! that only one thing of the project has.

use super::extract::{Facts, RefKind};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Component, Path, PathBuf},
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub kind: String,
    /// The file, from the project, with `/`.
    pub file: String,
    pub line: usize,
    pub community: usize,
    /// Links other than "contains": how much the rest of the project leans on this.
    pub degree: usize,
    /// Size of the file in bytes (file nodes only).
    #[serde(default)]
    pub size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Link {
    pub source: usize,
    pub target: usize,
    pub kind: String,
    pub confidence: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Community {
    pub id: usize,
    pub label: String,
    /// Indices of the file nodes.
    pub files: Vec<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    pub communities: Vec<Community>,
    /// When it was made, in milliseconds since the epoch.
    pub generated: u64,
}

/// Names too common to say anything when they are the only match.
const COMMON: [&str; 40] = [
    "new", "get", "set", "map", "push", "pop", "len", "iter", "from", "into", "default", "clone",
    "next", "init", "main", "run", "test", "name", "show", "update", "close", "open", "read",
    "write", "contains", "remove", "insert", "replace", "count", "find", "take", "ready", "with",
    "lines", "start", "stop", "build", "parse", "load", "save",
];
/// Links a single symbol may have of one kind: a function that calls everything says nothing.
const MAX_LINKS_PER_SOURCE: usize = 40;

fn norm(path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(n) => parts.push(n.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    parts.join("/")
}

/// The directory that holds the module files of `rel`: the one of `lib.rs`, `main.rs` and
/// `mod.rs`, else a folder named like the file.
fn module_dir(rel: &str) -> PathBuf {
    let path = Path::new(rel);
    let dir = path.parent().unwrap_or(Path::new(""));
    match path.file_stem().and_then(|s| s.to_str()) {
        Some("lib" | "main" | "mod") | None => dir.to_path_buf(),
        Some(stem) => dir.join(stem),
    }
}

/// The folder of the crate a file belongs to (the one with `src/`): `src/graph/build.rs` is in the
/// crate of `src`.
fn crate_src(rel: &str, files: &BTreeMap<String, Facts>) -> String {
    let mut dir = Path::new(rel).parent();
    while let Some(d) = dir {
        let candidate = norm(&d.join("lib.rs"));
        if files.contains_key(&candidate) || files.contains_key(&norm(&d.join("main.rs"))) {
            return norm(d);
        }
        dir = d.parent();
    }
    "src".into()
}

/// The file a Rust path of a `use` or `mod` stands for.
fn resolve_rust(from: &str, spec: &str, files: &BTreeMap<String, Facts>) -> Option<String> {
    let exists = |p: PathBuf| -> Option<String> {
        let rs = norm(&p.with_extension("rs"));
        if files.contains_key(&rs) {
            return Some(rs);
        }
        let module = norm(&p.join("mod.rs"));
        files.contains_key(&module).then_some(module)
    };
    if let Some(name) = spec.strip_prefix("mod:") {
        return exists(module_dir(from).join(name));
    }
    let mut parts: Vec<&str> = spec.split("::").collect();
    let mut base = match parts.first().copied() {
        Some("crate") => {
            parts.remove(0);
            PathBuf::from(crate_src(from, files))
        }
        Some("self") => {
            parts.remove(0);
            module_dir(from)
        }
        Some("super") => {
            let mut dir = module_dir(from);
            while parts.first() == Some(&"super") {
                parts.remove(0);
                dir = dir.parent().map(Path::to_path_buf).unwrap_or_default();
            }
            dir
        }
        // Other crates and the standard library are not part of the project.
        _ => return None,
    };
    // The longest start of the path that is a module file: the rest names things inside it.
    let mut found = None;
    for part in &parts {
        base.push(part);
        match exists(base.clone()) {
            Some(file) => found = Some(file),
            None => break,
        }
    }
    found.filter(|f| f != from)
}

/// The file a JavaScript or Markdown specifier stands for.
fn resolve_relative(
    root: &Path,
    from: &str,
    spec: &str,
    language: &str,
    files: &BTreeMap<String, Facts>,
) -> Option<String> {
    let dir = Path::new(from).parent().unwrap_or(Path::new(""));
    if language == "md" {
        let target = norm(&dir.join(spec));
        return files.contains_key(&target).then_some(target);
    }
    let absolute_dir = root.join(dir);
    let resolved = crate::languages::javascript::navigate::resolve_module(&absolute_dir, spec)?;
    let resolved = crate::project::simplify(&resolved);
    let rel = resolved
        .strip_prefix(crate::project::simplify(root))
        .ok()
        .map(norm)?;
    files.contains_key(&rel).then_some(rel)
}

/// Builds the graph of the project at `root` from the facts of its files (by path from `root`).
pub fn build(root: &Path, files: &BTreeMap<String, Facts>, generated: u64) -> Graph {
    let mut graph = Graph {
        generated,
        ..Graph::default()
    };
    let mut file_node: HashMap<&str, usize> = HashMap::new();
    // Symbol nodes by file: the index of each symbol of the facts.
    let mut symbol_node: HashMap<(&str, usize), usize> = HashMap::new();
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut links: BTreeSet<(usize, usize, &'static str, &'static str)> = BTreeSet::new();

    for (rel, facts) in files {
        let at = graph.nodes.len();
        graph.nodes.push(Node {
            id: format!("f:{rel}"),
            label: rel.clone(),
            kind: "file".into(),
            file: rel.clone(),
            line: 1,
            community: 0,
            degree: 0,
            size: 0,
        });
        file_node.insert(rel, at);
        for (index, symbol) in facts.symbols.iter().enumerate() {
            let mut id = format!("s:{rel}#{}", symbol.name);
            if !seen_ids.insert(id.clone()) {
                id = format!("{id}@{}", symbol.line);
                seen_ids.insert(id.clone());
            }
            let node = graph.nodes.len();
            graph.nodes.push(Node {
                id,
                label: symbol.name.clone(),
                kind: symbol.kind.clone(),
                file: rel.clone(),
                line: symbol.line,
                community: 0,
                degree: 0,
                size: 0,
            });
            symbol_node.insert((rel, index), node);
            by_name.entry(symbol.name.clone()).or_default().push(node);
            if let Some((_, last)) = symbol.name.rsplit_once("::") {
                by_name.entry(last.to_string()).or_default().push(node);
            }
            // A method belongs to its type when the file defines it, else to the file.
            let parent = symbol
                .name
                .rsplit_once("::")
                .and_then(|(ty, _)| facts.symbols.iter().position(|s| s.name == ty));
            let source = parent
                .and_then(|p| symbol_node.get(&(rel.as_str(), p)).copied())
                .unwrap_or(at);
            links.insert((source, node, "contains", "EXTRACTED"));
        }
    }

    // What each file imports, as files of the project.
    let mut imported: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (rel, facts) in files {
        for spec in &facts.imports {
            let target = match facts.language.as_str() {
                "rs" => resolve_rust(rel, spec, files),
                _ => resolve_relative(root, rel, spec, &facts.language, files),
            };
            let Some(target) =
                target.and_then(|t| files.get_key_value(&t).map(|(k, _)| k.as_str()))
            else {
                continue;
            };
            if target == rel {
                continue;
            }
            let kind = if facts.language == "md" {
                "links"
            } else if spec.starts_with("mod:") {
                "declares"
            } else {
                "imports"
            };
            links.insert((
                file_node[rel.as_str()],
                file_node[target],
                kind,
                "EXTRACTED",
            ));
            imported.entry(rel.as_str()).or_default().insert(target);
        }
    }

    // What each file calls, extends or implements, by name.
    for (rel, facts) in files {
        let mut per_source: HashMap<usize, usize> = HashMap::new();
        for reference in &facts.refs {
            let Some(candidates) = by_name.get(&reference.name) else {
                continue;
            };
            // The type of the symbol the reference is in: `Self::x()` and `self.x()` mean it.
            let own_type = reference.from.and_then(|f| {
                facts.symbols[f]
                    .name
                    .rsplit_once("::")
                    .map(|(t, _)| t.to_string())
            });
            let qualifier = match reference.qualifier.as_deref() {
                Some("Self" | "self") => own_type.clone(),
                other => other.map(str::to_string),
            };
            let pool: Vec<usize> = if let Some(q) = &qualifier {
                // A call on a type, or through a module: what does not fit is another crate's.
                let wanted = format!("{q}::{}", reference.name);
                candidates
                    .iter()
                    .copied()
                    .filter(|&c| {
                        let node = &graph.nodes[c];
                        let module = Path::new(&node.file)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .map(|s| {
                                if s == "mod" {
                                    Path::new(&node.file)
                                        .parent()
                                        .and_then(|p| p.file_name())
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(s)
                                } else {
                                    s
                                }
                            });
                        node.label == wanted
                            || (!node.label.contains("::") && module == Some(q.as_str()))
                    })
                    .collect()
            } else if let Some(receiver) = &reference.receiver {
                if receiver == "self" || receiver == "this" {
                    // A method of the same type.
                    match &own_type {
                        Some(t) => {
                            let wanted = format!("{t}::{}", reference.name);
                            candidates
                                .iter()
                                .copied()
                                .filter(|&c| graph.nodes[c].label == wanted)
                                .collect()
                        }
                        None => Vec::new(),
                    }
                } else {
                    // On some other value: only a method that stands out, of a file this one imports.
                    candidates
                        .iter()
                        .copied()
                        .filter(|&c| {
                            reference.name.len() >= 5
                                && !COMMON.contains(&reference.name.as_str())
                                && imported
                                    .get(rel.as_str())
                                    .is_some_and(|set| set.contains(graph.nodes[c].file.as_str()))
                        })
                        .collect()
                }
            } else {
                candidates.clone()
            };
            let source = match reference.from {
                Some(from) => symbol_node[&(rel.as_str(), from)],
                None => file_node[rel.as_str()],
            };
            let same_file: Vec<usize> = pool
                .iter()
                .copied()
                .filter(|&c| graph.nodes[c].file == *rel)
                .collect();
            let known: Vec<usize> = pool
                .iter()
                .copied()
                .filter(|&c| {
                    imported
                        .get(rel.as_str())
                        .is_some_and(|set| set.contains(graph.nodes[c].file.as_str()))
                })
                .collect();
            let (targets, confidence) = if !same_file.is_empty() {
                (same_file, "EXTRACTED")
            } else if !known.is_empty() {
                (known, "EXTRACTED")
            } else if pool.len() == 1
                && reference.name.len() >= 4
                && !COMMON.contains(&reference.name.as_str())
                && reference.receiver.is_none()
            {
                (pool, "INFERRED")
            } else {
                continue;
            };
            let kind = match reference.kind {
                RefKind::Calls => "calls",
                RefKind::Extends => "extends",
                RefKind::Implements => "implements",
                RefKind::Instantiates => "instantiates",
            };
            for target in targets.into_iter().take(2) {
                if target == source
                    || per_source.get(&source).copied().unwrap_or(0) >= MAX_LINKS_PER_SOURCE
                {
                    continue;
                }
                if links.insert((source, target, kind, confidence)) {
                    *per_source.entry(source).or_default() += 1;
                }
            }
        }
    }

    graph.links = links
        .into_iter()
        .map(|(source, target, kind, confidence)| Link {
            source,
            target,
            kind: kind.into(),
            confidence: confidence.into(),
        })
        .collect();
    for link in graph.links.iter().filter(|l| l.kind != "contains") {
        graph.nodes[link.source].degree += 1;
        graph.nodes[link.target].degree += 1;
    }
    cluster(&mut graph, &file_node);
    graph
}

/// Groups the files that lean on each other: each file takes the group most of its neighbours
/// are in, until nothing moves. Files that nothing links to are grouped by folder.
fn cluster(graph: &mut Graph, file_node: &HashMap<&str, usize>) {
    let files: Vec<usize> = {
        let mut v: Vec<usize> = file_node.values().copied().collect();
        v.sort_unstable();
        v
    };
    let position: HashMap<usize, usize> = files.iter().enumerate().map(|(i, &n)| (n, i)).collect();
    let file_of = |graph: &Graph, node: usize| {
        position
            .get(&file_node[graph.nodes[node].file.as_str()])
            .copied()
    };
    let mut weights: Vec<HashMap<usize, u32>> = vec![HashMap::new(); files.len()];
    for link in graph.links.iter().filter(|l| l.kind != "contains") {
        if let (Some(a), Some(b)) = (file_of(graph, link.source), file_of(graph, link.target))
            && a != b
        {
            *weights[a].entry(b).or_default() += 1;
            *weights[b].entry(a).or_default() += 1;
        }
    }
    let mut label: Vec<usize> = (0..files.len()).collect();
    for _ in 0..20 {
        let mut moved = false;
        for i in 0..files.len() {
            let mut score: BTreeMap<usize, u32> = BTreeMap::new();
            for (&n, &w) in &weights[i] {
                *score.entry(label[n]).or_default() += w;
            }
            // The most frequent label among the neighbours; the smallest one on a tie.
            if let Some((&best, _)) = score
                .iter()
                .max_by_key(|(l, w)| (**w, std::cmp::Reverse(**l)))
                && best != label[i]
            {
                label[i] = best;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    // Files with no link at all: one group per folder.
    let mut by_folder: BTreeMap<String, usize> = BTreeMap::new();
    for (i, &node) in files.iter().enumerate() {
        if weights[i].is_empty() {
            let folder = Path::new(&graph.nodes[node].file)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            label[i] = *by_folder.entry(folder).or_insert(files.len() + i);
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, &node) in files.iter().enumerate() {
        groups.entry(label[i]).or_default().push(node);
    }
    let mut groups: Vec<Vec<usize>> = groups.into_values().collect();
    groups.sort_by_key(|g| (std::cmp::Reverse(g.len()), g[0]));
    for (id, members) in groups.iter().enumerate() {
        let folders: BTreeSet<&str> = members
            .iter()
            .map(|&n| {
                Path::new(graph.nodes[n].file.as_str())
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or("")
            })
            .collect();
        let name = if folders.len() == 1 && !folders.iter().next().is_some_and(|f| f.is_empty()) {
            folders
                .iter()
                .next()
                .map(|f| f.to_string())
                .unwrap_or_default()
        } else {
            // The file the group leans on most.
            members
                .iter()
                .max_by_key(|&&n| (graph.nodes[n].degree, std::cmp::Reverse(n)))
                .map(|&n| graph.nodes[n].label.clone())
                .unwrap_or_default()
        };
        graph.communities.push(Community {
            id,
            label: name,
            files: members.clone(),
        });
    }
    let community_of_file: HashMap<String, usize> = graph
        .communities
        .iter()
        .flat_map(|c| c.files.iter().map(move |&n| (n, c.id)))
        .map(|(n, c)| (graph.nodes[n].file.clone(), c))
        .collect();
    for node in &mut graph.nodes {
        node.community = community_of_file.get(&node.file).copied().unwrap_or(0);
    }
}

#[cfg(test)]
#[path = "../../test/graph/build/tests.rs"]
mod tests;
