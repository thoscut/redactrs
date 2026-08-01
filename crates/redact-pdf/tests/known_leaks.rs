//! Regressionstests für die im Audit belegten Lecks.
//!
//! Aufbau, damit die Suite grün bleibt **und** die Defekte dokumentiert sind:
//!
//! * Der eigentliche Test formuliert das **gewünschte** Verhalten („das
//!   Geheimnis steht nach der Schwärzung nirgends mehr in der Datei“). Solange
//!   der Defekt besteht, schlägt er fehl und trägt deshalb ein `#[ignore]` mit
//!   der Aufgabennummer. Ist der Defekt behoben, genügt es, das `#[ignore]`
//!   zu streichen — die Zusicherung wurde nicht abgeschwächt.
//! * Dazu gehört jeweils ein **Kanarienvogel**, der ohne `#[ignore]` läuft und
//!   den *aktuellen* Zustand festhält: er verlangt, dass [`leaks`] das
//!   Geheimnis findet. Sobald der Defekt behoben ist, schlägt der Kanarienvogel
//!   fehl und zeigt an, welches `#[ignore]` jetzt weg kann. Ohne ihn würden
//!   die ignorierten Tests still veralten.

mod common;

use common::SECRET;
use redact_core::{Action, Extractor, Rect, Redaction, Redactor, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

// ---------------------------------------------------------------------------
// Werkzeug
// ---------------------------------------------------------------------------

fn extract(bytes: &[u8]) -> Vec<TextRun> {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new().extract(&doc).expect("Extraktion")
}

/// Die vollständige Verarbeitung, so wie das Werkzeug sie fährt.
fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> Vec<u8> {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply(&mut doc, redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    save_to_bytes(&doc).expect("Speichern")
}

/// Eine manuelle Schwärzung über einen Bereich — unabhängig davon, ob die
/// Analyse dort Text gefunden hat.
fn manual(page: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "Audit".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Deckt die beiden Textzeilen ab, die [`common::page`] setzt (y = 700 und 685).
fn whole_text_area() -> Redaction {
    manual(0, Rect::new(40.0, 600.0, 560.0, 760.0))
}

/// Schwärzung für ein gefundenes Textstück; `None`, wenn die Analyse es gar
/// nicht sieht.
fn redaction_for(runs: &[TextRun], needle: &str) -> Option<Redaction> {
    runs.iter().find_map(|run| {
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
    })
}

#[track_caller]
fn assert_no_leak(bytes: &[u8], what: &str) {
    let hits = leaks(bytes, SECRET);
    assert!(
        hits.is_empty(),
        "{what}: „{SECRET}“ steht noch {} mal in der Ausgabe:\n{}",
        hits.len(),
        hits.join("\n")
    );
}

#[track_caller]
fn assert_still_leaking(bytes: &[u8], hint: &str) {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "Der Defekt scheint behoben ({hint}). Dann bitte das zugehörige \
         #[ignore] entfernen und diesen Kanarienvogel löschen."
    );
}

// ---------------------------------------------------------------------------
// #25 — Inline-Bilder zerreißen den Content-Stream
// ---------------------------------------------------------------------------
//
// Ursache war: `lopdf::content::Content::decode` kennt keine Inline-Bilder
// (`BI … ID … EI`). Der Parser verliert alles hinter dem `ID`, deshalb wurde
// der Text dahinter weder gefunden noch beim Neuschreiben wieder ausgegeben —
// und ein `Do` auf ein Form-XObject dahinter wurde nie ausgeführt.
//
// Behoben: **beide** Pfade dekodieren inzwischen mit `crate::ops::decode_content`,
// der Schreibpfad (`redact.rs`) wie der Lesepfad (`content.rs`). Die Tests
// laufen deshalb wieder als Zusicherung; ihre Kanarienvögel sind entfallen.

#[test]
fn text_after_an_inline_image_is_still_found_by_the_extractor() {
    let pdf = common::inline_image_before_text(SECRET);
    let runs = extract(&pdf);
    let joined = runs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("|");
    assert!(
        joined.contains(SECRET),
        "Text hinter dem Inline-Bild ist unsichtbar für die Analyse: {joined:?}"
    );
}

/// Das Geheimnis ist weg, **weil** geschwärzt wurde — nicht mehr, weil die
/// Seite dabei kaputtgeht. Die Gegenprobe dazu steht in
/// [`unrelated_text_after_an_inline_image_survives_the_rewrite`].
#[test]
fn redacting_a_page_with_an_inline_image_removes_the_secret() {
    let pdf = common::inline_image_before_text(SECRET);
    let out = pipeline(&pdf, &[whole_text_area()]);
    assert_no_leak(&out, "Inline-Bild vor Text");
}

#[test]
fn unrelated_text_after_an_inline_image_survives_the_rewrite() {
    let pdf = common::inline_image_before_text(SECRET);
    // Geschwärzt wird nur die IBAN-Zeile bei y≈685.
    let out = pipeline(&pdf, &[manual(0, Rect::new(40.0, 678.0, 560.0, 696.0))]);
    let text = extract(&out)
        .iter()
        .map(|r| r.text.clone())
        .collect::<Vec<_>>()
        .join("|");
    assert!(
        text.contains("Kontoinhaber"),
        "unbeteiligter Text hinter dem Inline-Bild wurde mitgelöscht: {text:?}"
    );
}

#[test]
fn form_xobject_text_is_redacted_when_nothing_blocks_the_parser() {
    // Gegenprobe: ohne Inline-Bild funktioniert der XObject-Pfad.
    let pdf = common::form_xobject(SECRET, false);
    let runs = extract(&pdf);
    let redaction = redaction_for(&runs, SECRET).expect("IBAN im XObject gefunden");
    let out = pipeline(&pdf, &[redaction]);
    assert_no_leak(&out, "Form-XObject ohne Inline-Bild");
}

/// Behoben: der Text im XObject wird auch dann geschwärzt, wenn ein
/// Inline-Bild den Seiten-Stream zerreißt und das `Do` nie ausgeführt wird.
#[test]
fn iban_inside_a_form_xobject_is_redacted_despite_an_inline_image() {
    let pdf = common::form_xobject(SECRET, true);
    let out = pipeline(&pdf, &[whole_text_area()]);
    assert_no_leak(&out, "Form-XObject hinter Inline-Bild");
}

// ---------------------------------------------------------------------------
// #26 — verwaiste Objekte werden von `save_to` wortwörtlich mitgeschrieben
// ---------------------------------------------------------------------------
//
// Ursache war: an mehreren Stellen wurde nur die *Referenz* entfernt, nicht das
// Objekt — `remove_annotations` löschte die Annotation, nicht deren
// `/AP`-Stream; `strip_metadata` entfernte `/Metadata` aus dem
// Seiten-Dictionary, aber nie das XMP-Objekt; vom Struct-Tree wurde nur die
// Wurzel gelöscht, nicht ihre Kinder. `lopdf::Document::save_to` schrieb
// anschließend alles mit, was in `doc.objects` stand.
//
// Inzwischen werden unerreichbare Objekte vor dem Schreiben entfernt; die
// folgenden Tests laufen deshalb wieder als Zusicherung. Offen bleibt der Fall,
// in dem das Objekt **erreichbar** ist: eine Annotation außerhalb des
// Schwärzungsbereichs behält ihren Appearance-Stream samt Geheimnis.

#[test]
fn appearance_stream_of_a_removed_annotation_is_gone() {
    let pdf = common::annotation_appearance(SECRET, true);
    let runs = extract(&pdf);
    let redaction = redaction_for(&runs, SECRET).expect("IBAN auf der Seite gefunden");
    let out = pipeline(&pdf, &[redaction]);
    assert_no_leak(&out, "/AP der entfernten Annotation");
}

#[test]
#[ignore = "bekannter Leak, siehe Aufgabe #26"]
fn appearance_stream_outside_the_redaction_is_also_cleaned() {
    // Die Annotation überlappt die Schwärzung nicht und bleibt deshalb stehen —
    // ihr Appearance-Stream trägt das Geheimnis trotzdem weiter.
    let pdf = common::annotation_appearance(SECRET, false);
    let runs = extract(&pdf);
    let redaction = redaction_for(&runs, SECRET).expect("IBAN auf der Seite gefunden");
    let out = pipeline(&pdf, &[redaction]);
    assert_no_leak(&out, "/AP einer nicht überlappenden Annotation");
}

#[test]
fn canary_appearance_stream_outside_the_redaction_still_leaks() {
    let pdf = common::annotation_appearance(SECRET, false);
    let runs = extract(&pdf);
    let redaction = redaction_for(&runs, SECRET).expect("IBAN auf der Seite gefunden");
    let out = pipeline(&pdf, &[redaction]);
    assert_still_leaking(&out, "/AP einer nicht überlappenden Annotation");
}

#[test]
fn struct_elem_actual_text_does_not_mirror_the_redacted_text() {
    let pdf = common::struct_elem_actual_text(SECRET);
    let runs = extract(&pdf);
    let redaction = redaction_for(&runs, SECRET).expect("IBAN auf der Seite gefunden");
    let out = pipeline(&pdf, &[redaction]);
    assert_no_leak(&out, "/StructElem /ActualText");
}

#[test]
fn page_level_xmp_metadata_is_removed_not_just_dereferenced() {
    let pdf = common::page_metadata_xmp(SECRET);
    let runs = extract(&pdf);
    let redaction = redaction_for(&runs, SECRET).expect("IBAN auf der Seite gefunden");
    let out = pipeline(&pdf, &[redaction]);
    assert_no_leak(&out, "seitenweites /Metadata");
}

// ---------------------------------------------------------------------------
// #27 — Formularfelder und die Historie inkrementeller Updates
// ---------------------------------------------------------------------------

/// Behoben (Aufgabe #4): `strip_metadata` löscht `/V`, `/DV` und `/RV` in
/// jedem Feld des Formularbaums und entfernt anschließend `/AcroForm`. Vorher
/// wurde nur der sichtbare Text geschwärzt — im Feldwert stand das Geheimnis
/// als UTF-16BE weiter. Die übrigen Restdatenstellen (Dateianhänge,
/// JavaScript, `/OpenAction`, `/AA`, `/OCProperties`) prüft
/// `tests/residual_data.rs`.
#[test]
fn form_field_value_is_redacted_too() {
    let pdf = common::form_field_value(SECRET);
    let runs = extract(&pdf);
    let redaction = redaction_for(&runs, SECRET).expect("IBAN auf der Seite gefunden");
    let out = pipeline(&pdf, &[redaction]);
    assert_no_leak(&out, "AcroForm /V");
}

/// Läuft grün, weil das Werkzeug die Datei komplett neu schreibt statt
/// inkrementell zu speichern. Der Test bleibt stehen: er schlägt zu, sobald
/// jemand auf inkrementelles Schreiben umstellt — dann käme die gesamte
/// Historie ungeschwärzt mit.
#[test]
fn incremental_history_is_dropped_when_the_file_is_rewritten() {
    // Eingabe: `/Prev`-Kette, deren Basisrevision das Geheimnis enthält.
    let pdf = common::incremental_history(SECRET, "XXXX XXXX XXXX XXXX XXXX XX");
    assert!(
        !leaks(&pdf, SECRET).is_empty(),
        "Testdaten taugen nicht: die Historie enthält das Geheimnis gar nicht"
    );

    let runs = extract(&pdf);
    let redaction =
        redaction_for(&runs, "XXXX XXXX XXXX XXXX XXXX XX").expect("aktuelle Revision ist lesbar");
    let out = pipeline(&pdf, &[redaction]);
    assert_no_leak(&out, "Basisrevision einer /Prev-Kette");
}

#[test]
fn objects_unpacked_from_an_object_stream_are_not_carried_over() {
    // `/ObjStm` ist der Container, den eine reine Rohbyte-Suche nicht sieht.
    // `lopdf` packt ihn beim Laden aus; ohne Aufräumen landete sein Inhalt als
    // gewöhnliches — und von niemandem referenziertes — Objekt in der Ausgabe.
    // Aus komprimiert-versteckt wäre also lesbar geworden.
    let pdf = common::object_stream(SECRET);
    let runs = extract(&pdf);
    let redaction = redaction_for(&runs, "Max Mustermann").expect("Text auf der Seite");
    let out = pipeline(&pdf, &[redaction]);
    assert_no_leak(&out, "aus /ObjStm ausgepacktes Objekt");
}
