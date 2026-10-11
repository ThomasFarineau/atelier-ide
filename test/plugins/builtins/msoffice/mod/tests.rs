use super::*;
use std::io::Write;

const DOCUMENT: &str = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
  xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
  xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<w:body>
  <w:p><w:pPr><w:pStyle w:val="Titre1"/></w:pPr><w:r><w:t>Rapport &amp; bilan</w:t></w:r></w:p>
  <w:p><w:pPr><w:jc w:val="center"/></w:pPr>
    <w:r><w:rPr><w:b/><w:color w:val="FF0000"/><w:sz w:val="28"/></w:rPr><w:t xml:space="preserve">Gras </w:t></w:r>
    <w:r><w:rPr><w:b/><w:color w:val="FF0000"/><w:sz w:val="28"/></w:rPr><w:t>rouge</w:t></w:r>
    <w:r><w:tab/><w:t>fin</w:t></w:r>
    <w:hyperlink r:id="rId5"><w:r><w:t>site</w:t></w:r></w:hyperlink>
  </w:p>
  <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>un</w:t></w:r></w:p>
  <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>deux</w:t></w:r></w:p>
  <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>puce</w:t></w:r></w:p>
  <w:tbl>
    <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="4000"/></w:tblGrid>
    <w:tr>
      <w:tc><w:tcPr><w:shd w:fill="DDDDDD"/></w:tcPr><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc>
      <w:tc><w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc>
    </w:tr>
    <w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>fusion</w:t></w:r></w:p></w:tc></w:tr>
  </w:tbl>
  <w:p><w:r><w:drawing><wp:inline><wp:extent cx="952500" cy="476250"/><a:graphic><a:graphicData><a:pic><a:blipFill><a:blip r:embed="rId9"/></a:blipFill></a:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
</w:body></w:document>"#;

const STYLES: &str = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:style w:type="paragraph" w:styleId="Titre1"><w:name w:val="heading 1"/></w:style></w:styles>"#;

const NUMBERING: &str = r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl></w:abstractNum>
<w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val=""/></w:lvl></w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
<w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#;

const RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.org" TargetMode="External"/>
<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
</Relationships>"#;

fn png() -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(4, 2, image::Rgba([200, 30, 30, 255]))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn sample(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("atelier-docx-{}-{name}.docx", std::process::id()));
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for (name, data) in [
        ("word/document.xml", DOCUMENT.as_bytes().to_vec()),
        ("word/styles.xml", STYLES.as_bytes().to_vec()),
        ("word/numbering.xml", NUMBERING.as_bytes().to_vec()),
        ("word/_rels/document.xml.rels", RELS.as_bytes().to_vec()),
        ("word/media/image1.png", png()),
    ] {
        zip.start_file(name, options).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap();
    path
}

#[test]
fn reads_headings_formatting_lists_tables_links_and_images() {
    let path = sample("read");
    let doc = load_doc(&path).unwrap();
    let _ = fs::remove_file(&path);
    assert_eq!(doc.blocks.len(), 7);
    let Block::Para(title) = &doc.blocks[0] else {
        panic!("title")
    };
    assert_eq!(title.kind, Kind::Heading(1));
    let Inline::Text { text, .. } = &title.inlines[0] else {
        panic!("text")
    };
    assert_eq!(text, "Rapport & bilan");

    let Block::Para(p) = &doc.blocks[1] else {
        panic!("paragraph")
    };
    assert_eq!(p.align, Align::Center);
    // The two bold red runs are merged, the tab and the link keep their own.
    let Inline::Text { text, format, link } = &p.inlines[0] else {
        panic!("text")
    };
    assert_eq!(text, "Gras rouge");
    assert!(format.bold);
    assert_eq!(format.color, Some(Color32::from_rgb(255, 0, 0)));
    assert_eq!(format.size, Some(14.0));
    assert!(link.is_none());
    let Some(Inline::Text { link, .. }) = p.inlines.last() else {
        panic!("link")
    };
    assert_eq!(link.as_deref(), Some("https://example.org"));

    let markers: Vec<_> = (2..5)
        .map(|i| match &doc.blocks[i] {
            Block::Para(p) => p.marker.clone().unwrap(),
            Block::Table(_) => panic!("list item"),
        })
        .collect();
    assert_eq!(markers, ["1.", "2.", "•"]);

    let Block::Table(table) = &doc.blocks[5] else {
        panic!("table")
    };
    assert_eq!(table.rows.len(), 2);
    assert_eq!(
        table.rows[0][0].fill,
        Some(Color32::from_rgb(0xDD, 0xDD, 0xDD))
    );
    assert_eq!(table.rows[1][0].span, 2);
    assert_eq!(table.widths, [2000.0, 4000.0]);

    let Block::Para(image) = &doc.blocks[6] else {
        panic!("image")
    };
    let Inline::Image { rel, size } = &image.inlines[0] else {
        panic!("image")
    };
    assert_eq!(rel, "rId9");
    assert_eq!(*size, Some([100.0, 50.0]));
    assert!(doc.images.contains_key("rId9"));
    assert!(doc.words >= 8);
}

#[test]
fn numbers_nested_lists_and_formats() {
    let mut ctx = Ctx::default();
    ctx.abstracts.insert(
        "0".into(),
        vec![
            Level {
                format: "decimal".into(),
                text: "%1.".into(),
                start: 1,
            },
            Level {
                format: "lowerLetter".into(),
                text: "%2)".into(),
                start: 1,
            },
        ],
    );
    ctx.nums.insert("7".into(), "0".into());
    assert_eq!(ctx.marker("7", 0), "1.");
    assert_eq!(ctx.marker("7", 1), "a)");
    assert_eq!(ctx.marker("7", 1), "b)");
    assert_eq!(ctx.marker("7", 0), "2.");
    assert_eq!(ctx.marker("7", 1), "a)", "a lower level starts again");
    assert_eq!(roman(1994, true), "MCMXCIV");
    assert_eq!(letters(27, false), "aa");
}

#[test]
fn rejects_other_files_with_a_readable_message() {
    let dir = std::env::temp_dir();
    let text = dir.join(format!("atelier-docx-{}-text.docx", std::process::id()));
    fs::write(&text, "not a zip").unwrap();
    let error = format!("{:#}", load_doc(&text).err().unwrap());
    let _ = fs::remove_file(&text);
    assert!(error.contains("pas un document Word"), "{error}");
    let old = dir.join("atelier-old.doc");
    fs::write(&old, "x").unwrap();
    let error = format!("{:#}", load_doc(&old).err().unwrap());
    let _ = fs::remove_file(&old);
    assert!(error.contains(".docx"), "{error}");
    assert!(parse_xml(b"<a><b></a>").is_err());
}

#[test]
fn word_files_open_in_the_viewer_and_it_renders() {
    let mut plugin = MsOfficeViewerPlugin::default();
    let path = sample("render");
    assert!(plugin.opens_binary(&path));
    assert!(plugin.opens_binary(Path::new("x.DOCX")));
    assert!(!plugin.opens_binary(Path::new("x.txt")));
    assert_eq!(plugin.editor_layout(&path), Some(EditorLayout::Preview));
    assert!(!plugin.layout_switch(&path));
    let ctx = egui::Context::default();
    egui_extras::install_image_loaders(&ctx);
    ui::install(&ctx, ui::theme_by_index(0), 14.0);
    let mut text = String::new();
    for _ in 0..3 {
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                plugin.preview(ui, &path, &mut text, None);
            });
        });
    }
    let _ = fs::remove_file(&path);
    let loaded = &plugin.loaded[&path];
    assert!(loaded.doc.is_ok());
    assert!(
        loaded.heights.iter().all(|h| *h > 0.0),
        "{:?}",
        loaded.heights
    );
    assert!(loaded.textures["rId9"].is_some());
}
