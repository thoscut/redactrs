//! Gegenprüfung Fix-Runde 5 (Q1): **Befund Q1-2** — der Maßstab, den
//! `MetadataReport` sich selbst setzt, hält nicht.
//!
//! Der Doc-Kommentar von `MetadataReport` endet mit einem absoluten Satz:
//!
//! > Der Maßstab ist `--check-leaks` an der geschriebenen Datei: **keine
//! > Zahl** hier darf eine Entfernung melden, die dort noch zu finden ist.
//!
//! Das gilt für die vier Nutzlast-Zähler (die zählen nach `prune_unreachable`)
//! — für die Schlüssel-Zähler nicht. Steht der Text hinter einem **indirekten**
//! Verweis, den noch etwas anderes hält, fällt nur der Schlüssel: der Bericht
//! meldet die Entfernung, `--check-leaks` findet den Text.
//!
//! `#[ignore]`, weil rot: der Beleg.

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport};

fn strip(bytes: &[u8]) -> (MetadataReport, Vec<u8>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = strip_metadata(&mut doc);
    (report, save_to_bytes(&doc).expect("Speichern"))
}

/// Ein Lesezeichen, dessen `/Title` ein **eigenes Objekt** ist, das noch
/// jemand anders hält. Der Bericht meldet „1 Lesezeichen“, der Titel steht
/// weiter in der Datei.
#[test]
#[ignore = "Befund Q1-2a: outlines_removed meldet eine Entfernung, die --check-leaks findet"]
fn ein_geteilter_lesezeichentitel_darf_nicht_als_entfernt_gelten() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let titel = d.add(Object::string_literal(format!("Kontoauszug {SECRET}")));
    let eintrag = d.doc.new_object_id();
    let wurzel = d.add(Object::Dictionary(dictionary! {
        "Type" => "Outlines",
        "First" => Object::Reference(eintrag),
        "Last" => Object::Reference(eintrag),
        "Count" => 1_i64,
    }));
    d.doc.objects.insert(
        eintrag,
        Object::Dictionary(dictionary! {
            "Title" => Object::Reference(titel),
            "Parent" => Object::Reference(wurzel),
        }),
    );
    d.catalog_set("Outlines", Object::Reference(wurzel));
    // Der zweite Halter: irgendein Objekt der Seite nennt denselben String.
    d.page_dict_set("Zusatz", Object::Reference(titel));

    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "Probe trägt das Geheimnis"
    );
    let (report, out) = strip(&bytes);
    let hits = leaks(&out, SECRET);

    assert!(
        report.outlines_removed == 0 || hits.is_empty(),
        "der Bericht meldet {} Lesezeichen als entfernt, --check-leaks findet den Titel noch:\n{}",
        report.outlines_removed,
        hits.join("\n")
    );
}

/// Dasselbe an einer Annotation: `/Contents` ist ein eigenes Objekt, das noch
/// jemand anders hält. Der Bericht meldet „1 Kommentartext“.
#[test]
#[ignore = "Befund Q1-2b: annotation_texts_cleared meldet eine Entfernung, die --check-leaks findet"]
fn ein_geteilter_annotationstext_darf_nicht_als_entfernt_gelten() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let text = d.add(Object::string_literal(format!("Notiz {SECRET}")));
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "Contents" => Object::Reference(text),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
    d.page_dict_set("Zusatz", Object::Reference(text));

    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "Probe trägt das Geheimnis"
    );
    let (report, out) = strip(&bytes);
    let hits = leaks(&out, SECRET);

    assert!(
        report.annotation_texts_cleared == 0 || hits.is_empty(),
        "der Bericht meldet {} Kommentartexte als entfernt, --check-leaks findet den Text noch:\n{}",
        report.annotation_texts_cleared,
        hits.join("\n")
    );
}
