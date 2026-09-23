//! Spur A, Runde 2, Prüfer B-1 (Register #90): Erscheinungsströme von
//! Annotationen, die auf keiner Seite stehen.
//!
//! * **#90:** ein Widget, das nur in den `/Kids` seines Formularfelds hängt,
//!   nicht in `/Annots` einer Seite. `meta.rs` erreicht es über `/Fields` und
//!   nimmt ihm die Texte, hält es damit aber am Leben; sein `/AP` las
//!   niemand — `crate::content` liest, was eine Seite zeigt. Der gezeichnete
//!   Feldwert stand nach dem Lauf in der Datei, ohne Warnung.
//!
//! Ein solcher Träger steht auf keiner Seite und wird von keinem Betrachter gezeichnet.
//! Jetzt verliert ein Träger, der in keinem `/Annots` steht, sein
//! Erscheinungsbild — was niemand zeigt, braucht keins, und was bleibt,
//! könnte niemand schwärzen.

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfRedactor};

fn schwaerzung() -> Redaction {
    Redaction::new(
        Region::new(
            0,
            Rect::new(40.0, 600.0, 560.0, 760.0),
            None,
            Source::Manual {
                reason: "Spur A, Prüfer B".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Der volle Lauf, wie `redact-pipeline` ihn fährt; danach darf das
/// Geheimnis nirgends mehr stehen.
#[track_caller]
fn muss_fallen(bytes: &[u8], was: &str) {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "{was}: die Probe muss das Geheimnis vorher tragen"
    );
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[schwaerzung()])
        .expect("Schwärzung läuft durch");
    let meta = strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    let hits = leaks(&out, SECRET);
    assert!(
        hits.is_empty(),
        "{was}: STILLES LECK.\n  Warnungen: {:?}\n  Bericht: {:?}\n  Fundstellen:\n{}",
        report.warnings,
        meta.summary(),
        hits.join("\n")
    );
}

fn probe() -> Doc {
    page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")])
}

fn rect() -> Object {
    vec![50.into(), 400.into(), 250.into(), 420.into()]
        .into_iter()
        .collect::<Vec<Object>>()
        .into()
}

fn form_mit_text(d: &mut Doc) -> ObjectId {
    let font_id = d.font_id;
    d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 200.into(), 20.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => Object::Reference(font_id) } },
            },
            format!("BT /F1 8 Tf 2 2 Td ({SECRET}) Tj ET\n").into_bytes(),
        )
        .with_compression(false),
    ))
}

/// #90: das Widget hängt nur in den `/Kids` des Felds. Sein Geschwister
/// steht auf der Seite und hält über `/Parent` das Feld — und damit dessen
/// `/Kids` — am Leben; das Formular selbst fällt mit `/AcroForm`.
#[test]
fn ein_widget_nur_in_den_kids_seines_felds_verliert_sein_erscheinungsbild() {
    let mut d = probe();
    let ap = form_mit_text(&mut d);
    let leer = d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 20.into(), 20.into()] },
        b"0.9 g 0 0 20 20 re f\n".to_vec(),
    )));
    let auf_der_seite = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "Rect" => rect(),
        "AP" => dictionary! { "N" => Object::Reference(leer) },
    }));
    let nur_in_kids = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "Rect" => rect(),
        "AP" => dictionary! { "N" => Object::Reference(ap) },
    }));
    let feld = d.add(Object::Dictionary(dictionary! {
        "FT" => "Tx", "T" => Object::string_literal("konto"),
        "Kids" => vec![Object::Reference(auf_der_seite), Object::Reference(nur_in_kids)],
    }));
    for widget in [auf_der_seite, nur_in_kids] {
        if let Ok(w) = d.doc.get_dictionary_mut(widget) {
            w.set("Parent", Object::Reference(feld));
        }
    }
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(auf_der_seite)]),
    );
    d.catalog_set(
        "AcroForm",
        Object::Dictionary(dictionary! { "Fields" => vec![Object::Reference(feld)] }),
    );
    muss_fallen(&d.finish(), "Widget nur in /Kids");
}

/// Gegenprobe: ein Widget, das in `/Annots` steht, behält sein
/// Erscheinungsbild — es wird gezeichnet, und die Analyse liest es.
#[test]
fn ein_widget_auf_der_seite_behaelt_sein_erscheinungsbild() {
    let mut d = probe();
    let leer = d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 20.into(), 20.into()] },
        b"0.9 g 0 0 20 20 re f\n".to_vec(),
    )));
    let widget = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "Rect" => rect(),
        "AP" => dictionary! { "N" => Object::Reference(leer) },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(widget)]));
    let mut doc = load_from_bytes(&d.finish()).expect("PDF ladbar");
    strip_metadata(&mut doc);
    let w = doc.get_dictionary(widget).expect("Widget bleibt");
    assert!(w.has(b"AP"), "das gezeichnete Widget verlor sein /AP");
}

fn leer(d: &mut Doc) -> ObjectId {
    d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 20.into(), 20.into()] },
        b"0.9 g 0 0 20 20 re f\n".to_vec(),
    )))
}

/// Das erste Widget in `/Annots` der Seite nach dem Metadatenlauf.
fn erstes_widget(bytes: &[u8]) -> lopdf::Dictionary {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    strip_metadata(&mut doc);
    let page = *doc.get_pages().values().next().expect("eine Seite");
    let annots = doc
        .get_dictionary(page)
        .expect("Seite")
        .get(b"Annots")
        .expect("/Annots")
        .clone();
    let annots = match annots {
        Object::Reference(id) => doc.get_object(id).expect("Array").clone(),
        other => other,
    };
    match &annots.as_array().expect("Array")[0] {
        Object::Reference(id) => doc.get_dictionary(*id).expect("Widget").clone(),
        Object::Dictionary(dict) => dict.clone(),
        other => panic!("kein Widget: {other:?}"),
    }
}

/// Gegenprobe: ein direkt in `/Annots` eingebettetes Widget zeigt die Seite.
#[test]
fn ein_direktes_widget_in_annots_behaelt_sein_erscheinungsbild() {
    let mut d = probe();
    let leer = leer(&mut d);
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "Rect" => rect(),
            "AP" => dictionary! { "N" => Object::Reference(leer) },
        })]),
    );
    assert!(erstes_widget(&d.finish()).has(b"AP"));
}

/// Gegenprobe: `/Annots` als eigenes Objekt — seine direkten Einträge zeigt
/// die Seite ebenso.
#[test]
fn ein_widget_in_einem_annots_objekt_behaelt_sein_erscheinungsbild() {
    let mut d = probe();
    let leer = leer(&mut d);
    let annots = d.add(Object::Array(vec![Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "Rect" => rect(),
        "AP" => dictionary! { "N" => Object::Reference(leer) },
    })]));
    d.page_dict_set("Annots", Object::Reference(annots));
    assert!(erstes_widget(&d.finish()).has(b"AP"));
}

/// Gegenprobe: ein `/MK` als eigenes Objekt, das ein gezeigtes Widget
/// benutzt, behält sein Symbol — auch wenn ein nicht gezeigtes es teilt.
#[test]
fn ein_geteiltes_mk_eines_gezeigten_widgets_behaelt_sein_symbol() {
    let mut d = probe();
    let leer = leer(&mut d);
    let mk = d.add(Object::Dictionary(
        dictionary! { "I" => Object::Reference(leer) },
    ));
    let gezeigt = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "Rect" => rect(),
        "AP" => dictionary! { "N" => Object::Reference(leer) }, "MK" => Object::Reference(mk),
    }));
    let versteckt = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "Rect" => rect(),
        "MK" => Object::Reference(mk), "IRT" => Object::Reference(gezeigt),
    }));
    let halter = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(), "IRT" => Object::Reference(versteckt),
    }));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(halter), Object::Reference(gezeigt)]),
    );
    let mut doc = load_from_bytes(&d.finish()).expect("PDF ladbar");
    strip_metadata(&mut doc);
    let mk = doc.get_dictionary(mk).expect("/MK bleibt");
    assert!(mk.has(b"I"), "das Symbol des gezeigten Widgets fiel");
}

/// Das Symbol eines Widgets, das keine Seite zeigt, fällt — auch hinter
/// einem `/MK` als eigenem Objekt.
#[test]
fn das_mk_symbol_eines_nicht_gezeigten_widgets_faellt() {
    let mut d = probe();
    let icon = form_mit_text(&mut d);
    let leer = leer(&mut d);
    let mk = d.add(Object::Dictionary(
        dictionary! { "I" => Object::Reference(icon) },
    ));
    let auf_der_seite = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "Rect" => rect(),
        "AP" => dictionary! { "N" => Object::Reference(leer) },
    }));
    let nur_in_kids = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "Rect" => rect(),
        "MK" => Object::Reference(mk),
    }));
    let feld = d.add(Object::Dictionary(dictionary! {
        "FT" => "Btn", "T" => Object::string_literal("knopf"),
        "Kids" => vec![Object::Reference(auf_der_seite), Object::Reference(nur_in_kids)],
    }));
    for widget in [auf_der_seite, nur_in_kids] {
        if let Ok(w) = d.doc.get_dictionary_mut(widget) {
            w.set("Parent", Object::Reference(feld));
        }
    }
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(auf_der_seite)]),
    );
    muss_fallen(&d.finish(), "/MK /I eines Widgets nur in /Kids");
}
