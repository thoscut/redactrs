//! Register #92, erster Teil (Prüfer B-3, Spur-A-Runde 2): die Einstellungen
//! einer RichMedia-Annotation halten dieselben Dateien wie ihr Inhalt.
//!
//! Nach PDF 32000-1 (Erweiterungsstufe 3, 13.7.2) trägt eine
//! `/RichMedia`-Annotation zwei Dictionaries: `/RichMediaContent` mit den
//! Dateien unter `/Assets` und `/RichMediaSettings` mit `/Activation`. Unter
//! `/Activation /Configuration` zeigt jede Instanz mit `/Asset` auf ein
//! Filespec, unter `/Activation /Scripts` steht eine Liste weiterer Filespecs
//! — Acrobat und das LaTeX-Paket media9 schreiben beides. Der Metadatenlauf
//! nahm nur `/RichMediaContent`; die eingebettete Datei blieb über die
//! Einstellungen erreichbar und stand nach dem Lauf in der Ausgabe, ohne
//! Warnung.
//!
//! Orakel ist [`redact_pdf::leaks`] an den geschriebenen Bytes. Die Seite
//! trägt das Geheimnis auch im Seitentext, und der Lauf schwärzt diese Zeile:
//! ein Fund kommt dann nur noch aus dem Träger.

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
                reason: "Register #92".into(),
            },
        ),
        Action::Blackout,
    )
}

fn muss_fallen(bytes: &[u8], was: &str) {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "{was}: die Probe muss das Geheimnis vorher tragen"
    );
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, &[schwaerzung()])
        .expect("Schwärzung");
    let meta = strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    let hits = leaks(&out, SECRET);
    assert!(
        hits.is_empty(),
        "{was}: STILLES LECK — nach dem Lauf steht der Klartext noch in der Ausgabe.\n  \
         Bericht: {:?}\n  Fundstellen:\n{}",
        meta.summary(),
        hits.join("\n")
    );
}

fn probe() -> Doc {
    page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")])
}

fn s(text: &str) -> Object {
    Object::string_literal(text)
}

fn filespec(d: &mut Doc, name: &str) -> ObjectId {
    let strom = d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "EmbeddedFile" },
        format!("Kontoauszug\nIBAN {SECRET}\n").into_bytes(),
    )));
    d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => s(name),
        "UF" => s(name),
        "EF" => dictionary! { "F" => Object::Reference(strom) },
    }))
}

fn richmedia(d: &mut Doc, settings: lopdf::Dictionary, assets: Vec<Object>) {
    let annot = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "RichMedia",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "RichMediaContent" => dictionary! {
            "Type" => "RichMediaContent",
            "Assets" => dictionary! { "Names" => assets },
        },
        "RichMediaSettings" => settings,
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot)]));
}

/// Die Instanz einer Konfiguration zeigt mit `/Asset` auf dieselbe Datei,
/// die `/Assets` nennt — so schreibt es Acrobat.
#[test]
fn die_instanz_einer_konfiguration_haelt_die_datei() {
    let mut d = probe();
    let fs = filespec(&mut d, "auszug.swf");
    let settings = dictionary! {
        "Type" => "RichMediaSettings",
        "Activation" => dictionary! {
            "Type" => "RichMediaActivation",
            "Condition" => "XA",
            "Configuration" => dictionary! {
                "Type" => "RichMediaConfiguration",
                "Subtype" => "Flash",
                "Instances" => vec![Object::Dictionary(dictionary! {
                    "Type" => "RichMediaInstance",
                    "Subtype" => "Flash",
                    "Asset" => Object::Reference(fs),
                })],
            },
        },
    };
    richmedia(
        &mut d,
        settings,
        vec![s("auszug.swf"), Object::Reference(fs)],
    );
    muss_fallen(&d.finish(), "/RichMediaSettings /Activation /Configuration");
}

/// `/Activation /Scripts` ist eine Liste von Filespecs mit eingebetteter
/// Datei — hier nur dort, nicht unter `/Assets`.
#[test]
fn die_skripte_der_aktivierung_halten_ihre_dateien() {
    let mut d = probe();
    let skript = filespec(&mut d, "start.js");
    let settings = dictionary! {
        "Type" => "RichMediaSettings",
        "Activation" => dictionary! {
            "Type" => "RichMediaActivation",
            "Scripts" => vec![Object::Reference(skript)],
        },
    };
    richmedia(&mut d, settings, Vec::new());
    muss_fallen(&d.finish(), "/RichMediaSettings /Activation /Scripts");
}
