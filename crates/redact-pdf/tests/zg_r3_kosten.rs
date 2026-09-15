//! Gegenprüfung Fix-Runde 6 (R3): **Kosten und Endlichkeit** der Zählung an
//! der aufgeräumten Datei.
//!
//! `Tally` merkt sich je gefallenem Verweis eine `ObjectId`; `settled`
//! schlägt am Ende jede davon einmal nach (`contains_key`, O(log n)). Der
//! Lauf bleibt damit O(n log n) — hier gemessen an drei Größen, damit die
//! Frage „quadratisch?“ eine Zahl bekommt und nicht eine Lesart.
//!
//! Material im Speicher, nicht auf der Platte: 200 000 Annotationen mit
//! `/Contents` als Verweis; Lesezeichenketten mit geteilten `/Title`-Objekten
//! in 50 000, 100 000 und 200 000 Einträgen. Die Zeitschranken sind
//! großzügig (ein geteilter Rechner); die Linearität misst sich am
//! Verhältnis, nicht an der absoluten Zahl.

use std::time::Instant;

use lopdf::{dictionary, Document, Object, ObjectId};
use redact_pdf::strip_metadata;

fn grundgeruest() -> (Document, ObjectId, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, catalog_id, page_id)
}

/// `n` Annotationen an einer Seite, jede mit `/Contents k 0 R` auf ein
/// eigenes Zeichenkettenobjekt.
fn annotationen_mit_verweisen(n: usize) -> Document {
    let (mut doc, _, page_id) = grundgeruest();
    let mut annots = Vec::with_capacity(n);
    for i in 0..n {
        let text = doc.add_object(Object::string_literal(format!("Notiz {i}")));
        let a = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
            "Contents" => Object::Reference(text),
        });
        annots.push(Object::Reference(a));
    }
    doc.get_dictionary_mut(page_id)
        .unwrap()
        .set("Annots", Object::Array(annots));
    doc
}

/// Eine flache Lesezeichenkette mit `n` Einträgen; je zwei Einträge teilen
/// ein `/Title`-Objekt.
fn lesezeichen_mit_geteilten_titeln(n: usize) -> Document {
    let (mut doc, catalog_id, _) = grundgeruest();
    let root = doc.new_object_id();
    let ids: Vec<ObjectId> = (0..n).map(|_| doc.new_object_id()).collect();
    let mut title = None;
    for (i, id) in ids.iter().enumerate() {
        if i % 2 == 0 {
            title = Some(doc.add_object(Object::string_literal(format!("Kapitel {i}"))));
        }
        let mut dict = dictionary! {
            "Title" => Object::Reference(title.unwrap()),
            "Parent" => Object::Reference(root),
        };
        if i + 1 < n {
            dict.set("Next", Object::Reference(ids[i + 1]));
        }
        if i > 0 {
            dict.set("Prev", Object::Reference(ids[i - 1]));
        }
        doc.objects.insert(*id, Object::Dictionary(dict));
    }
    doc.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => Object::Reference(ids[0]),
            "Last" => Object::Reference(ids[n - 1]),
            "Count" => n as i64,
        }),
    );
    doc.get_dictionary_mut(catalog_id)
        .unwrap()
        .set("Outlines", Object::Reference(root));
    doc
}

#[test]
fn zweihunderttausend_annotationen_mit_verweis_contents_in_sekunden() {
    let mut doc = annotationen_mit_verweisen(200_000);
    let start = Instant::now();
    let report = strip_metadata(&mut doc);
    let dauer = start.elapsed();
    println!("200 000 Annotationen mit /Contents k 0 R: {dauer:?}");
    assert_eq!(report.annotation_texts_cleared, 200_000);
    assert!(
        dauer.as_secs() < 30,
        "200 000 Annotationen brauchten {dauer:?} — das ist nicht mehr linear"
    );
}

/// Drei Größen, ein Verhältnis: linear heißt t(4n)/t(n) ≈ 4, quadratisch
/// ≈ 16. Geprüft wird < 8, mit Luft für einen geteilten Rechner.
#[test]
fn lesezeichen_mit_geteilten_titeln_bleiben_linear() {
    let mut zeiten = Vec::new();
    for n in [50_000usize, 100_000, 200_000] {
        let mut doc = lesezeichen_mit_geteilten_titeln(n);
        let start = Instant::now();
        let report = strip_metadata(&mut doc);
        let dauer = start.elapsed();
        println!("{n} Lesezeichen mit geteilten /Title: {dauer:?}");
        assert_eq!(
            report.outlines_removed, n,
            "jeder Eintrag gilt als entfernt"
        );
        zeiten.push(dauer.as_secs_f64());
    }
    let verhaeltnis = zeiten[2] / zeiten[0].max(0.001);
    println!("t(200k)/t(50k) = {verhaeltnis:.1}");
    assert!(
        verhaeltnis < 8.0,
        "t(200k)/t(50k) = {verhaeltnis:.1}: das wächst schneller als linear"
    );
}

/// Werkzeug: eine Million Lesezeichen mit geteilten Titeln, nur die Zeit.
#[test]
#[ignore = "Werkzeug, keine Prüfung"]
fn eine_million_lesezeichen_messen() {
    let mut doc = lesezeichen_mit_geteilten_titeln(1_000_000);
    let start = Instant::now();
    let report = strip_metadata(&mut doc);
    println!(
        "1 000 000 Lesezeichen mit geteilten /Title: {:?}, gemeldet {}",
        start.elapsed(),
        report.outlines_removed
    );
}
