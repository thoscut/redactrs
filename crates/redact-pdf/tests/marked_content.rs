//! #71 — `/ActualText` und `/Alt` in Marked Content überleben die Schwärzung.
//!
//! Der Textspiegel eines Marked-Content-Abschnitts (`/Span <</ActualText …>>
//! BDC`) steht als Klartext im **Content-Stream**, nicht in einem Objekt. Die
//! Glyphen werden korrekt entfernt, das Deck-Rechteck sitzt richtig — und
//! `pdftotext` gibt in der Voreinstellung trotzdem die vollständige IBAN aus,
//! weil es den Spiegel bevorzugt.
//!
//! Orakel ist ausschließlich [`redact_pdf::leaks`] — nie der eigene Extraktor:
//! der liest den Spiegel gar nicht und sähe deshalb nichts.

mod common;

use common::{Mirror, SECRET};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfRedactor};

/// Die vollständige Verarbeitung, so wie das Werkzeug sie fährt — samt Bericht.
fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> (redact_pdf::RedactionReport, Vec<u8>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    (report, save_to_bytes(&doc).expect("Speichern"))
}

fn manual(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
            rect,
            None,
            Source::Manual {
                reason: "Audit".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Ein ganzseitiges Rechteck — der Befund gilt ausdrücklich auch dann, wenn die
/// Schwärzung wirklich alles überdeckt.
fn whole_page() -> Redaction {
    manual(Rect::new(0.0, 0.0, 595.0, 842.0))
}

/// Führt eine Variante durch die Schwärzung und prüft das Ergebnis.
///
/// Geprüft wird dreierlei, weil jedes für sich täuschen kann:
/// die Glyphen sind wirklich entfernt (sonst misst der Test die falsche
/// Ursache), es gibt keine Warnung (der Nutzer bekäme sonst wenigstens einen
/// Anhaltspunkt) — und das Geheimnis steht nirgends mehr in der Datei.
#[track_caller]
fn assert_mirror_is_gone(mirror: Mirror, what: &str) {
    let pdf = common::marked_content_mirror(SECRET, mirror);
    assert!(
        !leaks(&pdf, SECRET).is_empty(),
        "{what}: die Testdatei enthält das Geheimnis gar nicht"
    );

    let (report, out) = pipeline(&pdf, &[whole_page()]);
    assert!(
        report.removed_glyphs > 0,
        "{what}: es wurde kein einziges Zeichen entfernt — der Test misst nicht, \
         was er messen soll"
    );
    let hits = leaks(&out, SECRET);
    assert!(
        hits.is_empty(),
        "{what}: „{SECRET}“ steht noch {} mal in der Ausgabe:\n{}",
        hits.len(),
        hits.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Die sechs belegten Varianten
// ---------------------------------------------------------------------------

#[test]
fn span_with_actual_text_in_the_page_stream() {
    assert_mirror_is_gone(Mirror::default(), "/Span <</ActualText (…)>> BDC");
}

#[test]
fn figure_with_alt_text_in_the_page_stream() {
    assert_mirror_is_gone(
        Mirror {
            tag: "Figure",
            key: "Alt",
            ..Mirror::default()
        },
        "/Figure <</Alt (…)>> BDC",
    );
}

#[test]
fn marked_content_point_with_actual_text() {
    assert_mirror_is_gone(
        Mirror {
            operator: "DP",
            ..Mirror::default()
        },
        "/Span <</ActualText (…)>> DP",
    );
}

#[test]
fn actual_text_written_as_a_hex_string() {
    assert_mirror_is_gone(
        Mirror {
            hex: true,
            ..Mirror::default()
        },
        "/ActualText <4445…> (Hex-String)",
    );
}

#[test]
fn actual_text_inside_a_form_xobject() {
    assert_mirror_is_gone(
        Mirror {
            in_form: true,
            ..Mirror::default()
        },
        "/ActualText im Form-XObject",
    );
}

#[test]
fn actual_text_reached_through_resources_properties() {
    assert_mirror_is_gone(
        Mirror {
            via_properties: true,
            ..Mirror::default()
        },
        "BDC /MC0 über /Resources /Properties",
    );
}

/// Steht die Eigenschaftsliste als eigenes Objekt in der Datei, darf ihr
/// `/ActualText` seinerseits ein indirekter Verweis sein. Wer nur auf eine
/// direkt eingetragene Zeichenkette prüft, hält genau diese Datei für
/// unauffällig.
#[test]
fn actual_text_as_an_indirect_reference() {
    assert_mirror_is_gone(
        Mirror {
            via_properties: true,
            indirect_value: true,
            ..Mirror::default()
        },
        "/ActualText 12 0 R in /Properties",
    );
}

/// Auch der Weg über `/Properties` muss im Form-XObject halten — dort steht das
/// `/Properties`-Dictionary in den Ressourcen des Formulars, nicht der Seite.
#[test]
fn actual_text_through_properties_inside_a_form_xobject() {
    assert_mirror_is_gone(
        Mirror {
            via_properties: true,
            in_form: true,
            ..Mirror::default()
        },
        "BDC /MC0 im Form-XObject",
    );
}

// ---------------------------------------------------------------------------
// Gegenprobe
// ---------------------------------------------------------------------------

/// Ein Abschnitt, den **keine** Schwärzung berührt, behält seinen Spiegel.
///
/// Ohne diese Zusicherung wäre die Korrektur wertlos: ein Durchgang, der jeden
/// `/ActualText` löscht, macht getaggte PDFs unbrauchbar (Screenreader,
/// PDF/UA) — und niemand würde es merken, weil das Leck-Orakel schweigt.
///
/// Die Datei trägt drei Spiegel: einen über der IBAN-Zeile, einen über einer
/// Zeile weit darunter und einen als `DP` in einem eigenen Textobjekt. Nur der
/// erste wird berührt. Dass die anderen beiden überleben, wird **nicht** über
/// [`leaks`] geprüft — der fände auch die sichtbaren Glyphen —, sondern direkt
/// im Content-Stream der Ausgabe.
#[test]
fn an_untouched_section_keeps_its_actual_text() {
    const HARMLESS: &str = "Musterbank Filiale Nord";
    let pdf = common::marked_content_two_sections(SECRET, HARMLESS);
    assert_eq!(
        actual_text_values(&pdf),
        vec![SECRET, HARMLESS, HARMLESS],
        "Testdatei trägt nicht die erwarteten drei Spiegel"
    );

    // Geschwärzt wird nur die IBAN-Zeile bei y ≈ 700.
    let (report, out) = pipeline(&pdf, &[manual(Rect::new(40.0, 692.0, 560.0, 712.0))]);
    assert!(report.removed_glyphs > 0, "nichts geschwärzt");

    let hits = leaks(&out, SECRET);
    assert!(
        hits.is_empty(),
        "der berührte Spiegel steht noch {} mal in der Ausgabe:\n{}",
        hits.len(),
        hits.join("\n")
    );
    assert_eq!(
        actual_text_values(&out),
        vec![HARMLESS, HARMLESS],
        "die Spiegel unbeteiligter Abschnitte wurden mitgelöscht"
    );
}

/// Die Werte aller `/ActualText` im Content-Stream der ersten Seite, in
/// Reihenfolge. Bewusst eine reine Textsuche über den entpackten Strom: sie
/// misst, was wirklich in der Datei steht.
fn actual_text_values(pdf: &[u8]) -> Vec<String> {
    let doc = load_from_bytes(pdf).expect("PDF ladbar");
    let page_id = *doc.get_pages().values().next().expect("eine Seite");
    let content = doc.get_page_content(page_id).expect("Content lesbar");
    let text = String::from_utf8_lossy(&content).into_owned();
    text.match_indices("/ActualText")
        .filter_map(|(at, _)| {
            let rest = &text[at..];
            let open = rest.find('(')?;
            let close = rest[open..].find(')')?;
            Some(rest[open + 1..open + close].to_string())
        })
        .collect()
}
