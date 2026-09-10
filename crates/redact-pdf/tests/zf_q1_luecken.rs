//! Gegenprüfung Fix-Runde 5 (Q1): **Befund Q1-1** — vier Klartexte, die der
//! neue Trägerlauf erreicht (oder erreichbar hält) und stehen lässt.
//!
//! Der Modulkommentar von `meta.rs` nennt genau **eine** Lücke
//! („**Benannte Lücke:** `/DA` … bleibt“) und sagt über die Dokumentstruktur,
//! der `/K`-Baum unter `/StructTreeRoot` „verwaist damit“. Beides hält an
//! diesem Material nicht:
//!
//! 1. `/Movie /F` — der Dateiname einer Movie-Annotation (PDF 32000-1,
//!    12.5.6.17, Tabelle 293/294) ist eine Dateiangabe und frei wählbarer
//!    Text.
//! 2. `/Measure /X … /U` — die Einheitenbeschriftung einer Vermessung
//!    (12.9, Tabelle 199) ist ein Textstring.
//! 3. `/RichMediaContent /Assets` — ein Namensbaum eingebetteter Dateien an
//!    der Annotation. `/Names /EmbeddedFiles` und `/FileAttachment` sind
//!    abgedeckt, dieser dritte Weg nicht.
//! 4. `/IRT` auf ein Struktur-Element: der Lauf **läuft dorthin** (seit
//!    dieser Runde), erkennt es als Nicht-Träger, lässt es stehen — und der
//!    Verweis hält es über `prune_unreachable` hinweg am Leben, samt `/Alt`.
//!
//! Alle vier enden mit Rückgabewert 0 und leerem bzw. schweigendem Bericht.
//! Deshalb sind diese Tests `#[ignore]` und rot; sie sind der Beleg.

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object, Stream};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata};

fn strip(bytes: &[u8]) -> Vec<u8> {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    strip_metadata(&mut doc);
    save_to_bytes(&doc).expect("Speichern")
}

#[track_caller]
fn muss_fallen(bytes: &[u8], was: &str) {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "{was}: die Probe muss das Geheimnis vorher tragen"
    );
    let out = strip(bytes);
    let hits = leaks(&out, SECRET);
    assert!(
        hits.is_empty(),
        "{was}: nach dem Metadatenlauf steht der Klartext noch in der Datei:\n{}",
        hits.join("\n")
    );
}

/// Befund Q1-1a: `/Movie /F`.
#[test]
#[ignore = "Befund Q1-1a: der Dateiname einer Movie-Annotation bleibt"]
fn der_dateiname_einer_movie_annotation_bleibt() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Movie",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "Movie" => dictionary! {
            "F" => Object::string_literal(format!("Kontoauszug {SECRET}.mov")),
            "Aspect" => vec![320.into(), 240.into()],
        },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
    muss_fallen(&d.finish(), "/Movie /F");
}

/// Befund Q1-1b: `/Measure /X /U`.
#[test]
#[ignore = "Befund Q1-1b: die Einheitenbeschriftung einer Vermessung bleibt"]
fn die_einheitenbeschriftung_einer_vermessung_bleibt() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "PolyLine",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "Vertices" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "Measure" => dictionary! {
            "Type" => "Measure",
            "R" => Object::string_literal("1 zu 1"),
            "X" => Object::Array(vec![Object::Dictionary(dictionary! {
                "U" => Object::string_literal(format!("Einheit {SECRET}")),
                "C" => Object::Integer(1),
            })]),
        },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
    muss_fallen(&d.finish(), "/Measure /X /U");
}

/// Befund Q1-1c: eine eingebettete Datei unter `/RichMediaContent /Assets`.
#[test]
#[ignore = "Befund Q1-1c: eingebettete Datei an einer RichMedia-Annotation bleibt"]
fn eine_eingebettete_datei_an_einer_richmedia_annotation_bleibt() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let strom = d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "EmbeddedFile" },
        format!("<daten>{SECRET}</daten>").into_bytes(),
    )));
    let filespec = d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("daten.xml"),
        "EF" => dictionary! { "F" => Object::Reference(strom) },
    }));
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "RichMedia",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "RichMediaContent" => dictionary! {
            "Assets" => dictionary! {
                "Names" => vec![
                    Object::string_literal("daten.xml"),
                    Object::Reference(filespec),
                ],
            },
        },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
    muss_fallen(&d.finish(), "/RichMediaContent /Assets");
}

/// Befund Q1-1d: `/IRT` hält ein Struktur-Element samt `/Alt` am Leben,
/// obwohl `/StructTreeRoot` fällt — der `/K`-Baum „verwaist“ eben nicht.
#[test]
#[ignore = "Befund Q1-1d: /Alt eines Struktur-Elements hinter /IRT bleibt"]
fn ein_strukturelement_hinter_irt_behaelt_seinen_alt_text() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let elem = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "P",
        "Alt" => Object::string_literal(format!("Kontoauszug {SECRET}")),
    }));
    let root = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructTreeRoot",
        "K" => Object::Reference(elem),
    }));
    d.catalog_set("StructTreeRoot", Object::Reference(root));
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "IRT" => Object::Reference(elem),
        "Contents" => Object::string_literal("Antwort"),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
    muss_fallen(&d.finish(), "/IRT auf ein Struktur-Element");
}
