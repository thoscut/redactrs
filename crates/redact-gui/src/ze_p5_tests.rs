//! Gegenprüfung P5 (Fix-Runde 4): die Oberfläche nach dem Fix —
//! Normalform der Nachprüfung, Entpackbudget, Warnungen.
//!
//! Tests mit `#[ignore = "Befund …"]` tragen die **richtige** Erwartung und
//! sind bis zur Korrektur rot; die grünen Tests daneben halten fest, was der
//! Code heute wirklich tut (`cargo test -p redact-gui ze_p5`).

use std::path::PathBuf;

use redact_core::{Rect, Region, Source};
use redact_pdf::testing::{build_pdf, TextItem};
use redact_pipeline::Config;

use crate::state::{AnnotatedRegion, HitOutcome};
use crate::RedactApp;

// --------------------------------------------------------------- Hilfsmittel

/// Dieselbe IBAN in zwei Schreibweisen — die Normalform ist dieselbe.
const PLAIN: &str = "DE89370400440532013000";
const SPACED: &str = "DE89 3704 0044 0532 0130 00";

fn no_patterns() -> Config {
    Config {
        no_patterns: true,
        ..Config::default()
    }
}

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ze-p5-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn text_region(page: usize, rect: Rect, text: &str) -> AnnotatedRegion {
    AnnotatedRegion::new(Region::new(
        page,
        rect,
        Some(text.to_string()),
        Source::Manual {
            reason: "P5".into(),
        },
    ))
}

/// Ein Rechteck, unter dem kein Text liegt: die Zeile gilt als geschwärzt,
/// entfernt aber nichts — ein echtes Leck.
fn empty_corner(i: usize) -> Rect {
    let x = 400.0 + (i % 50) as f64 * 3.0;
    Rect::new(x, 20.0, x + 2.0, 40.0)
}

/// Eine Seite mit beiden Schreibweisen.
fn two_spellings() -> Vec<u8> {
    build_pdf(&[vec![
        TextItem::new(72.0, 700.0, 10.0, format!("Konto {PLAIN}")),
        TextItem::new(72.0, 660.0, 10.0, format!("IBAN: {SPACED}")),
    ]])
}

fn over_plain() -> Rect {
    Rect::new(60.0, 690.0, 400.0, 714.0)
}

fn over_spaced() -> Rect {
    Rect::new(60.0, 650.0, 400.0, 674.0)
}

// ===========================================================================
// 1 — Die Normalform: `squeeze` deckt mehr ab, als die Suche einlöst
// ===========================================================================

/// **Befund P5-1.** Zwei geschwärzte Zeilen, deren Texte sich nur im
/// Leerraum unterscheiden, sind seit Fix-Runde 4 **ein** Text; gesucht wird
/// der Originaltext der **ersten** Zeile. Steht dort die Schreibweise
/// **ohne** Leerraum, sucht `leaks_many` nur diese wörtlich —
/// `Needle::squeezed` ist `None`, wenn der Begriff selbst keinen Leerraum
/// enthält. Die zweite Schreibweise bleibt in der Ausgabe stehen (das
/// Rechteck trifft sie nicht) und wird nicht mehr gemeldet: vor Runde 4
/// waren es zwei Begriffe, und der zweite hätte angeschlagen.
#[test]
fn ze_p5_1_die_normalform_verliert_die_zweite_schreibweise() {
    let out = tmp("1-normalform").join("out.pdf");
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&two_spellings(), "zwei.pdf");

    // Zuerst die Schreibweise OHNE Leerraum (Rechteck trifft), dann die MIT
    // Leerraum (Rechteck trifft nichts — echtes Leck).
    app.state.regions.push(text_region(0, over_plain(), PLAIN));
    app.state
        .regions
        .push(text_region(0, empty_corner(0), SPACED));
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.outcome(1), HitOutcome::Redacted);
    app.state.export(&out, None).expect("Export");

    // Das Orakel: die Schreibweise mit Leerraum steht noch in der Datei.
    let bytes = std::fs::read(&out).unwrap();
    let spaced_hits = redact_pdf::leaks(&bytes, SPACED);
    let plain_hits = redact_pdf::leaks(&bytes, PLAIN);
    println!("Orakel SPACED: {spaced_hits:?}");
    println!("Orakel PLAIN:  {plain_hits:?}");
    assert!(!spaced_hits.is_empty(), "die Vorlage muss lecken");

    let plan = app.state.plan_export_check(&summary);
    println!("needles: {:?}", plan.needles);
    // Ein Begriff — und zwar der ohne Leerraum.
    assert_eq!(plan.needles, vec![PLAIN.to_string()], "{plan:?}");
    let check = plan.run(&out);
    println!("Statuszeile: {}", check.sentence());

    // Der Befund: die Oberfläche schweigt über ein echtes Leck.
    assert!(
        !check.found_leak(),
        "hier wäre der Befund behoben: {}",
        check.sentence()
    );
    assert!(check.warning().is_none(), "{:?}", check.warning());
    assert!(
        check
            .sentence()
            .contains("stehen nicht mehr in der Ausgabe"),
        "{}",
        check.sentence()
    );
}

/// Dieselbe Vorlage, richtige Erwartung: ein Leck muss gemeldet werden.
#[test]
#[ignore = "Befund P5-1: die zweite Schreibweise wird nicht gesucht"]
fn ze_p5_1_die_zweite_schreibweise_muesste_ein_leck_sein() {
    let out = tmp("1-soll").join("out.pdf");
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&two_spellings(), "zwei.pdf");
    app.state.regions.push(text_region(0, over_plain(), PLAIN));
    app.state
        .regions
        .push(text_region(0, empty_corner(0), SPACED));
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");
    let check = app.state.plan_export_check(&summary).run(&out);
    assert!(check.found_leak(), "{}", check.sentence());
}

/// Die Gegenprobe zur Reihenfolge: steht die Schreibweise **mit** Leerraum
/// vorn, trägt der Begriff seine Fassung ohne Leerraum mit — dann findet
/// dieselbe Suche dasselbe Leck. Der Befund hängt allein an der
/// Listenreihenfolge, und die ist die des Dokuments.
#[test]
fn ze_p5_1_umgekehrte_reihenfolge_findet_dasselbe_leck() {
    let out = tmp("1-umgekehrt").join("out.pdf");
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&two_spellings(), "zwei.pdf");
    // Erst die Schreibweise MIT Leerraum (Rechteck trifft), dann die ohne
    // (Rechteck trifft nichts — echtes Leck).
    app.state
        .regions
        .push(text_region(0, over_spaced(), SPACED));
    app.state
        .regions
        .push(text_region(0, empty_corner(1), PLAIN));
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");
    let plan = app.state.plan_export_check(&summary);
    assert_eq!(plan.needles, vec![SPACED.to_string()], "{plan:?}");
    let check = plan.run(&out);
    println!("Statuszeile: {}", check.sentence());
    assert!(check.found_leak(), "{}", check.sentence());
}

// ===========================================================================
// 2 — Die Entscheidung „kept“ deckt seit Runde 4 mehr ab als die Suche
// ===========================================================================

/// **Befund P5-2.** Ein **abgewählter** Text deckt jetzt jede Schreibweise
/// derselben Normalform. Eine geschwärzte Zeile, deren Rechteck danebengeht
/// und deren Text sich vom abgewählten nur im Leerraum unterscheidet, wird
/// deshalb gar nicht mehr gesucht — `kept` statt `needle`. Vor Runde 4 stand
/// sie in `needles`, und die Suche fand sie. Der Fehlalarm, den Runde 4
/// abstellen wollte (G5-B1), lag in der **anderen** Richtung: dort trug der
/// Begriff selbst Leerraum, und nur deshalb sucht `leaks_many` ihn auch
/// gequetscht.
#[test]
fn ze_p5_2_ein_abgewaehlter_text_deckt_die_andere_schreibweise() {
    let out = tmp("2-kept").join("out.pdf");
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&two_spellings(), "zwei.pdf");
    // Geschwärzt, Rechteck geht daneben: echtes Leck.
    app.state
        .regions
        .push(text_region(0, empty_corner(2), SPACED));
    // Bewusst stehen gelassen — eine andere Zeile, eine andere Schreibweise.
    app.state.regions.push(text_region(0, over_plain(), PLAIN));
    assert!(app.state.set_enabled(1, false));
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.outcome(1), HitOutcome::Disabled);
    app.state.export(&out, None).expect("Export");

    let bytes = std::fs::read(&out).unwrap();
    assert!(
        !redact_pdf::leaks(&bytes, SPACED).is_empty(),
        "die Vorlage muss lecken"
    );

    let plan = app.state.plan_export_check(&summary);
    println!("needles: {:?}, kept: {}", plan.needles, plan.kept);
    assert!(plan.needles.is_empty(), "{plan:?}");
    assert_eq!(plan.kept, 1, "{plan:?}");
    let check = plan.run(&out);
    println!("Statuszeile: {}", check.sentence());
    assert!(
        !check.found_leak(),
        "hier wäre der Befund behoben: {}",
        check.sentence()
    );
    assert!(check.warning().is_none(), "{:?}", check.warning());
    assert!(
        check
            .sentence()
            .contains("1 Text(e) stehen auch in einer abgewählten"),
        "{}",
        check.sentence()
    );
}

/// Dieselbe Vorlage, richtige Erwartung.
#[test]
#[ignore = "Befund P5-2: der abgewählte Text deckt die andere Schreibweise"]
fn ze_p5_2_die_andere_schreibweise_muesste_ein_leck_sein() {
    let out = tmp("2-soll").join("out.pdf");
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&two_spellings(), "zwei.pdf");
    app.state
        .regions
        .push(text_region(0, empty_corner(2), SPACED));
    app.state.regions.push(text_region(0, over_plain(), PLAIN));
    assert!(app.state.set_enabled(1, false));
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");
    let check = app.state.plan_export_check(&summary).run(&out);
    assert!(check.found_leak(), "{}", check.sentence());
}

// ===========================================================================
// 3 — Die Ränder der Normalform (kein Befund)
// ===========================================================================

/// Was `squeeze` zusammenfasst und was nicht: geschütztes Leerzeichen,
/// Tabulator und Zeilenumbruch sind Leerraum und fallen zusammen; Text, der
/// nur aus Leerraum besteht, zählt als „ohne Text“; Groß-/Kleinschreibung,
/// Unicode-Normalisierung (é als ein oder zwei Codepunkte) und Bindestriche
/// bleiben eigene Texte — die Normalform macht die Suche dort **nicht**
/// gröber, also entsteht dort auch kein blinder Fleck.
#[test]
fn ze_p5_3_die_raender_der_normalform() {
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&redact_pdf::testing::minimal_pdf("x"), "eins.pdf");
    let texts = [
        "AB\tCD",     // 0 — steht vorn, also der gesuchte Originaltext
        "AB CD",      // 1 — dieselbe Normalform
        "AB\u{a0}CD", // 2 — geschütztes Leerzeichen, dieselbe Normalform
        "AB\nCD",     // 3 — Zeilenumbruch, dieselbe Normalform
        "ABCD",       // 4 — ganz ohne Leerraum, dieselbe Normalform
        "\u{a0}",     // 5 — nur Leerraum: „ohne Text“
        " \t\n ",     // 6 — nur Leerraum: „ohne Text“
        "ab cd",      // 7 — andere Schreibung: eigener Text
        "AB-CD",      // 8 — Bindestrich ist kein Leerraum: eigener Text
        "\u{e9}",     // 9 — é als ein Codepunkt
        "e\u{301}",   // 10 — é als zwei Codepunkte: eigener Text
    ];
    for (i, text) in texts.iter().enumerate() {
        app.state
            .regions
            .push(text_region(0, empty_corner(i), text));
    }
    let summary = app.state.hit_summary();
    for i in 0..texts.len() {
        assert_eq!(summary.outcome(i), HitOutcome::Redacted, "Region {i}");
    }
    let plan = app.state.plan_export_check(&summary);
    println!("needles: {:?}", plan.needles);
    assert_eq!(
        plan.needles,
        vec![
            "AB\tCD".to_string(),
            "ab cd".to_string(),
            "AB-CD".to_string(),
            "\u{e9}".to_string(),
            "e\u{301}".to_string(),
        ],
        "{plan:?}"
    );
    assert_eq!(plan.without_text, 2, "{plan:?}");
    assert_eq!(plan.kept, 0, "{plan:?}");
}

// ===========================================================================
// 4 — Das Budget in der Oberfläche
// ===========================================================================

/// Die Zahl im Plan ist die des Ladens — auch nach einem zweiten Dokument
/// und nach einer Review-Datei. Gegenprobe: die Vorgabe ist eine andere Zahl.
#[test]
fn ze_p5_4_das_budget_bleibt_das_des_ladens() {
    let tight = 7 * 1024 * 1024;
    assert_ne!(
        tight,
        redact_pdf::document::Limits::default().max_decompressed_bytes
    );
    let mut app = RedactApp::new(Config {
        limits: redact_pdf::document::Limits {
            max_decompressed_bytes: tight,
            ..redact_pdf::document::Limits::default()
        },
        ..no_patterns()
    });
    let budget = |app: &RedactApp| {
        let summary = app.state.hit_summary();
        app.state.plan_export_check(&summary).max_decompressed_bytes
    };

    app.open_bytes_and_analyze(&two_spellings(), "erste.pdf");
    assert_eq!(budget(&app), tight, "nach dem ersten Dokument");

    app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "zweite.pdf");
    assert_eq!(budget(&app), tight, "nach dem zweiten Dokument");

    let review = tmp("4-budget").join("review.json");
    app.state
        .regions
        .push(text_region(0, empty_corner(0), "Musterbank"));
    app.save_review_to(&review);
    assert!(review.exists(), "{}", app.state.status);
    app.load_review_file(&review);
    assert_eq!(budget(&app), tight, "nach der Review-Datei");
}

// ===========================================================================
// 5 — `unchecked` kommt bis in den Satz (Lücke im Testbau der Runde 4)
// ===========================================================================

/// **Befund P5-5.** Die Verdrahtung `ExportCheckPlan::run` →
/// `ExportCheck::unchecked` hat keinen Test: Mutation
/// `unchecked: found.unchecked` → `unchecked: Vec::new()` und alle 318 Tests
/// der Oberfläche bleiben grün. `g5a2_der_lauf_reicht_unchecked_durch`
/// prüft nur `is_empty()` — das gilt so oder so —, und
/// `g5a2_nicht_geprueft_steht_im_satz` baut sein `ExportCheck` von Hand.
/// Dieser Test geht den ganzen Weg: ein Budget, das nichts durchlässt, muss
/// bis in den Satz und in die Warnung durchschlagen.
#[test]
fn ze_p5_5_ein_zu_kleines_budget_kommt_bis_in_den_satz() {
    let out = tmp("5-budget").join("out.pdf");
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&two_spellings(), "zwei.pdf");
    app.state.regions.push(text_region(0, over_plain(), PLAIN));
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");

    // Gegenprobe zuerst: mit dem Budget des Ladens ist nichts ungeprüft.
    let full = app.state.plan_export_check(&summary).run(&out);
    assert!(!full.incomplete(), "{:?}", full.unchecked);
    assert_eq!(full.warning(), None);

    // Ein Byte Budget: nichts lässt sich entpacken, und das muss man sehen.
    let mut plan = app.state.plan_export_check(&summary);
    plan.max_decompressed_bytes = 1;
    let check = plan.run(&out);
    println!("unchecked: {:#?}", check.unchecked);
    assert!(check.incomplete(), "{check:?}");
    assert!(!check.found_leak(), "{}", check.sentence());
    let sentence = check.sentence();
    println!("Statuszeile: {sentence}");
    assert!(
        sentence.contains("wurden nicht geprüft (Entpackgrenze) — die Antwort ist unvollständig."),
        "{sentence}"
    );
    assert!(
        sentence.contains("Geprüft ist genau diese Liste, nicht die Datei."),
        "{sentence}"
    );
    let warning = check.warning().expect("unvollständig ist eine Warnung");
    assert!(
        warning.contains("nicht geprüft (Entpackgrenze)"),
        "{warning}"
    );
}

/// Dass P5-1 kein Kunstgriff ist: die **Analyse** selbst liefert die beiden
/// Schreibweisen — Seite 1 ohne, Seite 2 mit Leerraum, beide echte
/// Mustertreffer, beide geschwärzt. Der Plan trägt trotzdem nur einen
/// Begriff, und der ist der ohne Leerraum. Vor Fix-Runde 4 waren es zwei.
#[test]
fn ze_p5_1_auch_die_analyse_liefert_beide_schreibweisen() {
    let iban_only = Config {
        patterns: vec!["iban_de".to_string()],
        ..Config::default()
    };
    let pdf = build_pdf(&[
        vec![TextItem::new(72.0, 700.0, 10.0, format!("Konto {PLAIN}"))],
        vec![TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {SPACED}"))],
    ]);
    let mut app = RedactApp::new(iban_only);
    app.open_bytes_and_analyze(&pdf, "zwei.pdf");
    assert_eq!(app.state.regions.len(), 2, "{}", app.state.status);
    assert_eq!(app.state.regions[0].region.text.as_deref(), Some(PLAIN));
    assert_eq!(app.state.regions[1].region.text.as_deref(), Some(SPACED));
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.outcome(1), HitOutcome::Redacted);
    let plan = app.state.plan_export_check(&summary);
    println!("needles: {:?}", plan.needles);
    assert_eq!(plan.needles, vec![PLAIN.to_string()], "{plan:?}");
    // Und der eine Begriff trägt keine Fassung ohne Leerraum: er hat keinen.
    assert_eq!(redact_pdf::squeeze(PLAIN), PLAIN);
}
