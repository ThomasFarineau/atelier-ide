use super::*;

#[test]
fn corners_are_read_in_every_form_and_written_in_the_shortest() {
    let radius = Radius::parse("8px 4px").unwrap();
    assert_eq!(radius.corners[2].write(), "8px");
    assert_eq!(radius.corners[3].write(), "4px");
    assert_eq!(radius.write(), "8px 4px");
    assert_eq!(Radius::parse("1px 2px 3px").unwrap().write(), "1px 2px 3px");
    assert_eq!(Radius::parse("5px 5px 5px 5px").unwrap().write(), "5px");
    assert_eq!(Radius::parse("50%").unwrap().write(), "50%");
    // Elliptic corners and other values are not for the editor.
    assert!(Radius::parse("10px / 20px").is_none());
    assert!(Radius::parse("var(--r)").is_none());
}

#[test]
fn a_changed_corner_keeps_its_unit() {
    let mut radius = Radius::parse("0.5rem").unwrap();
    radius.corners[1].set(1.0);
    assert_eq!(radius.write(), "0.5rem 1rem 0.5rem 0.5rem");
    assert_eq!(radius.pixels(0, 100.0, 50.0), 8.0);
    assert_eq!(Radius::parse("50%").unwrap().pixels(0, 100.0, 50.0), 25.0);
}

#[test]
fn radii_are_found_in_sheets_and_scripts() {
    let text = "a { border-radius: 4px; border-top-left-radius: 2px; }\nconst s = { borderRadius: '8px 2px' };";
    let found: Vec<&str> = find(text).iter().map(|(r, _)| &text[r.clone()]).collect();
    assert_eq!(found, ["4px", "8px 2px"]);
}
