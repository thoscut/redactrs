//! Register #106 (bei #94 gefunden, Spur-A-Runde 2): ein Inhaltsstrom oder
//! ein Formular, das sich viele Seiten teilen.
//!
//! Das Aufwandskonto galt je Seite und schrieb jeden Strom gut, den die Seite
//! zum ersten Mal las — auch wenn eine frühere Seite ihn schon gelesen hatte.
//! Ein Strom, den P Seiten zeigen, brachte sein Guthaben P-mal ein; die
//! Arbeit wuchs mit Seiten × Inhalt. Gemessen (Release, 200 Seiten × 1 000
//! Zeilen, 32 kB): 14,4 s und 421 MB ohne ein einziges Muster,
//! `--check-leaks` 10,5 s.
//!
//! Jetzt bringt jeder Strom sein Guthaben einmal je Dokument ein. Die Proben:
//! ein geteilter Strom und ein geteiltes Formular werden abgelehnt — im
//! Extraktor, im Redaktor und in der Nachprüfung als „nicht geprüft“ —,
//! während eine Datei mit eigenem Inhalt je Seite und einem geteilten
//! Briefkopf durchläuft.

mod common;

use common::SECRET;
use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfExtractor, PdfRedactor};

/// Wie jeder Test hier geteilten Inhalt baut.
#[derive(Clone, Copy)]
enum Bauart {
    /// Alle Seiten nennen denselben Inhaltsstrom.
    Strom,
    /// Jede Seite zeichnet dasselbe Formular.
    Formular,
    /// Jede Seite hat eigenen Inhalt und zeichnet dazu einen geteilten
    /// Briefkopf — das, was echte Kontoauszüge tun.
    Briefkopf,
}

fn zeilen(anzahl: usize, seite: usize) -> Vec<u8> {
    let mut body = Vec::new();
    for i in 0..anzahl {
        body.extend_from_slice(
            format!(
                "BT /F1 8 Tf 40 {} Td (Seite {seite} Zeile {i} Kunde {SECRET}) Tj ET\n",
                800 - (i % 88) * 9
            )
            .as_bytes(),
        );
    }
    body
}

fn dokument(seiten: usize, zeilen_je_seite: usize, bauart: Bauart) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let fonts = dictionary! { "F1" => font };
    let formular = |doc: &mut Document, body: Vec<u8>| -> ObjectId {
        doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "Font" => fonts.clone() },
            },
            body,
        ))
    };
    let geteilt_strom = doc.add_object(Stream::new(dictionary! {}, zeilen(zeilen_je_seite, 0)));
    let geteilt_form = formular(&mut doc, zeilen(zeilen_je_seite, 0));
    let briefkopf = formular(&mut doc, zeilen(5, 0));
    let tree = doc.new_object_id();
    let mut ids = Vec::new();
    for k in 0..seiten {
        let (contents, xobjects) = match bauart {
            Bauart::Strom => (geteilt_strom, dictionary! {}),
            Bauart::Formular => (
                doc.add_object(Stream::new(dictionary! {}, b"q /Fm0 Do Q\n".to_vec())),
                dictionary! { "Fm0" => geteilt_form },
            ),
            Bauart::Briefkopf => {
                let mut body = b"q /Kopf Do Q\n".to_vec();
                body.extend_from_slice(&zeilen(zeilen_je_seite, k));
                (
                    doc.add_object(Stream::new(dictionary! {}, body)),
                    dictionary! { "Kopf" => briefkopf },
                )
            }
        };
        ids.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => tree,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => dictionary! { "Font" => fonts.clone(), "XObject" => xobjects },
            "Contents" => contents,
        }));
    }
    doc.objects.insert(
        tree,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
            "Count" => seiten as i64,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
    doc.trailer.set("Root", catalog);
    save_to_bytes(&doc).expect("Speichern")
}

#[track_caller]
fn abgelehnt(bytes: &[u8], was: &str) {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    let err = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect_err(&format!("{was}: der Extraktor muss ablehnen"));
    assert!(
        err.to_string().contains("denselben Inhalt vielfach"),
        "{was}: {err}"
    );
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let redaction = Redaction::new(
        Region::new(
            0,
            Rect::new(40.0, 790.0, 300.0, 810.0),
            None,
            Source::Manual {
                reason: "Vervielfachung".into(),
            },
        ),
        Action::Blackout,
    );
    let err = PdfRedactor::new()
        .apply_with_report(&mut doc, &[redaction])
        .expect_err(&format!("{was}: der Redaktor muss ablehnen"));
    assert!(
        err.to_string().contains("denselben Inhalt vielfach"),
        "{was}: {err}"
    );
}

/// 200 Seiten, ein Inhaltsstrom mit 1 000 Zeilen — die Probe aus der
/// Messung.
#[test]
fn ein_strom_auf_vielen_seiten_wird_abgelehnt() {
    abgelehnt(&dokument(200, 1_000, Bauart::Strom), "geteilter Strom");
}

/// Dasselbe mit einem Formular, das jede Seite zeichnet.
#[test]
fn ein_formular_auf_vielen_seiten_wird_abgelehnt() {
    abgelehnt(
        &dokument(200, 1_000, Bauart::Formular),
        "geteiltes Formular",
    );
}

/// Die Nachprüfung lehnt die Datei nicht ab, sie sagt, was sie nicht lesen
/// konnte: die übrigen Seiten stehen als „nicht geprüft“ im Bericht, das
/// Ergebnis ist keine Entwarnung.
#[test]
fn die_nachpruefung_nennt_die_nicht_gelesenen_seiten() {
    let bytes = dokument(200, 1_000, Bauart::Strom);
    let hits = leaks(&bytes, SECRET);
    assert!(!hits.is_empty(), "das Geheimnis steht roh in der Datei");
    let report = redact_pdf::leaks_many_within(&bytes, &[SECRET], u64::MAX);
    assert!(
        report
            .unchecked
            .iter()
            .any(|line| line.contains("denselben Inhalt vielfach")),
        "{:?}",
        report.unchecked
    );
}

/// Gegenprobe: eigener Inhalt je Seite und ein geteilter Briefkopf — die Form
/// echter Kontoauszüge — läuft durch, auch über viele Seiten.
#[test]
fn eigener_inhalt_mit_geteiltem_briefkopf_laeuft_durch() {
    let bytes = dokument(300, 40, Bauart::Briefkopf);
    let doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let (runs, _) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("eigener Inhalt je Seite wird gelesen");
    let seiten: std::collections::BTreeSet<usize> = runs.iter().map(|r| r.page).collect();
    assert_eq!(seiten.len(), 300, "jede Seite liefert Text");
}

/// Gegenprobe am Rand: wenige Seiten, die denselben Strom zeigen (ein
/// Deckblatt, das zweimal vorkommt), bleiben weit unter der Decke.
#[test]
fn wenige_seiten_mit_demselben_strom_laufen_durch() {
    let bytes = dokument(3, 1_000, Bauart::Strom);
    let doc = load_from_bytes(&bytes).expect("PDF ladbar");
    PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("drei Seiten desselben Inhalts");
}
