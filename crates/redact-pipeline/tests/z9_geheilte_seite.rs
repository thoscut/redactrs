//! Eine geheilte MediaBox darf auf keinem der beiden Wege stumm bleiben.
//!
//! # Der Befund (gemessen am gebauten Binary, vor der Änderung)
//!
//! Eine einseitige Datei mit `/MediaBox [0 0 0 0]` und einer IBAN darauf:
//!
//! ```text
//! $ redact-rs entartet.pdf -o entartet_out.pdf -f
//! Seiten:             1
//! Entfernte Zeichen:  28
//! Deck-Rechtecke:     1
//! Ausgabe:            entartet_out.pdf
//! $ echo $?
//! 0
//! ```
//!
//! Keine Warnung, Rückgabewert 0 — Zeile für Zeile dieselbe Ausgabe wie für
//! dieselbe Datei mit `/MediaBox [0 0 595 842]`. Die Oberfläche sagte an
//! derselben Stelle einen Satz dazu (`redact_gui::state::healed_page_warning`,
//! festgehalten in `z8_die_geheilte_seite_warnt_nur_in_der_oberflaeche`). Zwei
//! Wege, eine Wahrheit, und einer davon schwieg.
//!
//! # Warum das Schweigen die falsche Richtung ist
//!
//! Die Ausgabedatei behält `/MediaBox [0 0 0 0]` (nachgesehen in den
//! geschriebenen Bytes), das Deck-Rechteck steht darin bei
//! `105.68 696.36 168.788 13.64 re` — also außerhalb des Kastens, den die Datei
//! selbst als Seite angibt. Ob davon etwas zu sehen ist, entscheidet der
//! Betrachter.
//!
//! Und die Wirkungsprüfung läuft gegen das Ersatzblatt: mit
//! `/MediaBox [0 0 300000 300000]` und Text bei (250000, 250000) meldete
//! derselbe Lauf „Entfernte Zeichen: 28“ **und** „liegen vollständig neben der
//! Seite … der Text der Seite steht unverändert in der Ausgabe“ — zwei
//! Aussagen, die einander widersprechen, bei Rückgabewert 0.
//!
//! Damit trifft der zweite Teil der Definition aus `redact_pipeline::coverage`
//! zu: gelesen wurde die Seite vollständig, **nachgemessen** wurde das Ergebnis
//! dort aber gegen ein erfundenes Blatt. Deshalb Rückgabewert 3.
//!
//! # Die Gegenrichtung
//!
//! Eine Grenze, die gewöhnliche Dateien anfasst, wäre derselbe Fehler.
//! `redact_pdf::document::sane_box` greift erst außerhalb von 1 pt … 200 000 pt
//! oder bei nicht endlichen Werten; [`eine_gewoehnliche_seite_bleibt_stumm`]
//! hält das fest.

use std::path::PathBuf;

use redact_pdf::testing::TextItem;
use redact_pipeline::{healed_page_warnings, Config, Outcome};

/// Die IBAN, an der gemessen wird.
const IBAN: &str = "DE89 3704 0044 0532 0130 00";

/// Ein eigenes Verzeichnis je Test — die Tests laufen nebenläufig.
fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redactrs_z9_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("Arbeitsverzeichnis anlegbar");
    dir
}

/// Baut ein PDF und setzt der Seite `page` die angegebene MediaBox.
///
/// Von Hand über `lopdf`, weil `redact_pdf::testing::build_pdf` immer A4
/// schreibt — und genau die kaputte Angabe ist hier der Prüfgegenstand.
fn pdf_mit_mediabox(seiten: usize, page: usize, media: [f64; 4]) -> Vec<u8> {
    let inhalt: Vec<Vec<TextItem>> = (0..seiten)
        .map(|i| {
            vec![TextItem::new(
                72.0,
                700.0,
                12.0,
                format!("Seite {i}: IBAN {IBAN}"),
            )]
        })
        .collect();
    let bytes = redact_pdf::testing::build_pdf(&inhalt);
    let mut doc = lopdf::Document::load_mem(&bytes).expect("ladbar");
    let ids: Vec<lopdf::ObjectId> = doc.get_pages().values().copied().collect();
    doc.get_object_mut(ids[page])
        .expect("Seite vorhanden")
        .as_dict_mut()
        .expect("Seite ist ein Dictionary")
        .set(
            "MediaBox",
            media
                .iter()
                .map(|v| lopdf::Object::Real(*v as f32))
                .collect::<Vec<_>>(),
        );
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    out
}

/// Schreibt die Bytes und lässt die Kette darüber laufen.
fn lauf(name: &str, bytes: &[u8]) -> Outcome {
    let dir = tmp(name);
    let input = dir.join("ein.pdf");
    std::fs::write(&input, bytes).expect("schreibbar");
    let config = Config {
        input,
        output: Some(dir.join("aus.pdf")),
        force: true,
        ..Config::default()
    };
    redact_pipeline::run(&config).expect("Lauf gelingt")
}

/// Sätze über eine geheilte Seite — an dem Halbsatz erkannt, den
/// `redact_pdf::document::SaneBox::warning` erzeugt.
fn heilungssaetze(outcome: &Outcome) -> Vec<&str> {
    outcome
        .warnings
        .iter()
        .filter(|w| w.contains("Unbrauchbare MediaBox"))
        .map(String::as_str)
        .collect()
}

/// **Der Befund.** Die Kommandozeile schwieg zu `/MediaBox [0 0 0 0]`; jetzt
/// steht der Satz in `warnings` — derselben Liste, aus der das Audit-Log und
/// die Oberfläche lesen.
#[test]
fn die_kette_meldet_die_geheilte_seite() {
    let outcome = lauf("gemeldet", &pdf_mit_mediabox(1, 0, [0.0, 0.0, 0.0, 0.0]));

    let saetze = heilungssaetze(&outcome);
    assert_eq!(
        saetze.len(),
        1,
        "genau ein Satz zur geheilten Seite erwartet: {:?}",
        outcome.warnings
    );
    assert!(
        saetze[0].contains("Seite 1"),
        "die Seite muss beim Namen genannt werden: {}",
        saetze[0]
    );

    // Die Schwärzung selbst bleibt, wie sie war — der Satz kommt hinzu, er
    // ersetzt nichts.
    assert!(
        outcome.removed_glyphs > 0,
        "auf der geheilten Seite wird weiterhin geschwärzt: {outcome:?}"
    );
}

/// **Der Rückgabewert.** `fully_inspected()` ist genau das, was
/// `redact_cli::exit_code_for` liest: `false` ⇒ `EXIT_INCOMPLETE` (3).
#[test]
fn die_geheilte_seite_ist_eine_deckungsluecke() {
    let outcome = lauf("luecke", &pdf_mit_mediabox(1, 0, [0.0, 0.0, 0.0, 0.0]));
    assert!(
        !outcome.fully_inspected(),
        "eine gegen ein Ersatzblatt nachgemessene Seite ist keine geprüfte: {:?}",
        outcome.warnings
    );
    assert_eq!(
        outcome.coverage_gaps().len(),
        1,
        "und zwar genau eine Lücke: {:?}",
        outcome.coverage_gaps()
    );
}

/// **Die Gegenrichtung.** Ein gewöhnliches A4-Blatt löst nichts aus — sonst
/// wäre der Rückgabewert 3 nach der zweiten Datei ein Wert, den man wegdrückt.
#[test]
fn eine_gewoehnliche_seite_bleibt_stumm() {
    let outcome = lauf(
        "stumm",
        &pdf_mit_mediabox(1, 0, [0.0, 0.0, 595.276, 841.89]),
    );
    assert!(
        heilungssaetze(&outcome).is_empty(),
        "kein Wort über eine Seite, die in Ordnung ist: {:?}",
        outcome.warnings
    );
    assert!(
        outcome.fully_inspected(),
        "und Rückgabewert 0: {:?}",
        outcome.warnings
    );
    assert!(outcome.removed_glyphs > 0);
}

/// Die Seitenzahl im Satz ist die aus der Ausgabe (1-basiert) und meint die
/// Seite, die wirklich geheilt wurde — nicht ihre Nachbarin.
#[test]
fn der_satz_nennt_die_richtige_seite() {
    let outcome = lauf("seitenzahl", &pdf_mit_mediabox(3, 1, [0.0, 0.0, 0.0, 0.0]));
    let saetze = heilungssaetze(&outcome);
    assert_eq!(saetze.len(), 1, "eine von drei Seiten: {saetze:?}");
    assert!(
        saetze[0].starts_with("Seite 2:"),
        "geheilt wurde die zweite Seite: {}",
        saetze[0]
    );
}

/// Alle drei Entartungen, die `sane_box` kennt, kommen hier an — und die
/// Rohangabe steht im Satz, damit nachvollziehbar ist, was in der Datei stand.
#[test]
fn ohne_flaeche_zu_klein_und_absurd_gross_werden_alle_gemeldet() {
    for (name, media, spur) in [
        ("ohne_flaeche", [0.0, 0.0, 0.0, 0.0], "(0 x 0)"),
        ("zu_klein", [0.0, 0.0, 0.5, 0.5], "(0.5 x 0.5)"),
        (
            "riesig",
            [0.0, 0.0, 300_000.0, 300_000.0],
            "(300000 x 300000)",
        ),
    ] {
        let outcome = lauf(name, &pdf_mit_mediabox(1, 0, media));
        let saetze = heilungssaetze(&outcome);
        assert_eq!(saetze.len(), 1, "{name}: {:?}", outcome.warnings);
        assert!(
            saetze[0].contains(spur),
            "{name}: die Rohangabe gehört in den Satz — {}",
            saetze[0]
        );
        assert!(!outcome.fully_inspected(), "{name}: Rückgabewert 3");
    }
}

/// **Die zweite Gegenrichtung, und sie war eine Überraschung.**
///
/// `/MediaBox [0 0 -595 -842]` sieht entartet aus, ist es aber nicht:
/// `sane_box` normalisiert zuerst, und übrig bleibt ein gewöhnliches A4-Blatt
/// von (−595, −842) bis (0, 0). Geheilt wird da nichts — die Seite liegt bloß
/// im negativen Quadranten, und ein Rechteck bei (72, 700) liegt wirklich
/// daneben. Genau das meldet der Lauf, und zwar mit dem dafür vorgesehenen
/// Satz.
///
/// Der Test steht hier, weil die naheliegende Lesart („negativ ⇒ kaputt“)
/// falsch ist und die Grenze sonst irgendwann in diese Richtung verschoben
/// würde — und das wäre eine Grenze, die gewöhnliche Dateien anfasst.
#[test]
fn eine_vertauschte_mediabox_wird_normalisiert_und_nicht_geheilt() {
    let outcome = lauf(
        "vertauscht",
        &pdf_mit_mediabox(1, 0, [0.0, 0.0, -595.0, -842.0]),
    );
    assert!(
        heilungssaetze(&outcome).is_empty(),
        "vertauschte Ecken sind kein Heilungsfall: {:?}",
        outcome.warnings
    );
    assert_eq!(
        outcome.off_page_redactions, 1,
        "das Rechteck liegt wirklich neben diesem Blatt: {outcome:?}"
    );
}

/// Der Satz kommt aus **einer** Quelle: `healed_page_warnings` über
/// `SaneBox::warning`. Was die Kette in ihre Warnungen legt, ist wörtlich das,
/// was diese Funktion für dasselbe Dokument liefert — die Oberfläche kann
/// deshalb dieselbe Funktion rufen, statt einen zweiten Satz zu pflegen.
#[test]
fn kette_und_funktion_sagen_denselben_satz() {
    let bytes = pdf_mit_mediabox(1, 0, [0.0, 0.0, 0.0, 0.0]);
    let outcome = lauf("einequelle", &bytes);

    let doc = lopdf::Document::load_mem(&bytes).expect("ladbar");
    let direkt = healed_page_warnings(&redact_pdf::document::sane_page_boxes(&doc));

    assert_eq!(direkt.len(), 1);
    assert!(
        outcome.warnings.contains(&direkt[0]),
        "die Kette sagt etwas anderes als die Funktion:\nKette: {:?}\nFunktion: {:?}",
        outcome.warnings,
        direkt
    );
}
