use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    path::Path,
    sync::mpsc::{self, Receiver},
    thread,
};

pub struct Terminal {
    master: Box<dyn MasterPty + Send>,
    child: Option<Box<dyn Child + Send + Sync>>,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Vec<u8>>,
    parser: vt100::Parser,
    pub input: String,
    pub raw: bool,
    pub exited: bool,
    size: (u16, u16),
    query_tail: Vec<u8>,
}

impl Terminal {
    pub fn start(root: &Path) -> anyhow::Result<Self> {
        let pair = native_pty_system().openpty(PtySize {
            rows: 24,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut cmd = if cfg!(windows) {
            let mut cmd = CommandBuilder::new("powershell.exe");
            cmd.args(["-NoLogo", "-NoProfile", "-NoExit"]);
            cmd
        } else {
            CommandBuilder::new("/bin/sh")
        };
        cmd.cwd(root);
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        // Bound buffering when a process prints faster than the UI can draw.
        let (tx, rx) = mpsc::sync_channel(64);
        thread::spawn(move || {
            let mut buffer = [0; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buffer[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Ok(Self {
            master: pair.master,
            child: Some(child),
            writer,
            rx,
            parser: vt100::Parser::new(24, 120, 2000),
            input: String::new(),
            raw: false,
            exited: false,
            size: (24, 120),
            query_tail: Vec::new(),
        })
    }
    pub fn poll(&mut self) {
        for _ in 0..64 {
            let Ok(bytes) = self.rx.try_recv() else { break };
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
    }
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
                self.parser.set_size(rows, cols);
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
    use super::*;
    #[test]
    fn ansi_screen_handles_carriage_return_and_escape_sequences() {
        let mut parser = vt100::Parser::new(24, 80, 100);
        parser.process(b"old\r\x1b[2K\x1b[32mready\x1b[0m\r\n");
        assert_eq!(parser.screen().contents(), "ready");
    }
    #[test]
    fn real_shell_executes_and_keeps_its_working_directory() {
        let root = std::env::current_dir().unwrap();
        let mut terminal = Terminal::start(&root).expect("A native shell is required");
        let command = if cfg!(windows) {
            "Write-Output ('FORGE_' + 'PTY_OK'); Write-Output ((Get-Location).Path)"
        } else {
            "printf 'FORGE_%s\\n' PTY_OK; pwd"
        };
        terminal.command(command).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            terminal.poll();
            let screen = terminal.screen();
            if screen.contains("FORGE_PTY_OK") && screen.contains(&root.display().to_string()) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Shell did not produce the expected output: {screen}"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
}
