//! Expertenprüfung: Speichergrenzen beim Auspacken von Prädiktoren und
//! Bildfiltern.
//!
//! * Der PNG-Prädiktor belegte `2 × Columns × bpp` Byte, bevor er eine Zeile
//!   las — `/Columns` gehört dem Angreifer, und die Vorprüfung las
//!   `/DecodeParms` nicht. Eine 504-Byte-Datei mit `/Columns 2000000000`
//!   belegte 3,8 GB, an `--max-decompressed-mb` vorbei; im Objekt-Stream
//!   schon beim Laden durch `lopdf`.
//! * `/Colors × /BitsPerComponent` lief über (Debug-Build: Panik).
//! * Der Bilddekoder entpackte Flate ohne Grenze und fiel auf rohes Deflate
//!   ab Byte 0 zurück, Vorprüfung und `lopdf` ab Byte 2: ein Strom, der dort
//!   sofort scheitert, war hier eine Bombe.
//! * An einem Bild-XObject galt `/F` als Filter (der Kurzname gilt nur für
//!   Inline-Bilder); die Vorprüfung sah keinen Filter und buchte Rohbytes.
//! * Ein Inline-Bild mit `/F /Fl` wurde ohne jede Grenze entpackt.
//!
//! Gemessen wird die Spitzenbelegung über einen zählenden Allocator. Die Tests
//! messen den ganzen Prozess; damit Nachbartests nicht mitzählen, laufen sie
//! hintereinander (ein gemeinsames `Mutex`).

use lopdf::dictionary;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

struct Peak;
static CUR: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Peak {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = CUR.fetch_add(l.size(), Ordering::SeqCst) + l.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        CUR.fetch_sub(l.size(), Ordering::SeqCst);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        if !q.is_null() {
            if n > l.size() {
                let now = CUR.fetch_add(n - l.size(), Ordering::SeqCst) + (n - l.size());
                PEAK.fetch_max(now, Ordering::SeqCst);
            } else {
                CUR.fetch_sub(l.size() - n, Ordering::SeqCst);
            }
        }
        q
    }
}

#[global_allocator]
static A: Peak = Peak;

/// Hält Nachbartests aus der Messung heraus.
static SERIELL: Mutex<()> = Mutex::new(());

fn seriell() -> MutexGuard<'static, ()> {
    SERIELL.lock().unwrap_or_else(|e| e.into_inner())
}

fn reset() -> usize {
    let c = CUR.load(Ordering::SeqCst);
    PEAK.store(c, Ordering::SeqCst);
    c
}
fn extra_since(base: usize) -> usize {
    PEAK.load(Ordering::SeqCst) - base
}

const MB: usize = 1 << 20;
const COLUMNS: usize = 100_000_000; // 2 × 100 MB

fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn assemble(body: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (id, data) in body {
        offsets.push((*id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendobj\n");
    }
    let max_id = body.iter().map(|(id, _)| *id).max().unwrap();
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max_id + 1).as_bytes());
    for id in 1..=max_id {
        let off = offsets
            .iter()
            .find(|(o, _)| *o == id)
            .map_or(0, |(_, o)| *o);
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            max_id + 1
        )
        .as_bytes(),
    );
    out
}

fn stream(dict_extra: &str, payload: &[u8]) -> Vec<u8> {
    let packed = deflate(payload);
    let mut out = format!(
        "<< /Length {} /Filter /FlateDecode{dict_extra} >>\nstream\n",
        packed.len()
    )
    .into_bytes();
    out.extend_from_slice(&packed);
    out.extend_from_slice(b"\nendstream");
    out
}

fn page_pdf(content_dict_extra: &str, extra: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let mut body = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, stream(content_dict_extra, b"\x00BT ET\n")),
    ];
    body.extend(extra);
    assemble(&body)
}

fn predictor() -> String {
    format!(" /DecodeParms << /Predictor 12 /Columns {COLUMNS} >>")
}

/// `decoded_content_within` mit 1 MB Grenze belegte über 190 MB.
#[test]
fn praediktor_sprengt_die_grenze_von_decoded_content_within() {
    let _seriell = seriell();
    let doc = lopdf::Document::with_version("1.5");
    let s = lopdf::Stream::new(
        dictionary! {
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 12, "Columns" => COLUMNS as i64 },
        },
        deflate(b"\x00abc"),
    )
    .with_compression(false);
    let base = reset();
    let _ = redact_pdf::filters::decoded_content_within(&doc, &s, MB);
    let extra = extra_since(base);
    assert!(extra < 16 * MB, "Spitze {} MB bei Grenze 1 MB", extra / MB);
}

/// Seiteninhalt: Laden mit Vorgabegrenzen und `scan_page`.
#[test]
fn praediktor_im_seiteninhalt() {
    let _seriell = seriell();
    let pdf = page_pdf(&predictor(), vec![]);
    let base = reset();
    // Die Vorprüfung darf die Datei ablehnen (dieselbe Zeile wäre in einem
    // Objekt-Stream eine Speicheranforderung an `lopdf`); lädt sie, darf das
    // Auspacken nicht mehr belegen als die Daten hergeben.
    if let Ok(doc) = redact_pdf::load_from_bytes(&pdf) {
        let page = *doc.get_pages().values().next().unwrap();
        let _ = redact_pdf::scan_page(&doc, page);
    }
    let extra = extra_since(base);
    assert!(extra < 64 * MB, "Spitze {} MB", extra / MB);
}

/// Objekt-Stream: schon `load_from_bytes` entpackt ihn (in `lopdf`) mit
/// Prädiktor. Die Vorprüfung lehnt die Datei ab, bevor `lopdf` belegt.
#[test]
fn praediktor_im_objektstrom_beim_laden() {
    let _seriell = seriell();
    let objstm = stream(
        &format!(" /Type /ObjStm /N 1 /First 4{}", predictor()),
        b"\x007 0 << >>",
    );
    let pdf = page_pdf("", vec![(5, objstm)]);
    let base = reset();
    let geladen = redact_pdf::load_from_bytes(&pdf);
    let extra = extra_since(base);
    let Err(fehler) = geladen else {
        panic!("die Vorprüfung lehnt nicht ab");
    };
    let fehler = fehler.to_string();
    assert!(fehler.contains("Prädiktorzeile"), "{fehler}");
    assert!(extra < 64 * MB, "Spitze {} MB", extra / MB);
}

/// Orakel: `leaks_many_within` mit 1 MB Budget.
#[test]
fn praediktor_im_orakel() {
    let _seriell = seriell();
    let pdf = page_pdf(&predictor(), vec![]);
    let base = reset();
    let _ = redact_pdf::leaks_many_within(&pdf, &["geheim"], MB as u64);
    let extra = extra_since(base);
    assert!(extra < 64 * MB, "Spitze {} MB bei Budget 1 MB", extra / MB);
}

/// `Colors × BitsPerComponent` läuft nicht mehr über (Debug-Build: Panik).
#[test]
fn colors_mal_bits_laeuft_ueber() {
    let _seriell = seriell();
    let doc = lopdf::Document::with_version("1.5");
    let s = lopdf::Stream::new(
        dictionary! {
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! {
                "Predictor" => 12, "Colors" => i64::MAX / 2, "BitsPerComponent" => 8 },
        },
        deflate(b"\x00abc"),
    )
    .with_compression(false);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        redact_pdf::filters::decoded_content(&doc, &s)
    }));
    assert!(r.is_ok(), "Panik beim Überlauf");
}

/// Rohes Deflate ab Byte 0 ist eine Bombe; ab Byte 2 (dort fällt die
/// Vorprüfung zurück) ist das Byte 0x06 ein Blockkopf mit BTYPE=3 →
/// sofortiger Fehler, 0 Byte. Zlib ab Byte 0: CM=0 → sofortiger Fehler.
fn divergent_flate(megabytes: usize) -> Vec<u8> {
    use std::io::Write;
    let mut out = vec![0x00, 0x00, 0x06, 0xFF, 0xF9]; // gespeicherter Block, LEN 0x0600
    out.extend(std::iter::repeat_n(0u8, 0x0600));
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    let zeros = vec![0u8; MB];
    for _ in 0..megabytes {
        e.write_all(&zeros).unwrap();
    }
    out.extend(e.finish().unwrap());
    out
}

fn image_pdf(megabytes: usize) -> Vec<u8> {
    image_pdf_with(divergent_flate(megabytes), "/Filter")
}

fn image_pdf_with(data: Vec<u8>, key: &str) -> Vec<u8> {
    let mut img = format!(
        "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
         /BitsPerComponent 8 {key} /FlateDecode /Length {} >>\nstream\n",
        data.len()
    )
    .into_bytes();
    img.extend_from_slice(&data);
    img.extend_from_slice(b"\nendstream");
    let body = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R \
              /Resources << /XObject << /Im1 5 0 R >> >> >>"
                .to_vec(),
        ),
        (4, stream("", b"q 595 0 0 842 0 0 cm /Im1 Do Q\n")),
        (5, img),
    ];
    assemble(&body)
}

/// Bild-Flate: der Rückfall auf rohes Deflate liest jetzt wie Vorprüfung und
/// `lopdf` ab Byte 2, und begrenzt. Vorher ließ die Vorprüfung (16 MB) die
/// Datei durch, und `page_ops` entpackte 256 MB.
#[test]
fn bild_flate_rueckfall_umgeht_die_vorpruefung() {
    let _seriell = seriell();
    let pdf = image_pdf(256);
    let limits = redact_pdf::document::Limits {
        max_decompressed_bytes: 16 * MB as u64,
        ..Default::default()
    };
    let doc = redact_pdf::document::load_from_bytes_with_limits(&pdf, &limits)
        .expect("Vorprüfung lässt die Datei durch");
    let base = reset();
    let _ = redact_pdf::page_ops(&doc, 0);
    let extra = extra_since(base);
    assert!(extra < 64 * MB, "Spitze {} MB bei Budget 16 MB", extra / MB);
}

/// An einem Bild-**XObject** ist `/F` kein Filter (die Abkürzung gilt nur für
/// Inline-Bilder; an einem Strom ist `/F` die Dateispezifikation). Die
/// Vorprüfung liest nur `/Filter` und bucht die Rohbytes; der Dekoder las `/F`
/// als Filter — ein zlib-Strom darunter war eine Bombe ohne Grenze.
#[test]
fn bild_xobject_mit_f_statt_filter_umgeht_die_vorpruefung() {
    let _seriell = seriell();
    let zeros = vec![0u8; 256 * MB];
    let pdf = image_pdf_with(deflate(&zeros), "/F");
    drop(zeros);
    let limits = redact_pdf::document::Limits {
        max_decompressed_bytes: 16 * MB as u64,
        ..Default::default()
    };
    let doc = redact_pdf::document::load_from_bytes_with_limits(&pdf, &limits)
        .expect("Vorprüfung lässt die Datei durch");
    let base = reset();
    let _ = redact_pdf::page_ops(&doc, 0);
    let extra = extra_since(base);
    assert!(extra < 64 * MB, "Spitze {} MB bei Budget 16 MB", extra / MB);
}

/// Inline-Bild mit `/F /Fl`: die Vorprüfung sieht nur den Seiteninhalt
/// (roh 256 KB); `page_ops` entpackte das Inline-Bild ohne Grenze.
#[test]
fn inline_bild_flate_ohne_grenze() {
    let _seriell = seriell();
    let zeros = vec![0u8; 256 * MB];
    let packed = deflate(&zeros);
    drop(zeros);
    let mut content = b"q 595 0 0 842 0 0 cm BI /W 1 /H 1 /CS /G /BPC 8 /F /Fl ID ".to_vec();
    content.extend_from_slice(&packed);
    content.extend_from_slice(b"\nEI Q\n");
    let mut s = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    s.extend_from_slice(&content);
    s.extend_from_slice(b"\nendstream");
    let body = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, s),
    ];
    let pdf = assemble(&body);
    let limits = redact_pdf::document::Limits {
        max_decompressed_bytes: 16 * MB as u64,
        ..Default::default()
    };
    let doc = redact_pdf::document::load_from_bytes_with_limits(&pdf, &limits)
        .expect("Vorprüfung lässt die Datei durch");
    let base = reset();
    let _ = redact_pdf::page_ops(&doc, 0);
    let extra = extra_since(base);
    assert!(extra < 64 * MB, "Spitze {} MB bei Budget 16 MB", extra / MB);
}

/// Der Kurzname `/F` gilt an einem Inline-Bild weiter: ein kleines
/// Flate-Inline-Bild wird dekodiert, nicht als Platzhalter gezeichnet.
#[test]
fn ein_kleines_inline_bild_mit_f_wird_weiter_dekodiert() {
    let _seriell = seriell();
    let packed = deflate(&[0x80; 4]);
    let mut content = b"q 595 0 0 842 0 0 cm BI /W 2 /H 2 /CS /G /BPC 8 /F /Fl ID ".to_vec();
    content.extend_from_slice(&packed);
    content.extend_from_slice(b"\nEI Q\n");
    let mut s = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    s.extend_from_slice(&content);
    s.extend_from_slice(b"\nendstream");
    let body = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, s),
    ];
    let doc = redact_pdf::load_from_bytes(&assemble(&body)).expect("lädt");
    let ops = redact_pdf::page_ops(&doc, 0).unwrap();
    let bild = ops
        .ops
        .iter()
        .find_map(|op| match op {
            redact_pdf::DrawOp::Image { image, .. } => Some(&ops.images[*image]),
            _ => None,
        })
        .expect("ein Bild");
    assert!(!bild.placeholder);
    assert_eq!(bild.rgba[..4], [0x80, 0x80, 0x80, 255]);
}

/// Die Vorprüfung rechnet die Prädiktorzeile wie `lopdf` mit mindestens 8 Bit
/// je Komponente. Ein gültiges bitonales Bild aus einer einzigen Zeile ist
/// dort achtmal so breit wie seine Daten — und wird trotzdem nicht abgelehnt.
#[test]
fn ein_einzeiliges_1_bit_praediktorbild_wird_nicht_abgelehnt() {
    let _seriell = seriell();
    let mut zeile = vec![0u8];
    zeile.extend_from_slice(&[0xAA; 27]);
    let packed = deflate(&zeile);
    let mut img = format!(
        "<< /Type /XObject /Subtype /Image /Width 216 /Height 1 /ColorSpace /DeviceGray \
         /BitsPerComponent 1 /Filter /FlateDecode /DecodeParms << /Predictor 15 \
         /Columns 216 /BitsPerComponent 1 >> /Length {} >>\nstream\n",
        packed.len()
    )
    .into_bytes();
    img.extend_from_slice(&packed);
    img.extend_from_slice(b"\nendstream");
    let body = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R \
              /Resources << /XObject << /Im1 5 0 R >> >> >>"
                .to_vec(),
        ),
        (4, stream("", b"q 216 0 0 1 0 0 cm /Im1 Do Q\n")),
        (5, img),
    ];
    let doc = redact_pdf::load_from_bytes(&assemble(&body)).expect("lädt");
    let ops = redact_pdf::page_ops(&doc, 0).unwrap();
    assert!(ops.images.iter().all(|b| !b.placeholder));
}
