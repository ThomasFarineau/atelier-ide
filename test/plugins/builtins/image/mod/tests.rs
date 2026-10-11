use super::*;

#[test]
fn images_are_recognized_and_have_no_layout_switch() {
    let plugin = ImagePlugin::default();
    assert!(plugin.opens_binary(Path::new("a/Photo.JPG")));
    assert!(!plugin.opens_binary(Path::new("logo.svg")));
    assert_eq!(
        plugin.editor_layout(Path::new("x.png")),
        Some(EditorLayout::Preview)
    );
    assert!(!plugin.layout_switch(Path::new("x.png")));
}
