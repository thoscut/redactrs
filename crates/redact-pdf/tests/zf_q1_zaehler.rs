//! Gegenprüfung Fix-Runde 5 (Q1): die Zähler des Metadatenlaufs.
//!
//! `MetadataReport` verspricht für `embedded_files_removed`,
//! `javascript_removed`, `xfa_removed` und `outlines_removed`: gezählt wird
//! **nach** `prune_unreachable`, und gemeldet wird nur, was dann wirklich
//! fehlt. Hier steht Material, in dem ein zweiter Halter den Teilbaum am
//! Leben hält — der Bericht muss dann 0 melden — und die Gegenprobe, in der
//! alles fällt.
//!
//! Zweites Orakel neben dem Bericht: ein **eigener** Zähler, der den
//! Objektgraphen vor und nach dem Lauf vergleicht, und `leaks` an den
//! geschriebenen Bytes.

mod common;

use std::collections::BTreeSet;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Document, Object, ObjectId, Stream};
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

fn secret_stream(doc: &mut Document) -> ObjectId {
    doc.add_object(Object::Stream(Stream::new(
        dictionary! {},
        format!("Anhang: {SECRET}").into_bytes(),
    )))
}

/// Ein `/Names`-Baum mit **einem** Eintrag unter `key`.
fn names_tree(doc: &mut Document, key: &str, name: &str, value: ObjectId) -> ObjectId {
    let leaf = doc.add_object(dictionary! {
        "Names" => vec![Object::string_literal(name), Object::Reference(value)],
    });
    doc.add_object(dictionary! { key => Object::Reference(leaf) })
}

// ---------------------------------------------------------------------------
// A) Ein zweiter Halter — der Bericht darf nichts melden
// ---------------------------------------------------------------------------

/// Ein Dateianhang, dessen Filespec zusätzlich an einem Schlüssel der Seite
/// hängt, den dieser Lauf nicht anfasst. `/Names` fällt, der Stream bleibt —
/// und mit ihm der Klartext.
///
/// Erwartung laut `MetadataReport`: `embedded_files_removed == 0`.
///
/// (Der zweite Halter war bis Fix-Runde 7 ein `/AF` an der Seite — der
/// ZUGFeRD-Weg. Der fällt jetzt selbst, siehe
/// `zg_r3_beiwerk::zugeordnete_datei_an_seite_und_annotation_bleibt`; als
/// *Halter* taugt er deshalb nicht mehr.)
#[test]
fn ein_anhang_mit_zweitem_halter_wird_nicht_als_entfernt_gemeldet() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let stream = secret_stream(&mut d.doc);
    let filespec = d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("anhang.txt"),
        "EF" => dictionary! { "F" => Object::Reference(stream) },
    }));
    let names = names_tree(&mut d.doc, "EmbeddedFiles", "anhang.txt", filespec);
    d.catalog_set("Names", Object::Reference(names));
    // Der zweite Halter: irgendetwas außerhalb des Metadatenlaufs.
    d.zweiter_halter(Object::Reference(filespec));

    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "Probe trägt das Geheimnis"
    );
    let (report, out) = strip(&bytes);

    // Eigener Zähler: der Stream steht noch in der Ausgabe.
    assert!(
        ids_in(&out).contains(&stream),
        "der Anhang-Stream müsste den Lauf überleben (er hängt an /AF)"
    );
    assert!(
        !leaks(&out, SECRET).is_empty(),
        "und damit auch sein Klartext"
    );
    assert_eq!(
        report.embedded_files_removed,
        0,
        "der Bericht meldet eine Entfernung, die nicht stattfand: {:?}",
        report.summary()
    );
}

/// Ein `/JavaScript`-Eintrag, dessen Quelltext als Stream zugleich an einem
/// Schlüssel der Seite hängt. Der Name fällt mit `/Names`, der Quelltext
/// bleibt.
#[test]
fn ein_javascript_mit_zweitem_halter_wird_nicht_als_entfernt_gemeldet() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let js = d.add(Object::Stream(Stream::new(
        dictionary! {},
        format!("var iban = \"{SECRET}\";").into_bytes(),
    )));
    let action = d.add(Object::Dictionary(dictionary! {
        "S" => "JavaScript",
        "JS" => Object::Reference(js),
    }));
    let names = names_tree(&mut d.doc, "JavaScript", "start", action);
    d.catalog_set("Names", Object::Reference(names));
    d.zweiter_halter(Object::Reference(js));

    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "Probe trägt das Geheimnis"
    );
    let (report, out) = strip(&bytes);

    assert!(
        ids_in(&out).contains(&js),
        "der JavaScript-Strom müsste den Lauf überleben"
    );
    assert_eq!(
        report.javascript_removed,
        0,
        "der Bericht meldet eine Entfernung, die nicht stattfand: {:?}",
        report.summary()
    );
}

/// `/XFA` als Array aus Paaren, von dem ein Strom geteilt ist.
#[test]
fn ein_geteilter_xfa_strom_wird_nicht_als_entfernt_gemeldet() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let preamble = d.add(Object::Stream(Stream::new(
        dictionary! {},
        b"<xdp:xdp>".to_vec(),
    )));
    let dataset = d.add(Object::Stream(Stream::new(
        dictionary! {},
        format!("<konto>{SECRET}</konto>").into_bytes(),
    )));
    let acroform = d.add(Object::Dictionary(dictionary! {
        "Fields" => Object::Array(Vec::new()),
        "XFA" => Object::Array(vec![
            Object::string_literal("preamble"),
            Object::Reference(preamble),
            Object::string_literal("datasets"),
            Object::Reference(dataset),
        ]),
    }));
    d.catalog_set("AcroForm", Object::Reference(acroform));
    d.zweiter_halter(Object::Reference(dataset));

    let bytes = d.finish();
    let (report, out) = strip(&bytes);

    assert!(
        ids_in(&out).contains(&dataset),
        "der geteilte XFA-Strom müsste den Lauf überleben"
    );
    assert!(
        !report.xfa_removed,
        "der Bericht meldet /XFA als entfernt, obwohl der Datensatz noch dasteht: {:?}",
        report.summary()
    );
}

// ---------------------------------------------------------------------------
// B) Gegenprobe: alles weg — dann muss der Bericht es auch sagen
// ---------------------------------------------------------------------------

/// Dieselben drei Nutzlasten **ohne** zweiten Halter. Eigener Zähler:
/// sämtliche Ids des Teilbaums sind aus der Ausgabe verschwunden.
#[test]
fn ohne_zweiten_halter_meldet_der_bericht_die_entfernung() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let stream = secret_stream(&mut d.doc);
    let filespec = d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("anhang.txt"),
        "EF" => dictionary! { "F" => Object::Reference(stream) },
    }));
    let embedded = names_tree(&mut d.doc, "EmbeddedFiles", "anhang.txt", filespec);
    let js = d.add(Object::Dictionary(dictionary! {
        "S" => "JavaScript",
        "JS" => Object::string_literal(format!("var iban = \"{SECRET}\";")),
    }));
    let leaf = d.add(Object::Dictionary(dictionary! {
        "Names" => vec![Object::string_literal("start"), Object::Reference(js)],
    }));
    let names = d.add(Object::Dictionary(dictionary! {
        "EmbeddedFiles" => d.doc.get_dictionary(embedded).unwrap().get(b"EmbeddedFiles").unwrap().clone(),
        "JavaScript" => Object::Reference(leaf),
    }));
    d.catalog_set("Names", Object::Reference(names));
    let dataset = d.add(Object::Stream(Stream::new(
        dictionary! {},
        format!("<konto>{SECRET}</konto>").into_bytes(),
    )));
    let acroform = d.add(Object::Dictionary(dictionary! {
        "Fields" => Object::Array(Vec::new()),
        "XFA" => Object::Array(vec![
            Object::string_literal("datasets"),
            Object::Reference(dataset),
        ]),
    }));
    d.catalog_set("AcroForm", Object::Reference(acroform));

    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "Probe trägt das Geheimnis"
    );
    let (report, out) = strip(&bytes);
    let ids = ids_in(&out);

    for (id, was) in [(stream, "Anhang"), (dataset, "XFA-Datensatz"), (js, "JS")] {
        assert!(!ids.contains(&id), "{was} steht noch in der Ausgabe");
    }
    assert!(
        leaks(&out, SECRET).is_empty(),
        "Ausgabe trägt das Geheimnis noch: {:?}",
        leaks(&out, SECRET)
    );
    assert_eq!(report.embedded_files_removed, 1, "{:?}", report.summary());
    assert_eq!(report.javascript_removed, 1, "{:?}", report.summary());
    assert!(report.xfa_removed, "{:?}", report.summary());
}
