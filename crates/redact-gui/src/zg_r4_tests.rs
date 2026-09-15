//! Gegenprüfung R4 (Fix-Runde 6): die Oberfläche nach dem Umbau der
//! Nachprüfung — `ExportCheck::unsearched`, die Sätze, die Warnungen, die
//! Kennung einer laufenden Prüfung.
//!
//! `cargo test -p redact-gui zg_r4`
//!
//! Kindmodul der Crate-Wurzel: `RedactApp::export_to`, `checks` und
//! `hold_check` sind privat, deshalb läuft hier der öffentliche Weg
//! (`AppState::export` → `plan_export_check` → `ExportCheckPlan::run`).
//! Was den Prüf-Thread selbst braucht, steht in `zg_r4_app_tests.rs`
//! (Kindmodul von `app`).
//!
//! Tests mit `#[ignore = "Befund …"]` tragen die **richtige** Erwartung und
//! sind bis zur Korrektur rot; der grüne Test daneben hält fest, was der Code
//! heute wirklich tut.

use std::path::{Path, PathBuf};

use redact_core::{MatchType, Rect, Region, Source};
use redact_pdf::testing::{build_pdf, TextItem};
use redact_pipeline::Config;

use crate::state::{AnnotatedRegion, ExportCheck, ExportCheckPlan, HitOutcome};
use crate::AppState;

// --------------------------------------------------------------- Hilfsmittel

fn no_patterns() -> Config {
    Config {
        no_patterns: true,
        ..Config::default()
    }
}

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zg-r4-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Eine Zeile mit bekanntem Text, wie sie aus einer Review-Datei käme.
fn text_region(page: usize, rect: Rect, text: &str) -> AnnotatedRegion {
    AnnotatedRegion::new(Region::new(
        page,
        rect,
        Some(text.to_string()),
        Source::Manual {
            reason: "R4".into(),
        },
    ))
}

/// Ein Mustertreffer — der einzige, den die Negativliste blockieren kann
/// (`AnnotatedRegion::is_blockable`).
fn pattern_region(page: usize, rect: Rect, text: &str) -> AnnotatedRegion {
    AnnotatedRegion::new(Region::new(
        page,
        rect,
        Some(text.to_string()),
        Source::Pattern {
            pattern_id: "r4".into(),
            confidence: 1.0,
        },
    ))
}

/// Ein Schutzeintrag der Negativliste.
fn protecting_region(page: usize, rect: Rect, text: Option<&str>) -> AnnotatedRegion {
    AnnotatedRegion::new(Region::new(
        page,
        rect,
        text.map(str::to_string),
        Source::Booking {
            booking_id: "schutz".into(),
            match_type: MatchType::Negative,
        },
    ))
}

/// Das Rechteck über Zeile `row` (0-basiert) einer Vorlage mit Zeilen bei
/// y = 700, 660, 620, …
fn over(row: usize) -> Rect {
    let y = 700.0 - 40.0 * row as f64;
    Rect::new(60.0, y - 10.0, 520.0, y + 14.0)
}

/// Vier Zeilen, jede trägt „Betrag“ — ein häufiger Text.
fn vier_betraege() -> Vec<u8> {
    build_pdf(&[vec![
        TextItem::new(72.0, 700.0, 10.0, "Betrag"),
        TextItem::new(72.0, 660.0, 10.0, "Betrag"),
        TextItem::new(72.0, 620.0, 10.0, "Betrag"),
        TextItem::new(72.0, 580.0, 10.0, "Betrag"),
    ]])
}

fn state_with(bytes: &[u8]) -> AppState {
    let mut state = AppState::with_config(no_patterns());
    state.load_bytes(bytes, None).expect("ladbar");
    state
}

fn export_and_check(state: &AppState, out: &Path) -> (ExportCheckPlan, ExportCheck) {
    let summary = state.hit_summary();
    state.export(out, None).expect("Export");
    let plan = state.plan_export_check(&summary);
    let check = plan.clone().run(out);
    (plan, check)
}

// ===========================================================================
// 1 — Jeder Weg zu `kept`/`unsearched`
// ===========================================================================

/// Jeder Weg, auf dem `kept`/`unsearched` entsteht, einzeln: abgewählte
/// Zeile, Negativliste (blockierter Mustertreffer), Schutzeintrag,
/// Handregion ohne Text, Text nur aus Leerraum, Datei nach dem Export
/// überschrieben, Datei nach dem Export gelöscht.
///
/// Geprüft wird gegen den Zustand, nicht gegen den Wortlaut: wo nicht
/// gesucht wurde, muss `unsearched` es zählen und die Warnung bleiben; wo
/// gesucht wurde, darf `unsearched` nichts zählen.
#[test]
fn zg_r4_1_jeder_weg_zu_kept_und_unsearched() {
    let dir = tmp("wege");

    // (a) Abgewählte Zeile: Zeile 0 geschwärzt (trifft), Zeile 1 abgewählt.
    let mut a = state_with(&vier_betraege());
    a.regions.push(text_region(0, over(0), "Betrag"));
    a.regions.push(text_region(0, over(1), "Betrag"));
    assert!(a.set_enabled(1, false));
    let summary = a.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.outcome(1), HitOutcome::Disabled);
    let (plan, check) = export_and_check(&a, &dir.join("a.pdf"));
    println!(
        "(a) abgewählt:   {}\n    Warnung: {:?}",
        check.sentence(),
        check.warning()
    );
    assert!(plan.needles.is_empty());
    assert_eq!((check.kept, check.unsearched, check.checked), (1, 1, 0));
    assert!(
        check.warning().is_some(),
        "nicht gesucht — die Warnung muss bleiben"
    );

    // (b) Negativliste: ein Mustertreffer, den ein Schutzeintrag derselben
    // Fläche blockiert; daneben eine geschwärzte Zeile mit demselben Text.
    let mut b = state_with(&vier_betraege());
    b.regions.push(text_region(0, over(0), "Betrag"));
    b.regions.push(pattern_region(0, over(1), "Betrag"));
    b.regions
        .push(protecting_region(0, over(1), Some("Kundennummer 4711")));
    let summary = b.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.outcome(1), HitOutcome::Blocked, "{summary:?}");
    assert_eq!(summary.outcome(2), HitOutcome::Protecting);
    let (plan, check) = export_and_check(&b, &dir.join("b.pdf"));
    println!(
        "(b) blockiert:   {}\n    Warnung: {:?}",
        check.sentence(),
        check.warning()
    );
    assert!(plan.needles.is_empty(), "{plan:?}");
    assert_eq!((check.kept, check.unsearched), (1, 1));
    assert!(check.warning().is_some());

    // (c) Schutzeintrag selbst trägt den Text: nur er und eine geschwärzte
    // Zeile mit demselben Text.
    let mut c = state_with(&vier_betraege());
    c.regions.push(text_region(0, over(0), "Betrag"));
    c.regions.push(protecting_region(
        0,
        Rect::new(60.0, 570.0, 520.0, 594.0),
        Some("Betrag"),
    ));
    let summary = c.hit_summary();
    assert_eq!(summary.outcome(1), HitOutcome::Protecting);
    let (plan, check) = export_and_check(&c, &dir.join("c.pdf"));
    println!(
        "(c) Schutz:      {}\n    Warnung: {:?}",
        check.sentence(),
        check.warning()
    );
    assert!(plan.needles.is_empty(), "{plan:?}");
    assert_eq!((check.kept, check.unsearched), (1, 1));
    assert!(check.warning().is_some());

    // (d) Handregion ohne Text über Zeile 1, daneben eine Zeile mit Text:
    // nichts ist gedeckt, die Handregion zählt als „ohne bekannten Text“.
    let mut d = state_with(&vier_betraege());
    d.regions.push(text_region(0, over(0), "Betrag"));
    assert!(d.add_manual_region(0, over(1), "Hand").is_some());
    let (plan, check) = export_and_check(&d, &dir.join("d.pdf"));
    println!(
        "(d) Hand:        {}\n    Warnung: {:?}",
        check.sentence(),
        check.warning()
    );
    assert_eq!(plan.needles, vec!["Betrag".to_string()]);
    assert_eq!(
        (check.kept, check.unsearched, check.without_text),
        (0, 0, 1)
    );
    // Zeilen 2 und 3 tragen „Betrag“ weiter — wörtlich, also ein Fund.
    assert!(check.found_leak(), "{}", check.sentence());
    assert!(check.warning().is_some());

    // (e) Text nur aus Leerraum: eine abgewählte Zeile „   “ deckt nichts,
    // eine geschwärzte Zeile „   “ ist ein Rechteck ohne bekannten Text.
    let mut e = state_with(&vier_betraege());
    e.regions.push(text_region(0, over(0), "Betrag"));
    e.regions.push(text_region(0, over(1), "   "));
    assert!(e.set_enabled(1, false));
    e.regions.push(text_region(0, over(2), " \t "));
    let (plan, check) = export_and_check(&e, &dir.join("e.pdf"));
    println!(
        "(e) Leerraum:    {}\n    Warnung: {:?}",
        check.sentence(),
        check.warning()
    );
    assert_eq!(plan.needles, vec!["Betrag".to_string()]);
    assert_eq!(
        (check.kept, check.unsearched, check.without_text),
        (0, 0, 1)
    );

    // (f) Nach dem Export überschrieben — mit der **Vorlage**, also ohne
    // jede Schwärzung. Mit einer abgewählten Zeile gleichen Texts sagt die
    // Prüfung: nichts. Ohne sie: Fund.
    let out = dir.join("f.pdf");
    let (plan_f, _) = export_and_check(&a, &out);
    std::fs::write(&out, vier_betraege()).unwrap();
    let check = plan_f.run(&out);
    println!(
        "(f) überschrieben, abgewählt: {}\n    Warnung: {:?}",
        check.sentence(),
        check.warning()
    );
    assert!(!check.found_leak(), "gar nicht gesucht");
    assert_eq!(check.unsearched, 1);
    let warning = check.warning().expect("die Warnung bleibt");
    assert!(warning.contains("sagt sie nichts"), "{warning}");
    let out2 = dir.join("f2.pdf");
    let (plan_f2, _) = export_and_check(&d, &out2);
    std::fs::write(&out2, vier_betraege()).unwrap();
    let check = plan_f2.run(&out2);
    println!("(f) überschrieben, gesucht:   {}", check.sentence());
    assert!(check.found_leak());

    // (g) Nach dem Export gelöscht: unlesbar, und `unsearched` bleibt, was
    // der Plan zählte.
    let gone = dir.join("g.pdf");
    let (plan_g, _) = export_and_check(&a, &gone);
    std::fs::remove_file(&gone).unwrap();
    let check = plan_g.run(&gone);
    println!(
        "(g) gelöscht:    {}\n    Warnung: {:?}",
        check.sentence(),
        check.warning()
    );
    assert!(check.unreadable.is_some());
    assert_eq!((check.kept, check.unsearched), (1, 1));
    let warning = check.warning().expect("unlesbar gehört in die Warnungen");
    assert!(warning.contains("zurücklesen"), "{warning}");

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Gegenrichtung.** Eine einzige abgewählte Zeile „Betrag“ nimmt der
/// Nachprüfung jede Aussage über **alle** geschwärzten „Betrag“-Zeilen —
/// auch wenn die Ausgabe danach mit der ungeschwärzten Vorlage überschrieben
/// wird, steht dort kein Alarm, nur „sagt nichts“.
///
/// Und das Orakel könnte es unterscheiden: [`redact_pdf::leaks_many_within`]
/// liefert je Begriff **alle** Fundstellen. Die Vorlage trägt vier, die
/// gewollte Ausgabe eine (die abgewählte Zeile). Der Zähler steht in der
/// Antwort — die Oberfläche fragt ihn nicht.
#[test]
fn zg_r4_1_eine_abgewaehlte_zeile_nimmt_jede_nachpruefung() {
    let dir = tmp("betrag");
    let out = dir.join("out.pdf");
    let mut state = state_with(&vier_betraege());
    for row in 0..4 {
        state.regions.push(text_region(0, over(row), "Betrag"));
    }
    assert!(state.set_enabled(3, false));
    let (plan, check) = export_and_check(&state, &out);
    println!(
        "gewollt:  {}\n  Warnung: {:?}",
        check.sentence(),
        check.warning()
    );
    assert!(plan.needles.is_empty());
    assert_eq!(check.unsearched, 1, "ein Text, drei Zeilen — eine Zahl");

    // Das Orakel an der gewollten Ausgabe: die abgewählte Zeile steht da —
    // je **Sicht** eine Fundstelle, nicht je Vorkommen.
    let gewollt = std::fs::read(&out).unwrap();
    let probe = redact_pdf::leaks_many_within(&gewollt, &["Betrag"], u64::MAX);
    println!(
        "Fundstellen in der gewollten Ausgabe ({}):\n  {}",
        probe.findings[0].len(),
        probe.findings[0].join("\n  ")
    );
    assert!(!probe.findings[0].is_empty(), "die abgewählte Zeile");
    let gewollt_stellen = probe.findings[0].len();

    // Danebengegangen (hier: die Datei ist wieder die Vorlage): viermal
    // „Betrag“ — und die Oberfläche sagt dasselbe wie oben.
    std::fs::write(&out, vier_betraege()).unwrap();
    let probe = redact_pdf::leaks_many_within(&vier_betraege(), &["Betrag"], u64::MAX);
    println!(
        "Fundstellen in der Vorlage ({}):\n  {}",
        probe.findings[0].len(),
        probe.findings[0].join("\n  ")
    );
    println!(
        "Fundstellen je Sicht, nicht je Vorkommen: {gewollt_stellen} gegen {} — \
         der Zähler des Orakels unterscheidet die beiden Dateien {}.",
        probe.findings[0].len(),
        if probe.findings[0].len() > gewollt_stellen {
            "doch"
        } else {
            "nicht"
        }
    );
    let daneben = plan.run(&out);
    println!(
        "daneben:  {}\n  Warnung: {:?}",
        daneben.sentence(),
        daneben.warning()
    );
    assert_eq!(
        daneben.sentence(),
        check.sentence(),
        "kein Unterschied in der Zeile"
    );
    assert_eq!(
        daneben.warning(),
        check.warning(),
        "kein Unterschied in der Warnung"
    );
    assert!(!daneben.found_leak());

    std::fs::remove_dir_all(&dir).ok();
}

/// Der andere Weg, eine Zeile stehen zu lassen: **löschen** statt abwählen
/// (Entf, [`AppState::delete_selected`]). Dann ist der Text nicht mehr
/// gedeckt, wird gesucht — und die bewusst stehen gelassene Zeile ist ein
/// „Fund“: „darf so nicht weitergegeben werden“ über eine Datei, die genau
/// so gewollt war (Befund 5 aus Fix-Runde 4, über den Umweg Löschen).
///
/// Beide Wege zusammen: wer eine Zeile behalten will, bekommt entweder eine
/// Warnung, die nie verschwindet (abwählen), oder einen falschen Alarm
/// (löschen). Einen Weg ohne beides gibt es nicht.
#[test]
fn zg_r4_1_loeschen_statt_abwaehlen_gibt_falschen_alarm() {
    let dir = tmp("loeschen");
    let out = dir.join("out.pdf");
    let mut state = state_with(&vier_betraege());
    for row in 0..4 {
        state.regions.push(text_region(0, over(row), "Betrag"));
    }
    state.selected_region = Some(3);
    assert!(state.delete_selected());
    assert_eq!(state.regions.len(), 3);
    let (plan, check) = export_and_check(&state, &out);
    println!(
        "gelöscht: {}\n  Warnung: {:?}",
        check.sentence(),
        check.warning()
    );
    assert_eq!(plan.needles, vec!["Betrag".to_string()]);
    assert_eq!(check.unsearched, 0);
    // Die vierte Zeile steht bewusst da — und ist der einzige Rest: der
    // Schriftdekoder sieht auf Seite 1 genau ein „Betrag“.
    let bytes = std::fs::read(&out).unwrap();
    let hits = redact_pdf::leaks(&bytes, "Betrag");
    println!("Fundstellen:\n  {}", hits.join("\n  "));
    assert!(!hits.is_empty());
    let seite: Vec<&String> = hits.iter().filter(|h| h.starts_with("Seite 1 ")).collect();
    assert_eq!(seite.len(), 1, "{hits:?}");
    assert!(
        check.found_leak(),
        "so verhält es sich heute: die gewollte Zeile ist ein Alarm — {}",
        check.sentence()
    );
    assert!(check
        .sentence()
        .contains("darf so nicht weitergegeben werden"));
    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 3 — Die Sätze: alle 128 Lagen
// ===========================================================================

/// Die sieben Achsen von [`ExportCheck`]: Fund, `kept` (der beurteilte
/// Teil), `unsearched`, `skipped`, `unchecked`, `unreadable`, `without_text`.
#[derive(Debug, Clone, Copy)]
struct Lage {
    fund: bool,
    judged: bool,
    unsearched: bool,
    skipped: bool,
    unchecked: bool,
    unreadable: bool,
    hand: bool,
}

impl Lage {
    fn all() -> Vec<Lage> {
        (0..128u8)
            .map(|bits| Lage {
                fund: bits & 1 != 0,
                judged: bits & 2 != 0,
                unsearched: bits & 4 != 0,
                skipped: bits & 8 != 0,
                unchecked: bits & 16 != 0,
                unreadable: bits & 32 != 0,
                hand: bits & 64 != 0,
            })
            .collect()
    }

    /// Kann [`ExportCheckPlan::run_within`] diese Lage liefern? Eine
    /// unlesbare Datei hat keinen Fund, keine ungeprüfte Stelle und kein
    /// Urteil am Fund (`checked == 0`).
    fn reachable(self) -> bool {
        !(self.unreadable && (self.fund || self.judged || self.unchecked))
    }

    fn check(self) -> ExportCheck {
        let judged = usize::from(self.judged);
        let unsearched = usize::from(self.unsearched);
        let leaking: Vec<String> = if self.fund {
            vec!["GEHEIM".into()]
        } else {
            Vec::new()
        };
        // Ein sauber gesuchter Begriff kommt immer dazu, damit `checked` die
        // Funde und Urteile trägt — so wie `run_within` es füllt.
        let checked = if self.unreadable {
            0
        } else {
            leaking.len() + judged + 1
        };
        ExportCheck {
            unreadable: self.unreadable.then(|| "Datei fehlt".to_string()),
            checked,
            leaking,
            without_text: usize::from(self.hand) * 2,
            skipped: usize::from(self.skipped) * 3,
            kept: judged + unsearched,
            unsearched,
            limit: redact_core::MAX_CHECK_NEEDLES,
            unchecked: if self.unchecked {
                vec!["Objekt 7 0: nicht entpackt — 67 Byte gepackt, entpackt mehr".into()]
            } else {
                Vec::new()
            },
        }
    }
}

/// Was der Satz über die Lage behauptet — und was er nicht behaupten darf.
/// Geprüft auf Wahrheit gegen den Zustand, nicht auf Wortlaut.
fn pruefe_satz(lage: Lage, check: &ExportCheck) -> Vec<String> {
    let s = check.sentence();
    let w = check.warning();
    let mut fehler = Vec::new();
    let mut muss = |bedingung: bool, was: &str| {
        if !bedingung {
            fehler.push(format!(
                "{lage:?}: {was}\n    Satz: {s}\n    Warnung: {w:?}"
            ));
        }
    };
    let entwarnung = s.contains("stehen nicht mehr in der Ausgabe");
    let fund = s.contains("NOCH in der Ausgabe");
    let nichts_gesucht = s.contains("es wurde nichts gesucht") || s.contains("nichts nachgeprüft");

    muss(
        s.starts_with("Nachprüfung: "),
        "beginnt nicht mit „Nachprüfung: “",
    );
    // Ein Fund führt, und nichts im Satz nimmt ihn zurück.
    muss(fund == check.found_leak(), "Fund im Satz ≠ Fund im Zustand");
    if check.found_leak() {
        muss(
            !entwarnung && !nichts_gesucht,
            "Fund und Entwarnung im selben Satz",
        );
        muss(s.starts_with("Nachprüfung: 1 von "), "der Fund führt nicht");
        muss(
            w.as_deref() == Some(s.as_str()),
            "die Warnung ist nicht der ganze Satz",
        );
    }
    // Unlesbar: keine Aussage, und nichts, was wie eine klingt.
    if check.unreadable.is_some() {
        muss(s.contains("zurücklesen"), "unlesbar ohne Grund");
        muss(!entwarnung && !fund, "unlesbar, aber ein Urteil");
        muss(w.as_deref() == Some(s.as_str()), "unlesbar ohne Warnung");
    } else {
        muss(
            s.contains("Geprüft ist genau diese Liste, nicht die Datei."),
            "der Vorbehalt fehlt",
        );
        // Genau eine Kopfaussage.
        muss(
            usize::from(fund) + usize::from(entwarnung) + usize::from(nichts_gesucht) == 1,
            "nicht genau eine Kopfaussage",
        );
        // Entwarnung nur, wenn wirklich etwas gesucht und nichts gefunden wurde.
        muss(
            entwarnung == (check.checked > 0 && !check.found_leak()),
            "Entwarnung ≠ (gesucht und nichts gefunden)",
        );
        if entwarnung {
            muss(
                s.contains(&format!("{} gesuchte Text(e)", check.checked)),
                "die Zahl der gesuchten Texte fehlt",
            );
        }
    }
    // Ungeprüfte Stellen: Zahl und Grund, und die Warnung ist der Satz.
    if check.incomplete() {
        muss(
            s.contains(&format!(
                "{} Stelle(n) wurden nicht geprüft",
                check.unchecked.len()
            )),
            "ungeprüfte Stellen ohne Zahl",
        );
        muss(s.contains("nicht entpackt"), "der Grund der Stelle fehlt");
        muss(!s.contains("entpackt mehr"), "die Stelle ist nicht gekürzt");
        if check.unreadable.is_none() {
            muss(
                w.as_deref() == Some(s.as_str()),
                "unvollständig ohne ganzen Satz als Warnung",
            );
        }
    }
    // Nicht gesucht (Decke): Zahl und Decke im Satz — und in der Warnung.
    if check.skipped > 0 && check.unreadable.is_none() {
        muss(
            s.contains(&format!(
                "{} weitere Text(e) wurden nicht gesucht",
                check.skipped
            )) && s.contains(&format!("höchstens {} Begriffe", check.limit)),
            "Decke ohne Zahl oder Grenze im Satz",
        );
        // Befund R4 (siehe `zg_r4_3_die_warnung_muesste_beide_gruende_nennen`):
        // steht daneben `unsearched`, verdeckt es die Decke in der Warnung.
        // Die vier Lagen sind hier ausgenommen, damit der Rest geprüft bleibt.
        if check.unsearched == 0 {
            muss(
                w.as_deref()
                    .is_some_and(|w| w.contains(&format!("{}", check.skipped))),
                "die Zahl der Texte jenseits der Decke fehlt in der Warnung",
            );
        }
    }
    // Nicht gesucht (wörtlich gedeckt): kein Urteil, und die Warnung bleibt.
    if check.unsearched > 0 {
        muss(
            check.unreadable.is_some()
                || s.contains(&format!(
                    "{} Text(e) stehen wörtlich auch in einer abgewählten oder geschützten Zeile",
                    check.unsearched
                )),
            "unsearched ohne Zahl im Satz",
        );
        muss(
            w.as_deref().is_some_and(|w| w.contains("nichts")),
            "unsearched ohne Warnung",
        );
    }
    // Der beurteilte Teil von `kept` — mit **seiner** Zahl, nicht mit `kept`.
    let judged = check.kept - check.unsearched;
    muss(
        check.unreadable.is_some() || s.contains("zählen deshalb nicht als Leck") == (judged > 0),
        "„zählen nicht als Leck“ ≠ beurteilter Teil",
    );
    if judged > 0 && check.unreadable.is_none() {
        muss(
            s.contains(&format!("{judged} Text(e) decken sich")),
            "das Urteil trägt die falsche Zahl",
        );
    }
    // Handregionen stehen immer im Satz.
    muss(
        s.contains("Rechteck(e) ohne bekannten Text") == (check.without_text > 0),
        "Handregionen ≠ Zustand",
    );
    // Vorbehalt vor Entwarnung.
    if entwarnung && !check.found_leak() {
        for marke in [
            "wurden nicht geprüft",
            "wurden nicht gesucht",
            "sagt diese Prüfung nichts",
        ] {
            if let Some(at) = s.find(marke) {
                muss(
                    at < s.find("stehen nicht mehr in der Ausgabe").unwrap(),
                    &format!("„{marke}“ steht hinter der Entwarnung"),
                );
            }
        }
    }
    // Nichts zu warnen ⇔ kein Fund, keine Lücke, nichts unlesbar.
    let ruhig = !check.found_leak()
        && !check.incomplete()
        && check.unreadable.is_none()
        && check.skipped == 0
        && check.unsearched == 0;
    muss(w.is_none() == ruhig, "Warnung ≠ (etwas zu warnen)");
    fehler
}

/// **Alle 128 Lagen** aus den sieben Achsen, jede gegen ihren Zustand
/// geprüft — und die Länge je Lage gemessen.
///
/// Unerreichbare Lagen (unlesbar **und** Fund/Urteil/ungeprüfte Stelle —
/// `run_within` kehrt vor der Suche um) werden gebaut und ausgegeben, aber
/// nicht bewertet.
#[test]
fn zg_r4_3_alle_128_lagen_gegen_den_zustand() {
    let mut fehler = Vec::new();
    let mut laengste = (0usize, String::new());
    let mut erreichbar = 0;
    for lage in Lage::all() {
        let check = lage.check();
        let s = check.sentence();
        let n = s.chars().count();
        if n > laengste.0 {
            laengste = (n, s.clone());
        }
        if !lage.reachable() {
            println!("[unerreichbar, {n} Zeichen] {s}");
            continue;
        }
        erreichbar += 1;
        println!("[{n} Zeichen] {s}");
        fehler.extend(pruefe_satz(lage, &check));
    }
    println!(
        "erreichbar: {erreichbar} von 128; längster Satz: {} Zeichen",
        laengste.0
    );
    println!("  {}", laengste.1);
    assert_eq!(erreichbar, 128 - 7 * 8);
    assert!(
        fehler.is_empty(),
        "{} Lage(n) sagen etwas Falsches:\n{}",
        fehler.len(),
        fehler.join("\n")
    );
}

/// Was die 128 Lagen gemeinsam festhalten, hier für den einen Fall
/// einzeln — Fix-Runde 6 hat den Vorbehalt „über sie sagt diese Prüfung
/// nichts“ **vor** die Decke gesetzt: sind beide da, nennt die Warnung
/// nur noch den einen Grund. Die Zahl der Texte jenseits der Decke und die
/// Decke selbst stehen nur in der Statuszeile, die die nächste Aktion
/// überschreibt.
///
/// So verhält es sich heute; die richtige Erwartung steht daneben.
#[test]
fn zg_r4_3_unsearched_verdeckt_die_decke_in_der_warnung_heute() {
    let check = ExportCheck {
        checked: 1,
        skipped: 5,
        kept: 1,
        unsearched: 1,
        limit: 1,
        ..ExportCheck::default()
    };
    println!("Satz:    {}", check.sentence());
    let warning = check.warning().expect("unvollständig");
    println!("Warnung: {warning}");
    assert!(warning.contains("über 1 Text(e) sagt sie nichts"));
    assert!(!warning.contains('5'), "heute fehlt die Decke: {warning}");
    // Und über den Lauf, nicht nur über den Bau: eine Decke von 1 mit zwei
    // Begriffen und einer gedeckten Zeile.
    let dir = tmp("verdeckt");
    let out = dir.join("out.pdf");
    let mut state = state_with(&vier_betraege());
    state.regions.push(text_region(0, over(0), "Betrag"));
    state.regions.push(text_region(0, over(1), "Betrag"));
    assert!(state.set_enabled(1, false));
    state.regions.push(text_region(0, over(2), "Anderer Text"));
    state.regions.push(text_region(0, over(3), "Dritter Text"));
    let summary = state.hit_summary();
    state.export(&out, None).expect("Export");
    let check = state.plan_export_check(&summary).run_within(&out, 1);
    println!(
        "Lauf:    {}\nWarnung: {:?}",
        check.sentence(),
        check.warning()
    );
    assert_eq!((check.checked, check.skipped, check.unsearched), (1, 1, 1));
    assert!(!check.warning().unwrap().contains("höchstens"));
    std::fs::remove_dir_all(&dir).ok();
}

/// **Befund R4 (die richtige Erwartung):** stehen Texte jenseits der Decke
/// **und** wörtlich gedeckte Texte in derselben Prüfung, muss die Warnung
/// beide Gründe nennen — sonst fehlt in der Liste, die bleibt, die Zahl
/// der gar nicht gesuchten Begriffe und die Decke, die sie erklärt.
#[test]
#[ignore = "Befund R4: die Warnung nennt bei unsearched > 0 die Decke (skipped) nicht mehr"]
fn zg_r4_3_die_warnung_muesste_beide_gruende_nennen() {
    let check = ExportCheck {
        checked: 1,
        skipped: 5,
        kept: 1,
        unsearched: 1,
        limit: 1,
        ..ExportCheck::default()
    };
    let warning = check.warning().expect("unvollständig");
    assert!(
        warning.contains("5") && warning.contains("höchstens 1"),
        "die Decke fehlt in der Warnung: {warning}"
    );
}

// ===========================================================================
// 2 — Die Kennung einer Prüfung ist ein `PathBuf`
// ===========================================================================

/// `start_export_check` beendet die ältere Prüfung **derselben Datei** —
/// entschieden mit `pending.out != out`, also am Pfad Zeichen für Zeichen
/// (nach Komponenten). Was für den Dateisystem dieselbe Datei ist, ist für
/// diesen Vergleich eine andere Prüfung: `./a.pdf`, `x/../y/a.pdf`, ein
/// Symlink. Die Gegenrichtung des Vergleichs steht in
/// `zg_r4_app_tests::zg_r4_2_dieselbe_datei_unter_anderem_namen_haelt_die_alte_pruefung_am_leben`.
#[test]
fn zg_r4_2_pfadschreibweisen_sind_verschiedene_kennungen() {
    let dir = tmp("pfade");
    let a = dir.join("a.pdf");
    std::fs::write(&a, b"x").unwrap();
    // Ein `.` **mitten** im Pfad normalisiert der Vergleich weg — das ist
    // dieselbe Kennung. Ein führendes `./`, ein `..` und ein Symlink nicht.
    let dot_inside = dir.join(".").join("a.pdf");
    assert_eq!(
        dot_inside, a,
        "„x/./a.pdf“ ist dieselbe Kennung wie „x/a.pdf“"
    );
    assert_ne!(
        Path::new("./a.pdf"),
        Path::new("a.pdf"),
        "ein führendes „./“ bleibt stehen"
    );
    let up = dir.join("..").join(dir.file_name().unwrap()).join("a.pdf");
    let link = dir.join("link");
    std::os::unix::fs::symlink(&dir, &link).unwrap();
    let via_link = link.join("a.pdf");
    for (name, other) in [("../", &up), ("Symlink", &via_link)] {
        let gleiche_datei =
            std::fs::canonicalize(other).unwrap() == std::fs::canonicalize(&a).unwrap();
        println!(
            "{name}: {} — dieselbe Datei: {gleiche_datei}, dieselbe Kennung: {}",
            other.display(),
            *other == a
        );
        assert!(gleiche_datei);
        assert_ne!(*other, a, "der Pfadvergleich hält sie für zwei Dateien");
    }
    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 1b — Ist die ehrliche Aussage auch nützlich?
// ===========================================================================

/// **Der Preis der Ehrlichkeit.** Die Zeile, die „Betrag“ trägt, wird
/// abgewählt; das Rechteck der geschwärzten Zeile daneben ist so groß, dass
/// es **beide** Vorkommen entfernt. Danach steht „Betrag“ **nachweislich
/// nirgends** mehr in der Ausgabe — und die Oberfläche sagt trotzdem „über
/// sie sagt diese Prüfung nichts“, mit einer Warnung, die bleibt.
///
/// Die Entwarnung wäre umsonst zu haben: derselbe Plan, nur mit dem Begriff
/// in `needles` statt in `kept`, liefert „steht nicht mehr in der Ausgabe“
/// und **keine** Warnung. Nicht gefunden ist eine Aussage, und zwar eine
/// sichere — nur der **Fund** ist zweideutig. `plan_export_check` wirft beides
/// zusammen weg, weil es vor der Suche entscheidet.
#[test]
fn zg_r4_1_ein_text_der_nachweislich_weg_ist_bleibt_trotzdem_ungeprueft() {
    let dir = tmp("nuetzlich");
    let out = dir.join("out.pdf");
    // Zwei Zeilen, beide „Betrag“ — mehr gibt es in dieser Vorlage nicht.
    let doc = build_pdf(&[vec![
        TextItem::new(72.0, 700.0, 10.0, "Betrag"),
        TextItem::new(72.0, 660.0, 10.0, "Betrag"),
    ]]);
    let mut state = state_with(&doc);
    // Ein Rechteck über Zeile 0 **und** Zeile 1.
    state.regions.push(text_region(
        0,
        Rect::new(60.0, 650.0, 520.0, 714.0),
        "Betrag",
    ));
    // Zeile 1 ist abgewählt — bewusst stehen gelassen, aber das Rechteck
    // darüber räumt sie mit ab.
    state.regions.push(text_region(0, over(1), "Betrag"));
    assert!(state.set_enabled(1, false));
    let summary = state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.outcome(1), HitOutcome::Disabled);
    state.export(&out, None).expect("Export");
    let bytes = std::fs::read(&out).unwrap();

    // Der Nachweis: „Betrag“ steht auf Seite 1 nicht mehr.
    let rest = redact_pdf::leaks(&bytes, "Betrag");
    println!("Fundstellen „Betrag“ in der Ausgabe: {}", rest.len());
    assert!(rest.is_empty(), "„Betrag“ steht noch da: {rest:?}");

    let plan = state.plan_export_check(&summary);
    let check = plan.run(&out);
    println!("heute:   {}", check.sentence());
    println!("Warnung: {:?}", check.warning());
    assert_eq!((check.kept, check.unsearched, check.checked), (1, 1, 0));
    assert!(
        check.warning().is_some(),
        "die Warnung bleibt, obwohl nichts mehr da ist"
    );

    // Derselbe Begriff, nur gesucht statt übersprungen: eine sichere
    // Entwarnung, umsonst.
    let gesucht = ExportCheckPlan {
        needles: vec!["Betrag".to_string()],
        kept_forms: vec![true],
        ..ExportCheckPlan::default()
    }
    .run(&out);
    println!("gesucht: {}", gesucht.sentence());
    println!("Warnung: {:?}", gesucht.warning());
    assert!(!gesucht.found_leak(), "nichts gefunden");
    assert_eq!(gesucht.checked, 1);
    assert!(
        gesucht.warning().is_none(),
        "nicht gefunden ist eine Aussage: {:?}",
        gesucht.warning()
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 3b — Wo der Satz sich selbst widerspricht
// ===========================================================================

/// **Die Entwarnung zählt einen Text mit, den das Orakel gefunden hat.**
/// `checked` ist die Zahl der **gesuchten** Schreibweisen, nicht die der
/// verschwundenen. Trifft der einzige gesuchte Begriff (nur ohne Leerraum,
/// gedeckt von einer abgewählten Zeile), sagt derselbe Satz beides: „1
/// gesuchte Text(e) stehen nicht mehr in der Ausgabe“ **und** „1 Text(e)
/// decken sich mit einer abgewählten … Zeile“ — über **denselben** Text.
#[test]
fn zg_r4_3_die_entwarnung_zaehlt_den_gefundenen_text_mit() {
    const PLAIN: &str = "DE89370400440532013000";
    const SPACED: &str = "DE89 3704 0044 0532 0130 00";
    let dir = tmp("doppelt");
    let out = dir.join("out.pdf");
    let doc = build_pdf(&[vec![
        TextItem::new(72.0, 700.0, 10.0, format!("A {PLAIN}")),
        TextItem::new(72.0, 660.0, 10.0, format!("B {SPACED}")),
    ]]);
    let mut state = state_with(&doc);
    // Geschwärzt wird die Schreibweise **mit** Leerraum; die abgewählte
    // Zeile trägt dieselbe Normalform ohne.
    state.regions.push(text_region(0, over(1), SPACED));
    state.regions.push(text_region(0, over(0), PLAIN));
    assert!(state.set_enabled(1, false));
    let summary = state.hit_summary();
    state.export(&out, None).expect("Export");
    let plan = state.plan_export_check(&summary);
    assert_eq!(plan.needles, vec![SPACED.to_string()]);
    assert_eq!(plan.kept_forms, vec![true]);
    let check = plan.run(&out);
    println!("Satz: {}", check.sentence());
    assert_eq!(
        (
            check.checked,
            check.kept,
            check.unsearched,
            check.leaking.len()
        ),
        (1, 1, 0, 0)
    );
    let s = check.sentence();
    assert!(
        s.contains("1 gesuchte Text(e) stehen nicht mehr in der Ausgabe")
            && s.contains("1 Text(e) decken sich"),
        "{s}"
    );
    // Und das Orakel hat den Begriff sehr wohl gefunden — nur nicht wörtlich.
    let bytes = std::fs::read(&out).unwrap();
    let probe = redact_pdf::leaks_many_within(&bytes, &[SPACED], u64::MAX);
    println!(
        "Fundstellen: {} (wörtlich: {})",
        probe.findings[0].len(),
        probe.literal[0]
    );
    assert!(!probe.findings[0].is_empty(), "der Begriff steht noch da");
    assert!(!probe.literal[0], "aber nicht wörtlich");
    std::fs::remove_dir_all(&dir).ok();
}

/// **Zwei Kopfaussagen, die einander ausschließen.** Mit einer Decke von 0
/// ([`ExportCheckPlan::run_within`] ist `pub`) steht im selben Satz „2
/// weitere Text(e) wurden nicht gesucht“ und „keine geschwärzte Zeile mit
/// bekanntem Text“ — es gab zwei, sie lagen nur über der Decke. Auch
/// „**weitere**“ ist falsch: es gab keine ersten.
#[test]
fn zg_r4_3_bei_der_decke_null_widerspricht_sich_der_satz() {
    let dir = tmp("decke-null");
    let out = dir.join("out.pdf");
    let mut state = state_with(&vier_betraege());
    state.regions.push(text_region(0, over(0), "Betrag"));
    state.regions.push(text_region(0, over(1), "Anderer"));
    let summary = state.hit_summary();
    state.export(&out, None).expect("Export");
    let check = state.plan_export_check(&summary).run_within(&out, 0);
    let s = check.sentence();
    println!("Satz: {s}");
    assert_eq!((check.checked, check.skipped), (0, 2));
    assert!(s.contains("2 weitere Text(e) wurden nicht gesucht"), "{s}");
    assert!(
        s.contains("eine geschwärzte Zeile mit bekanntem Text"),
        "derselbe Satz sagt beides: {s}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// **Die 804 sind wieder da.** Fix-Runde 6 hat *eine* ungeprüfte Stelle
/// gekürzt (804 → 411 Zeichen, `zf_q4_3_…`). Gekürzt ist damit ein
/// **Bestandteil**, nicht die Zeile: die anderen Teilsätze sind unberührt,
/// und sie treten zusammen auf. Gemessen mit den **echten** Stellen aus
/// einem Lauf über 60 zu große Ströme, dazu Fund, Decke, wörtlich gedeckte
/// Texte, ein Urteil am Fund und Handregionen — jede Achse einzeln in
/// `zg_r4_3_alle_128_lagen_gegen_den_zustand` belegt.
#[test]
fn zg_r4_3_die_laengste_statuszeile_ist_wieder_ueber_804_zeichen() {
    let dir = tmp("laenge");
    let out = dir.join("viele.pdf");
    // 60 gepackte Ströme, jeder für sich zu groß für das Budget.
    let items: Vec<Vec<TextItem>> = (0..60)
        .map(|p| {
            vec![TextItem::new(
                20.0,
                700.0,
                10.0,
                format!("Seite {p} {}", "x".repeat(400)),
            )]
        })
        .collect();
    let mut doc = lopdf::Document::load_mem(&build_pdf(&items)).expect("ladbar");
    let ids: Vec<lopdf::ObjectId> = doc.objects.keys().copied().collect();
    for id in ids {
        if let Ok(stream) = doc
            .get_object_mut(id)
            .and_then(lopdf::Object::as_stream_mut)
        {
            stream.compress().expect("packbar");
        }
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("speicherbar");
    std::fs::write(&out, &bytes).unwrap();

    // Ein Lauf mit winzigem Budget: die echten Stellen.
    let echt = ExportCheckPlan {
        needles: vec!["Betrag".to_string()],
        kept_forms: vec![false],
        max_decompressed_bytes: 64,
        ..ExportCheckPlan::default()
    }
    .run(&out);
    let nur_stellen = echt.sentence();
    println!(
        "nur ungeprüfte Stellen ({} Zeichen): {nur_stellen}",
        nur_stellen.chars().count()
    );
    assert!(echt.incomplete(), "{:?}", echt.unchecked);

    // Dieselben Stellen, dazu die übrigen Teilsätze — jeder einzeln erreicht.
    let voll = ExportCheck {
        unreadable: None,
        checked: 3,
        leaking: vec!["GEHEIM".to_string()],
        without_text: 2,
        skipped: 3,
        kept: 2,
        unsearched: 1,
        limit: redact_core::MAX_CHECK_NEEDLES,
        unchecked: echt.unchecked.clone(),
    };
    let s = voll.sentence();
    let n = s.chars().count();
    println!("die volle Statuszeile ({n} Zeichen):\n{s}");
    assert!(
        n > 804,
        "die Zeile ist kürzer als die 804, die Fix-Runde 6 abstellen wollte ({n})"
    );
    std::fs::remove_dir_all(&dir).ok();
}
