//! Der Speicher der Schwärzung darf nicht am **Produkt** aus Zeichen und
//! Bereichen hängen.
//!
//! ## Warum eine eigene Datei mit einem einzigen Test
//!
//! Gemessen wird mit einem zählenden Allokator, und der zählt den **ganzen
//! Prozess**. Liefe daneben ein zweiter Test, mäße dieser hier dessen
//! Speicher mit. Ein Test je Testprogramm ist die einzige Anordnung, in der
//! die Zahl etwas bedeutet.
//!
//! ## Warum keine Uhr
//!
//! Eine Zeitmessung flattert mit der Fremdlast der Maschine. Belegter
//! Speicher tut das nicht: dieselbe Eingabe fordert dieselben Bytes an,
//! Lauf für Lauf. Die Messreihen mit Uhr stehen in `rev4_bench.rs` und sind
//! dort bewusst `#[ignore]`.
//!
//! ## Was hier schiefging
//!
//! Je Textoperation und je Bereich, der ihre Hülle berührte, wurde ein
//! `Vec<bool>` über **alle** Zeichen der Operation angelegt — und in der
//! Auswahl behalten. Die Hülle einer langen `Tj` überdeckt die ganze Zeile,
//! also berührt sie jeder Bereich der Zeile. Gemessen an k Geheimnissen in
//! **einer** `Tj` (Spitzenspeicher des Prozesses):
//!
//! | k     |   vorher |  nachher |
//! |------:|---------:|---------:|
//! |   500 |  16,5 MB |   9,7 MB |
//! | 1 000 |  42,6 MB |  15,2 MB |
//! | 2 000 | 136,4 MB |  26,1 MB |
//! | 4 000 | 489,8 MB |  48,6 MB |
//! | 8 000 | 1 860 MB |  91,9 MB |
//!
//! Der Zähler vervierfachte sich bei jeder Verdopplung; jetzt verdoppelt er
//! sich. Die rechnerische Obergrenze aus `MAX_GLYPHS_PER_SCAN` (10⁶ Zeichen)
//! und der CLI-Schranke `--max-candidates` (10⁵) war 100 GB.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use lopdf::{dictionary, Document, Object, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, save_to_bytes, scan_page, PdfRedactor};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

/// Wie viele Geheimnisse in **eine** `Tj` gesetzt werden.
const SECRETS: usize = 2_000;

/// Obergrenze für den Höchststand *während* der Schwärzung.
///
/// Gemessen wurden 17,0 MB (nachher) gegen 127,3 MB (vorher) — die Schranke
/// liegt mit reichlich Luft dazwischen und fällt trotzdem sofort, wenn je
/// Bereich wieder ein Vektor über alle Zeichen entsteht: allein diese
/// Vektoren wären 2 000 × 58 000 Byte = 116 MB. Was übrig bleibt, ist der
/// Bauplan der neu zu schreibenden Operation selbst, und der hängt an der
/// Zahl der Zeichen — nicht an der Zahl der Bereiche.
const MAX_SPITZE: usize = 48 * 1024 * 1024;

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

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let neu = unsafe { System.realloc(ptr, layout, new_size) };
        if !neu.is_null() {
            Self::buche(new_size as isize - layout.size() as isize);
        }
        neu
    }
}

#[global_allocator]
static ALLOC: Zaehlend = Zaehlend;

// ---------------------------------------------------------------------------

fn blackout(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
            rect,
            None,
            Source::Manual {
                reason: "Speichermessung".into(),
            },
        ),
        Action::Blackout,
    )
}

#[test]
fn der_speicher_haengt_nicht_am_produkt_aus_zeichen_und_bereichen() {
    let mut doc = Document::with_version("1.7");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let mut text = String::with_capacity(SECRETS * (SECRET.len() + 2));
    for _ in 0..SECRETS {
        text.push_str(SECRET);
        text.push_str("  ");
    }
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        format!("BT /F1 4 Tf 1 0 0 1 5 400 Tm ({text}) Tj ET\n").into_bytes(),
    ));
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

    // Je Vorkommen ein Bereich, genau über dessen Zeichen.
    let scan = scan_page(&doc, page_id).expect("lesbar");
    let glyphs: Vec<_> = scan.shows[0].glyphs().collect();
    let volltext: String = glyphs.iter().map(|g| g.text.as_str()).collect();
    let mut redactions = Vec::new();
    let mut from = 0usize;
    while let Some(at) = volltext[from..].find(SECRET) {
        let start = from + at;
        let end = start + SECRET.len();
        redactions.push(blackout(
            glyphs[start..end]
                .iter()
                .map(|g| g.rect)
                .reduce(|a, b| a.union(&b))
                .expect("nicht leer"),
        ));
        from = end;
    }
    drop(glyphs);
    drop(scan);
    assert_eq!(redactions.len(), SECRETS);

    // Ab hier wird gemessen.
    let vorher = LIVE.load(Ordering::Relaxed);
    PEAK.store(vorher, Ordering::Relaxed);
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &redactions)
        .expect("schwärzbar");
    let spitze = PEAK.load(Ordering::Relaxed).saturating_sub(vorher);

    // Die Zahlen müssen stimmen, sonst misst der Test Sparsamkeit beim
    // Nichtstun.
    assert_eq!(report.removed_glyphs, SECRET.len() * SECRETS);
    assert!(
        report.per_redaction.iter().all(|n| *n == SECRET.len()),
        "nicht jede Region hat ihr Vorkommen ganz getroffen"
    );
    assert!(
        leaks(&save_to_bytes(&doc).expect("speicherbar"), SECRET).is_empty(),
        "das Geheimnis steht noch in der Datei"
    );

    assert!(
        spitze <= MAX_SPITZE,
        "Spitzenspeicher der Schwärzung {spitze} Byte über der Schranke {MAX_SPITZE} — \
         wächst der Speicher wieder mit Zeichen × Bereichen?"
    );
}
