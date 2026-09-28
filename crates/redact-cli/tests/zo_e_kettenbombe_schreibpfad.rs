//! Gegenprüfung Spur A, Runde 1, Gebiet E (Decken und Dienstverweigerung):
//! die Entpackgrenze `--max-decompressed-mb` gilt **nicht** auf dem
//! Schreibpfad, sobald eine Filterkette einen Filter enthält, den die
//! Vorprüfung nicht auspackt.
//!
//! # Der Befund (Dienstverweigerung, neue Klasse)
//!
//! Die Vorprüfung [`redact_pdf::document::prescan`] bucht einen Stream, dessen
//! Filterkette **irgendein** nicht selbst auspackbares Glied enthält
//! (`RunLengthDecode`, Bildfilter, Unbekanntes), nur mit seiner **rohen**
//! Größe und packt ihn gar nicht erst aus (`document.rs`, `Prescan::account`,
//! Zweig `if !decodable`). Das ist für die *Tiefen*prüfung richtig — aus einem
//! solchen Strom wird keine verschachtelte PDF-Syntax. Für die *Entpackgrenze*
//! ist es ein Loch: legt man den teuren Filter **hinter** einen, den die
//! Vorprüfung auspackt, wird die ganze Kette roh gebucht, und die rohe Größe
//! lässt sich durch das äußere `FlateDecode` beliebig klein drücken.
//!
//! `/Filter [/FlateDecode /RunLengthDecode]` über einem RunLength-Strom, der
//! sich auf ~2,4 GB aufbläst, steht als **39 KB** in der Datei. Die
//! Vorprüfung sieht 39 KB und lässt durch. Der **Schreibpfad**
//! (`redact::rewrite_page` → `filters::page_content` → `decoded_content`)
//! entpackt die Kette dann mit `usize::MAX` als Grenze — der Dokumentkommentar
//! dort sagt es ausdrücklich: „Ohne Grenze gibt es kein `Oversize`.“ Aus 39 KB
//! werden im Speicher über 2 GB, ganz ohne Rücksicht auf `--max-decompressed-mb`
//! (Vorgabe 1024 MB).
//!
//! Zum Vergleich: dieselbe Aufblähung als **reine** `/FlateDecode`-Bombe wird
//! von der Vorprüfung sauber mit Rückgabewert 1 und „entpackte Streams
//! überschreiten das Budget von 1024 MB“ abgelehnt, und das ehrliche Orakel
//! (`--check-leaks`) meldet die Kettenbombe als `NICHT GEPRÜFT` (Rückgabewert
//! 3) — es hält seine eigene Grenze ein. Nur der Schreibpfad, der die Datei
//! wirklich verarbeitet, tut es nicht.
//!
//! Gemessen am gebauten Binary (`redact-cli`, `--no-default-features`), Datei
//! 39 381 Byte:
//!
//! ```text
//! $ /usr/bin/time -v redact-rs chain.pdf -o out.pdf -f -q   (kein ulimit)
//!   Elapsed 0:28.89, Maximum resident set size 4 007 668 KB  (≈ 3,8 GB)  rc 0
//! $ bash -c 'ulimit -v 1572864; exec redact-rs chain.pdf -o out.pdf -f -q'
//!   memory allocation of 2147483648 bytes failed → SIGABRT, rc 134
//! ```
//!
//! Eine Datei unter 1 MB treibt den Prozess also über 2 GB Arbeitsspeicher
//! (Mandatsfrage 1: ja) — und unter einer knappen Adressraumgrenze stirbt er
//! mit Signal, statt die Datei mit einer Budgetmeldung abzulehnen.
//!
//! # Was dieser Test festhält
//!
//! Er war **absichtlich rot** und ist seit Register #64 grün: die Vorprüfung
//! packt jetzt jede Kette aus, deren Glieder `filters.rs` begrenzt entpacken
//! kann (`RunLengthDecode` und `ASCIIHexDecode` eingeschlossen), Glied für
//! Glied gegen das Budget — die Kettenbombe fällt dort, bevor aus ihr
//! Speicher wird. Er verlangt das Verhalten, das die reine
//! Flate-Bombe schon zeigt: unter einer Adressraumgrenze, die ein
//! gewöhnliches Dokument nie erreicht, endet der Lauf **geordnet** — mit einem
//! Rückgabewert (nicht mit einem Signal) und mit der Budgetmeldung, weil die
//! entpackte Kette das 1024-MB-Budget sprengt. Solange der Schreibpfad die
//! Kette mit `usize::MAX` entpackt, wird der Prozess stattdessen vom
//! Speicherlimit getötet, `status.code()` ist `None`, und der Test fällt.
//!
//! Vermutete Ursache: `redact-pdf/src/filters.rs`, `decoded_content` ruft
//! `decoded_content_within(doc, stream, usize::MAX)` — der Schreibpfad
//! (`redact.rs:814`, `filters::page_content`) trägt keine Grenze, während das
//! Orakel (`audit_bytes.rs`) `decoded_content_within` mit dem echten Budget
//! aufruft. Und `document.rs`, `Prescan::account`, bucht eine nicht ganz
//! auspackbare Kette (`if !decodable`) nur roh.
//!
//! Linux, weil der Nachweis eine Adressraumgrenze über `ulimit -v` setzt.

#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::{Command, Output};

use lopdf::{dictionary, Stream};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "redact-zo-e-{}-{name}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

/// Der Seiteninhalt als `/Filter [/FlateDecode /RunLengthDecode]`: innen ein
/// RunLength-Strom, der sich auf `reps * 128` Byte aufbläst, außen mit Flate
/// klein gedrückt. `reps = 20_000_000` → ~2,4 GB entpackt, ~39 KB in der Datei.
fn pdf_mit_ketten_bombe(reps: usize) -> Vec<u8> {
    // RunLength: 129 heißt „das nächste Byte 128-mal“. 0x80 beendet den Strom.
    let mut runlength = Vec::with_capacity(reps * 2 + 1);
    for _ in 0..reps {
        runlength.push(129u8);
        runlength.push(b' ');
    }
    runlength.push(0x80);

    // Über lopdf mit Flate komprimieren — kein zusätzliches Crate nötig.
    let mut flate = Stream::new(dictionary! {}, runlength);
    flate.compress().expect("komprimierbar");
    let packed = flate.content;

    let mut content = format!(
        "<< /Length {} /Filter [/FlateDecode /RunLengthDecode] >>\nstream\n",
        packed.len()
    )
    .into_bytes();
    content.extend_from_slice(&packed);
    content.extend_from_slice(b"\nendstream");

    assemble(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R \
              /Resources << >> >>"
                .to_vec(),
        ),
        (4, content),
    ])
}

/// Führt das Binary unter einer Adressraumgrenze (`ulimit -v`, in KiB) aus.
/// `None`, wenn `bash` nicht startbar ist — dann fehlt die Messung, nicht
/// der Test (Plattformregel aus `zf_q5_plattformzusagen`).
fn run_with_address_limit(kib: u64, args: &[&str]) -> Option<Output> {
    let quoted: Vec<String> = args.iter().map(|a| format!("'{a}'")).collect();
    let line = format!("ulimit -v {kib}; exec '{}' {}", bin(), quoted.join(" "));
    Command::new("bash")
        .arg("-c")
        .arg(line)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .output()
        .ok()
}

/// **Befund E-1 — behoben (Register #64).** Bis dahin war dieser Test
/// absichtlich rot.
///
/// Eine 39-KB-Datei, deren Seiteninhalt sich beim Entpacken auf ~2,4 GB
/// aufbläst, muss der Schreibpfad **geordnet** ablehnen — das entpackte
/// Volumen sprengt das 1024-MB-Budget. Die Adressraumgrenze (1,5 GB) liegt
/// weit über allem, was ein gewöhnliches Dokument braucht (ein 500-seitiger
/// Auszug läuft in ~48 MB), aber unter der Bombe.
#[test]
fn die_entpackgrenze_gilt_auch_auf_dem_schreibpfad() {
    let dir = workdir("kette");
    let datei = "kette.pdf";
    let bytes = pdf_mit_ketten_bombe(20_000_000);
    assert!(
        bytes.len() < 1024 * 1024,
        "die Bombe soll unter 1 MB bleiben, ist aber {} Byte",
        bytes.len()
    );
    std::fs::write(dir.join(datei), &bytes).unwrap();
    let aus = dir.join("aus.pdf");

    let Some(out) = run_with_address_limit(
        1_572_864, // 1,5 GiB Adressraum
        &[
            dir.join(datei).to_str().unwrap(),
            "-o",
            aus.to_str().unwrap(),
            "-f",
            "-q",
        ],
    ) else {
        eprintln!("übergangen: bash ist auf diesem Rechner nicht startbar");
        return;
    };
    let stderr = String::from_utf8_lossy(&out.stderr);

    // Der Kern des Befundes: der Lauf endet mit einem Rückgabewert, nicht mit
    // einem Signal. Heute wird der Prozess vom Speicherlimit getötet
    // (SIGABRT, „memory allocation of 2147483648 bytes failed“), also ist
    // `code()` None.
    assert!(
        out.status.code().is_some(),
        "der Schreibpfad wurde vom Speicherlimit getötet, statt die Bombe \
         geordnet abzulehnen — die Entpackgrenze --max-decompressed-mb gilt \
         hier nicht (stderr: {stderr})"
    );

    // Und die Ablehnung ist die des Budgets, wie bei der reinen Flate-Bombe.
    assert_eq!(
        out.status.code(),
        Some(1),
        "erwartet: Rückgabewert 1 wegen überschrittenem Entpackbudget \
         (stderr: {stderr})"
    );
    assert!(
        stderr.contains("Budget") && stderr.contains("entpackt"),
        "die Ablehnung nennt das Entpackbudget nicht (stderr: {stderr})"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
