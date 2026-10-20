//! Runde 2, Seitenbaum: Seiten, die ein Betrachter zeigt und `lopdf` nicht
//! aufzählt.
//!
//! `Document::get_pages` zählt nur ein Blatt, das als **Verweis** in `/Kids`
//! steht **und** ein direktes `/Type /Page` trägt. poppler, pdf.js und mupdf
//! zeigen auch ein Blatt ohne `/Type`, mit `/Type 9 0 R` oder `/Type /Pagex`,
//! und eines, das direkt in `/Kids` steht. Bis Runde 2 sah redact-rs so eine
//! Seite nicht: „Seiten: 1“, Rückgabewert 0, die IBAN von Seite 2
//! unverändert in der Ausgabe. Seitdem bringt
//! `redact_pdf::document::normalize_page_tree` den Baum beim Laden in Form.
//!
//! Zusicherung jedes Tests: **Rückgabewert 0 der Schwärzung heißt, die IBAN
//! steht nicht mehr in der Ausgabe** — gemessen mit `--check-leaks` und
//! zusätzlich über die Seitenzahl, die `lopdf` in der Ausgabe aufzählt.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

const FONT: &str = "/Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                    /Encoding /WinAnsiEncoding >> >>";

fn workdir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("r2-seitenbaum-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("Arbeitsverzeichnis");
    dir
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_redact-rs"))
        .args(args)
        .current_dir(dir)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .expect("Binary startet")
}

fn stream(payload: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", payload.len()).into_bytes();
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Setzt eine Datei aus Objekten zusammen, mit klassischer
/// Querverweistabelle.
fn assemble(objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (id, body) in objects {
        offsets.push((*id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let max = objects.iter().map(|(id, _)| *id).max().unwrap() + 1;
    out.extend_from_slice(format!("xref\n0 {max}\n0000000000 65535 f \n").as_bytes());
    for id in 1..max {
        match offsets.iter().find(|(o, _)| *o == id) {
            Some((_, at)) => out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {max} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn iban_text() -> Vec<u8> {
    format!("BT /F1 10 Tf 72 700 Td (IBAN {SECRET}) Tj ET").into_bytes()
}

/// Schwärzt `pdf` und prüft die Zusicherung. Ein Lauf mit Rückgabewert
/// ungleich 0 ist ehrlich (abgelehnt oder als Lücke gemeldet); einer mit 0
/// muss beide Seiten gesehen und die IBAN entfernt haben.
fn assert_exit0_means_clean(name: &str, pdf: Vec<u8>) {
    let dir = workdir(name);
    std::fs::write(dir.join("ein.pdf"), pdf).expect("Eingabe schreibbar");
    let run = run_in(&dir, &["ein.pdf", "-o", "aus.pdf", "-f"]);
    let summary = String::from_utf8_lossy(&run.stdout).into_owned();
    if run.status.code() != Some(0) {
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let check = run_in(&dir, &["aus.pdf", "--check-leaks", SECRET]);
    let pages = std::fs::read(dir.join("aus.pdf"))
        .ok()
        .and_then(|b| lopdf::Document::load_mem(&b).ok())
        .map(|d| d.get_pages().len());
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        check.status.code(),
        Some(0),
        "{name}: Rückgabewert 0, aber die IBAN steht in der Ausgabe.\nLauf:\n{summary}\n\
         check-leaks:\n{}",
        String::from_utf8_lossy(&check.stdout)
    );
    assert_eq!(
        pages,
        Some(2),
        "{name}: die Ausgabe zählt nicht zwei Seiten"
    );
}

/// Seitenbaum mit zwei Blättern; das zweite trägt die IBAN und statt
/// `/Type /Page` den Eintrag `p2type`.
fn two_pages(p2type: &str, extra: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let mut objects = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            format!("<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 /Resources << {FONT} >> >>")
                .into_bytes(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, stream(b"BT /F1 10 Tf 72 700 Td (Hallo) Tj ET")),
        (
            5,
            format!("<< {p2type} /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 6 0 R >>")
                .into_bytes(),
        ),
        (6, stream(&iban_text())),
    ];
    objects.extend(extra);
    assemble(&objects)
}

/// Ein Blatt ohne `/Type`, mit indirektem `/Type` und mit falschem
/// `/Type` — jedes wird als Seite gelesen und geschwärzt.
#[test]
fn blatt_ohne_brauchbares_type_wird_geschwaerzt() {
    assert_exit0_means_clean("ohne_type", two_pages("", vec![]));
    assert_exit0_means_clean(
        "type_verweis",
        two_pages("/Type 9 0 R", vec![(9, b"/Page".to_vec())]),
    );
    assert_exit0_means_clean("type_falsch", two_pages("/Type /Pagex", vec![]));
}

/// Ein Seiten-Dictionary, das **direkt** (nicht als Verweis) in `/Kids`
/// steht, wird ein eigenes Objekt und geschwärzt.
#[test]
fn blatt_direkt_in_kids_wird_geschwaerzt() {
    let objects = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R << /Type /Page /MediaBox [0 0 595 842] \
                 /Contents 6 0 R >>] /Count 2 /Resources << {FONT} >> >>"
            )
            .into_bytes(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, stream(b"BT /F1 10 Tf 72 700 Td (Hallo) Tj ET")),
        (6, stream(&iban_text())),
    ];
    assert_exit0_means_clean("direkt_in_kids", assemble(&objects));
}

/// Ein Zwischenknoten ohne `/Type` (mit `/Kids`) wird als `/Pages` gelesen;
/// sein Blatt ist eine Seite wie jede andere.
#[test]
fn zwischenknoten_ohne_type_traegt_seine_blaetter() {
    let objects = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            format!("<< /Type /Pages /Kids [3 0 R 7 0 R] /Count 2 /Resources << {FONT} >> >>")
                .into_bytes(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, stream(b"BT /F1 10 Tf 72 700 Td (Hallo) Tj ET")),
        (
            5,
            b"<< /Type /Page /Parent 7 0 R /MediaBox [0 0 595 842] /Contents 6 0 R >>".to_vec(),
        ),
        (6, stream(&iban_text())),
        (7, b"<< /Parent 2 0 R /Kids [5 0 R] /Count 1 >>".to_vec()),
    ];
    assert_exit0_means_clean("knoten_ohne_type", assemble(&objects));
}
