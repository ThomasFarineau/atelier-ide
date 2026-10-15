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

/// A selection made with the mouse starts where the button went down, not where the drag was
/// noticed a few pixels further, so that Ctrl+C copies the whole of it.
#[test]
fn a_mouse_selection_starts_where_the_button_went_down() {
    use eframe::egui::{self, Event, Modifiers, PointerButton, Rect, pos2, vec2};
    let root = std::env::current_dir().unwrap();
    let mut term =
        Terminal::start(&root, &Shell::default_shell(), &[]).expect("A native shell is required");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let command = if cfg!(windows) {
        "Write-Output ('ATELIER_' + 'SELECTED')"
    } else {
        "printf 'ATELIER_%s\n' SELECTED"
    };
    while term.screen().trim().is_empty() {
        term.poll();
        assert!(
            std::time::Instant::now() < deadline,
            "The shell did not start"
        );
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    term.command(command).unwrap();
    // The output, not the command line that also holds the word.
    let output_row = |term: &Terminal| {
        term.screen_ref()
            .contents()
            .lines()
            .position(|l| l.starts_with("ATELIER_SELECTED"))
    };
    while output_row(&term).is_none() {
        term.poll();
        assert!(std::time::Instant::now() < deadline, "No output");
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let row = output_row(&term).unwrap();
    let ctx = egui::Context::default();
    let theme = crate::ui::theme_by_index(0);
    crate::ui::install(&ctx, theme, 14.0);
    let mut selection = crate::terminal_ui::Selection::default();
    let mut time = 0.0;
    let mut frame = |term: &mut Terminal, events: Vec<Event>| {
        time += 0.05;
        let mut rect = Rect::NOTHING;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        rect =
                            crate::terminal_ui::show(ui, term, &mut selection, &theme, &root).rect;
                    });
            },
        );
        (rect, output.platform_output.commands)
    };
    let (rect, _) = frame(&mut term, vec![]);
    let font = egui::FontId::monospace(crate::terminal_ui::FONT_SIZE);
    let (row_h, char_w) =
        ctx.fonts_mut(|f| (f.row_height(&font).ceil(), f.glyph_width(&font, 'M')));
    // From the middle of the first character to the middle of the last one.
    let y = rect.top() + (row as f32 + 0.5) * row_h;
    let from = pos2(rect.left() + char_w * 0.5, y);
    let to = pos2(rect.left() + char_w * 15.5, y);
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    frame(
        &mut term,
        vec![Event::PointerMoved(from), button(from, true)],
    );
    for i in 1..=5 {
        frame(
            &mut term,
            vec![Event::PointerMoved(from + (to - from) * (i as f32 / 5.0))],
        );
    }
    frame(&mut term, vec![button(to, false)]);
    let (_, commands) = frame(&mut term, vec![Event::Copy]);
    assert!(
        commands.contains(&egui::OutputCommand::CopyText("ATELIER_SELECTED".into())),
        "{commands:?}"
    );
}

#[test]
fn the_wheel_goes_to_a_program_that_took_the_screen() {
    use crate::terminal_ui::wheel_bytes;
    let mut parser = vt100::Parser::new(24, 80, 100);
    // A pager: the alternate screen, no mouse, the arrows of the cursor mode.
    parser.process(b"[?1049h");
    assert_eq!(wheel_bytes(parser.screen(), true, 2, (0, 0)), b"[A[A");
    parser.process(b"[?1h");
    assert_eq!(wheel_bytes(parser.screen(), false, 1, (0, 0)), b"OB");
    // A program that asked for the mouse, in SGR: wheel events where the pointer is.
    parser.process(b"[?1000h[?1006h");
    assert_eq!(
        wheel_bytes(parser.screen(), true, 1, (4, 9)),
        b"[<64;10;5M"
    );
    assert_eq!(
        wheel_bytes(parser.screen(), false, 1, (0, 0)),
        b"[<65;1;1M"
    );
    // The first encoding: three bytes, each 32 more than the value.
    parser.process(b"[?1006l");
    assert_eq!(
        wheel_bytes(parser.screen(), true, 1, (0, 0)),
        [0x1b, b'[', b'M', 32 + 64, 33, 33]
    );
}

/// A full-screen AI copies its own selection with OSC 52: the text is kept for the clipboard.
#[test]
fn a_program_copies_to_the_clipboard() {
    let mut parser = vt100::Parser::new_with_callbacks(4, 40, 0, Requests::default());
    parser.process(b"\x1b]52;c;aMOobGxv\x07");
    assert_eq!(
        parser.callbacks_mut().copied.take().as_deref(),
        Some("hèllo")
    );
    // Asking for the clipboard's content is not answered.
    parser.process(b"\x1b]52;c;?\x07");
    assert_eq!(parser.callbacks_mut().copied.take(), None);
}
