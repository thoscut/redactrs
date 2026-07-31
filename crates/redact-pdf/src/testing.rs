//! Hilfsmittel zum Erzeugen einfacher PDFs.
//!
//! Wird von den Tests benutzt und von `redact-rs demo`, um ohne fremde
//! Beispieldateien einen realistischen Kontoauszug zum Ausprobieren zu bauen.

use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};

/// Ein Textelement auf einer Seite.
#[derive(Debug, Clone)]
pub struct TextItem {
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub text: String,
}

impl TextItem {
    pub fn new(x: f64, y: f64, size: f64, text: impl Into<String>) -> Self {
        Self {
            x,
            y,
            size,
            text: text.into(),
        }
    }
}

/// Erzeugt ein PDF mit einer Seite je `Vec<TextItem>`.
///
/// Der Text wird als Helvetica/WinAnsi gesetzt — genau die Konstellation, mit
/// der die Extraktion in der Praxis am häufigsten zu tun hat.
pub fn build_pdf(pages: &[Vec<TextItem>]) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });

    let pages_id = doc.new_object_id();
    let mut page_ids = Vec::new();

    for items in pages {
        let mut operations = vec![Operation::new("BT", vec![])];
        for item in items {
            operations.push(Operation::new(
                "Tf",
                vec![Object::Name(b"F1".to_vec()), Object::Real(item.size as f32)],
            ));
            operations.push(Operation::new(
                "Tm",
                vec![
                    Object::Real(1.0),
                    Object::Real(0.0),
                    Object::Real(0.0),
                    Object::Real(1.0),
                    Object::Real(item.x as f32),
                    Object::Real(item.y as f32),
                ],
            ));
            operations.push(Operation::new(
                "Tj",
                vec![Object::String(win_ansi(&item.text), StringFormat::Literal)],
            ));
        }
        operations.push(Operation::new("ET", vec![]));

        let content = Content { operations }.encode().expect("Content kodierbar");
        let content_id = doc.add_object(Stream::new(dictionary! {}, content));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        page_ids.push(page_id);
    }

    let count = page_ids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.into_iter().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => count,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let info_id = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Kontoauszug Max Mustermann"),
        "Author" => Object::string_literal("Max Mustermann"),
        "Producer" => Object::string_literal("redact-rs testing"),
    });
    doc.trailer.set("Info", info_id);

    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).expect("PDF speicherbar");
    buffer
}

/// Ein einseitiges PDF mit einer einzigen Textzeile bei (72, 700).
pub fn minimal_pdf(text: &str) -> Vec<u8> {
    build_pdf(&[vec![TextItem::new(72.0, 700.0, 12.0, text)]])
}

/// Ein realistischer zweiseitiger Beispiel-Kontoauszug.
pub fn demo_statement() -> Vec<u8> {
    let page1 = vec![
        TextItem::new(72.0, 780.0, 16.0, "Musterbank AG - Kontoauszug"),
        TextItem::new(72.0, 750.0, 10.0, "Kontoinhaber: Max Mustermann"),
        TextItem::new(72.0, 735.0, 10.0, "IBAN: DE89 3704 0044 0532 0130 00"),
        TextItem::new(72.0, 720.0, 10.0, "BIC: COBADEFFXXX"),
        TextItem::new(72.0, 705.0, 10.0, "Kontonummer: 532013000"),
        TextItem::new(72.0, 675.0, 10.0, "Buchungen:"),
        TextItem::new(
            72.0,
            655.0,
            10.0,
            "05.01.2026  Ueberweisung an Musterfirma GmbH   1.234,56 EUR",
        ),
        TextItem::new(
            72.0,
            640.0,
            10.0,
            "12.01.2026  Gehalt Arbeitgeber XY              3.500,00 EUR",
        ),
        TextItem::new(
            72.0,
            625.0,
            10.0,
            "18.01.2026  Lastschrift Stadtwerke              89,90 EUR",
        ),
        TextItem::new(
            72.0,
            610.0,
            10.0,
            "23.01.2026  Ueberweisung an Musterfirma GmbH     420,00 EUR",
        ),
    ];
    let page2 = vec![
        TextItem::new(72.0, 780.0, 14.0, "Seite 2"),
        TextItem::new(
            72.0,
            750.0,
            10.0,
            "Empfaenger-IBAN: DE02 1203 0000 0000 2020 51",
        ),
        TextItem::new(72.0, 735.0, 10.0, "Kontakt: max.mustermann@example.org"),
        TextItem::new(72.0, 720.0, 10.0, "Telefon: +49 30 123456789"),
        TextItem::new(72.0, 705.0, 10.0, "Steuer-ID: 12345678901"),
    ];
    build_pdf(&[page1, page2])
}

fn win_ansi(text: &str) -> Vec<u8> {
    let table = crate::encoding::win_ansi_encoding();
    text.chars()
        .map(|ch| {
            table
                .iter()
                .position(|entry| *entry == Some(ch))
                .map(|i| i as u8)
                .unwrap_or(b'?')
        })
        .collect()
}
