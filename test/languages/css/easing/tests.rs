use super::*;

#[test]
fn curves_and_keywords_are_read() {
    assert_eq!(
        Easing::parse("cubic-bezier(0.1, 0.7, 1.0, 0.1)"),
        Some(Easing::new(0.1, 0.7, 1.0, 0.1))
    );
    assert_eq!(Easing::parse("ease-in-out"), Some(PRESETS[3].1));
    // The times must stay between 0 and 1; the progress may go past.
    assert!(Easing::parse("cubic-bezier(1.5, 0, 0, 1)").is_none());
    assert!(Easing::parse("cubic-bezier(0.3, -0.6, 0.7, 1.6)").is_some());
}

#[test]
fn a_curve_is_written_as_its_keyword_or_its_numbers() {
    assert_eq!(Easing::new(0.42, 0.0, 1.0, 1.0).keyword(), Some("ease-in"));
    assert_eq!(Easing::new(0.5, 0.0, 1.0, 1.0).keyword(), None);
    assert_eq!(
        Easing::new(0.5, -0.25, 0.1, 1.0).write(),
        "cubic-bezier(0.5, -0.25, 0.1, 1)"
    );
}

#[test]
fn the_progress_follows_the_curve() {
    let linear = PRESETS[4].1;
    assert!((linear.progress(0.3) - 0.3).abs() < 0.01);
    let ease_in = PRESETS[1].1;
    assert!(ease_in.progress(0.5) < 0.4);
    let ease_out = PRESETS[2].1;
    assert!(ease_out.progress(0.5) > 0.6);
}

#[test]
fn curves_are_found_in_functions_and_timing_properties() {
    let text = "a { transition: opacity 0.3s ease-out, color 1s cubic-bezier(0.2, 0, 0, 1); }\n\
                b { animation-timing-function: linear; transition-delay: 1s; }\n\
                p { content: \"ease\"; }";
    let found: Vec<&str> = find(text).iter().map(|(r, _)| &text[r.clone()]).collect();
    assert_eq!(found, ["ease-out", "cubic-bezier(0.2, 0, 0, 1)", "linear"]);
}
