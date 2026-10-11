//! Development mode: `cargo dev` (alias of `cargo run -- --dev`).
//!
//! The supervisor process has no window. It runs the IDE as a child process, watches the
//! IDE's own sources, rebuilds into `target/hot` and swaps the child for the new build.
//! The child saves its session (project, tabs, unsaved text) before exiting, and the new
//! child restores it, so a reload keeps the work in progress.
//!
//! Builds go to a separate target directory and each build is copied before running,
//! because Windows locks a running executable and cargo could not overwrite it.

use anyhow::{Context, Result};
use std::{
    ffi::OsString,
    fs,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

pub const FLAG: &str = "--dev";
/// Set on the child: it listens on stdin for reload requests.
pub const CHILD_ENV: &str = "ATELIER_DEV_CHILD";
/// Set on the child: where to save the session before a reload and restore it at startup.
pub const SESSION_ENV: &str = "ATELIER_DEV_SESSION";
const RELOAD: &str = "reload";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn hot_dir() -> PathBuf {
    crate_dir().join("target").join("hot")
}

pub fn session_path() -> Option<PathBuf> {
    std::env::var_os(SESSION_ENV).map(PathBuf::from)
}

/// In the child, returns a flag raised when the supervisor asks for a reload.
pub fn listen(ctx: &eframe::egui::Context) -> Option<Arc<AtomicBool>> {
    std::env::var_os(CHILD_ENV)?;
    let requested = Arc::new(AtomicBool::new(false));
    let flag = requested.clone();
    let ctx = ctx.clone();
    thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if line.trim() == RELOAD {
                flag.store(true, Ordering::SeqCst);
                ctx.request_repaint();
            }
        }
    });
    Some(requested)
}

/// Latest modification time and file count of the watched sources.
fn snapshot(dir: &Path) -> (Option<SystemTime>, usize) {
    fn walk(path: &Path, latest: &mut Option<SystemTime>, count: &mut usize) {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, latest, count);
            } else if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                *count += 1;
                *latest = (*latest).max(Some(modified));
            }
        }
    }
    let (mut latest, mut count) = (None, 0);
    walk(&dir.join("src"), &mut latest, &mut count);
    for file in ["Cargo.toml", "Cargo.lock"] {
        if let Ok(modified) = fs::metadata(dir.join(file)).and_then(|m| m.modified()) {
            count += 1;
            latest = latest.max(Some(modified));
        }
    }
    (latest, count)
}

fn build(dir: &Path) -> Result<PathBuf> {
    let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["build", "--color", "always", "--target-dir"])
        .arg(hot_dir())
        .current_dir(dir)
        .status()
        .context(tr!("cannot_start_cargo"))?;
    anyhow::ensure!(status.success(), "{}", tr!("build_failed"));
    let name = format!("atelier-ide{}", std::env::consts::EXE_SUFFIX);
    Ok(hot_dir().join("debug").join(name))
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    exe: PathBuf,
}

fn spawn(exe: &Path, args: &[OsString], session: &Path) -> Result<Running> {
    let mut child = Command::new(exe)
        .args(args)
        .env(CHILD_ENV, "1")
        .env(SESSION_ENV, session)
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| trf!("cannot_start_3", exe = exe.display()))?;
    let stdin = child.stdin.take().context(tr!("stdin_child_process"))?;
    Ok(Running {
        child,
        stdin,
        exe: exe.to_path_buf(),
    })
}

/// Asks the child to save its session and quit; kills it if it does not answer in time.
fn stop(mut running: Running) {
    let asked = writeln!(running.stdin, "{RELOAD}").and_then(|()| running.stdin.flush());
    let deadline = Instant::now() + Duration::from_secs(5);
    while asked.is_ok() && Instant::now() < deadline {
        if let Ok(Some(_)) = running.child.try_wait() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    if !matches!(running.child.try_wait(), Ok(Some(_))) {
        eprintln!("[dev] {}", tr!("ide_does_not_answer_forced"));
        let _ = running.child.kill();
        let _ = running.child.wait();
    }
    // Remove the copy that just stopped; the original cargo build output is never deleted.
    if running.exe.starts_with(hot_dir().join("run")) {
        let _ = fs::remove_file(&running.exe);
    }
}

pub fn supervise() -> Result<()> {
    let dir = crate_dir();
    let run_dir = hot_dir().join("run");
    fs::create_dir_all(&run_dir)?;
    for old in fs::read_dir(&run_dir)?.flatten() {
        let _ = fs::remove_file(old.path());
    }
    let session = hot_dir().join("session.json");
    let _ = fs::remove_file(&session);
    let args: Vec<OsString> = std::env::args_os().skip(1).filter(|a| a != FLAG).collect();

    println!(
        "[dev] {}",
        trf!("hot_reload_watching", dir = dir.join("src").display())
    );
    // The first child is the binary cargo just built: no extra build at startup.
    let mut running = spawn(&std::env::current_exe()?, &args, &session)?;
    let mut seen = snapshot(&dir);
    let mut generation = 0u32;
    loop {
        thread::sleep(Duration::from_millis(300));
        if let Some(status) = running.child.try_wait()? {
            println!("[dev] {}", trf!("ide_closed_end_dev_mode", status = status));
            return Ok(());
        }
        let now = snapshot(&dir);
        if now == seen {
            continue;
        }
        // Let editors finish writing (format on save, several files saved at once).
        thread::sleep(Duration::from_millis(250));
        seen = snapshot(&dir);
        println!("[dev] {}", tr!("change_detected_building"));
        let started = Instant::now();
        let built = match build(&dir) {
            Ok(exe) => exe,
            Err(e) => {
                eprintln!(
                    "[dev] {}",
                    trf!("current_ide_keeps_running", e = format!("{:#}", e))
                );
                continue;
            }
        };
        generation += 1;
        let copy = run_dir.join(format!(
            "atelier-ide-{generation}{}",
            std::env::consts::EXE_SUFFIX
        ));
        if let Err(e) = fs::copy(&built, &copy) {
            eprintln!(
                "[dev] {}",
                trf!("cannot_copy", built = built.display(), e = e)
            );
            continue;
        }
        stop(running);
        running = spawn(&copy, &args, &session)?;
        println!(
            "[dev] {}",
            trf!(
                "reloaded_s_generation",
                started = format!("{:.1}", started.elapsed().as_secs_f32()),
                generation = generation
            )
        );
    }
}
