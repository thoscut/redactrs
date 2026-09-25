//! Dieselbe Warnung steht auch in der Vorschau genau **einmal**.
//!
//! Gegenstück zu `redact-pdf/tests/z8_warnungen_entdoppelt.rs`, für den
//! Rasterizer. `Painter::warn` wird je **Zeichenoperation** gerufen: eine
//! Seite mit zehntausend Glyphen im Konturmodus (`1 Tr`) löst zehntausendmal
//! denselben Satz aus. Entdoppelt wurde das schon immer, gehalten hat es kein
//! Test — ausgebaut blieb die ganze Suite grün, Bildvergleich eingeschlossen.
//!
//! Die Entdopplung lief dabei über `warnings.contains(…)`, also quadratisch;
//! jetzt über eine Menge daneben. Sichtbar ist davon nichts, und genau das
//! prüft diese Datei.

use lopdf::{dictionary, Document, Object, Stream};
use redact_render::{PageRenderer, RenderOptions};

/// Wie viele Zeichen im Konturmodus gesetzt werden.
const GLYPHEN: usize = 4_000;

fn seite_mit_konturtext() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    // `1 Tr` ist der Konturmodus; er wird flächig gefüllt gezeichnet, und
    // genau das meldet der Maler — je Glyphe einmal.
    let mut content = String::from("BT /F1 8 Tf 1 Tr\n");
    for zeile in 0..(GLYPHEN / 100) {
        content.push_str(&format!("1 0 0 1 20 {} Tm (", 820 - zeile * 20));
        content.push_str(&"A".repeat(100));
        content.push_str(") Tj\n");
    }
    content.push_str("ET\n");

    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let res_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font },
    });
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Resources" => res_id,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1_i64,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    out
}

#[test]
fn dieselbe_warnung_steht_genau_einmal_in_der_vorschau() {
    let bytes = seite_mit_konturtext();
    let doc = redact_pdf::document::load_from_bytes(&bytes).expect("ladbar");
    let seite = PageRenderer::new().render(&doc, 0, &RenderOptions::default());

    let kontur: Vec<&String> = seite
        .warnings
        .iter()
        .filter(|w| w.contains("nur als Kontur"))
        .collect();
    assert_eq!(
        kontur.len(),
        1,
        "{GLYPHEN} Glyphen im Konturmodus, {} gleichlautende Warnungen",
        kontur.len()
    );
    // Und die Seite ist dabei wirklich gezeichnet worden.
    assert!(!seite.degraded, "Notnagel-Pfad statt echtem Bild");
    assert!(
        seite.drawn_ops > GLYPHEN / 2,
        "nur {} Operationen gezeichnet",
        seite.drawn_ops
    );
}
