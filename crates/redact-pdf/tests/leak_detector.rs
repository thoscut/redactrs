//! Kalibrierung des Messgeräts.
//!
//! Jeder Test versteckt [`common::SECRET`] an genau einer Stelle und verlangt,
//! dass [`redact_pdf::leaks`] sie findet **und** benennt. Ein Detektor, den man
//! nicht gegen bekannte Proben geprüft hat, ist wertlos — und ein Detektor, der
//! überall anschlägt, genauso.

mod common;

use common::SECRET;
use lopdf::{dictionary, Object, StringFormat};
use redact_pdf::leaks;

/// Kurzform: findet der Detektor das Geheimnis, und passt die Fundstelle?
fn expect_leak(bytes: &[u8], marker: &str) -> Vec<String> {
    let hits = leaks(bytes, SECRET);
    assert!(
        !hits.is_empty(),
        "Detektor blind: „{SECRET}“ nicht gefunden (erwartet bei {marker})"
    );
    assert!(
        hits.iter().any(|h| h.contains(marker)),
        "Fundstelle nennt „{marker}“ nicht:\n{}",
        hits.join("\n")
    );
    hits
}

// ---------------------------------------------------------------------------
// Negativkontrolle
// ---------------------------------------------------------------------------

#[test]
fn clean_document_reports_nothing() {
    let pdf = common::page(&["Kontoinhaber: Max Mustermann", "Nichts Geheimes hier"]).finish();
    let hits = leaks(&pdf, SECRET);
    assert!(hits.is_empty(), "Fehlalarm:\n{}", hits.join("\n"));
}

#[test]
fn unrelated_needle_is_not_reported() {
    let pdf = common::page(&[&format!("IBAN: {SECRET}")]).finish();
    assert!(leaks(&pdf, "DE99 9999 9999").is_empty());
}

// ---------------------------------------------------------------------------
// Ebene 1: Seiteninhalt und Rohbytes
// ---------------------------------------------------------------------------

#[test]
fn finds_it_in_the_page_content_stream() {
    let pdf = common::page(&[&format!("IBAN: {SECRET}")]).finish();
    expect_leak(&pdf, "Objekt");
}

#[test]
fn finds_text_split_across_tj_fragments() {
    // Kerning zerlegt die IBAN in Bruchstücke — eine reine Bytesuche im Stream
    // findet sie nicht mehr, die Verkettung der Literale schon.
    let mut d = common::page(&[]);
    let raw = b"BT\n/F1 10 Tf\n72 700 Td\n[(DE89 3704 0044 )-15(0532 0130 00)] TJ\nET\n";
    d.set_content(raw);
    let pdf = d.finish();
    expect_leak(&pdf, "Zeichenketten-Verkettung");
}

#[test]
fn finds_hex_string_syntax_in_a_content_stream() {
    let mut d = common::page(&[]);
    let hex: String = SECRET.bytes().map(|b| format!("{b:02X}")).collect();
    d.set_content(format!("BT\n/F1 10 Tf\n72 700 Td\n<{hex}> Tj\nET\n").as_bytes());
    let pdf = d.finish();
    expect_leak(&pdf, "Zeichenketten-Verkettung");
}

// ---------------------------------------------------------------------------
// Ebene 2: Streamfilter
// ---------------------------------------------------------------------------

#[test]
fn finds_it_behind_every_supported_stream_filter() {
    for filter in [
        "FlateDecode",
        "LZWDecode",
        "ASCII85Decode",
        "ASCIIHexDecode",
        "RunLengthDecode",
    ] {
        let pdf = common::filtered_stream(SECRET, filter);
        let hits = leaks(&pdf, SECRET);
        assert!(
            !hits.is_empty(),
            "Filter {filter} nicht dekodiert — Geheimnis unsichtbar"
        );
    }
}

#[test]
fn falls_back_to_raw_bytes_for_an_unsupported_filter() {
    // /Crypt kennt der Detektor nicht; die Rohbytes bleiben durchsuchbar.
    let mut d = common::page(&["Kontoinhaber: Max Mustermann"]);
    d.add(Object::Stream(
        lopdf::Stream::new(
            dictionary! { "Filter" => "Crypt" },
            format!("Notiz {SECRET}").into_bytes(),
        )
        .with_compression(false),
    ));
    let pdf = d.finish();
    expect_leak(&pdf, "roh");
}

// ---------------------------------------------------------------------------
// Ebene 3: Objekt-Streams und verwaiste Objekte
// ---------------------------------------------------------------------------

#[test]
fn finds_it_inside_an_object_stream() {
    let pdf = common::object_stream(SECRET);
    // Gegenprobe: roh steht es dort nicht — der Container ist komprimiert.
    assert!(
        !pdf.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()),
        "Testdaten taugen nicht: der ObjStm ist unkomprimiert"
    );
    expect_leak(&pdf, "ObjStm");
}

#[test]
fn finds_it_in_an_orphaned_object() {
    let pdf = common::orphan_object(SECRET);
    expect_leak(&pdf, "ActualText");
}

// ---------------------------------------------------------------------------
// Ebene 4: Zeichenketten überall im Objektgraph
// ---------------------------------------------------------------------------

#[test]
fn finds_it_in_document_info() {
    let mut d = common::page(&["Nichts Geheimes"]);
    let info = d.add(Object::Dictionary(dictionary! {
        "Title" => Object::string_literal(format!("Kontoauszug {SECRET}")),
    }));
    d.doc.trailer.set("Info", Object::Reference(info));
    let pdf = d.finish();
    expect_leak(&pdf, "/Title");
}

#[test]
fn finds_it_in_a_utf16_form_field_value() {
    let pdf = common::form_field_value(SECRET);
    // Gegenprobe: als Latin-1-Bytes steht es im /V nicht.
    let hits = expect_leak(&pdf, "/V");
    assert!(
        hits.iter().any(|h| h.contains("UTF-16BE")),
        "UTF-16BE nicht als solches erkannt:\n{}",
        hits.join("\n")
    );
}

#[test]
fn finds_it_in_a_hex_encoded_string_object() {
    let mut d = common::page(&["Nichts Geheimes"]);
    d.add(Object::Dictionary(dictionary! {
        "Type" => "Vergessen",
        "ActualText" => Object::String(SECRET.as_bytes().to_vec(), StringFormat::Hexadecimal),
    }));
    let pdf = d.finish();
    let hits = expect_leak(&pdf, "/ActualText");
    assert!(
        hits.iter().any(|h| h.contains("hex")),
        "Hex-Syntax nicht benannt:\n{}",
        hits.join("\n")
    );
}

#[test]
fn finds_it_in_a_names_tree_entry() {
    let mut d = common::page(&["Nichts Geheimes"]);
    let names = d.add(Object::Dictionary(dictionary! {
        "Names" => vec![
            Object::string_literal("konto"),
            Object::string_literal(SECRET),
        ],
    }));
    d.catalog_set(
        "Names",
        Object::Dictionary(dictionary! { "Dests" => names }),
    );
    let pdf = d.finish();
    expect_leak(&pdf, "/Names");
}

#[test]
fn finds_it_in_a_struct_elem_actual_text() {
    let pdf = common::struct_elem_actual_text(SECRET);
    expect_leak(&pdf, "/ActualText");
}

#[test]
fn finds_it_in_an_annotation_appearance_stream() {
    let pdf = common::annotation_appearance(SECRET, false);
    expect_leak(&pdf, "Stream");
}

// ---------------------------------------------------------------------------
// Ebene 5: Metadaten, XObjects, Inline-Bilder, Historie
// ---------------------------------------------------------------------------

#[test]
fn finds_it_in_compressed_page_metadata() {
    let pdf = common::page_metadata_xmp(SECRET);
    expect_leak(&pdf, "Stream");
}

#[test]
fn finds_it_inside_a_form_xobject() {
    let pdf = common::form_xobject(SECRET, false);
    expect_leak(&pdf, "Stream");
}

#[test]
fn finds_it_after_an_inline_image() {
    let pdf = common::inline_image_before_text(SECRET);
    expect_leak(&pdf, "Stream");
}

#[test]
fn finds_it_in_the_base_revision_of_an_incremental_update() {
    let pdf = common::incremental_history(SECRET, "XXXX XXXX XXXX XXXX XXXX XX");
    // Die aktuelle Revision ist sauber — der Extraktor sieht nichts.
    let doc = redact_pdf::load_from_bytes(&pdf).expect("ladbar");
    let content = doc
        .get_page_content(*doc.get_pages().values().next().unwrap())
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&content).contains("DE89"),
        "Testdaten taugen nicht: die neue Revision enthält das Geheimnis noch"
    );
    expect_leak(&pdf, "Rohdatei");
}
