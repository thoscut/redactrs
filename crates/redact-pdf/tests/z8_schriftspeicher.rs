//! Der Speicher eines Scans darf nicht an der **Zahl der Schriften** eines
//! `/Resources` hängen.
//!
//! ## Warum eine eigene Datei mit einem einzigen Test
//!
//! Gemessen wird mit einem zählenden Allokator, und der zählt den **ganzen
//! Prozess**. Ein Test je Testprogramm ist die einzige Anordnung, in der die
//! Zahl etwas bedeutet — dieselbe Begründung wie in
//! `rev4_speicher_je_bereich.rs`.
//!
//! ## Was hier schiefging
//!
//! `Budget::load_font_map` lädt **alle** Schriften eines
//! Ressourcenverzeichnisses in eine [`FontMap`], und die hält der Interpreter,
//! solange er in diesem Strom ist. Die vorhandene Decke
//! (`MAX_CACHED_FONT_ENTRIES`) sagt nur, was darüber hinaus *gemerkt* wird —
//! die Karte selbst sah sie nicht. Gemessen (Spitzenspeicher, je Schrift eine
//! `/ToUnicode` über den vollen Bereich von 65 536 Einträgen, alle in **einem**
//! `/Resources`, gesetzt wird nur mit der ersten):
//!
//! | Schriften | Datei    |   vorher | nachher |
//! |----------:|---------:|---------:|--------:|
//! |         1 |   0,9 kB |   4,0 MB |  4,0 MB |
//! |        10 |   4,9 kB |  39,8 MB | 39,8 MB |
//! |        50 |  22,5 kB | 198,9 MB | 63,6 MB |
//! |       100 |  44,8 kB | 397,7 MB | 63,7 MB |
//! |       500 | 223,2 kB |     — ¹  | 63,8 MB |
//!
//! ¹ nicht mehr gemessen; linear wären es rund 2 GB.
//!
//! Rund 4 MB je Schrift bei rund 450 Byte Dateizuwachs je Schrift — Faktor
//! 9 000, ohne Decke. Eine Datei von einem Megabyte käme so auf neun Gigabyte.
//!
//! ## Die Antwort, und was sie kostet
//!
//! Ab `MAX_FONT_ENTRIES_PER_SCAN` wird die Datei **abgelehnt** statt still
//! weitergebaut. Eine Schrift, die nicht geladen ist, hat keine
//! `/ToUnicode`-Zuordnung; ihr Text würde falsch oder gar nicht dekodiert, und
//! „ich habe den Text nicht gelesen“ ist für ein Schwärzungswerkzeug kein
//! zulässiges Zwischenergebnis.
//!
//! Der Preis ist die Grenze selbst: eine Million Tabelleneinträge sind rund
//! 60 MB und entsprechen fünfzehn CJK-Schriften vollen Umfangs **auf einer
//! Seite**. Fünfzehn passen durch, sechzehn nicht. Eine Seite mit fünf bis
//! zehn Schriften — der Normalfall — bleibt weit darunter, und
//! `rev4_interpreter_ceilings::die_schriftendecke_haelt` (sieben mal 65 536 =
//! 458 752) ebenfalls.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

/// Wie viele **verschiedene** schwere Schriften in einem `/Resources` stehen.
const SCHRIFTEN: usize = 100;

/// Wie viele Einträge die `/ToUnicode` je Schrift hat — der volle Bereich, also
/// so viel, wie eine ehrliche CJK-Schrift höchstens trägt.
const EINTRAEGE: u32 = 65_536;

/// Obergrenze für den Höchststand *während* des Scans.
///
/// Gemessen wurden 63,7 MB (nachher) gegen 397,7 MB (vorher); die Schranke
/// liegt mit reichlich Luft dazwischen und fällt sofort, sobald wieder alle
/// Schriften eines Verzeichnisses gleichzeitig geladen werden: allein deren
/// Tabellen wären 100 × 65 536 Einträge.
const MAX_SPITZE: usize = 160 * 1024 * 1024;

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
// Testdateien
// ---------------------------------------------------------------------------

/// Eine Schrift mit `entries` Einträgen in `/ToUnicode`.
fn schrift(doc: &mut Document, entries: u32, base: u32) -> ObjectId {
    let hi = entries.saturating_sub(1).min(0xFFFF);
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         /CMapName /Test def /CMapType 2 def\n\
         1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
         1 beginbfrange <0000> <{hi:04X}> <{base:04X}> endbfrange\n\
         endcmap CMapName currentdict /CMap defineresource pop end end\n"
    );
    let cmap_id = doc.add_object(Stream::new(dictionary! {}, cmap.into_bytes()));
    doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
        "ToUnicode" => cmap_id,
    })
}

/// Eine Seite mit `k` verschiedenen Schriften in **einem** `/Resources`;
/// gesetzt wird nur mit der ersten.
fn seite(k: usize, entries: u32) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut fonts = Dictionary::new();
    for j in 0..k {
        let f = schrift(&mut doc, entries, 0x2000 + j as u32 * 0x10);
        fonts.set(format!("F{j}"), f);
    }
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /F0 12 Tf 20 700 Td (A) Tj ET\n".to_vec(),
    ));
    let res_id = doc.add_object(dictionary! { "Font" => fonts });
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
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    out
}

fn scan(bytes: &[u8]) -> redact_core::Result<redact_pdf::ScanResult> {
    let doc = redact_pdf::document::load_from_bytes(bytes).expect("ladbar");
    let seite = doc
        .get_pages()
        .values()
        .copied()
        .next()
        .expect("eine Seite");
    redact_pdf::scan_page(&doc, seite)
}

// ---------------------------------------------------------------------------
// Die Messung
// ---------------------------------------------------------------------------

#[test]
fn der_schriftspeicher_haengt_nicht_an_der_zahl_der_schriften() {
    let bytes = seite(SCHRIFTEN, EINTRAEGE);
    let doc = redact_pdf::document::load_from_bytes(&bytes).expect("ladbar");
    let seite_id = doc
        .get_pages()
        .values()
        .copied()
        .next()
        .expect("eine Seite");

    // Erst hier zählt die Messung: das Dokument steht, gemessen wird der Scan.
    let grundlast = LIVE.load(Ordering::Relaxed);
    PEAK.store(grundlast, Ordering::Relaxed);
    let ergebnis = redact_pdf::scan_page(&doc, seite_id);
    let spitze = PEAK.load(Ordering::Relaxed).saturating_sub(grundlast);

    assert!(
        spitze <= MAX_SPITZE,
        "{SCHRIFTEN} Schriften aus einer Datei von {} Byte haben {:.1} MB gekostet \
         (Schranke {:.1} MB). Es sind wieder alle Schriften eines Verzeichnisses \
         gleichzeitig geladen.",
        bytes.len(),
        spitze as f64 / 1_048_576.0,
        MAX_SPITZE as f64 / 1_048_576.0
    );

    // Und zwar, weil abgelehnt wird — nicht, weil stillschweigend weniger
    // geladen wurde. Der Unterschied ist der ganze Punkt.
    let fehler = ergebnis
        .expect_err("die Datei muss abgelehnt werden")
        .to_string();
    assert!(
        fehler.contains("Schrift-Tabelleneinträge"),
        "abgelehnt, aber mit anderer Begründung: {fehler}"
    );

    // --- Gegenprobe: das gewöhnliche Dokument läuft unverändert durch -------
    //
    // Zehn Schriften auf einer Seite sind normal. Sie müssen durchgehen, und
    // der Text muss über die `/ToUnicode` der gesetzten Schrift richtig
    // dekodiert werden.
    let gewoehnlich = scan(&seite(10, EINTRAEGE)).expect("zehn Schriften sind gewöhnlich");
    assert_eq!(gewoehnlich.effort.parsed_fonts, 10);
    let text: String = gewoehnlich
        .shows
        .iter()
        .flat_map(|r| r.glyphs())
        .map(|g| g.text.as_str())
        .collect();
    assert_eq!(
        text,
        char::from_u32(0x2000 + u32::from(b'A'))
            .expect("Zeichen")
            .to_string(),
        "der Text wird nicht mehr über die /ToUnicode dekodiert"
    );

    // Und ein Verzeichnis, das dieselbe schwere Schrift unter vielen Namen
    // führt, kostet einmal — die Decke zählt Objekte, nicht Namen.
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let eine = schrift(&mut doc, EINTRAEGE, 0x2000);
    let mut fonts = Dictionary::new();
    for j in 0..200 {
        fonts.set(format!("F{j}"), eine);
    }
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /F0 12 Tf 20 700 Td (A) Tj ET\n".to_vec(),
    ));
    let res_id = doc.add_object(dictionary! { "Font" => fonts });
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
    let mut viele_namen = Vec::new();
    doc.save_to(&mut viele_namen).expect("speicherbar");
    assert!(
        scan(&viele_namen).is_ok(),
        "zweihundert Namen auf **eine** Schrift dürfen die Decke nicht reißen"
    );
}
