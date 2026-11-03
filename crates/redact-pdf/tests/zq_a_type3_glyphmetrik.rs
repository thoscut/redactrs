//! Prüfrunde „Inhalt“, Gegenprobe an echten Dateien: eine Type3-Glyphe, die
//! nur ihre Metrik setzt (`d0`/`d1`) und sonst nichts zeichnet.
//!
//! Chrome schreibt das Leerzeichen seiner Type3-Schriften als Glyphprozedur
//! `485 0 0 0 0 0 d1` — ohne einen weiteren Operator. lopdf liest als
//! Operator nur Buchstaben: aus `d1` wurden `d` und ein Operand `1` ohne
//! Operator, die Prozedur galt als „nicht vollständig lesbar“, und **jede**
//! mit Chrome gedruckte Datei endete mit NICHT GEPRÜFT und Rückgabewert 3 —
//! eine gewöhnliche Datei, abgelehnt, obwohl jeder Betrachter sie liest.

mod common;

use lopdf::{dictionary, Dictionary, Object, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{load_from_bytes, save_to_bytes, PdfRedactor, RedactionReport};

/// Eine Seite mit einem Type3-Font, dessen Glyphen `procs` sind; gesetzt
/// werden `codes` (ab `'a'`) bei (72,700).
fn page_with_type3(procs: &[&[u8]], codes: &str) -> Vec<u8> {
    let mut d = common::page(&[]);
    let mut char_procs = Dictionary::new();
    let mut differences: Vec<Object> = vec![Object::Integer(b'a' as i64)];
    for (i, proc_content) in procs.iter().enumerate() {
        let name = format!("g{i}");
        let id = d.add(Object::Stream(
            Stream::new(dictionary! {}, proc_content.to_vec()).with_compression(false),
        ));
        char_procs.set(name.as_bytes().to_vec(), Object::Reference(id));
        differences.push(Object::Name(name.into_bytes()));
    }
    let widths: Vec<Object> = procs.iter().map(|_| Object::Integer(1000)).collect();
    let font_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type3",
        "FontBBox" => vec![0.into(), 0.into(), 1000.into(), 1000.into()],
        "FontMatrix" => vec![
            Object::Real(0.001), Object::Real(0.0), Object::Real(0.0),
            Object::Real(0.001), Object::Real(0.0), Object::Real(0.0),
        ],
        "CharProcs" => char_procs,
        "Encoding" => dictionary! {
            "Type" => "Encoding",
            "Differences" => Object::Array(differences),
        },
        "FirstChar" => b'a' as i64,
        "LastChar" => (b'a' as usize + procs.len() - 1) as i64,
        "Widths" => Object::Array(widths),
    }));
    d.doc
        .get_dictionary_mut(d.resources_id)
        .expect("Resources")
        .set("Font", dictionary! { "T3" => font_id });
    d.set_content(format!("BT\n/T3 10 Tf\n72 700 Td\n({codes}) Tj\nET\n").as_bytes());
    d.finish()
}

fn pipeline(bytes: &[u8]) -> RedactionReport {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let redaction = Redaction::new(
        Region::new(
            0,
            Rect::new(60.0, 690.0, 200.0, 715.0),
            None,
            Source::Manual {
                reason: "Type3-Metrik".into(),
            },
        ),
        Action::Blackout,
    );
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[redaction])
        .expect("Schwärzung");
    save_to_bytes(&doc).expect("Speichern");
    report
}

fn charproc_warnings(report: &RedactionReport) -> Vec<&String> {
    report
        .warnings
        .iter()
        .filter(|w| w.contains("CharProcs"))
        .collect()
}

/// Der Fall aus Chrome: eine Glyphe malt, die andere ist ein Leerzeichen
/// aus nichts als `d1`. Beide werden entfernt, und nichts ist „nicht
/// vollständig lesbar“.
#[test]
fn eine_glyphe_nur_aus_d1_ist_lesbar() {
    let pdf = page_with_type3(
        &[
            b"1000 0 0 0 1000 1000 d1\n0 0 800 800 re f\n",
            b"485 0 0 0 0 0 d1\n\n",
        ],
        "ab",
    );
    let report = pipeline(&pdf);
    assert_eq!(report.removed_glyphs, 2, "{:?}", report.warnings);
    assert!(
        charproc_warnings(&report).is_empty(),
        "Fehlalarm an einer gewöhnlichen Chrome-Glyphe: {:?}",
        report.warnings
    );
}

/// Dasselbe mit `d0`.
#[test]
fn eine_glyphe_nur_aus_d0_ist_lesbar() {
    let pdf = page_with_type3(&[b"500 0 d0"], "a");
    let report = pipeline(&pdf);
    assert!(
        charproc_warnings(&report).is_empty(),
        "Fehlalarm: {:?}",
        report.warnings
    );
}

/// Gegenprobe: eine wirklich abgeschnittene Glyphprozedur bleibt gemeldet.
#[test]
fn eine_kaputte_glyphprozedur_bleibt_gemeldet() {
    let pdf = page_with_type3(&[b"485 0 0 0 0 0 d1\n0 0 m \x01\x02 1 1 l S"], "a");
    let report = pipeline(&pdf);
    assert!(
        !charproc_warnings(&report).is_empty(),
        "die abgeschnittene Prozedur muss gemeldet werden: {:?}",
        report.warnings
    );
}
