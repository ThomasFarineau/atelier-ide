use super::*;

#[test]
fn versions_are_in_the_order_of_semver() {
    assert_eq!(compare("1.10.0", "1.9.0"), Ordering::Greater);
    assert_eq!(compare("2.0.0-beta.1", "2.0.0"), Ordering::Less);
    assert_eq!(compare("2.0.0-beta.2", "2.0.0-beta.10"), Ordering::Less);
    assert_eq!(compare("2.0.0-alpha", "2.0.0-beta"), Ordering::Less);
    assert_eq!(compare("1.0.0+build", "1.0.0"), Ordering::Equal);
}

#[test]
fn a_document_of_the_registry_gives_its_versions_newest_first() {
    let document = serde_json::json!({
        "dist-tags": { "latest": "1.10.0", "next": "2.0.0-rc.1" },
        "versions": {
            "1.9.0": {},
            "1.10.0": {},
            "2.0.0-rc.1": {},
            "0.1.0": { "deprecated": "use 1.x" }
        }
    });
    let versions = parse(&document);
    let numbers: Vec<&str> = versions.iter().map(|v| v.number.as_str()).collect();
    assert_eq!(numbers, ["2.0.0-rc.1", "1.10.0", "1.9.0", "0.1.0"]);
    assert_eq!(versions[0].tags, ["next"]);
    assert_eq!(versions[1].tags, ["latest"]);
    assert!(versions[3].deprecated);
}

#[test]
fn the_latest_comes_first_with_the_range_typed() {
    let versions = parse(&serde_json::json!({
        "dist-tags": { "latest": "1.10.0" },
        "versions": { "1.9.0": {}, "1.10.0": {}, "2.0.0-rc.1": {} }
    }));
    let items = items(&versions, "^", true);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["^1.10.0", "^2.0.0-rc.1", "^1.9.0"]);
    assert_eq!(items[0].detail.as_deref(), Some("latest"));
    assert!(items[0].rank < items[1].rank);
    // Out of quotes, the value is a string of JSON.
    assert_eq!(super::items(&versions, "", false)[0].insert, "\"1.10.0\"");
}

#[test]
fn only_the_versions_of_dependencies_are_offered() {
    let at = |section: &str, name: &str| vec![Step::Key(section.into()), Step::Key(name.into())];
    assert!(complete(&at("scripts", "build"), "", true).is_none());
    assert!(complete(&at("dependencies", "bad name"), "", true).is_none());
    assert!(complete(&at("dependencies", "a/b"), "", true).is_none());
    assert!(complete(&at("dependencies", "lib"), "file:../lib", true).is_none());
    assert!(complete(&at("dependencies", "lib"), "workspace:*", true).is_none());
    assert!(valid_name("@scope/name"));
    assert!(valid_name("lodash.merge"));
}
