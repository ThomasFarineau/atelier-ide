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
    assert!(
        shells
            .iter()
            .all(|s| !s.name.is_empty() && !s.program.is_empty())
    );
}
#[test]
fn real_shell_executes_and_keeps_its_working_directory() {
    let root = std::env::current_dir().unwrap();
    let mut terminal =
        Terminal::start(&root, &Shell::default_shell(), &[]).expect("A native shell is required");
    let command = if cfg!(windows) {
        "Write-Output ('ATELIER_' + 'PTY_OK'); Write-Output ((Get-Location).Path)"
    } else {
        "printf 'ATELIER_%s\\n' PTY_OK; pwd"
    };
    // The shell can take a while to start when the machine is busy: wait for its prompt.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while terminal.screen().trim().is_empty() {
        terminal.poll();
        assert!(
            std::time::Instant::now() < deadline,
            "The shell did not start"
        );
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
        let mut terminal = Terminal::start(&root, &shell, &[]).expect("wsl.exe starts");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while terminal.screen().trim().is_empty() {
            terminal.poll();
            assert!(
                std::time::Instant::now() < deadline,
                "The shell did not start"
            );
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
