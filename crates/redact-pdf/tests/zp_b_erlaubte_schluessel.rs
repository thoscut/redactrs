//! Register #92, dritter Teil (Prüfer B-5, Spur-A-Runde 2): Schlüssel an
//! Katalog, Seitenbaum und Seite, die die Aufzählung nicht kannte.
//!
//! Der Metadatenlauf nahm bis hierher eine **Sperrliste** von Schlüsseln;
//! was nicht auf ihr stand, blieb. Prüfer B fand vier weitere Stellen mit
//! Klartext, jede mit Rückgabewert 0 und „nichts zu entfernen“:
//! `/SpiderInfo` (Web Capture: `/C` mit `/URL` und `/P`), `/Legal` mit
//! `/Attestation`, `/Requirements` mit Text am Katalog, `/OutputIntents` an
//! der Seite (PDF 2.0), `/3DU` an einer 3D-Annotation. Jede Runde findet
//! weitere. Katalog, Seitenbaum und Seite laufen deshalb jetzt über eine
//! **Erlaubnisliste**: was nicht auf ihr steht, fällt als Beiwerk.
//!
//! Orakel ist [`redact_pdf::leaks`] an den geschriebenen Bytes.

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport};

fn s(text: &str) -> Object {
    Object::string_literal(text)
}

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
    report
}

/// `/SpiderInfo` (14.10.2): der Web-Capture-Befehl nennt die abgerufene
/// Adresse und die gesendeten Formulardaten.
#[test]
fn spiderinfo_am_katalog_faellt() {
    let mut d = page(&["Hallo"]);
    d.catalog_set(
        "SpiderInfo",
        Object::Dictionary(dictionary! {
            "V" => Object::Real(1.0),
            "C" => vec![Object::Dictionary(dictionary! {
                "URL" => s("https://bank.example/konto"),
                "P" => geheim(),
            })],
        }),
    );
    let report = muss_fallen(&d, "/SpiderInfo");
    assert!(
        report.anything_removed(),
        "der Bericht muss die Entfernung nennen"
    );
}

/// `/Legal` (12.8.7) mit `/Attestation`, `/Requirements` (12.10) mit Text —
/// und ein Schlüssel, den keine Norm kennt.
#[test]
fn legal_requirements_und_ein_fremder_schluessel_am_katalog_fallen() {
    let mut d = page(&["Hallo"]);
    d.catalog_set(
        "Legal",
        Object::Dictionary(dictionary! { "JavaScriptActions" => 0, "Attestation" => geheim() }),
    );
    d.catalog_set(
        "Requirements",
        vec![Object::Dictionary(dictionary! {
            "Type" => "Requirement",
            "S" => "EnableJavaScripts",
            "RH" => vec![Object::Dictionary(dictionary! { "Type" => "ReqHandler", "S" => "JS", "Script" => geheim() })],
        })]
        .into(),
    );
    d.catalog_set("XYZ:Kundennummer", geheim());
    muss_fallen(&d, "/Legal, /Requirements, fremder Schlüssel");
}

/// `/OutputIntents` an der Seite (PDF 2.0, 14.11.5): die Texte fallen wie
/// am Katalog, der Normbezeichner bleibt.
#[test]
fn outputintents_an_der_seite_verlieren_ihre_texte() {
    let mut d = page(&["Hallo"]);
    d.page_dict_set(
        "OutputIntents",
        vec![Object::Dictionary(dictionary! {
            "Type" => "OutputIntent",
            "S" => "GTS_PDFX",
            "OutputConditionIdentifier" => s("FOGRA39"),
            "Info" => geheim(),
            "OutputCondition" => geheim(),
            "RegistryName" => s("http://www.color.org"),
        })]
        .into(),
    );
    muss_fallen(&d, "/OutputIntents an der Seite");
    let (_, _, doc) = lauf(&d);
    let seite = doc
        .get_pages()
        .values()
        .next()
        .copied()
        .and_then(|id| doc.get_dictionary(id).ok().cloned())
        .expect("Seite");
    let intents = seite
        .get(b"OutputIntents")
        .and_then(Object::as_array)
        .expect("bleibt");
    let intent = intents[0].as_dict().expect("Dictionary");
    assert!(intent.get(b"OutputConditionIdentifier").is_ok());
    assert!(intent.get(b"Info").is_err());
}

/// Ein fremder Schlüssel an der Seite und einer am Knoten des Seitenbaums.
#[test]
fn fremde_schluessel_an_seite_und_seitenbaum_fallen() {
    let mut d = page(&["Hallo"]);
    d.page_dict_set("Kundennotiz", geheim());
    let pages_id = d.pages_id;
    d.doc
        .get_dictionary_mut(pages_id)
        .expect("Seitenbaum")
        .set("Stapel", geheim());
    muss_fallen(&d, "fremder Schlüssel an Seite und Seitenbaum");
}

/// `/3DU` (PDF 2.0, 13.6.2) an einer 3D-Annotation: `/TU`, `/UU`, `/DU`
/// sind frei wählbare Einheitennamen.
#[test]
fn einheiten_einer_3d_annotation_fallen() {
    let mut d = page(&["Hallo"]);
    let annot = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "3D",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "3DU" => dictionary! { "TU" => geheim(), "UU" => s("mm"), "DU" => s("mm") },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot)]));
    muss_fallen(&d, "/3DU");
}

/// Gegenprobe: was Seite und Katalog zum Anzeigen brauchen, bleibt.
#[test]
fn was_die_anzeige_braucht_bleibt() {
    let mut d = page(&["Hallo"]);
    d.page_dict_set("Rotate", 90.into());
    d.page_dict_set("UserUnit", Object::Real(2.0));
    d.page_dict_set("Tabs", "S".into());
    d.page_dict_set(
        "CropBox",
        vec![0.into(), 0.into(), 500.into(), 800.into()].into(),
    );
    d.catalog_set("PageMode", "UseNone".into());
    d.catalog_set("PageLayout", "SinglePage".into());
    d.catalog_set("Lang", s("de-DE"));
    d.catalog_set(
        "ViewerPreferences",
        Object::Dictionary(dictionary! { "Direction" => "L2R" }),
    );
    d.page_dict_set("Kundennotiz", geheim());
    let (hits, _, doc) = lauf(&d);
    assert!(hits.is_empty(), "{}", hits.join("\n"));
    let seite_id = *doc.get_pages().values().next().expect("Seite");
    let seite = doc.get_dictionary(seite_id).expect("Seite");
    for key in [
        &b"Rotate"[..],
        b"UserUnit",
        b"Tabs",
        b"CropBox",
        b"MediaBox",
        b"Contents",
        b"Resources",
        b"Parent",
    ] {
        assert!(
            seite.get(key).is_ok(),
            "/{} fehlt an der Seite",
            String::from_utf8_lossy(key)
        );
    }
    let katalog = doc.catalog().expect("Katalog");
    for key in [
        &b"PageMode"[..],
        b"PageLayout",
        b"Lang",
        b"ViewerPreferences",
        b"Pages",
    ] {
        assert!(
            katalog.get(key).is_ok(),
            "/{} fehlt am Katalog",
            String::from_utf8_lossy(key)
        );
    }
}
