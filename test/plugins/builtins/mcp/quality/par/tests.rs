use super::*;
use std::collections::HashSet;
use std::thread::ThreadId;

#[test]
fn results_keep_the_order_of_the_items() {
    let items: Vec<usize> = (0..50).collect();
    assert_eq!(
        map(&items, 4, |i| i * 2),
        (0..50).map(|i| i * 2).collect::<Vec<_>>()
    );
}

#[test]
fn empty_input_gives_empty_output() {
    assert!(map(&Vec::<u8>::new(), 4, |x| *x).is_empty());
}

#[test]
fn work_is_shared_between_threads() {
    let items: Vec<usize> = (0..8).collect();
    let ids: Vec<ThreadId> = map(&items, 4, |_| {
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::thread::current().id()
    });
    assert!(ids.iter().collect::<HashSet<_>>().len() > 1);
}

#[test]
fn one_job_runs_on_one_thread() {
    let items: Vec<usize> = (0..6).collect();
    let ids: Vec<ThreadId> = map(&items, 1, |_| std::thread::current().id());
    assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 1);
}

#[test]
fn jobs_option_defaults_and_rejects_zero() {
    let parse = |a: &[&str]| {
        let argv: Vec<String> = a.iter().map(|s| s.to_string()).collect();
        jobs(&Args::parse(&argv, &[], &["jobs"]).unwrap())
    };
    assert_eq!(parse(&[]).unwrap(), default_jobs());
    assert_eq!(parse(&["--jobs", "3"]).unwrap(), 3);
    assert!(parse(&["--jobs", "0"]).is_err());
    assert!(parse(&["--jobs", "x"]).is_err());
}
