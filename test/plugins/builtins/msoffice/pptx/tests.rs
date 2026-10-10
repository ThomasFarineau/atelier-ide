use super::*;
use std::io::Write;

const PRESENTATION: &str = r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#;

const PRESENTATION_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;

const SLIDE_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#;

const LAYOUT_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#;

const LAYOUT: &str = r#"<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Titre"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="457200" y="228600"/><a:ext cx="8229600" cy="857250"/></a:xfrm></p:spPr></p:sp>
</p:spTree></p:cSld></p:sldLayout>"#;

const MASTER: &str = r#"<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="EEEEEE"/></a:solidFill></p:bgPr></p:bg><p:spTree>
<p:sp><p:nvSpPr><p:cNvPr id="3" name="Barre"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="9144000" cy="100000"/></a:xfrm><a:prstGeom prst="rect"/><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></p:spPr></p:sp>
</p:spTree></p:cSld>
<p:txStyles><p:titleStyle><a:lvl1pPr algn="ctr"><a:defRPr sz="4000"/></a:lvl1pPr></p:titleStyle></p:txStyles></p:sldMaster>"#;

const THEME: &str = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="t">
<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
<a:dk2><a:srgbClr val="1F497D"/></a:dk2><a:lt2><a:srgbClr val="EEECE1"/></a:lt2>
<a:accent1><a:srgbClr val="4F81BD"/></a:accent1></a:clrScheme></a:themeElements></a:theme>"#;

const SLIDE: &str = r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Titre"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/>
 <p:txBody><a:bodyPr/><a:p><a:r><a:rPr lang="fr-FR" b="1"/><a:t>Bilan annuel</a:t></a:r></a:p></p:txBody></p:sp>
<p:grpSp><p:nvGrpSpPr><p:cNvPr id="9" name="Groupe"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
 <p:grpSpPr><a:xfrm><a:off x="1000000" y="1000000"/><a:ext cx="2000000" cy="1000000"/><a:chOff x="0" y="0"/><a:chExt cx="1000000" cy="500000"/></a:xfrm></p:grpSpPr>
 <p:sp><p:nvSpPr><p:cNvPr id="4" name="Carré"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
  <p:spPr><a:xfrm><a:off x="100000" y="100000"/><a:ext cx="400000" cy="200000"/></a:xfrm><a:prstGeom prst="ellipse"/>
  <a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:ln w="25400"><a:solidFill><a:srgbClr val="0000FF"/></a:solidFill></a:ln></p:spPr>
  <p:txBody><a:bodyPr anchor="ctr"/><a:p><a:pPr algn="ctr"/><a:r><a:rPr sz="2000"><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:rPr><a:t>Rond</a:t></a:r></a:p></p:txBody></p:sp>
</p:grpSp>
<p:pic><p:nvPicPr><p:cNvPr id="5" name="Image"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>
 <p:blipFill><a:blip r:embed="rId2"/><a:srcRect l="10000" r="10000"/></p:blipFill>
 <p:spPr><a:xfrm><a:off x="5000000" y="2000000"/><a:ext cx="1000000" cy="500000"/></a:xfrm></p:spPr></p:pic>
<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="6" name="Tableau"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>
 <p:xfrm><a:off x="500000" y="3000000"/><a:ext cx="4000000" cy="600000"/></p:xfrm>
 <a:graphic><a:graphicData><a:tbl><a:tblPr firstRow="1" bandRow="1"><a:tableStyleId>{X}</a:tableStyleId></a:tblPr>
  <a:tblGrid><a:gridCol w="2000000"/><a:gridCol w="2000000"/></a:tblGrid>
  <a:tr h="300000"><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Mois</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Total</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc></a:tr>
  <a:tr h="300000"><a:tc gridSpan="2"><a:txBody><a:bodyPr/><a:p><a:r><a:t>fusion</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc><a:tc hMerge="1"><a:txBody><a:bodyPr/><a:p/></a:txBody><a:tcPr/></a:tc></a:tr>
 </a:tbl></a:graphicData></a:graphic></p:graphicFrame>
</p:spTree></p:cSld></p:sld>"#;

fn png() -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(4, 2, image::Rgba([200, 30, 30, 255]))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn sample(name: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("atelier-pptx-{}-{name}.pptx", std::process::id()));
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for (name, data) in [
        ("ppt/presentation.xml", PRESENTATION.as_bytes().to_vec()),
        (
            "ppt/_rels/presentation.xml.rels",
            PRESENTATION_RELS.as_bytes().to_vec(),
        ),
        ("ppt/slides/slide1.xml", SLIDE.as_bytes().to_vec()),
        (
            "ppt/slides/_rels/slide1.xml.rels",
            SLIDE_RELS.as_bytes().to_vec(),
        ),
        (
            "ppt/slideLayouts/slideLayout1.xml",
            LAYOUT.as_bytes().to_vec(),
        ),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            LAYOUT_RELS.as_bytes().to_vec(),
        ),
        (
            "ppt/slideMasters/slideMaster1.xml",
            MASTER.as_bytes().to_vec(),
        ),
        ("ppt/theme/theme1.xml", THEME.as_bytes().to_vec()),
        ("ppt/media/image1.png", png()),
    ] {
        zip.start_file(name, options).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap();
    path
}

#[test]
fn reads_shapes_inheritance_groups_pictures_and_tables() {
    let path = sample("read");
    let deck = load(&path).unwrap();
    let _ = fs::remove_file(&path);
    assert_eq!(deck.size, vec2(9_144_000.0, 5_143_500.0));
    assert_eq!(deck.slides.len(), 1);
    let slide = &deck.slides[0];
    assert_eq!(
        slide.bg,
        Color32::from_rgb(0xEE, 0xEE, 0xEE),
        "background of the master"
    );
    // The bar of the master, the title, the group's ellipse, the picture, the table.
    assert_eq!(slide.items.len(), 5);
    let Item::Shape { fill, .. } = &slide.items[0] else {
        panic!("master bar")
    };
    assert_eq!(*fill, Some(Color32::from_rgb(0x4F, 0x81, 0xBD)));

    let Item::Shape { rect, text, .. } = &slide.items[1] else {
        panic!("title")
    };
    assert_eq!(
        rect.min,
        pos2(457_200.0, 228_600.0),
        "position of the layout"
    );
    let text = text.as_ref().unwrap();
    assert_eq!(text.paras[0].runs[0].text, "Bilan annuel");
    assert_eq!(text.paras[0].runs[0].size, 40.0, "size of the master");
    assert_eq!(text.paras[0].align, Align::Center);
    assert_eq!(text.anchor, Anchor::Middle);
    assert!(text.paras[0].runs[0].bold);

    let Item::Shape {
        rect,
        geom,
        fill,
        line,
        text,
    } = &slide.items[2]
    else {
        panic!("ellipse")
    };
    assert_eq!(*geom, Geom::Ellipse);
    // The group is drawn twice as large as its content.
    assert_eq!(rect.min, pos2(1_200_000.0, 1_200_000.0));
    assert_eq!(rect.size(), vec2(800_000.0, 400_000.0));
    assert_eq!(*fill, Some(Color32::RED));
    assert_eq!(*line, Some((Color32::BLUE, 25_400.0)));
    assert_eq!(
        text.as_ref().unwrap().paras[0].runs[0].color,
        Color32::WHITE
    );

    let Item::Image { name, crop, .. } = &slide.items[3] else {
        panic!("picture")
    };
    assert_eq!(name, "ppt/media/image1.png");
    assert_eq!(*crop, [0.1, 0.0, 0.1, 0.0]);
    assert!(deck.media.contains_key(name));

    let Item::Table { columns, rows, .. } = &slide.items[4] else {
        panic!("table")
    };
    assert_eq!(columns, &[2_000_000.0, 2_000_000.0]);
    assert_eq!(
        rows[0].cells[0].fill,
        Some(Color32::from_rgb(0x4F, 0x81, 0xBD)),
        "header of the table style"
    );
    assert_eq!(rows[0].cells[0].text.paras[0].runs[0].color, Color32::WHITE);
    assert_eq!(rows[1].cells[0].span, 2);
    assert_eq!(rows[1].cells[1].span, 0);
    assert!(deck.words >= 6);
}

#[test]
fn exports_and_renders_slides() {
    let path = sample("export");
    let deck = load(&path).unwrap();
    let _ = fs::remove_file(&path);
    let target = std::env::temp_dir().join(format!("atelier-pptx-{}.pdf", std::process::id()));
    export_pdf(&deck, &target).unwrap();
    let bytes = fs::read(&target).unwrap();
    let _ = fs::remove_file(&target);
    assert!(bytes.starts_with(b"%PDF-"));

    let ctx = egui::Context::default();
    egui_extras::install_image_loaders(&ctx);
    ui::install(&ctx, ui::theme_by_index(0), 14.0);
    let mut textures = Textures::new();
    for _ in 0..2 {
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                show(ui, &deck, &mut textures, Path::new("a.pptx"), 1.0)
            });
        });
    }
    assert!(textures.values().all(Option::is_some));
}

#[test]
fn resolves_relative_targets() {
    assert_eq!(
        resolve("ppt/slides/slide1.xml", "../media/a.png"),
        "ppt/media/a.png"
    );
    assert_eq!(
        resolve("ppt/presentation.xml", "slides/slide1.xml"),
        "ppt/slides/slide1.xml"
    );
    assert_eq!(
        resolve("ppt/slides/slide1.xml", "/ppt/media/b.png"),
        "ppt/media/b.png"
    );
}

#[test]
fn rejects_other_files() {
    let text = std::env::temp_dir().join(format!("atelier-pptx-{}-text.pptx", std::process::id()));
    fs::write(&text, "not a zip").unwrap();
    let error = format!("{:#}", load(&text).err().unwrap());
    let _ = fs::remove_file(&text);
    assert!(error.contains("pas une présentation"), "{error}");
}
