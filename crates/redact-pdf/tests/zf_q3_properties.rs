//! Gegenprüfung Q3 (Fix-Runde 5): wo ein Textspiegel überall **im
//! Ressourcenverzeichnis** stehen kann.
//!
//! Register #34 hält einen Fall fest: eine direkt in `/Resources /Properties`
//! der **Seite** stehende Eigenschaftsliste behält ihren Spiegel
//! (`ze_p2_spiegel::befund_direkte_eigenschaftsliste_behaelt_ihren_spiegel`).
//! Hier steht die Frage, ob es wirklich nur dieser eine Ort ist.
//!
//! Geprüft werden vier Orte für dieselbe Liste:
//!
//! | Ort                                        | Ergebnis |
//! |--------------------------------------------|----------|
//! | Seite, Liste als eigenes Objekt             | bereinigt |
//! | Seite, Liste direkt (Register #34)          | bleibt |
//! | **Formular**, Liste direkt                  | bleibt |
//! | **geerbt vom Seitenbaum**, Liste direkt     | bleibt |
//! | **`/Properties` als geteiltes Objekt**, Liste direkt | bleibt |

mod common;

use common::{page, text_ops, Doc, SECRET};
use lopdf::{dictionary, Dictionary, Object, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn text_at(y: i32, text: &str) -> String {
    format!("BT /F1 10 Tf 72 {y} Td ({}) Tj ET\n", escape(text))
}

fn analyse(bytes: &[u8]) -> (Vec<TextRun>, Vec<String>) {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion")
}

fn redactions_for(runs: &[TextRun], needle: &str) -> Vec<Redaction> {
    runs.iter()
        .filter_map(|run| {
            let pos = run.text.find(needle)?;
            let rect = run.rect_for_byte_range(pos, pos + needle.len())?;
            Some(Redaction::new(
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
            ))
        })
        .collect()
}

fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> (Vec<u8>, Vec<String>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    (save_to_bytes(&doc).expect("Speichern"), report.warnings)
}

/// Wo die Eigenschaftsliste steht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ort {
    /// `/Resources /Properties /MC0` der Seite, Liste als **eigenes Objekt**.
    SeiteAlsObjekt,
    /// dasselbe, aber die Liste steht **direkt** im Verzeichnis (Register #34).
    SeiteDirekt,
    /// `/Resources /Properties /MC0` eines **Formulars**, Liste direkt.
    FormularDirekt,
    /// `/Resources` steht am **Seitenbaum** (`/Pages`), die Seite erbt es.
    GeerbtDirekt,
    /// `/Properties` selbst ist ein **eigenes, geteiltes Objekt**; die Liste
    /// darin steht direkt.
    GeteiltesPropertiesDirekt,
}

fn liste(d: &mut Doc, lie: &str, als_objekt: bool) -> Object {
    let list = dictionary! { "ActualText" => Object::string_literal(lie.to_string()) };
    if als_objekt {
        Object::Reference(d.add(Object::Dictionary(list)))
    } else {
        Object::Dictionary(list)
    }
}

fn material(ort: Ort, lie: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let page_resources = d.resources_id;

    match ort {
        Ort::SeiteAlsObjekt | Ort::SeiteDirekt => {
            let list = liste(&mut d, lie, ort == Ort::SeiteAlsObjekt);
            let mut props = Dictionary::new();
            props.set("MC0", list);
            d.doc
                .get_dictionary_mut(page_resources)
                .expect("Ressourcen")
                .set("Properties", props);
            let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
            raw.extend_from_slice(b"/Span /MC0 BDC\n");
            raw.extend_from_slice(text_at(600, SECRET).as_bytes());
            raw.extend_from_slice(b"EMC\n");
            d.set_content(&raw);
        }
        Ort::GeteiltesPropertiesDirekt => {
            let list = liste(&mut d, lie, false);
            let mut props = Dictionary::new();
            props.set("MC0", list);
            let props_id = d.add(Object::Dictionary(props));
            d.doc
                .get_dictionary_mut(page_resources)
                .expect("Ressourcen")
                .set("Properties", Object::Reference(props_id));
            let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
            raw.extend_from_slice(b"/Span /MC0 BDC\n");
            raw.extend_from_slice(text_at(600, SECRET).as_bytes());
            raw.extend_from_slice(b"EMC\n");
            d.set_content(&raw);
        }
        Ort::GeerbtDirekt => {
            // Die Seite bekommt gar keine eigenen Ressourcen; sie erbt sie
            // vom `/Pages`-Knoten. Genau der Fall, für den `page_resources`
            // die Kette hochläuft.
            let list = liste(&mut d, lie, false);
            let mut props = Dictionary::new();
            props.set("MC0", list);
            d.doc
                .get_dictionary_mut(page_resources)
                .expect("Ressourcen")
                .set("Properties", props);
            let pages_id = d.pages_id;
            d.doc
                .get_dictionary_mut(pages_id)
                .expect("Seitenbaum")
                .set("Resources", Object::Reference(page_resources));
            let page_id = d.page_id;
            d.doc
                .get_dictionary_mut(page_id)
                .expect("Seite")
                .remove(b"Resources");
            let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
            raw.extend_from_slice(b"/Span /MC0 BDC\n");
            raw.extend_from_slice(text_at(600, SECRET).as_bytes());
            raw.extend_from_slice(b"EMC\n");
            d.set_content(&raw);
        }
        Ort::FormularDirekt => {
            let font_id = d.font_id;
            let list = liste(&mut d, lie, false);
            let mut props = Dictionary::new();
            props.set("MC0", list);
            let form_resources = d.add(Object::Dictionary(dictionary! {
                "Font" => dictionary! { "F1" => font_id },
                "Properties" => props,
            }));
            let body = format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET));
            let form_id = d.add(Object::Stream(
                Stream::new(
                    dictionary! {
                        "Type" => "XObject",
                        "Subtype" => "Form",
                        "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                        "Resources" => form_resources,
                    },
                    body.as_bytes().to_vec(),
                )
                .with_compression(false),
            ));
            d.doc
                .get_dictionary_mut(page_resources)
                .expect("Ressourcen")
                .set("XObject", dictionary! { "Fm0" => form_id });
            let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
            raw.extend_from_slice(b"q /Fm0 Do Q\n");
            d.set_content(&raw);
        }
    }
    d.finish()
}

/// Schickt das Material durch die Pipeline und liefert die Fundstellen.
fn nach_der_pipeline(ort: Ort) -> (Vec<String>, Vec<String>) {
    let lie = format!("Zahlung an {SECRET}");
    let bytes = material(ort, &lie);
    let (runs, _) = analyse(&bytes);
    let redactions = redactions_for(&runs, SECRET);
    assert!(!redactions.is_empty(), "{ort:?}: nichts zu schwärzen");
    let (out, warnings) = pipeline(&bytes, &redactions);
    (
        leaks(&out, SECRET).iter().map(|f| f.to_string()).collect(),
        warnings,
    )
}

/// Die Gegenprobe: steht die Liste als **eigenes Objekt** in `/Properties`,
/// wird sie bereinigt. Nur daran lässt sich ablesen, dass der Unterschied
/// wirklich an „direkt oder Verweis“ hängt und nicht am Aufbau der Datei.
#[test]
fn liste_als_eigenes_objekt_verliert_ihren_spiegel() {
    let (found, warnings) = nach_der_pipeline(Ort::SeiteAlsObjekt);
    assert!(found.is_empty(), "Warnungen {warnings:?}, Lecks {found:?}");
}

/// **Befund Q3-5 (neu, offen).** Register #34 nennt nur die Seite (erster
/// Fall in der Schleife, hier zum Vergleich mit dabei). Derselbe Klartext
/// bleibt aber genauso stehen, wenn die Liste
///
/// * in den Ressourcen eines **Form-XObjects** steht,
/// * über den **Seitenbaum geerbt** wird (`/Pages /Resources`), oder
/// * in einem **`/Properties`-Objekt** steht, das mehrere Ströme teilen.
///
/// Alle drei sind eigene Fundorte in der Datei: die Korrektur, die Register
/// #34 beschreibt („die Objekt-Id des Verzeichnisses, in dem die Liste
/// steht“), muss für jeden von ihnen greifen, nicht nur für
/// `page_resources`. Ohne Warnung, mit Rückgabewert 0.
///
/// Lauf: `cargo test -p redact-pdf --test zf_q3_properties -- --ignored`.
#[test]
#[ignore = "Befund Q3-5: direkte Eigenschaftsliste behält ihren Spiegel auch außerhalb der Seitenressourcen"]
fn befund_direkte_liste_bleibt_an_drei_weiteren_orten() {
    let mut offen = Vec::new();
    for ort in [
        Ort::SeiteDirekt,
        Ort::FormularDirekt,
        Ort::GeerbtDirekt,
        Ort::GeteiltesPropertiesDirekt,
    ] {
        let (found, warnings) = nach_der_pipeline(ort);
        if !found.is_empty() {
            offen.push(format!("{ort:?}: {found:?} (Warnungen {warnings:?})"));
        }
    }
    assert!(offen.is_empty(), "{}", offen.join("\n"));
}
