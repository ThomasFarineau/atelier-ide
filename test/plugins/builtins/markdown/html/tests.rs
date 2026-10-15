use super::*;

#[test]
fn tags_of_a_paragraph_become_markdown() {
    let source = "Press <kbd>Ctrl</kbd> and <b>save</b><br>See <a href=\"doc.md\">the doc</a> <img src=\"a.png\" alt=\"A\">\n";
    let converted = inline_to_markdown(source).unwrap();
    assert_eq!(
        converted.text,
        "Press `Ctrl` and **save**\\\nSee [the doc](<doc.md>) ![A](<a.png>)\n"
    );
    assert!(inline_to_markdown("# Plain *Markdown*\n").is_none());
    // Code keeps its tags.
    assert!(inline_to_markdown("`<b>` and\n\n```\n<br>\n```\n").is_none());
}

#[test]
fn offsets_of_the_converted_text_map_back_to_the_source() {
    let source = "- [ ] <b>a</b>\n- [ ] b\n";
    let converted = inline_to_markdown(source).unwrap();
    let box_at = converted.text.rfind("[ ]").unwrap() + 1;
    assert_eq!(
        converted.source_offset(box_at),
        source.rfind("[ ]").unwrap() + 1
    );
}

#[test]
fn a_centered_block_keeps_its_alignment_and_images() {
    let lines = layout(
        "<p align=\"center\">\n  <a href=\"https://x.y\"><img src=\"logo.png\" width=\"120\"></a>\n</p>\n<h1 align=\"center\">Atelier &amp; co</h1>\n<!-- hidden -->\n<hr>\n<ul><li>one</li><li>two</li></ul>",
    );
    assert_eq!(lines.len(), 5);
    let Line::Content { align, items, .. } = &lines[0] else {
        panic!()
    };
    assert_eq!(*align, Align::Center);
    assert!(matches!(
        &items[0],
        Inline::Image { src, width: Some(w), link: Some(l), .. }
            if src == "logo.png" && *w == 120.0 && l == "https://x.y"
    ));
    let Line::Content { heading, items, .. } = &lines[1] else {
        panic!()
    };
    assert_eq!(*heading, Some(1));
    assert!(matches!(&items[0], Inline::Text { text, strong: true, .. } if text == "Atelier & co"));
    assert_eq!(lines[2], Line::Rule);
    assert!(matches!(
        &lines[4],
        Line::Content { indent: 1, items, .. }
            if matches!(&items[0], Inline::Text { text, .. } if text == "• two")
    ));
}

#[test]
fn entities_are_decoded() {
    assert_eq!(
        decode_entities("a &lt;b&gt; &#39;c&#x27; &unknown; &"),
        "a <b> 'c' &unknown; &"
    );
}
