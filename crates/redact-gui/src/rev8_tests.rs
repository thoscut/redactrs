//! Prüfrunde 8: die **Nachprüfung nach dem Export** und drei Ungenauigkeiten
//! der Oberfläche, die die Runde davor belegt hinterlassen hat.
//!
//! Kindmodul von [`crate::app`], damit `RedactApp::silent`, `export_to` und
//! `status_bar` erreichbar sind — die drei Stellen, an denen die neue Auskunft
//! entsteht bzw. gezeichnet wird.
//!
//! Belege über die geschriebene Datei gehen ausschließlich über
//! [`redact_pdf::leaks`]; Belege über das, was der Nutzer wirklich **liest**,
//! gehen über die gemalten egui-Shapes, wie es `rev5_tests` vormacht.

use super::*;

use redact_core::{MatchType, Rect, Region, Source};

use crate::state::{AnnotatedRegion, ExportCheck, HitOutcome, NudgeKind};

// --------------------------------------------------------------- Hilfsmittel

fn iban_only() -> Config {
    Config {
        patterns: vec!["iban_de".to_string()],
        ..Config::default()
    }
}

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rev8-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn demo_app() -> RedactApp {
    let mut app = RedactApp::silent(iban_only());
    app.state
        .load_bytes(
            &redact_pdf::testing::demo_statement(),
            Some(PathBuf::from("demo.pdf")),
        )
        .expect("Demo-PDF ladbar");
    app
}

/// Alle Texte, die ein Bild wirklich malt.
fn painted_texts(output: &egui::FullOutput) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    walk(s, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// Was die Statuszeile in einem Bild **zeichnet** — nicht, was im Feld steht.
fn painted_status(app: &mut RedactApp) -> String {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        // Breit genug, damit egui die Zeile nicht kürzt.
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(6000.0, 900.0),
        )),
        ..Default::default()
    };
    let output = ctx.run(input, |ctx| {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            app.status_bar(ui);
        });
    });
    painted_texts(&output).join("\n")
}

// ===========================================================================
// R1 — Die Nachprüfung: ohne Fund
// ===========================================================================

/// Der Alltagsfall: analysieren, exportieren, und die Oberfläche prüft von
/// selbst nach.
///
/// Geprüft wird über die **geschriebenen Bytes** — die Gegenprobe darunter
/// zeigt, dass derselbe Maßstab an der Eingabe anschlägt.
#[test]
fn r1_die_nachpruefung_nach_dem_export_ohne_fund() {
    let dir = tmp("r1");
    let out = dir.join("out.pdf");

    let mut app = demo_app();
    app.state.analyze().expect("Analyse läuft");
    let mit_text = app
        .state
        .regions
        .iter()
        .filter(|a| {
            a.region
                .text
                .as_deref()
                .is_some_and(|t| !t.trim().is_empty())
        })
        .count();
    assert!(mit_text > 0, "die Demo muss Treffer mit Text liefern");

    app.export_to(out.clone());
    let status = app.state.status.clone();

    assert!(
        status.contains("Nachprüfung:"),
        "die Nachprüfung fehlt in der Statuszeile: {status}"
    );
    assert!(
        status.contains("stehen nicht mehr in der Ausgabe"),
        "Statuszeile: {status}"
    );
    // **Der Vorbehalt.** Ohne ihn ist die Anzeige eine neue falsche Entwarnung.
    assert!(
        status.contains("Geprüft ist genau diese Liste, nicht die Datei."),
        "der Vorbehalt fehlt: {status}"
    );
    // Kein Fund heißt: keine zusätzliche Warnung ganz vorn.
    assert!(
        !app.state.warnings.iter().any(|w| w.contains("Nachprüfung")),
        "ohne Fund gehört nichts in die Warnungen: {:?}",
        app.state.warnings
    );

    // Und der Maßstab selbst, direkt: die IBAN ist wirklich weg …
    let bytes = std::fs::read(&out).expect("Ausgabe lesbar");
    assert!(
        redact_pdf::leaks(&bytes, "DE89 3704 0044 0532 0130 00").is_empty(),
        "die IBAN steht noch in der Ausgabe"
    );
    // … und stand vorher drin.
    assert!(
        !redact_pdf::leaks(
            &redact_pdf::testing::demo_statement(),
            "DE89 3704 0044 0532 0130 00"
        )
        .is_empty(),
        "die Gegenprobe: sonst prüfte der Test darüber nichts"
    );
}

// ===========================================================================
// R2 — Die Nachprüfung: mit Fund
// ===========================================================================

/// **Der Fall, für den es die Anzeige gibt.** Eine Zeile trägt einen Text, der
/// nach dem Export noch in der Datei steht — weil ihr Rechteck woanders liegt,
/// genau wie bei Koordinaten aus einer veralteten Review-Datei.
///
/// Die Kopfzeile verspricht „wird geschwärzt“, der Export meldet Erfolg, und
/// erst die Nachprüfung sagt, dass „Musterbank“ unverändert dasteht.
#[test]
fn r2_die_nachpruefung_findet_ein_leck() {
    let dir = tmp("r2");
    let out = dir.join("out.pdf");

    let mut app = demo_app();
    app.state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        // Leerer Rand unten links — dort steht nichts.
        Rect::new(40.0, 40.0, 120.0, 60.0),
        Some("Musterbank".into()),
        Source::Manual {
            reason: "aus einer veralteten Review-Datei".into(),
        },
    )));
    let index = app.state.regions.len() - 1;
    assert_eq!(
        app.state.hit_summary().outcome(index),
        HitOutcome::Redacted,
        "die Zeile geht als Schwärzung durch — das ist der Ausgangspunkt"
    );

    app.export_to(out.clone());
    let status = app.state.status.clone();

    assert!(
        status.contains("steht NOCH in der Ausgabe"),
        "der Fund muss dastehen: {status}"
    );
    assert!(
        status.contains("darf so nicht weitergegeben werden"),
        "Statuszeile: {status}"
    );
    // Ein Fund gehört **ganz nach vorn** in die Warnungen: die Statuszeile
    // zeigt nur die erste, und keine ist wichtiger.
    assert!(
        app.state
            .warnings
            .first()
            .is_some_and(|w| w.contains("NOCH")),
        "der Fund muss die erste Warnung sein: {:?}",
        app.state.warnings
    );

    // Und der Maßstab bestätigt es an der Datei selbst.
    let bytes = std::fs::read(&out).expect("Ausgabe lesbar");
    assert!(
        !redact_pdf::leaks(&bytes, "Musterbank").is_empty(),
        "…sonst hätte die Anzeige unrecht"
    );
}

// ===========================================================================
// R3 — Die drei Auflagen, an den gemalten Shapes
// ===========================================================================

/// **Was der Nutzer wirklich liest.** Nicht das Feld, sondern das Bild.
///
/// Drei Dinge müssen darin stehen: das Ergebnis, der Vorbehalt und der Satz
/// über die Rechtecke ohne bekannten Text. Der dritte ist der wichtigste:
/// wird er verschwiegen, ist die neue Anzeige an einem Dokument mit lauter
/// selbst gezogenen Rechtecken selbst eine falsche Entwarnung.
#[test]
fn r3_vorbehalt_und_handregionen_werden_wirklich_gemalt() {
    let dir = tmp("r3");
    let out = dir.join("out.pdf");

    let mut app = demo_app();
    app.state.analyze().expect("Analyse läuft");
    // Zwei von Hand gezogene Rechtecke ohne Text.
    for i in 0..2 {
        let y = 300.0 + 30.0 * i as f64;
        app.state
            .add_manual_region(0, Rect::new(100.0, y, 240.0, y + 14.0), "Adresse")
            .expect("angelegt");
    }

    app.export_to(out);
    let painted = painted_status(&mut app);

    assert!(
        painted.contains("Nachprüfung:"),
        "die Nachprüfung wird nicht gemalt: {painted}"
    );
    assert!(
        painted.contains("Geprüft ist genau diese Liste, nicht die Datei."),
        "der Vorbehalt wird nicht gemalt: {painted}"
    );
    assert!(
        painted.contains("2 Rechteck(e) ohne bekannten Text — dafür bleibt die Sichtprüfung."),
        "der Handregion-Satz wird nicht gemalt: {painted}"
    );
}

/// Ohne Handregionen steht der Satz auch nicht da — sonst wäre er Beiwerk und
/// niemand läse ihn dort, wo er zählt.
#[test]
fn r3b_ohne_handregionen_steht_der_satz_nicht_da() {
    let check = ExportCheck {
        checked: 3,
        ..Default::default()
    };
    assert!(!check.sentence().contains("ohne bekannten Text"));
    assert!(check.sentence().contains("Geprüft ist genau diese Liste"));

    let mit = ExportCheck {
        checked: 3,
        without_text: 1,
        ..Default::default()
    };
    assert!(mit
        .sentence()
        .contains("1 Rechteck(e) ohne bekannten Text — dafür bleibt die Sichtprüfung."));
}

/// Ein Dokument, dessen Schwärzungen **sämtlich** von Hand gezogen sind: die
/// Prüfung hat dann gar nichts zu suchen — und muss genau das sagen, statt zu
/// schweigen.
#[test]
fn r3c_nur_handregionen_heisst_nichts_nachgeprueft() {
    let dir = tmp("r3c");
    let out = dir.join("out.pdf");

    let mut app = demo_app();
    app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    app.state
        .add_manual_region(0, Rect::new(70.0, 730.0, 320.0, 748.0), "Kontoinhaber")
        .expect("angelegt");

    app.export_to(out);
    let status = app.state.status.clone();
    assert!(
        status.contains("keine geschwärzte Zeile mit bekanntem Text — es wurde nichts nachgeprüft"),
        "Statuszeile: {status}"
    );
    assert!(
        status.contains("1 Rechteck(e) ohne bekannten Text"),
        "Statuszeile: {status}"
    );
}

// ===========================================================================
// R4 — Der Zeiger auf die Blockade
// ===========================================================================

/// **Befund Z5, behoben.** Zwei deckungsgleiche Handregionen neben einem
/// geschützten Mustertreffer derselben Fläche.
///
/// Vorher verbrauchte die zweite Handregion den Platz des wirklich blockierten
/// Mustertreffers: an einem selbst gezogenen Rechteck stand „geschützt durch
/// Ihre Liste“ (falsch — Handregionen überstimmen die Liste), und der
/// blockierte Mustertreffer hieß „doppelt“.
#[test]
fn r4_eine_handregion_kann_nicht_blockiert_sein() {
    let mut state = AppState::new();
    state
        .load_bytes(
            &redact_pdf::testing::minimal_pdf("nichts"),
            Some(PathBuf::from("x.pdf")),
        )
        .expect("ladbar");
    let r = Rect::new(100.0, 100.0, 300.0, 140.0);

    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        Rect::new(90.0, 90.0, 310.0, 150.0),
        Some("Schutz".into()),
        Source::Booking {
            booking_id: "b001".into(),
            match_type: MatchType::Negative,
        },
    )));
    for _ in 0..2 {
        state.regions.push(AnnotatedRegion::new(Region::new(
            0,
            r,
            None,
            Source::Manual {
                reason: "mit der Tastatur angelegt".into(),
            },
        )));
    }
    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        r,
        Some("DE89".into()),
        Source::Pattern {
            pattern_id: "iban_de".into(),
            confidence: 0.9,
        },
    )));

    let outcomes = state.hit_summary().outcomes;
    assert_eq!(outcomes[0], HitOutcome::Protecting);
    assert_eq!(outcomes[1], HitOutcome::Redacted);
    assert_eq!(
        outcomes[2],
        HitOutcome::Duplicate,
        "die zweite Handregion ist doppelt, nicht blockiert: {outcomes:?}"
    );
    assert_eq!(
        outcomes[3],
        HitOutcome::Blocked,
        "und der Mustertreffer ist der blockierte: {outcomes:?}"
    );

    // Die Wahrheit aus der Auflösung — dieselbe Zeile, die das Audit-Log nennt.
    let blocked = state.blocked_regions();
    assert_eq!(blocked.len(), 1);
    assert_eq!(
        blocked[0].blocked_reason.as_deref().unwrap_or(""),
        state.regions[3].region.reason(),
        "Anzeige und Audit-Log nennen jetzt dieselbe Zeile"
    );
}

/// Die Gegenprobe: ein **Mustertreffer** darf weiterhin blockiert heißen —
/// sonst hätte die neue Frage den richtigen Fall mit weggeschnitten.
#[test]
fn r4b_ein_mustertreffer_bleibt_blockierbar() {
    let mut state = AppState::new();
    state
        .load_bytes(
            &redact_pdf::testing::minimal_pdf("nichts"),
            Some(PathBuf::from("x.pdf")),
        )
        .expect("ladbar");
    let r = Rect::new(100.0, 100.0, 300.0, 140.0);
    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        Rect::new(90.0, 90.0, 310.0, 150.0),
        Some("Schutz".into()),
        Source::Booking {
            booking_id: "b001".into(),
            match_type: MatchType::Negative,
        },
    )));
    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        r,
        Some("DE89".into()),
        Source::Pattern {
            pattern_id: "iban_de".into(),
            confidence: 0.9,
        },
    )));
    let outcomes = state.hit_summary().outcomes;
    assert_eq!(outcomes[1], HitOutcome::Blocked, "{outcomes:?}");
    assert_eq!(outcomes[1].note(), "geschützt durch Ihre Liste");
}

// ===========================================================================
// R5 — Die Wortwahl der Absage
// ===========================================================================

/// **Befund Z1c, behoben.** Wer die Größe ändern wollte, liest jetzt auch das.
#[test]
fn r5_die_absage_nennt_das_verb_um_das_es_ging() {
    assert_eq!(NudgeKind::Move.refusal(), "Nicht verschoben");
    assert_eq!(NudgeKind::Resize.refusal(), "Größe nicht geändert");

    let mut app = demo_app();
    app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    app.state.add_region_in_page_middle().expect("angelegt");
    app.state.set_page(1);

    assert!(!app.state.resize_selected(1.0, 0.0));
    let status = app.state.status.clone();
    assert!(
        status.starts_with("Größe nicht geändert"),
        "unerwarteter Wortlaut: {status}"
    );
    assert!(status.contains("Seite 1"), "Statuszeile: {status}");

    // Das Schieben behält seinen Satz.
    assert!(!app.state.move_selected(1.0, 0.0));
    assert!(
        app.state.status.starts_with("Nicht verschoben"),
        "Statuszeile: {}",
        app.state.status
    );
}

// ===========================================================================
// R6 — Die Absage bei einer Seite, die es nicht gibt
// ===========================================================================

/// **Befund Z6, behoben.** „Zu ihr blättern“ schickte zu Seite 8 eines
/// zweiseitigen Dokuments.
#[test]
fn r6_die_absage_schickt_nicht_zu_einer_seite_die_es_nicht_gibt() {
    let mut app = demo_app();
    app.state.regions.push(AnnotatedRegion::new(Region::new(
        7,
        Rect::new(10.0, 10.0, 50.0, 20.0),
        Some("DE89".into()),
        Source::Manual {
            reason: "aus einer Review-Datei".into(),
        },
    )));
    let index = app.state.regions.len() - 1;
    assert_eq!(
        app.state.hit_summary().outcome(index),
        HitOutcome::MissingPage
    );
    app.state.selected_region = Some(index);

    assert!(!app.state.resize_selected(1.0, 0.0));
    let status = app.state.status.clone();
    assert_eq!(app.state.page_count(), 2);
    assert!(
        status.contains("die es in diesem Dokument nicht gibt"),
        "Statuszeile: {status}"
    );
    assert!(
        status.contains("es hat 2 Seite(n)"),
        "die Seitenzahl gehört dazu: {status}"
    );
    assert!(
        !status.contains("Zu ihr blättern"),
        "der falsche Rat steht noch da: {status}"
    );
    assert!(
        status.contains("Entf") && status.contains("Review-Datei"),
        "die Absage muss einen gangbaren Ausweg nennen: {status}"
    );
}

/// Gegenprobe: eine Seite, die es **gibt**, bekommt weiterhin den Rat zu
/// blättern — sonst hätte die neue Unterscheidung den häufigen Fall
/// mitgenommen.
#[test]
fn r6b_eine_vorhandene_seite_bekommt_weiter_den_rat_zu_blaettern() {
    let mut app = demo_app();
    app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    app.state.set_page(1);
    app.state.add_region_in_page_middle().expect("angelegt");
    app.state.set_page(0);

    assert!(!app.state.move_selected(1.0, 0.0));
    let status = app.state.status.clone();
    assert!(status.contains("Zu ihr blättern"), "Statuszeile: {status}");
    assert!(status.contains("Seite 2"), "Statuszeile: {status}");
}

// ===========================================================================
// R7 — Der Verlauf erfasst wirklich alle zehn Änderungen
// ===========================================================================

/// Die README zählt zehn Verlaufsschritte auf. **Analysieren** ist der, den
/// die alte Aufzählung verschwieg — und der, bei dem es am meisten kostet.
///
/// Gemessen, damit die README nicht mehr verspricht, als der Code tut: die
/// Trefferliste wird **ausgetauscht**. Selbst gezogene Rechtecke
/// ([`Source::Manual`]) trägt `analyze` ausdrücklich hinüber; die
/// Entscheidungen an den **Mustertreffern** — abgewählt, andere
/// Schwärzungsart, eigener Ersatztext — sind danach weg, weil deren Zeilen
/// neu entstehen. Und genau deshalb ist es ein Verlaufsschritt.
#[test]
fn r7_analysieren_ist_ein_verlaufsschritt() {
    let mut app = demo_app();
    app.state.analyze().expect("Analyse läuft");
    assert!(!app.state.regions.is_empty(), "die Demo liefert Treffer");

    // Handarbeit zweierlei Art: ein eigenes Rechteck …
    app.state
        .add_manual_region(0, Rect::new(100.0, 300.0, 240.0, 314.0), "Adresse")
        .expect("angelegt");
    // … und eine Entscheidung an einem Mustertreffer.
    assert!(app.state.set_enabled(0, false));
    assert!(app.state.set_action(0, redact_core::Action::Whiteout));

    app.state.analyze().expect("Analyse läuft");

    assert!(
        app.state.regions.iter().any(|a| matches!(
            &a.region.source,
            Source::Manual { reason } if reason == "Adresse"
        )),
        "selbst gezogene Rechtecke überleben die Analyse"
    );
    assert!(
        app.state.regions[0].enabled,
        "**die Entscheidung am Mustertreffer ist weg** — die Zeile ist neu"
    );
    assert_ne!(app.state.regions[0].action, redact_core::Action::Whiteout);

    // Und genau dafür gibt es den Verlaufsschritt.
    assert!(app.state.undo(), "Analysieren ist rückgängig zu machen");
    assert!(!app.state.regions[0].enabled, "die Abwahl ist zurück");
    assert_eq!(app.state.regions[0].action, redact_core::Action::Whiteout);
}

/// Und die drei, die in der alten Aufzählung fehlten.
#[test]
fn r7b_groesse_schwaerzungsart_und_ersatztext_sind_schritte() {
    let mut app = demo_app();
    app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    let index = app.state.add_region_in_page_middle().expect("angelegt");
    let angelegt = app.state.regions[index].region.rect;

    assert!(app.state.resize_selected(10.0, 0.0));
    assert_ne!(app.state.regions[index].region.rect, angelegt);
    assert!(app.state.undo());
    assert_eq!(app.state.regions[index].region.rect, angelegt);

    assert!(app.state.set_action(index, redact_core::Action::Whiteout));
    assert_eq!(
        app.state.regions[index].action,
        redact_core::Action::Whiteout
    );
    assert!(app.state.undo());
    assert_ne!(
        app.state.regions[index].action,
        redact_core::Action::Whiteout
    );

    assert!(app
        .state
        .set_action(index, redact_core::Action::Replace("XXX".to_string())));
    assert!(app.state.edit_replacement(index, "YYY"));
    assert!(app.state.undo());
    assert!(matches!(
        &app.state.regions[index].action,
        redact_core::Action::Replace(text) if text == "XXX"
    ));
}
