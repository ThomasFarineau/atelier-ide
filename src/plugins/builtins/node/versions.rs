//! The versions of a dependency of `package.json`, from the npm registry, for the completion of
//! its value: the newest first, the `latest` one on top, as IntelliJ offers them.

use crate::languages::json::parse::Step;
use crate::languages::{CompletionItem, ItemKind, Oneshot};
use serde_json::Value;
use std::{
    cmp::Ordering,
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    thread,
    time::{Duration, Instant},
};

/// The fields of `package.json` whose keys are packages and whose values are versions.
const SECTIONS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];
/// How long the versions of a package are kept, and an error before it is asked again.
const KEEP: Duration = Duration::from_secs(10 * 60);
const KEEP_ERROR: Duration = Duration::from_secs(30);
/// How many versions the list shows at most.
const MAX_ITEMS: usize = 200;

/// A version of a package on the registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub number: String,
    /// The dist-tags that point to it (`latest`, `next`).
    pub tags: Vec<String>,
    pub deprecated: bool,
}

type Answer = Oneshot<Arc<Vec<Version>>>;

/// The versions asked for, by package: when, and the answer (pending while it comes).
static CACHE: LazyLock<Mutex<HashMap<String, (Instant, Answer)>>> = LazyLock::new(Mutex::default);

/// The hook of the schema of `package.json`: the versions of the dependency the caret is the
/// value of; None elsewhere.
pub fn complete(path: &[Step], typed: &str, quoted: bool) -> Option<Oneshot<Vec<CompletionItem>>> {
    let [Step::Key(section), Step::Key(name)] = path else {
        return None;
    };
    if !SECTIONS.contains(&section.as_str()) || !valid_name(name) || !is_range(typed) {
        return None;
    }
    let answer = versions(name);
    let prefix: String = typed
        .chars()
        .take_while(|c| matches!(c, '^' | '~' | '>' | '<' | '='))
        .collect();
    let out = Oneshot::pending();
    let slot = out.clone();
    thread::spawn(move || {
        let started = Instant::now();
        let found = loop {
            if let Some(found) = answer.take() {
                // Kept for the next one who asks.
                answer.fulfill(found.clone());
                break found;
            }
            if started.elapsed() > Duration::from_secs(10) {
                break Arc::default();
            }
            thread::sleep(Duration::from_millis(15));
        };
        slot.fulfill(items(&found, &prefix, quoted));
    });
    Some(out)
}

/// A name of the registry: `name` or `@scope/name`, nothing that would break the address.
fn valid_name(name: &str) -> bool {
    let bare = name.strip_prefix('@').unwrap_or(name);
    !bare.is_empty()
        && bare.matches('/').count() == usize::from(name.starts_with('@'))
        && bare
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '~'))
}

/// The value is a range of versions, not a path, an address, a tag of Git or a workspace.
fn is_range(typed: &str) -> bool {
    !typed.contains(':') && !typed.contains('/')
}

/// The completion items of the versions: the `latest` one first, then the newest first.
pub fn items(versions: &[Version], prefix: &str, quoted: bool) -> Vec<CompletionItem> {
    let latest = versions
        .iter()
        .position(|v| v.tags.iter().any(|t| t == "latest"));
    let order = latest
        .into_iter()
        .chain((0..versions.len()).filter(|i| Some(*i) != latest));
    order
        .take(MAX_ITEMS)
        .enumerate()
        .map(|(rank, i)| {
            let version = &versions[i];
            let label = format!("{prefix}{}", version.number);
            let insert = if quoted {
                label.clone()
            } else {
                Value::String(label.clone()).to_string()
            };
            let mut detail = version.tags.join(", ");
            if version.deprecated {
                if !detail.is_empty() {
                    detail.push_str(", ");
                }
                detail.push_str(tr!("node.deprecated"));
            }
            let mut item = CompletionItem::new(label, ItemKind::Constant)
                .insert(insert, None)
                .rank(rank as u32);
            if !detail.is_empty() {
                item = item.detail(detail);
            }
            item
        })
        .collect()
}

/// The versions of a package: from the cache, else asked for in the background.
fn versions(name: &str) -> Answer {
    let Ok(mut cache) = CACHE.lock() else {
        return Oneshot::ready(Arc::default());
    };
    if let Some((at, answer)) = cache.get(name) {
        let keep = match answer.take() {
            Some(found) => {
                let keep = if found.is_empty() { KEEP_ERROR } else { KEEP };
                answer.fulfill(found);
                keep
            }
            None => KEEP,
        };
        if at.elapsed() < keep {
            return answer.clone();
        }
    }
    let answer = Answer::pending();
    cache.insert(name.to_string(), (Instant::now(), answer.clone()));
    let slot = answer.clone();
    let name = name.to_string();
    thread::spawn(move || {
        slot.fulfill(Arc::new(fetch(&name).unwrap_or_default()));
    });
    answer
}

/// The registry: the one of the environment of npm, else npmjs.
fn registry() -> String {
    std::env::var("npm_config_registry")
        .or_else(|_| std::env::var("NPM_CONFIG_REGISTRY"))
        .ok()
        .filter(|r| r.starts_with("http"))
        .unwrap_or_else(|| "https://registry.npmjs.org".into())
}

fn fetch(name: &str) -> Option<Vec<Version>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .into();
    let url = format!(
        "{}/{}",
        registry().trim_end_matches('/'),
        name.replace('/', "%2F")
    );
    let text = agent
        .get(&url)
        .header("User-Agent", "atelier-ide")
        // The short form of the document: its versions and tags, without their readmes.
        .header("Accept", "application/vnd.npm.install-v1+json")
        .call()
        .ok()?
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_string()
        .ok()?;
    Some(parse(&serde_json::from_str(&text).ok()?))
}

/// The versions of a document of the registry, the newest first.
pub fn parse(document: &Value) -> Vec<Version> {
    let tags = document["dist-tags"].as_object();
    let Some(listed) = document["versions"].as_object() else {
        return Vec::new();
    };
    let mut versions: Vec<Version> = listed
        .iter()
        .map(|(number, meta)| Version {
            number: number.clone(),
            tags: tags
                .into_iter()
                .flatten()
                .filter(|(_, v)| v.as_str() == Some(number))
                .map(|(tag, _)| tag.clone())
                .collect(),
            deprecated: meta.get("deprecated").is_some_and(|d| !d.is_null()),
        })
        .collect();
    versions.sort_by(|a, b| compare(&b.number, &a.number));
    versions
}

/// Two versions in the order of semver: by their numbers, then a pre-release before its release.
pub fn compare(a: &str, b: &str) -> Ordering {
    fn split(v: &str) -> (Vec<u64>, Option<&str>) {
        let v = v.split('+').next().unwrap_or(v);
        let (core, pre) = match v.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (v, None),
        };
        (
            core.split('.').map(|n| n.parse().unwrap_or(0)).collect(),
            pre,
        )
    }
    fn pre_order(a: &str, b: &str) -> Ordering {
        for (x, y) in a.split('.').zip(b.split('.')) {
            let order = match (x.parse::<u64>(), y.parse::<u64>()) {
                (Ok(x), Ok(y)) => x.cmp(&y),
                (Ok(_), Err(_)) => Ordering::Less,
                (Err(_), Ok(_)) => Ordering::Greater,
                (Err(_), Err(_)) => x.cmp(y),
            };
            if order != Ordering::Equal {
                return order;
            }
        }
        a.split('.').count().cmp(&b.split('.').count())
    }
    let (core_a, pre_a) = split(a);
    let (core_b, pre_b) = split(b);
    core_a.cmp(&core_b).then_with(|| match (pre_a, pre_b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => pre_order(x, y),
    })
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/node/versions/tests.rs"]
mod tests;
