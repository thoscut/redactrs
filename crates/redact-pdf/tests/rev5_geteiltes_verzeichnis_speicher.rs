//! Die eine Messung, die wirklich der Speicher ist — und deshalb **allein** in
//! ihrer eigenen Testdatei steht.
//!
//! `VmHWM` ist ein Höchststand des **ganzen Prozesses**. Ein zweiter Test im
//! selben Programm, der nebenher ein paar Megabyte belegt, hebt den Wert und
//! macht die Messung wertlos — ausprobiert: derselbe Test wurde grün, sobald er
//! allein lief, und rot mit drei Nachbarn. Eine eigene Datei ist ein eigenes
//! Programm, und damit ist der Zähler wieder sauber.
//!
//! ## Der Befund
//!
//! Jeder ausgepackte Strom klonte sein aufgelöstes `/Resources`-Verzeichnis
//! **vollständig**. *n* Form-XObjects, die per Referenz dasselbe Verzeichnis
//! erben, hielten also *n* Kopien derselben Zeichenkette — und die Decke
//! `MAX_CACHED_OPERATIONS` sah davon nichts, weil eine Zeichenkette in ihrem
//! Zähler *eins* wog.
//!
//! Gemessen (Release, `VmHWM`, ein Verzeichnis mit einer 16-kB-Zeichenkette):
//!
//! | Datei | Formulare | vorher | nachher |
//! |---|---:|---:|---:|
//! | 0,76 MB | 5 000 | 107 MB | 26 MB |
//! | 7,60 MB | 50 000 | 1 037 MB | 226 MB |
//!
//! Der Zähler las dabei vorher 5 000 bzw. 50 000 ab — beides weit unter der
//! Decke von 100 000.
//!
//! Gelesen wird aus `/proc`, deshalb nur unter Linux. Die übrigen
//! Zusicherungen zu diesem Befund hängen an Zählern statt an der Maschine und
//! stehen in `rev5_geteilte_ressourcen.rs`; die gelten überall.

#![cfg(target_os = "linux")]

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_pdf::scan_page;

fn vm_hwm_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1).map(str::to_string))
        })
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// `n` Formulare mit leerem Rumpf, die alle dasselbe `/Resources`-Objekt
/// erben — dieses trägt eine Zeichenkette von `junk` Byte.
fn seite_mit_geteiltem_verzeichnis(n: usize, junk: usize) -> (Document, ObjectId) {
    let mut doc = Document::with_version("1.7");
    let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
    let geteilt = doc.add_object(dictionary! {
        "Junk" => Object::string_literal(vec![b'A'; junk]),
    });

    let mut xobjects = Dictionary::new();
    let mut content = String::new();
    for i in 0..n {
        let form = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                "Resources" => geteilt,
            },
            Vec::new(),
        ));
        xobjects.set(format!("X{i}"), form);
        content.push_str(&format!("q /X{i} Do Q\n"));
    }

    let resources_id = doc.add_object(dictionary! { "XObject" => xobjects });
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.objects.insert(
        content_id,
        Object::Stream(Stream::new(dictionary! {}, content.into_bytes())),
    );
    (doc, page_id)
}

/// **Der Befund:** 4 000 Formulare, die per Referenz *ein* Verzeichnis mit
/// einer 16-kB-Zeichenkette erben, dürfen nicht 4 000 Kopien davon anlegen.
///
/// Vorher waren das 4 000 × 16 kB = 64 MB allein an Zeichenketten, aus einer
/// Datei, die die Zeichenkette **einmal** enthält.
///
/// Gibt man `Budget::resource_dict` das geteilte Verzeichnis nicht mehr als
/// `Rc` heraus, sondern klont es wieder je Strom, geht dieser Test rot —
/// gemessen 78 MB Zuwachs statt 1,5 MB.
#[test]
fn ein_geteiltes_verzeichnis_wird_nicht_je_strom_kopiert() {
    let (doc, page_id) = seite_mit_geteiltem_verzeichnis(4_000, 16 * 1024);
    let vorher = vm_hwm_kb();
    let scan = scan_page(&doc, page_id).expect("lesbar");
    let zuwachs = vm_hwm_kb().saturating_sub(vorher);

    assert_eq!(scan.effort.decoded_streams, 4_000, "jedes Formular einmal");
    assert!(
        zuwachs < 20 * 1024,
        "der Scan hat {zuwachs} kB belegt. 4 000 Formulare teilen sich **ein** \
         Verzeichnis mit einer 16-kB-Zeichenkette; wird es je Strom kopiert, \
         sind das allein 64 MB. Gemessen: vorher 78 MB Zuwachs, nachher 1,5 MB."
    );
}
