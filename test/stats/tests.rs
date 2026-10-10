use super::*;

#[test]
fn memory_is_shown_in_mo_then_in_go() {
    assert_eq!(format_memory(312 * 1024 * 1024 + 1000), "312 Mo");
    assert_eq!(format_memory(1536 * 1024 * 1024), "1,5 Go");
    assert_eq!(format_memory(0), "0 Mo");
}

#[test]
fn the_process_is_looked_at_and_its_memory_is_known_within_a_few_seconds() {
    start();
    start();
    for _ in 0..500 {
        if memory_bytes() > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        memory_bytes() > 1024 * 1024,
        "this test process uses more than a megabyte: {}",
        memory_bytes()
    );
    let summary = summary().expect("a summary once the memory is known");
    assert!(
        summary.starts_with("RAM ") && summary.contains("CPU ") && summary.ends_with('%'),
        "{summary}"
    );
    assert!(cpu_percent() >= 0.0 && cpu_percent() <= 100.0 + 1.0);
}

#[test]
fn a_tree_adds_the_process_and_all_that_it_started() {
    let usage: HashMap<u32, (u64, f32)> = [
        (1, (100, 1.0)),
        (2, (50, 2.0)),
        (3, (25, 0.5)),
        (9, (1000, 9.0)),
    ]
    .into();
    let children: HashMap<u32, Vec<u32>> = [(1, vec![2]), (2, vec![3]), (3, vec![1])].into();
    assert_eq!(
        tree_total(&children, &usage, 1),
        (175, 3.5),
        "a loop (a reused pid) is counted once, other trees are not"
    );
    assert_eq!(tree_total(&children, &usage, 3), (175, 3.5));
    assert_eq!(tree_total(&children, &usage, 77), (0, 0.0));
    assert_eq!(format_usage(312 * 1024 * 1024, 0.31), "312 Mo · 0,3 %");
    assert_eq!(format_usage(2048 * 1024 * 1024, 42.0), "2,0 Go · 42 %");
}

#[test]
fn what_a_process_started_is_counted_with_it() {
    // A child that sleeps for a few seconds, started by this test process.
    #[cfg(windows)]
    let mut child = std::process::Command::new("ping")
        .args(["-n", "60", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    #[cfg(not(windows))]
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let mut found = None;
    // A busy machine (the other tests run side by side) may take long to look at every process.
    for _ in 0..1600 {
        found = watch(child.id());
        if found.is_some_and(|(memory, _)| memory > 0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let _ = child.wait();
    let (memory, cpu) = found.expect("the tree is known within a few seconds");
    assert!(memory > 100 * 1024, "a process uses some memory: {memory}");
    assert!((0.0..=100.0).contains(&cpu));
}
