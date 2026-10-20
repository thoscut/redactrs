//! Lesarten: was ein Betrachter (poppler, pdf.js, mupdf) als **eine** Kennung
//! zeigt, muss redact-rs auch als eine lesen — oder ehrlich sagen, dass es
//! das nicht kann.
//!
//! Zusicherung, die jeder Fall prüft: **Rückgabewert 0 des Schwärzungslaufs
//! heißt, die IBAN steht nicht mehr in der Ausgabe.** Orakel ist
//! `--check-leaks` am Binary über der Ausgabedatei. Die Fälle stammen aus der
//! Prüfrunde 2 („Parser-Differenzen“); jeder wurde mit `pdftotext`
//! (poppler 24), pdf.js 4.10 und PyMuPDF gegen die Eingabe geprüft: der
//! Betrachter zeigt die IBAN.
//!
//! * Unsichtbare Zeichen in der Kennung (Steuerzeichen, Breite null, weicher
//!   Trennstrich) — `redact_pdf::extract` nimmt sie aus der Zeile.
//! * `/Contents`-Teilströme, deren Grenze in einer Zeichenkette oder in einem
//!   Wort liegt — `redact_pdf::filters::page_content_checked`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("r2-lesarten-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .current_dir(dir)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .expect("Binary startbar")
}

const FONT: &str = "/Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                    /Encoding /WinAnsiEncoding >> >>";

fn stream(payload: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", payload.len()).into_bytes();
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\nendstream");
    out
}

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

/// Eine Seite mit `/Contents {contents}` und den Fonts aus `font`.
fn one_page_with(font: &str, contents: &str, streams: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let mut objects = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents {contents} \
                 /Resources << {font} >> >>"
            )
            .into_bytes(),
        ),
    ];
    objects.extend(streams);
    assemble(&objects)
}

fn one_page(contents: &str, streams: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    one_page_with(FONT, contents, streams)
}

/// Rückgabewert und Zusammenfassung des Schwärzungslaufs über `pdf`.
fn redact(dir: &Path, pdf: &[u8]) -> (i32, String) {
    std::fs::write(dir.join("in.pdf"), pdf).unwrap();
    let run = run_in(dir, &["in.pdf", "-o", "out.pdf", "-f"]);
    let summary = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    (run.status.code().unwrap(), summary)
}

/// Rückgabewert von `--check-leaks` über `file` in `dir`.
fn leaks(dir: &Path, file: &str) -> (i32, String) {
    let check = run_in(dir, &[file, "--check-leaks", SECRET]);
    (
        check.status.code().unwrap(),
        String::from_utf8_lossy(&check.stdout).into_owned(),
    )
}

/// Der Lauf muss die IBAN **finden und schwärzen**: Rückgabewert 0, und die
/// Ausgabe ist sauber. Schärfer als „0 ⇒ sauber“ — eine Ablehnung wäre hier
/// zwar ehrlich, aber unnötig.
fn assert_redacted(name: &str, pdf: Vec<u8>) {
    let dir = workdir(name);
    let (code, summary) = redact(&dir, &pdf);
    assert_eq!(code, 0, "{name}: Lauf endet nicht mit 0:\n{summary}");
    assert!(
        !summary.contains("Treffer gesamt:     0"),
        "{name}: keine Treffer:\n{summary}"
    );
    let (check, report) = leaks(&dir, "out.pdf");
    assert_eq!(
        check, 0,
        "{name}: IBAN steht in der Ausgabe.\nLauf:\n{summary}\ncheck-leaks:\n{report}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Ein C0-Steuerzeichen (`\t`, `\000`, `\n`, `\037`) oder `DEL` mitten in
/// der IBAN. poppler und pdf.js zeigen
/// und kopieren `DE89370400440532013000` am Stück (gerendert geprüft: Lücke
/// ≈ 2,7 pt). Vorher stand an der Stelle ein Ersatzzeichen in der Zeile,
/// kein Muster griff — Treffer 0, Rückgabewert 0, IBAN in der Ausgabe.
#[test]
fn steuerzeichen_in_der_iban() {
    for (name, ch) in [
        ("tab", "\\t"),
        ("nul", "\\000"),
        ("nl", "\\n"),
        ("us", "\\037"),
        ("del", "\\177"),
    ] {
        let content = format!("BT /F1 10 Tf 72 700 Td (DE8937040044053{ch}2013000) Tj ET");
        assert_redacted(
            &format!("steuer_{name}"),
            one_page("4 0 R", vec![(4, stream(content.as_bytes()))]),
        );
    }
}

/// Dasselbe über eine `/ToUnicode`-Tabelle: der Code 1 liefert ein
/// Breite-null-Zeichen (U+200B), der Code 2 einen Tabulator, der Code 3 einen
/// weichen Trennstrich (U+00AD). Der Betrachter
/// kopiert die IBAN mit einem unsichtbaren Zeichen darin; die Zeile muss sie
/// am Stück tragen.
#[test]
fn unsichtbares_zeichen_aus_tounicode() {
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
        /CMapName /R2 def 1 begincodespacerange <00> <FF> endcodespacerange\n\
        3 beginbfchar <01> <200B> <02> <0009> <03> <00AD> endbfchar\n\
        1 beginbfrange <30> <39> <0030> endbfrange\n\
        1 beginbfrange <41> <5A> <0041> endbfrange\n\
        endcmap CMapName currentdict /CMap defineresource pop end end";
    let font = "/Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                /Encoding /WinAnsiEncoding /ToUnicode 5 0 R >> >>";
    for (name, ch) in [("zwsp", "\\001"), ("tab", "\\002"), ("shy", "\\003")] {
        let content = format!("BT /F1 10 Tf 72 700 Td (DE89370{ch}40044053{ch}2013000) Tj ET");
        assert_redacted(
            &format!("tounicode_{name}"),
            one_page_with(
                font,
                "4 0 R",
                vec![(4, stream(content.as_bytes())), (5, stream(cmap))],
            ),
        );
    }
}

/// `/Contents` als Feld, die Teilung liegt **in** der Zeichenkette. Früher
/// fügte `filters::page_content` zwischen die Teilströme ein `\n` ein — das
/// landete *in* der Zeichenkette und zerriss die IBAN. Alle drei Betrachter
/// zeigen sie am Stück; jetzt wird dort ohne Trenner verbunden.
///
/// Geprüft werden beide Seiten: `--check-leaks` findet die IBAN in der
/// Eingabe, und der Schwärzungslauf entfernt sie (die Seite wird als ein
/// Strom neu geschrieben, die verbundene Zeichenkette bleibt gültig).
#[test]
fn contents_teilung_in_der_zeichenkette() {
    let pdf = one_page(
        "[4 0 R 5 0 R]",
        vec![
            (4, stream(b"BT /F1 10 Tf 72 700 Td (IBAN DE89 37")),
            (5, stream(b"04 0044 0532 0130 00) Tj ET")),
        ],
    );
    let dir = workdir("teilung_string");
    std::fs::write(dir.join("in.pdf"), &pdf).unwrap();
    let (check, report) = leaks(&dir, "in.pdf");
    assert_ne!(
        check, 0,
        "check-leaks übersieht die über /Contents-Teilströme geteilte IBAN:\n{report}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert_redacted("teilung_string_lauf", pdf);
}

/// Teilung **im Operator** (`T` | `j`). pdf.js verkettet die Teilströme ohne
/// Trenner und zeichnet die IBAN; poppler und mupdf sehen zwei unbekannte
/// Operatoren. Welche Lesart gilt, hängt am Betrachter — der Lauf muss das
/// als Deckungslücke melden (Rückgabewert 3), statt mit 0 zu enden.
#[test]
fn contents_teilung_im_operator() {
    let first = format!("BT /F1 10 Tf 72 700 Td (IBAN {SECRET}) T").into_bytes();
    let pdf = one_page(
        "[4 0 R 5 0 R]",
        vec![(4, stream(&first)), (5, stream(b"j ET"))],
    );
    let dir = workdir("teilung_operator");
    let (code, summary) = redact(&dir, &pdf);
    assert_eq!(code, 3, "Wortteilung nicht als Lücke gemeldet:\n{summary}");
    assert!(
        summary.contains("mitten in einem Wort"),
        "Meldung fehlt:\n{summary}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Gegenprobe: eine Teilung **zwischen** zwei Wörtern — der Normalfall jedes
/// Erzeugers, der `/Contents` als Feld schreibt — bleibt ohne Meldung, und
/// die IBAN im zweiten Teil wird geschwärzt.
#[test]
fn contents_teilung_zwischen_woertern_bleibt_still() {
    let second = format!("72 700 Td (IBAN {SECRET}) Tj ET").into_bytes();
    let pdf = one_page(
        "[4 0 R 5 0 R]",
        vec![(4, stream(b"BT /F1 10 Tf\n")), (5, stream(&second))],
    );
    assert_redacted("teilung_zwischen", pdf);
}
