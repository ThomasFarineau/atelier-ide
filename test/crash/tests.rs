use super::*;

#[test]
fn a_crash_is_told_by_its_error_output() {
    let text = "warning: something\nthread 'main' panicked at src/app.rs:1:1:\nboom\n";
    assert_eq!(
        crash_line(text),
        Some("thread 'main' panicked at src/app.rs:1:1:")
    );
    assert_eq!(
        crash_line("memory allocation of 4294967296 bytes failed\n"),
        Some("memory allocation of 4294967296 bytes failed")
    );
    assert_eq!(crash_line("settings: cannot write\n"), None);
}

#[test]
fn days_since_1970_are_dates() {
    assert_eq!(civil(0), (1970, 1, 1));
    assert_eq!(civil(19_723), (2024, 1, 1));
    assert_eq!(civil(20_757), (2026, 10, 31));
}

#[test]
fn the_logs_of_processes_that_are_gone_are_gathered() {
    let dir = std::env::temp_dir().join(format!("atelier-crash-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("1.log"), "").unwrap();
    fs::write(
        dir.join("2.log"),
        "thread 'main' has overflowed its stack\n",
    )
    .unwrap();
    fs::write(dir.join("notes.txt"), "panicked at").unwrap();
    let found = collect(&dir, &dir.join("crash.log"));
    assert!(!dir.join("1.log").exists());
    assert!(dir.join("notes.txt").exists());
    assert!(!dir.join("2.log").exists());
    assert_eq!(
        found.as_deref(),
        Some("thread 'main' has overflowed its stack")
    );
    let log = fs::read_to_string(dir.join("crash.log")).unwrap();
    assert!(log.contains("---- process 2") && log.contains("overflowed"));
    let _ = fs::remove_dir_all(&dir);
}
