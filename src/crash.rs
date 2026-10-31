//! What the IDE leaves behind when it stops by accident.
//!
//! A program without a console (the IDE on Windows) has nowhere to write: a panic, a lack of
//! memory or an overflowed stack used to close it without a word. Each process now writes what
//! goes to the error output into a log of its own, `logs/<pid>.log` in the data folder, and
//! takes it away when it quits normally. A log left behind by a process that is gone and that
//! tells of a crash goes into `logs/crash.log`, and the next window says so.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

/// What the error output of a process says when it crashed.
const MARKERS: [&str; 4] = [
    "panicked at",
    "memory allocation of",
    "has overflowed its stack",
    "fatal runtime error",
];

/// Past this size, the crash log starts again.
const MAX_CRASH_LOG: u64 = 2 * 1024 * 1024;

/// The log of this process, and the file the error output goes to.
static LOG: Mutex<Option<(PathBuf, fs::File)>> = Mutex::new(None);

/// What the processes that crashed since the last start left, for the first window to say.
static PREVIOUS: OnceLock<Option<String>> = OnceLock::new();

pub fn logs_dir() -> PathBuf {
    crate::cli::data_dir().join("logs")
}

pub fn crash_log() -> PathBuf {
    logs_dir().join("crash.log")
}

/// Sends the error output into the log of this process, gathers the logs of the processes
/// that crashed, and adds the trace of the calls to the message of a panic.
pub fn install() {
    let dir = logs_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = PREVIOUS.set(collect(&dir, &crash_log()));
    redirect(&dir);
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let backtrace = std::backtrace::Backtrace::force_capture();
        eprintln!("{backtrace}");
        // Without a log to hold it, a panic of the interface is written down here at once.
        let main = std::thread::current().name() == Some("main");
        if main && LOG.lock().map_or(true, |log| log.is_none()) {
            append(
                &crash_log(),
                &format!(
                    "{} [pid {}] {info}\n{backtrace}\n",
                    stamp(),
                    std::process::id()
                ),
            );
        }
    }));
}

/// The crash of an earlier run, in a line, once.
pub fn previous() -> Option<String> {
    PREVIOUS.get().cloned().flatten()
}

/// The IDE quits normally: its log goes away.
pub fn clean_exit() {
    let Ok(mut log) = LOG.lock() else { return };
    if let Some((path, file)) = log.take() {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Console::{STD_ERROR_HANDLE, SetStdHandle};
            SetStdHandle(STD_ERROR_HANDLE, std::ptr::null_mut());
        }
        drop(file);
        let _ = fs::remove_file(path);
    }
}

fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // UTC, without a dependency for dates.
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// The date of a number of days since 1970 (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn append(path: &Path, text: &str) {
    if fs::metadata(path).is_ok_and(|m| m.len() > MAX_CRASH_LOG) {
        let _ = fs::rename(path, path.with_extension("old.log"));
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(text.as_bytes());
    }
}

/// Whether the error output of a process tells of a crash, and what in a line.
fn crash_line(text: &str) -> Option<&str> {
    text.lines()
        .find(|line| MARKERS.iter().any(|m| line.contains(m)))
        .map(str::trim)
}

/// The logs of the processes that are gone: the ones that tell of a crash go into the crash
/// log. The log of a running process cannot be removed, and stays.
fn collect(dir: &Path, crash_log: &Path) -> Option<String> {
    let mine = format!("{}.log", std::process::id());
    let mut last = None;
    for file in fs::read_dir(dir).ok()?.flatten() {
        let path = file.path();
        let name = file.file_name().to_string_lossy().into_owned();
        let pid_log = name
            .strip_suffix(".log")
            .is_some_and(|stem| !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_digit()));
        if !pid_log || name == mine {
            continue;
        }
        let text = fs::read(&path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        if fs::remove_file(&path).is_err() {
            continue;
        }
        if let Some(line) = crash_line(&text) {
            append(
                crash_log,
                &format!(
                    "---- process {}, found {}\n{text}\n",
                    name.trim_end_matches(".log"),
                    stamp(),
                ),
            );
            last = Some(line.to_string());
        }
    }
    last
}

/// The error output into `logs/<pid>.log`, when it goes nowhere (no console).
fn redirect(dir: &Path) {
    #[cfg(windows)]
    unsafe {
        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
        use windows_sys::Win32::{
            Foundation::INVALID_HANDLE_VALUE,
            System::Console::{GetStdHandle, STD_ERROR_HANDLE, SetStdHandle},
        };
        let current = GetStdHandle(STD_ERROR_HANDLE);
        if !current.is_null() && current != INVALID_HANDLE_VALUE {
            return;
        }
        const FILE_SHARE_READ: u32 = 1;
        let path = dir.join(format!("{}.log", std::process::id()));
        // Shared for reading only: while this process runs, nobody removes it.
        let Ok(file) = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
        else {
            return;
        };
        if SetStdHandle(STD_ERROR_HANDLE, file.as_raw_handle() as _) != 0
            && let Ok(mut log) = LOG.lock()
        {
            *log = Some((path, file));
        }
    }
    #[cfg(not(windows))]
    let _ = dir;
}

#[cfg(test)]
#[path = "../test/crash/tests.rs"]
mod tests;
