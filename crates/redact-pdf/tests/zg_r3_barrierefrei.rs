//! Gegenprüfung Fix-Runde 6 (R3): **zu viel entfernt?** `/Alt` und
//! `/ActualText` fallen jetzt an jedem Dictionary, das der Trägerlauf
//! erreicht. Was erreicht er an einer gewöhnlichen barrierefreien Datei?
//!
//! Material (PDF/UA-artig): `/MarkInfo`, `/Lang`, `/StructTreeRoot` mit
//! `/Document` → `/P` (mit `/ActualText`), `/Figure` (mit `/Alt`), `/Form`
//! und `/Link` (je mit `/OBJR` auf die Annotation), `/ParentTree`; ein
//! Formularfeld mit `/TU`; ein Link mit ausdrücklichem `/Dest`; markierter
//! Seiteninhalt (`BDC … EMC` mit `/MCID`). Kein Geheimnis in der Datei.
//!
//! Erwartung nach dem Code: der Lauf erreicht die Struktur-Elemente **nicht**
//! (`/StructParent` ist eine Zahl, `/OBJR` zeigt von der Struktur zur
//! Annotation, nicht umgekehrt). Die Barrierefreiheit fällt trotzdem — mit
//! `/StructTreeRoot`, wie seit jeher dokumentiert — und der Bericht sagt es.

mod common;

use common::{page, Doc};
use lopdf::{dictionary, Object};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport, PdfExtractor,
};

fn strip(bytes: &[u8]) -> (MetadataReport, Vec<u8>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = strip_metadata(&mut doc);
    (report, save_to_bytes(&doc).expect("Speichern"))
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

struct Ua {
    bytes: Vec<u8>,
    figure: lopdf::ObjectId,
    widget: lopdf::ObjectId,
    link: lopdf::ObjectId,
    ap: lopdf::ObjectId,
}

fn barrierefrei() -> Ua {
    let mut d: Doc = page(&[]);
    let (page_id, font_id) = (d.page_id, d.font_id);
    d.set_content(
        b"/P <</MCID 0>> BDC BT /F1 10 Tf 72 700 Td (Rechnung 4711 ueber 120,00 EUR) Tj ET EMC\n\
          /Figure <</MCID 1>> BDC 0 0 1 rg 72 600 50 50 re f EMC\n\
          /Link <</MCID 2>> BDC BT /F1 10 Tf 72 560 Td (Zur Bank) Tj ET EMC\n",
    );
    let ap = d.add(Object::Stream(lopdf::Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 120.into(), 20.into()],
        },
        b"0.9 g 0 0 120 20 re f\n".to_vec(),
    )));
    let widget = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Tx",
        "T" => Object::string_literal("name"),
        "TU" => Object::string_literal("Ihr Name (Pflichtfeld)"),
        "Rect" => vec![72.into(), 500.into(), 192.into(), 520.into()],
        "F" => 4,
        "P" => Object::Reference(page_id),
        "DA" => Object::string_literal("/Helv 10 Tf 0 g"),
        "AP" => dictionary! { "N" => Object::Reference(ap) },
        "StructParent" => 1,
    }));
    let link = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![72.into(), 555.into(), 150.into(), 570.into()],
        "Contents" => Object::string_literal("Zur Bank"),
        "Dest" => vec![Object::Reference(page_id), "XYZ".into(), 0.into(), 0.into(), 0.into()],
        "StructParent" => 2,
    }));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(widget), Object::Reference(link)]),
    );
    d.page_dict_set("StructParents", 0.into());
    d.page_dict_set("Tabs", "S".into());

    let doc_elem = d.doc.new_object_id();
    let p = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "P",
        "P" => Object::Reference(doc_elem),
        "Pg" => Object::Reference(page_id),
        "K" => 0,
        "ActualText" => Object::string_literal("Rechnung 4711 über 120,00 EUR"),
    }));
    let figure = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "Figure",
        "P" => Object::Reference(doc_elem),
        "Pg" => Object::Reference(page_id),
        "K" => 1,
        "Alt" => Object::string_literal("Logo der Musterbank"),
    }));
    let form = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "Form",
        "P" => Object::Reference(doc_elem),
        "Pg" => Object::Reference(page_id),
        "K" => dictionary! { "Type" => "OBJR", "Obj" => Object::Reference(widget) },
    }));
    let link_elem = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "Link",
        "P" => Object::Reference(doc_elem),
        "Pg" => Object::Reference(page_id),
        "K" => vec![
            2.into(),
            Object::Dictionary(dictionary! { "Type" => "OBJR", "Obj" => Object::Reference(link) }),
        ],
    }));
    let root = d.doc.new_object_id();
    d.doc.objects.insert(
        doc_elem,
        Object::Dictionary(dictionary! {
            "Type" => "StructElem",
            "S" => "Document",
            "P" => Object::Reference(root),
            "K" => vec![
                Object::Reference(p),
                Object::Reference(figure),
                Object::Reference(form),
                Object::Reference(link_elem),
            ],
        }),
    );
    let parent_tree = d.add(Object::Dictionary(dictionary! {
        "Nums" => vec![
            0.into(),
            Object::Array(vec![Object::Reference(p), Object::Reference(figure), Object::Reference(link_elem)]),
            1.into(),
            Object::Reference(form),
            2.into(),
            Object::Reference(link_elem),
        ],
    }));
    d.doc.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "StructTreeRoot",
            "K" => Object::Reference(doc_elem),
            "ParentTree" => Object::Reference(parent_tree),
            "ParentTreeNextKey" => 3,
        }),
    );
    d.catalog_set("StructTreeRoot", Object::Reference(root));
    d.catalog_set("MarkInfo", dictionary! { "Marked" => true }.into());
    d.catalog_set("Lang", Object::string_literal("de-DE"));
    d.catalog_set(
        "AcroForm",
        dictionary! {
            "Fields" => vec![Object::Reference(widget)],
            "DR" => dictionary! { "Font" => dictionary! { "Helv" => Object::Reference(font_id) } },
            "DA" => Object::string_literal("/Helv 0 Tf 0 g"),
        }
        .into(),
    );
    Ua {
        bytes: d.finish(),
        figure,
        widget,
        link,
        ap,
    }
}

/// Der Lauf erreicht an dieser Datei kein Struktur-Element: gezählt werden
/// genau `/T`, `/TU` und `/Contents` — kein `/Alt`, kein `/ActualText`.
/// Die Struktur fällt als Ganzes, und der Bericht nennt sie.
///
/// Mutation, die diesen Test rot macht: `take(catalog, b"StructTreeRoot",
/// …)` weglassen — dann steht die Struktur samt `/Alt` noch da, und der
/// Bericht behauptet nichts mehr über sie.
#[test]
fn barrierefreie_datei_verliert_nur_was_der_bericht_nennt() {
    let ua = barrierefrei();
    let (report, out) = strip(&ua.bytes);

    // Was fiel — und der Bericht sagt es.
    assert!(report.struct_tree_removed, "{:?}", report.summary());
    assert!(report.acroform_removed);
    assert_eq!(
        report.annotation_texts_cleared,
        3,
        "/T, /TU, /Contents — nicht mehr: {:?}",
        report.summary()
    );
    assert_eq!(
        report.annotation_actions_removed, 0,
        "ein ausdrückliches /Dest bleibt"
    );
    let zusammenfassung = report.summary().join("\n");
    assert!(zusammenfassung.contains("Dokumentstruktur (/StructTreeRoot)"));
    assert!(zusammenfassung.contains("/TU"));

    // Die Struktur ist weg — samt /Alt und /ActualText (mit ihr, nicht am
    // Struktur-Element gezielt).
    let doc = load_from_bytes(&out).expect("Ausgabe lädt");
    assert!(
        !doc.objects.contains_key(&ua.figure),
        "die Struktur fällt als Ganzes"
    );
    assert!(leaks(&out, "Logo der Musterbank").is_empty());
    assert!(leaks(&out, "Ihr Name (Pflichtfeld)").is_empty());

    // Was bleiben muss: Seitentext, markierter Inhalt, Sprache, das Widget
    // samt /AP, der Link samt /Dest.
    assert_eq!(text_of(&out), text_of(&ua.bytes));
    assert!(
        !leaks(&out, "/P <</MCID 0>> BDC").is_empty(),
        "markierter Inhalt bleibt"
    );
    let catalog = doc
        .trailer
        .get(b"Root")
        .and_then(|r| doc.get_dictionary(r.as_reference().unwrap()))
        .unwrap();
    assert!(catalog.get(b"Lang").is_ok(), "/Lang bleibt");
    assert!(
        catalog.get(b"MarkInfo").is_err(),
        "/MarkInfo fällt mit der Struktur"
    );
    let widget = doc.get_dictionary(ua.widget).expect("Widget steht");
    assert!(
        widget.get(b"AP").is_ok() && doc.objects.contains_key(&ua.ap),
        "/AP bleibt"
    );
    assert!(widget.get(b"DA").is_ok(), "/DA bleibt (benannte Lücke)");
    let link = doc.get_dictionary(ua.link).expect("Link steht");
    assert!(link.get(b"Dest").is_ok(), "ausdrückliches /Dest bleibt");
    assert!(link.get(b"Contents").is_err());
}

/// Ein Widget, dessen `/Parent`-Feld über `/Kids` ein zweites Widget auf
/// einer Notiz-Antwortkette hält: `/IRT` zeigt auf eine gewöhnliche
/// Annotation, nicht auf ein Struktur-Element. Auch dann verliert kein
/// Struktur-Element seinen `/Alt` — es wird nicht erreicht.
///
/// Mutation, die diesen Test rot macht: die Besuchsmenge / der Lauf über
/// alle Objekte (wer `/Alt` überall leert, nimmt hier „Logo der Musterbank“).
#[test]
fn irt_auf_eine_annotation_erreicht_kein_strukturelement() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let figure = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "Figure",
        "Alt" => Object::string_literal("Logo der Musterbank"),
    }));
    let root = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructTreeRoot",
        "K" => Object::Reference(figure),
    }));
    d.catalog_set("StructTreeRoot", Object::Reference(root));
    // Ein zweiter Halter (kein Träger) rettet das Element über das Aufräumen.
    d.page_dict_set("Zusatz", Object::Reference(figure));
    let erste = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "Contents" => Object::string_literal("Bitte prüfen."),
    }));
    let antwort = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![40.into(), 10.into(), 60.into(), 30.into()],
        "IRT" => Object::Reference(erste),
        "Contents" => Object::string_literal("Erledigt."),
    }));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(erste), Object::Reference(antwort)]),
    );
    let (report, out) = strip(&d.finish());
    assert_eq!(report.annotation_texts_cleared, 2, "{:?}", report.summary());
    assert!(
        !leaks(&out, "Logo der Musterbank").is_empty(),
        "ein Struktur-Element, das keine Annotation erreicht, behält seinen /Alt"
    );
}
