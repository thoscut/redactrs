//! Erkennung am gebauten Binary: zwei Schreibweisen, an denen die Analyse
//! vorbeisah, obwohl jeder Betrachter den Wert zeigt und kopiert.
//!
//! * **K1 — geschütztes Leerzeichen hinter dem Schlüsselwort.**
//!   `Steuer-ID:` U+00A0 `12345678903`: das Schlüsselwort griff nicht, der
//!   Treffer fiel auf `confidence_without_context` (0.25) und unter die
//!   Schwelle. Gemessen vor der Korrektur: `Treffer gesamt: 0`,
//!   Rückgabewert 0, alle drei Werte in der Ausgabe.
//! * **K2 — Ligatur als Codepunkt.** Bildet `/ToUnicode` einen Code auf
//!   U+FB01 („ﬁ“) ab, stand `Identiﬁkationsnummer` in der Zeile und
//!   `Max ﬁscher` statt `Max Fischer`. Weder das Schlüsselwort der Steuer-ID
//!   noch der Buchungslisten-Eintrag trafen: `Treffer gesamt: 0`,
//!   Rückgabewert 0.
//!
//! Orakel ist der Text der **Ausgabedatei**, gelesen mit dem Extraktor —
//! nach der Korrektur also mit Ligatur-Zerlegung. Dass der Extraktor die
//! Ligatur überhaupt als Text liest, belegt die Gegenprobe
//! `die_ligatur_steht_vor_dem_lauf_im_text`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("zq-c-erkennung-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .current_dir(dir)
        .env("REDACT_RS_CONFIG", "")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .expect("Binary startbar")
}

fn stream(payload: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", payload.len()).into_bytes();
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Einseitiges PDF in Helvetica/WinAnsi. Die `/ToUnicode`-Tabelle bildet
/// ASCII auf sich selbst ab und den Code `0xA0` auf `special` (UTF-16BE in
/// Hex, etwa `00A0` oder `FB01`). In `lines` steht `~` für den Code `0xA0`.
fn pdf(lines: &[&str], special: &str) -> Vec<u8> {
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def /CMapType 2 def\n\
         1 begincodespacerange <00> <FF> endcodespacerange\n\
         1 beginbfrange <20> <7E> <0020> endbfrange\n\
         1 beginbfchar <A0> <{special}> endbfchar\n\
         endcmap CMapName currentdict /CMap defineresource pop end end\n"
    );
    let mut content = b"BT /F1 11 Tf\n".to_vec();
    for (i, line) in lines.iter().enumerate() {
        content.extend_from_slice(format!("1 0 0 1 50 {} Tm (", 780 - 20 * i).as_bytes());
        for b in line.bytes() {
            match b {
                b'~' => content.push(0xA0),
                b'(' | b')' | b'\\' => content.extend_from_slice(&[b'\\', b]),
                b => content.push(b),
            }
        }
        content.extend_from_slice(b") Tj\n");
    }
    content.extend_from_slice(b"ET");
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
          /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
            .to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
          /Encoding /WinAnsiEncoding /ToUnicode 6 0 R >>"
            .to_vec(),
        stream(&content),
        stream(cmap.as_bytes()),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for at in offsets {
        out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

/// Der Text einer Datei, Zeile für Zeile, so wie die Analyse ihn liest.
fn text_of(path: &Path) -> Vec<String> {
    let doc = lopdf::Document::load(path).expect("PDF lesbar");
    redact_pdf::PdfExtractor::new()
        .extract(&doc)
        .expect("Text lesbar")
        .into_iter()
        .map(|run| run.text)
        .collect()
}

/// Schwärzt `in.pdf` in `dir`; liefert Rückgabewert, Zusammenfassung und den
/// Text der Ausgabe.
fn redact(dir: &Path, input: &[u8], extra: &[&str]) -> (i32, String, Vec<String>) {
    std::fs::write(dir.join("in.pdf"), input).unwrap();
    let mut args = vec!["in.pdf", "-o", "out.pdf", "-f"];
    args.extend_from_slice(extra);
    let run = run_in(dir, &args);
    let summary = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let text = text_of(&dir.join("out.pdf"));
    (run.status.code().unwrap(), summary, text)
}

#[test]
fn k1_geschuetztes_leerzeichen_hinter_dem_schluesselwort() {
    let dir = workdir("nbsp");
    let input = pdf(
        &[
            "Steuer-ID:~12345678903",
            "BLZ:~37040044",
            "Kto.-Nr.:~0532013000",
        ],
        "00A0",
    );
    let (code, summary, text) = redact(&dir, &input, &[]);
    assert_eq!(code, 0, "{summary}");
    assert!(summary.contains("Schwärzungen:       3"), "{summary}");
    let all = text.join("\n");
    for secret in ["12345678903", "37040044", "0532013000"] {
        assert!(
            !all.contains(secret),
            "{secret} steht in der Ausgabe:\n{all}"
        );
    }
    // Die Schlüsselwörter bleiben lesbar.
    assert!(all.contains("Steuer-ID:"), "{all}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn die_ligatur_steht_vor_dem_lauf_im_text() {
    let dir = workdir("lig-vorher");
    std::fs::write(
        dir.join("in.pdf"),
        pdf(&["Kontoinhaber: Max ~scher"], "FB01"),
    )
    .unwrap();
    let text = text_of(&dir.join("in.pdf")).join("\n");
    assert!(text.contains("Max fischer"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn k2_ligatur_als_codepunkt() {
    let dir = workdir("lig");
    std::fs::write(
        dir.join("liste.csv"),
        "id,list_type,pattern\nb1,positive,Max Fischer\n",
    )
    .unwrap();
    let input = pdf(
        &[
            "Identi~kationsnummer: 12345678903",
            "Kontoinhaber: Max ~scher",
        ],
        "FB01",
    );
    let (code, summary, text) = redact(&dir, &input, &["--booking-list", "liste.csv"]);
    assert_eq!(code, 0, "{summary}");
    assert!(summary.contains("Schwärzungen:       2"), "{summary}");
    let all = text.join("\n");
    assert!(
        !all.contains("12345678903"),
        "Steuer-ID in der Ausgabe:\n{all}"
    );
    // Die Ligatur ist **ein** Code: getroffen wird sie ganz, nicht halb.
    assert!(!all.contains("scher"), "Name in der Ausgabe:\n{all}");
    assert!(!all.contains('\u{FB01}') && !all.contains("Max f"), "{all}");
    assert!(all.contains("Identifikationsnummer:"), "{all}");
    let _ = std::fs::remove_dir_all(&dir);
}
