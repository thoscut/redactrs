//! Filter, die der Schwärzer nicht konnte, das Orakel schon.
//!
//! Gemessen (vor dieser Änderung) mit `redact-rs c.pdf -o out.pdf`, derselbe
//! Seiteninhalt je Filter:
//!
//! ```text
//! /ASCIIHexDecode                   → Rückgabewert 1 (abgelehnt)
//! /RunLengthDecode                  → Rückgabewert 1
//! [/ASCIIHexDecode /FlateDecode]    → Rückgabewert 1
//! [/RunLengthDecode /FlateDecode]   → Rückgabewert 1
//! /ASCII85Decode                    → Rückgabewert 0, 1 Treffer
//! ```
//!
//! Die Ablehnung war die ehrliche Richtung — aber eine Grenze, die
//! gewöhnliche Dateien abweist, ist genauso ein Fehler. `redact_pdf::leaks`
//! konnte diese Filter längst; seit `redact_pdf::filters` liest der
//! Schwärzer dasselbe.

mod common;

use common::{ascii85_encode, ascii_hex_encode, page, run_length_encode, text_ops, SECRET};
use lopdf::{dictionary, Object, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

/// Immer komprimiert — `Stream::compress` ließe einen kurzen Strom
/// unverändert, und ein unkomprimierter Strom mit `/FlateDecode` misst nichts.
fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).expect("komprimierbar");
    enc.finish().expect("komprimierbar")
}

/// Eine Seite, deren Inhalt mit `filters` (in Dekodierreihenfolge) kodiert ist.
fn page_with_filters(filters: &[&str]) -> Vec<u8> {
    let mut data = text_ops(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]);
    // Kodiert wird in umgekehrter Reihenfolge: der letzte Filter der Kette
    // wird zuerst angewandt.
    for filter in filters.iter().rev() {
        data = match *filter {
            "FlateDecode" => deflate(&data),
            "ASCIIHexDecode" => ascii_hex_encode(&data),
            "ASCII85Decode" => ascii85_encode(&data),
            "RunLengthDecode" => run_length_encode(&data),
            other => panic!("unbekannter Filter {other}"),
        };
    }
    let filter = if filters.len() == 1 {
        Object::Name(filters[0].as_bytes().to_vec())
    } else {
        Object::Array(
            filters
                .iter()
                .map(|f| Object::Name(f.as_bytes().to_vec()))
                .collect(),
        )
    };
    let mut d = page(&[]);
    d.doc.objects.insert(
        d.content_id,
        Object::Stream(
            Stream::new(dictionary! { "Filter" => filter }, data).with_compression(false),
        ),
    );
    d.finish()
}

fn extract(bytes: &[u8]) -> Vec<TextRun> {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new().extract(&doc).expect("Extraktion")
}

fn redaction_for(runs: &[TextRun], needle: &str) -> Redaction {
    let run = runs
        .iter()
        .find(|r| r.text.contains(needle))
        .expect("die Analyse sieht das Geheimnis");
    let pos = run.text.find(needle).unwrap();
    let rect = run.rect_for_byte_range(pos, pos + needle.len()).unwrap();
    Redaction::new(
        Region::new(
            run.page,
            rect,
            Some(needle.to_string()),
            Source::Pattern {
                pattern_id: "iban_de".into(),
                confidence: 1.0,
            },
        ),
        Action::Blackout,
    )
}

fn assert_read_and_redacted(filters: &[&str]) {
    let bytes = page_with_filters(filters);
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "{filters:?}: das Orakel muss den Filter lesen können, sonst misst der Test nichts"
    );
    let runs = extract(&bytes);
    let redaction = redaction_for(&runs, SECRET);
    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[redaction])
        .expect("Schwärzung");
    assert!(
        report.removed_glyphs > 0,
        "{filters:?}: es wurden Zeichen entfernt"
    );
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    assert!(
        leaks(&out, SECRET).is_empty(),
        "{filters:?}: die IBAN steht noch in der Ausgabe: {:?}",
        leaks(&out, SECRET)
    );
    assert!(
        !leaks(&out, "Kontoinhaber").is_empty(),
        "{filters:?}: der übrige Text bleibt lesbar"
    );
}

#[test]
fn asciihex_im_seiteninhalt_wird_gelesen_und_geschwaerzt() {
    assert_read_and_redacted(&["ASCIIHexDecode"]);
}

#[test]
fn runlength_im_seiteninhalt_wird_gelesen_und_geschwaerzt() {
    assert_read_and_redacted(&["RunLengthDecode"]);
}

#[test]
fn ascii85_im_seiteninhalt_wird_weiterhin_gelesen() {
    assert_read_and_redacted(&["ASCII85Decode"]);
}

#[test]
fn die_kette_asciihex_flate_wird_gelesen_und_geschwaerzt() {
    assert_read_and_redacted(&["ASCIIHexDecode", "FlateDecode"]);
}

#[test]
fn die_kette_runlength_flate_wird_gelesen_und_geschwaerzt() {
    assert_read_and_redacted(&["RunLengthDecode", "FlateDecode"]);
}

#[test]
fn die_kette_ascii85_flate_wird_gelesen_und_geschwaerzt() {
    assert_read_and_redacted(&["ASCII85Decode", "FlateDecode"]);
}

/// Ein Form-XObject mit demselben Filter — der Leser ist derselbe.
#[test]
fn asciihex_in_einem_form_xobject_wird_gelesen_und_geschwaerzt() {
    let mut d = page(&["Kontoinhaber: Max Mustermann"]);
    let font_id = d.font_id;
    let form = format!("BT /F1 10 Tf 0 0 Td (IBAN: {SECRET}) Tj ET").into_bytes();
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 400.into(), 20.into()],
                "Filter" => "ASCIIHexDecode",
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            ascii_hex_encode(&form),
        )
        .with_compression(false),
    ));
    d.doc
        .get_dictionary_mut(d.resources_id)
        .expect("Resources")
        .set("XObject", dictionary! { "Fm0" => form_id });
    let mut raw = text_ops(&["Kontoinhaber: Max Mustermann"]);
    raw.extend_from_slice(b"q 1 0 0 1 72 500 cm /Fm0 Do Q\n");
    d.set_content(&raw);
    let bytes = d.finish();

    let runs = extract(&bytes);
    let redaction = redaction_for(&runs, SECRET);
    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, &[redaction])
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    assert!(leaks(&out, SECRET).is_empty(), "{:?}", leaks(&out, SECRET));
}

/// Gegenprobe: ein Filter, den weder dieses Modul noch `lopdf` kennt, bleibt
/// eine gemeldete Lücke — der neue Dekoder darf nichts still schlucken.
#[test]
fn ein_unbekannter_filter_an_einem_form_xobject_wird_weiterhin_gemeldet() {
    let mut d = page(&["Kontoinhaber: Max Mustermann"]);
    let font_id = d.font_id;
    let form = format!("BT /F1 10 Tf 0 0 Td (IBAN: {SECRET}) Tj ET").into_bytes();
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 400.into(), 20.into()],
                "Filter" => "JBIG2Decode",
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            form,
        )
        .with_compression(false),
    ));
    d.doc
        .get_dictionary_mut(d.resources_id)
        .expect("Resources")
        .set("XObject", dictionary! { "Fm0" => form_id });
    let mut raw = text_ops(&["Kontoinhaber: Max Mustermann"]);
    raw.extend_from_slice(b"q 1 0 0 1 72 500 cm /Fm0 Do Q\n");
    d.set_content(&raw);
    let doc = load_from_bytes(&d.finish()).expect("PDF ladbar");
    let (_, warnings) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion");
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("dekodieren") && w.contains("Fm0")),
        "{warnings:?}"
    );
}
