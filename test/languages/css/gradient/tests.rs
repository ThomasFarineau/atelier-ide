use super::*;

#[test]
fn a_gradient_is_read_and_written_back_as_it_was() {
    let text = "linear-gradient(to right, #e34f26 10%, rgba(0, 0, 0, 0.5) 50%, transparent)";
    let gradient = parse(text).unwrap();
    assert_eq!(gradient.kind, Kind::Linear);
    assert_eq!(gradient.head.as_deref(), Some("to right"));
    assert_eq!(gradient.stops.len(), 3);
    assert_eq!(gradient.stops[0].color, [0xe3, 0x4f, 0x26, 255]);
    assert_eq!(gradient.stops[1].color[3], 128);
    assert_eq!(gradient.stops[2].color, [0, 0, 0, 0]);
    assert_eq!(gradient.write(), text);
}

#[test]
fn every_kind_and_the_repeating_ones_are_read() {
    let radial = parse("radial-gradient(circle at top, red, blue)").unwrap();
    assert_eq!(radial.kind, Kind::Radial);
    assert!(radial.circle());
    let conic = parse("repeating-conic-gradient(from 45deg, red 0 10%, blue 10% 20%)").unwrap();
    assert!(conic.repeating);
    assert_eq!(conic.angle(), Some(45.0));
    assert_eq!(conic.stops[0].second.as_deref(), Some("10%"));
    // Stops without a head, positions in pixels, colors with spaces.
    let plain = parse("linear-gradient(rgb(0 0 0 / 50%) 20px, white)").unwrap();
    assert_eq!(plain.head, None);
    assert_eq!(
        plain.stops[0].position,
        Some(Position::Other("20px".into()))
    );
}

#[test]
fn what_the_editor_cannot_show_is_left_out() {
    assert!(parse("linear-gradient(var(--a), red)").is_none());
    assert!(parse("linear-gradient(currentColor, red)").is_none());
    assert!(parse("linear-gradient(red)").is_none());
    assert!(parse("drop-shadow(red)").is_none());
}

#[test]
fn gradients_are_found_in_any_text() {
    let text = "a { background: linear-gradient(90deg, red, blue), url(x.png); }\n\
                const s = `repeating-radial-gradient(red 0 5px, blue 5px 10px)`;";
    let found = find(text);
    assert_eq!(found.len(), 2);
    assert_eq!(
        &text[found[0].0.clone()],
        "linear-gradient(90deg, red, blue)"
    );
    assert!(text[found[1].0.clone()].starts_with("repeating-radial"));
}

#[test]
fn stops_without_positions_are_spread_and_colors_mixed() {
    let gradient = parse("linear-gradient(red, lime 80%, blue, white)").unwrap();
    let positions = gradient.positions();
    assert_eq!(positions[0], 0.0);
    assert_eq!(positions[1], 0.8);
    assert!((positions[2] - 0.9).abs() < 1e-5);
    assert_eq!(positions[3], 1.0);
    assert_eq!(gradient.sample(0.0), [255, 0, 0, 255]);
    assert_eq!(gradient.sample(0.4), [128, 128, 0, 255]);
    // Toward transparent, the color stays and only its opacity goes.
    let fading = parse("linear-gradient(red, transparent)").unwrap();
    let half = fading.sample(0.5);
    assert_eq!(&half[..3], &[255, 0, 0]);
    assert_eq!(half[3], 128);
}

#[test]
fn the_direction_is_read_in_every_unit_and_written_in_degrees() {
    for (head, angle) in [
        ("to right", 90.0),
        ("to bottom left", 225.0),
        ("0.25turn", 90.0),
        ("45deg", 45.0),
    ] {
        let gradient = parse(&format!("linear-gradient({head}, red, blue)")).unwrap();
        assert_eq!(gradient.angle(), Some(angle), "{head}");
    }
    let mut gradient = parse("linear-gradient(red, blue)").unwrap();
    assert_eq!(gradient.angle(), Some(180.0));
    gradient.set_angle(-30.0);
    assert_eq!(gradient.write(), "linear-gradient(330deg, red, blue)");
    // A linear gradient to the right is red on the left, blue on the right.
    let right = parse("linear-gradient(to right, red, blue)").unwrap();
    assert_eq!(right.color_at(0.0, 5.0, 100.0, 10.0)[0], 255);
    assert_eq!(right.color_at(100.0, 5.0, 100.0, 10.0)[2], 255);
}

#[test]
fn an_edited_stop_is_written_in_its_notation() {
    let mut gradient = parse("linear-gradient(red, blue 50%)").unwrap();
    gradient.stops[1].set_color([0, 128, 0, 255], Notation::Hex);
    gradient
        .stops
        .push(Stop::new([255, 255, 255, 255], Notation::Rgb, 100.0));
    gradient.set_circle(true);
    assert_eq!(
        gradient.write(),
        "linear-gradient(red, #008000 50%, rgb(255, 255, 255) 100%)"
    );
    let mut radial = parse("radial-gradient(red, blue)").unwrap();
    radial.set_circle(true);
    assert_eq!(radial.write(), "radial-gradient(circle, red, blue)");
}
