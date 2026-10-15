use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
    sync::{
        Arc, OnceLock,
        mpsc::{self, Receiver},
    },
    thread,
};

/// Lines of history kept above the screen.
const SCROLLBACK: usize = 10_000;

/// A program the terminal can run: PowerShell, cmd, Git Bash, a WSL distribution…
#[derive(Clone, PartialEq, Debug)]
pub struct Shell {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

impl Shell {
    fn new(name: &str, program: &str, args: &[&str]) -> Self {
        Self {
            name: name.into(),
            program: program.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    /// A shell inside the WSL distribution `distro`.
    pub fn wsl(distro: &str) -> Self {
        Self::new(&format!("WSL : {distro}"), "wsl.exe", &["-d", distro])
    }

    /// The shell opened by default.
    pub fn default_shell() -> Self {
        if cfg!(windows) {
            Self::new("PowerShell", "powershell.exe", &["-NoLogo", "-NoExit"])
        } else {
            let program = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            let name = Path::new(&program)
                .file_name()
                .map_or("sh".into(), |n| n.to_string_lossy().into_owned());
            Self {
                name,
                program,
                args: Vec::new(),
            }
        }
    }

    /// Shells found on this machine, the default one first.
    pub fn available() -> Vec<Self> {
        let mut shells = vec![Self::default_shell()];
        if cfg!(windows) {
            if on_path("pwsh.exe") {
                shells.push(Self::new(
                    "PowerShell 7",
                    "pwsh.exe",
                    &["-NoLogo", "-NoExit"],
                ));
            }
            shells.push(Self::new(tr!("command_prompt"), "cmd.exe", &[]));
            let git_bash = [
                r"C:\Program Files\Git\bin\bash.exe",
                r"C:\Program Files (x86)\Git\bin\bash.exe",
            ]
            .into_iter()
            .find(|p| Path::new(p).is_file());
            if let Some(bash) = git_bash {
                shells.push(Self::new("Git Bash", bash, &["--login", "-i"]));
            }
            for distro in wsl_distributions() {
                shells.push(Self::wsl(&distro));
            }
        } else {
            for (name, path) in [
                ("bash", "/bin/bash"),
                ("zsh", "/bin/zsh"),
                ("sh", "/bin/sh"),
            ] {
                if Path::new(path).is_file()
                    && shells.iter().all(|s| s.program != path && s.name != name)
                {
                    shells.push(Self::new(name, path, &[]));
                }
            }
        }
        shells
    }
}

pub fn is_wsl(shell: &Shell) -> bool {
    Path::new(&shell.program)
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("wsl.exe"))
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// Installed WSL distributions; `wsl -l -q` prints UTF-16.
fn wsl_distributions() -> Vec<String> {
    let mut command = std::process::Command::new("wsl.exe");
    command.args(["-l", "-q"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(output) = command.output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let units: Vec<u16> = output
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect();
    String::from_utf16_lossy(&units)
        .lines()
        .map(|l| {
            l.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}' || c == '\0')
                .to_string()
        })
        // Docker's internal distributions are not meant to be opened.
        .filter(|l| !l.is_empty() && !l.starts_with("docker-desktop"))
        .collect()
}

pub struct Terminal {
    pub shell: Shell,
    master: Box<dyn MasterPty + Send>,
    child: Option<Box<dyn Child + Send + Sync>>,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Vec<u8>>,
    parser: vt100::Parser,
    /// The terminal has the keyboard: IDE shortcuts are off.
    pub focused: bool,
    /// Wheel distance not yet worth a line: a smooth wheel gives a few pixels per frame.
    pub wheel: f32,
    /// Takes the keyboard at the next frame (new tab or pane).
    pub want_focus: bool,
    pub exited: bool,
    /// Wakes the UI when the shell prints something.
    repaint: Arc<OnceLock<Box<dyn Fn() + Send + Sync>>>,
    size: (u16, u16),
    query_tail: Vec<u8>,
    /// What is typed on the current line, as far as the IDE can tell: None after a key that
    /// changes it in ways only the shell knows (history, completion).
    pub typed: Option<String>,
    /// The IDE offers to connect AIs it knows: the Enter that would start one is held back.
    pub ai_watch: bool,
    /// An AI whose start is held back until the user answers the offer.
    pub pending_ai: Option<crate::plugins::builtins::mcp::agents::Agent>,
    /// A path clicked in the output, to open in the editor: (file, 1-based line and column).
    pub open_file: Option<(std::path::PathBuf, Option<usize>, Option<usize>)>,
}

impl Terminal {
    /// Starts `shell` in `root`; `env` is added to its environment (the runtimes of the
    /// project), except in WSL, whose programs are not those of Windows.
    pub fn start(root: &Path, shell: &Shell, env: &[(String, OsString)]) -> anyhow::Result<Self> {
        let pair = native_pty_system().openpty(PtySize {
            rows: 24,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut cmd = CommandBuilder::new(&shell.program);
        cmd.args(&shell.args);
        if is_wsl(shell) {
            // wsl.exe cannot translate the `\\?\UNC\wsl.localhost\…` current directory of a project
            // opened from WSL: it gets the Linux path through `--cd` instead.
            match crate::wsl::parse(root) {
                Some(wsl) => {
                    let same = shell
                        .args
                        .windows(2)
                        .any(|a| a[0] == "-d" && a[1].eq_ignore_ascii_case(&wsl.distro));
                    cmd.args(["--cd", if same { wsl.linux.as_str() } else { "~" }]);
                    cmd.cwd(
                        std::env::var_os("USERPROFILE").map_or_else(std::env::temp_dir, Into::into),
                    );
                }
                None => cmd.cwd(crate::project::simplify(root)),
            }
        } else {
            cmd.cwd(crate::project::simplify(root));
        }
        cmd.env("TERM", "xterm-256color");
        // Where an AI started here finds the MCP server of the IDE.
        let mcp = crate::plugins::builtins::mcp::env();
        if is_wsl(shell) && !mcp.is_empty() {
            let shared: Vec<&str> = mcp.iter().map(|(name, _)| *name).collect();
            let existing = std::env::var("WSLENV").unwrap_or_default();
            cmd.env(
                "WSLENV",
                if existing.is_empty() {
                    shared.join(":")
                } else {
                    format!("{existing}:{}", shared.join(":"))
                },
            );
        }
        for (name, value) in mcp {
            cmd.env(name, value);
        }
        if !is_wsl(shell) {
            for (name, value) in env {
                cmd.env(name, value);
            }
        }
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        // Bound buffering when a process prints faster than the UI can draw.
        let (tx, rx) = mpsc::sync_channel(64);
        let repaint: Arc<OnceLock<Box<dyn Fn() + Send + Sync>>> = Arc::default();
        let wake = repaint.clone();
        thread::spawn(move || {
            let mut buffer = [0; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buffer[..n].to_vec()).is_err() {
                            break;
                        }
                        if let Some(wake) = wake.get() {
                            wake();
                        }
                    }
                }
            }
        });
        Ok(Self {
            shell: shell.clone(),
            master: pair.master,
            child: Some(child),
            writer,
            rx,
            parser: vt100::Parser::new(24, 120, SCROLLBACK),
            focused: false,
            want_focus: false,
            wheel: 0.0,
            exited: false,
            repaint,
            size: (24, 120),
            query_tail: Vec::new(),
            typed: Some(String::new()),
            ai_watch: false,
            pending_ai: None,
            open_file: None,
        })
    }
    /// The process id of the shell, which the processes started in this terminal descend from.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(|c| c.process_id())
    }
    /// Registers the function that wakes the UI on output; set once.
    pub fn on_output(&self, wake: impl Fn() + Send + Sync + 'static) {
        let _ = self.repaint.set(Box::new(wake));
    }
    pub fn has_wake(&self) -> bool {
        self.repaint.get().is_some()
    }
    pub fn screen_ref(&self) -> &vt100::Screen {
        self.parser.screen()
    }
    /// Scrolls the history: positive goes back in time. The alternate screen has none.
    pub fn scroll(&mut self, lines: isize) {
        let current = self.parser.screen().scrollback() as isize;
        self.parser
            .screen_mut()
            .set_scrollback((current + lines).max(0) as usize);
    }
    pub fn scroll_to_bottom(&mut self) {
        self.parser.screen_mut().set_scrollback(0);
    }
    /// Reads the shell output. True when something arrived.
    pub fn poll(&mut self) -> bool {
        let mut received = false;
        for _ in 0..64 {
            let Ok(bytes) = self.rx.try_recv() else { break };
            received = true;
            self.parser.process(&bytes);
            // ConPTY inherits the cursor and waits for a DSR response at startup.
            // Keep a small streaming tail because escape sequences may span reads.
            for byte in bytes {
                self.query_tail.push(byte);
                if self.query_tail.len() > 8 {
                    self.query_tail.remove(0);
                }
                if self.query_tail.ends_with(b"\x1b[6n") {
                    let (row, col) = self.parser.screen().cursor_position();
                    let reply = format!("\x1b[{};{}R", row + 1, col + 1);
                    let _ = self.write(reply.as_bytes());
                    self.query_tail.clear();
                } else if self.query_tail.ends_with(b"\x1b[5n") {
                    let _ = self.write(b"\x1b[0n");
                    self.query_tail.clear();
                }
            }
        }
        self.exited = self
            .child
            .as_mut()
            .is_none_or(|child| child.try_wait().ok().flatten().is_some());
        received
    }
    #[cfg(test)]
    pub fn screen(&self) -> String {
        self.parser.screen().contents()
    }
    pub fn write(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()?;
        Ok(())
    }
    pub fn command(&mut self, command: &str) -> anyhow::Result<()> {
        self.write(format!("{command}\r").as_bytes())
    }
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let size = (rows.clamp(4, 150), cols.clamp(20, 300));
        if size != self.size {
            let (rows, cols) = size;
            if self
                .master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .is_ok()
            {
                self.parser.screen_mut().set_size(rows, cols);
                self.size = size;
            }
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            // Reap asynchronously: Windows ConPTY shutdown may briefly block.
            thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

#[cfg(test)]
#[path = "../test/terminal/tests.rs"]
mod tests;
