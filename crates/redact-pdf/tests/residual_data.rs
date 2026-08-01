//! Restdaten: Stellen, an denen dasselbe Geheimnis steht wie im
//! Content-Stream — und die niemand angefasst hat (Aufgabe #4).
//!
//! Formularfeldwerte (`/V`), Dateianhänge, JavaScript, Öffnen- und
//! Ereignisaktionen und die Ebenenverwaltung transportieren Text genauso wie
//! eine Textoperation. Wird nur der Content-Stream bereinigt, ist die Datei
//! „geschwärzt“ und das Geheimnis trotzdem drin.
//!
//! ## Orakel
//!
//! Geprüft wird ausschließlich mit [`redact_pdf::leaks`] gegen die
//! **geschriebenen Bytes**. Der eigene Extraktor wäre ein Zirkelschluss: wovor
//! er blind ist, das schwärzt das Werkzeug nicht — und der Test sähe es
//! ebenfalls nicht.

mod common;

use common::SECRET;
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfRedactor};

// ---------------------------------------------------------------------------
// Werkzeug
// ---------------------------------------------------------------------------

/// Die vollständige Verarbeitung, so wie CLI und GUI sie fahren.
fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> (Vec<u8>, redact_pdf::MetadataReport) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, redactions)
        .expect("Schwärzung");
    let metadata = strip_metadata(&mut doc);
    (save_to_bytes(&doc).expect("Speichern"), metadata)
}

/// Deckt die beiden Textzeilen ab, die [`common::page`] setzt (y = 700 und 685).
fn whole_text_area() -> Redaction {
    Redaction::new(
        Region::new(
            0,
            Rect::new(40.0, 600.0, 560.0, 760.0),
            None,
            Source::Manual {
                reason: "Restdaten-Test".into(),
            },
        ),
        Action::Blackout,
    )
}

#[track_caller]
fn assert_no_leak(bytes: &[u8], what: &str) {
    let hits = leaks(bytes, SECRET);
    assert!(
        hits.is_empty(),
        "{what}: „{SECRET}“ steht noch {} mal in der Ausgabe:\n{}",
        hits.len(),
        hits.join("\n")
    );
}

/// Belegt, dass die Testdatei das Geheimnis überhaupt an der gemeinten Stelle
/// trägt — sonst misst der Test hinterher nichts.
#[track_caller]
fn assert_present(bytes: &[u8], what: &str) {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "Testdaten taugen nicht: {what} enthält das Geheimnis gar nicht"
    );
}

/// Ein Grundgerüst mit sichtbarem Text; die Bausteine hängen sich daran.
fn base() -> common::Doc {
    common::page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")])
}

fn attachment_stream(doc: &mut Document) -> ObjectId {
    let mut stream = Stream::new(
        Dictionary::new(),
        format!("Kontoauszug\r\nIBAN: {SECRET}\r\n").into_bytes(),
    );
    // Komprimiert — eine reine Rohbyte-Suche würde den Anhang sonst gar nicht
    // erst brauchen, um ihn zu finden.
    stream.compress().expect("komprimierbar");
    doc.add_object(Object::Stream(stream))
}

// ---------------------------------------------------------------------------
// /AcroForm — Feldwerte
// ---------------------------------------------------------------------------

#[test]
fn acroform_field_values_do_not_survive_the_redaction() {
    let mut d = base();
    let field = d.add(Object::Dictionary(dictionary! {
        "FT" => "Tx",
        "T" => Object::string_literal("iban"),
        // `/V` als UTF-16BE mit BOM — die Form, in der Acrobat schreibt.
        "V" => Object::String(common::utf16be_bom(SECRET), StringFormat::Literal),
        "DV" => Object::string_literal(SECRET),
    }));
    d.catalog_set(
        "AcroForm",
        Object::Dictionary(dictionary! {
            "Fields" => vec![Object::Reference(field)],
        }),
    );
    let pdf = d.finish();
    assert_present(&pdf, "das AcroForm-Feld");

    let (out, meta) = pipeline(&pdf, &[whole_text_area()]);
    assert!(meta.acroform_removed);
    assert_eq!(meta.field_values_cleared, 2);
    assert_no_leak(&out, "AcroForm /V und /DV");
}

#[test]
fn a_widget_that_hangs_on_the_page_loses_its_value_too() {
    // Der Fall, den das bloße Entfernen von `/AcroForm` nicht erschlägt: das
    // Feld ist zusätzlich als Widget-Annotation an der Seite verankert und
    // bleibt deshalb erreichbar.
    let mut d = base();
    let field = d.add(Object::Dictionary(dictionary! {
        "FT" => "Tx",
        "T" => Object::string_literal("iban"),
        "V" => Object::String(common::utf16be_bom(SECRET), StringFormat::Literal),
    }));
    let widget = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "Parent" => Object::Reference(field),
        // Weit weg von jeder Schwärzung — die Annotation überlebt also.
        "Rect" => vec![400.into(), 100.into(), 540.into(), 120.into()],
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(widget)]));
    d.catalog_set(
        "AcroForm",
        Object::Dictionary(dictionary! {
            "Fields" => vec![Object::Reference(field)],
        }),
    );
    let pdf = d.finish();
    assert_present(&pdf, "das Widget-Elternfeld");

    let (out, meta) = pipeline(&pdf, &[whole_text_area()]);
    assert_eq!(meta.field_values_cleared, 1);
    assert_no_leak(&out, "/V des über /Annots erreichbaren Feldes");
}

#[test]
fn xfa_form_data_does_not_survive() {
    // XFA ist ein zweiter, vollständiger Datensatz desselben Formulars — als
    // XML, in einem eigenen Strom.
    let mut d = base();
    let xfa = d.add(Object::Stream(
        Stream::new(
            Dictionary::new(),
            format!("<xfa:data><iban>{SECRET}</iban></xfa:data>").into_bytes(),
        )
        .with_compression(false),
    ));
    d.catalog_set(
        "AcroForm",
        Object::Dictionary(dictionary! {
            "Fields" => Object::Array(vec![]),
            "XFA" => vec![
                Object::string_literal("datasets"),
                Object::Reference(xfa),
            ],
        }),
    );
    let pdf = d.finish();
    assert_present(&pdf, "der XFA-Datensatz");

    let (out, meta) = pipeline(&pdf, &[whole_text_area()]);
    assert!(meta.xfa_removed);
    assert_no_leak(&out, "/AcroForm /XFA");
}

// ---------------------------------------------------------------------------
// /Names — eingebettete Dateien und JavaScript
// ---------------------------------------------------------------------------

#[test]
fn an_embedded_file_does_not_survive() {
    let mut d = base();
    let data = attachment_stream(&mut d.doc);
    let filespec = d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("kontoauszug.txt"),
        "UF" => Object::String(common::utf16be_bom("kontoauszug.txt"), StringFormat::Literal),
        "EF" => dictionary! { "F" => Object::Reference(data) },
    }));
    let names = d.add(Object::Dictionary(dictionary! {
        "EmbeddedFiles" => dictionary! {
            "Names" => vec![
                Object::string_literal("anhang"),
                Object::Reference(filespec),
            ],
        },
    }));
    d.catalog_set("Names", Object::Reference(names));
    let pdf = d.finish();
    assert_present(&pdf, "der Dateianhang");

    let (out, meta) = pipeline(&pdf, &[whole_text_area()]);
    assert_eq!(meta.embedded_files_removed, 1);
    assert_no_leak(&out, "/Names /EmbeddedFiles");
}

#[test]
fn a_file_attachment_annotation_does_not_survive() {
    // Ein Anhang muss nicht im `/Names`-Baum hängen — er kann auch direkt an
    // einer Seite kleben, weit weg von jedem Schwärzungsbereich.
    let mut d = base();
    let data = attachment_stream(&mut d.doc);
    let annot = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FileAttachment",
        "Rect" => vec![400.into(), 100.into(), 420.into(), 120.into()],
        "Contents" => Object::string_literal(format!("Anhang zur IBAN {SECRET}")),
        "FS" => dictionary! {
            "Type" => "Filespec",
            "F" => Object::string_literal("anhang.txt"),
            "EF" => dictionary! { "F" => Object::Reference(data) },
        },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot)]));
    let pdf = d.finish();
    assert_present(&pdf, "die Anhang-Annotation");

    let (out, meta) = pipeline(&pdf, &[whole_text_area()]);
    assert_eq!(meta.file_attachments_removed, 1);
    assert_no_leak(&out, "/FileAttachment-Annotation");
}

#[test]
fn document_level_javascript_does_not_survive() {
    let mut d = base();
    let names = d.add(Object::Dictionary(dictionary! {
        "JavaScript" => dictionary! {
            "Names" => vec![
                Object::string_literal("init"),
                Object::Dictionary(dictionary! {
                    "S" => "JavaScript",
                    "JS" => Object::string_literal(format!("var iban = \"{SECRET}\";")),
                }),
            ],
        },
    }));
    d.catalog_set("Names", Object::Reference(names));
    let pdf = d.finish();
    assert_present(&pdf, "das Dokument-JavaScript");

    let (out, meta) = pipeline(&pdf, &[whole_text_area()]);
    assert_eq!(meta.javascript_removed, 1);
    assert_no_leak(&out, "/Names /JavaScript");
}

// ---------------------------------------------------------------------------
// Aktionen und Ebenen
// ---------------------------------------------------------------------------

#[test]
fn open_action_javascript_does_not_survive() {
    let mut d = base();
    let action = d.add(Object::Dictionary(dictionary! {
        "S" => "JavaScript",
        "JS" => Object::String(
            common::utf16be_bom(&format!("app.alert('{SECRET}');")),
            StringFormat::Literal,
        ),
    }));
    d.catalog_set("OpenAction", Object::Reference(action));
    let pdf = d.finish();
    assert_present(&pdf, "die Öffnen-Aktion");

    let (out, meta) = pipeline(&pdf, &[whole_text_area()]);
    assert!(meta.open_action_removed);
    assert_no_leak(&out, "/OpenAction");
}

#[test]
fn additional_actions_on_catalog_and_page_do_not_survive() {
    let mut d = base();
    d.catalog_set(
        "AA",
        Object::Dictionary(dictionary! {
            "WC" => dictionary! {
                "S" => "JavaScript",
                "JS" => Object::string_literal(format!("// schliessen {SECRET}")),
            },
        }),
    );
    d.page_dict_set(
        "AA",
        Object::Dictionary(dictionary! {
            "O" => dictionary! {
                "S" => "JavaScript",
                "JS" => Object::string_literal(format!("// oeffnen {SECRET}")),
            },
        }),
    );
    let pdf = d.finish();
    assert_present(&pdf, "die Ereignisaktionen");

    let (out, meta) = pipeline(&pdf, &[whole_text_area()]);
    assert_eq!(meta.additional_actions_removed, 2);
    assert_no_leak(&out, "/AA in Katalog und Seite");
}

#[test]
fn optional_content_properties_do_not_survive() {
    let mut d = base();
    let ocg = d.add(Object::Dictionary(dictionary! {
        "Type" => "OCG",
        "Name" => Object::String(
            common::utf16be_bom(&format!("Ebene {SECRET}")),
            StringFormat::Literal,
        ),
    }));
    d.catalog_set(
        "OCProperties",
        Object::Dictionary(dictionary! {
            "OCGs" => vec![Object::Reference(ocg)],
            "D" => dictionary! { "ON" => vec![Object::Reference(ocg)] },
        }),
    );
    let pdf = d.finish();
    assert_present(&pdf, "die Ebenenverwaltung");

    let (out, meta) = pipeline(&pdf, &[whole_text_area()]);
    assert!(meta.optional_content_removed);
    assert_no_leak(&out, "/OCProperties");
}

// ---------------------------------------------------------------------------
// Seiten-/Metadata und die Gegenprobe
// ---------------------------------------------------------------------------

#[test]
fn page_level_xmp_does_not_survive() {
    let pdf = common::page_metadata_xmp(SECRET);
    assert_present(&pdf, "das seitenweite XMP");

    let (out, meta) = pipeline(&pdf, &[whole_text_area()]);
    assert!(meta.xmp_removed);
    assert_no_leak(&out, "seitenweites /Metadata");
}

/// Alles auf einmal — so sieht ein echtes Formular-PDF aus.
#[test]
fn everything_at_once_leaves_nothing_behind() {
    let mut d = base();
    let data = attachment_stream(&mut d.doc);
    let filespec = d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("anhang.txt"),
        "EF" => dictionary! { "F" => Object::Reference(data) },
    }));
    let field = d.add(Object::Dictionary(dictionary! {
        "FT" => "Tx",
        "T" => Object::string_literal("iban"),
        "V" => Object::String(common::utf16be_bom(SECRET), StringFormat::Literal),
    }));
    let names = d.add(Object::Dictionary(dictionary! {
        "EmbeddedFiles" => dictionary! {
            "Names" => vec![Object::string_literal("anhang"), Object::Reference(filespec)],
        },
        "JavaScript" => dictionary! {
            "Names" => vec![
                Object::string_literal("init"),
                Object::Dictionary(dictionary! {
                    "S" => "JavaScript",
                    "JS" => Object::string_literal(format!("var x='{SECRET}';")),
                }),
            ],
        },
    }));
    let action = d.add(Object::Dictionary(dictionary! {
        "S" => "JavaScript",
        "JS" => Object::string_literal(format!("app.alert('{SECRET}');")),
    }));
    d.catalog_set("Names", Object::Reference(names));
    d.catalog_set("OpenAction", Object::Reference(action));
    d.catalog_set(
        "AcroForm",
        Object::Dictionary(dictionary! {
            "Fields" => vec![Object::Reference(field)],
            "XFA" => Object::string_literal(format!("<xfa>{SECRET}</xfa>")),
        }),
    );
    d.page_dict_set(
        "AA",
        Object::Dictionary(dictionary! {
            "O" => dictionary! {
                "S" => "JavaScript",
                "JS" => Object::string_literal(format!("// {SECRET}")),
            },
        }),
    );
    let pdf = d.finish();
    assert_present(&pdf, "das Formular-PDF");

    let (out, meta) = pipeline(&pdf, &[whole_text_area()]);
    assert_no_leak(&out, "Formular-PDF mit allen Restdatenstellen");

    // Jede Entfernung muss im Bericht auftauchen — sonst kann sie nicht ins
    // Audit-Log.
    assert!(meta.acroform_removed);
    assert!(meta.xfa_removed);
    assert!(meta.open_action_removed);
    assert_eq!(meta.field_values_cleared, 1);
    assert_eq!(meta.embedded_files_removed, 1);
    assert_eq!(meta.javascript_removed, 1);
    assert_eq!(meta.additional_actions_removed, 1);
    assert_eq!(meta.names_removed, 1);

    let summary = meta.summary();
    for expected in [
        "/AcroForm",
        "/XFA",
        "/OpenAction",
        "Feldwert",
        "eingebettete Datei",
        "JavaScript-Eintrag",
    ] {
        assert!(
            summary.iter().any(|s| s.contains(expected)),
            "„{expected}“ fehlt in der Zusammenfassung: {summary:?}"
        );
    }
}

/// Gegenprobe: ein Dokument ohne Restdaten meldet auch keine.
#[test]
fn a_document_without_residual_data_reports_nothing_extra() {
    let pdf = base().finish();
    let (_, meta) = pipeline(&pdf, &[whole_text_area()]);
    assert!(!meta.acroform_removed);
    assert!(!meta.open_action_removed);
    assert!(!meta.optional_content_removed);
    assert_eq!(meta.field_values_cleared, 0);
    assert_eq!(meta.embedded_files_removed, 0);
    assert_eq!(meta.javascript_removed, 0);
    assert_eq!(meta.file_attachments_removed, 0);
    assert!(meta.summary().is_empty(), "{:?}", meta.summary());
}
