//! Spur A, Runde 2, Register #89: die Vorprüfung zählt das **Teilergebnis**
//! eines kaputten Flate-Stroms — so viel, wie jeder Leser nach ihr daraus
//! bekommt.
//!
//! `lopdf` (beim Laden eines Objekt-Streams) und der Schreibpfad
//! (`filters::decoded_content`) lesen einen zlib-Strom, der mitten drin
//! kaputt ist, bis zur kaputten Stelle und behalten, was bis dahin kam. Die
//! Vorprüfung gab beim ersten Fehler auf und buchte die Rohgröße: ein Strom
//! mit einem MiB Nullen vor einem ungültigen Block zählte mit rund einem KB.
//! Viele solcher Ströme, jeder unter dem Budget, gingen durch — und entpackt
//! wurden sie danach ohne Grenze.
//!
//! Der Fall mit der kaputten Prüfsumme am Ende
//! (`zp_e_vorpruefung_als_schranke`, CLI) fängt zusätzlich der Rückfall auf
//! rohes Deflate; hier ist der Block selbst ungültig, und es bleibt allein
//! das Teilergebnis.

use std::io::Read;

use redact_pdf::document::{prescan, Limits};

/// Ein MiB Nullen als zlib-Strom mit **nicht** abschließendem Block, danach
/// ein Blockkopf mit dem reservierten Typ 3 (`0x07`: letzter Block, Typ
/// binär 11) — ab dort ist der Strom ungültig.
fn mitten_kaputt() -> Vec<u8> {
    let input = vec![0u8; 1 << 20];
    let mut compress = flate2::Compress::new(flate2::Compression::best(), true);
    let mut out = Vec::with_capacity(64 * 1024);
    compress
        .compress_vec(&input, &mut out, flate2::FlushCompress::Sync)
        .expect("packbar");
    assert_eq!(
        compress.total_in(),
        input.len() as u64,
        "nicht ganz gepackt"
    );
    out.push(0x07);
    out.extend_from_slice(&[0xFF; 8]);
    out
}

/// Eine Seite aus `anzahl` Inhaltsströmen, jeder mit `daten` unter
/// `/FlateDecode`.
fn seite_aus_stroemen(anzahl: u32, daten: &[u8]) -> Vec<u8> {
    let inhalte: Vec<String> = (0..anzahl).map(|i| format!("{} 0 R", 4 + i)).collect();
    let mut objekte: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents [{}] >>",
            inhalte.join(" ")
        )
        .into_bytes(),
    ];
    for _ in 0..anzahl {
        let mut strom = format!(
            "<< /Filter /FlateDecode /Length {} >>\nstream\n",
            daten.len()
        )
        .into_bytes();
        strom.extend_from_slice(daten);
        strom.extend_from_slice(b"\nendstream");
        objekte.push(strom);
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objekte.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objekte.len() + 1).as_bytes(),
    );
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objekte.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Die Voraussetzung: die Leser nach der Vorprüfung bekommen aus diesem
/// Strom wirklich etwas heraus — der zlib-Leser meldet den Fehler und hat
/// trotzdem Bytes geliefert, `lopdf` gibt sie zurück.
#[test]
fn die_leser_nach_der_vorpruefung_behalten_das_teilergebnis() {
    let daten = mitten_kaputt();
    let mut out = Vec::new();
    let ergebnis = flate2::read::ZlibDecoder::new(&daten[..]).read_to_end(&mut out);
    assert!(ergebnis.is_err(), "der Strom muss kaputt sein");
    assert!(!out.is_empty(), "der zlib-Leser liefert nichts");

    let stream = lopdf::Stream::new(
        lopdf::dictionary! { "Filter" => "FlateDecode" },
        daten.clone(),
    );
    let gelesen = stream.decompressed_content().expect("lopdf liest");
    assert!(!gelesen.is_empty(), "lopdf liefert nichts");
}

/// 24 solcher Ströme, jeder ein MiB Teilergebnis, Budget 16 MiB: die
/// Vorprüfung muss ablehnen.
#[test]
fn viele_teilergebnisse_reissen_das_budget() {
    let pdf = seite_aus_stroemen(24, &mitten_kaputt());
    assert!(pdf.len() < 256 * 1024, "{} Byte", pdf.len());
    let limits = Limits {
        max_decompressed_bytes: 16 * 1024 * 1024,
        ..Limits::default()
    };
    let fehler = prescan(&pdf, &limits).expect_err("die Vorprüfung lässt die Datei durch");
    assert!(
        fehler
            .to_string()
            .contains("überschreiten das Budget von 16 MB"),
        "{fehler}"
    );
}
