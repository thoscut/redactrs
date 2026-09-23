//! Spur A, Runde 1, Nachtrag zu Register #64 (Register #83): die Entpackgrenze
//! galt nicht für den **Vorspann** einer Kette, die an einem Bildfilter oder
//! an einem unbekannten Filter endet.
//!
//! # Der Befund (Dienstverweigerung)
//!
//! Die Vorprüfung ([`redact_pdf::document::prescan`]) packte seit #64 jede
//! Kette aus, deren Glieder `filters.rs` **alle** begrenzt entpacken kann —
//! und buchte jede andere weiter ganz roh. `/Filter [/FlateDecode
//! /DCTDecode]` ist eine gewöhnliche Kette (ein JPEG, noch einmal mit Flate
//! gepackt); steht im Flate-Glied statt eines JPEG eine Bombe, sieht die
//! Vorprüfung nur die gepackten Bytes. Der Bilddekoder des Schreibpfads
//! (`ops.rs`, `apply_filters`) entpackt das Flate-Glied dann ohne Grenze, um
//! an die JPEG-Bytes zu kommen, und scheitert erst danach am JPEG.
//!
//! Gemessen am gebauten Binary (Debug, Stand `72711d0`, `/usr/bin/time -v`,
//! `--max-decompressed-mb 64`): eine Datei von 1 044 556 Byte mit 1 GiB
//! Nullen im Flate-Glied erreicht 1 138 680 KB Spitze, eine von 3 131 802
//! Byte mit 3 GiB erreicht 3 241 844 KB in 28,7 s. Beide enden mit
//! Rückgabewert 1 und der falschen Ursache „JPEG nicht dekodierbar“ — das
//! Budget hat nie gegriffen.
//!
//! # Was dieser Test festhält
//!
//! Die Vorprüfung packt den auspackbaren Vorspann jeder Kette begrenzt aus
//! und bucht ihn; die Bombe fällt dort mit der Budgetmeldung, bevor aus ihr
//! Speicher wird. Die Gegenproben: ein echtes JPEG hinter Flate wird weiter
//! geschwärzt, und die Rohgrößen-Grenze der Altlast-Filter greift nicht auf
//! eine Kette wie `[/ASCII85Decode /DCTDecode]` über, die vorher durchlief.

use std::path::PathBuf;
use std::process::{Command, Output};

use lopdf::{dictionary, Stream};
use redact_pdf::document::{prescan, Limits};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "redact-zo-e-vorspann-{}-{name}-{:?}",
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

/// Baut eine PDF-Datei aus fertigen Objektrümpfen, mit Querverweistabelle.
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

fn flate(data: Vec<u8>) -> Vec<u8> {
    let mut s = Stream::new(dictionary! {}, data);
    s.compress().expect("komprimierbar");
    s.content
}

/// Eine Seite mit einem Kontoinhaber-Satz und einem Bild darüber, dessen
/// Strom `/Filter {kette}` trägt und die Bytes `bild` enthält.
fn seite_mit_bild(kette: &str, bild: &[u8], breite: u32, hoehe: u32) -> Vec<u8> {
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
                     /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter {kette}"
                ),
                bild,
            ),
        ),
    ])
}

/// Ein **gültiges** Baseline-JPEG, 64 × 64, ein Kanal, gleichmäßig grau —
/// derselbe Aufbau wie in `redact-pdf/tests/zi_b_falschalarm_bild.rs`.
fn jpeg_grau_64() -> Vec<u8> {
    let mut out: Vec<u8> = vec![0xFF, 0xD8];
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    out.extend(std::iter::repeat_n(0x01u8, 64));
    out.extend_from_slice(&[
        0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x40, 0x00, 0x40, 0x01, 0x01, 0x11, 0x00,
    ]);
    out.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x14, 0x00, 0x01]);
    out.extend(std::iter::repeat_n(0x00u8, 15));
    out.push(0x00);
    out.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01]);
    out.extend(std::iter::repeat_n(0x00u8, 15));
    out.push(0x00);
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
    out.extend(std::iter::repeat_n(0x00u8, 16));
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

/// **Befund #83 — behoben.** Bis dahin war dieser Test absichtlich rot: der
/// Lauf endete mit Rückgabewert 1 und „JPEG nicht dekodierbar“, nachdem er
/// die ganze Bombe entpackt hatte.
///
/// 256 MiB Nullen im Flate-Glied vor `/DCTDecode`, Budget 16 MB: die
/// Vorprüfung muss die Datei mit der Budgetmeldung ablehnen.
#[test]
fn ein_flate_vorspann_vor_dctdecode_faellt_am_budget() {
    let dir = workdir("bombe");
    let bombe = flate(vec![0u8; 256 * 1024 * 1024]);
    let pdf = seite_mit_bild("[/FlateDecode /DCTDecode]", &bombe, 400, 100);
    assert!(pdf.len() < 1024 * 1024, "{} Byte", pdf.len());
    let datei = dir.join("bombe.pdf");
    std::fs::write(&datei, &pdf).unwrap();
    let aus = dir.join("aus.pdf");

    let out = run(&[
        datei.to_str().unwrap(),
        "-o",
        aus.to_str().unwrap(),
        "-f",
        "-q",
        "--max-decompressed-mb",
        "16",
    ]);
    let fehler = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{fehler}");
    assert!(
        fehler.contains("überschreiten das Budget von 16 MB"),
        "die Bombe muss am Budget fallen, nicht erst am JPEG: {fehler}"
    );
    assert!(!aus.exists(), "keine Ausgabedatei bei abgelehnter Eingabe");
    std::fs::remove_dir_all(&dir).ok();
}

/// Gegenprobe: ein echtes JPEG hinter Flate — die Kette, die Erzeuger
/// wirklich schreiben — wird weiter gelesen und geschwärzt.
#[test]
fn ein_echtes_jpeg_hinter_flate_bleibt_durchlaessig() {
    let dir = workdir("echt");
    let pdf = seite_mit_bild("[/FlateDecode /DCTDecode]", &flate(jpeg_grau_64()), 64, 64);
    let datei = dir.join("echt.pdf");
    std::fs::write(&datei, &pdf).unwrap();
    let aus = dir.join("aus.pdf");

    let out = run(&[
        datei.to_str().unwrap(),
        "-o",
        aus.to_str().unwrap(),
        "-f",
        "-q",
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(aus.exists(), "die Ausgabedatei fehlt");
    std::fs::remove_dir_all(&dir).ok();
}

/// ASCII85, vier Byte zu fünf Zeichen (PDF 32000-1, 7.4.3), ohne `z`.
fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 4 * 5 + 7);
    for chunk in data.chunks(4) {
        let mut group = [0u8; 4];
        group[..chunk.len()].copy_from_slice(chunk);
        let mut n = u32::from_be_bytes(group);
        let mut digits = [0u8; 5];
        for d in digits.iter_mut().rev() {
            *d = (n % 85) as u8 + b'!';
            n /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// Gegenprobe zur Rohgrößen-Grenze: `[/ASCII85Decode /DCTDecode]` über mehr
/// als 16 MiB ASCII85 (so schreiben ältere Distiller-Fassungen große Scans)
/// lief vorher roh durch. Dass die Vorprüfung den Vorspann jetzt auspackt,
/// durfte die damalige Grenze für ganze Altlast-Ketten nicht auf sie
/// übertragen — die Datei wäre sonst eine abgelehnte gewöhnliche Datei
/// geworden. Seit Register #82 gibt es die Grenze gar nicht mehr; die Probe
/// bleibt und hält fest, dass ein solcher Vorspann durchläuft.
#[test]
fn ein_ascii85_vorspann_ueber_16_mib_faellt_nicht_an_der_altlast_grenze() {
    let mut rauschen = Vec::with_capacity(14 * 1024 * 1024);
    let mut x: u32 = 0x9E37_79B9;
    while rauschen.len() < 14 * 1024 * 1024 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        rauschen.extend_from_slice(&x.to_le_bytes());
    }
    let kodiert = ascii85(&rauschen);
    assert!(kodiert.len() > 16 * 1024 * 1024, "{} Byte", kodiert.len());
    let pdf = seite_mit_bild("[/ASCII85Decode /DCTDecode]", &kodiert, 400, 100);

    prescan(&pdf, &Limits::default()).unwrap_or_else(|e| {
        panic!("die Vorprüfung lehnt eine Kette ab, die vorher durchlief: {e}")
    });
}
