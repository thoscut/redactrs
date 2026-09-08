//! Klartext-Träger außerhalb des Seiteninhalts.
//!
//! Gemessen (vor dieser Änderung) mit `redact-rs x.pdf -o out.pdf` und
//! anschließend `redact-rs out.pdf --check-leaks "DE89 …"`: alle folgenden
//! Träger endeten mit Rückgabewert 0 und ohne Warnung, und das Orakel fand die
//! IBAN in der Ausgabe.
//!
//! * Link-Annotation mit `/A << /S /URI /URI (mailto:…?subject=DE89 …) >>`,
//!   `/GoToR /F (Kontoauszug_DE89….pdf)`, `/Launch /F (…)`, `/GoTo /D (Name)`,
//!   `/AA` mit JavaScript, `/Dest (Name)`;
//! * Lesezeichen: `/Outlines` mit `/Title (IBAN: DE89 …)`;
//! * Notiz mit Symbol-Erscheinungsstrom und der IBAN in `/Contents`; `/T`
//!   und `/Subj` neben einem Erscheinungsstrom; `/RC` neben einem
//!   Erscheinungsstrom.
//!
//! `/Popup`- und `/IRT`-Inhalte sowie eine Annotation **ohne**
//! Erscheinungsstrom endeten schon vorher mit Rückgabewert 3 („nicht
//! durchsucht“) — die IBAN stand aber trotzdem in der Ausgabe. `/OpenAction`
//! und `/Dests` im Katalog wurden bereits entfernt (gemessen: kein Fund).
//!
//! Entscheidung je Träger — siehe `redact_pdf::meta`: alles hier ist
//! Metadatum ohne Glyphengeometrie und wird **entfernt** und im Bericht
//! genannt. Orakel ist [`redact_pdf::leaks`].

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object, Stream};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport, PdfRedactor,
};

/// Die vollständige Verarbeitung ohne eine einzige Schwärzung: was danach
/// fehlt, hat der Metadatenlauf entfernt.
fn pipeline(bytes: &[u8]) -> (MetadataReport, Vec<u8>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, &[])
        .expect("Schwärzung");
    let report = strip_metadata(&mut doc);
    (report, save_to_bytes(&doc).expect("Speichern"))
}

fn assert_gone(bytes: &[u8], what: &str) -> MetadataReport {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "{what}: die Probe muss das Geheimnis vorher tragen, sonst misst der Test nichts"
    );
    let (report, out) = pipeline(bytes);
    assert!(
        leaks(&out, SECRET).is_empty(),
        "{what}: nach der Verarbeitung steht die IBAN noch in der Datei: {:?}",
        leaks(&out, SECRET)
    );
    report
}

fn with_annotation(annot: lopdf::Dictionary) -> (Doc, lopdf::ObjectId) {
    let mut d = page(&["Harmloser Text"]);
    let id = d.add(Object::Dictionary(annot));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(id)]));
    (d, id)
}

fn link(action: lopdf::Dictionary) -> Vec<u8> {
    let (d, _) = with_annotation(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![400.into(), 100.into(), 500.into(), 120.into()],
        "A" => action,
    });
    d.finish()
}

fn icon_appearance(d: &mut Doc) -> lopdf::ObjectId {
    d.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 20.into(), 20.into()],
        },
        b"0 0 1 rg 0 0 20 20 re f".to_vec(),
    )))
}

// ---------------------------------------------------------------------------
// Aktionen und Ziele an Annotationen
// ---------------------------------------------------------------------------

#[test]
fn ein_uri_link_mit_der_iban_in_der_adresse_faellt() {
    let bytes = link(dictionary! {
        "S" => "URI",
        "URI" => Object::string_literal(format!("mailto:x@example.org?subject={SECRET}")),
    });
    let report = assert_gone(&bytes, "/URI");
    assert_eq!(report.annotation_actions_removed, 1);
}

#[test]
fn ein_gotor_link_mit_der_iban_im_dateinamen_faellt() {
    let bytes = link(dictionary! {
        "S" => "GoToR",
        "F" => Object::string_literal(format!("Kontoauszug_{SECRET}.pdf")),
        "D" => vec![0.into(), Object::Name(b"Fit".to_vec())],
    });
    assert_gone(&bytes, "/GoToR /F");
}

#[test]
fn ein_launch_link_faellt() {
    let bytes = link(dictionary! {
        "S" => "Launch",
        "F" => Object::string_literal(format!("Kontoauszug_{SECRET}.pdf")),
    });
    assert_gone(&bytes, "/Launch /F");
}

#[test]
fn ein_goto_auf_ein_benanntes_ziel_faellt() {
    let bytes = link(dictionary! {
        "S" => "GoTo",
        "D" => Object::string_literal(format!("IBAN {SECRET}")),
    });
    assert_gone(&bytes, "/GoTo /D (Name)");
}

#[test]
fn eine_ereignisaktion_mit_javascript_an_einer_annotation_faellt() {
    let (d, _) = with_annotation(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![400.into(), 100.into(), 500.into(), 120.into()],
        "AA" => dictionary! {
            "E" => dictionary! {
                "S" => "JavaScript",
                "JS" => Object::string_literal(format!("app.alert({SECRET})")),
            },
        },
    });
    let report = assert_gone(&d.finish(), "/AA /JS");
    assert_eq!(report.annotation_actions_removed, 1);
}

#[test]
fn ein_benanntes_dest_an_einer_annotation_faellt() {
    let (d, _) = with_annotation(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![400.into(), 100.into(), 500.into(), 120.into()],
        "Dest" => Object::string_literal(format!("IBAN {SECRET}")),
    });
    let report = assert_gone(&d.finish(), "/Dest (Name)");
    assert_eq!(report.annotation_actions_removed, 1);
}

/// Gegenprobe: ein ausdrückliches Ziel trägt keinen Text und bleibt — ein
/// Inhaltsverzeichnis mit Sprüngen ins eigene Dokument funktioniert weiter.
#[test]
fn ein_ausdrueckliches_dest_bleibt_stehen() {
    let (d, annot_id) = with_annotation(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![400.into(), 100.into(), 500.into(), 120.into()],
        "Dest" => vec![
            Object::Null,
            Object::Name(b"XYZ".to_vec()),
            72.into(),
            700.into(),
            Object::Real(1.5),
        ],
    });
    let page_id = d.page_id;
    let mut d = d;
    if let Object::Dictionary(dict) = d.doc.objects.get_mut(&annot_id).expect("Annotation") {
        if let Ok(Object::Array(dest)) = dict.get_mut(b"Dest") {
            dest[0] = Object::Reference(page_id);
        }
    }
    let (report, out) = pipeline(&d.finish());
    assert_eq!(report.annotation_actions_removed, 0);
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    let annot = doc.get_dictionary(annot_id).expect("Annotation steht noch");
    assert!(annot.has(b"Dest"), "das ausdrückliche Ziel bleibt");
}

// ---------------------------------------------------------------------------
// Lesezeichen
// ---------------------------------------------------------------------------

#[test]
fn lesezeichen_mit_der_iban_im_titel_fallen_samt_baum() {
    let mut d = page(&["Harmloser Text"]);
    let outlines_id = d.doc.new_object_id();
    let child_id = d.doc.new_object_id();
    let item_id = d.add(Object::Dictionary(dictionary! {
        "Title" => Object::string_literal("Kapitel 1"),
        "Parent" => outlines_id,
        "First" => child_id,
        "Last" => child_id,
        "Count" => 1,
    }));
    d.doc.objects.insert(
        child_id,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal(format!("IBAN: {SECRET}")),
            "Parent" => item_id,
            "Dest" => vec![Object::Reference(d.page_id), Object::Name(b"Fit".to_vec())],
        }),
    );
    d.doc.objects.insert(
        outlines_id,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => item_id,
            "Last" => item_id,
            "Count" => 2,
        }),
    );
    d.catalog_set("Outlines", Object::Reference(outlines_id));
    let report = assert_gone(&d.finish(), "/Outlines /Title");
    assert_eq!(report.outlines_removed, 2, "beide Einträge gezählt");
    assert!(
        report
            .summary()
            .iter()
            .any(|line| line == "2 Lesezeichen (/Outlines)"),
        "{:?}",
        report.summary()
    );
}

/// Ein Baum, der auf sich selbst zeigt, darf den Lauf nicht aufhalten.
#[test]
fn ein_zirkulaerer_lesezeichenbaum_endet() {
    let mut d = page(&["Harmloser Text"]);
    let outlines_id = d.doc.new_object_id();
    let item_id = d.doc.new_object_id();
    d.doc.objects.insert(
        item_id,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal(format!("IBAN: {SECRET}")),
            "Parent" => outlines_id,
            "Next" => item_id,
            "First" => item_id,
        }),
    );
    d.doc.objects.insert(
        outlines_id,
        Object::Dictionary(dictionary! { "Type" => "Outlines", "First" => item_id }),
    );
    d.catalog_set("Outlines", Object::Reference(outlines_id));
    let report = assert_gone(&d.finish(), "/Outlines (zirkulär)");
    assert_eq!(report.outlines_removed, 1);
}

// ---------------------------------------------------------------------------
// Kommentartexte
// ---------------------------------------------------------------------------

#[test]
fn der_inhalt_einer_notiz_mit_symbol_erscheinungsstrom_faellt() {
    let mut d = page(&["Harmloser Text"]);
    let ap = icon_appearance(&mut d);
    let id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![400.into(), 100.into(), 420.into(), 120.into()],
        "Contents" => Object::string_literal(format!("Notiz {SECRET}")),
        "T" => Object::string_literal("Max"),
        "AP" => dictionary! { "N" => ap },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(id)]));
    let report = assert_gone(&d.finish(), "/Contents neben /AP");
    assert_eq!(report.annotation_texts_cleared, 2, "/Contents und /T");
}

#[test]
fn verfasser_und_betreff_neben_einem_erscheinungsstrom_fallen() {
    let mut d = page(&["Harmloser Text"]);
    let ap = icon_appearance(&mut d);
    let id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![400.into(), 100.into(), 420.into(), 120.into()],
        "Contents" => Object::string_literal("Notiz"),
        "T" => Object::string_literal(format!("Autor {SECRET}")),
        "Subj" => Object::string_literal(format!("Betreff {SECRET}")),
        "AP" => dictionary! { "N" => ap },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(id)]));
    let report = assert_gone(&d.finish(), "/T und /Subj");
    assert_eq!(report.annotation_texts_cleared, 3);
}

#[test]
fn rich_text_neben_einem_erscheinungsstrom_faellt() {
    let mut d = page(&["Harmloser Text"]);
    let ap = icon_appearance(&mut d);
    let id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FreeText",
        "Rect" => vec![400.into(), 100.into(), 500.into(), 120.into()],
        "Contents" => Object::string_literal("Notiz"),
        "RC" => Object::string_literal(format!("<p>IBAN {SECRET}</p>")),
        "DA" => Object::string_literal("/Helv 10 Tf 0 g"),
        "AP" => dictionary! { "N" => ap },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(id)]));
    assert_gone(&d.finish(), "/RC");
}

#[test]
fn popup_und_antwortkette_tragen_danach_nichts_mehr() {
    let mut d = page(&["Harmloser Text"]);
    let popup_id = d.doc.new_object_id();
    let note_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![400.into(), 100.into(), 420.into(), 120.into()],
        "Contents" => Object::string_literal("Notiz"),
        "Popup" => popup_id,
    }));
    d.doc.objects.insert(
        popup_id,
        Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Popup",
            "Rect" => vec![400.into(), 100.into(), 600.into(), 300.into()],
            "Parent" => note_id,
            "Contents" => Object::string_literal(format!("Popup {SECRET}")),
        }),
    );
    let reply_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![400.into(), 100.into(), 420.into(), 120.into()],
        "IRT" => note_id,
        "RT" => "R",
        "Contents" => Object::string_literal(format!("Antwort {SECRET}")),
    }));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![
            Object::Reference(note_id),
            Object::Reference(popup_id),
            Object::Reference(reply_id),
        ]),
    );
    let report = assert_gone(&d.finish(), "/Popup und /IRT");
    assert_eq!(report.annotation_texts_cleared, 3);
}

/// Eine direkt in `/Annots` eingebettete Annotation (kein eigenes Objekt)
/// wird im Feld selbst bereinigt.
#[test]
fn eine_direkt_eingebettete_annotation_wird_im_feld_bereinigt() {
    let mut d = page(&["Harmloser Text"]);
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![400.into(), 100.into(), 420.into(), 120.into()],
            "Contents" => Object::string_literal(format!("Notiz {SECRET}")),
        })]),
    );
    let report = assert_gone(&d.finish(), "inline /Contents");
    assert_eq!(report.annotation_texts_cleared, 1);
}

/// Gegenprobe: was eine Annotation **zeichnet**, bleibt. Der
/// Erscheinungsstrom geht wie Seitentext durch die Schwärzung — hier ohne
/// Schwärzung, also unverändert.
#[test]
fn der_erscheinungsstrom_einer_annotation_bleibt_erhalten() {
    let mut d = page(&["Harmloser Text"]);
    let font_id = d.font_id;
    let ap = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 200.into(), 20.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            b"BT /F1 10 Tf 2 5 Td (Sichtbarer Hinweis) Tj ET".to_vec(),
        )
        .with_compression(false),
    ));
    let id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FreeText",
        "Rect" => vec![72.into(), 100.into(), 272.into(), 120.into()],
        "Contents" => Object::string_literal("Sichtbarer Hinweis"),
        "AP" => dictionary! { "N" => ap },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(id)]));
    let (report, out) = pipeline(&d.finish());
    assert_eq!(report.annotation_texts_cleared, 1, "/Contents fällt");
    assert!(
        !leaks(&out, "Sichtbarer Hinweis").is_empty(),
        "der Erscheinungsstrom trägt den Text weiterhin"
    );
    assert!(
        report
            .summary()
            .iter()
            .any(|line| line.contains("Kommentartext an einer Annotation")),
        "{:?}",
        report.summary()
    );
}

/// Ohne Annotationen und Lesezeichen ändert sich am Bericht nichts — eine
/// gewöhnliche Datei bekommt keine neuen Zeilen.
#[test]
fn eine_datei_ohne_traeger_meldet_nichts_neues() {
    let d = page(&["Kontoinhaber: Max Mustermann"]);
    let (report, _) = pipeline(&d.finish());
    assert_eq!(report.outlines_removed, 0);
    assert_eq!(report.annotation_actions_removed, 0);
    assert_eq!(report.annotation_texts_cleared, 0);
    assert!(report.summary().is_empty(), "{:?}", report.summary());
}
