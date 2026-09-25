//! Spur A, Runde 2, Prüfer E (Register #89): die Vorprüfung war keine
//! Schranke für die Leser nach ihr.
//!
//! # Der Befund (Dienstverweigerung)
//!
//! Die Vorprüfung ([`redact_pdf::document::prescan`]) bucht jeden Strom so,
//! wie sie ihn aus den Rohbytes liest. Sie las anders als `lopdf` und der
//! Schreibpfad — und wo sie weniger las, galt `--max-decompressed-mb` nicht:
//!
//! * **`/Filter` als Verweis** (`/Filter 5 0 R`, `/Filter [5 0 R]`): aus den
//!   Rohbytes nicht aufzulösen, also roh gebucht; der Schreibpfad löst auf
//!   und entpackt ohne Grenze.
//! * **Rohes Deflate hinter zwei beliebigen Bytes**, und ein zlib-Strom mit
//!   kaputtem Ende: die Vorprüfung las nur zlib und gab beim ersten Fehler
//!   auf; `lopdf` und der Schreibpfad lesen das Teilergebnis und fallen auf
//!   rohes Deflate zurück.
//! * **Der Schlüssel `/Filter` anders geschrieben**: mit `#xx` im Namen, als
//!   zweiter Eintrag, hinter einem `/Filter` in einem inneren Dictionary. Die
//!   Vorprüfung suchte die ersten Bytes `/Filter`, `lopdf` liest das
//!   Dictionary.
//!
//! # Was dieser Test festhält
//!
//! Jede dieser Bomben fällt mit der Budgetmeldung, bevor aus ihr Speicher
//! wird. Die Gegenprobe: eine gewöhnliche Datei mit `/Filter` als Verweis
//! wird weiter geschwärzt.

use std::path::PathBuf;
use std::process::{Command, Output};

use lopdf::{dictionary, Stream};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "redact-zp-e-schranke-{}-{name}-{:?}",
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

fn zlib(data: Vec<u8>) -> Vec<u8> {
    let mut s = Stream::new(dictionary! {}, data);
    s.compress().expect("komprimierbar");
    s.content
}

/// 256 MiB Nullen, mit zlib gepackt — rund 256 KB in der Datei.
fn bombe() -> Vec<u8> {
    zlib(vec![0u8; 256 * 1024 * 1024])
}

/// Eine Seite, deren Inhaltsstrom das Dictionary `kopf` (ohne `/Length`)
/// und die Bytes `daten` trägt; Objekt 6 ist der Name `/FlateDecode`, für
/// Ketten mit Verweis.
fn seite(kopf: &str, daten: &[u8]) -> Vec<u8> {
    let mut inhalt = format!("<< {kopf} /Length {} >>\nstream\n", daten.len()).into_bytes();
    inhalt.extend_from_slice(daten);
    inhalt.extend_from_slice(b"\nendstream");
    assemble(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R \
              /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_vec(),
        ),
        (4, inhalt),
        (
            5,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                .to_vec(),
        ),
        (6, b"/FlateDecode".to_vec()),
    ])
}

/// Schwärzt `pdf` mit einem Budget von 16 MB und verlangt die Budgetmeldung.
fn faellt_am_budget(name: &str, pdf: &[u8]) {
    assert!(pdf.len() < 1024 * 1024, "{name}: {} Byte", pdf.len());
    let dir = workdir(name);
    let datei = dir.join("bombe.pdf");
    std::fs::write(&datei, pdf).unwrap();
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
    assert_eq!(out.status.code(), Some(1), "{name}: {fehler}");
    assert!(
        fehler.contains("überschreiten das Budget von 16 MB"),
        "{name}: die Bombe muss am Budget fallen: {fehler}"
    );
    assert!(
        !aus.exists(),
        "{name}: keine Ausgabedatei bei abgelehnter Eingabe"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn filter_als_verweis_faellt_am_budget() {
    faellt_am_budget("verweis", &seite("/Filter 6 0 R", &bombe()));
}

#[test]
fn filter_liste_mit_verweis_faellt_am_budget() {
    faellt_am_budget("liste", &seite("/Filter [6 0 R]", &bombe()));
}

/// Der zlib-Kopf durch zwei Nullbytes ersetzt: zlib liefert nichts, der
/// Rückfall liest das rohe Deflate dahinter.
#[test]
fn rohes_deflate_hinter_zwei_bytes_faellt_am_budget() {
    let mut daten = bombe();
    daten[0] = 0;
    daten[1] = 0;
    faellt_am_budget("roh", &seite("/Filter /FlateDecode", &daten));
}

/// Eine Seite aus vielen Inhaltsströmen, jeder mit `daten`.
fn seite_aus_stroemen(anzahl: u32, daten: &[u8]) -> Vec<u8> {
    let inhalte: Vec<String> = (0..anzahl).map(|i| format!("{} 0 R", 10 + i)).collect();
    let mut objekte = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents [{}] >>",
                inhalte.join(" ")
            )
            .into_bytes(),
        ),
    ];
    for i in 0..anzahl {
        let mut strom = format!(
            "<< /Filter /FlateDecode /Length {} >>\nstream\n",
            daten.len()
        )
        .into_bytes();
        strom.extend_from_slice(daten);
        strom.extend_from_slice(b"\nendstream");
        objekte.push((10 + i, strom));
    }
    // `assemble` nummeriert die Querverweise fortlaufend: die Lücke 4 bis 9
    // mit leeren Objekten schließen.
    for id in 4..10 {
        objekte.push((id, b"null".to_vec()));
    }
    objekte.sort_by_key(|(id, _)| *id);
    assemble(&objekte)
}

/// Die Prüfsumme am Ende kaputt: zlib liefert alles und meldet erst dann
/// den Fehler; das Teilergebnis zählt. Ein einzelner Strom über dem Budget
/// fiel schon vorher — die Grenze schneidet ihn ab, bevor die Prüfsumme
/// gelesen wird. Offen war die Summe: viele Ströme, jeder unter dem Budget.
#[test]
fn zlib_mit_kaputtem_ende_zaehlt_mit_dem_teilergebnis() {
    let mut daten = zlib(vec![0u8; 1024 * 1024]);
    let n = daten.len();
    for b in &mut daten[n - 4..] {
        *b ^= 0xFF;
    }
    faellt_am_budget("ende", &seite_aus_stroemen(24, &daten));
}

#[test]
fn der_schluessel_mit_hexziffern_faellt_am_budget() {
    faellt_am_budget("hex", &seite("/Fil#74er /FlateDecode", &bombe()));
    faellt_am_budget("hexname", &seite("/Filter /Flate#44ecode", &bombe()));
}

#[test]
fn ein_filter_im_inneren_dictionary_taeuscht_nicht() {
    faellt_am_budget(
        "innen",
        &seite(
            "/X << /Filter /ASCIIHexDecode >> /Filter /FlateDecode",
            &bombe(),
        ),
    );
    faellt_am_budget(
        "doppelt",
        &seite("/Filter /ASCIIHexDecode /Filter /FlateDecode", &bombe()),
    );
}

/// Gegenprobe: eine gewöhnliche Seite mit `/Filter` als Verweis wird
/// gelesen und geschwärzt.
#[test]
fn eine_gewoehnliche_kette_mit_verweis_bleibt_durchlaessig() {
    let dir = workdir("gewoehnlich");
    // Lang genug, dass `Stream::compress` wirklich packt — einen kurzen
    // Strom ließe es ungepackt, und unter `/Filter` stünden Rohbytes.
    let mut text = b"BT /F1 12 Tf 72 700 Td (IBAN DE89 3704 0044 0532 0130 00) Tj ET\n".to_vec();
    for zeile in 0..40 {
        text.extend_from_slice(
            format!(
                "BT /F1 10 Tf 72 {} Td (Kontoinhaber Max Mustermann) Tj ET\n",
                650 - 12 * zeile
            )
            .as_bytes(),
        );
    }
    let gepackt = zlib(text.clone());
    assert!(gepackt.len() < text.len(), "der Inhalt ist nicht gepackt");
    let pdf = seite("/Filter [6 0 R]", &gepackt);
    let datei = dir.join("gewoehnlich.pdf");
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
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ausgabe = std::fs::read(&aus).expect("die Ausgabedatei fehlt");
    let found = redact_pdf::leaks(&ausgabe, "DE89 3704 0044 0532 0130 00");
    assert!(
        found.is_empty(),
        "der Inhalt wurde nicht gelesen: {found:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
