//! Zwei Lücken an Annotationen, die der Gegenprüfer der Fix-Runde 2 offen
//! gelassen fand (Befund #18 a/c/d).
//!
//! **Tooltip und Exportname.** Ein Widget trägt seinen Feldnamen in `/T`,
//! seinen *alternativen* Feldnamen in `/TU` und seinen Exportnamen in `/TM`
//! (PDF 32000-1, 12.7.3.1, Tabelle 220). `/TU` ist der Tooltip, den der
//! Betrachter beim Überfahren zeigt — und Formulargeneratoren füllen ihn
//! mit dem Beschriftungstext: „Konto von Max Mustermann“. Bis zu dieser
//! Runde fielen `/Contents`, `/RC`, `/T` und `/Subj`; `/TU` und `/TM`
//! blieben. Gemessen (mit `TU` aus der Schlüsselliste genommen): nach
//! `strip_metadata` fand `leaks` die IBAN im `/TU` weiterhin in der
//! geschriebenen Datei.
//!
//! **Ein `/Dest` hinter einem Verweis.** Der Modulkommentar von `meta.rs`
//! versprach: „Ein `/Dest` als Feld bleibt.“ Entschieden wurde aber über die
//! *Schreibweise*: nur ein direkt eingebettetes Feld galt als ausdrückliches
//! Ziel, ein `/Dest 12 0 R` fiel — auch dann, wenn Objekt 12 genau das Feld
//! war. Ein Erzeuger, der jedes Ziel als eigenes Objekt ablegt, verlor damit
//! grundlos seine Sprünge. Jetzt wird der Verweis aufgelöst und über das
//! Feld entschieden: ein Feld dahinter bleibt, eine Zeichenkette dahinter
//! fällt. Beide Richtungen stehen hier — nur „bleibt“ zu prüfen ließe die
//! Mutation „jeder Verweis bleibt“ grün.
//!
//! ## Orakel
//!
//! Geprüft wird gegen die **geschriebenen Bytes** mit [`redact_pdf::leaks`],
//! nicht mit dem eigenen Extraktor — der sieht Annotationstexte ohnehin
//! nicht, und was er nicht sieht, könnte der Test auch nicht messen.

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport};

/// Der Metadatenlauf allein, so wie CLI und GUI ihn nach der Schwärzung
/// fahren: laden, bereinigen, schreiben.
fn strip(bytes: &[u8]) -> (MetadataReport, Vec<u8>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = strip_metadata(&mut doc);
    (report, save_to_bytes(&doc).expect("Speichern"))
}

/// Belegt zuerst, dass die Probe das Geheimnis trägt — sonst misst der Test
/// nichts —, und danach, dass es weg ist.
#[track_caller]
fn assert_gone(bytes: &[u8], what: &str) -> MetadataReport {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "{what}: die Probe muss das Geheimnis vorher tragen, sonst misst der Test nichts"
    );
    let (report, out) = strip(bytes);
    let hits = leaks(&out, SECRET);
    assert!(
        hits.is_empty(),
        "{what}: nach der Verarbeitung steht die IBAN noch in der Datei:\n{}",
        hits.join("\n")
    );
    report
}

fn with_annotation(annot: lopdf::Dictionary) -> (Doc, ObjectId) {
    let mut d = page(&["Harmloser Text"]);
    let id = d.add(Object::Dictionary(annot));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(id)]));
    (d, id)
}

fn link_rect() -> Object {
    Object::Array(vec![400.into(), 100.into(), 500.into(), 120.into()])
}

/// Ein ausdrückliches Ziel auf die eigene Seite, wie es ein
/// Inhaltsverzeichnis schreibt: `[Seite /XYZ 72 700 1.5]`.
fn explicit_destination(page_id: ObjectId) -> Object {
    Object::Array(vec![
        Object::Reference(page_id),
        Object::Name(b"XYZ".to_vec()),
        72.into(),
        700.into(),
        Object::Real(1.5),
    ])
}

// ---------------------------------------------------------------------------
// #18(a)(d): Tooltip und Exportname eines Widgets
// ---------------------------------------------------------------------------

#[test]
fn tooltip_und_exportname_eines_widgets_fallen() {
    let (d, id) = with_annotation(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Tx",
        "Rect" => link_rect(),
        // Der Feldname darf bleiben, wie er ist — er steht in `/T` und fällt
        // ohnehin. Die beiden neuen Träger tragen je das Geheimnis, damit
        // jede einzeln fehlende Entfernung den Test rot macht.
        "T" => Object::string_literal("iban"),
        "TU" => Object::string_literal(format!("Konto von {SECRET}")),
        "TM" => Object::string_literal(format!("export_{SECRET}")),
    });
    let report = assert_gone(&d.finish(), "/TU und /TM an einem Widget");
    // `/T`, `/TU` und `/TM` — drei Texte, kein Feldwert (das Widget trägt
    // kein `/V`).
    assert_eq!(report.annotation_texts_cleared, 3, "/T, /TU und /TM");
    assert_eq!(report.field_values_cleared, 0);
    // Die Berichtszeile nennt, was sie zählt.
    let line = report
        .summary()
        .into_iter()
        .find(|l| l.contains("Kommentartexte an Annotationen"))
        .expect("Berichtszeile für Annotationstexte");
    assert!(
        line.contains("/TU") && line.contains("/TM"),
        "die Berichtszeile verschweigt die neuen Träger: {line}"
    );

    // Das Widget selbst steht noch — nur seine Texte sind weg.
    let (_, out) = strip(&d.finish());
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    let widget = doc.get_dictionary(id).expect("Widget steht noch");
    for key in [b"T".as_slice(), b"TU", b"TM"] {
        assert!(
            !widget.has(key),
            "/{} steht noch am Widget",
            String::from_utf8_lossy(key)
        );
    }
}

/// Gegenprobe: `/TU` und `/TM` ohne Geheimnis fallen ebenso — die Regel
/// hängt am Schlüssel, nicht am Inhalt (der Text hat keine Glyphengeometrie
/// und kann nicht anteilig geschwärzt werden).
#[test]
fn tooltip_ohne_geheimnis_faellt_ebenso() {
    let (d, id) = with_annotation(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Tx",
        "Rect" => link_rect(),
        "TU" => Object::string_literal("Bitte Kontonummer eintragen"),
    });
    let (report, out) = strip(&d.finish());
    assert_eq!(report.annotation_texts_cleared, 1);
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert!(!doc.get_dictionary(id).expect("Widget").has(b"TU"));
}

// ---------------------------------------------------------------------------
// #18(c): ein `/Dest` hinter einem Verweis
// ---------------------------------------------------------------------------

#[test]
fn ein_dest_als_verweis_auf_ein_feld_bleibt() {
    let mut d = page(&["Harmloser Text"]);
    let dest_id = d.add(explicit_destination(d.page_id));
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => link_rect(),
        "Dest" => Object::Reference(dest_id),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));

    let (report, out) = strip(&d.finish());
    assert_eq!(
        report.annotation_actions_removed, 0,
        "ein ausdrückliches Ziel hinter einem Verweis zählt nicht als entfernte Aktion"
    );
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    let annot = doc.get_dictionary(annot_id).expect("Annotation steht noch");
    let dest = annot
        .get(b"Dest")
        .expect("das ausdrückliche Ziel bleibt, auch wenn es als Verweis geschrieben ist");
    // Und der Verweis führt weiterhin auf das Feld — nicht ins Leere.
    let (_, target) = doc.dereference(dest).expect("Ziel auflösbar");
    let items = target.as_array().expect("Ziel ist ein Feld");
    assert!(
        matches!(items.get(1), Some(Object::Name(n)) if n == b"XYZ"),
        "das Feld hinter dem Verweis ist nicht mehr das Ziel: {items:?}"
    );
}

#[test]
fn ein_dest_als_verweis_auf_eine_zeichenkette_faellt() {
    let mut d = page(&["Harmloser Text"]);
    // Ein benanntes Ziel — eine Zeichenkette — als eigenes Objekt abgelegt.
    let dest_id = d.add(Object::string_literal(format!("IBAN {SECRET}")));
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => link_rect(),
        "Dest" => Object::Reference(dest_id),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));

    let report = assert_gone(&d.finish(), "/Dest 13 0 R auf eine Zeichenkette");
    assert_eq!(report.annotation_actions_removed, 1);
    let (_, out) = strip(&d.finish());
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    let annot = doc.get_dictionary(annot_id).expect("Annotation steht noch");
    assert!(
        !annot.has(b"Dest"),
        "das benannte Ziel hinter dem Verweis steht noch"
    );
    assert!(
        doc.get_object(dest_id).is_err(),
        "die Zeichenkette lebt als verwaistes Objekt weiter"
    );
}

/// Gegenprobe: das direkt eingebettete Feld bleibt weiterhin — die Auflösung
/// hat den bisherigen Weg nicht verändert.
#[test]
fn ein_dest_als_direktes_feld_bleibt_weiterhin() {
    let mut d = page(&["Harmloser Text"]);
    let dest = explicit_destination(d.page_id);
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => link_rect(),
        "Dest" => dest,
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));

    let (report, out) = strip(&d.finish());
    assert_eq!(report.annotation_actions_removed, 0);
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert!(doc
        .get_dictionary(annot_id)
        .expect("Annotation steht noch")
        .has(b"Dest"));
}

/// Gegenprobe: ein Verweis, der auf ein Feld mit einem fremden Namen zeigt,
/// fällt wie das direkte Feld mit fremdem Namen — die Auflösung lockert die
/// Regel nicht, sie wendet sie nur auf den Inhalt an.
#[test]
fn ein_dest_als_verweis_auf_ein_feld_mit_fremdem_namen_faellt() {
    let mut d = page(&["Harmloser Text"]);
    let dest_id = d.add(Object::Array(vec![
        Object::Reference(d.page_id),
        Object::Name(format!("Konto_{}", SECRET.replace(' ', "_")).into_bytes()),
    ]));
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => link_rect(),
        "Dest" => Object::Reference(dest_id),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));

    let (report, out) = strip(&d.finish());
    assert_eq!(report.annotation_actions_removed, 1);
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert!(!doc
        .get_dictionary(annot_id)
        .expect("Annotation steht noch")
        .has(b"Dest"));
}
