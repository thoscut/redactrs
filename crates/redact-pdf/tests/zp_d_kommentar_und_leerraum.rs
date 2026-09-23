//! Spur A, Runde 2, beim Befund #98 gefunden (Register #103): eine
//! gewöhnliche Seite wurde abgelehnt, weil der Zerleger von `lopdf` den
//! Leerraum der Norm nicht kennt.
//!
//! Ein Kommentar vor einer Leerzeile (`… ET\n% Kopf\n\nBT …`) brach
//! `Content::decode_strict`; die Seite galt als nicht zerlegbar, und die
//! Schwärzung endete mit Rückgabewert 1 und „ließ sich nicht in Operationen
//! zerlegen“ — obwohl `pdftotext` die Seite vollständig liest. Dasselbe galt
//! für NUL und Seitenvorschub als Leerraum (PDF 32000-1, Tabelle 1).
//!
//! Jetzt schreibt `ops::decode_content_checked` Kommentare und diesen
//! Leerraum außerhalb literaler Zeichenketten vor dem zweiten Versuch um.
//! Was wirklich keine Syntax ist, bleibt abgeschnitten.

mod common;

use common::{page, SECRET};
use lopdf::Object;
use redact_core::{Action, Redaction, Region, Source};
use redact_pdf::ops::decode_content_checked;
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfExtractor, PdfRedactor};

fn operatoren(data: &[u8]) -> (Vec<String>, Vec<usize>) {
    let decoded = decode_content_checked(data);
    (
        decoded
            .operations
            .iter()
            .map(|op| op.operator.clone())
            .collect(),
        decoded.truncated,
    )
}

#[test]
fn ein_kommentar_vor_einer_leerzeile_kostet_die_seite_nicht() {
    let (ops, truncated) = operatoren(b"BT (a) Tj ET\n% Kopf\n\nBT (b) Tj ET\n");
    assert!(truncated.is_empty(), "abgeschnitten: {truncated:?}");
    assert_eq!(ops, ["BT", "Tj", "ET", "BT", "Tj", "ET"]);
}

#[test]
fn nul_und_seitenvorschub_sind_leerraum() {
    let (ops, truncated) = operatoren(b"q\x0cQ\x00BT (a) Tj ET\n");
    assert!(truncated.is_empty(), "abgeschnitten: {truncated:?}");
    assert_eq!(ops, ["q", "Q", "BT", "Tj", "ET"]);
}

/// Ein `%` in einer Zeichenkette ist Text, kein Kommentar — auch auf dem
/// zweiten Weg, den ein Kommentar vor einer Leerzeile erzwingt.
#[test]
fn ein_prozentzeichen_in_einer_zeichenkette_bleibt_text() {
    let decoded = decode_content_checked(b"BT (100% sicher) Tj ET\n% Kopf\n\nBT (b) Tj ET\n");
    assert!(decoded.truncated.is_empty(), "{:?}", decoded.truncated);
    let text = decoded
        .operations
        .iter()
        .find(|op| op.operator == "Tj")
        .and_then(|op| op.operands.first())
        .and_then(|o| match o {
            Object::String(bytes, _) => Some(bytes.clone()),
            _ => None,
        })
        .expect("Tj mit Zeichenkette");
    assert_eq!(text, b"100% sicher");
}

/// Gegenprobe: was keine Syntax ist, bleibt ein Bruch.
#[test]
fn ungueltige_syntax_bleibt_abgeschnitten() {
    let (_, truncated) = operatoren(b"] BT (a) Tj ET\n% Kopf\n\n");
    assert!(!truncated.is_empty(), "ein verirrtes ] ging durch");
}

/// Am ganzen Weg: die Seite wird gelesen, das Geheimnis gefunden und
/// geschwärzt.
#[test]
fn die_seite_mit_kommentar_und_leerzeile_wird_geschwaerzt() {
    let mut d = page(&[]);
    d.set_content(
        format!(
            "BT /F1 12 Tf 72 700 Td (Kontoinhaber Max Mustermann) Tj ET\n% Kopfzeile\n\n\
             BT /F1 12 Tf 72 680 Td ({SECRET}) Tj ET\n"
        )
        .as_bytes(),
    );
    let bytes = d.finish();
    let doc = load_from_bytes(&bytes).expect("ladbar");
    let (runs, _) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("die Seite muss sich lesen lassen");
    let redactions: Vec<Redaction> = runs
        .iter()
        .filter_map(|run| {
            let pos = run.text.find(SECRET)?;
            let rect = run.rect_for_byte_range(pos, pos + SECRET.len())?;
            Some(Redaction::new(
                Region::new(
                    run.page,
                    rect,
                    Some(SECRET.to_string()),
                    Source::Pattern {
                        pattern_id: "iban_de".into(),
                        confidence: 1.0,
                    },
                ),
                Action::Blackout,
            ))
        })
        .collect();
    assert_eq!(redactions.len(), 1, "{runs:?}");
    let mut doc = load_from_bytes(&bytes).expect("ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, &redactions)
        .expect("Schwärzung");
    let out = save_to_bytes(&doc).expect("speicherbar");
    let found = leaks(&out, SECRET);
    assert!(found.is_empty(), "{found:?}");
}
