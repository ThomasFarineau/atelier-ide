use super::*;

#[test]
fn a_run_shows_its_gate_and_its_mutants_then_ends() {
    let _serial = serial();
    {
        let _run = start("cleaner").unwrap();
        gate("mutator");
        phase("mutants");
        total(3);
        tick();
        let p = current().unwrap();
        assert_eq!(
            (
                p.tool.as_str(),
                p.gate.as_str(),
                p.phase.as_str(),
                p.done,
                p.total
            ),
            ("cleaner", "mutator", "mutants", 1, 3)
        );
        assert!(start("dryer").is_err(), "one run at a time");
    }
    assert!(current().is_none());
    tick();
    assert!(current().is_none(), "no run, nothing to count");
}

#[test]
fn a_paused_run_waits_at_the_next_checkpoint_until_resumed() {
    let _serial = serial();
    let _run = start("mutator").unwrap();
    set_paused(true);
    assert!(paused());
    let waiting = std::thread::spawn(|| {
        let start = Instant::now();
        checkpoint();
        start.elapsed()
    });
    std::thread::sleep(Duration::from_millis(400));
    assert!(!waiting.is_finished(), "the checkpoint holds while paused");
    set_paused(false);
    let waited = waiting.join().unwrap();
    assert!(waited >= Duration::from_millis(300));
    let p = current().unwrap();
    assert!(p.paused_for >= Duration::from_millis(300));
    assert!(
        p.elapsed() < p.started.elapsed(),
        "the pause is not counted"
    );
}

#[test]
fn the_end_of_a_run_lifts_its_pause() {
    let _serial = serial();
    {
        let _run = start("mutator").unwrap();
        set_paused(true);
    }
    assert!(!paused());
    set_paused(true);
    assert!(!paused(), "nothing to pause without a run");
}
