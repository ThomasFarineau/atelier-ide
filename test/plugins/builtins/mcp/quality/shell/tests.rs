use super::*;
use crate::plugins::builtins::mcp::quality::scratch;

const SLEEP: &str = if cfg!(windows) {
    "ping -n 90 127.0.0.1 > nul"
} else {
    "sleep 90"
};

fn run(cmd: &str, secs: u64) -> Outcome {
    run_quiet(cmd, Path::new("."), Duration::from_secs(secs), &[])
}

#[test]
fn exit_status_decides_pass_or_fail() {
    assert_eq!(run("exit 0", 120), Outcome::Pass);
    assert_eq!(run("exit 1", 120), Outcome::Fail);
}

#[test]
fn slow_commands_time_out() {
    let start = Instant::now();
    assert_eq!(run(SLEEP, 1), Outcome::Timeout);
    assert!(start.elapsed() < Duration::from_secs(60));
}

#[test]
fn commands_see_ci_and_the_directory() {
    let dir = scratch();
    let check = if cfg!(windows) {
        "if \"%CI%\"==\"1\" (exit 0) else (exit 1)"
    } else {
        "test \"$CI\" = 1"
    };
    assert_eq!(
        run_quiet(check, dir.path(), Duration::from_secs(120), &[]),
        Outcome::Pass
    );
}

#[test]
fn extra_environment_reaches_the_command() {
    let check = if cfg!(windows) {
        "if \"%UB_TEST%\"==\"yes\" (exit 0) else (exit 1)"
    } else {
        "test \"$UB_TEST\" = yes"
    };
    let dir = scratch();
    let run = |env: &[(&str, &str)]| run_quiet(check, dir.path(), Duration::from_secs(120), env);
    assert_eq!(run(&[("UB_TEST", "yes")]), Outcome::Pass);
    assert_eq!(run(&[]), Outcome::Fail);
}
