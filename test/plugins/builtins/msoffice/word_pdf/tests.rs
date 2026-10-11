use super::*;
use crate::plugins::builtins::msoffice::{Cell, Inline};
use eframe::egui::{Align, Color32};

fn text_para(text: &str) -> Block {
    Block::Para(Para {
        kind: Kind::Body,
        align: Align::Min,
        indent: 0.0,
        marker: None,
        inlines: vec![Inline::Text {
            text: text.into(),
            format: Format::default(),
            link: None,
        }],
    })
}

#[test]
fn long_documents_span_several_pages() {
    let mut blocks: Vec<Block> = (0..120)
        .map(|i| text_para(&format!("Paragraphe numéro {i}")))
        .collect();
    blocks.push(Block::Table(Table {
        widths: vec![1.0, 1.0],
        rows: vec![vec![
            Cell {
                span: 1,
                fill: Some(Color32::LIGHT_GRAY),
                blocks: vec![text_para("A")],
            },
            Cell {
                span: 1,
                fill: None,
                blocks: vec![text_para("B")],
            },
        ]],
    }));
    let doc = Doc {
        blocks,
        images: HashMap::new(),
        words: 0,
    };
    let path = std::env::temp_dir().join(format!("atelier-word-{}.pdf", std::process::id()));
    export(&doc, &path).unwrap();
    let bytes = fs::read(&path).unwrap();
    let _ = fs::remove_file(&path);
    let text = String::from_utf8_lossy(&bytes);
    assert!(bytes.starts_with(b"%PDF-"));
    let pages: usize = text
        .split("/Count ")
        .nth(1)
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(pages >= 3, "{pages} pages");
}
