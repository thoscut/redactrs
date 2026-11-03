//! Prüfrunde „stille Lecks im Inhalt“, Befund 1: die Schrift eines
//! `/ExtGState` (`/Font [ref size]`, PDF 32000-1 Tabelle 58).
//!
//! `/GS1 gs` darf die Schrift setzen wie ein `Tf`. Poppler wertet das aus;
//! der Interpreter übersprang es und dekodierte mit der Schrift des letzten
//! `Tf` — oder, ohne `Tf`, mit gar keiner. Steht die IBAN in einer Schrift,
//! deren `/Differences` die Codes `abkl …` auf `DE89 …` legt, zeigte
//! `pdftotext` die IBAN, die Analyse las `abkl …`: „Treffer gesamt: 0“,
//! Rückgabewert 0, und `--check-leaks` sagte „nicht gefunden“ — es liest
//! dieselbe Sicht.
//!
//! Orakel überall: [`redact_pdf::leaks`]. Weil das Orakel genau die Sicht des
//! Interpreters liest, ist [`das_orakel_sieht_die_iban_in_der_eingabe`] der
//! Kalibrierfall: ohne die Korrektur ist er rot.

mod common;

use common::{page, SECRET};
use lopdf::{dictionary, Object};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

/// `SECRET` in den Codes der Schrift F2: a→D, b→E, c…l→0…9.
const ENCODED: &str = "abkl fjcg ccgg chfe cdfc cc";

/// Eine Seite mit F1 (Helvetica/WinAnsi) und F2 (Helvetica mit
/// `/Differences`), dazu `/GS1` mit `/Font [F2 12]`. `content` ist der
/// Seiteninhalt.
fn dokument(content: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let f2 = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => dictionary! {
            "Type" => "Encoding",
            "Differences" => vec![
                97.into(),
                Object::Name(b"D".to_vec()),
                Object::Name(b"E".to_vec()),
                Object::Name(b"zero".to_vec()),
                Object::Name(b"one".to_vec()),
                Object::Name(b"two".to_vec()),
                Object::Name(b"three".to_vec()),
                Object::Name(b"four".to_vec()),
                Object::Name(b"five".to_vec()),
                Object::Name(b"six".to_vec()),
                Object::Name(b"seven".to_vec()),
                Object::Name(b"eight".to_vec()),
                Object::Name(b"nine".to_vec()),
            ],
        },
    }));
    let gs1 = d.add(Object::Dictionary(dictionary! {
        "Type" => "ExtGState",
        "Font" => vec![Object::Reference(f2), 12.into()],
    }));
    let gs_leer = d.add(Object::Dictionary(dictionary! {
        "Type" => "ExtGState",
        "LW" => 2,
    }));
    let resources = d
        .doc
        .get_dictionary_mut(d.resources_id)
        .expect("Ressourcen");
    resources.set("ExtGState", dictionary! { "GS1" => gs1, "GS0" => gs_leer });
    d.set_content(content.as_bytes());
    d.finish()
}

fn analyse(bytes: &[u8]) -> Vec<TextRun> {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion")
        .0
}

fn redaction_for(run: &TextRun, needle: &str) -> Option<Redaction> {
    let pos = run.text.find(needle)?;
    let rect = run.rect_for_byte_range(pos, pos + needle.len())?;
    Some(Redaction::new(
        Region::new(
            run.page,
            rect,
            Some(needle.to_string()),
            Source::Pattern {
                pattern_id: "iban_de".into(),
                confidence: 1.0,
            },
        ),
        Action::Blackout,
    ))
}

/// Schwärzt jede Fundstelle von `SECRET`; liefert die Lecks der Ausgabe.
fn nach_der_pipeline(bytes: &[u8]) -> Vec<String> {
    let runs = analyse(bytes);
    let redactions: Vec<Redaction> = runs
        .iter()
        .filter_map(|run| redaction_for(run, SECRET))
        .collect();
    assert!(
        !redactions.is_empty(),
        "die Analyse findet die IBAN nicht: {:?}",
        runs.iter().map(|r| &r.text).collect::<Vec<_>>()
    );
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, &redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    leaks(&out, SECRET)
}

fn nach_tf() -> Vec<u8> {
    dokument(&format!(
        "BT /F1 12 Tf /GS1 gs 72 700 Td (IBAN: {ENCODED}) Tj ET"
    ))
}

fn ohne_tf() -> Vec<u8> {
    dokument(&format!("BT /GS1 gs 72 700 Td (IBAN: {ENCODED}) Tj ET"))
}

#[test]
fn das_orakel_sieht_die_iban_in_der_eingabe() {
    assert!(
        !leaks(&nach_tf(), SECRET).is_empty(),
        "die Schrift aus /ExtGState /Font wurde nicht ausgewertet"
    );
    assert!(!leaks(&ohne_tf(), SECRET).is_empty());
}

#[test]
fn die_schrift_des_gs_gilt_nach_einem_tf() {
    let leaks = nach_der_pipeline(&nach_tf());
    assert!(
        leaks.is_empty(),
        "IBAN steht noch in der Ausgabe: {leaks:?}"
    );
}

#[test]
fn die_schrift_des_gs_gilt_ohne_tf() {
    let leaks = nach_der_pipeline(&ohne_tf());
    assert!(
        leaks.is_empty(),
        "IBAN steht noch in der Ausgabe: {leaks:?}"
    );
}

/// Ein `Tf` nach dem `gs` gewinnt wieder — die Reihenfolge entscheidet,
/// wie bei jedem anderen Eintrag des Textzustands.
#[test]
fn ein_spaeteres_tf_gewinnt() {
    let bytes = dokument(&format!(
        "BT /GS1 gs /F1 12 Tf 72 700 Td (IBAN: {SECRET}) Tj ET"
    ));
    let leaks = nach_der_pipeline(&bytes);
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// Gegenprobe: ein `/ExtGState` ohne `/Font` lässt die Schrift stehen, und
/// `q`/`Q` stellt die Schrift vor dem `gs` wieder her.
#[test]
fn ohne_font_eintrag_und_ueber_q_bleibt_die_schrift() {
    let bytes = dokument(&format!(
        "BT /F1 12 Tf /GS0 gs 72 700 Td (IBAN: {SECRET}) Tj ET \
         q BT /F1 12 Tf q /GS1 gs Q 72 680 Td (IBAN: {SECRET}) Tj ET Q"
    ));
    let runs = analyse(&bytes);
    let gefunden = runs.iter().filter(|r| r.text.contains(SECRET)).count();
    assert_eq!(
        gefunden,
        2,
        "{:?}",
        runs.iter().map(|r| &r.text).collect::<Vec<_>>()
    );
    let leaks = nach_der_pipeline(&bytes);
    assert!(leaks.is_empty(), "{leaks:?}");
}
