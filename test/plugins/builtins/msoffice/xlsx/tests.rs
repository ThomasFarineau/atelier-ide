use super::*;
use std::io::Write;

const WORKBOOK: &str = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Ventes" sheetId="1" r:id="rId1"/><sheet name="Cachée" sheetId="2" state="hidden" r:id="rId2"/></sheets></workbook>"#;

const RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="x/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="x/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#;

const STRINGS: &str = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<si><t>Produit</t></si><si><r><t>Prix </t></r><r><t>HT</t></r></si><si><t>Café &amp; thé</t></si></sst>"#;

const STYLES: &str = r##"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<numFmts count="1"><numFmt numFmtId="164" formatCode="#,##0.00\ &quot;€&quot;"/></numFmts>
<fonts count="2"><font><sz val="11"/></font><font><b/><color rgb="FFFF0000"/></font></fonts>
<fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>
<fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/></patternFill></fill></fills>
<cellXfs count="4">
<xf numFmtId="0" fontId="0" fillId="0"/>
<xf numFmtId="164" fontId="0" fillId="0"/>
<xf numFmtId="14" fontId="0" fillId="0"/>
<xf numFmtId="0" fontId="1" fillId="2"><alignment horizontal="center"/></xf>
</cellXfs></styleSheet>"##;

const SHEET: &str = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<cols><col min="1" max="1" width="20" customWidth="1"/><col min="3" max="3" width="10" hidden="1"/></cols>
<sheetData>
<row r="1"><c r="A1" t="s" s="3"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="inlineStr"><is><t>caché</t></is></c></row>
<row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2" s="1"><v>1234.5</v></c><c r="D2" s="2"><v>45000</v></c></row>
<row r="3"><c r="A3" t="b"><v>1</v></c><c r="B3"><f>B2*2</f><v>2469</v></c><c r="D3" t="e"><v>#DIV/0!</v></c></row>
</sheetData></worksheet>"#;

fn sample(name: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("atelier-xlsx-{}-{name}.xlsx", std::process::id()));
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for (name, data) in [
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", RELS),
        ("xl/sharedStrings.xml", STRINGS),
        ("xl/styles.xml", STYLES),
        ("xl/worksheets/sheet1.xml", SHEET),
        ("xl/worksheets/sheet2.xml", "<worksheet/>"),
    ] {
        zip.start_file(name, options).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    path
}

#[test]
fn reads_values_styles_and_widths() {
    let path = sample("read");
    let wb = load(&path).unwrap();
    let _ = fs::remove_file(&path);
    assert_eq!(wb.sheets.len(), 1, "the hidden sheet is skipped");
    let sheet = &wb.sheets[0];
    assert_eq!((sheet.rows, sheet.cols), (3, 4));
    let text = |r, c| sheet.cells[&(r, c)].text.as_str();
    assert_eq!(text(0, 0), "Produit");
    assert_eq!(text(0, 1), "Prix HT");
    assert_eq!(text(0, 2), "caché");
    assert_eq!(text(1, 0), "Café & thé");
    assert_eq!(text(1, 1), "1\u{a0}234,50 €");
    assert_eq!(text(1, 3), "15/03/2023");
    assert_eq!(text(2, 0), "VRAI");
    assert_eq!(text(2, 1), "2469");
    assert_eq!(text(2, 3), "#DIV/0!");
    let header = wb.style(&sheet.cells[&(0, 0)]);
    assert!(header.bold);
    assert_eq!(header.fill, Some(Color32::from_rgb(255, 255, 0)));
    assert_eq!(header.color, Some(Color32::from_rgb(255, 0, 0)));
    assert_eq!(sheet.cells[&(0, 0)].align, Align::Center);
    assert_eq!(sheet.cells[&(1, 1)].align, Align::Max);
    assert_eq!(sheet.widths, [145.0, 64.0, 0.0, 64.0]);
}

#[test]
fn formats_numbers_and_dates() {
    let f = |code: &str, v: f64| format_value(&parse_format(code), v);
    assert_eq!(f("General", 0.1 + 0.2), "0,3");
    assert_eq!(f("General", 42.0), "42");
    assert_eq!(f("0.00", 1.23456), "1,23");
    assert_eq!(f("0%", 0.256), "26%");
    assert_eq!(f("0.0%", 0.256), "25,6%");
    assert_eq!(f("#,##0", 1234567.0), "1\u{a0}234\u{a0}567");
    assert_eq!(f("000", 7.0), "007");
    assert_eq!(f("#,##0.00;(#,##0.00)", -5.5), "(5,50)");
    assert_eq!(f("0.00", -5.5), "-5,50");
    assert_eq!(f("[$€-40C] #,##0", 12.0), "€ 12");
    assert_eq!(f("yyyy-mm-dd", 45000.0), "2023-03-15");
    assert_eq!(f("dd mmmm yyyy", 45000.0), "15 mars 2023");
    assert_eq!(f("dddd d mmm", 45000.0), "mercredi 15 mars");
    assert_eq!(f("h:mm", 0.75), "18:00");
    assert_eq!(f("h:mm:ss AM/PM", 0.75), "6:00:00 PM");
    assert_eq!(f("dd/mm/yyyy hh:mm", 45000.5), "15/03/2023 12:00");
    assert_eq!(f("@", 5.0), "5");
    assert_eq!(cell_ref("AB12"), Some((11, 27)));
    assert_eq!(column_name(27), "AB");
}

#[test]
fn rejects_other_files_and_exports_a_pdf() {
    let text = std::env::temp_dir().join(format!("atelier-xlsx-{}-text.xlsx", std::process::id()));
    fs::write(&text, "not a zip").unwrap();
    let error = format!("{:#}", load(&text).err().unwrap());
    let _ = fs::remove_file(&text);
    assert!(error.contains("pas un classeur"), "{error}");

    let path = sample("pdf");
    let wb = load(&path).unwrap();
    let _ = fs::remove_file(&path);
    let target = std::env::temp_dir().join(format!("atelier-xlsx-{}.pdf", std::process::id()));
    export_pdf(&wb, &target).unwrap();
    let bytes = fs::read(&target).unwrap();
    let _ = fs::remove_file(&target);
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn the_grid_renders() {
    let path = sample("render");
    let wb = load(&path).unwrap();
    let _ = fs::remove_file(&path);
    let ctx = egui::Context::default();
    ui::install(&ctx, ui::theme_by_index(0), 14.0);
    let mut state = SheetView::default();
    for _ in 0..2 {
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                show(ui, &wb, &mut state, Path::new("a.xlsx"), 1.0)
            });
        });
    }
}
