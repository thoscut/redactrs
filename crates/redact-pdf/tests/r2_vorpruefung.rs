//! Runde 2, Vorprüfung und Lecksuche: Befunde aus der adversarialen Prüfung
//! des neuen Codes.
//!
//! * Ein doppeltes `/DecodeParms` versteckte den Prädiktor vor der
//!   Vorprüfung: die Rohsicht las den **ersten** Eintrag, `lopdf` nimmt den
//!   **letzten**.
//! * Der Name `/BI` (ein Marked-Content-Tag) startete in Sicht 6 den Sprung
//!   über ein Inline-Bild und blendete die Verkettung bis zum nächsten `EI`
//!   aus.
//! * Eine riesige Feldbreite in `/W` eines XRef-Stroms ließ `lopdf` eine
//!   unerfüllbare Allokation anfordern — Prozessabbruch statt `Err`.
//! * `/Index` eines XRef-Stroms mit Objektnummern um 2^31 machte den
//!   Schreibpfad 25–50 s langsam und `new_object_id` überlauffähig.
//! * Dazu der differentielle Test der Prädiktoren gegen eine eigene
//!   Referenz-Kodierung.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use common::{page, SECRET};
use lopdf::{dictionary, Object, Stream};

/// Zählt den Spitzenbedarf des Prozesses an Heap — gemessen wird, was
/// `lopdf` beim Laden belegt.
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

fn deflate(data: &[u8]) -> Vec<u8> {
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

/// `lopdf_predictor_rows` las `/DecodeParms` mit `raw_dict_entry` — dort
/// galt der **erste** Eintrag. `lopdf` legt das Dictionary mit
/// `Dictionary::set` an: es gilt der **letzte**. Ein harmloses erstes
/// `/DecodeParms` versteckte das zweite mit `/Columns 100000000` vor der
/// Vorprüfung, und `lopdf` belegte beim Laden des Objekt-Streams 2 × 100 MB.
#[test]
fn doppeltes_decodeparms_umgeht_die_praediktor_vorpruefung_nicht() {
    let _s = seriell();
    let objstm = stream(
        " /Type /ObjStm /N 1 /First 4 /DecodeParms << /Predictor 1 >> \
         /DecodeParms << /Predictor 12 /Columns 100000000 >>",
        b"\x007 0 << >>",
    );
    let body = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>".to_vec(),
        ),
        (5, objstm),
    ];
    let pdf = assemble(&body);
    let base = reset();
    let geladen = redact_pdf::load_from_bytes(&pdf);
    let extra = extra_since(base);
    assert!(
        extra < 64 * MB,
        "Spitze {} MB beim Laden einer {}-Byte-Datei (geladen: {})",
        extra / MB,
        pdf.len(),
        geladen.is_ok()
    );
}

/// Gegenprobe: dieselbe Zeile ohne Dublette lehnt die Vorprüfung ab.
#[test]
fn einfaches_decodeparms_wird_abgelehnt() {
    let _s = seriell();
    let objstm = stream(
        " /Type /ObjStm /N 1 /First 4 /DecodeParms << /Predictor 12 /Columns 100000000 >>",
        b"\x007 0 << >>",
    );
    let body = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>".to_vec(),
        ),
        (5, objstm),
    ];
    let pdf = assemble(&body);
    assert!(redact_pdf::load_from_bytes(&pdf).is_err());
}

fn leak_seite(content: &[u8]) -> Vec<u8> {
    let mut d = page(&[]);
    // Eine Schrift mit eigenen Glyphennamen (wie eine Teilmenge aus einem
    // Satzprogramm): der Schriftdekoder kennt `/g48` nicht und liest keinen
    // Text, die Bytes in den Zeichenketten sind trotzdem ASCII.
    let mut diffs: Vec<Object> = vec![32.into()];
    for c in 32..=122 {
        diffs.push(Object::Name(format!("g{c}").into_bytes()));
    }
    let font = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "ABCDEF+Satz",
        "Encoding" => dictionary! { "Type" => "Encoding", "Differences" => diffs },
    }));
    d.doc
        .get_dictionary_mut(d.resources_id)
        .unwrap()
        .set("Font", dictionary! { "F1" => font });
    d.doc.objects.insert(
        d.content_id,
        Object::Stream(Stream::new(dictionary! {}, content.to_vec())),
    );
    d.finish()
}

/// Sicht 6 (Zeichenketten-Verkettung) springt bei `BI` über ein
/// Inline-Bild. `keyword_at` gilt aber auch hinter `/` als abgegrenzt: der
/// **Name** `/BI` (ein Marked-Content-Tag) startete den Sprung, `(ID)` galt
/// als `ID`, `( EI)` als `EI` — alles dazwischen fiel aus der Verkettung.
/// Gibt die Zahl der Funde in Sicht 6 unter dem Tag `tag` zurück.
fn view6_hits(tag: &str) -> usize {
    // Eine Schrift, deren Codes der Dekoder nicht als Text liest: die
    // IBAN steht nur in den Bytes, über zwei TJ-Bruchstücke verteilt.
    let content = format!(
        "BT /F1 10 Tf 72 700 Td /{tag} BMC (ID) Tj [(DE89 3704 0044) -10 (0532 0130 00)] TJ ( EI) Tj EMC ET\n"
    );
    let pdf = leak_seite(content.as_bytes());
    let check = redact_pdf::leaks_many_within(&pdf, &[SECRET], u64::MAX);
    check.findings[0]
        .iter()
        .filter(|f| f.contains("Verkettung"))
        .count()
}

/// Unter `/BI` findet Sicht 6 dasselbe wie unter `/Span`.
#[test]
fn name_bi_blendet_die_zeichenketten_verkettung_nicht() {
    let _s = seriell();
    let span = view6_hits("Span");
    let bi = view6_hits("BI");
    assert!(span > 0, "Gegenprobe: Sicht 6 findet die IBAN unter /Span");
    assert_eq!(bi, span, "Sicht 6 ist unter dem Namen /BI blind");
}

// ---------------------------------------------------------------------------
// Prädiktoren, differentiell gegen eine eigene Referenz-Kodierung
// ---------------------------------------------------------------------------

/// Xorshift — reproduzierbar, ohne Abhängigkeit.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn paeth_ref(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i32 + b as i32 - c as i32;
    let (pa, pb, pc) = (
        (p - a as i32).abs(),
        (p - b as i32).abs(),
        (p - c as i32).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn png_encode(raw: &[u8], row: usize, bpp: usize, rng: &mut Rng) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev = vec![0u8; row];
    for line in raw.chunks(row) {
        let kind = rng.below(5) as u8;
        out.push(kind);
        for i in 0..line.len() {
            let a = if i >= bpp { line[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let pred = match kind {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) / 2) as u8,
                _ => paeth_ref(a, b, c),
            };
            out.push(line[i].wrapping_sub(pred));
        }
        prev[..line.len()].copy_from_slice(line);
    }
    out
}

fn get_bits(row: &[u8], idx: usize, bits: usize) -> u32 {
    let mut v = 0u32;
    for k in 0..bits {
        let bit = idx * bits + k;
        v = (v << 1) | u32::from((row[bit / 8] >> (7 - bit % 8)) & 1);
    }
    v
}
fn set_bits(row: &mut [u8], idx: usize, bits: usize, val: u32) {
    for k in 0..bits {
        let bit = idx * bits + k;
        let b = ((val >> (bits - 1 - k)) & 1) as u8;
        row[bit / 8] = (row[bit / 8] & !(1 << (7 - bit % 8))) | (b << (7 - bit % 8));
    }
}

fn tiff_encode(raw: &[u8], row: usize, colors: usize, bits: usize, columns: usize) -> Vec<u8> {
    let mut out = raw.to_vec();
    let modulo = if bits == 32 { 0 } else { 1u64 << bits };
    for (r, line) in raw.chunks(row).enumerate() {
        let o = &mut out[r * row..r * row + line.len()];
        for k in (colors..columns * colors).rev() {
            let own = get_bits(line, k, bits) as u64;
            let left = get_bits(line, k - colors, bits) as u64;
            set_bits(o, k, bits, ((own + modulo - left) % modulo) as u32);
        }
    }
    out
}

fn decode_with(
    predictor: i64,
    colors: usize,
    bits: usize,
    columns: usize,
    data: Vec<u8>,
) -> Option<Vec<u8>> {
    let doc = lopdf::Document::with_version("1.5");
    let s = Stream::new(
        dictionary! {
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! {
                "Predictor" => predictor, "Colors" => colors as i64,
                "BitsPerComponent" => bits as i64, "Columns" => columns as i64 },
        },
        deflate(&data),
    )
    .with_compression(false);
    redact_pdf::filters::decoded_content(&doc, &s)
}

/// 3000 zufällige Fälle aus PNG- (10–15) und TIFF-Prädiktor (2) über
/// Farben, Bittiefe, Breite und Höhe: der Dekoder muss jeden genau
/// zurückrechnen.
#[test]
fn praediktoren_rechnen_differentiell_richtig_zurueck() {
    let _s = seriell();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut failures = Vec::new();
    for case in 0..3000 {
        let colors = 1 + rng.below(4) as usize;
        let bits = [1usize, 2, 4, 8, 16][rng.below(5) as usize];
        let columns = 1 + rng.below(24) as usize;
        let rows = 1 + rng.below(6) as usize;
        let row = (colors * bits * columns).div_ceil(8);
        let mut raw: Vec<u8> = (0..row * rows).map(|_| rng.next() as u8).collect();
        // Füllbits am Zeilenende sind 0 (wie sie ein Erzeuger schreibt).
        let used = colors * bits * columns;
        if !used.is_multiple_of(8) {
            for line in raw.chunks_mut(row) {
                let last = line.len() - 1;
                line[last] &= 0xFFu8 << (8 - used % 8);
            }
        }
        let tiff = rng.below(2) == 0;
        let (pred, enc) = if tiff {
            (2, tiff_encode(&raw, row, colors, bits, columns))
        } else {
            let bpp = (colors * bits).div_ceil(8).max(1);
            (
                10 + rng.below(6) as i64,
                png_encode(&raw, row, bpp, &mut rng),
            )
        };
        let mut got = decode_with(pred, colors, bits, columns, enc);
        // Füllbits sind keine Abtastwerte: beim Vergleich ausgeblendet.
        if let Some(g) = got.as_mut() {
            if !used.is_multiple_of(8) && g.len() == raw.len() {
                for line in g.chunks_mut(row) {
                    let last = line.len() - 1;
                    line[last] &= 0xFFu8 << (8 - used % 8);
                }
            }
        }
        if got.as_deref() != Some(&raw[..]) {
            failures.push(format!(
                "Fall {case}: P{pred} C{colors} B{bits} W{columns} H{rows}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} Abweichungen, z. B. {:?}",
        failures.len(),
        &failures[..failures.len().min(8)]
    );
}

// ---------------------------------------------------------------------------
// XRef-Strom: Feldbreiten und Objektnummern aus der Datei
// ---------------------------------------------------------------------------

/// Eine Seite, Querverweise als **XRef-Strom** mit den Feldbreiten `w`; die
/// Objekte tragen die Nummern `first + 1` bis `first + 5`, und `/Index`
/// beginnt bei `first`. Bei `w = [1, 2, 2]` und `first = 0` ist die Datei
/// gewöhnlich.
fn pdf_with_xref_stream(w: [u64; 3], first: u64) -> Vec<u8> {
    let n = |k: u64| first + k;
    let mut out = b"%PDF-1.5\n".to_vec();
    let mut offsets = Vec::new();
    let objects = [
        format!("<</Type/Catalog/Pages {} 0 R>>", n(2)),
        format!("<</Type/Pages/Kids[{} 0 R]/Count 1>>", n(3)),
        format!(
            "<</Type/Page/Parent {} 0 R/MediaBox[0 0 200 200]/Contents {} 0 R>>",
            n(2),
            n(4)
        ),
        "<</Length 18>>stream\nBT /F1 12 Tf ET\n\nendstream".to_string(),
    ];
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", n(i as u64 + 1)).as_bytes());
        out.extend_from_slice(body.as_bytes());
        out.extend_from_slice(b"\nendobj\n");
    }
    // Einträge mit den echten Breiten 1/2/2 (für die gültige Fassung).
    let xref_off = out.len();
    let mut data = vec![0u8, 0, 0, 0xff, 0xff];
    for off in &offsets {
        data.push(1);
        data.extend_from_slice(&(*off as u16).to_be_bytes());
        data.extend_from_slice(&[0, 0]);
    }
    data.push(1);
    data.extend_from_slice(&(xref_off as u16).to_be_bytes());
    data.extend_from_slice(&[0, 0]);
    out.extend_from_slice(
        format!(
            "{} 0 obj\n<</Type/XRef/Size {}/Index[{first} 6]/W[{} {} {}]/Root {} 0 R/Length {}>>stream\n",
            n(5),
            n(6),
            w[0],
            w[1],
            w[2],
            n(1),
            data.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&data);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    out.extend_from_slice(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    out
}

/// Gegenprobe: die gewöhnliche Fassung lädt und behält ihre Nummern.
#[test]
fn xref_strom_gewoehnlich_laedt() {
    let _s = seriell();
    let doc = redact_pdf::load_from_bytes(&pdf_with_xref_stream([1, 2, 2], 0))
        .expect("die gültige Fassung muss laden");
    assert_eq!(redact_pdf::page_count(&doc), 1);
    assert!(doc.objects.contains_key(&(4, 0)), "Nummern verändert");
}

const CHILD_ENV: &str = "R2_VORPRUEFUNG_KIND";

/// Läuft nur als Kindprozess (siehe unten): ein Abbruch beendete sonst die
/// ganze Testbinary.
#[test]
#[ignore = "wird von riesige_feldbreite_wird_abgelehnt_statt_abzubrechen als Kindprozess gestartet"]
fn kind_riesige_feldbreite() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    // 2^60 Byte: auf keiner Maschine anlegbar, auch nicht mit Overcommit.
    let bytes = pdf_with_xref_stream([1, 1 << 60, 2], 0);
    assert!(
        redact_pdf::load_from_bytes(&bytes).is_err(),
        "eine Feldbreite von 2^60 Byte ist kein PDF"
    );
    // Und die Lecksuche, die über die Vorprüfung selbst lädt.
    let check = redact_pdf::leaks_many_within(&bytes, &[SECRET], 1 << 26);
    assert!(!check.unchecked.is_empty(), "Lecksuche ohne Vorbehalt");
}

/// `/W [1 2^60 2]` in einer 700-Byte-Datei: abgelehnt, nicht abgebrochen.
#[test]
fn riesige_feldbreite_wird_abgelehnt_statt_abzubrechen() {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "kind_riesige_feldbreite",
            "--ignored",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("Kindprozess startet");
    assert!(
        out.status.success(),
        "Laden mit /W [1 2^60 2] hat den Prozess beendet statt Err zu liefern: {:?}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .take(3)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Objektnummern um 2^31 aus `/Index`: nach dem Laden fortlaufend, und
/// `max_id` steht in der Größenordnung der Objektzahl.
#[test]
fn objektnummern_aus_index_werden_verdichtet() {
    let _s = seriell();
    let mut doc =
        redact_pdf::load_from_bytes(&pdf_with_xref_stream([1, 2, 2], 1 << 31)).expect("lädt");
    assert!(
        u64::from(doc.max_id) <= doc.objects.len() as u64 + 1,
        "max_id {} bei {} Objekten",
        doc.max_id,
        doc.objects.len()
    );
    assert_eq!(redact_pdf::page_count(&doc), 1, "Verweise nicht mitgezogen");
    // Ein neues Objekt bekommt eine kleine Nummer, kein Überlauf.
    let id = doc.add_object(Object::Null);
    assert!(id.0 <= doc.objects.len() as u32);
}
