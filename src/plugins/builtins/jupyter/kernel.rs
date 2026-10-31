//! A Jupyter kernel of a notebook: `kernel.py` runs it with the Python of the project
//! (`jupyter_client` and `ipykernel`) and talks JSON lines with the IDE.

use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver},
    },
    thread,
    time::Duration,
};

const BRIDGE: &str = include_str!(r"kernel.py");

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Ready(String),
    /// `jupyter_client` or `ipykernel` is not installed.
    Missing,
    Failed(String),
    Output {
        cell: u64,
        kind: String,
        content: Value,
    },
    Done {
        cell: u64,
        count: Option<u64>,
    },
}

/// What the event of a line of the bridge is.
pub fn parse_event(line: &str) -> Option<Event> {
    let value: Value = serde_json::from_str(line).ok()?;
    Some(match value["type"].as_str()? {
        "ready" => Event::Ready(value["version"].as_str().unwrap_or_default().into()),
        "missing" => Event::Missing,
        "failed" => Event::Failed(value["error"].as_str().unwrap_or_default().into()),
        "output" => Event::Output {
            cell: value["cell"].as_u64()?,
            kind: value["kind"].as_str()?.into(),
            content: value["content"].clone(),
        },
        "done" => Event::Done {
            cell: value["cell"].as_u64()?,
            count: value["count"].as_u64(),
        },
        _ => return None,
    })
}

pub struct Kernel {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    events: Receiver<Event>,
}

impl Kernel {
    /// Starts a kernel in `cwd`. `wake` is called when an event comes.
    pub fn start(
        python: &Path,
        cwd: &Path,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        let mut command = Command::new(python);
        command
            .args(["-u", "-c", BRIDGE])
            .arg(cwd)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PYTHONIOENCODING", "utf-8");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let (tx, events) = mpsc::channel();
        // The end of what the bridge wrote on its error output, to explain a failure.
        let errors = Arc::new(Mutex::new(String::new()));
        let tail = errors.clone();
        thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            let start = text.len().saturating_sub(4000);
            let start = (start..text.len())
                .find(|i| text.is_char_boundary(*i))
                .unwrap_or(0);
            *tail.lock().unwrap_or_else(|e| e.into_inner()) = text[start..].to_string();
        });
        let failure = errors;
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(event) = parse_event(&line) {
                    if tx.send(event).is_err() {
                        return;
                    }
                    wake();
                }
            }
            // The bridge stopped: let its error output arrive, then say why.
            thread::sleep(Duration::from_millis(200));
            let why = failure.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let _ = tx.send(Event::Failed(why));
            wake();
        });
        Ok(Self {
            child: Some(child),
            stdin,
            events,
        })
    }

    fn send(&mut self, request: Value) {
        if let Some(stdin) = &mut self.stdin {
            let _ = writeln!(stdin, "{request}");
            let _ = stdin.flush();
        }
    }

    /// Runs the code of a cell; its outputs come back as events.
    pub fn execute(&mut self, cell: u64, code: &str) {
        self.send(json!({"op": "execute", "cell": cell, "code": code}));
    }

    pub fn interrupt(&mut self) {
        self.send(json!({"op": "interrupt"}));
    }

    pub fn events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

impl Drop for Kernel {
    /// Closing the input shuts the kernel down; a bridge that does not stop is killed.
    fn drop(&mut self) {
        self.interrupt();
        self.stdin = None;
        let Some(mut child) = self.child.take() else {
            return;
        };
        thread::spawn(move || {
            for _ in 0..50 {
                if child.try_wait().ok().flatten().is_some() {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
            let _ = child.kill();
            let _ = child.wait();
        });
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/jupyter/kernel/tests.rs"]
mod tests;
