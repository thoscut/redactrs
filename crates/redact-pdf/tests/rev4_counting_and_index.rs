//! Die Buchführung je Region — und die Vorauswahl, an der sie hängt.
//!
//! Zwei Umbauten treffen sich hier:
//!
//! * Die Zeichen, die **eine** Schwärzung verdeckt, standen früher als
//!   `Vec<bool>` über *alle* Zeichen der Textoperation in der Auswahl — je
//!   Bereich einer, und behalten. Jetzt stehen dort zusammenhängende Läufe.
//!   Das ist eine reine Darstellungsfrage: **die Zahlen im Bericht und im
//!   Audit-Log dürfen sich nicht ändern.** Genau das prüfen die Fälle unten,
//!   zeichengenau und mit festen Zahlen statt mit Ungleichungen.
//! * Die Vorauswahl der Bereiche war ein Streifenzug über x und siebte in
//!   einer **Spalte** nichts aus. Jetzt ist es ein Gitter über beide Achsen,
//!   und gefragt wird je Zeichen statt je Textoperation. Eine Vorauswahl darf
//!   schneller werden — verlieren darf sie nichts.
//!
//! Der Maßstab für „nichts verloren“ ist [`redact_pdf::leaks`] an der
//! **fertigen Datei**: wovor der eigene Extraktor blind ist, wird nicht
//! geschwärzt und wäre für ihn trotzdem unsichtbar.

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, save_to_bytes, scan_page, PdfRedactor, RedactionReport};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

/// Code der Ligatur „fi“ im Ligaturfall.
const LIGATURE_CODE: u8 = 0xC8;

// ---------------------------------------------------------------------------
// Werkzeug
// ---------------------------------------------------------------------------

struct Doc {
    doc: Document,
    page_id: ObjectId,
    content_id: ObjectId,
    resources_id: ObjectId,
}

impl Doc {
    fn new() -> Self {
        let mut doc = Document::with_version("1.7");
        let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        let resources_id = doc.add_object(dictionary! {});
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
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
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        Self {
            doc,
            page_id,
            content_id,
            resources_id,
        }
    }

    fn add(&mut self, object: impl Into<Object>) -> ObjectId {
        self.doc.add_object(object)
    }

    fn set_content(&mut self, raw: impl AsRef<[u8]>) {
        let stream = self
            .doc
            .get_object_mut(self.content_id)
            .expect("Strom")
            .as_stream_mut()
            .expect("Strom");
        stream.set_plain_content(raw.as_ref().to_vec());
    }

    fn set_resources(&mut self, dict: lopdf::Dictionary) {
        self.doc
            .objects
            .insert(self.resources_id, Object::Dictionary(dict));
    }

    fn bytes(&self) -> Vec<u8> {
        save_to_bytes(&self.doc).expect("speicherbar")
    }
}

fn helvetica(doc: &mut Doc) -> ObjectId {
    doc.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    })
}

/// Eine Schrift, in der jedes Zeichen von `A` bis `Z` genau eine Geviertbreite
/// hat. Bei 10 pt belegt Zeichen `k` damit exakt `[10k, 10k+10]` — die
/// Geometrie im Formularfall lässt sich so von Hand nachrechnen.
fn monospace(doc: &mut Doc) -> ObjectId {
    doc.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
        "FirstChar" => 65_i64,
        "LastChar" => 90_i64,
        "Widths" => (65..=90).map(|_| Object::Integer(1000)).collect::<Vec<_>>(),
    })
}

fn blackout(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
            rect,
            None,
            Source::Manual {
                reason: "Prüfung".into(),
            },
        ),
        Action::Blackout,
    )
}

fn apply(doc: &mut Doc, padding: f64, redactions: &[Redaction]) -> RedactionReport {
    PdfRedactor::with_padding(padding)
        .apply_with_report(&mut doc.doc, redactions)
        .expect("schwärzbar")
}

/// Alle Vorkommen des Geheimnisses auf der Seite, als Schwärzungen — die Hülle
/// ist genau die der getroffenen Zeichen.
fn redactions_for_secret(doc: &Doc) -> Vec<Redaction> {
    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    let mut out = Vec::new();
    for record in &scan.shows {
        let glyphs: Vec<_> = record.glyphs().collect();
        let text: String = glyphs.iter().map(|g| g.text.as_str()).collect();
        let mut from = 0usize;
        while let Some(at) = text[from..].find(SECRET) {
            let start = from + at;
            let end = start + SECRET.len();
            let rect = glyphs[start..end]
                .iter()
                .map(|g| g.rect)
                .reduce(|a, b| a.union(&b))
                .expect("nicht leer");
            out.push(blackout(rect));
            from = end;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Punkt 1 — die Zählung je Region, zeichengenau
// ---------------------------------------------------------------------------

/// Feste Zahlen statt Ungleichungen: teilweise überlappende Bereiche auf
/// **einer** Textoperation.
///
/// Die Zeile ist `ABCDEFGHIJKLMNOPQRST` in der Geviertschrift, Zeichen `k`
/// liegt also bei `[10k, 10k+10]`. Drei Bereiche:
///
/// * `[0, 50]` → A B C D E (5)
/// * `[30, 90]` → D E F G H I (6)
/// * `[0, 200]` → alle (20)
///
/// Vereinigung ist die ganze Zeile, entfernt werden also 20 Zeichen — die
/// Summe der drei Zahlen ist mit 31 größer, und das ist Absicht: die Frage
/// „hat *dieser* Bereich etwas bewirkt?“ ist nur so zu beantworten.
///
/// **Mutationsnachweis:** `HiddenRuns::count` als `self.0.len()` → `[1, 1, 1]`
/// statt `[5, 6, 20]`.
#[test]
fn teilweise_ueberlappende_bereiche_zaehlen_zeichengenau() {
    let mut doc = Doc::new();
    let font = monospace(&mut doc);
    doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
    doc.set_content("BT /F1 10 Tf 1 0 0 1 0 700 Tm (ABCDEFGHIJKLMNOPQRST) Tj ET\n");

    let band = |x0: f64, x1: f64| blackout(Rect::new(x0, 695.0, x1, 715.0));
    let report = apply(
        &mut doc,
        0.0,
        &[band(0.0, 50.0), band(30.0, 90.0), band(0.0, 200.0)],
    );

    assert_eq!(report.per_redaction, vec![5, 6, 20]);
    assert_eq!(report.removed_glyphs, 20);
    assert_eq!(report.per_redaction.iter().sum::<usize>(), 31);
}

/// Dieselbe Frage über mehrere Zeilen und mit einem Bereich, der **nichts**
/// trifft: eine `0` ist ein Befund und muss eine `0` bleiben.
#[test]
fn eine_wirkungslose_region_bleibt_null_und_die_anderen_zaehlen_genau() {
    let mut doc = Doc::new();
    let font = monospace(&mut doc);
    doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
    doc.set_content(
        "BT /F1 10 Tf 1 0 0 1 0 700 Tm (ABCDEFGHIJ) Tj \
         1 0 0 1 0 680 Tm (KLMNOPQRST) Tj ET\n",
    );

    let report = apply(
        &mut doc,
        0.0,
        &[
            blackout(Rect::new(0.0, 695.0, 40.0, 715.0)),
            blackout(Rect::new(300.0, 300.0, 400.0, 340.0)),
            blackout(Rect::new(50.0, 675.0, 100.0, 695.0)),
        ],
    );
    assert_eq!(report.per_redaction, vec![4, 0, 5]);
    assert_eq!(report.removed_glyphs, 9);
    // Ohne Überlappung muss die Summe aufgehen.
    assert_eq!(
        report.per_redaction.iter().sum::<usize>(),
        report.removed_glyphs
    );
}

/// Eine Ligatur ist **ein** Code für **zwei** Zeichen. Trifft der Bereich nur
/// die zweite Hälfte, verschwindet die ganze Ligatur — und die Region hat
/// dann auch **beide** Zeichen zu verantworten.
///
/// **Mutationsnachweis:** In `code_groups` die innere `while`-Schleife
/// entfernen (jedes Teilzeichen wird eine eigene Gruppe) → `per_redaction`
/// fällt auf `[1]` und die Ligatur überlebt im Strom.
#[test]
fn eine_halb_getroffene_ligatur_zaehlt_ganz() {
    let mut doc = Doc::new();
    let font = doc.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => dictionary! {
            "Type" => "Encoding",
            "BaseEncoding" => "WinAnsiEncoding",
            "Differences" => vec![
                Object::Integer(i64::from(LIGATURE_CODE)),
                Object::Name(b"uni00660069".to_vec()),
            ],
        },
    });
    doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
    doc.set_content(format!(
        "BT /F1 10 Tf 1 0 0 1 72 700 Tm <{:02X}> Tj ET\n",
        LIGATURE_CODE
    ));

    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    let halves: Vec<_> = scan.shows[0].glyphs().collect();
    assert_eq!(halves.len(), 2, "die Ligatur muss zwei Teilzeichen haben");
    assert!(halves[1].bytes.is_empty(), "nur das erste trägt die Bytes");
    let zweite = halves[1].rect;
    drop(scan);

    let report = apply(&mut doc, 0.0, &[blackout(zweite)]);
    assert_eq!(
        report.per_redaction,
        vec![2],
        "die halb getroffene Ligatur zählt ganz"
    );
    assert_eq!(report.removed_glyphs, 2);
}

/// **Der Fall, an dem eine bloße Zahl scheitern würde.**
///
/// Ein Formular mit `ABCDEFGHIJKLMNOPQRST` (Geviertschrift, 10 pt: Zeichen `k`
/// liegt bei `[10k, 10k+10]`), zweimal platziert — einmal bei `x = 0`, einmal
/// bei `x = 30`. **Ein** Bereich `[100, 160]` trifft dadurch in den beiden
/// Platzierungen **verschiedene** Zeichen:
///
/// * Platzierung bei 0: Formularkoordinaten `[100, 160]` → K L M N O P (6)
/// * Platzierung bei 30: Formularkoordinaten `[70, 130]` → H I J K L M (6)
///
/// Der Strom des Formulars wird **einmal** neu geschrieben, es verschwindet
/// die Vereinigung: H bis P, neun Zeichen. Genau diese neun muss die Region
/// auch zugeschrieben bekommen — die Summe (12) zählte doppelt, das Maximum
/// (6) unterschlüge drei Zeichen.
///
/// **Mutationsnachweis:** in `merge_plan` `known.union(&runs)` durch
/// `known.count().max(runs.count())`-Semantik ersetzen (also den Zweig auf
/// „das größere gewinnt“ umstellen) → `per_redaction` fällt auf `[6]`,
/// während `removed_glyphs` bei 9 bleibt.
#[test]
fn ein_zweimal_platziertes_formular_vereinigt_die_zeichen_einer_region() {
    let mut doc = Doc::new();
    let font = monospace(&mut doc);
    let form_resources = doc.add(dictionary! { "Font" => dictionary! { "F1" => font } });
    let form = doc.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), (-5).into(), 300.into(), 15.into()],
                "Resources" => form_resources,
            },
            b"BT /F1 10 Tf 1 0 0 1 0 0 Tm (ABCDEFGHIJKLMNOPQRST) Tj ET\n".to_vec(),
        )
        .with_compression(false),
    ));
    doc.set_resources(dictionary! { "XObject" => dictionary! { "Fx" => form } });
    doc.set_content(
        "q 1 0 0 1 0 700 cm /Fx Do Q\n\
         q 1 0 0 1 30 700 cm /Fx Do Q\n",
    );

    let report = apply(
        &mut doc,
        0.0,
        &[blackout(Rect::new(100.0, 695.0, 160.0, 712.0))],
    );
    assert_eq!(
        report.removed_glyphs, 9,
        "der Strom verliert die Vereinigung H..P"
    );
    assert_eq!(
        report.per_redaction,
        vec![9],
        "die Region hat alle neun Zeichen zu verantworten — nicht 6, nicht 12"
    );
}

// ---------------------------------------------------------------------------
// Punkt 2 — die Vorauswahl darf sieben, aber nichts verlieren
// ---------------------------------------------------------------------------

/// Eine **Spalte** von Treffern: gleiche x-Spanne, verschiedene y. Das ist die
/// Anordnung, in der ein Streifenzug über x nichts aussiebt — und die, bei der
/// ein Gitter am ehesten etwas verlieren könnte.
///
/// Geprüft wird beides: dass jede Region genau ihre Zeichen zählt und dass das
/// Geheimnis danach nicht mehr in der Datei steht.
#[test]
fn eine_spalte_von_treffern_verliert_keine_schwaerzung() {
    for lines in [1usize, 7, 400] {
        let mut doc = Doc::new();
        let font = helvetica(&mut doc);
        doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
        let mut content = String::from("BT /F1 4 Tf\n");
        for line in 0..lines {
            let y = 5.0 + line as f64 * 4.0;
            content.push_str(&format!("1 0 0 1 5 {y} Tm ({SECRET}) Tj\n"));
        }
        content.push_str("ET\n");
        doc.set_content(content);

        let redactions = redactions_for_secret(&doc);
        assert_eq!(redactions.len(), lines);
        let report = apply(&mut doc, 0.0, &redactions);
        assert_eq!(report.removed_glyphs, SECRET.len() * lines);
        assert!(
            report.per_redaction.iter().all(|n| *n == SECRET.len()),
            "{lines} Zeilen: {:?}",
            report.per_redaction
        );
        assert!(
            leaks(&doc.bytes(), SECRET).is_empty(),
            "{lines} Zeilen: das Geheimnis steht noch in der Datei"
        );
    }
}

/// Dieselbe Frage für **eine einzige** lange Textoperation: dort ist die Hülle
/// die ganze Zeile, jeder Bereich berührt sie, und die alte Vorauswahl über
/// Hüllen konnte nichts sparen. Die neue fragt je Zeichen.
#[test]
fn viele_bereiche_in_einer_operation_treffen_weiterhin_jedes_zeichen() {
    for k in [1usize, 5, 400] {
        let mut doc = Doc::new();
        let font = helvetica(&mut doc);
        doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
        let mut text = String::new();
        for _ in 0..k {
            text.push_str(SECRET);
            text.push_str("  ");
        }
        doc.set_content(format!("BT /F1 4 Tf 1 0 0 1 5 400 Tm ({text}) Tj ET\n"));

        let redactions = redactions_for_secret(&doc);
        assert_eq!(redactions.len(), k);
        let report = apply(&mut doc, 0.0, &redactions);
        assert_eq!(report.removed_glyphs, SECRET.len() * k);
        assert!(
            report.per_redaction.iter().all(|n| *n == SECRET.len()),
            "k={k}: {:?}",
            report.per_redaction
        );
        assert!(leaks(&doc.bytes(), SECRET).is_empty(), "k={k}");
    }
}

/// Ein **riesenhaftes** Zeichen bei winzigen Bereichen belegt mehr Gitterzellen
/// als die Abfrage einzeln absucht. Dann liefert das Gitter alles Eingetragene
/// — langsamer, aber vollständig. Ohne diese Rückfallebene bliebe das Zeichen
/// stehen.
#[test]
fn ein_riesiges_zeichen_findet_seinen_winzigen_bereich() {
    let mut doc = Doc::new();
    let font = monospace(&mut doc);
    doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
    // 400 pt hoch: das Zeichen ist um Größenordnungen größer als die Bereiche.
    doc.set_content("BT /F1 400 Tf 1 0 0 1 0 300 Tm (A) Tj ET\n");

    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    let glyph = scan.shows[0].glyphs().next().expect("ein Zeichen").rect;
    drop(scan);
    assert!(
        glyph.ur.x - glyph.ll.x > 300.0,
        "das Zeichen muss riesig sein"
    );

    // Ein winziger Bereich mitten im Zeichen, und viele andere daneben — so
    // viele, dass das Gitter feine Zellen wählt.
    let mitte = glyph.center();
    let mut redactions = vec![blackout(Rect::new(
        mitte.x - 0.25,
        mitte.y - 0.25,
        mitte.x + 0.25,
        mitte.y + 0.25,
    ))];
    for k in 0..200 {
        let x = 500.0 + k as f64 * 0.4;
        redactions.push(blackout(Rect::new(x, 800.0, x + 0.3, 800.3)));
    }

    let report = apply(&mut doc, 0.0, &redactions);
    assert_eq!(
        report.per_redaction[0], 1,
        "der winzige Bereich mitten im Zeichen muss es treffen"
    );
    assert_eq!(report.removed_glyphs, 1);
    assert!(report.per_redaction[1..].iter().all(|n| *n == 0));
}

/// **Warum die Vorauswahl alle Zellen absuchen muss, nicht nur die des
/// Mittelpunkts.**
///
/// Ein Zeichen gilt schon ab einem Viertel Überdeckung als verdeckt. Ein
/// Bereich, der ein Zeichen zu 30 % überdeckt, enthält dessen Mittelpunkt
/// aber nicht — und muss es trotzdem treffen. Ist das Zeichen groß und sind
/// die Bereiche klein (das Zellenmaß richtet sich nach den Bereichen), dann
/// liegt der Mittelpunkt des Zeichens in einer Zelle, die der Bereich gar
/// nicht belegt.
///
/// **Mutationsnachweis:** in `RectIndex::candidates` `touching_into` durch
/// `candidates_covering` ersetzen (also die Mittelpunktsregel von
/// [`redact_core::conflict::RectGrid`] auch hier anwenden) → `per_redaction`
/// fällt auf `[0]`, das Zeichen bleibt stehen.
#[test]
fn ein_bereich_ohne_den_mittelpunkt_trifft_trotzdem() {
    let mut doc = Doc::new();
    let font = monospace(&mut doc);
    doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
    doc.set_content("BT /F1 100 Tf 1 0 0 1 0 400 Tm (A) Tj ET\n");

    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    let glyph = scan.shows[0].glyphs().next().expect("ein Zeichen").rect;
    drop(scan);

    // Genau 30 % der Breite, über die ganze Höhe: über der Schwelle von 25 %,
    // aber ohne den Mittelpunkt.
    let breite = glyph.ur.x - glyph.ll.x;
    let treffer = Rect::new(
        glyph.ll.x,
        glyph.ll.y,
        glyph.ll.x + 0.3 * breite,
        glyph.ur.y,
    );
    assert!(
        !treffer.contains(glyph.center()),
        "der Bereich darf den Mittelpunkt gerade nicht enthalten"
    );
    let mut redactions = vec![blackout(treffer)];
    // Viele schmale, hohe Bereiche daneben: sie legen das Zellenmaß fest —
    // schmal in x (viele Spalten über die Zeichenbreite), hoch in y (damit
    // weder der Treffer noch das Zeichen an der Zellenschranke scheitern und
    // in die „überall“-Liste rutschen, wo sie ohnehin gefunden würden).
    for k in 0..50 {
        let x = 400.0 + k as f64 * 6.0;
        redactions.push(blackout(Rect::new(x, 300.0, x + 4.0, 400.0)));
    }

    let report = apply(&mut doc, 0.0, &redactions);
    assert_eq!(
        report.per_redaction[0], 1,
        "das Zeichen wurde nicht getroffen"
    );
    assert_eq!(report.removed_glyphs, 1);
    assert!(report.per_redaction[1..].iter().all(|n| *n == 0));
}

/// Ein maßloser Rand bläht jeden Bereich über die ganze Zahlenebene auf. Das
/// Gitter muss das aushalten: die aufgeblähten Bereiche belegen zu viele
/// Zellen und landen in der „überall“-Liste, gefunden werden sie trotzdem.
#[test]
fn ein_masslose_rand_verliert_keine_schwaerzung() {
    for padding in [1e308_f64, f64::MAX] {
        let mut doc = Doc::new();
        let font = helvetica(&mut doc);
        doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
        doc.set_content(format!(
            "BT /F1 10 Tf 1 0 0 1 20 700 Tm ({SECRET}) Tj \
             1 0 0 1 20 600 Tm (Kontoinhaber Max Mustermann) Tj ET\n"
        ));
        // Ein Bereich weitab vom Text — erst der Rand zieht ihn darüber.
        let report = apply(
            &mut doc,
            padding,
            &[blackout(Rect::new(1.0, 1.0, 2.0, 2.0))],
        );
        assert_eq!(
            report.per_redaction,
            vec![SECRET.len() + "Kontoinhaber Max Mustermann".len()],
            "Rand {padding}: der aufgeblähte Bereich muss alles treffen"
        );
        assert!(leaks(&doc.bytes(), SECRET).is_empty(), "Rand {padding}");
    }
}

/// Ein Bereich mit unbrauchbaren Koordinaten (NaN) darf weder etwas treffen
/// noch die anderen Bereiche aus dem Tritt bringen. Regionen kommen aus
/// JSON-Dateien.
#[test]
fn ein_nan_bereich_stoert_die_anderen_nicht() {
    let mut doc = Doc::new();
    let font = monospace(&mut doc);
    doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
    doc.set_content("BT /F1 10 Tf 1 0 0 1 0 700 Tm (ABCDEFGHIJ) Tj ET\n");

    let report = apply(
        &mut doc,
        0.0,
        &[
            blackout(Rect::new(f64::NAN, f64::NAN, f64::NAN, f64::NAN)),
            blackout(Rect::new(0.0, 695.0, 40.0, 715.0)),
        ],
    );
    assert_eq!(report.per_redaction, vec![0, 4]);
    assert_eq!(report.removed_glyphs, 4);
}
