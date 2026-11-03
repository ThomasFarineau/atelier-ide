use super::*;

#[test]
fn filters_are_read_and_written_back_as_they_were() {
    let value = "blur(4px) brightness(120%) drop-shadow(0 0 2px rgb(0 0 0)) hue-rotate(0.5turn)";
    let effects = Effects::parse(value, EffectKind::Filter).unwrap();
    assert_eq!(effects.functions.len(), 4);
    // What the editor does not change is kept whole.
    assert_eq!(
        effects.functions[2].raw.as_deref(),
        Some("drop-shadow(0 0 2px rgb(0 0 0))")
    );
    assert_eq!(effects.functions[3].args[0].degrees(), 180.0);
    assert_eq!(effects.write(), value);
    assert!(Effects::parse("none", EffectKind::Filter).is_none());
    assert!(Effects::parse("url(#f)", EffectKind::Filter).is_none());
}

#[test]
fn a_filter_changes_colors_as_browsers_do() {
    let gray = Effects::parse("grayscale(1)", EffectKind::Filter).unwrap();
    let [r, g, b, _] = gray.filter([255, 0, 0, 255]);
    assert_eq!((r, g, b), (54, 54, 54));
    let invert = Effects::parse("invert(100%)", EffectKind::Filter).unwrap();
    assert_eq!(invert.filter([255, 0, 0, 255]), [0, 255, 255, 255]);
    let faded = Effects::parse("opacity(0.5) brightness(0.5)", EffectKind::Filter).unwrap();
    assert_eq!(faded.filter([200, 100, 0, 255]), [100, 50, 0, 128]);
    let turned = Effects::parse("hue-rotate(0deg)", EffectKind::Filter).unwrap();
    assert_eq!(turned.filter([10, 200, 30, 255]), [10, 200, 30, 255]);
}

#[test]
fn transforms_move_points_from_the_last_function_to_the_first() {
    let effects = Effects::parse(
        "translate(10px, 0) rotate(90deg) scale(2)",
        EffectKind::Transform,
    )
    .unwrap();
    // (1, 0) doubled, turned to (0, 2), then moved by 10 across.
    let (x, y) = effects.transform(1.0, 0.0);
    assert!((x - 10.0).abs() < 1e-4 && (y - 2.0).abs() < 1e-4, "{x} {y}");
    let skewed = Effects::parse("skewX(45deg)", EffectKind::Transform).unwrap();
    let (x, _) = skewed.transform(0.0, 10.0);
    assert!((x - 10.0).abs() < 1e-3);
}

#[test]
fn functions_are_added_and_the_last_one_taken_gives_none() {
    let mut effects = Effects::parse("rotate(45deg)", EffectKind::Transform).unwrap();
    effects.add(spec(EffectKind::Transform, "translate").unwrap());
    assert_eq!(effects.write(), "rotate(45deg) translate(10px, 0px)");
    effects.functions.clear();
    assert_eq!(effects.write(), "none");
}

#[test]
fn filters_and_transforms_are_found_but_not_their_neighbors() {
    let text = "a { filter: blur(2px); backdrop-filter: saturate(2); text-transform: uppercase; \
                transform: rotate(5deg); transform-origin: center; }\nlist.filter(x => x);";
    let found: Vec<&str> = find(text).iter().map(|(r, _)| &text[r.clone()]).collect();
    assert_eq!(found, ["blur(2px)", "saturate(2)", "rotate(5deg)"]);
}
