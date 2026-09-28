//! Der Speicher der Vorschau darf nicht an der **Zahl der Clip-Pfade** hängen.
//!
//! ## Warum eine eigene Datei mit einem einzigen Test
//!
//! Gemessen wird mit einem zählenden Allokator, und der zählt den **ganzen
//! Prozess**. Liefe daneben ein zweiter Test, mäße dieser hier dessen Speicher
//! mit. Ein Test je Testprogramm ist die einzige Anordnung, in der die Zahl
//! etwas bedeutet — dieselbe Begründung wie in
//! `redact-pdf/tests/rev4_speicher_je_bereich.rs`, aus dem der Allokator
//! stammt.
//!
//! ## Was hier schiefging
//!
//! Der Maler legte je **verschiedenem** Clip-Pfad eine `tiny_skia::Mask` an und
//! hielt sie bis zum Ende der Seite. Eine Maske ist immer so groß wie das ganze
//! Bild, gleich wie klein der Pfad ist: bei den Vorgabeoptionen
//! 1000 × 1415 Byte = 1,35 MB. Eine Decke gab es nicht. Gemessen
//! (Spitzenspeicher des Prozesses, Release, je ein eigenes `re W n`):
//!
//! | Clips |  vorher   | nachher |
//! |------:|----------:|--------:|
//! |    50 |   72,9 MB | 70,2 MB |
//! |   200 |  275,4 MB | 70,3 MB |
//! |   500 |  680,5 MB | 70,5 MB |
//! | 1 000 | 1355,6 MB | 70,9 MB |
//!
//! Linear, 1,35 MB je Pfad, aus einer Datei von 48 kB. Die Operationsdecke von
//! einer Million erlaubt rund 333 000 Pfade (ein `re W n` sind drei
//! Operationen) — das wären 450 GB.
//!
//! ## Der Preis, und warum er tragbar ist
//!
//! Über der Decke wird eine Maske neu gebaut, wenn sie wieder gebraucht wird.
//! Gehalten werden deshalb die **zuletzt** gebrauchten: ein Beschnitt, der
//! gesetzt und dann benutzt wird, und ein äußerer, der nach `Q` wieder
//! auflebt, treffen beide. Gemessen an einer Seite, deren Operationen
//! reihum zwischen den Clips wechseln (Release, 20 000 Operationen):
//! bei 2 und bei 40 verschiedenen Clips kostet die Decke nichts (3,1 s gegen
//! 3,3 s, abwechselnd gemessen), bei 100 verschiedenen — also mehr, als unter
//! die Decke passen — das 3,0- bis 3,5-fache. Dafür fällt der Speicher dort
//! von 149,7 MB auf 84,5 MB und bei 1 000 Clips von 1 355,6 MB auf 70,9 MB.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use lopdf::{dictionary, Document, Object, Stream};
use redact_render::{PageRenderer, RenderOptions};

/// Wie viele **verschiedene** seitengroße Clip-Pfade die Seite setzt.
const CLIPS: usize = 500;

/// Obergrenze für den Höchststand *während* des Renderns.
///
/// Gemessen wurden 70,5 MB (nachher) gegen 680,5 MB (vorher); die Schranke
/// liegt mit reichlich Luft dazwischen. Sie fällt sofort, sobald wieder eine
/// Maske je Pfad gehalten wird: allein die Masken wären dann
/// 500 × 1 415 000 Byte = 675 MB.
const MAX_SPITZE: usize = 224 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Der zählende Allokator
// ---------------------------------------------------------------------------

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Zaehlend;

impl Zaehlend {
    fn buche(delta: isize) {
        let vorher = LIVE.load(Ordering::Relaxed);
        let nachher = vorher.saturating_add_signed(delta);
        LIVE.store(nachher, Ordering::Relaxed);
        PEAK.fetch_max(nachher, Ordering::Relaxed);
    }
}

// SAFETY: Jede Anforderung wird unverändert an den Systemallokator
// weitergereicht; gezählt wird nur nebenher.
unsafe impl GlobalAlloc for Zaehlend {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            Self::buche(layout.size() as isize);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        Self::buche(-(layout.size() as isize));
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, neu: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, layout, neu) };
        if !ptr.is_null() {
            Self::buche(neu as isize - layout.size() as isize);
        }
        ptr
    }
}

#[global_allocator]
static ALLOC: Zaehlend = Zaehlend;

// ---------------------------------------------------------------------------
// Die Messung
// ---------------------------------------------------------------------------

/// Eine Seite, die `n` verschiedene seitengroße Beschnitte setzt und unter
/// jedem ein kleines Rechteck füllt.
fn seite_mit_clips(n: usize) -> Vec<u8> {
    let mut content = String::new();
    for k in 0..n {
        let x = k as f64 * 0.001;
        content.push_str(&format!("q {x} 0 595 842 re W n 10 10 100 100 re f Q\n"));
    }
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
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
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    out
}

#[test]
fn der_maskenspeicher_haengt_nicht_an_der_zahl_der_clips() {
    let bytes = seite_mit_clips(CLIPS);
    let doc = redact_pdf::document::load_from_bytes(&bytes).expect("ladbar");

    // Erst hier zählt die Messung: das Dokument steht, gemessen wird das Malen.
    let grundlast = LIVE.load(Ordering::Relaxed);
    PEAK.store(grundlast, Ordering::Relaxed);
    let seite = PageRenderer::new().render(&doc, 0, &RenderOptions::default());
    let spitze = PEAK.load(Ordering::Relaxed).saturating_sub(grundlast);

    // Die Seite muss dabei richtig bleiben — die Decke darf nichts verschlucken.
    assert_eq!(seite.drawn_ops, CLIPS, "nicht alle Operationen gezeichnet");
    assert!(!seite.degraded, "Notnagel-Pfad statt echtem Bild");
    assert!(
        seite.warnings.is_empty(),
        "unerwartete Warnungen: {:?}",
        seite.warnings
    );

    assert!(
        spitze <= MAX_SPITZE,
        "{CLIPS} Clip-Pfade aus einer Datei von {} Byte haben {:.1} MB gekostet \
         (Schranke {:.1} MB). Der Maler hält wieder eine bildgroße Maske je \
         Pfad — {CLIPS} × {} × {} Byte.",
        bytes.len(),
        spitze as f64 / 1_048_576.0,
        MAX_SPITZE as f64 / 1_048_576.0,
        seite.width,
        seite.height
    );
}
