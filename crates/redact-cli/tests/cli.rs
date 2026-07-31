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

/// Verbleibende Ursache: das `/V` des AcroForm-Feldes wird nie betrachtet.
/// Die verwaisten `/AP`-, `/Metadata`- und `/StructElem`-Objekte werden
/// inzwischen vor dem Schreiben entfernt.
#[test]
#[ignore = "bekannter Leak, siehe Aufgabe #27"]
fn binary_does_not_leak_the_iban_outside_the_page_content() {
    let output = run_audit_case("audit-clean");
    assert_no_leak(&output, AUDIT_IBAN, "Audit-Dokument");
}

/// Kanarienvogel zum vorigen Test: hält den aktuellen Zustand fest. Schlägt er
/// fehl, ist der Defekt behoben — dann kann das `#[ignore]` oben weg und dieser
/// Test hier verschwinden.
#[test]
fn canary_binary_still_leaks_the_iban_outside_the_page_content() {
    let output = run_audit_case("audit-canary");
    let hits = leaks_in(&output, AUDIT_IBAN);
    assert!(
        !hits.is_empty(),
        "Der Defekt scheint behoben. Dann bitte das #[ignore] an \
         binary_does_not_leak_the_iban_outside_the_page_content entfernen."
    );
    // Der sichtbare Seitentext selbst ist geschwärzt — das Leck sitzt daneben.
    assert!(!visible_text(&output).contains("DE89"));
}
