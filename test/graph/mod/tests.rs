use super::*;

fn wait_for(service: &Service, what: impl Fn(&Status) -> bool) -> Status {
    // A busy machine (the other tests run side by side) may take a while.
    for _ in 0..2000 {
        let status = service.status();
        if what(&status) {
            return status;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("never reached: {:?}", service.status());
}

#[test]
fn the_graph_is_made_kept_up_to_date_and_dropped_when_turned_off() {
    let root = std::env::temp_dir().join(format!("atelier-graph-service-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/a.js"),
        "import './b.js';\nexport function first() { second(); }\n",
    )
    .unwrap();
    fs::write(root.join("src/b.js"), "export function second() {}\n").unwrap();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    let mut service = Service::default();
    service.set_root(&root, wake.clone());
    assert!(
        !service.enabled() && !service.should_offer(),
        "off until the user says so (and no repository here, so no offer)"
    );
    fs::create_dir_all(root.join(".git")).unwrap();
    service.set_root(&root, wake.clone());
    assert!(
        service.should_offer(),
        "a repository nobody answered for is offered the graph"
    );
    service.postpone();
    assert!(!service.should_offer());
    service.enable(wake.clone());
    let status = wait_for(&service, |s| s.files == 2 && !s.busy);
    assert_eq!(status.symbols, 2);
    // The graph of the IDE is global, and other tests start services of their own: the file
    // tells what this one made.
    let dir = folder(&root);
    assert!(
        fs::read_to_string(dir.join("graph.json"))
            .unwrap()
            .contains("\"first\"")
    );
    assert!(dir.join("cache.json").exists());
    assert!(
        fs::read_to_string(dir.join("GRAPH_REPORT.md"))
            .unwrap()
            .contains("# Context graph")
    );
    assert_eq!(
        fs::read_to_string(root.join(".atelier/.gitignore")).unwrap(),
        "graph/\n"
    );
    // A file changes, one appears, one goes: the graph follows without a new request.
    fs::write(
        root.join("src/a.js"),
        "import './b.js';\nexport function first() { second(); }\nexport function third() {}\n",
    )
    .unwrap();
    fs::write(root.join("src/c.rs"), "pub fn fourth() {}\n").unwrap();
    fs::remove_file(root.join("src/b.js")).unwrap();
    service.file_changed();
    // The status and the published graph are read apart: wait until they agree.
    wait_for(&service, |s| {
        s.symbols == 3
            && s.files == 2
            && current().is_some_and(|g| {
                g.communities.len() == s.groups
                    && g.nodes.iter().any(|n| n.label == "fourth")
                    && !g.nodes.iter().any(|n| n.label == "second")
            })
    });
    // A restart reads nothing again: the cache answers for the files that did not change.
    let cached = load_cache(&root);
    assert_eq!(cached.files.len(), 2);
    service.disable();
    assert!(current().is_none() && !service.enabled());
    assert_eq!(crate::atelier::graph_setting(&root), Some(false));
    let _ = fs::remove_dir_all(&root);
}
