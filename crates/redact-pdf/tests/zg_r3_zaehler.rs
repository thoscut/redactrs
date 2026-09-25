//! Gegenprüfung Fix-Runde 6 (R3): die Zählung **an der aufgeräumten Datei**
//! (`Tally`, `settled`, `Cleaned::alive`).
//!
//! `MetadataReport` verspricht: keine Zahl meldet eine Entfernung, die
//! `--check-leaks` noch findet — und zwar für **jede** Zahl. Die Gegenrichtung
//! („zu klein“) ist erlaubt. Hier stehen die Fälle, an denen sich beides
//! misst:
//!
//! * ein zweiter Halter (die Zahl muss 0 sein — und 0 ist richtig, der Text
//!   steht noch da),
//! * eine Verweiskette (`/Contents 4 0 R`, Objekt 4 ist selbst `5 0 R`),
//! * zwei gefallene Schlüssel auf dasselbe Objekt,
//! * ein geteilter `/Title` an Lesezeichen,
//! * `optional_content_names_cleared` mit `/Name 12 0 R`.
//!
//! Zweites Orakel neben dem Bericht: der eigene Zähler (welche Objekt-Ids
//! stehen nach dem Lauf noch in der geschriebenen Datei) und `leaks`.

mod common;

use std::collections::BTreeSet;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport};

fn strip(bytes: &[u8]) -> (MetadataReport, Vec<u8>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = strip_metadata(&mut doc);
    (report, save_to_bytes(&doc).expect("Speichern"))
}

/// Der eigene Zähler: welche Objekt-Ids stehen nach dem Lauf noch in der
/// geschriebenen Datei?
fn ids_in(bytes: &[u8]) -> BTreeSet<ObjectId> {
    load_from_bytes(bytes)
        .expect("Ausgabe ladbar")
        .objects
        .keys()
        .copied()
        .collect()
}

fn notiz(d: &mut Doc, contents: Object) -> ObjectId {
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "Contents" => contents,
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
    a
}

// ---------------------------------------------------------------------------
// A) Zweiter Halter: 0 — und 0 ist richtig
// ---------------------------------------------------------------------------

/// `/Contents 4 0 R` fällt, eine Eigenschaftsliste der Seite hält Objekt 4. Der
/// Schlüssel ist weg, der Text nicht: der Bericht sagt 0, der eigene Zähler
/// sieht Objekt 4, `leaks` findet den Text. Alle drei stimmen überein.
///
/// Mutation, die diesen Test rot macht: `Tally::settled` ohne `alive`
/// (jeder Verweis zählt) — dann meldet der Bericht 1 über einen Text, der
/// noch da ist.
#[test]
fn zweiter_halter_bericht_null_eigener_zaehler_sieht_das_objekt() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let text = d.add(Object::string_literal(format!("Notiz {SECRET}")));
    let a = notiz(&mut d, Object::Reference(text));
    d.zweiter_halter(Object::Reference(text));

    let (report, out) = strip(&d.finish());
    let ids = ids_in(&out);
    assert!(ids.contains(&text), "der zweite Halter hält Objekt 4");
    let annot = load_from_bytes(&out)
        .unwrap()
        .get_dictionary(a)
        .unwrap()
        .clone();
    assert!(
        annot.get(b"Contents").is_err(),
        "der Schlüssel selbst ist gefallen"
    );
    assert!(
        !leaks(&out, SECRET).is_empty(),
        "der Text steht noch in der Datei"
    );
    assert_eq!(
        report.annotation_texts_cleared,
        0,
        "ein Text, den --check-leaks findet, darf nicht als entfernt gemeldet werden: {:?}",
        report.summary()
    );
}

/// Gegenprobe ohne zweiten Halter: 1, Objekt 4 weg, kein Fund.
#[test]
fn ohne_zweiten_halter_eins_und_das_objekt_ist_weg() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let text = d.add(Object::string_literal(format!("Notiz {SECRET}")));
    notiz(&mut d, Object::Reference(text));

    let (report, out) = strip(&d.finish());
    assert!(!ids_in(&out).contains(&text));
    assert!(leaks(&out, SECRET).is_empty());
    assert_eq!(report.annotation_texts_cleared, 1);
}

// ---------------------------------------------------------------------------
// B) Verweiskette
// ---------------------------------------------------------------------------

/// `/Contents 4 0 R`, Objekt 4 ist selbst der Verweis `5 0 R`, Objekt 5 die
/// Zeichenkette — und eine Eigenschaftsliste der Seite hält **5**. `Tally` merkt
/// sich 4; 4 fällt beim Aufräumen (niemand hält es), 5 bleibt. Der Bericht
/// meldet 1 — über einen Text, den `--check-leaks` findet.
#[test]
fn verweiskette_meldet_eine_entfernung_ueber_text_der_bleibt() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let ziel = d.add(Object::string_literal(format!("Notiz {SECRET}")));
    let mitte = d.add(Object::Reference(ziel));
    notiz(&mut d, Object::Reference(mitte));
    d.zweiter_halter(Object::Reference(ziel));

    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "Probe trägt das Geheimnis"
    );
    let (report, out) = strip(&bytes);
    let ids = ids_in(&out);
    let hits = leaks(&out, SECRET);
    assert!(
        report.annotation_texts_cleared == 0 || hits.is_empty(),
        "Bericht: {} Kommentartext(e) entfernt; Objekt 4 (Mitte) steht noch: {}; \
         Objekt 5 (Ziel) steht noch: {}; Fundstellen:\n{}",
        report.annotation_texts_cleared,
        ids.contains(&mitte),
        ids.contains(&ziel),
        hits.join("\n")
    );
}

// ---------------------------------------------------------------------------
// C) Zwei gefallene Schlüssel auf dasselbe Objekt
// ---------------------------------------------------------------------------

/// `/Contents 4 0 R` und `/Subj 4 0 R` an derselben Notiz. Zwei Schlüssel
/// fielen, Objekt 4 ist weg: 2 — „je Schlüssel einer“, wie dokumentiert.
/// Kein Doppelzählen im Sinne der Zusicherung (nichts wird gemeldet, das
/// noch da wäre).
#[test]
fn zwei_gefallene_schluessel_auf_dasselbe_objekt_zaehlen_je_schluessel() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let text = d.add(Object::string_literal(format!("Notiz {SECRET}")));
    let a = notiz(&mut d, Object::Reference(text));
    d.doc
        .get_dictionary_mut(a)
        .unwrap()
        .set("Subj", Object::Reference(text));

    let (report, out) = strip(&d.finish());
    assert!(!ids_in(&out).contains(&text));
    assert!(leaks(&out, SECRET).is_empty());
    assert_eq!(report.annotation_texts_cleared, 2, "{:?}", report.summary());
}

// ---------------------------------------------------------------------------
// C2) Der Zähler der Dateiverweise
// ---------------------------------------------------------------------------

fn zugeordnete_datei(d: &mut Doc) -> (ObjectId, ObjectId) {
    let strom = d.add(Object::Stream(lopdf::Stream::new(
        dictionary! { "Type" => "EmbeddedFile", "Subtype" => "text/xml" },
        format!("<ram:IBANID>{SECRET}</ram:IBANID>").into_bytes(),
    )));
    let filespec = d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("factur-x.xml"),
        "AFRelationship" => "Alternative",
        "EF" => dictionary! { "F" => Object::Reference(strom) },
    }));
    d.catalog_set("AF", Object::Array(vec![Object::Reference(filespec)]));
    (filespec, strom)
}

/// `/AF` am Katalog fällt, die Datei dahinter auch: der Bericht sagt es.
#[test]
fn eine_zugeordnete_datei_faellt_und_wird_gemeldet() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let (filespec, strom) = zugeordnete_datei(&mut d);

    let (report, out) = strip(&d.finish());
    let ids = ids_in(&out);
    assert!(!ids.contains(&filespec) && !ids.contains(&strom));
    assert!(leaks(&out, SECRET).is_empty());
    assert_eq!(report.file_specs_removed, 1, "{:?}", report.summary());
    assert!(report
        .summary()
        .iter()
        .any(|zeile| zeile.contains("Dateiverweis (/AF, /FS)")));
}

/// Dieselbe zugeordnete Datei, von einem zweiten Halter gehalten: der
/// Schlüssel fällt, die Datei bleibt — und der Bericht meldet **nichts**.
///
/// Mutation, die diesen Test rot macht: `Tally::settled` ohne `alive`.
#[test]
fn eine_gehaltene_zugeordnete_datei_wird_nicht_gemeldet() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let (filespec, _) = zugeordnete_datei(&mut d);
    d.zweiter_halter(Object::Reference(filespec));

    let (report, out) = strip(&d.finish());
    assert!(ids_in(&out).contains(&filespec));
    assert!(
        !leaks(&out, SECRET).is_empty(),
        "die Datei steht noch in der Ausgabe"
    );
    assert_eq!(
        report.file_specs_removed,
        0,
        "eine Datei, die --check-leaks findet, darf nicht als entfernt gemeldet werden: {:?}",
        report.summary()
    );
}

// ---------------------------------------------------------------------------
// D) Lesezeichen mit geteiltem /Title
// ---------------------------------------------------------------------------

fn lesezeichen(d: &mut Doc, title: Object) -> (ObjectId, ObjectId) {
    let root = d.doc.new_object_id();
    let i1 = d.doc.new_object_id();
    let i2 = d.doc.new_object_id();
    d.doc.objects.insert(
        i1,
        Object::Dictionary(dictionary! {
            "Title" => title.clone(),
            "Parent" => Object::Reference(root),
            "Next" => Object::Reference(i2),
        }),
    );
    d.doc.objects.insert(
        i2,
        Object::Dictionary(dictionary! {
            "Title" => title,
            "Parent" => Object::Reference(root),
            "Prev" => Object::Reference(i1),
        }),
    );
    d.doc.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => Object::Reference(i1),
            "Last" => Object::Reference(i2),
            "Count" => 2,
        }),
    );
    d.catalog_set("Outlines", Object::Reference(root));
    (i1, i2)
}

/// Zwei Lesezeichen teilen `/Title 4 0 R`, die Seite hält Objekt 4. Der
/// Titel bleibt in der Datei — kein Lesezeichen gilt als entfernt.
///
/// Mutation, die diesen Test rot macht: `Cleaned::alive` immer 0 — dann
/// meldet der Bericht 2 Lesezeichen über einen Text, der noch da ist.
#[test]
fn geteilter_titel_mit_zweitem_halter_kein_lesezeichen_gilt_als_entfernt() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let title = d.add(Object::string_literal(format!("Kontoauszug {SECRET}")));
    lesezeichen(&mut d, Object::Reference(title));
    d.zweiter_halter(Object::Reference(title));

    let (report, out) = strip(&d.finish());
    assert!(
        ids_in(&out).contains(&title),
        "der zweite Halter hält den Titel"
    );
    assert!(
        !leaks(&out, SECRET).is_empty(),
        "der Titel steht noch in der Datei"
    );
    assert_eq!(report.outlines_removed, 0, "{:?}", report.summary());
}

/// Ohne zweiten Halter fällt der geteilte Titel mit beiden Einträgen: 2.
#[test]
fn geteilter_titel_ohne_zweiten_halter_beide_lesezeichen_entfernt() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let title = d.add(Object::string_literal(format!("Kontoauszug {SECRET}")));
    lesezeichen(&mut d, Object::Reference(title));

    let (report, out) = strip(&d.finish());
    assert!(!ids_in(&out).contains(&title));
    assert!(leaks(&out, SECRET).is_empty());
    assert_eq!(report.outlines_removed, 2, "{:?}", report.summary());
}

// ---------------------------------------------------------------------------
// E) Ebenennamen: /Name 12 0 R
// ---------------------------------------------------------------------------

fn ebene_mit_verweisnamen(d: &mut Doc) -> ObjectId {
    let name = d.add(Object::string_literal(format!("Ebene {SECRET}")));
    let ocg = d.add(Object::Dictionary(dictionary! {
        "Type" => "OCG",
        "Name" => Object::Reference(name),
    }));
    let resources_id = d.resources_id;
    d.doc.get_dictionary_mut(resources_id).unwrap().set(
        "Properties",
        dictionary! { "oc1" => Object::Reference(ocg) },
    );
    d.set_content(b"/OC /oc1 BDC BT /F1 10 Tf 72 700 Td (Rechnung 4711) Tj ET EMC\n");
    name
}

/// Die Doku sagt: `optional_content_names_cleared` ist bei `/Name 12 0 R`
/// planmäßig zu klein — der Name wird ersetzt, das Objekt erst beim
/// Schreiben weggeräumt. Gemessen: der Name ist aus der Ausgabe verschwunden,
/// der Bericht meldet 0 — und `summary()` ist **leer**: die Konsole sagt
/// „Metadaten: nichts zu entfernen“ über eine Datei, aus der gerade ein
/// Ebenenname entfernt wurde.
#[test]
fn ebenenname_als_verweis_wird_entfernt_aber_nicht_gemeldet() {
    let mut d: Doc = page(&[]);
    let name = ebene_mit_verweisnamen(&mut d);

    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "Probe trägt das Geheimnis"
    );
    let (report, out) = strip(&bytes);
    assert!(
        !ids_in(&out).contains(&name),
        "das Namensobjekt ist aus der Ausgabe weg"
    );
    assert!(
        leaks(&out, SECRET).is_empty(),
        "der Name steht nicht mehr in der Datei"
    );
    assert!(
        report.anything_removed(),
        "ein Ebenenname ist gefallen, die Zusammenfassung sagt „nichts zu entfernen“: {:?}",
        report.summary()
    );
}

/// Derselbe Name, gehalten von einem zweiten Halter: 0 ist dann richtig —
/// der Text steht noch in der Datei.
#[test]
fn ebenenname_als_verweis_mit_zweitem_halter_bleibt_und_zaehlt_nicht() {
    let mut d: Doc = page(&[]);
    let name = ebene_mit_verweisnamen(&mut d);
    d.zweiter_halter(Object::Reference(name));

    let (report, out) = strip(&d.finish());
    assert!(ids_in(&out).contains(&name));
    assert!(!leaks(&out, SECRET).is_empty());
    assert_eq!(
        report.optional_content_names_cleared,
        0,
        "{:?}",
        report.summary()
    );
}
