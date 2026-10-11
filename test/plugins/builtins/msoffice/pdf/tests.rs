use super::*;

fn span(text: &str) -> Inl {
    Inl::Text(Span {
        text: text.into(),
        font: Font::Regular,
        size: 10.0,
        color: Color32::BLACK,
        underline: false,
        strike: false,
        bg: None,
        raise: 0.0,
    })
}

#[test]
fn measures_and_encodes_text() {
    assert!((text_width(Font::Regular, 10.0, "Hello") - 22.78).abs() < 0.01);
    assert_eq!(encode('é'), Some(0xE9));
    assert_eq!(encode('’'), Some(0x92));
    assert_eq!(encode('\u{1}'), None);
    assert!(fit(Font::Regular, 10.0, "un texte bien trop long", 40.0).ends_with('…'));
    assert_eq!(fit(Font::Regular, 10.0, "ok", 40.0), "ok");
}

#[test]
fn wraps_words_and_cuts_overlong_ones() {
    let lines = flow(
        &[span("un deux trois quatre cinq six sept huit")],
        60.0,
        10.0,
    );
    assert!(lines.len() > 2);
    assert!(
        lines.iter().all(|l| l.width <= 60.5),
        "{:?}",
        lines.iter().map(|l| l.width).collect::<Vec<_>>()
    );
    let lines = flow(&[span("supercalifragilisticexpialidocious")], 50.0, 10.0);
    assert!(lines.len() > 1);
    let lines = flow(&[span("a\nb")], 100.0, 10.0);
    assert_eq!(lines.len(), 2);
    assert_eq!(flow(&[], 100.0, 10.0).len(), 1);
}

#[test]
fn writes_a_well_formed_file() {
    let mut pdf = Pdf::default();
    pdf.page(200.0, 100.0);
    pdf.text(
        10.0,
        20.0,
        Font::Bold,
        12.0,
        Color32::RED,
        "Bonjour (le) monde é",
    );
    pdf.fill_rect(5.0, 5.0, 20.0, 10.0, Color32::BLUE);
    pdf.ellipse(
        30.0,
        30.0,
        20.0,
        10.0,
        Some(Color32::GREEN),
        Some((Color32::BLACK, 1.0)),
    );
    pdf.rounded_rect(
        60.0,
        30.0,
        20.0,
        10.0,
        3.0,
        None,
        Some((Color32::BLACK, 1.0)),
    );
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        2,
        2,
        image::Rgba([1, 2, 3, 100]),
    ));
    let index = pdf.add_image(&img);
    pdf.image(0.0, 0.0, 10.0, 10.0, index);
    pdf.page(200.0, 100.0);
    let bytes = pdf.finish();
    let text = String::from_utf8_lossy(&bytes);
    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert!(text.contains("/Count 2"));
    assert!(text.contains("/SMask"));
    assert!(text.trim_end().ends_with("%%EOF"));
    // The xref table points at the objects.
    let start: usize = text
        .rsplit("startxref\n")
        .next()
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(bytes[start..].starts_with(b"xref"));
}
