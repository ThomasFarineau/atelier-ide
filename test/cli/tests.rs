use super::*;

#[test]
fn a_path_goes_to_the_ide_of_its_project_and_a_dead_one_is_forgotten() {
    let base = std::env::temp_dir().join(format!("atelier-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let (a, b) = (base.join("a"), base.join("b"));
    fs::create_dir_all(a.join("src")).unwrap();
    fs::create_dir_all(&b).unwrap();
    // SAFETY: the only test that reads or writes this variable.
    unsafe { std::env::set_var(DIR_ENV, base.join("registry")) };
    let server_a = serve(&a, Arc::new(|| {})).unwrap();
    // A second IDE has its own entry: the file name is the process id, so fake another one.
    let mut server_b = serve(&b, Arc::new(|| {})).unwrap();
    server_b.file = base.join("registry").join("2.json");
    server_b.entry.pid = 2;
    server_b.write();
    server_a.write();
    let file = a.join("src").join("main.js");
    fs::write(&file, "x").unwrap();
    assert!(forward(&file), "an IDE takes it");
    assert_eq!(
        server_a.poll(),
        vec![file.clone()],
        "the one of the project that contains the file"
    );
    assert!(server_b.poll().is_empty());
    // Outside every project: the one used last.
    let elsewhere = base.join("notes.txt");
    fs::write(&elsewhere, "x").unwrap();
    server_b.entry.active_at = now() + 1000;
    server_b.write();
    assert!(forward(&elsewhere));
    assert_eq!(server_b.poll(), vec![elsewhere]);
    // Once the IDEs are gone, nobody answers.
    let registry = registry_dir();
    drop(server_a);
    drop(server_b);
    let _ = fs::remove_file(registry.join("2.json"));
    assert!(!forward(&file));
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn a_wrong_secret_is_ignored() {
    let base = std::env::temp_dir().join(format!("atelier-cli-secret-{}", std::process::id()));
    fs::create_dir_all(&base).unwrap();
    let listener_free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let entry = Entry {
        pid: 1,
        port: listener_free.local_addr().unwrap().port(),
        token: "x".into(),
        root: base.clone(),
        active_at: 0,
    };
    // Nobody serves this port: the request gets no "ok".
    drop(listener_free);
    assert!(!send(&entry, &base));
    let _ = fs::remove_dir_all(&base);
}
