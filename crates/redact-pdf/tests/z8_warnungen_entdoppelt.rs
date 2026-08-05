//! Dieselbe Warnung steht genau **einmal** in der Liste — und in der
//! Reihenfolge ihres ersten Auftretens.
//!
//! ## Warum diese Datei entstanden ist
//!
//! Die Entdopplung gab es längst: [`ScanResult::warn`] und
//! `OpsCollector::note` prüften vor jedem Anhängen, ob der Text schon
//! dasteht. Gehalten hat sie aber **kein Test**. Ausgebaut (in
//! `redact-pdf/src/content.rs`, `redact-pdf/src/ops.rs` und
//! `redact-render/src/raster.rs` zusammen) blieb die ganze Suite grün: 190
//! Unit-Tests, alle Integrationstests, `redact-render` samt Bildvergleich.
//!
//! Das ist derselbe Befund wie beim Scheintest um die Clip-Entdopplung: eine
//! Zusage, die niemand nachhält, ist keine. Und sie wiegt hier schwer — ein
//! mehrfach platziertes Form-XObject, ein wiederholt gezeichnetes Bild oder
//! eine Seite mit zehntausend Glyphen im Konturmodus lösen dieselbe Warnung
//! beliebig oft aus. Ohne Entdopplung stünde sie zehntausendmal vor dem
//! Nutzer.
//!
//! ## Was sich unter der Haube geändert hat (und was nicht)
//!
//! Nachgeschlagen wird jetzt in einer Menge statt in der Liste. Am Verhalten
//! ändert das nichts — genau das prüft diese Datei —, an der Laufzeit sehr
//! wohl: die Suche war quadratisch. Gemessen an derselben Datei, beide
//! Fassungen abwechselnd (Debug):
//!
//! | Warnungen | mit Liste | mit Menge |
//! |----------:|----------:|----------:|
//! |     2 000 |   0,065 s |   0,069 s |
//! |     8 000 |   0,576 s |   0,199 s |
//! |    32 000 |   7,110 s |   0,815 s |

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_pdf::{page_ops, scan_page};

// ---------------------------------------------------------------------------
// Werkzeug
// ---------------------------------------------------------------------------

/// Baut eine Ein-Seiten-Datei aus Inhalt und Ressourcen.
fn seite(content: &str, resources: Dictionary, weitere: impl FnOnce(&mut Document)) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let res_id = doc.add_object(resources);
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Resources" => res_id,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1_i64,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    weitere(&mut doc);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    out
}

fn lade(bytes: &[u8]) -> Document {
    redact_pdf::document::load_from_bytes(bytes).expect("ladbar")
}

fn erste_seite(doc: &Document) -> ObjectId {
    doc.get_pages()
        .values()
        .copied()
        .next()
        .expect("eine Seite")
}

// ---------------------------------------------------------------------------
// Der Scanner
// ---------------------------------------------------------------------------

/// Zweitausendmal derselbe Fehler ergibt **eine** Warnung.
///
/// Zweitausend `Do` auf denselben ins Leere zeigenden Ressourcennamen. Ohne
/// die Entdopplung stünden zweitausend gleichlautende Sätze im Ergebnis.
#[test]
fn dieselbe_warnung_steht_genau_einmal_im_scan() {
    let content = "q /X0 Do Q\n".repeat(2_000);
    let resources = dictionary! {
        "XObject" => dictionary! { "X0" => Object::Reference((900_001, 0)) },
    };
    let doc = lade(&seite(&content, resources, |_| {}));
    let scan = scan_page(&doc, erste_seite(&doc)).expect("Scan");

    assert_eq!(
        scan.warnings.len(),
        1,
        "zweitausendmal derselbe Fehler, {} Warnungen: {:?}",
        scan.warnings.len(),
        &scan.warnings[..scan.warnings.len().min(3)]
    );
    assert!(
        scan.warnings[0].contains("X0"),
        "die Warnung nennt den Namen nicht: {:?}",
        scan.warnings
    );
}

/// Verschiedene Fehler bleiben verschieden — und in ihrer Reihenfolge.
///
/// Die Entdopplung darf nicht zum Sortieren werden: gemeldet wird in der
/// Reihenfolge des **ersten** Auftretens, und das ist die Reihenfolge im
/// Strom.
#[test]
fn verschiedene_warnungen_bleiben_in_ihrer_reihenfolge() {
    let mut xobjects = Dictionary::new();
    for j in 0..3 {
        xobjects.set(format!("X{j}"), Object::Reference((900_001 + j as u32, 0)));
    }
    // Absichtlich nicht in Namensreihenfolge, und mit Wiederholungen.
    let content = "q /X2 Do Q\nq /X0 Do Q\nq /X2 Do Q\nq /X1 Do Q\nq /X0 Do Q\n";
    let doc = lade(&seite(
        content,
        dictionary! { "XObject" => xobjects },
        |_| {},
    ));
    let scan = scan_page(&doc, erste_seite(&doc)).expect("Scan");

    let namen: Vec<&str> = scan
        .warnings
        .iter()
        .map(|w| {
            if w.contains("X0") {
                "X0"
            } else if w.contains("X1") {
                "X1"
            } else {
                "X2"
            }
        })
        .collect();
    assert_eq!(
        namen,
        vec!["X2", "X0", "X1"],
        "Warnungen in falscher Reihenfolge oder Zahl: {:?}",
        scan.warnings
    );
}

// ---------------------------------------------------------------------------
// Die Zeichenoperationen
// ---------------------------------------------------------------------------

/// Dasselbe für die Hinweise aus `page_ops`.
///
/// Eine Stencil-Maske (`/ImageMask true`) wird **nicht** zwischengespeichert —
/// sie hängt an der Füllfarbe. Ein unbrauchbarer Filter darauf erzeugt also
/// bei jedem `Do` denselben Hinweis.
#[test]
fn derselbe_hinweis_steht_genau_einmal_in_den_operationen() {
    let content = "q 100 0 0 100 10 10 cm /I0 Do Q\n".repeat(500);
    let bytes = seite(&content, Dictionary::new(), |_| {});
    // Die Ressourcen von Hand nachtragen, weil das Bild eine eigene Id braucht.
    let mut doc = Document::load_mem(&bytes).expect("ladbar");
    let bild = doc.add_object(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "ImageMask" => true,
            "Width" => 8_i64,
            "Height" => 8_i64,
            "BitsPerComponent" => 1_i64,
            "Filter" => "JPXDecode",
        },
        vec![0u8; 8],
    )));
    let page_id = erste_seite(&doc);
    let res_id = doc
        .get_dictionary(page_id)
        .and_then(|d| d.get(b"Resources"))
        .and_then(|o| o.as_reference())
        .expect("Ressourcen");
    doc.get_dictionary_mut(res_id)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "I0" => bild });

    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    let doc = lade(&out);
    let ops = page_ops(&doc, 0).expect("Operationen");

    assert_eq!(
        ops.notes.len(),
        1,
        "fünfhundertmal dasselbe Bild, {} Hinweise: {:?}",
        ops.notes.len(),
        &ops.notes[..ops.notes.len().min(3)]
    );
}
