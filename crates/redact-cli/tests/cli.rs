//! End-to-End-Tests gegen das gebaute Binary.
//!
//! ## Zwei Orakel, bewusst getrennt
//!
//! * „Der Text ist weg“ wird ausschließlich mit [`redact_pdf::leaks`] geprüft.
//!   Das durchsucht die geschriebene Datei auf allen Ebenen — Rohbytes, jeden
//!   dekodierten Stream, Objekt-Streams, sämtliche Zeichenketten. Der frühere
//!   Helfer sah nur `doc.get_page_content()` und war damit blind für
//!   Form-XObjects, Annotation-Appearances, Metadaten, Struct-Tree-Strings,
//!   verwaiste Objekte und die Historie inkrementeller Updates.
//! * „Der Text ist noch da“ wird mit dem Extraktor geprüft — denn hier soll
//!   nicht irgendein Byte-Rest überleben, sondern der Text tatsächlich noch
//!   lesbar auf der Seite stehen.

use redact_core::Extractor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

/// Eigenes Verzeichnis je Test, damit parallele Läufe sich nicht stören.
fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("Binary startbar")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Alle Fundstellen von `needle` in der geschriebenen Datei — auf jeder Ebene.
fn leaks_in(path: &Path, needle: &str) -> Vec<String> {
    let bytes = std::fs::read(path).expect("Ausgabedatei lesbar");
    redact_pdf::leaks(&bytes, needle)
}

/// „Der Text ist weg“ — das ehrliche Orakel.
#[track_caller]
fn assert_no_leak(path: &Path, needle: &str, what: &str) {
    let hits = leaks_in(path, needle);
    assert!(
        hits.is_empty(),
        "{what}: „{needle}“ steht noch {} mal in {}:\n{}",
        hits.len(),
        path.display(),
        hits.join("\n")
    );
}

/// „Der Text ist noch da“ — was ein Leser tatsächlich auf der Seite sieht.
fn visible_text(path: &Path) -> String {
    let doc = redact_pdf::load(path).expect("PDF ladbar");
    redact_pdf::PdfExtractor::new()
        .extract(&doc)
        .expect("Extraktion")
        .iter()
        .map(|run| run.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn demo(dir: &Path) -> PathBuf {
    let pdf = dir.join("kontoauszug.pdf");
    let out = run(&["--write-demo", pdf.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    pdf
}

#[test]
fn lists_builtin_patterns() {
    let out = run(&["--list-patterns"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("iban_de"));
    assert!(text.contains("bic"));
}

#[test]
fn redacts_with_patterns() {
    let dir = workdir("patterns");
    let input = demo(&dir);
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de,email",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert_no_leak(&output, "DE89", "IBAN");
    assert_no_leak(&output, "example.org", "E-Mail");
    assert!(
        visible_text(&output).contains("Musterbank"),
        "unbeteiligter Text verloren"
    );
}

#[test]
fn negative_list_prevents_redaction() {
    let dir = workdir("negative");
    let input = demo(&dir);
    let csv = dir.join("buchungen.csv");
    std::fs::write(
        &csv,
        "id,list_type,pattern,context_before,context_after,is_regex\n\
         n001,negative,\"DE89 3704 0044 0532 0130 00\",,,\n",
    )
    .unwrap();
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--booking-list",
        csv.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout(&out).contains("blockiert"),
        "Blockade nicht gemeldet"
    );

    assert!(
        visible_text(&output).contains("DE89"),
        "Negativliste hat nicht geschützt"
    );
    // Die IBAN auf Seite 2 steht nicht auf der Negativliste und muss weg sein.
    assert_no_leak(&output, "DE02", "zweite IBAN");
}

#[test]
fn positive_list_forces_redaction_without_patterns() {
    let dir = workdir("positive");
    let input = demo(&dir);
    let csv = dir.join("buchungen.csv");
    std::fs::write(
        &csv,
        "id,list_type,pattern,context_before,context_after,is_regex\n\
         b001,positive,\"Musterfirma GmbH\",,,\n",
    )
    .unwrap();
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--no-patterns",
        "--booking-list",
        csv.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert_no_leak(&output, "Musterfirma", "Positivtreffer");
    assert!(
        visible_text(&output).contains("DE89"),
        "ohne Patterns darf die IBAN bleiben"
    );
}

#[test]
fn review_then_apply_roundtrip() {
    let dir = workdir("review");
    let input = demo(&dir);
    let review = dir.join("review.json");
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        review.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(review.exists());

    // Ersten Treffer abwählen — er darf danach nicht geschwärzt werden.
    let data = std::fs::read_to_string(&review).unwrap();
    let mut file: redact_core::ReviewFile = serde_json::from_str(&data).unwrap();
    assert!(
        file.items.len() >= 2,
        "es sollten zwei IBANs gefunden werden"
    );
    file.items[0].enabled = false;
    std::fs::write(&review, file.to_json().unwrap()).unwrap();

    let audit = dir.join("audit.json");
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--apply-review",
        review.to_str().unwrap(),
        "--audit-log",
        audit.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        visible_text(&output).contains("DE89"),
        "abgewählter Treffer wurde trotzdem geschwärzt"
    );
    assert_no_leak(&output, "DE02", "ausgewählter Treffer");

    let log: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&audit).unwrap()).unwrap();
    assert_eq!(log["redactions"].as_array().unwrap().len(), 1);
    assert_eq!(log["metadata_stripped"], serde_json::json!(true));
    assert_eq!(log["input"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(log["output"]["sha256"].as_str().unwrap().len(), 64);
}

#[test]
fn review_file_from_another_document_is_rejected() {
    let dir = workdir("mismatch");
    let input = demo(&dir);
    let other = dir.join("andere.pdf");
    std::fs::write(&other, redact_pdf::testing::minimal_pdf("nichts geheimes")).unwrap();
    let review = dir.join("review.json");

    run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        review.to_str().unwrap(),
    ]);

    let out = run(&[
        other.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
        "--apply-review",
        review.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("andere Eingabe"));
}

#[test]
fn manual_regions_are_applied() {
    let dir = workdir("manual");
    let input = demo(&dir);
    let regions = dir.join("regions.json");
    // Deckt die Zeile „Kontoinhaber: Max Mustermann“ bei y≈750 ab.
    std::fs::write(
        &regions,
        r#"[{"page":0,
             "rect":{"ll":{"x":60.0,"y":745.0},"ur":{"x":300.0,"y":760.0}},
             "text":null,
             "source":{"manual":{"reason":"Kontoinhaber"}}}]"#,
    )
    .unwrap();
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--no-patterns",
        "--manual-regions",
        regions.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_no_leak(&output, "Max Mustermann", "manuelle Region");
    assert!(
        visible_text(&output).contains("Musterbank"),
        "zu viel geschwärzt"
    );
}

#[test]
fn rejects_broken_pdf_with_clear_message() {
    let dir = workdir("broken");
    let broken = dir.join("kaputt.pdf");
    std::fs::write(&broken, b"das ist kein PDF").unwrap();

    let out = run(&[
        broken.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("PDF-Fehler"), "unklare Meldung: {err}");
    assert!(err.contains("kaputt.pdf"), "Dateiname fehlt: {err}");
}

#[test]
fn refuses_to_overwrite_the_input() {
    let dir = workdir("overwrite");
    let input = demo(&dir);
    let out = run(&[input.to_str().unwrap(), "-o", input.to_str().unwrap()]);
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("identisch"));
}

#[test]
fn json_output_is_machine_readable() {
    let dir = workdir("json");
    let input = demo(&dir);
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--json",
    ]);
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(value["pages"], serde_json::json!(2));
    assert_eq!(value["redactions"], serde_json::json!(2));
    assert!(value["removed_glyphs"].as_u64().unwrap() > 40);
}

#[test]
fn same_input_and_config_produce_identical_output() {
    let dir = workdir("deterministic");
    let input = demo(&dir);
    let a = dir.join("a.pdf");
    let b = dir.join("b.pdf");
    for target in [&a, &b] {
        let out = run(&[
            input.to_str().unwrap(),
            "-o",
            target.to_str().unwrap(),
            "--patterns",
            "iban_de,bic",
        ]);
        assert!(out.status.success());
    }
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}

#[test]
fn writes_next_to_the_input_when_output_is_omitted() {
    let dir = workdir("default-output");
    let input = demo(&dir);

    let out = run(&[input.to_str().unwrap(), "--patterns", "iban_de"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let expected = dir.join("kontoauszug_geschwaerzt.pdf");
    assert!(
        expected.exists(),
        "Standardausgabe fehlt: {}",
        expected.display()
    );
    assert_no_leak(&expected, "DE89", "Standardausgabe");

    // Ein zweiter Lauf darf das Ergebnis nicht unbemerkt überschreiben.
    let again = run(&[input.to_str().unwrap(), "--patterns", "iban_de"]);
    assert_eq!(again.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&again.stderr).contains("existiert bereits"));

    // Mit --force schon.
    let forced = run(&[input.to_str().unwrap(), "--patterns", "iban_de", "--force"]);
    assert!(forced.status.success());
}

#[test]
fn output_suffix_is_configurable() {
    let dir = workdir("suffix");
    let input = demo(&dir);

    let out = run(&[
        input.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--output-suffix",
        "_anonym",
    ]);
    assert!(out.status.success());
    assert!(dir.join("kontoauszug_anonym.pdf").exists());
    assert!(!dir.join("kontoauszug_geschwaerzt.pdf").exists());
}

#[test]
fn review_defaults_to_a_sibling_json_file() {
    let dir = workdir("review-default");
    let input = demo(&dir);

    let out = run(&[input.to_str().unwrap(), "--review", "--patterns", "iban_de"]);
    assert!(out.status.success());
    assert!(dir.join("kontoauszug_review.json").exists());
}

// ---------------------------------------------------------------------------
// Regression: die im Audit belegten Lecks, gemessen am echten Binary
// ---------------------------------------------------------------------------

const AUDIT_IBAN: &str = "DE89 3704 0044 0532 0130 00";

/// Ein Dokument, das dieselbe IBAN an fünf Stellen trägt: im Seiteninhalt, im
/// Appearance-Stream einer Annotation, in `/ActualText` eines `/StructElem`,
/// im seitenweiten XMP und im `/V` eines Formularfeldes.
///
/// Die Analyse findet nur die erste Stelle. Ob die übrigen vier überleben,
/// misst [`binary_does_not_leak_the_iban_outside_the_page_content`].
fn audit_pdf(dir: &Path) -> PathBuf {
    use lopdf::{dictionary, Document, Object, Stream, StringFormat};

    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1",
        "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content = format!(
        "BT\n/F1 10 Tf\n72 700 Td\n(Kontoinhaber: Max Mustermann) Tj\n\
         0 -15 Td\n(IBAN: {AUDIT_IBAN}) Tj\nET\n"
    );
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));

    let ap_id = doc.add_object(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 240.into(), 20.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        },
        format!("BT\n/F1 8 Tf\n0 4 Td\n(Notiz: {AUDIT_IBAN}) Tj\nET\n").into_bytes(),
    )));
    let annot_id = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "FreeText",
        "Rect" => vec![72.into(), 680.into(), 312.into(), 700.into()],
        "F" => 4_i64,
        "AP" => dictionary! { "N" => ap_id },
    });
    let xmp_id = doc.add_object(Object::Stream(Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        format!("<x:xmpmeta><dc:title>Konto {AUDIT_IBAN}</dc:title></x:xmpmeta>").into_bytes(),
    )));

    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "Contents" => content_id, "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Annots" => vec![Object::Reference(annot_id)],
        "Metadata" => xmp_id,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );

    let elem_id = doc.add_object(dictionary! {
        "Type" => "StructElem", "S" => "Span",
        "ActualText" => Object::string_literal(AUDIT_IBAN),
    });
    let struct_root_id = doc.add_object(dictionary! {
        "Type" => "StructTreeRoot",
        "K" => vec![Object::Reference(elem_id)],
    });
    let mut utf16 = vec![0xfe_u8, 0xff];
    utf16.extend(AUDIT_IBAN.encode_utf16().flat_map(|u| u.to_be_bytes()));
    let field_id = doc.add_object(dictionary! {
        "FT" => "Tx",
        "T" => Object::string_literal("iban"),
        "V" => Object::String(utf16, StringFormat::Literal),
    });
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "StructTreeRoot" => struct_root_id,
        "AcroForm" => dictionary! { "Fields" => vec![Object::Reference(field_id)] },
    });
    doc.trailer.set("Root", catalog_id);

    let path = dir.join("audit.pdf");
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("speicherbar");
    std::fs::write(&path, bytes).unwrap();
    path
}

fn run_audit_case(name: &str) -> PathBuf {
    let dir = workdir(name);
    let input = audit_pdf(&dir);
    let output = dir.join("out.pdf");
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    output
}

/// Behoben: die verwaisten `/AP`-, `/Metadata`- und `/StructElem`-Objekte
/// werden vor dem Schreiben entfernt, und `strip_metadata` löscht seit
/// Aufgabe #4 auch das `/V` des AcroForm-Feldes (samt `/DV`, `/RV` und `/XFA`).
/// Der Kanarienvogel dazu ist entfallen.
#[test]
fn binary_does_not_leak_the_iban_outside_the_page_content() {
    let output = run_audit_case("audit-clean");
    assert_no_leak(&output, AUDIT_IBAN, "Audit-Dokument");
    // Und der sichtbare Seitentext ist ebenfalls geschwärzt.
    assert!(!visible_text(&output).contains("DE89"));
}

// ---------------------------------------------------------------------------
// Härtung gegen bösartige Eingaben und gegen Schreiben an falsche Stellen
//
// Jeder Test hier gehört zu einem reproduzierten Befund. Sie sind bewusst
// End-to-End: die Frage ist nicht, ob eine Funktion einen Fehler liefert,
// sondern ob das *Binary* mit einer Meldung stehen bleibt statt abzustürzen.
// ---------------------------------------------------------------------------

/// Baut ein PDF mit `nest` offenen `[` in einem Objekt, das vom Katalog aus
/// erreichbar ist. `lopdf 0.34` parst rekursiv und läuft dabei über den Stack.
fn deeply_nested_pdf(nest: usize) -> Vec<u8> {
    let deep: Vec<u8> = "[".repeat(nest).into_bytes();
    let closing: Vec<u8> = "]".repeat(nest).into_bytes();
    let mut body: Vec<(u32, Vec<u8>)> = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R /Junk 5 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, b"<< /Length 5 >>\nstream\nBT ET\nendstream".to_vec()),
    ];
    let mut junk = deep;
    junk.extend_from_slice(&closing);
    body.push((5, junk));
    assemble_pdf(&body)
}

/// Fügt Objekte zu einer Datei mit klassischer xref-Tabelle zusammen.
fn assemble_pdf(body: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (id, data) in body {
        offsets.push(out.len());
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", body.len() + 1).as_bytes());
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            body.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Ein PDF, dessen Content-Stream sich um ein Vielfaches aufbläht.
///
/// Auf der Platte wenige Dutzend Kilobyte, im Speicher `megabytes` MB — und
/// beim Parsen ein Vielfaches davon, weil aus jedem Operator eine eigene
/// `Operation` mit Vektor wird.
fn decompression_bomb(megabytes: usize) -> Vec<u8> {
    use lopdf::{dictionary, Document, Object, Stream};

    let unit = b"0 0 0 rg\n";
    let mut content = Vec::with_capacity(megabytes * 1024 * 1024 + unit.len());
    while content.len() < megabytes * 1024 * 1024 {
        content.extend_from_slice(unit);
    }

    let mut doc = Document::with_version("1.5");
    let mut stream = Stream::new(dictionary! {}, content);
    stream.compress().expect("komprimierbar");
    let content_id = doc.add_object(stream);
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);

    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("speicherbar");
    bytes
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Ein Absturz ist kein Fehlerwert. Exit 134 (SIGABRT) hieße Stapelüberlauf
/// oder gescheiterte Speicheranforderung — genau das soll nicht mehr passieren.
#[track_caller]
fn assert_clean_refusal(out: &Output, needle: &str) {
    assert!(
        !out.status.success(),
        "die Datei wurde angenommen, obwohl sie abgelehnt gehört"
    );
    assert_ne!(
        out.status.code(),
        Some(134),
        "Abbruch statt Fehlermeldung (SIGABRT)"
    );
    assert!(
        out.status.code().is_some(),
        "durch ein Signal beendet statt mit einem Rückgabewert"
    );
    let err = stderr(out);
    assert!(
        err.contains(needle),
        "Meldung nennt „{needle}“ nicht: {err}"
    );
}

// --- A1: Verschachtelungstiefe ---------------------------------------------

#[test]
fn deep_nesting_is_refused_instead_of_crashing() {
    let dir = workdir("deep-nesting");
    let input = dir.join("tief.pdf");
    std::fs::write(&input, deeply_nested_pdf(200_000)).unwrap();

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_clean_refusal(&out, "Verschachtelungstiefe");
    assert!(!dir.join("out.pdf").exists(), "trotz Abbruch geschrieben");
}

#[test]
fn deep_nesting_hidden_in_a_compressed_stream_is_also_refused() {
    // Die Verschachtelung in einem komprimierten Objekt-Stream zu verstecken
    // ist der offensichtliche nächste Versuch — die Rohbytes der Datei zeigen
    // sie dann nicht mehr.
    use lopdf::{dictionary, Stream};

    let dir = workdir("deep-nesting-objstm");
    let inner = format!("{}{}", "[".repeat(50_000), "]".repeat(50_000));
    let payload = format!("6 0 {inner} ");
    let mut stream = Stream::new(
        dictionary! { "Type" => "ObjStm", "N" => 1_i64, "First" => 4_i64 },
        payload.into_bytes(),
    );
    stream.compress().expect("komprimierbar");
    let mut header = format!(
        "<< /Type /ObjStm /N 1 /First 4 /Filter /FlateDecode /Length {} >>\nstream\n",
        stream.content.len()
    )
    .into_bytes();
    header.extend_from_slice(&stream.content);
    header.extend_from_slice(b"\nendstream");

    let body: Vec<(u32, Vec<u8>)> = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R /Junk 6 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, b"<< /Length 5 >>\nstream\nBT ET\nendstream".to_vec()),
        (5, header),
    ];
    let input = dir.join("tief_objstm.pdf");
    std::fs::write(&input, assemble_pdf(&body)).unwrap();

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_clean_refusal(&out, "Verschachtelungstiefe");
}

#[test]
fn ordinary_nesting_is_still_accepted() {
    // Gegenprobe: die Grenze darf normale Dokumente nicht treffen.
    let dir = workdir("shallow-nesting");
    let input = dir.join("flach.pdf");
    std::fs::write(&input, deeply_nested_pdf(40)).unwrap();

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
}

// --- B1: Dekompressionsbombe ------------------------------------------------

#[test]
fn a_decompression_bomb_is_refused_with_a_message() {
    let dir = workdir("bomb");
    let input = dir.join("bombe.pdf");
    let bytes = decompression_bomb(20);
    std::fs::write(&input, &bytes).unwrap();
    assert!(
        bytes.len() < 1024 * 1024,
        "Testdatei ist keine Bombe: {} Bytes auf der Platte",
        bytes.len()
    );

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_clean_refusal(&out, "Budget");
}

#[test]
fn the_stream_budget_can_be_raised_deliberately() {
    // Die Grenze ist eine Voreinstellung, keine Mauer — wer weiß, was er tut,
    // hebt sie an.
    let dir = workdir("bomb-override");
    let input = dir.join("gross.pdf");
    std::fs::write(&input, decompression_bomb(20)).unwrap();

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
        "--max-parsed-mb",
        "64",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
}

// --- B2: Zeitbremse ---------------------------------------------------------

#[test]
fn too_many_candidates_are_refused() {
    let dir = workdir("candidates");
    let input = demo(&dir);
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
        "--max-candidates",
        "1",
    ]);
    assert_clean_refusal(&out, "Trefferkandidaten");
    assert_eq!(out.status.code(), Some(2));
}

// --- A2: Schreibpfad --------------------------------------------------------

#[test]
fn the_input_is_protected_in_every_spelling() {
    let dir = workdir("aliases");
    let input = demo(&dir);
    let sub = dir.join("unter");
    std::fs::create_dir_all(&sub).unwrap();

    let mut aliases: Vec<PathBuf> = vec![
        // absolut
        input.clone(),
        // über einen Umweg zurück
        sub.join("..").join("kontoauszug.pdf"),
        // mit führendem ./
        dir.join(".").join("kontoauszug.pdf"),
    ];
    // Ein Symlink auf die Eingabe ist derselbe Inhalt unter anderem Namen.
    #[cfg(unix)]
    {
        let link = dir.join("zeigt_auf_eingabe.pdf");
        std::os::unix::fs::symlink(&input, &link).unwrap();
        aliases.push(link);
        // Ein Hardlink ebenfalls — und den erkennt kein Pfadvergleich.
        let hard = dir.join("hardlink.pdf");
        std::fs::hard_link(&input, &hard).unwrap();
        aliases.push(hard);
    }

    let before = std::fs::read(&input).unwrap();
    for alias in aliases {
        // Auch mit --force: das Original ist nicht wiederherstellbar.
        let out = run(&[
            input.to_str().unwrap(),
            "-o",
            alias.to_str().unwrap(),
            "--force",
        ]);
        assert!(
            !out.status.success(),
            "„{}“ hat die Eingabedatei überschrieben",
            alias.display()
        );
        assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
        assert_eq!(
            std::fs::read(&input).unwrap(),
            before,
            "die Eingabedatei wurde über „{}“ verändert",
            alias.display()
        );
    }
}

#[cfg(unix)]
#[test]
fn writing_through_a_symlink_fails() {
    let dir = workdir("symlink-out");
    let input = demo(&dir);
    let victim = dir.join("fremde_datei.txt");
    std::fs::write(&victim, b"das hier gehoert jemand anderem").unwrap();
    let link = dir.join("ausgabe.pdf");
    std::os::unix::fs::symlink(&victim, &link).unwrap();

    for force in [&[][..], &["--force"][..]] {
        let mut args = vec![input.to_str().unwrap(), "-o", link.to_str().unwrap()];
        args.extend_from_slice(force);
        let out = run(&args);
        assert!(!out.status.success(), "Schreiben durch den Link erlaubt");
        assert!(
            stderr(&out).contains("symbolischer Link"),
            "{}",
            stderr(&out)
        );
    }
    assert_eq!(
        std::fs::read(&victim).unwrap(),
        b"das hier gehoert jemand anderem",
        "die Zieldatei des Links wurde überschrieben"
    );
}

#[test]
fn write_demo_respects_force() {
    let dir = workdir("demo-force");
    let target = dir.join("beispiel.pdf");
    std::fs::write(&target, b"vorhanden").unwrap();

    let out = run(&["--write-demo", target.to_str().unwrap()]);
    assert!(!out.status.success(), "hat ohne --force überschrieben");
    assert_eq!(std::fs::read(&target).unwrap(), b"vorhanden");

    let out = run(&["--write-demo", target.to_str().unwrap(), "--force"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(std::fs::read(&target).unwrap().starts_with(b"%PDF-"));
}

#[test]
fn review_out_respects_force() {
    let dir = workdir("review-force");
    let input = demo(&dir);
    let target = dir.join("review.json");
    std::fs::write(&target, b"{\"vorhanden\":true}").unwrap();

    let out = run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        target.to_str().unwrap(),
    ]);
    assert!(!out.status.success(), "hat ohne --force überschrieben");
    assert_eq!(std::fs::read(&target).unwrap(), b"{\"vorhanden\":true}");

    let out = run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        target.to_str().unwrap(),
        "--force",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn audit_log_respects_force() {
    let dir = workdir("audit-force");
    let input = demo(&dir);
    let target = dir.join("audit.json");
    std::fs::write(&target, b"{\"vorhanden\":true}").unwrap();

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
        "--audit-log",
        target.to_str().unwrap(),
    ]);
    assert!(!out.status.success(), "hat ohne --force überschrieben");
    assert_eq!(std::fs::read(&target).unwrap(), b"{\"vorhanden\":true}");
    assert!(
        !dir.join("out.pdf").exists(),
        "das PDF wurde geschrieben, obwohl das Audit-Log das Ziel blockiert — \
         die Prüfung muss vor der Arbeit laufen"
    );
}

#[cfg(unix)]
#[test]
fn review_and_audit_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = workdir("modes");
    let input = demo(&dir);
    let review = dir.join("review.json");
    let out = run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        review.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let mode = std::fs::metadata(&review).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "Review-Datei mit Modus {mode:o} — sie enthält die Fundstellen im Klartext"
    );

    let audit = dir.join("audit.json");
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
        "--audit-log",
        audit.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let mode = std::fs::metadata(&audit).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "Audit-Log mit Modus {mode:o}");
}

#[test]
fn a_failed_run_leaves_no_temporary_file_behind() {
    // Geschrieben wird über eine temporäre Datei im Zielverzeichnis. Bleibt
    // sie liegen, steht dort ungeschützt ein Teilergebnis.
    let dir = workdir("no-temp");
    let input = demo(&dir);
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));

    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp") || n.starts_with('.'))
        .collect();
    assert!(leftovers.is_empty(), "Reste geblieben: {leftovers:?}");
}

#[test]
fn deep_nesting_cannot_hide_behind_a_binary_looking_stream() {
    // Naheliegender Umgehungsversuch: den Content-Stream so aussehen lassen
    // wie Binärdaten (Rauschen, dazu ein /Length1 im Dictionary, das sonst nur
    // in Schriftprogrammen steht), damit die Vorprüfung ihn für Nutzlast hält.
    // Geparst wird er trotzdem — also muss er auch geprüft werden.
    let dir = workdir("binary-disguise");
    let mut payload: Vec<u8> = (0..20_000u32).map(|i| (i * 61 % 256) as u8).collect();
    payload.extend(std::iter::repeat_n(b'[', 2_000));
    payload.extend(std::iter::repeat_n(b']', 2_000));

    let mut stream =
        format!("<< /Length1 4711 /Length {} >>\nstream\n", payload.len()).into_bytes();
    stream.extend_from_slice(&payload);
    stream.extend_from_slice(b"\nendstream");

    let body: Vec<(u32, Vec<u8>)> = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, stream),
    ];
    let input = dir.join("getarnt.pdf");
    std::fs::write(&input, assemble_pdf(&body)).unwrap();

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_clean_refusal(&out, "Verschachtelungstiefe");
}

// ---------------------------------------------------------------------------
// Ausweichmöglichkeiten, die es bisher nicht gab (Aufgaben 53a und 54)
// ---------------------------------------------------------------------------

/// Ein Dokument mit einem Bild, das sich nicht dekodieren lässt (JPX), und
/// einem Textstück daneben.
fn pdf_with_an_undecodable_image(dir: &Path) -> PathBuf {
    use lopdf::{dictionary, Document, Object, Stream};

    let mut doc = Document::with_version("1.5");
    let image_id = doc.add_object(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image",
            "Width" => 20_i64, "Height" => 20_i64,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8_i64,
            "Filter" => "JPXDecode",
        },
        vec![0x99; 64],
    )));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => image_id },
    });
    // Das Bild liegt bei (50,600)–(150,700).
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
    ));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "Contents" => content_id, "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);

    let path = dir.join("mit_bild.pdf");
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Eine Regionsdatei, die genau auf dem Bild liegt.
fn region_on_the_image(dir: &Path) -> PathBuf {
    let path = dir.join("regionen.json");
    std::fs::write(
        &path,
        r#"[{"page":0,
             "rect":{"ll":{"x":75.0,"y":650.0},"ur":{"x":100.0,"y":675.0}},
             "text":null,
             "source":{"manual":{"reason":"Bild"}}}]"#,
    )
    .unwrap();
    path
}

/// **Aufgabe 53a.** Ohne Schalter bricht der Lauf bei einem nicht
/// dekodierbaren Bild ab — richtig so, denn die Schwärzung läge nur obenauf.
/// Bis hierher gab es aber gar keinen Ausweg: die Bibliothek konnte es,
/// die Kommandozeile nicht.
#[test]
fn an_undecodable_image_can_be_waved_through_on_request() {
    let dir = workdir("undecodable");
    let input = pdf_with_an_undecodable_image(&dir);
    let regions = region_on_the_image(&dir);
    let out = dir.join("out.pdf");

    let refused = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--manual-regions",
        regions.to_str().unwrap(),
        "--no-patterns",
    ]);
    assert!(!refused.status.success(), "hätte abbrechen müssen");
    let message = String::from_utf8_lossy(&refused.stderr);
    assert!(message.contains("JPXDecode"), "{message}");
    assert!(!out.exists(), "es darf keine Ausgabe entstanden sein");

    // Mit dem Schalter entsteht eine Ausgabe — und der Lauf sagt, was das
    // bedeutet.
    let allowed = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--manual-regions",
        regions.to_str().unwrap(),
        "--no-patterns",
        "--allow-undecodable-images",
    ]);
    assert!(allowed.status.success(), "{}", stderr(&allowed));
    assert!(out.exists());
    let warned = stderr(&allowed);
    assert!(
        warned.to_lowercase().contains("bild"),
        "der Lauf muss auf das ungeschwärzte Bild hinweisen: {warned}"
    );
}

/// **Aufgabe 54.** `"sha256": ""` von Hand in eine Review-Datei geschrieben
/// hebelte die Identitätsprüfung aus: die Kette gab stillschweigend `Ok`
/// zurück, und die Rechtecke landeten auf einem beliebigen Dokument.
#[test]
fn a_review_file_without_a_checksum_is_refused_unless_allowed() {
    let dir = workdir("unverified");
    let input = demo(&dir);
    let review = dir.join("review.json");

    let out = run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        review.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));

    // Von Hand entwertet — genau der Fall, um den es geht.
    let data = std::fs::read_to_string(&review).unwrap();
    let mut file: redact_core::ReviewFile = serde_json::from_str(&data).unwrap();
    file.input.sha256 = String::new();
    std::fs::write(&review, file.to_json().unwrap()).unwrap();

    let output = dir.join("out.pdf");
    let refused = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--apply-review",
        review.to_str().unwrap(),
    ]);
    assert!(!refused.status.success(), "leere Prüfsumme kam durch");
    let message = String::from_utf8_lossy(&refused.stderr);
    assert!(message.contains("keine Prüfsumme"), "{message}");
    assert!(message.contains("--allow-unverified-review"), "{message}");
    assert!(!output.exists());

    // Ausdrücklich überstimmt geht es durch.
    let allowed = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--apply-review",
        review.to_str().unwrap(),
        "--allow-unverified-review",
    ]);
    assert!(allowed.status.success(), "{}", stderr(&allowed));
    assert_no_leak(&output, "DE02", "die Review-Datei wurde angewendet");
}

/// Eine Review-Datei mit **falscher** Prüfsumme bleibt auch mit dem Schalter
/// abgelehnt: „ungeprüft“ ist etwas anderes als „nachweislich fremd“.
#[test]
fn the_switch_does_not_help_a_review_file_of_another_document() {
    let dir = workdir("still-refused");
    let input = demo(&dir);
    let other = dir.join("andere.pdf");
    std::fs::write(&other, redact_pdf::testing::minimal_pdf("nichts geheimes")).unwrap();
    let review = dir.join("review.json");

    run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        review.to_str().unwrap(),
    ]);

    let out = run(&[
        other.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
        "--apply-review",
        review.to_str().unwrap(),
        "--allow-unverified-review",
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("andere Eingabe"));
}

// ---------------------------------------------------------------------------
// Aufgabe #58 — die Obergrenze für dekodierte Bildbytes
// ---------------------------------------------------------------------------

/// Ein 1-Bit-Graustufenbild mit `/FlateDecode` — die gewöhnliche Kodierung
/// eines Schwarzweiß-Scans. 2000×2000 Bildpunkte sind 500 kB roh und 16 MB
/// dekodiert; genau dieser Faktor 32 machte `--max-decompressed-mb` wirkungslos.
fn pdf_with_a_bilevel_scan(dir: &Path) -> PathBuf {
    use lopdf::{dictionary, Document, Object, Stream};

    let (width, height) = (2000_i64, 2000_i64);
    let stride = (width as usize).div_ceil(8);
    let mut image = Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image",
            "Width" => width, "Height" => height,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 1_i64,
        },
        vec![0u8; stride * height as usize],
    );
    let _ = image.compress();

    let mut doc = Document::with_version("1.5");
    let image_id = doc.add_object(Object::Stream(image));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => image_id },
    });
    // Das Bild liegt bei (50,600)–(150,700), genau unter `region_on_the_image`.
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
    ));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "Contents" => content_id, "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);

    let path = dir.join("scan.pdf");
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Zu enge Grenze: der Lauf endet mit einer Meldung und einem Rückgabewert —
/// nicht mit SIGABRT und nicht mit einer gescheiterten Speicheranforderung.
/// Genau das sichert `SECURITY.md` zu.
#[test]
fn the_image_budget_ends_the_run_with_a_message_not_a_crash() {
    let dir = workdir("image-budget");
    let input = pdf_with_a_bilevel_scan(&dir);
    let regions = region_on_the_image(&dir);
    let out = dir.join("out.pdf");

    let refused = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--manual-regions",
        regions.to_str().unwrap(),
        "--no-patterns",
        "--max-image-mb",
        "1",
    ]);
    assert!(!refused.status.success(), "hätte abbrechen müssen");
    assert_ne!(
        refused.status.code(),
        Some(134),
        "SIGABRT ist kein kontrollierter Abbruch"
    );
    let message = stderr(&refused);
    assert!(
        message.contains("Grenze") && message.contains("max-image-mb"),
        "{message}"
    );
    assert!(!out.exists(), "es darf keine Ausgabe entstanden sein");

    // Mit ausreichender Grenze — und mit der Vorgabe — läuft dieselbe Datei durch.
    let allowed = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--manual-regions",
        regions.to_str().unwrap(),
        "--no-patterns",
        "--max-image-mb",
        "64",
    ]);
    assert!(allowed.status.success(), "{}", stderr(&allowed));
    assert!(out.exists());
}
