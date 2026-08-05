//! Messreihen zu Punkt 1 und 2 der Prüfung — **nicht** Teil des Gates.
//!
//! Alle Tests hier sind `#[ignore]`: sie messen Zeit und Spitzenspeicher und
//! flattern deshalb mit der Fremdlast der Maschine. Gedacht sind sie zum
//! Nachrechnen von Hand:
//!
//! ```text
//! cargo test --release -p redact-pdf --test rev4_bench -- --ignored --nocapture
//! ```
//!
//! Ein Fall je Prozess (`--test-threads=1` und ein Filter), sonst mischen sich
//! die Spitzenspeicher: `VmHWM` ist ein Höchststand des ganzen Prozesses und
//! fällt nie wieder.

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{scan_page, PdfRedactor};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

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

struct Doc {
    doc: Document,
    page_id: ObjectId,
    content_id: ObjectId,
}

impl Doc {
    fn new() -> Self {
        let mut doc = Document::with_version("1.7");
        let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
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
        Self {
            doc,
            page_id,
            content_id,
        }
    }

    fn set_content(&mut self, raw: impl AsRef<[u8]>) {
        if let Ok(stream) = self
            .doc
            .get_object_mut(self.content_id)
            .unwrap()
            .as_stream_mut()
        {
            stream.set_plain_content(raw.as_ref().to_vec());
        }
    }

    fn size(&mut self) -> usize {
        let mut buf = Vec::new();
        self.doc.save_to(&mut buf).expect("speicherbar");
        buf.len()
    }
}

fn blackout(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
            rect,
            None,
            Source::Manual {
                reason: "Messung".into(),
            },
        ),
        Action::Blackout,
    )
}

/// `k` Geheimnisse in **einer einzigen** `Tj`-Operation.
fn one_tj(k: usize) -> Doc {
    let mut doc = Doc::new();
    let mut text = String::with_capacity(k * (SECRET.len() + 2));
    for _ in 0..k {
        text.push_str(SECRET);
        text.push_str("  ");
    }
    doc.set_content(format!("BT /F1 4 Tf 1 0 0 1 5 400 Tm ({text}) Tj ET\n"));
    doc
}

/// Dieselben Glyphen, aber in `k` einzelne `Tj` zerlegt — die Kontrolle.
fn many_tj(k: usize) -> Doc {
    let mut doc = Doc::new();
    let mut content = String::from("BT /F1 4 Tf\n");
    let mut x = 5.0f64;
    for _ in 0..k {
        content.push_str(&format!("1 0 0 1 {x} 400 Tm ({SECRET}) Tj\n"));
        x += 4.0 * 0.55 * (SECRET.len() + 2) as f64;
    }
    content.push_str("ET\n");
    doc.set_content(content);
    doc
}

/// `k` Geheimnisse **untereinander** — eine Spalte, gleiche x-Spanne.
/// Das ist der Fall, an dem ein Streifenzug über x nichts aussiebt.
fn column(k: usize) -> Doc {
    let mut doc = Doc::new();
    let mut content = String::from("BT /F1 4 Tf\n");
    for line in 0..k {
        let y = 5.0 + line as f64 * 4.0;
        content.push_str(&format!("1 0 0 1 5 {y} Tm ({SECRET}) Tj\n"));
    }
    content.push_str("ET\n");
    doc.set_content(content);
    doc
}

fn redactions_for_secret(doc: &Doc) -> Vec<Redaction> {
    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    let mut out = Vec::new();
    for record in &scan.shows {
        let glyphs: Vec<_> = record.glyphs().collect();
        let text: String = glyphs.iter().map(|g| g.text.as_str()).collect();
        let mut from = 0usize;
        while let Some(at) = text[from..].find(SECRET) {
            let start = from + at;
            let end = start + SECRET.len();
            let rect = glyphs[start..end]
                .iter()
                .map(|g| g.rect)
                .reduce(|a, b| a.union(&b))
                .expect("nicht leer");
            out.push(blackout(rect));
            from = end;
        }
    }
    out
}

fn measure(name: &str, mut doc: Doc) {
    let size = doc.size();
    let redactions = redactions_for_secret(&doc);
    let before = vm_hwm_kb();
    let start = std::time::Instant::now();
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc.doc, &redactions)
        .expect("schwärzbar");
    let elapsed = start.elapsed();
    let after = vm_hwm_kb();
    println!(
        "{name}: Datei {} kB, {} Bereiche, {:.3} s, VmHWM {:.1} MB (vorher {:.1} MB), \
         entfernt {}, Σ je Region {}",
        size / 1024,
        redactions.len(),
        elapsed.as_secs_f64(),
        after as f64 / 1024.0,
        before as f64 / 1024.0,
        report.removed_glyphs,
        report.per_redaction.iter().sum::<usize>(),
    );
}

macro_rules! bench {
    ($name:ident, $build:ident, $k:expr) => {
        #[test]
        #[ignore = "Messung, keine Zusicherung"]
        fn $name() {
            measure(
                concat!(stringify!($build), " k=", stringify!($k)),
                $build($k),
            );
        }
    };
}

/// **Differenzprobe zur Zählung.** Druckt für viele zufällig gelegte Bereiche
/// die vollständige Aufschlüsselung je Region. Der Lauf ist deterministisch;
/// die Ausgabe muss vor und nach dem Umbau Zeile für Zeile dieselbe sein.
///
/// ```text
/// cargo test --release -p redact-pdf --test rev4_bench -- --ignored --nocapture \
///     --exact zaehlung_fingerabdruck > nachher.txt
/// ```
#[test]
#[ignore = "Differenzprobe von Hand"]
fn zaehlung_fingerabdruck() {
    // Ein einfacher LCG — überall gleich, ohne Fremdcode.
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = |modulo: u64| {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) % modulo
    };

    for fall in 0..40 {
        // Mal alles in eine Operation, mal je Vorkommen eine.
        let mut doc = if fall % 2 == 0 {
            one_tj(12)
        } else {
            many_tj(12)
        };
        let mut redactions = redactions_for_secret(&doc);
        // Dazu Bereiche, die quer über die Zeile liegen.
        for _ in 0..20 {
            let x = next(1200) as f64;
            let w = 1.0 + next(60) as f64;
            let y = 390.0 + next(20) as f64;
            let h = 1.0 + next(12) as f64;
            redactions.push(blackout(Rect::new(x, y, x + w, y + h)));
        }
        let report = PdfRedactor::with_padding(0.0)
            .apply_with_report(&mut doc.doc, &redactions)
            .expect("schwärzbar");
        println!(
            "{fall}: entfernt {} je Region {:?}",
            report.removed_glyphs, report.per_redaction
        );
    }
}

bench!(one_tj_00500, one_tj, 500);
bench!(one_tj_01000, one_tj, 1_000);
bench!(one_tj_02000, one_tj, 2_000);
bench!(one_tj_04000, one_tj, 4_000);
bench!(one_tj_08000, one_tj, 8_000);

bench!(many_tj_04000, many_tj, 4_000);
bench!(many_tj_08000, many_tj, 8_000);

bench!(column_02000, column, 2_000);
bench!(column_04000, column, 4_000);
bench!(column_08000, column, 8_000);
bench!(column_16000, column, 16_000);
