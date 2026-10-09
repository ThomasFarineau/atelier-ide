use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
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
                shells.push(Self::new("PowerShell 7", "pwsh.exe", &["-NoLogo", "-NoExit"]));
            }
            shells.push(Self::new("Invite de commandes", "cmd.exe", &[]));
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
            for (name, path) in [("bash", "/bin/bash"), ("zsh", "/bin/zsh"), ("sh", "/bin/sh")] {
                if Path::new(path).is_file() && shells.iter().all(|s| s.program != path && s.name != name) {
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
    let Ok(output) = std::process::Command::new("wsl.exe").args(["-l", "-q"]).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let units: Vec<u16> = output
        .stdout
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    String::from_utf16_lossy(&units)
        .lines()
        .map(|l| l.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}' || c == '\0').to_string())
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
}

impl Terminal {
    pub fn start(root: &Path, shell: &Shell) -> anyhow::Result<Self> {
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
                    cmd.cwd(std::env::var_os("USERPROFILE").map_or_else(std::env::temp_dir, Into::into));
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
            cmd.env("WSLENV", if existing.is_empty() { shared.join(":") } else { format!("{existing}:{}", shared.join(":")) });
        }
        for (name, value) in mcp {
            cmd.env(name, value);
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
        })
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
        self.parser.screen_mut().set_scrollback((current + lines).max(0) as usize);
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
mod tests {
    #[test]
    fn the_screen_scrolls_back_through_the_history() {
        let mut parser = vt100::Parser::new(10, 40, 1000);
        for i in 0..100 {
            parser.process(format!("ligne {i}\r\n").as_bytes());
        }
        let bottom = parser.screen().contents();
        parser.screen_mut().set_scrollback(30);
        assert_eq!(parser.screen().scrollback(), 30);
        assert_ne!(parser.screen().contents(), bottom);
        assert!(parser.screen().contents().contains("ligne 6"));
        parser.screen_mut().set_scrollback(usize::MAX);
        assert!(parser.screen().scrollback() >= 90);
    }

    use super::*;
    #[test]
    fn ansi_screen_handles_carriage_return_and_escape_sequences() {
        let mut parser = vt100::Parser::new(24, 80, 100);
        parser.process(b"old\r\x1b[2K\x1b[32mready\x1b[0m\r\n");
        assert_eq!(parser.screen().contents(), "ready");
    }
    #[test]
    fn available_shells_start_with_the_default_one() {
        let shells = Shell::available();
        assert_eq!(shells[0], Shell::default_shell());
        assert!(shells.iter().all(|s| !s.name.is_empty() && !s.program.is_empty()));
    }
    #[test]
    fn real_shell_executes_and_keeps_its_working_directory() {
        let root = std::env::current_dir().unwrap();
        let mut terminal = Terminal::start(&root, &Shell::default_shell()).expect("A native shell is required");
        let command = if cfg!(windows) {
            "Write-Output ('ATELIER_' + 'PTY_OK'); Write-Output ((Get-Location).Path)"
        } else {
            "printf 'ATELIER_%s\\n' PTY_OK; pwd"
        };
        // The shell can take a while to start when the machine is busy: wait for its prompt.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while terminal.screen().trim().is_empty() {
            terminal.poll();
            assert!(std::time::Instant::now() < deadline, "The shell did not start");
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        terminal.command(command).unwrap();
        loop {
            terminal.poll();
            let screen = terminal.screen();
            if screen.contains("ATELIER_PTY_OK") && screen.contains(&root.display().to_string()) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Shell did not produce the expected output: {screen}"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }

    /// A project of a WSL distribution opens a shell of that distribution in the project folder,
    /// whatever the form of the Windows path. Skipped on machines without WSL.
    #[test]
    fn wsl_terminal_starts_in_the_project_folder() {
        let Some(shell) = Shell::available().into_iter().find(is_wsl) else {
            return;
        };
        let distro = shell.args[1].clone();
        let folder = crate::wsl::to_windows(&distro, "/tmp");
        if !folder.is_dir() {
            return;
        }
        let verbatim = std::path::PathBuf::from(format!(r"\\?\UNC\{}", &folder.to_string_lossy()[2..]));
        for root in [folder, verbatim] {
            let mut terminal = Terminal::start(&root, &shell).expect("wsl.exe starts");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while terminal.screen().trim().is_empty() {
                terminal.poll();
                assert!(std::time::Instant::now() < deadline, "The shell did not start");
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            terminal.command("echo CWD_IS:$(pwd)").unwrap();
            loop {
                terminal.poll();
                let screen = terminal.screen();
                if screen.contains("CWD_IS:/tmp") {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "{root:?} did not give /tmp: {screen}"
                );
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
    }
}
