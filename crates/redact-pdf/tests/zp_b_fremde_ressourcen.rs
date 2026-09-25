//! Register #107 (bei #92 gefunden, Spur-A-Runde 2): ein Schlüssel in einem
//! `/Resources`-Verzeichnis, der keine Ressourcenart ist.
//!
//! Seit #92 behalten Katalog, Seitenbaum und Seite nur, was auf ihrer
//! Erlaubnisliste steht. Das Ressourcenverzeichnis darunter behielt jeden
//! Schlüssel: `/Resources << /Font … /Zusatz (Kunde …) >>` stand nach dem
//! Lauf in der Ausgabe — an der Seite, geerbt am Seitenbaum, am Formular und
//! am Erscheinungsbild einer Annotation. Keiner dieser Schlüssel zeichnet
//! etwas; die Arten, die ein Inhaltsstrom nachschlagen kann, sind genau die
//! acht aus ISO 32000-2, Tabelle 34.
//!
//! Orakel ist [`redact_pdf::leaks`] an den geschriebenen Bytes.

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport};

fn geheim() -> Object {
    Object::string_literal(format!("Kunde {SECRET}"))
}

fn lauf(d: &Doc) -> (Vec<String>, MetadataReport, lopdf::Document) {
    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "die Probe muss das Geheimnis vorher tragen"
    );
    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let report = strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    (leaks(&out, SECRET), report, doc)
}

#[track_caller]
fn muss_fallen(d: &Doc, was: &str) -> MetadataReport {
    let (hits, report, _) = lauf(d);
    assert!(
        hits.is_empty(),
        "{was}: STILLES LECK — der Klartext steht nach dem Lauf in der Ausgabe.\n  \
         Bericht: {:?}\n  Fundstellen:\n{}",
        report.summary(),
        hits.join("\n")
    );
    assert!(
        report.anything_removed(),
        "{was}: der Bericht muss die Entfernung nennen"
    );
    report
}

fn ressourcen_set(d: &mut Doc, key: &str, value: Object) {
    let id = d.resources_id;
    d.doc
        .get_dictionary_mut(id)
        .expect("Ressourcen")
        .set(key, value);
}

/// Ein Formular mit eigenem `/Resources`, das `extra` zusätzlich trägt, auf
/// der Seite unter `/Fm1` platziert und gezeichnet.
fn formular(d: &mut Doc, extra: Object) -> ObjectId {
    let font_id = d.font_id;
    let form = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 200.into(), 20.into()],
                "Resources" => dictionary! {
                    "Font" => dictionary! { "F1" => Object::Reference(font_id) },
                    "Zusatz" => extra,
                },
            },
            b"BT /F1 8 Tf 2 2 Td (Formular) Tj ET\n".to_vec(),
        )
        .with_compression(false),
    ));
    ressourcen_set(
        d,
        "XObject",
        Object::Dictionary(dictionary! { "Fm1" => Object::Reference(form) }),
    );
    d.set_content(b"BT /F1 12 Tf 72 700 Td (Hallo) Tj ET q 1 0 0 1 72 600 cm /Fm1 Do Q\n");
    form
}

/// Das Ressourcenobjekt der Seite (ein eigenes Objekt) trägt einen fremden
/// Schlüssel.
#[test]
fn ein_fremder_schluessel_im_ressourcenobjekt_der_seite_faellt() {
    let mut d = page(&["Hallo"]);
    ressourcen_set(&mut d, "Zusatz", geheim());
    muss_fallen(&d, "/Resources der Seite (eigenes Objekt)");
}

/// Dasselbe, wenn `/Resources` direkt in der Seite steht — und der Wert
/// selbst ein eigenes Objekt ist.
#[test]
fn ein_fremder_schluessel_in_direkten_ressourcen_faellt() {
    let mut d = page(&["Hallo"]);
    let text = d.add(geheim());
    let font_id = d.font_id;
    d.page_dict_set(
        "Resources",
        Object::Dictionary(dictionary! {
            "Font" => dictionary! { "F1" => Object::Reference(font_id) },
            "Notiz" => Object::Reference(text),
        }),
    );
    muss_fallen(&d, "/Resources direkt in der Seite");
}

/// Geerbt: `/Resources` am Knoten des Seitenbaums.
#[test]
fn ein_fremder_schluessel_in_geerbten_ressourcen_faellt() {
    let mut d = page(&["Hallo"]);
    let pages_id = d.pages_id;
    d.doc.get_dictionary_mut(pages_id).expect("Seitenbaum").set(
        "Resources",
        Object::Dictionary(dictionary! { "Zusatz" => geheim() }),
    );
    muss_fallen(&d, "/Resources am Seitenbaum");
}

/// Am Formular, das die Seite zeichnet.
#[test]
fn ein_fremder_schluessel_in_den_ressourcen_eines_formulars_faellt() {
    let mut d = page(&["Hallo"]);
    formular(&mut d, geheim());
    muss_fallen(&d, "/Resources eines Formulars");
}

/// Am Erscheinungsbild einer Annotation.
#[test]
fn ein_fremder_schluessel_in_den_ressourcen_eines_erscheinungsbilds_faellt() {
    let mut d = page(&["Hallo"]);
    let font_id = d.font_id;
    let ap = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 20.into(), 20.into()],
                "Resources" => dictionary! {
                    "Font" => dictionary! { "F1" => Object::Reference(font_id) },
                    "Zusatz" => geheim(),
                },
            },
            b"0.9 g 0 0 20 20 re f\n".to_vec(),
        )
        .with_compression(false),
    ));
    let annot = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Square",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "AP" => dictionary! { "N" => Object::Reference(ap) },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot)]));
    muss_fallen(&d, "/Resources eines Erscheinungsbilds");
}

/// Ein Ressourcenobjekt, das sich Seite und Formular teilen: einmal
/// bereinigt, für beide.
#[test]
fn ein_geteiltes_ressourcenobjekt_verliert_seinen_fremden_schluessel() {
    let mut d = page(&["Hallo"]);
    ressourcen_set(&mut d, "Zusatz", geheim());
    let resources_id = d.resources_id;
    let form = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 200.into(), 20.into()],
                "Resources" => Object::Reference(resources_id),
            },
            b"BT /F1 8 Tf 2 2 Td (Formular) Tj ET\n".to_vec(),
        )
        .with_compression(false),
    ));
    ressourcen_set(
        &mut d,
        "XObject",
        Object::Dictionary(dictionary! { "Fm1" => Object::Reference(form) }),
    );
    d.set_content(b"BT /F1 12 Tf 72 700 Td (Hallo) Tj ET /Fm1 Do\n");
    muss_fallen(&d, "geteiltes Ressourcenobjekt");
}

/// Gegenprobe: jede Ressourcenart aus Tabelle 34 bleibt, und die
/// Eigenschaftslisten unter `/Properties` bleiben frei gestaltbar.
#[test]
fn was_der_inhalt_nachschlagen_kann_bleibt() {
    let mut d = page(&["Hallo"]);
    formular(&mut d, Object::Null);
    ressourcen_set(
        &mut d,
        "ExtGState",
        Object::Dictionary(dictionary! { "GS1" => dictionary! { "CA" => 1 } }),
    );
    ressourcen_set(
        &mut d,
        "ColorSpace",
        Object::Dictionary(dictionary! { "CS1" => "DeviceRGB" }),
    );
    ressourcen_set(
        &mut d,
        "ProcSet",
        Object::Array(vec!["PDF".into(), "Text".into()]),
    );
    d.zweiter_halter(Object::string_literal("frei gestaltbar"));
    ressourcen_set(&mut d, "Zusatz", geheim());
    let (hits, _, doc) = lauf(&d);
    assert!(hits.is_empty(), "{}", hits.join("\n"));
    let seite = *doc.get_pages().values().next().expect("Seite");
    let ressourcen = match doc.get_dictionary(seite).expect("Seite").get(b"Resources") {
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).expect("Ressourcen").clone(),
        Ok(Object::Dictionary(dict)) => dict.clone(),
        other => panic!("/Resources fehlt: {other:?}"),
    };
    for key in [
        &b"Font"[..],
        b"XObject",
        b"ExtGState",
        b"ColorSpace",
        b"ProcSet",
        b"Properties",
    ] {
        assert!(
            ressourcen.get(key).is_ok(),
            "/{} fehlt in den Ressourcen der Seite",
            String::from_utf8_lossy(key)
        );
    }
    let properties = ressourcen
        .get(b"Properties")
        .and_then(Object::as_dict)
        .expect("/Properties");
    assert!(
        properties.get(b"Halter0").is_ok(),
        "die Eigenschaftsliste unter /Properties muss bleiben"
    );
    assert!(
        !leaks(&save_to_bytes(&doc).expect("Speichern"), "frei gestaltbar").is_empty(),
        "der Inhalt einer Eigenschaftsliste bleibt"
    );
}
