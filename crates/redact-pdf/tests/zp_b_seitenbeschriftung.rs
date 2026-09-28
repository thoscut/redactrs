//! Register #92, zweiter Teil (Prüfer B-4, Spur-A-Runde 2): die
//! Seitenbeschriftung `/PageLabels` behielt Präfixe.
//!
//! Der Zahlenbaum `/PageLabels` (PDF 32000-1, 7.9.7 und Tabelle 159) trägt
//! je Bereich ein Beschriftungs-Dictionary; sein Präfix `/P` ist Freitext
//! („Kontoauszug DE89 … – “). Zwei Wege ließen ihn stehen, beide ohne
//! Warnung:
//!
//! * Nach dem **ersten direkten** Dictionary in `/Nums` brach die Schleife
//!   ab; jedes spätere Dictionary als eigenes Objekt blieb unbearbeitet.
//! * Ein Baum, der tiefer ging als eine feste Stufe, wurde dort still
//!   abgeschnitten; die Blätter darunter behielten ihr Präfix.
//!
//! Orakel ist [`redact_pdf::leaks`] an den geschriebenen Bytes.

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata};

fn nach_dem_lauf(d: &Doc) -> Vec<String> {
    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "die Probe muss das Geheimnis vorher tragen"
    );
    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    strip_metadata(&mut doc);
    leaks(&save_to_bytes(&doc).expect("Speichern"), SECRET)
}

fn praefix() -> Object {
    Object::string_literal(format!("Auszug {SECRET} - "))
}

/// `/Nums [0 <</S /r>> 1 6 0 R]`: das direkte Dictionary zuerst, das
/// indirekte danach. Vorher blieb das zweite stehen.
#[test]
fn ein_indirektes_praefix_nach_einem_direkten_faellt() {
    let mut d = page(&["Hallo"]);
    let label = d.add(Object::Dictionary(
        dictionary! { "S" => "D", "P" => praefix() },
    ));
    d.catalog_set(
        "PageLabels",
        Object::Dictionary(dictionary! {
            "Nums" => vec![
                0.into(),
                Object::Dictionary(dictionary! { "S" => "r" }),
                1.into(),
                Object::Reference(label),
            ],
        }),
    );
    let hits = nach_dem_lauf(&d);
    assert!(hits.is_empty(), "Präfix blieb stehen:\n{}", hits.join("\n"));
}

/// Die Gegenprobe in der anderen Reihenfolge, und zwei direkte
/// Dictionaries mit Präfix im selben Feld.
#[test]
fn direkte_und_indirekte_praefixe_in_jeder_reihenfolge_fallen() {
    let mut d = page(&["Hallo"]);
    let label = d.add(Object::Dictionary(
        dictionary! { "S" => "D", "P" => praefix() },
    ));
    d.catalog_set(
        "PageLabels",
        Object::Dictionary(dictionary! {
            "Nums" => vec![
                0.into(),
                Object::Reference(label),
                1.into(),
                Object::Dictionary(dictionary! { "S" => "r", "P" => praefix() }),
                2.into(),
                Object::Dictionary(dictionary! { "S" => "a", "P" => praefix() }),
            ],
        }),
    );
    let hits = nach_dem_lauf(&d);
    assert!(hits.is_empty(), "Präfix blieb stehen:\n{}", hits.join("\n"));
}

/// Ein Baum aus vierzig Stufen `/Kids`, das Blatt mit Präfix ganz unten —
/// jeder Knoten ein eigenes Objekt. Vorher endete der Lauf bei einer festen
/// Stufe, ohne es zu sagen.
#[test]
fn ein_tiefer_baum_verliert_das_praefix_im_blatt() {
    let mut d = page(&["Hallo"]);
    let mut knoten = d.add(Object::Dictionary(dictionary! {
        "Nums" => vec![0.into(), Object::Dictionary(dictionary! { "S" => "D", "P" => praefix() })],
    }));
    for _ in 0..40 {
        knoten = d.add(Object::Dictionary(dictionary! {
            "Kids" => vec![Object::Reference(knoten)],
        }));
    }
    d.catalog_set("PageLabels", Object::Reference(knoten));
    let hits = nach_dem_lauf(&d);
    assert!(
        hits.is_empty(),
        "Präfix im tiefen Blatt blieb stehen:\n{}",
        hits.join("\n")
    );
}

/// `/S` und `/St` bleiben: der Betrachter zählt weiter, nur ohne Präfix.
#[test]
fn stil_und_start_bleiben() {
    let mut d = page(&["Hallo"]);
    let label = d.add(Object::Dictionary(
        dictionary! { "S" => "D", "St" => 5, "P" => praefix() },
    ));
    d.catalog_set(
        "PageLabels",
        Object::Dictionary(dictionary! {
            "Nums" => vec![
                0.into(),
                Object::Dictionary(dictionary! { "S" => "r" }),
                1.into(),
                Object::Reference(label),
            ],
        }),
    );
    let mut doc = load_from_bytes(&d.finish()).expect("PDF ladbar");
    strip_metadata(&mut doc);
    let label = doc.get_dictionary(label).expect("Beschriftung bleibt");
    assert!(label.get(b"P").is_err());
    assert_eq!(
        label.get(b"S").and_then(Object::as_name).ok(),
        Some(&b"D"[..])
    );
    assert_eq!(label.get(b"St").and_then(Object::as_i64).ok(), Some(5));
}

/// Ein direkt eingebetteter Knoten unter `/Kids` (regelwidrig) wird in
/// seinem Elternknoten bereinigt; der Baum darüber bleibt, wie er war.
#[test]
fn ein_direkter_knoten_wird_in_seinem_elternknoten_bereinigt() {
    let mut d = page(&["Hallo"]);
    let eltern = d.add(Object::Dictionary(dictionary! {
        "Kids" => vec![Object::Dictionary(dictionary! {
            "Nums" => vec![0.into(), Object::Dictionary(dictionary! { "S" => "D", "P" => praefix() })],
        })],
    }));
    d.catalog_set("PageLabels", Object::Reference(eltern));
    let hits = nach_dem_lauf(&d);
    assert!(
        hits.is_empty(),
        "Präfix im direkten Knoten blieb stehen:\n{}",
        hits.join("\n")
    );

    let mut doc = load_from_bytes(&d.finish()).expect("PDF ladbar");
    strip_metadata(&mut doc);
    let catalog = doc.catalog().expect("Katalog");
    assert!(
        matches!(catalog.get(b"PageLabels"), Ok(Object::Reference(id)) if *id == eltern),
        "der Katalog zeigt weiter auf den Elternknoten"
    );
}
