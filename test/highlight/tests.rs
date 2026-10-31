use super::*;

fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn color_at(h: &mut Incremental, line: usize, text: &str, needle: &str) -> Color32 {
    let start = text.find(needle).unwrap();
    h.line(line, text)
        .unwrap()
        .spans
        .iter()
        .find(|(r, _)| r.contains(&start))
        .unwrap()
        .1
}

#[test]
fn keywords_strings_and_comments_use_intellij_colors() {
    let lines = ["fn main() {", "    let s = \"x\"; // c", "}"];
    let mut h = Incremental::new("rs", true);
    assert!(h.advance(
        |i| lines.get(i).map(|l| l.to_string()),
        3,
        Duration::from_secs(5)
    ));
    assert_eq!(color_at(&mut h, 0, lines[0], "fn"), rgb(DARK.keyword));
    assert_eq!(color_at(&mut h, 0, lines[0], "main"), rgb(DARK.function));
    assert_eq!(color_at(&mut h, 1, lines[1], "\"x\""), rgb(DARK.string));
    assert_eq!(color_at(&mut h, 1, lines[1], "// c"), rgb(DARK.comment));
}

#[test]
fn edits_recompute_from_the_edited_line_only() {
    let mut lines: Vec<String> = (0..200).map(|i| format!("let a{i} = {i};")).collect();
    let mut h = Incremental::new("rs", true);
    let snapshot = |lines: &Vec<String>| {
        let lines = lines.clone();
        move |i: usize| lines.get(i).cloned()
    };
    assert!(h.advance(snapshot(&lines), 200, Duration::from_secs(5)));
    // Opening a block comment on line 100 changes the colors of the lines after it.
    lines[100] = "/* début".into();
    h.invalidate_from(100);
    assert_eq!(
        h.lines.len(),
        96,
        "restarts at the checkpoint before the edit"
    );
    assert!(h.advance(snapshot(&lines), 200, Duration::from_secs(5)));
    let line = lines[150].clone();
    let colors: Vec<_> = h
        .line(150, &line)
        .unwrap()
        .spans
        .iter()
        .map(|s| s.1)
        .collect();
    assert!(colors.iter().all(|&c| c == rgb(DARK.comment)));
    // A line changed behind the highlighter's back is detected.
    assert!(h.line(10, "autre texte").is_none());
    assert_eq!(h.lines.len(), 0);
}

#[test]
fn work_stops_at_the_time_budget() {
    let mut h = Incremental::new("rs", true);
    let lines = |i: usize| (i < 100_000).then(|| format!("let x{i} = \"{i}\";"));
    assert!(!h.advance(lines, 100_000, Duration::from_millis(5)));
    assert!(h.lines.len() < 100_000);
    assert!(Incremental::new("txt", true).advance(|_| None, 10, Duration::ZERO));
}

/// The color of the first `needle` of the line, the text color where nothing colors it.
fn shown_at(h: &mut Incremental, line: usize, text: &str, needle: &str) -> Color32 {
    let start = text.find(needle).unwrap();
    h.line(line, text)
        .unwrap()
        .spans
        .iter()
        .find(|(r, _)| r.contains(&start))
        .map_or(rgb(DARK.text), |(_, c)| *c)
}

#[test]
fn the_code_of_a_template_string_is_colored_as_code() {
    let line = r#"log(`${project.name} : ${project.languages.join(", ")}`);"#;
    for language in ["js", "ts"] {
        let mut h = Incremental::new(language, true);
        assert!(h.advance(
            |i| (i == 0).then(|| line.to_string()),
            1,
            Duration::from_secs(5)
        ));
        // The text of the template is a string, the code between `${` and `}` is not.
        assert_eq!(
            shown_at(&mut h, 0, line, " : "),
            rgb(DARK.string),
            "{language}"
        );
        assert_eq!(
            shown_at(&mut h, 0, line, "project"),
            rgb(DARK.text),
            "{language}"
        );
        assert_eq!(
            shown_at(&mut h, 0, line, "join"),
            rgb(DARK.function),
            "{language}"
        );
        assert_eq!(
            shown_at(&mut h, 0, line, "\", \""),
            rgb(DARK.string),
            "{language}"
        );
    }
}
