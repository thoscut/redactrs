//! Spur-A-Runde 2, Prüfer A (Register #101): die Bilddecke galt den Maßen
//! des Bild-Dictionaries, der JPEG-Dekoder belegte die Maße aus dem JPEG.
//!
//! # Der Befund (Dienstverweigerung)
//!
//! `--max-image-mb` und die harte Pixeldecke prüfen `/Width` × `/Height`.
//! `decode_jpeg` (`ops.rs`) las die Maße dagegen erst beim Dekodieren aus dem
//! SOF-Kopf des JPEG und belegte sie ohne Frage. Ein Dictionary mit 100 × 100
//! über einem JPEG mit 6000 × 6000 kam an jeder Decke vorbei: am gebauten
//! Binary 262 MB Spitze bei `--max-image-mb 8`, Rückgabewert 0; mit 16000 ×
//! 16000 über 1 GB und Minuten Rechenzeit. Voraussetzung war nur, dass eine
//! Schwärzungszone das Bild berührt.
//!
//! # Was dieser Test festhält
//!
//! Der Dekoder liest zuerst den Kopf. Ein JPEG mit mehr Bildpunkten, als das
//! Dictionary angibt, gilt als nicht dekodierbar und nennt den Grund. Die
//! Gegenproben: ein JPEG mit denselben Maßen wie sein Dictionary und eines,
//! das **kleiner** ist, laufen weiter durch.

use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "redact-zp-a-{}-{name}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .output()
        .expect("Binary startet")
}

fn assemble(objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (id, body) in objects {
        offsets.push(out.len());
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn stream_obj(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Ein **gültiges** Baseline-JPEG, `w` × `h` (je ein Vielfaches von 32), ein
/// Kanal, gleichmäßig grau — derselbe Aufbau wie `jpeg_grau_64` in
/// `redact-pdf/tests/zi_b_falschalarm_bild.rs`: je Block zwei Nullbits, bei
/// einem Vielfachen von 32 also ganze Bytes ohne `FF`-Stuffing.
fn jpeg_grau(w: u16, h: u16) -> Vec<u8> {
    assert!(w.is_multiple_of(32) && h.is_multiple_of(32));
    let mut out: Vec<u8> = vec![0xFF, 0xD8];
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    out.extend(std::iter::repeat_n(0x01u8, 64));
    let [h1, h0] = h.to_be_bytes();
    let [w1, w0] = w.to_be_bytes();
    out.extend_from_slice(&[
        0xFF, 0xC0, 0x00, 0x0B, 0x08, h1, h0, w1, w0, 0x01, 0x01, 0x11, 0x00,
    ]);
    out.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x14, 0x00, 0x01]);
    out.extend(std::iter::repeat_n(0x00u8, 15));
    out.push(0x00);
    out.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01]);
    out.extend(std::iter::repeat_n(0x00u8, 15));
    out.push(0x00);
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
    let blocks = (usize::from(w) / 8) * (usize::from(h) / 8);
    out.extend(std::iter::repeat_n(0x00u8, blocks / 4));
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

/// Eine Seite mit einem IBAN-Satz über einem Bild, dessen Dictionary
/// `breite` × `hoehe` sagt und das `jpeg` enthält.
fn seite(jpeg: &[u8], breite: u32, hoehe: u32) -> Vec<u8> {
    let inhalt = b"q 400 0 0 100 60 690 cm /Im1 Do Q \
        BT /F1 12 Tf 72 700 Td (IBAN DE89 3704 0044 0532 0130 00) Tj ET";
    assemble(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R \
              /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 6 0 R >> >> >>"
                .to_vec(),
        ),
        (4, stream_obj("", inhalt)),
        (
            5,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                .to_vec(),
        ),
        (
            6,
            stream_obj(
                &format!(
                    "/Type /XObject /Subtype /Image /Width {breite} /Height {hoehe} \
                     /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode"
                ),
                jpeg,
            ),
        ),
    ])
}

fn schwaerze(name: &str, pdf: &[u8]) -> (Output, bool) {
    let dir = workdir(name);
    let datei = dir.join("ein.pdf");
    std::fs::write(&datei, pdf).unwrap();
    let aus = dir.join("aus.pdf");
    let out = run(&[
        datei.to_str().unwrap(),
        "-o",
        aus.to_str().unwrap(),
        "-f",
        "-q",
        "--max-image-mb",
        "8",
    ]);
    let geschrieben = aus.exists();
    std::fs::remove_dir_all(&dir).ok();
    (out, geschrieben)
}

/// **Befund #101 — behoben.** Bis dahin war dieser Test absichtlich rot: der
/// Lauf dekodierte das JPEG in voller Größe und endete mit Rückgabewert 0.
///
/// 4000 × 4000 im JPEG, 100 × 100 im Dictionary, Decke 8 MB: das JPEG
/// bräuchte rund 61 MB RGBA — die Decke muss greifen, mit dem Grund.
#[test]
fn ein_jpeg_groesser_als_sein_dictionary_faellt_am_kopf() {
    let (out, geschrieben) = schwaerze("gross", &seite(&jpeg_grau(4000, 4000), 100, 100));
    let fehler = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{fehler}");
    assert!(
        fehler.contains("JPEG größer als im Bild-Dictionary angegeben (4000x4000 statt 100x100)"),
        "der Grund fehlt: {fehler}"
    );
    assert!(
        !geschrieben,
        "keine Ausgabedatei bei nicht dekodierbarem Bild unter der Zone"
    );
}

/// Gegenprobe: gleiche Maße in JPEG und Dictionary — geschwärzt wie immer.
#[test]
fn ein_jpeg_mit_seinen_eigenen_massen_laeuft_durch() {
    let (out, geschrieben) = schwaerze("gleich", &seite(&jpeg_grau(64, 64), 64, 64));
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(geschrieben);
}

/// Gegenprobe: ein JPEG, das **kleiner** ist als sein Dictionary, bleibt
/// erlaubt — es belegt weniger, als die Decke reserviert hat.
#[test]
fn ein_kleineres_jpeg_laeuft_durch() {
    let (out, geschrieben) = schwaerze("klein", &seite(&jpeg_grau(64, 64), 100, 100));
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(geschrieben);
}
