//! Gegenprüfung G1 (Fix-Runde 3), Gebiet C: `filters::decoded_content` und
//! `/DecodeParms`.
//!
//! Grüne Tests belegen, was hält; `#[ignore]`-Tests sind Befunde (rot am
//! Stand `76bdcf9`). Orakel: der Klartext `PLAIN` muss aus
//! `decoded_content` kommen **und** derselbe Strom als Seiteninhalt muss
//! durch Extraktion und Schwärzung gehen, ohne Warnung, ohne Leck.

mod common;

use std::io::Write;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Dictionary, Object, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::filters::decoded_content;
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

const PLAIN: &[u8] = b"BT /F1 10 Tf 72 700 Td (IBAN: DE89 3704 0044 0532 0130 00) Tj ET";

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).expect("deflate");
    e.finish().expect("deflate")
}

fn hex(data: &[u8]) -> Vec<u8> {
    let mut s: String = data.iter().map(|b| format!("{b:02X}")).collect();
    s.push('>');
    s.into_bytes()
}

fn a85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let mut buf = [0u8; 4];
        buf[..chunk.len()].copy_from_slice(chunk);
        let mut v = u32::from_be_bytes(buf);
        if chunk.len() == 4 && v == 0 {
            out.push(b'z');
            continue;
        }
        let mut digits = [0u8; 5];
        for d in digits.iter_mut().rev() {
            *d = (v % 85) as u8 + b'!';
            v /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// PNG-Prädiktor „None“: ein Filterbyte 0 vor jeder Zeile von `columns` Byte.
fn png_rows(data: &[u8], columns: usize) -> Vec<u8> {
    let mut rows = Vec::new();
    for chunk in data.chunks(columns) {
        rows.push(0u8);
        rows.extend_from_slice(chunk);
    }
    rows
}

fn predictor() -> Dictionary {
    dictionary! { "Predictor" => 12, "Columns" => 8 }
}

fn stream(dict: Dictionary, content: Vec<u8>) -> Stream {
    Stream::new(dict, content).with_compression(false)
}

fn redactions_for(runs: &[TextRun]) -> Vec<Redaction> {
    runs.iter()
        .filter_map(|run| {
            let pos = run.text.find(SECRET)?;
            let rect = run.rect_for_byte_range(pos, pos + SECRET.len())?;
            Some(Redaction::new(
                Region::new(
                    run.page,
                    rect,
                    Some(SECRET.to_string()),
                    Source::Pattern {
                        pattern_id: "iban_de".into(),
                        confidence: 1.0,
                    },
                ),
                Action::Blackout,
            ))
        })
        .collect()
}

/// Der Strom als Seiteninhalt: Extraktion findet das Geheimnis genau
/// einmal ohne Warnung, die Schwärzung lässt kein Leck.
fn assert_end_to_end(label: &str, mut d: Doc, s: Stream) {
    d.doc.objects.insert(d.content_id, Object::Stream(s));
    let bytes = d.finish();
    let doc = load_from_bytes(&bytes).expect("ladbar");
    let (runs, warnings) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .unwrap_or_else(|e| panic!("{label}: abgelehnt: {e}"));
    assert!(warnings.is_empty(), "{label}: {warnings:?}");
    assert_eq!(
        runs.iter().filter(|r| r.text.contains(SECRET)).count(),
        1,
        "{label}: {runs:?}"
    );
    let mut doc = load_from_bytes(&bytes).expect("ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, &redactions_for(&runs))
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    let found = leaks(&out, SECRET);
    assert!(found.is_empty(), "{label}: {found:?}");
}

// ---------------------------------------------------------------------------
// Hält
// ---------------------------------------------------------------------------

/// Ein **einzelnes** `/DecodeParms`-Dictionary an einer Filterliste: es
/// gilt für jeden Index; `ASCIIHexDecode` ignoriert den Prädiktor, `Flate`
/// bekommt ihn.
#[test]
fn einzelnes_parms_dictionary_an_einer_filterliste() {
    let d = page(&[]);
    let s = stream(
        dictionary! {
            "Filter" => vec!["ASCIIHexDecode".into(), "FlateDecode".into()],
            "DecodeParms" => predictor(),
        },
        hex(&deflate(&png_rows(PLAIN, 8))),
    );
    assert_eq!(decoded_content(&d.doc, &s).as_deref(), Some(PLAIN));
    assert_end_to_end("einzelnes Dict", d, s);
}

/// Drei Filter (`ASCIIHex → ASCII85 → Flate`) mit dem Prädiktor am dritten.
#[test]
fn drei_filter_mit_praediktor_am_letzten() {
    let d = page(&[]);
    let s = stream(
        dictionary! {
            "Filter" => vec!["ASCIIHexDecode".into(), "ASCII85Decode".into(), "FlateDecode".into()],
            "DecodeParms" => vec![Object::Null, Object::Null, Object::Dictionary(predictor())],
        },
        hex(&a85(&deflate(&png_rows(PLAIN, 8)))),
    );
    assert_eq!(decoded_content(&d.doc, &s).as_deref(), Some(PLAIN));
    assert_end_to_end("drei Filter", d, s);
}

/// `/Filter /FlateDecode` mit `/DecodeParms [<<…>>]` — ein Array mit einem
/// Eintrag, wie es manche Erzeuger schreiben. `lopdf` allein liest das
/// nicht als Dictionary (Prädiktor fällt weg); hier kommt der Klartext.
#[test]
fn parms_array_mit_einem_eintrag_bei_einem_filter() {
    let d = page(&[]);
    let s = stream(
        dictionary! {
            "Filter" => "FlateDecode",
            "DecodeParms" => vec![Object::Dictionary(predictor())],
        },
        deflate(&png_rows(PLAIN, 8)),
    );
    assert_ne!(
        s.decompressed_content().ok().as_deref(),
        Some(PLAIN),
        "lopdf allein: sonst prüft der Test nichts"
    );
    assert_eq!(decoded_content(&d.doc, &s).as_deref(), Some(PLAIN));
    assert_end_to_end("Array mit einem Eintrag", d, s);
}

/// Gegenrichtung: `/DecodeParms null`, Array kürzer als die Filterliste,
/// leeres Array, Verweis ins Leere, ohne Parms — gewöhnliche Dateien,
/// nichts davon darf abgelehnt werden oder warnen.
#[test]
fn gewoehnliche_parms_formen_werden_nicht_abgelehnt() {
    let cases: Vec<(&str, Dictionary, Vec<u8>)> = vec![
        (
            "null",
            dictionary! { "Filter" => "FlateDecode", "DecodeParms" => Object::Null },
            deflate(PLAIN),
        ),
        (
            "Array kürzer",
            dictionary! {
                "Filter" => vec!["ASCIIHexDecode".into(), "FlateDecode".into()],
                "DecodeParms" => vec![Object::Null],
            },
            hex(&deflate(PLAIN)),
        ),
        (
            "leeres Array",
            dictionary! { "Filter" => "FlateDecode", "DecodeParms" => Vec::<Object>::new() },
            deflate(PLAIN),
        ),
        (
            "Verweis ins Leere",
            dictionary! { "Filter" => "FlateDecode", "DecodeParms" => Object::Reference((999, 0)) },
            deflate(PLAIN),
        ),
        (
            "ohne Parms, Filterliste",
            dictionary! { "Filter" => vec!["FlateDecode".into()] },
            deflate(PLAIN),
        ),
    ];
    for (label, dict, content) in cases {
        let d = page(&[]);
        let s = stream(dict, content);
        assert_eq!(
            decoded_content(&d.doc, &s).as_deref(),
            Some(PLAIN),
            "{label}"
        );
        assert_end_to_end(label, d, s);
    }
}

// ---------------------------------------------------------------------------
// Befunde — rot am Stand 76bdcf9, seit Fix-Runde 4 scharf
// ---------------------------------------------------------------------------

/// **Befund G1-C1 (klein).** `/Filter 5 0 R` — der Filtername als
/// indirekter Verweis (PDF 32000-1 lässt jeden Wert eines Dictionaries als
/// Verweis zu). `Stream::filters()` von `lopdf` liest keinen Verweis, die
/// Rohbytes gelten als Klartext, die Seite wird als „nicht zerlegbar“
/// abgelehnt: Rückgabewert 1 für eine gültige Datei. Das Orakel sieht die
/// IBAN (es versucht Flate an jedem Strom), der Schwärzer nicht.
#[test]
fn befund_filter_als_verweis_wird_abgelehnt() {
    let mut d = page(&[]);
    let name = d.add(Object::Name(b"FlateDecode".to_vec()));
    let s = stream(dictionary! { "Filter" => name }, deflate(PLAIN));
    assert_eq!(decoded_content(&d.doc, &s).as_deref(), Some(PLAIN));
    assert_end_to_end("/Filter als Verweis", d, s);
}

/// **Befund G1-C2 (klein).** Werte **innerhalb** des `/DecodeParms`-Dictionarys
/// als Verweis (`/Columns 7 0 R`, `/Predictor 7 0 R`): aufgelöst wird die
/// Liste und der Eintrag, nicht der Wert. `lopdf` liest `Columns` dann als
/// 1 (falsche Zeilenlänge, `None`), und ein `/Predictor` als Verweis gilt als
/// „kein Prädiktor“ — die Filterbytes bleiben im Text. Beides endet als
/// Ablehnung (Rückgabewert 1) einer gültigen Datei; das Orakel findet die
/// IBAN dort ebenfalls nicht.
#[test]
fn befund_werte_im_parms_dictionary_als_verweis() {
    for key in ["Columns", "Predictor"] {
        let mut d = page(&[]);
        let parms = if key == "Columns" {
            let c = d.add(Object::Integer(8));
            dictionary! { "Predictor" => 12, "Columns" => c }
        } else {
            let p = d.add(Object::Integer(12));
            dictionary! { "Predictor" => p, "Columns" => 8 }
        };
        let s = stream(
            dictionary! { "Filter" => "FlateDecode", "DecodeParms" => parms },
            deflate(&png_rows(PLAIN, 8)),
        );
        assert_eq!(decoded_content(&d.doc, &s).as_deref(), Some(PLAIN), "{key}");
        assert_end_to_end(key, d, s);
    }
}

/// **Befund G1-C3.** Ein anderer Weg zum Seiteninhalt: `image.rs` liest
/// die Seite über `lopdf::Document::get_page_content`, nicht über
/// `filters::page_content`. Auf einer `ASCIIHexDecode`-kodierten Seite
/// findet die Bildschwärzung deshalb **kein** Bild: die Fläche bekommt ihr
/// Deckrechteck, die Bildpunkte bleiben unverändert in der Datei, der
/// Bericht zählt 0 geschwärzte Bilder und warnt nicht. Auf derselben Seite
/// ohne Filter wird das Bild überschrieben.
#[test]
fn befund_bild_auf_hex_kodierter_seite_bleibt_unveraendert() {
    const PIXELS: [u8; 4] = [0x80, 0x80, 0x80, 0x80];
    for encoded in [false, true] {
        let mut d = page(&[]);
        let image_id = d.add(Object::Stream(
            Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 2,
                    "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
                },
                PIXELS.to_vec(),
            )
            .with_compression(false),
        ));
        d.doc
            .get_dictionary_mut(d.resources_id)
            .expect("Resources")
            .set("XObject", dictionary! { "Im0" => image_id });
        let raw = b"q 200 0 0 100 72 500 cm /Im0 Do Q\n".to_vec();
        let s = if encoded {
            stream(dictionary! { "Filter" => "ASCIIHexDecode" }, hex(&raw))
        } else {
            stream(dictionary! {}, raw)
        };
        d.doc.objects.insert(d.content_id, Object::Stream(s));
        let bytes = d.finish();
        let mut doc = load_from_bytes(&bytes).expect("ladbar");
        let over_image = Redaction::new(
            Region::new(
                0,
                redact_core::Rect::new(72.0, 500.0, 272.0, 600.0),
                None,
                Source::Manual {
                    reason: "Bild".into(),
                },
            ),
            Action::Blackout,
        );
        let report = PdfRedactor::new()
            .apply_with_report(&mut doc, &[over_image])
            .expect("Schwärzung");
        let after: Vec<Vec<u8>> = doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .filter(|s| s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image"))
            .map(|s| s.content.clone())
            .collect();
        assert_eq!(
            report.redacted_images, 1,
            "encoded={encoded}: Bericht {report:?}"
        );
        assert!(
            after.iter().all(|c| c != &PIXELS),
            "encoded={encoded}: die Bildpunkte stehen unverändert in der Datei"
        );
    }
}
