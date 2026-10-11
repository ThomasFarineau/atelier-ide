use super::*;

#[test]
fn a_task_is_listed_while_it_lives_even_after_a_panic() {
    let marker = format!("test-{}", NEXT.load(Ordering::Relaxed));
    let task = start(marker.clone());
    let mine = |list: Vec<Info>| list.into_iter().find(|i| i.title == marker);
    assert!(mine(running()).is_some());
    drop(task);
    assert!(mine(running()).is_none());
    let marker2 = format!("{marker}-panic");
    let name = marker2.clone();
    let _ = std::thread::spawn(move || {
        let _task = start(name);
        panic!("boom");
    })
    .join();
    assert!(
        running().iter().all(|i| i.title != marker2),
        "the guard went with the thread"
    );
}
