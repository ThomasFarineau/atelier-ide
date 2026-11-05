//! Running project commands (tests, coverage) with a timeout, without a console window.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Outcome {
    Pass,
    Fail,
    Timeout,
}

/// `program`, without the console window Windows would open for it from the IDE.
pub fn hidden(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

fn command(cmd: &str, dir: &Path) -> Command {
    let mut c;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c = hidden("cmd");
        c.arg("/C").raw_arg(cmd);
    }
    #[cfg(not(windows))]
    {
        c = hidden("sh");
        c.args(["-c", cmd]);
    }
    c.current_dir(dir).env("CI", "1");
    c
}

fn kill_tree(child: &mut Child) {
    if cfg!(windows) {
        let _ = hidden("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Run `cmd` with extra environment variables, output silenced; kill it after `timeout`.
pub fn run_quiet(cmd: &str, dir: &Path, timeout: Duration, env: &[(&str, &str)]) -> Outcome {
    let spawned = command(cmd, dir)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        Ok(child) => wait_for(child, timeout),
        Err(_) => Outcome::Fail,
    }
}

fn wait_for(mut child: Child, timeout: Duration) -> Outcome {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Outcome::Pass,
            Ok(Some(_)) => return Outcome::Fail,
            Ok(None) if start.elapsed() > timeout => {
                kill_tree(&mut child);
                return Outcome::Timeout;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return Outcome::Fail,
        }
    }
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/shell/tests.rs"]
mod tests;
