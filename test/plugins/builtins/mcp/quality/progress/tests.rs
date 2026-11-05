use super::*;

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn durations_are_short_and_readable() {
    assert_eq!(fmt_duration(secs(0)), "0s");
    assert_eq!(fmt_duration(secs(59)), "59s");
    assert_eq!(fmt_duration(secs(60)), "1m00s");
    assert_eq!(fmt_duration(secs(185)), "3m05s");
    assert_eq!(fmt_duration(secs(3599)), "59m59s");
    assert_eq!(fmt_duration(secs(3600)), "1h00m");
    assert_eq!(fmt_duration(secs(3720)), "1h02m");
}

#[test]
fn summaries_name_the_work_threads_and_time() {
    assert_eq!(
        summary("analyzed 4 files", 8, secs(75)),
        "analyzed 4 files on 8 thread(s) in 1m15s"
    );
}
