//! Was über das **gelesene Dokument** gesagt wird, gilt auf beiden Aufrufarten.
//!
//! # Der Befund (gemessen am gebauten Binary, vor der Änderung)
//!
//! Die vorige Runde zog die Warnung zur geheilten Seite in die gemeinsame
//! Hälfte (`redact_pipeline::apply`). `--review` kommt dort nie an: der
//! Review-Zweig in `run` kehrt mit `return Ok(outcome)` zurück, **bevor**
//! `apply` gerufen wird.
//!
//! ```text
//! $ redact-rs entartet.pdf --review --review-out r_ent.json   # MediaBox [0 0 0 0]
//! Eingabe:            entartet.pdf
//! Seiten:             1
//! Textzeilen:         1
//! Treffer gesamt:     1
//! Review geschrieben: r_ent.json
//! $ echo $?
//! 0
//! ```
//!
//! Zeile für Zeile dasselbe wie für dieselbe Datei mit `/MediaBox [0 0 595
//! 842]`, beide Rückgabewert 0 — und in der Review-Datei stand
//! `{ll:(100.9,697.8), ur:(239.89,707.5)}`, also A4-Koordinaten für ein Blatt,
//! das sich selbst als 0 x 0 angibt. Ausgerechnet die Aufrufart, bei der jemand
//! die Koordinaten von Hand prüfen soll.
//!
//! # Die Klasse, nicht der eine Satz
//!
//! Drei Zeilen über jenem `return` steht die Begründung, warum
//! `detection_notice` in den Review-Zweig dupliziert wurde: „`--review` ist die
//! Stelle, an der jemand die Trefferliste prüft“. Dasselbe Argument trägt für
//! jede Aussage über das **gelesene Dokument**. Aussagen über das **Ergebnis
//! der Schwärzung** (Wirkungsprüfung, Bildkodierung) gibt es ohne Schwärzung
//! nicht und fehlen dem Review-Weg zu Recht.
//!
//! Gemessen wurden drei Aussagen, die auf dem Review-Weg fehlten; zwei davon
//! entstehen in `redact-pdf` und sind hier nur festgehalten, siehe
//! [`was_dem_review_weg_weiterhin_fehlt`].

use std::path::PathBuf;

use lopdf::dictionary;
use redact_pdf::testing::TextItem;
use redact_pipeline::{Config, Outcome};

/// Die IBAN, an der gemessen wird.
const IBAN: &str = "DE89 3704 0044 0532 0130 00";

/// Ein eigenes Verzeichnis je Test — die Tests laufen nebenläufig.
fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redactrs_zar_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("Arbeitsverzeichnis anlegbar");
    dir
}

/// Baut ein einseitiges PDF mit der angegebenen MediaBox.
///
/// Von Hand über `lopdf`, weil `redact_pdf::testing::build_pdf` immer A4
/// schreibt — und genau die kaputte Angabe ist hier der Prüfgegenstand.
fn pdf_mit_mediabox(media: [f64; 4]) -> Vec<u8> {
    pdf_mit_mediabox_und_text(media, 72.0, 700.0)
}

/// Wie [`pdf_mit_mediabox`], aber mit frei wählbarer Textstelle — für die
/// Fälle, in denen das Rechteck neben dem Ersatzblatt liegen soll.
fn pdf_mit_mediabox_und_text(media: [f64; 4], x: f64, y: f64) -> Vec<u8> {
    let bytes =
        redact_pdf::testing::build_pdf(&[vec![TextItem::new(x, y, 12.0, format!("IBAN {IBAN}"))]]);
    let mut doc = lopdf::Document::load_mem(&bytes).expect("ladbar");
    let ids: Vec<lopdf::ObjectId> = doc.get_pages().values().copied().collect();
    doc.get_object_mut(ids[0])
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

/// Schreibt die Bytes und lässt die Kette darüber laufen — als Review-Lauf
/// oder als gewöhnlicher Lauf mit Ausgabedatei.
fn lauf(name: &str, bytes: &[u8], review: bool) -> Outcome {
    let dir = tmp(name);
    let input = dir.join("ein.pdf");
    std::fs::write(&input, bytes).expect("schreibbar");
    let config = Config {
        input,
        output: if review {
            None
        } else {
            Some(dir.join("aus.pdf"))
        },
        review,
        review_out: if review {
            Some(dir.join("review.json"))
        } else {
            None
        },
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

/// **Der Befund.** `--review` schwieg zu `/MediaBox [0 0 0 0]`.
#[test]
fn der_review_weg_meldet_die_geheilte_seite() {
    let outcome = lauf("review", &pdf_mit_mediabox([0.0, 0.0, 0.0, 0.0]), true);

    assert!(
        outcome.review_out.is_some(),
        "das ist der Review-Weg: {outcome:?}"
    );
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
}

/// **Der Rückgabewert.** `fully_inspected()` ist genau das, was
/// `redact_cli::exit_code_for` liest: `false` ⇒ Rückgabewert 3. Die Koordinaten
/// in der Review-Datei sind gegen ein erfundenes Blatt gerechnet — wer sie
/// prüfen soll, muss das erfahren, und ein Skript muss es am Rückgabewert
/// sehen können.
#[test]
fn der_review_weg_endet_bei_einer_geheilten_seite_auf_drei() {
    let outcome = lauf("rueckgabe", &pdf_mit_mediabox([0.0, 0.0, 0.0, 0.0]), true);
    assert!(
        !outcome.fully_inspected(),
        "eine gegen ein Ersatzblatt gerechnete Trefferliste ist keine geprüfte: {:?}",
        outcome.warnings
    );
    assert_eq!(
        outcome.coverage_gaps().len(),
        1,
        "und zwar genau eine Lücke: {:?}",
        outcome.coverage_gaps()
    );
}

/// **Beide Wege, ein Wortlaut.** Nicht „auch der Review-Weg sagt irgendetwas“,
/// sondern: er sagt **dasselbe**. Ein zweiter, eigener Satz wäre dieselbe
/// Divergenz, nur an einer anderen Stelle.
#[test]
fn review_und_schwaerzen_sagen_ueber_das_dokument_dasselbe() {
    let bytes = pdf_mit_mediabox([0.0, 0.0, 0.0, 0.0]);
    let review = lauf("vgl_review", &bytes, true);
    let apply = lauf("vgl_apply", &bytes, false);

    assert_eq!(
        heilungssaetze(&review),
        heilungssaetze(&apply),
        "die beiden Aufrufarten sagen über dieselbe Datei Verschiedenes"
    );
    assert_eq!(heilungssaetze(&review).len(), 1);
}

/// **Die Gegenrichtung.** Ein gewöhnliches A4-Blatt löst auf dem Review-Weg
/// genauso wenig aus wie auf dem anderen — sonst wäre der Rückgabewert 3 nach
/// der zweiten Datei ein Wert, den man wegdrückt.
#[test]
fn ein_gewoehnliches_blatt_bleibt_auch_im_review_stumm() {
    let outcome = lauf(
        "stumm",
        &pdf_mit_mediabox([0.0, 0.0, 595.276, 841.89]),
        true,
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
    assert!(
        outcome.candidates > 0,
        "die Trefferliste steht trotzdem: {outcome:?}"
    );
}

/// **Die Trennlinie**, festgehalten: was die Schwärzung *misst*, darf dem
/// Review-Weg fehlen.
///
/// `/MediaBox [0 0 300000 300000]` mit Text bei (250000, 250000): auf dem
/// Schwärzungsweg kommt zur Heilungsmeldung die Wirkungsprüfung hinzu („liegen
/// vollständig neben der Seite“, gerechnet gegen das Ersatzblatt A4). Ohne
/// Schwärzung gibt es diese Messung nicht — der Review-Weg sagt deshalb genau
/// den einen Satz über das Dokument und keinen über ein Ergebnis, das es noch
/// nicht gibt.
#[test]
fn die_wirkungspruefung_bleibt_dem_schwaerzungsweg_vorbehalten() {
    let bytes = pdf_mit_mediabox_und_text([0.0, 0.0, 300_000.0, 300_000.0], 250_000.0, 250_000.0);
    let review = lauf("grenze_review", &bytes, true);
    let apply = lauf("grenze_apply", &bytes, false);

    let neben = |o: &Outcome| {
        o.warnings
            .iter()
            .filter(|w| w.contains("liegen vollständig neben der Seite"))
            .count()
    };
    assert_eq!(neben(&review), 0, "ohne Schwärzung nichts zu messen");
    assert_eq!(neben(&apply), 1, "mit Schwärzung gemessen: {apply:?}");
    assert_eq!(heilungssaetze(&review).len(), 1);
    assert_eq!(heilungssaetze(&apply).len(), 1);
}

/// Hängt der Seite ein (winziges) Rasterbild in die Ressourcen.
///
/// Mehr braucht `redact_pdf::image::page_has_images` nicht: es fragt die
/// Ressourcen der Seite nach einem XObject mit `/Subtype /Image`.
fn pdf_mit_rasterbild() -> Vec<u8> {
    let bytes = redact_pdf::testing::build_pdf(&[vec![TextItem::new(
        72.0,
        700.0,
        12.0,
        format!("IBAN {IBAN}"),
    )]]);
    let mut doc = lopdf::Document::load_mem(&bytes).expect("ladbar");
    let bild = doc.add_object(lopdf::Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 1,
            "Height" => 1,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
        },
        vec![0u8],
    ));
    let seite = *doc.get_pages().values().next().expect("eine Seite");
    let ressourcen = doc
        .get_dictionary(seite)
        .expect("Seiten-Dictionary")
        .get(b"Resources")
        .expect("Ressourcen")
        .as_reference()
        .expect("Ressourcen sind ein eigenes Objekt");
    doc.get_object_mut(ressourcen)
        .expect("Ressourcen vorhanden")
        .as_dict_mut()
        .expect("Ressourcen sind ein Dictionary")
        .set("XObject", dictionary! { "Im0" => bild });
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    out
}

/// **Was dem Review-Weg weiterhin fehlt** — gemessen, nicht behoben.
///
/// Zwei weitere Aussagen über das gelesene Dokument entstehen erst in
/// `redact_pdf::PdfRedactor::apply_with_report` und damit nur auf dem
/// Schwärzungsweg:
///
/// * `warn_about_images` (`redact-pdf/src/redact.rs:1508`) — „n von m Seite(n)
///   enthalten Rasterbilder … ohne OCR nicht“; hier gemessen;
/// * die Meldung über inkrementelle Revisionen
///   (`redact-pdf/src/redact.rs:344`). Gemessen am gebauten Binary über eine
///   Datei mit `/Prev` im Trailer: `--review --json` lieferte
///   `"warnings": []`, der Schwärzungslauf die Meldung.
///
/// Beide lesen allein das Dokument und gehörten nach derselben Trennlinie auf
/// beide Wege. Ihr Wortlaut steht aber in `redact-pdf`, und ihn hier ein
/// zweites Mal hinzuschreiben wäre genau die Divergenz, gegen die dieses Crate
/// gebaut ist. Der Test hält den Stand fest, damit die offene Hälfte nicht
/// unbemerkt bleibt: **schlägt er fehl, ist die Lücke geschlossen** und die
/// Zusicherung gehört umgedreht.
#[test]
fn was_dem_review_weg_weiterhin_fehlt() {
    let bytes = pdf_mit_rasterbild();
    let review = lauf("fehlt_review", &bytes, true);
    let apply = lauf("fehlt_apply", &bytes, false);

    let raster = |o: &Outcome| {
        o.warnings
            .iter()
            .filter(|w| w.contains("enthalten Rasterbilder"))
            .count()
    };
    assert_eq!(
        raster(&apply),
        1,
        "der Schwärzungsweg sagt es: {:?}",
        apply.warnings
    );
    assert_eq!(
        raster(&review),
        0,
        "wenn das hier fehlschlägt, ist die zweite Hälfte der Klasse geschlossen — \
         dann gehört diese Zusicherung umgedreht: {:?}",
        review.warnings
    );
}

/// **Die Begründung, die nicht trug** (Auftrag D), als Messung festgehalten.
///
/// Der Rückgabewert 3 für eine geheilte Seite stützte sich im Code auf den
/// Widerspruch zwischen „Entfernte Zeichen: 28“ und „der Text der Seite steht
/// unverändert in der Ausgabe“. Derselbe Widerspruch entsteht **ohne jede
/// Heilung**: `/MediaBox [0 0 -595 -842]` wird von
/// `redact_pdf::document::sane_box` zu einem gewöhnlichen A4-Blatt im negativen
/// Quadranten normalisiert, das Rechteck bei (72, 700) liegt wirklich daneben —
/// und trotzdem entfernt die Schwärzung ihre Zeichen aus dem Content-Stream.
/// Beides zugleich, bei Rückgabewert 0.
///
/// Der Test steht hier, damit die alte Begründung nicht noch einmal
/// hergeleitet wird: dieser Widerspruch gehört dem Wortlaut der
/// Off-Page-Warnung, nicht der Heilung.
#[test]
fn der_widerspruch_entsteht_auch_ohne_heilung() {
    let outcome = lauf(
        "ohne_heilung",
        &pdf_mit_mediabox([0.0, 0.0, -595.0, -842.0]),
        false,
    );

    assert!(
        heilungssaetze(&outcome).is_empty(),
        "vertauschte Ecken sind kein Heilungsfall: {:?}",
        outcome.warnings
    );
    assert!(
        outcome.removed_glyphs > 0,
        "Zeichen wurden entfernt: {outcome:?}"
    );
    assert!(
        outcome
            .warnings
            .iter()
            .any(|w| w.contains("der Text der Seite steht unverändert in der Ausgabe")),
        "und im selben Lauf steht das Gegenteil: {:?}",
        outcome.warnings
    );
    assert!(
        outcome.fully_inspected(),
        "trotzdem Rückgabewert 0 — der Widerspruch allein trägt die 3 nicht: {:?}",
        outcome.warnings
    );
}
