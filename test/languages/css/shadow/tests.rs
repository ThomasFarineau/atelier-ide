use super::*;

#[test]
fn shadows_are_read_and_written_back_as_they_were() {
    let value = "0 4px 12px rgba(0, 0, 0, 0.25), inset 0 0 0 1px #fff";
    let shadows = Shadows::parse(value, false).unwrap();
    assert_eq!(shadows.list.len(), 2);
    let first = &shadows.list[0];
    assert_eq!(first.y.value, 4.0);
    assert_eq!(first.blur.as_ref().unwrap().value, 12.0);
    assert_eq!(first.rgba()[3], 64);
    assert!(shadows.list[1].inset);
    assert_eq!(shadows.list[1].spread.as_ref().unwrap().write(), "1px");
    assert_eq!(shadows.write(), value);
    // The color first stays first.
    let shadows = Shadows::parse("red 2px 2px", true).unwrap();
    assert_eq!(shadows.write(), "red 2px 2px");
}

#[test]
fn what_is_not_a_shadow_is_left_out() {
    assert!(Shadows::parse("none", false).is_none());
    assert!(Shadows::parse("var(--shadow)", false).is_none());
    assert!(Shadows::parse("2px", false).is_none());
    // A text shadow has no spread and no inset.
    assert!(Shadows::parse("1px 1px 2px 3px red", true).is_none());
    assert!(Shadows::parse("inset 1px 1px red", true).is_none());
}

#[test]
fn shadows_are_found_in_sheets_attributes_and_scripts() {
    let text = "a { box-shadow: 0 1px 2px #000 !important; }\n\
                <p style=\"text-shadow: 1px 1px red\">\n\
                const s = { boxShadow: '0 0 4px blue' };\n\
                .x { --my-box-shadow-size: 2px; }";
    let found: Vec<&str> = find(text).iter().map(|(r, _)| &text[r.clone()]).collect();
    assert_eq!(found, ["0 1px 2px #000", "1px 1px red", "0 0 4px blue"]);
}

#[test]
fn an_edited_length_keeps_its_unit() {
    let mut shadows = Shadows::parse("0 0.5em 1em black", false).unwrap();
    shadows.list[0].y.set(1.25);
    shadows.list[0].x.set(3.0);
    assert_eq!(shadows.write(), "3px 1.25em 1em black");
    shadows.list.push(Shadow::new());
    assert_eq!(
        shadows.write(),
        "3px 1.25em 1em black, 0 4px 12px rgba(0, 0, 0, 0.25)"
    );
}
