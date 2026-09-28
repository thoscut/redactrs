//! Register #94: ein `/Annots`, das sich Seiten teilen.
//!
//! Nach PDF 32000-1, 12.5.2 steht eine Annotation im `/Annots` genau einer
//! Seite. Teilten sich P Seiten dasselbe Array mit N Annotationen, las die
//! Analyse P × N Erscheinungsströme, der Metadatenlauf gab jeder Seite eine
//! eigene Kopie des Arrays, und der Redaktor klonte je Seite jedes
//! Dictionary — ohne dass ein Konto es sah. Die Proben hier prüfen, was der
//! Fix verspricht: gelesen wird unter derselben Ressourcenumgebung einmal,
//! unter einer anderen erneut; bereinigt wird das geteilte Array an seinem
//! Objekt, einmal.

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream, StringFormat};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

/// Ein Erscheinungsstrom, der `text` zeigt — mit eigenen Ressourcen oder
/// ohne, dann gelten die der Seite.
fn appearance(d: &mut Doc, text: &str, own_resources: bool) -> ObjectId {
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), 200.into(), 20.into()],
    };
    if own_resources {
        dict.set(
            "Resources",
            dictionary! { "Font" => dictionary! { "F1" => Object::Reference(d.font_id) } },
        );
    }
    d.add(Object::Stream(Stream::new(
        dict,
        format!("BT /F1 10 Tf 2 5 Td ({text}) Tj ET\n").into_bytes(),
    )))
}

fn free_text(d: &mut Doc, ap: ObjectId, y: i64) -> ObjectId {
    d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FreeText",
        "Rect" => vec![50.into(), y.into(), 250.into(), (y + 20).into()],
        "AP" => dictionary! { "N" => Object::Reference(ap) },
    }))
}

/// `n` Annotationen „Notiz 0“ … in einem Array als eigenem Objekt.
fn notizen(d: &mut Doc, n: usize, own_resources: bool) -> (ObjectId, Vec<ObjectId>) {
    let mut ids = Vec::new();
    for i in 0..n {
        let ap = appearance(d, &format!("Notiz {i}"), own_resources);
        ids.push(free_text(d, ap, 100 + 25 * i as i64));
    }
    let items: Vec<Object> = ids.iter().map(|id| Object::Reference(*id)).collect();
    (d.add(Object::Array(items)), ids)
}

/// Hängt `extra` Kopien der ersten Seite an und gibt alle Seiten zurück.
fn weitere_seiten(d: &mut Doc, extra: usize) -> Vec<ObjectId> {
    let template = d.doc.get_dictionary(d.page_id).expect("Seite").clone();
    let mut ids = vec![d.page_id];
    for _ in 0..extra {
        ids.push(d.add(Object::Dictionary(template.clone())));
    }
    let kids: Vec<Object> = ids.iter().map(|id| Object::Reference(*id)).collect();
    let pages = d.doc.get_dictionary_mut(d.pages_id).expect("Seitenbaum");
    pages.set("Count", ids.len() as i64);
    pages.set("Kids", kids);
    ids
}

fn seiten_mit(runs: &[redact_core::TextRun], text: &str) -> Vec<usize> {
    runs.iter()
        .filter(|r| r.text.contains(text))
        .map(|r| r.page)
        .collect()
}

fn zone(page: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "geteiltes /Annots".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Vierzig Seiten, ein Array: jede Notiz steht **einmal** im Ergebnis, auf
/// der ersten Seite. Vorher stand sie auf jeder der vierzig Seiten — das war
/// die Arbeit Seiten × Annotationen.
#[test]
fn ein_geteiltes_array_wird_einmal_gelesen() {
    let mut d = page(&["Hallo"]);
    let (array, _) = notizen(&mut d, 12, true);
    d.page_dict_set("Annots", Object::Reference(array));
    weitere_seiten(&mut d, 39);
    let doc = load_from_bytes(&d.finish()).expect("ladbar");

    let runs = PdfExtractor::new().extract(&doc).expect("Extraktion");
    for i in [0, 5, 11] {
        assert_eq!(
            seiten_mit(&runs, &format!("Notiz {i}")),
            vec![0],
            "Notiz {i} gehört einmal ins Ergebnis, auf die erste Seite, die sie zeigt"
        );
    }
    assert_eq!(
        seiten_mit(&runs, "Hallo").len(),
        40,
        "der Seitentext bleibt je Seite"
    );
}

/// Dieselben Annotationen in je einem **eigenen** Array jeder Seite, unter
/// denselben Ressourcen: auch hier einmal. Das Buch zählt Annotationen, nicht
/// nur Arrays.
#[test]
fn dieselben_annotationen_in_eigenen_arrays_werden_einmal_gelesen() {
    let mut d = page(&["Hallo"]);
    let (_, ids) = notizen(&mut d, 6, true);
    let pages = weitere_seiten(&mut d, 9);
    for page_id in pages {
        let items: Vec<Object> = ids.iter().map(|id| Object::Reference(*id)).collect();
        d.doc
            .get_dictionary_mut(page_id)
            .expect("Seite")
            .set("Annots", Object::Array(items));
    }
    let doc = load_from_bytes(&d.finish()).expect("ladbar");

    let runs = PdfExtractor::new().extract(&doc).expect("Extraktion");
    assert_eq!(seiten_mit(&runs, "Notiz 3"), vec![0]);
}

/// Ein Erscheinungsstrom **ohne** eigene Ressourcen benutzt die der Seite.
/// Zeigt Seite 2 dieselbe Annotation mit einer anderen Schrift unter `/F1`,
/// sieht der Betrachter dort anderen Text — hier das Geheimnis. Wer die
/// Annotation je Dokument nur einmal läse, fände es nie: stilles Leck. Das
/// Buch liest deshalb unter jeder neuen Ressourcenumgebung erneut.
#[test]
fn unter_anderen_ressourcen_wird_neu_gelesen() {
    let mut d = page(&["Hallo"]);
    // "abcbde" zeigt unter der Umkodierung "GEHEIM".
    let ap = appearance(&mut d, "abcbde", false);
    let annot = free_text(&mut d, ap, 300);
    let array = d.add(Object::Array(vec![Object::Reference(annot)]));
    d.page_dict_set("Annots", Object::Reference(array));
    let pages = weitere_seiten(&mut d, 1);
    let umkodiert = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => dictionary! {
            "Type" => "Encoding",
            "BaseEncoding" => "WinAnsiEncoding",
            "Differences" => vec![
                97.into(),
                Object::Name(b"G".to_vec()),
                Object::Name(b"E".to_vec()),
                Object::Name(b"H".to_vec()),
                Object::Name(b"I".to_vec()),
                Object::Name(b"M".to_vec()),
            ],
        },
    }));
    d.doc.get_dictionary_mut(pages[1]).expect("Seite 2").set(
        "Resources",
        dictionary! { "Font" => dictionary! { "F1" => Object::Reference(umkodiert) } },
    );
    let doc = load_from_bytes(&d.finish()).expect("ladbar");

    let runs = PdfExtractor::new().extract(&doc).expect("Extraktion");
    assert_eq!(
        seiten_mit(&runs, "abcbde"),
        vec![0],
        "Seite 1 zeigt die Bytes"
    );
    assert_eq!(
        seiten_mit(&runs, "GEHEIM"),
        vec![1],
        "Seite 2 zeigt dieselbe Annotation unter ihrer eigenen Schrift"
    );
}

/// Eine Seite, deren Scan abgelehnt wird, hat ihre Annotationen nicht
/// gelesen. Im nachsichtigen Weg des Orakels fehlt sie als Lücke — die
/// nächste Seite mit demselben Array muss es dann selbst lesen, sonst fehlte
/// die Notiz in der Sicht ganz, ohne eigene Zeile.
#[test]
fn eine_abgelehnte_seite_liest_nicht_fuer_die_naechste() {
    let mut d = page(&["Hallo"]);
    // Die dokumentierte Fächerung (8⁷ Durchläufe) auf Seite 1.
    let mut unten = d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), 1.into(), 1.into()] },
        b"0 0 1 1 re f\n".to_vec(),
    )));
    for _ in 0..7 {
        let liste = d.add(Object::Dictionary(
            dictionary! { "N" => Object::Reference(unten) },
        ));
        let eigene = d.add(Object::Dictionary(
            dictionary! { "XObject" => Object::Reference(liste) },
        ));
        unten = d.add(Object::Stream(Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 1.into(), 1.into()],
            "Resources" => Object::Reference(eigene) },
            "q /N Do Q\n".repeat(8).into_bytes(),
        )));
    }
    let resources_id = d.resources_id;
    d.doc
        .get_dictionary_mut(resources_id)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "T" => Object::Reference(unten) });
    let (array, _) = notizen(&mut d, 1, true);
    d.page_dict_set("Annots", Object::Reference(array));
    let pages = weitere_seiten(&mut d, 1);
    // Seite 1 zeichnet die Fächerung, Seite 2 nicht; Ressourcen und Array
    // teilen sich beide.
    let faecher = d.add(Object::Stream(Stream::new(
        dictionary! {},
        b"q /T Do Q\n".to_vec(),
    )));
    d.doc
        .get_dictionary_mut(pages[0])
        .expect("Seite 1")
        .set("Contents", Object::Reference(faecher));
    let doc = load_from_bytes(&d.finish()).expect("ladbar");

    let (runs, _, gaps) = PdfExtractor::new().extract_lenient_with_gaps(&doc);
    assert_eq!(gaps.len(), 1, "Seite 1 ist eine Lücke: {gaps:?}");
    assert_eq!(gaps[0].0, pages[0]);
    assert_eq!(
        seiten_mit(&runs, "Notiz 0"),
        vec![1],
        "Seite 2 liest die Notiz selbst"
    );
}

/// Eine Schwärzung auf Seite 3 über einer geteilten Annotation nimmt sie aus
/// dem Array — an seinem Objekt. Alle Seiten zeigen danach weiter dasselbe
/// Array, ohne die Annotation und ohne toten Verweis; vorher bekam Seite 3
/// eine eigene Kopie und die übrigen behielten den Verweis auf ein gelöschtes
/// Objekt.
#[test]
fn eine_schwaerzung_bereinigt_das_geteilte_array_an_seinem_objekt() {
    let mut d = page(&["Hallo"]);
    let (array, ids) = notizen(&mut d, 8, true);
    d.page_dict_set("Annots", Object::Reference(array));
    let pages = weitere_seiten(&mut d, 4);
    let mut doc = load_from_bytes(&d.finish()).expect("ladbar");

    // Notiz 5 liegt bei y = 225 … 245.
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[zone(2, Rect::new(100.0, 230.0, 120.0, 240.0))])
        .expect("Schwärzung");
    assert_eq!(report.removed_annotations, 1);
    for page_id in &pages {
        let annots = doc.get_dictionary(*page_id).expect("Seite").get(b"Annots");
        assert!(
            matches!(annots, Ok(Object::Reference(id)) if *id == array),
            "jede Seite zeigt weiter das eine Array: {annots:?}"
        );
    }
    let items = doc
        .get_object(array)
        .and_then(|o| o.as_array())
        .expect("Array");
    assert_eq!(items.len(), 7);
    assert!(!items
        .iter()
        .any(|o| matches!(o, Object::Reference(id) if *id == ids[5])));
    assert!(
        doc.get_object(ids[5]).is_err(),
        "die Annotation selbst ist fort"
    );

    let bytes = save_to_bytes(&doc).expect("speicherbar");
    let out = load_from_bytes(&bytes).expect("ladbar");
    let runs = PdfExtractor::new().extract(&out).expect("Extraktion");
    assert!(seiten_mit(&runs, "Notiz 5").is_empty());
    assert_eq!(seiten_mit(&runs, "Notiz 4"), vec![0]);
}

/// Ein Dateianhang im geteilten Array: der Metadatenlauf nimmt ihn aus dem
/// Array an seinem Objekt, einmal. Vorher bekam **jede** Seite eine eigene
/// Kopie des Arrays — Seiten × Einträge Verweise in der Ausgabe.
#[test]
fn ein_anhang_im_geteilten_array_faellt_einmal_am_objekt() {
    let mut d = page(&["Hallo"]);
    let (array, ids) = notizen(&mut d, 3, true);
    let datei = d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "EmbeddedFile" },
        format!("Anhang {SECRET}").into_bytes(),
    )));
    let spec = d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => Object::String(b"anhang.txt".to_vec(), StringFormat::Literal),
        "EF" => dictionary! { "F" => Object::Reference(datei) },
    }));
    let anhang = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FileAttachment",
        "Rect" => vec![300.into(), 700.into(), 320.into(), 720.into()],
        "FS" => Object::Reference(spec),
    }));
    if let Ok(Object::Array(items)) = d.doc.get_object_mut(array) {
        items.push(Object::Reference(anhang));
    }
    d.page_dict_set("Annots", Object::Reference(array));
    let pages = weitere_seiten(&mut d, 5);
    let mut doc = load_from_bytes(&d.finish()).expect("ladbar");

    strip_metadata(&mut doc);
    for page_id in &pages {
        let annots = doc.get_dictionary(*page_id).expect("Seite").get(b"Annots");
        assert!(
            matches!(annots, Ok(Object::Reference(id)) if *id == array),
            "jede Seite zeigt weiter das eine Array: {annots:?}"
        );
    }
    let items = doc
        .get_object(array)
        .and_then(|o| o.as_array())
        .expect("Array");
    let rest: Vec<ObjectId> = items
        .iter()
        .filter_map(|o| match o {
            Object::Reference(id) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(rest, ids, "die Notizen bleiben, der Anhang fällt");
    let bytes = save_to_bytes(&doc).expect("speicherbar");
    assert!(
        leaks(&bytes, SECRET).is_empty(),
        "die Datei des Anhangs ist fort"
    );
}
