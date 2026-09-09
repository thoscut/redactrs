//! Die nachsichtige Extraktion bleibt **linear** in der Seitenzahl — auch mit
//! einer kaputten Seite.
//!
//! ## Der Befund
//!
//! Sicht 7 des ehrlichen Orakels (`audit_bytes::scan_decoded_text`) las bis
//! zu dieser Korrektur erst `extract()` und fiel nach dessen erstem Fehler
//! auf eine Schleife über `extract_page(doc, i)` zurück. Die baute je Aufruf
//! den Seitenbaum neu (`get_pages()`: ein Durchlauf des `/Kids`-Baums und
//! eine frische `BTreeMap` über **alle** Seiten) — je Seite einmal, also
//! quadratisch. Keiner der Tests bemerkte es: die Mutation „unbedingte
//! Seitenschleife“ ließ alle grün.
//!
//! ## Warum Speicher, nicht Zeit
//!
//! Eine Uhr ist auf einem geteilten Rechner keine Messgröße; gezählt werden
//! deshalb die **kumulativ** angeforderten Bytes eines zählenden Allokators.
//! Sie sind deterministisch und machen den quadratischen Anteil sichtbar: je
//! `get_pages()` eine `BTreeMap` mit n Einträgen, n-mal.
//!
//! ## Warum `extract_lenient` und nicht `leaks`
//!
//! Gemessen wird der Aufruf, den Sicht 7 macht — nicht das ganze Orakel.
//! Dessen Bytesichten fordern je Seite rund 215 KB an, die Extraktion
//! selbst rund 55 KB; der quadratische Anteil eines `get_pages()` je Seite
//! liegt bei rund 46 Byte je Seite² (gemessen: 74 KB je Seite bei 1 600
//! Seiten). Unter den Bytesichten verschwindet er: mit `leaks()` als
//! Messgröße ergab die Mutation „`get_pages()` in der Schleife“ bei 50 gegen
//! 1 600 Seiten das Verhältnis 42 statt 32 — ein Test mit Schwelle 64 bliebe
//! grün. An `extract_lenient` allein ist derselbe Unterschied 69 gegen 32,
//! und bei 50 gegen 3 200 Seiten (Faktor 64) deutlich über 200 gegen 64.
//!
//! ## Gemessen (Debug, kumulativ angeforderte Bytes von `extract_lenient`)
//!
//! | Seiten | linear (dieser Stand) | `get_pages()` je Seite (Mutation) |
//! |-------:|----------------------:|----------------------------------:|
//! |     50 |                2,7 MB |                            2,8 MB |
//! |  3 200 |              177,3 MB |                          649,7 MB |
//! | Verh.  |         65,4 (1,02×)  |                    229,3 (3,58×)  |
//!
//! ## Warum eine eigene Datei mit einem einzigen Test
//!
//! Der Allokator zählt den ganzen Prozess; ein zweiter Test daneben zählte
//! mit. Dieselbe Begründung wie in `za_objektspeicher_gerechnet.rs`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use lopdf::{dictionary, Document, Object, Stream};
use redact_pdf::PdfExtractor;

// ---------------------------------------------------------------------------
// Der zählende Allokator — kumulativ, nicht „gerade belegt“
// ---------------------------------------------------------------------------

static TOTAL: AtomicUsize = AtomicUsize::new(0);

struct Zaehlend;

// SAFETY: Jede Anforderung wird unverändert an den Systemallokator
// weitergereicht; gezählt wird nur nebenher.
unsafe impl GlobalAlloc for Zaehlend {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            TOTAL.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let neu = unsafe { System.realloc(ptr, layout, new_size) };
        if !neu.is_null() && new_size > layout.size() {
            TOTAL.fetch_add(new_size - layout.size(), Ordering::Relaxed);
        }
        neu
    }
}

#[global_allocator]
static ALLOC: Zaehlend = Zaehlend;

/// Kumulativ angeforderte Bytes während `f`.
fn allocated_by(f: impl FnOnce()) -> usize {
    let before = TOTAL.load(Ordering::Relaxed);
    f();
    TOTAL.load(Ordering::Relaxed) - before
}

// ---------------------------------------------------------------------------
// Das Dokument: Seite 1 kaputt, die übrigen gewöhnlich
// ---------------------------------------------------------------------------

const SMALL: usize = 50;
const LARGE: usize = 3_200;

/// `pages` Seiten mit einer gemeinsamen Schrift. Seite 1 trägt ein nie
/// geschlossenes Zeichenkettenliteral — der Interpreter lehnt sie ab, und
/// `extract()` damit das ganze Dokument. Jede weitere Seite nennt ihre
/// Nummer, damit das Lesen der letzten Seite nachprüfbar ist.
fn document(pages: usize) -> Document {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let pages_id = doc.new_object_id();
    let mut kids: Vec<Object> = Vec::with_capacity(pages);
    for number in 1..=pages {
        let content = if number == 1 {
            b"BT (offen".to_vec()
        } else {
            format!("BT /F1 10 Tf 72 700 Td (Seite {number} Kontoinhaber Max Mustermann) Tj ET")
                .into_bytes()
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        kids.push(page_id.into());
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => pages as i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    // Einmal durch die Datei: die Sicht liest ein geparstes Dokument, nicht
    // das eben gebaute — Querverweistabelle, Objektspeicher, alles wie im
    // Ernstfall.
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("speicherbar");
    Document::load_mem(&bytes).expect("parsebar")
}

/// Der Lauf, dessen Speicher gezählt wird — und die Nachweise, dass er das
/// tut, was gemessen werden soll: `extract()` lehnt ab, die nachsichtige
/// Extraktion liest trotzdem bis zur letzten Seite und nennt genau die
/// kaputte.
fn measured_run(pages: usize) -> usize {
    let doc = document(pages);
    let extractor = PdfExtractor::new();
    assert!(
        extractor.extract(&doc).is_err(),
        "Seite 1 sollte den Interpreter zum Abbruch bringen — sonst misst der Test den Regelweg"
    );
    let mut result = (Vec::new(), Vec::new());
    let allocated = allocated_by(|| result = extractor.extract_lenient(&doc));
    let (runs, warnings) = result;
    let last = format!("Seite {pages} ");
    assert!(
        runs.iter()
            .any(|r| r.page + 1 == pages && r.text.contains(&last)),
        "{pages} Seiten: die letzte Seite muss trotz Seite 1 gelesen werden"
    );
    assert!(
        runs.iter().all(|r| r.page != 0),
        "von der kaputten Seite darf nichts kommen"
    );
    let skipped: Vec<&String> = warnings
        .iter()
        .filter(|w| w.contains("fehlt in dieser Sicht"))
        .collect();
    assert!(
        skipped.len() == 1 && skipped[0].starts_with("Seite 1 "),
        "genau die kaputte Seite wird übersprungen und genannt: {warnings:?}"
    );
    allocated
}

/// Faktor 64 in den Seiten, Faktor < 128 im Speicher — je Seite also
/// weniger als das Doppelte. Ein linearer Lauf liegt bei etwa 64; die
/// Schleife mit `get_pages()` je Seite deutlich über 200 (Zahlen im
/// Modulkommentar).
#[test]
fn die_nachsichtige_extraktion_bleibt_linear_in_der_seitenzahl() {
    // Ein Aufwärmlauf, damit einmalige Anforderungen (Muster, Tabellen)
    // nicht in der kleinen Messung landen.
    let _ = measured_run(4);

    let small = measured_run(SMALL);
    let large = measured_run(LARGE);
    let ratio = large as f64 / small as f64;
    let per_page = ratio / (LARGE as f64 / SMALL as f64);
    eprintln!(
        "kumulativ angefordert: {SMALL} Seiten {small} Byte, {LARGE} Seiten {large} Byte, \
         Verhältnis {ratio:.1}, je Seite das {per_page:.2}-fache"
    );
    assert!(
        per_page < 2.0,
        "quadratisch: {LARGE} Seiten fordern je Seite das {per_page:.2}-fache von {SMALL} \
         Seiten an ({large} gegen {small} Byte, Verhältnis {ratio:.1})"
    );
}
