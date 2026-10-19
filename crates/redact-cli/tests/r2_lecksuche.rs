//! Runde 2, `--check-leaks`: Träger, die das Orakel bis dahin als „nicht
//! gefunden“ durchwinkte.
//!
//! * eine `/ToUnicode`-CMap, deren Zuordnung in **unbenutzten** Codes die
//!   IBAN buchstabiert — kein Betrachter zeigt sie, jeder, der die Schrift
//!   ausliest, liest sie;
//! * dasselbe mit Glyphennamen in einem `/Differences`-Array;
//! * eine Zeichenkette, die über zwei Teile eines `/Contents`-Felds geteilt
//!   ist — poppler, pdf.js und mupdf zeigen sie am Stück.
//!
//! In allen drei Fällen steht die IBAN in keiner einzelnen Bytefolge der
//! Datei, und keine Sicht fand sie.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use lopdf::{dictionary, Document, Object, ObjectId, Stream};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

fn squeezed() -> String {
    SECRET.chars().filter(|c| !c.is_whitespace()).collect()
}

fn workdir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("r2-lecksuche-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("Arbeitsverzeichnis");
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_redact-rs"))
        .args(args)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .expect("Binary startet")
}

/// Eine Seite mit harmlosem Text; gibt Dokument und Schrift-Id zurück.
/// `contents` sind die Inhaltsströme der Seite, in Reihenfolge.
fn page_with(contents: &[&[u8]]) -> (Document, ObjectId) {
    let mut doc = Document::with_version("1.7");
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let ids: Vec<Object> = contents
        .iter()
        .map(|c| Object::Reference(doc.add_object(Stream::new(dictionary! {}, c.to_vec()))))
        .collect();
    let contents = if ids.len() == 1 {
        ids[0].clone()
    } else {
        Object::Array(ids)
    };
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => contents,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1_i64,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    (doc, font)
}

const HARMLOS: &[u8] = b"BT /F1 10 Tf 72 700 Td (Kontoauszug) Tj ET\n";

/// Rückgabewert von `--check-leaks` über `doc`, mit beiden Fassungen des
/// Begriffs (mit und ohne Leerraum).
fn check_leaks(name: &str, doc: &mut Document) -> Output {
    let dir = workdir(name);
    let input = dir.join("ein.pdf");
    doc.save(&input).expect("Probe schreibbar");
    let out = run(&[
        input.to_str().unwrap(),
        "--check-leaks",
        SECRET,
        "--check-leaks",
        &squeezed(),
    ]);
    let _ = std::fs::remove_dir_all(&dir);
    out
}

fn tounicode(doc: &mut Document, font: ObjectId, cmap: String) {
    let tu = doc.add_object(Stream::new(dictionary! {}, cmap.into_bytes()));
    doc.get_dictionary_mut(font).unwrap().set("ToUnicode", tu);
}

fn cmap(body: &str) -> String {
    format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         /CMapName /Adobe-Identity-UCS def\n\
         1 begincodespacerange <00> <FF> endcodespacerange\n{body}\
         endcmap end end\n"
    )
}

/// Ein `bfrange` mit Array-Ziel in unbenutzten Codes buchstabiert die IBAN.
#[test]
fn tounicode_bfrange_buchstabiert_die_iban() {
    let (mut doc, font) = page_with(&[HARMLOS]);
    let targets: String = squeezed()
        .chars()
        .map(|c| format!("<{:04X}> ", c as u32))
        .collect();
    tounicode(
        &mut doc,
        font,
        cmap(&format!(
            "1 beginbfrange\n<C0> <D5> [{targets}]\nendbfrange\n"
        )),
    );
    let out = check_leaks("bfrange", &mut doc);
    assert_ne!(
        out.status.code(),
        Some(0),
        "--check-leaks liest die ToUnicode-Ziele nicht:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Dasselbe als Reihe von `bfchar`, in der Datei **rückwärts** notiert —
/// gelesen wird in Code-Reihenfolge.
#[test]
fn tounicode_bfchar_in_code_reihenfolge() {
    let (mut doc, font) = page_with(&[HARMLOS]);
    let chars: Vec<char> = squeezed().chars().collect();
    let mut body = format!("{} beginbfchar\n", chars.len());
    for (i, c) in chars.iter().enumerate().rev() {
        body.push_str(&format!("<{:02X}> <{:04X}>\n", 0xC0 + i, *c as u32));
    }
    body.push_str("endbfchar\n");
    tounicode(&mut doc, font, cmap(&body));
    let out = check_leaks("bfchar", &mut doc);
    assert_ne!(
        out.status.code(),
        Some(0),
        "--check-leaks liest bfchar nicht in Code-Reihenfolge:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Gegenprobe: eine gewöhnliche CMap (ein `bfrange`, das das ASCII-Alphabet
/// hochzählt) ist kein Fund.
#[test]
fn gewoehnliche_cmap_ist_kein_fund() {
    let (mut doc, font) = page_with(&[HARMLOS]);
    tounicode(
        &mut doc,
        font,
        cmap("1 beginbfrange\n<20> <7E> <0020>\nendbfrange\n"),
    );
    let out = check_leaks("gewoehnlich", &mut doc);
    assert_eq!(
        out.status.code(),
        Some(0),
        "Fehlalarm an einer gewöhnlichen CMap:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Glyphennamen eines `/Differences`-Arrays buchstabieren die IBAN.
#[test]
fn differences_glyphennamen_buchstabieren_die_iban() {
    let (mut doc, font) = page_with(&[HARMLOS]);
    let names = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    ];
    let mut diffs: Vec<Object> = vec![192.into()];
    for c in squeezed().chars() {
        let name = match c.to_digit(10) {
            Some(d) => names[d as usize].to_string(),
            None => c.to_string(),
        };
        diffs.push(Object::Name(name.into_bytes()));
    }
    doc.get_dictionary_mut(font).unwrap().set(
        "Encoding",
        dictionary! { "Type" => "Encoding", "BaseEncoding" => "WinAnsiEncoding",
        "Differences" => diffs },
    );
    let out = check_leaks("differences", &mut doc);
    assert_ne!(
        out.status.code(),
        Some(0),
        "--check-leaks liest die Glyphennamen nicht:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Eine Zeichenkette, die im ersten Teil eines `/Contents`-Felds beginnt und
/// im zweiten endet. Sicht 6 las je Strom und fand zwei Bruchstücke.
#[test]
fn zeichenkette_ueber_contents_teile_wird_gefunden() {
    let (mut doc, _) = page_with(&[
        b"BT /F1 10 Tf 72 700 Td (IBAN DE89 37",
        b"04 0044 0532 0130 00) Tj ET",
    ]);
    let out = check_leaks("contents_teile", &mut doc);
    assert_ne!(
        out.status.code(),
        Some(0),
        "--check-leaks übersieht die über /Contents-Teile geteilte IBAN:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}
