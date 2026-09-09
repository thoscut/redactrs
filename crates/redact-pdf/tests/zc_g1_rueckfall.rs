//! Gegenprüfung G1 (Fix-Runde 3), Gebiet B: `PdfExtractor::extract_lenient`.
//!
//! Seitennummer in der Warnung bei verschachteltem Baum mit einem Kind, das
//! ins Leere zeigt; Seite ohne `/Resources`; `/Contents` ins Leere;
//! zirkulärer Seitenbaum; null Seiten.

mod common;

use common::{page, text_ops, Doc};
use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_pdf::{load_from_bytes, PdfExtractor};

fn add_page(d: &mut Doc, content: &[u8]) -> ObjectId {
    let content_id = d.add(Object::Stream(Stream::new(
        dictionary! {},
        content.to_vec(),
    )));
    let resources = d.resources_id;
    let pages_id = d.pages_id;
    let page_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    }));
    let pages = d.doc.get_dictionary_mut(pages_id).expect("Pages");
    let mut kids = pages
        .get(b"Kids")
        .and_then(|k| k.as_array())
        .cloned()
        .expect("Kids");
    kids.push(page_id.into());
    let count = kids.len() as i64;
    pages.set("Kids", kids);
    pages.set("Count", count);
    page_id
}

/// Ein nie geschlossenes Zeichenkettenliteral — der Interpreter lehnt ab.
const BROKEN: &[u8] = b"BT (offen";

/// Baum `Pages → [Pages → [S1, 999 0 R, S2], S3]`, S2 kaputt: die Warnung
/// nennt die **dokumentweite** 1-basierte Nummer (2), das Kind ins Leere
/// zählt nicht, S3 wird als Seite 3 gelesen.
#[test]
fn seitennummer_der_warnung_ist_die_dokumentweite_bei_verschachteltem_baum() {
    let mut d = page(&["Seite eins"]);
    let inner = d.add(Object::Dictionary(dictionary! {
        "Type" => "Pages", "Kids" => Vec::<Object>::new(), "Count" => 0,
    }));
    let p1 = d.page_id;
    let p2 = add_page(&mut d, BROKEN);
    let p3 = add_page(&mut d, &text_ops(&["Seite drei"]));
    for pid in [p1, p2] {
        d.doc
            .get_dictionary_mut(pid)
            .expect("Seite")
            .set("Parent", inner);
    }
    let inner_dict = d.doc.get_dictionary_mut(inner).expect("Pages");
    inner_dict.set(
        "Kids",
        vec![p1.into(), Object::Reference((999, 0)), p2.into()],
    );
    inner_dict.set("Count", 2);
    let root = d.pages_id;
    let root_dict = d.doc.get_dictionary_mut(root).expect("Pages");
    root_dict.set("Kids", vec![inner.into(), p3.into()]);
    root_dict.set("Count", 3);

    let doc = load_from_bytes(&d.finish()).expect("ladbar");
    assert!(
        PdfExtractor::new().extract(&doc).is_err(),
        "streng: Abbruch"
    );
    let (runs, warnings) = PdfExtractor::new().extract_lenient(&doc);
    let skipped: Vec<&String> = warnings
        .iter()
        .filter(|w| w.contains("fehlt in dieser Sicht"))
        .collect();
    assert_eq!(skipped.len(), 1, "{warnings:?}");
    assert!(skipped[0].starts_with("Seite 2 "), "{}", skipped[0]);
    assert!(runs.iter().any(|r| r.page == 0 && r.text.contains("eins")));
    assert!(runs.iter().any(|r| r.page == 2 && r.text.contains("drei")));
    assert!(runs.iter().all(|r| r.page != 1));
}

/// Eine Seite ohne `/Resources` und eine mit `/Contents` ins Leere sind
/// keine abgelehnten Seiten: beide Wege (streng, nachsichtig) lesen die
/// übrigen Seiten, keine Warnung.
#[test]
fn seite_ohne_ressourcen_und_contents_ins_leere_sind_kein_abbruch() {
    let mut d = page(&["Seite eins"]);
    let p2 = add_page(&mut d, &text_ops(&["Seite zwei ohne Ressourcen"]));
    d.doc
        .get_dictionary_mut(p2)
        .expect("Seite")
        .remove(b"Resources");
    let p3 = add_page(&mut d, b"");
    d.doc
        .get_dictionary_mut(p3)
        .expect("Seite")
        .set("Contents", Object::Reference((999, 0)));
    add_page(&mut d, &text_ops(&["Seite vier"]));
    let doc = load_from_bytes(&d.finish()).expect("ladbar");
    let (strict_runs, strict_warnings) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("streng liest");
    let (runs, warnings) = PdfExtractor::new().extract_lenient(&doc);
    assert_eq!(strict_runs.len(), runs.len());
    assert!(
        strict_warnings.is_empty() && warnings.is_empty(),
        "{warnings:?}"
    );
    assert!(runs.iter().any(|r| r.page == 3 && r.text.contains("vier")));
}

/// Zirkulärer Seitenbaum (`/Kids [S1, Pages]`): kein Endlosschleifen, kein
/// Absturz. Beobachtung: `lopdf` zählt dieselbe Seite mehrfach
/// (`iter_limit = objects.len()`), eine Warnung gibt es nicht.
#[test]
fn zirkulaerer_seitenbaum_endet() {
    let mut d = page(&["Seite eins"]);
    let root = d.pages_id;
    let p1 = d.page_id;
    d.doc
        .get_dictionary_mut(root)
        .expect("Pages")
        .set("Kids", vec![p1.into(), root.into()]);
    let doc = load_from_bytes(&d.finish()).expect("ladbar");
    let pages = doc.get_pages().len();
    let (runs, warnings) = PdfExtractor::new().extract_lenient(&doc);
    eprintln!("Zyklus: {pages} Seiten, {} Läufe, {warnings:?}", runs.len());
    assert!(pages >= 1 && runs.len() == pages);
    assert!(warnings.is_empty());
}

/// Null Seiten: `load_from_bytes` lehnt ab; direkt aufgerufen liefert die
/// nachsichtige Extraktion nichts und warnt nicht.
#[test]
fn null_seiten_liefern_nichts() {
    let mut d = page(&[]);
    let root = d.pages_id;
    d.doc
        .get_dictionary_mut(root)
        .expect("Pages")
        .set("Kids", Vec::<Object>::new());
    let bytes = d.finish();
    assert!(load_from_bytes(&bytes).is_err(), "keine Seiten → Ablehnung");
    let doc = Document::load_mem(&bytes).expect("parsebar");
    let (runs, warnings) = PdfExtractor::new().extract_lenient(&doc);
    assert!(runs.is_empty() && warnings.is_empty(), "{warnings:?}");
}
