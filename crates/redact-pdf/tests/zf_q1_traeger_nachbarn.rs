//! Gegenprüfung Fix-Runde 5 (Q1): der besuchsgeführte Trägerlauf hat **keine
//! Tiefengrenze** und folgt jetzt auch eingebetteten Dictionaries hinter
//! `/Popup`, `/Parent`, `/Kids` und `/IRT`. Die Frage dieser Datei ist die
//! umgekehrte zur Lücke: nimmt er dabei etwas mit, das bleiben muss?
//!
//! Material: ein Träger, der an Dinge grenzt, die bleiben müssen — `/Parent`
//! auf den Seitenbaum, `/Kids` mit einer echten Seite, ein `/AcroForm
//! /Fields`, das auf die Seite zeigt, ein `/IRT` auf ein Struktur-Element,
//! ein Objekt, das zugleich Schrift und Träger-artig ist, `/MK` mit einem
//! `/I`-Icon-Verweis auf ein XObject, `/Opt` mit Arrays aus Arrays,
//! `/DR`-Ressourcen unter einem Feld.
//!
//! Orakel: Ausgabe lädt, Seitenzahl gleich, der gezogene Text ist
//! unverändert, Schrift, `/AP` und `/MK /I` stehen noch, und der Lauf endet
//! ohne Fehler. Die Bildgleichheit vor/nach ist außerhalb dieser Datei
//! gemessen (`redact-render` und `pdftoppm`, beide bytegleich) — `redact-pdf`
//! hat keine Abhängigkeit auf den Rasterer.

mod common;

use common::{page, Doc};
use lopdf::{dictionary, Object, ObjectId};
use redact_pdf::{load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor};

fn strip(bytes: &[u8]) -> Vec<u8> {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    strip_metadata(&mut doc);
    save_to_bytes(&doc).expect("Speichern")
}

fn text_of(bytes: &[u8]) -> String {
    let doc = load_from_bytes(bytes).expect("ladbar");
    PdfExtractor::new()
        .extract(&doc)
        .expect("Text lesbar")
        .iter()
        .map(|r| r.text.clone())
        .collect::<Vec<_>>()
        .join("|")
}

/// Baut die Datei aus dem Modulkommentar. Rückgabe: Bytes und die Ids, die
/// danach geprüft werden.
struct Nachbarn {
    bytes: Vec<u8>,
    page_id: ObjectId,
    pages_id: ObjectId,
    font_id: ObjectId,
    schriftlos: ObjectId,
    xobject: ObjectId,
    widget: ObjectId,
    dr: ObjectId,
}

fn nachbarn() -> Nachbarn {
    let mut d: Doc = page(&["Rechnung 4711 ueber 120,00 EUR"]);
    let (page_id, pages_id, font_id) = (d.page_id, d.pages_id, d.font_id);

    // Ein Form-XObject: es zeichnet die Erscheinung des Widgets und ist
    // zugleich das Icon in /MK /I.
    let xobject = d.add(Object::Stream(lopdf::Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 120.into(), 20.into()],
        },
        b"0 0 1 rg 0 0 120 20 re f\n".to_vec(),
    )));
    // Eine Schrift **ohne** `/Type` — damit ist sie für `is_carrier` ein
    // Träger. Ihre Schlüssel dürfen trotzdem nicht fallen.
    let schriftlos = d.add(Object::Dictionary(dictionary! {
        "Subtype" => "Type1",
        "BaseFont" => "Courier",
        "Encoding" => "WinAnsiEncoding",
    }));
    let dr = d.add(Object::Dictionary(dictionary! {
        "Font" => dictionary! { "Helv" => Object::Reference(font_id) },
    }));
    let struct_elem = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "P",
        "Alt" => Object::string_literal("Alternativtext"),
    }));

    let widget = d.doc.new_object_id();
    let feld = d.doc.new_object_id();
    let wurzel = d.doc.new_object_id();
    let auswahl = d.doc.new_object_id();

    d.doc.objects.insert(
        widget,
        Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "Rect" => vec![72.into(), 600.into(), 192.into(), 620.into()],
            "Parent" => Object::Reference(feld),
            "AP" => dictionary! { "N" => Object::Reference(xobject) },
            "MK" => dictionary! {
                "BG" => vec![Object::Real(0.9), Object::Real(0.9), Object::Real(0.9)],
                "I" => Object::Reference(xobject),
            },
            "DA" => Object::string_literal("/Helv 8 Tf 0 g"),
            "IRT" => Object::Reference(struct_elem),
        }),
    );
    d.doc.objects.insert(
        feld,
        Object::Dictionary(dictionary! {
            "T" => Object::string_literal("Feld1"),
            "FT" => "Tx",
            // `/Kids` nennt neben dem Widget die **Schrift** und die echte Seite.
            "Kids" => vec![
                Object::Reference(widget),
                Object::Reference(schriftlos),
                Object::Reference(page_id),
            ],
            "Parent" => Object::Reference(wurzel),
            "DR" => Object::Reference(dr),
        }),
    );
    d.doc.objects.insert(
        auswahl,
        Object::Dictionary(dictionary! {
            "T" => Object::string_literal("Feld2"),
            "FT" => "Ch",
            // `/Parent` zeigt auf den Seitenbaum (kaputt geschriebene Datei).
            "Parent" => Object::Reference(pages_id),
            "Opt" => Object::Array(vec![
                Object::Array(vec![
                    Object::string_literal("a"),
                    Object::Array(vec![Object::string_literal("Auswahl A")]),
                ]),
                Object::Array(vec![Object::string_literal("b")]),
            ]),
        }),
    );
    d.doc.objects.insert(
        wurzel,
        Object::Dictionary(dictionary! {
            "T" => Object::string_literal("Wurzel"),
            "Kids" => vec![Object::Reference(feld), Object::Reference(auswahl)],
        }),
    );
    let acroform = d.add(Object::Dictionary(dictionary! {
        // Die zweite Wurzel des Laufs nennt neben dem Feldbaum die Seite.
        "Fields" => vec![Object::Reference(wurzel), Object::Reference(page_id)],
        "DR" => Object::Reference(dr),
    }));
    d.catalog_set("AcroForm", Object::Reference(acroform));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(widget)]));
    // Das XObject muss aus den Seitenressourcen erreichbar sein, sonst räumt
    // es der Lauf zu Recht weg.
    let resources = d.resources_id;
    d.doc
        .get_dictionary_mut(resources)
        .expect("Ressourcen")
        .set(
            "XObject",
            Object::Dictionary(dictionary! { "Xf1" => Object::Reference(xobject) }),
        );

    Nachbarn {
        bytes: d.finish(),
        page_id,
        pages_id,
        font_id,
        schriftlos,
        xobject,
        widget,
        dr,
    }
}

/// **Der Kern.** Nichts, was die Seite braucht, fällt.
///
/// Mutationsnachweis: in `meta.rs` `is_carrier` so ändern, dass ein fremdes
/// `/Type` auch als Träger gilt (`Some(_) => false` → `Some(_) => true`) —
/// dann verliert die Seite `/Contents` und der Test ist rot.
#[test]
fn der_lauf_nimmt_der_seite_ihren_nachbarn_nichts() {
    let n = nachbarn();
    let vorher = text_of(&n.bytes);
    let out = strip(&n.bytes);
    let doc = load_from_bytes(&out).expect("Ausgabe lädt");

    assert_eq!(doc.get_pages().len(), 1, "Seitenzahl");
    assert_eq!(text_of(&out), vorher, "der gezogene Text muss gleich sein");

    let page = doc.get_dictionary(n.page_id).expect("Seite steht noch");
    assert!(page.get(b"Contents").is_ok(), "/Contents der Seite");
    assert!(page.get(b"Resources").is_ok(), "/Resources der Seite");
    let pages = doc
        .get_dictionary(n.pages_id)
        .expect("Seitenbaum steht noch");
    assert!(pages.get(b"Kids").is_ok(), "/Kids des Seitenbaums");
    assert!(pages.get(b"Count").is_ok(), "/Count des Seitenbaums");
}

/// Schriften bleiben — auch eine ohne `/Type`, die der Lauf als Träger
/// besucht, und die im `/DR` eines Feldes genannte.
#[test]
fn schriften_und_ressourcen_bleiben_vollstaendig() {
    let n = nachbarn();
    let out = strip(&n.bytes);
    let doc = load_from_bytes(&out).expect("Ausgabe lädt");

    let font = doc.get_dictionary(n.font_id).expect("Seitenschrift");
    assert_eq!(
        font.get(b"BaseFont").and_then(Object::as_name).ok(),
        Some(b"Helvetica".as_slice())
    );
    let ohne_type = doc
        .get_dictionary(n.schriftlos)
        .expect("Schrift ohne /Type steht noch");
    for key in [b"Subtype".as_slice(), b"BaseFont", b"Encoding"] {
        assert!(
            ohne_type.get(key).is_ok(),
            "einer Schrift ohne /Type fehlt {} nach dem Lauf: {ohne_type:?}",
            String::from_utf8_lossy(key)
        );
    }
    let dr = doc.get_dictionary(n.dr).expect("/DR steht noch");
    assert!(dr.get(b"Font").is_ok(), "/DR /Font");
}

/// `/AP` und das Icon in `/MK /I` bleiben; nur die Beschriftungen `/CA`,
/// `/RC`, `/AC` fallen dort.
#[test]
fn erscheinungsstrom_und_mk_icon_bleiben() {
    let n = nachbarn();
    let out = strip(&n.bytes);
    let doc = load_from_bytes(&out).expect("Ausgabe lädt");

    assert!(
        doc.objects.contains_key(&n.xobject),
        "das XObject hinter /AP und /MK /I ist weg"
    );
    let widget = doc.get_dictionary(n.widget).expect("Widget steht noch");
    assert!(widget.get(b"AP").is_ok(), "/AP der Annotation");
    assert!(widget.get(b"Rect").is_ok(), "/Rect der Annotation");
    let mk = widget
        .get(b"MK")
        .and_then(Object::as_dict)
        .expect("/MK steht noch");
    assert!(mk.get(b"I").is_ok(), "/MK /I (Icon-Verweis)");
    assert!(mk.get(b"BG").is_ok(), "/MK /BG (Hintergrundfarbe)");
}
