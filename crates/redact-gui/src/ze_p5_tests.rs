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

use crate::state::{AnnotatedRegion, ExportCheckPlan, HitOutcome};
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

/// **Befund P5-1, behoben.** Zwei geschwärzte Zeilen, deren Texte sich nur
/// im Leerraum unterscheiden, waren seit Fix-Runde 4 **ein** Begriff;
/// gesucht wurde der Originaltext der **ersten** Zeile. Stand dort die
/// Schreibweise **ohne** Leerraum, suchte `leaks_many` nur diese wörtlich —
/// `Needle::squeezed` ist `None`, wenn der Begriff selbst keinen Leerraum
/// enthält —, und die zweite Schreibweise blieb ungesucht in der Ausgabe
/// stehen (das Rechteck trifft sie nicht): sieben Fundstellen, gemeldet
/// wurde „1 gesuchte(r) Text steht nicht mehr in der Ausgabe“.
/// Jetzt steht **jede** Schreibweise in `needles`.
/// Mutation (`seen` auf `squeeze(&text)` statt auf `text`): rot.
#[test]
fn ze_p5_1_jede_schreibweise_wird_gesucht() {
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
    // Beide Schreibweisen — die Decke zählt Schreibweisen, nicht Normalformen.
    assert_eq!(
        plan.needles,
        vec![PLAIN.to_string(), SPACED.to_string()],
        "{plan:?}"
    );
    let check = plan.run(&out);
    println!("Statuszeile: {}", check.sentence());

    // Und das echte Leck wird gemeldet — mit der Schreibweise, die leckt.
    assert!(check.found_leak(), "{}", check.sentence());
    assert_eq!(check.leaking, vec![SPACED.to_string()]);
    assert_eq!(check.checked, 2);
    assert!(check.warning().is_some(), "{:?}", check.warning());
    assert!(
        check
            .sentence()
            .contains("1 von 2 gesuchten Text(en) steht NOCH in der Ausgabe"),
        "{}",
        check.sentence()
    );
}

/// Dieselbe Vorlage, richtige Erwartung: ein Leck muss gemeldet werden.
#[test]
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

/// Die Gegenprobe zur Reihenfolge: dieselbe Vorlage andersherum. Vorher
/// hing das Urteil an der Listenreihenfolge (mit Leerraum vorn → gefunden,
/// ohne Leerraum vorn → stilles Leck); jetzt stehen beide Schreibweisen in
/// `needles`, und das Urteil ist in beiden Reihenfolgen dasselbe.
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
    assert_eq!(
        plan.needles,
        vec![SPACED.to_string(), PLAIN.to_string()],
        "{plan:?}"
    );
    let check = plan.run(&out);
    println!("Statuszeile: {}", check.sentence());
    assert!(check.found_leak(), "{}", check.sentence());
    // Ohne eine bewusst stehen gelassene Zeile deckt nichts irgendetwas ab:
    // jeder Fund zählt, auch der, den nur die Fassung ohne Leerraum bringt.
    // Mutation (`kept_forms` im Lauf nicht beachtet, jeder gequetschte Fund
    // gilt als gedeckt): `kept` 1 statt 0, rot.
    assert_eq!(check.kept, 0, "{check:?}");
    assert_eq!(check.leaking.len(), 2, "{:?}", check.leaking);
}

// ===========================================================================
// 2 — „kept“ deckte seit Runde 4 mehr ab als die Suche
// ===========================================================================

/// **Befund P5-2, behoben.** Ein **abgewählter** Text deckte seit Fix-Runde 4
/// jede Schreibweise derselben Normalform. Eine geschwärzte Zeile, deren
/// Rechteck danebengeht und deren Text sich vom abgewählten nur im Leerraum
/// unterscheidet, wurde deshalb gar nicht mehr gesucht — `kept` statt
/// `needle`; vor Runde 4 stand sie in `needles`, und die Suche fand sie.
///
/// Jetzt wird sie gesucht, und entschieden wird am **Fund**: getroffen hat
/// die Schreibweise hier Zeichen für Zeichen ([`redact_pdf::LeakCheck`]s
/// `literal`), also ist es ein Leck — gleich, was daneben abgewählt ist.
/// Der Fehlalarm, den Runde 4 abstellen wollte (G5-B1), liegt in der
/// anderen Hälfte desselben Urteils: dort trifft **nur** die Fassung ohne
/// Leerraum, und die kann die abgewählte Zeile sein
/// (`ze_p5_2_ein_fund_nur_ohne_leerraum_ist_kein_leck`).
/// Mutation (die `literal`-Prüfung im Lauf weg, Entscheidung zurück in den
/// Plan): rot.
#[test]
fn ze_p5_2_die_andere_schreibweise_muesste_ein_leck_sein() {
    let out = tmp("2-soll").join("out.pdf");
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

    // Das Orakel: die geschwärzte Schreibweise steht wörtlich noch da.
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        !redact_pdf::leaks(&bytes, SPACED).is_empty(),
        "die Vorlage muss lecken"
    );

    let plan = app.state.plan_export_check(&summary);
    println!(
        "needles: {:?}, kept_forms: {:?}",
        plan.needles, plan.kept_forms
    );
    assert_eq!(plan.needles, vec![SPACED.to_string()], "{plan:?}");
    assert_eq!(plan.kept_forms, vec![true], "{plan:?}");
    assert_eq!(plan.kept, 0, "{plan:?}");
    let check = plan.run(&out);
    println!("Statuszeile: {}", check.sentence());
    assert!(check.found_leak(), "{}", check.sentence());
    assert_eq!(check.leaking, vec![SPACED.to_string()]);
    assert_eq!(check.kept, 0, "{check:?}");
    assert!(check.warning().is_some(), "{:?}", check.warning());
}

/// Die Gegenrichtung, und der Fehlalarm aus Runde 4 darf nicht zurückkommen:
/// die geschwärzte Schreibweise ist wirklich weg, die abgewählte steht da.
/// Getroffen hat dann **nur** die Fassung ohne Leerraum — das kann die
/// abgewählte Zeile sein, also kein Leck, sondern `kept`. Dieselbe Vorlage
/// wie oben, nur trifft das Rechteck. Schwester von
/// `zc_g5_tests::g5b_dieselbe_iban_ohne_leerraum_abgewaehlt_ist_kein_leck`.
#[test]
fn ze_p5_2_ein_fund_nur_ohne_leerraum_ist_kein_leck() {
    let out = tmp("2-kept").join("out.pdf");
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&two_spellings(), "zwei.pdf");
    // Geschwärzt, und das Rechteck trifft: die Schreibweise ist weg.
    app.state
        .regions
        .push(text_region(0, over_spaced(), SPACED));
    // Bewusst stehen gelassen.
    app.state.regions.push(text_region(0, over_plain(), PLAIN));
    assert!(app.state.set_enabled(1, false));
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");

    // Das Orakel, und zugleich der ganze Mechanismus: die geschwärzte
    // Schreibweise ist **wörtlich** weg, getroffen wird sie nur noch über
    // ihre Fassung ohne Leerraum — nämlich an der abgewählten Zeile, die
    // bewusst dasteht.
    let bytes = std::fs::read(&out).unwrap();
    let probe = redact_pdf::leaks_many_within(&bytes, &[SPACED], u64::MAX);
    println!("Orakel: {:?}", probe.findings[0]);
    assert!(!probe.findings[0].is_empty(), "gequetscht trifft es");
    assert!(!probe.literal[0], "wörtlich weg: {:?}", probe.findings[0]);
    assert!(!redact_pdf::leaks(&bytes, PLAIN).is_empty(), "bewusst da");

    let plan = app.state.plan_export_check(&summary);
    assert_eq!(plan.needles, vec![SPACED.to_string()], "{plan:?}");
    assert_eq!(plan.kept_forms, vec![true], "{plan:?}");
    let check = plan.run(&out);
    println!("Statuszeile: {}", check.sentence());
    assert!(
        !check.found_leak(),
        "der Fehlalarm aus Runde 4 wäre zurück: {}",
        check.sentence()
    );
    assert_eq!(check.kept, 1, "{check:?}");
    assert!(check.warning().is_none(), "{:?}", check.warning());
    assert!(
        check
            .sentence()
            .contains("1 Text(e) decken sich mit einer abgewählten"),
        "{}",
        check.sentence()
    );
}

/// Und dieselbe Vorlage andersherum: geschwärzt die Schreibweise **ohne**
/// Leerraum (Rechteck daneben — echtes Leck), stehen gelassen die **mit**.
/// Hier konnte es nie einen Fehlalarm geben (ein Begriff ohne Leerraum wird
/// nur wörtlich gesucht) — Fix-Runde 4 deckte trotzdem auch diese Richtung.
#[test]
fn ze_p5_2_auch_andersherum_ist_es_ein_leck() {
    let out = tmp("2-andersherum").join("out.pdf");
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&two_spellings(), "zwei.pdf");
    app.state
        .regions
        .push(text_region(0, empty_corner(3), PLAIN));
    app.state
        .regions
        .push(text_region(0, over_spaced(), SPACED));
    assert!(app.state.set_enabled(1, false));
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.outcome(1), HitOutcome::Disabled);
    app.state.export(&out, None).expect("Export");

    let bytes = std::fs::read(&out).unwrap();
    assert!(
        !redact_pdf::leaks(&bytes, PLAIN).is_empty(),
        "die Vorlage muss lecken"
    );

    let plan = app.state.plan_export_check(&summary);
    assert_eq!(plan.needles, vec![PLAIN.to_string()], "{plan:?}");
    let check = plan.run(&out);
    println!("Statuszeile: {}", check.sentence());
    assert!(check.found_leak(), "{}", check.sentence());
    assert_eq!(check.leaking, vec![PLAIN.to_string()]);
    assert_eq!(check.kept, 0, "{check:?}");
}

// ===========================================================================
// 3 — Die Ränder der Normalform (kein Befund)
// ===========================================================================

/// Jede Schreibweise steht in `needles` — auch die vier, die dieselbe
/// Normalform tragen (geschütztes Leerzeichen, Tabulator, Zeilenumbruch,
/// ganz ohne Leerraum). Text, der nur aus Leerraum besteht, zählt als „ohne
/// Text“; Groß-/Kleinschreibung, Unicode-Normalisierung (é als ein oder
/// zwei Codepunkte) und Bindestriche waren schon vorher eigene Texte.
/// Mutation (`seen` auf der Normalform): fünf statt neun Begriffe, rot.
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
            "AB CD".to_string(),
            "AB\u{a0}CD".to_string(),
            "AB\nCD".to_string(),
            "ABCD".to_string(),
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
        sentence.contains("wurden nicht geprüft — die Antwort ist unvollständig:"),
        "{sentence}"
    );
    // Der Grund steht bei der Stelle, nicht im Satzbau: hier ist er wirklich
    // die Entpackgrenze, und der Satz gibt ihn wieder, statt ihn anzunehmen.
    assert!(
        check
            .unchecked
            .iter()
            .any(|u| u.contains("Entpackgrenze") || u.contains("Vorprüfung")),
        "{:#?}",
        check.unchecked
    );
    // Im Satz steht die Stelle **gekürzt** — Ort und Grund, ohne die
    // Buchhaltung des Budgets (Befund Q4-3).
    let voll = check.unchecked.first().expect("eine Stelle").clone();
    let (ort_und_grund, rest) = voll.split_once(", ").expect("die volle Zeile rechnet vor");
    assert!(sentence.contains(ort_und_grund), "{sentence}");
    assert!(
        !sentence.contains(rest),
        "gekürzt, nicht abgeschrieben: {sentence}"
    );
    assert!(
        sentence.contains("Geprüft ist genau diese Liste, nicht die Datei."),
        "{sentence}"
    );
    // Kein doppelter Punkt, wo die Stelle schon einen mitbringt.
    assert!(!sentence.contains(".. Geprüft"), "{sentence}");
    let warning = check.warning().expect("unvollständig ist eine Warnung");
    assert!(warning.contains("nicht geprüft"), "{warning}");
    assert!(warning.contains(ort_und_grund), "{warning}");
}

/// **Die zweite Ursache, an einem echten Lauf (Fix-Runde 5).** Seit der
/// Korrektur in `redact-pdf` ist die Entpackgrenze nicht mehr der einzige
/// Weg zu „nicht geprüft“: die Objektsicht meldet auch, wo der Objektgraph
/// tiefer ist, als sie geht (`MAX_DEPTH` = 32; der Lader lässt bis 100 zu).
/// Hier ist das Budget **voll** — der alte Satz „nicht geprüft
/// (Entpackgrenze)“ hätte eine Ursache behauptet, die es nicht gibt, und in
/// die falsche Richtung geschickt. Jetzt zählt der Satz die Stellen und gibt
/// ihren Grund wieder; er muss bis in die Statuszeile und in die Warnung
/// kommen. Die Gegenrichtung (Budget als Ursache) steht in
/// `ze_p5_5_ein_zu_kleines_budget_kommt_bis_in_den_satz`, beide Ursachen
/// nebeneinander in `zc_g5_tests::g5a2_nicht_geprueft_steht_im_satz_…`.
#[test]
fn ze_p5_5_die_tiefengrenze_kommt_auch_bis_in_den_satz() {
    let dir = tmp("5-zwei-gruende");
    let out = dir.join("tief.pdf");

    // Dieselbe Vorlage, dazu ein 40 Ebenen tiefes Objekt mit Klartext ganz
    // unten — genau die Stelle, die keine Sicht mehr liest.
    let mut doc = lopdf::Document::load_mem(&two_spellings()).expect("ladbar");
    let mut inner = lopdf::Dictionary::new();
    inner.set("Leck", lopdf::Object::string_literal(SPACED));
    let mut deep = lopdf::Object::Dictionary(inner);
    for _ in 0..40 {
        deep = lopdf::Object::Array(vec![deep]);
    }
    doc.add_object(deep);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("schreibbar");
    std::fs::write(&out, &bytes).expect("Datei");

    // Das volle Budget des Ladens: an der Entpackgrenze liegt es diesmal
    // **nicht**, und trotzdem ist die Antwort unvollständig.
    let plan = ExportCheckPlan {
        needles: vec![PLAIN.to_string()],
        ..ExportCheckPlan::default()
    };
    let check = plan.run(&out);
    println!("unchecked: {:#?}", check.unchecked);
    assert!(check.incomplete(), "{check:?}");

    let tiefe = check
        .unchecked
        .iter()
        .find(|u| u.contains("Verschachtelungstiefe"))
        .cloned()
        .unwrap_or_else(|| panic!("keine Tiefengrenze: {:#?}", check.unchecked));
    assert!(
        !check.unchecked.iter().any(|u| u.contains("Entpackgrenze")),
        "{:#?}",
        check.unchecked
    );

    let sentence = check.sentence();
    println!("Statuszeile: {sentence}");
    let warning = check.warning().expect("unvollständig ist eine Warnung");
    let (ort_und_grund, rest) = tiefe
        .split_once("; ")
        .expect("die volle Zeile erklärt nach");
    assert!(sentence.contains(ort_und_grund), "{sentence}");
    assert!(warning.contains(ort_und_grund), "{warning}");
    assert!(
        !sentence.contains(rest),
        "gekürzt, nicht abgeschrieben: {sentence}"
    );
    // Der alte Satz hätte hier die falsche Ursache genannt und in die falsche
    // Richtung geschickt: „(Entpackgrenze)“ an einem Lauf mit vollem Budget.
    assert!(
        !sentence.contains("nicht geprüft (Entpackgrenze)"),
        "{sentence}"
    );
}

/// Dass P5-1 kein Kunstgriff ist: die **Analyse** selbst liefert die beiden
/// Schreibweisen — Seite 1 ohne, Seite 2 mit Leerraum, beide echte
/// Mustertreffer, beide geschwärzt. Der Plan trägt jetzt wieder beide.
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
    assert_eq!(
        plan.needles,
        vec![PLAIN.to_string(), SPACED.to_string()],
        "{plan:?}"
    );
    // Und warum die eine Schreibweise die andere nicht mitsucht: sie trägt
    // keinen Leerraum, also gibt es zu ihr keine gequetschte Fassung.
    assert_eq!(redact_pdf::squeeze(PLAIN), PLAIN);
}
