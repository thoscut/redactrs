//! Gegenprüfung Q4 (Fix-Runde 5): die Entscheidung am Fund, die Decke in
//! Schreibweisen, die Sätze der Nachprüfung.
//!
//! `cargo test -p redact-gui zf_q4`
//!
//! Tests mit `#[ignore = "Befund …"]` tragen die **richtige** Erwartung und
//! sind bis zur Korrektur rot; der grüne Test daneben hält fest, was der Code
//! heute wirklich tut.

use std::path::PathBuf;

use redact_core::{Rect, Region, Source};
use redact_pdf::testing::{build_pdf, TextItem};
use redact_pipeline::Config;

use crate::state::{AnnotatedRegion, ExportCheck, ExportCheckPlan, HitOutcome};
use crate::RedactApp;

// --------------------------------------------------------------- Hilfsmittel

/// Dieselbe IBAN in drei Schreibweisen — eine Normalform.
const PLAIN: &str = "DE89370400440532013000";
const SPACED: &str = "DE89 3704 0044 0532 0130 00";
const GROUPED: &str = "DE89 37040044 05320130 00";

fn no_patterns() -> Config {
    Config {
        no_patterns: true,
        ..Config::default()
    }
}

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zf-q4-{tag}-{}-{:?}",
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
            reason: "Q4".into(),
        },
    ))
}

/// Ein Rechteck, unter dem kein Text liegt — die Zeile gilt als geschwärzt,
/// entfernt aber nichts. Genau der Fall, den Fix-Runde 5 für die Normalform
/// geschlossen hat (Befund P5-2).
fn empty_corner(i: usize) -> Rect {
    let x = 400.0 + (i % 40) as f64 * 3.0;
    Rect::new(x, 20.0, x + 2.0, 40.0)
}

/// Ein eigenes, kleines Rechteck je Nummer — ohne Überschneidung, damit die
/// Konfliktauflösung jede Zeile als eigene Schwärzung stehen lässt. Unter
/// keinem liegt Text (die Zeilen der Vorlage stehen weit oben).
fn spot(i: usize) -> Rect {
    let x = 10.0 + (i % 30) as f64 * 19.0;
    let y = 10.0 + (i / 30) as f64 * 8.0;
    Rect::new(x, y, x + 2.0, y + 2.0)
}

/// Drei Zeilen, je eine Schreibweise, in dieser Reihenfolge.
fn three_spellings() -> Vec<u8> {
    build_pdf(&[vec![
        TextItem::new(72.0, 700.0, 10.0, format!("A {PLAIN}")),
        TextItem::new(72.0, 660.0, 10.0, format!("B {SPACED}")),
        TextItem::new(72.0, 620.0, 10.0, format!("C {GROUPED}")),
    ]])
}

/// Das Rechteck über Zeile `row` (0-basiert) der Vorlage oben.
fn over(row: usize) -> Rect {
    let y = 700.0 - 40.0 * row as f64;
    Rect::new(60.0, y - 10.0, 520.0, y + 14.0)
}

/// Was mit einer Schreibweise geschehen soll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// geschwärzt, Rechteck trifft — die Schreibweise ist danach weg
    Hit,
    /// geschwärzt, Rechteck geht daneben — sie steht noch da (echtes Leck)
    Miss,
    /// abgewählt — sie steht bewusst noch da
    Off,
}

/// Baut die Lage, exportiert und gibt (Bytes, Plan, Urteil) zurück.
fn run_case(tag: &str, roles: [Role; 3]) -> (Vec<u8>, ExportCheckPlan, ExportCheck) {
    let dir = tmp(tag);
    let out = dir.join(format!("{roles:?}.pdf").replace([' ', ',', '[', ']'], "_"));
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&three_spellings(), "drei.pdf");
    for (row, (role, text)) in roles.iter().zip([PLAIN, SPACED, GROUPED]).enumerate() {
        let rect = match role {
            Role::Hit | Role::Off => over(row),
            Role::Miss => empty_corner(row),
        };
        app.state.regions.push(text_region(0, rect, text));
    }
    for (index, role) in roles.iter().enumerate() {
        if *role == Role::Off {
            assert!(app.state.set_enabled(index, false));
        }
    }
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");
    let bytes = std::fs::read(&out).unwrap();
    let plan = app.state.plan_export_check(&summary);
    let check = plan.clone().run(&out);
    (bytes, plan, check)
}

// ===========================================================================
// 1 — Die Entscheidung am Fund: drei Schreibweisen, alle Rollen
// ===========================================================================

/// **Alle 26 Rollenverteilungen** über drei Schreibweisen derselben
/// Normalform (alles abgewählt geht nicht: dann lehnt der Export ab).
///
/// Das Orakel sind die geschriebenen Bytes ([`redact_pdf::leaks_many_within`]),
/// die Wahrheit ist die Rollenliste: eine daneben gegangene Schwärzung
/// (`Miss`) lässt ihre Schreibweise wörtlich stehen, und das ist ein Leck —
/// gleich, was daneben abgewählt ist. Der Satz muss es sagen.
///
/// Mutation (`literal` im Lauf ignoriert, also Entscheidung wieder im Plan):
/// rot, sieben Fälle werden still.
#[test]
fn zf_q4_1_drei_schreibweisen_alle_rollen() {
    let roles = [Role::Hit, Role::Miss, Role::Off];
    let mut still = Vec::new();
    let mut falscher_alarm = Vec::new();
    let mut faelle = 0;
    for a in roles {
        for b in roles {
            for c in roles {
                let case = [a, b, c];
                if case.iter().all(|r| *r == Role::Off) {
                    continue; // „Nichts ausgewählt“ — der Export lehnt ab.
                }
                faelle += 1;
                let (bytes, plan, check) = run_case("matrix", case);
                let probe =
                    redact_pdf::leaks_many_within(&bytes, &[PLAIN, SPACED, GROUPED], u64::MAX);
                // Wahrheit aus der Vorlage: `Miss` steht wörtlich noch da,
                // `Hit` ist weg. Das Orakel bestätigt beides.
                for (i, role) in case.iter().enumerate() {
                    match role {
                        Role::Hit => assert!(
                            probe.findings[i].is_empty() || !probe.literal[i],
                            "{case:?}: Schreibweise {i} sollte weg sein: {:?}",
                            probe.findings[i]
                        ),
                        _ => assert!(
                            !probe.findings[i].is_empty() && probe.literal[i],
                            "{case:?}: Schreibweise {i} sollte wörtlich dastehen"
                        ),
                    }
                }
                // Alle drei Schreibweisen haben **eine** Normalform: eine
                // abgewählte Zeile deckt damit jede andere Schreibweise —
                // gesucht wird sie trotzdem.
                let gedeckt = case.contains(&Role::Off);
                assert_eq!(
                    plan.kept_forms,
                    vec![gedeckt; plan.needles.len()],
                    "{case:?}: {plan:?}"
                );
                let leckt = case.contains(&Role::Miss);
                println!(
                    "{case:?} needles={} kept_forms={:?} kept_literal={:?} -> {}",
                    plan.needles.len(),
                    plan.kept_forms,
                    plan.kept_literal,
                    check.sentence()
                );
                if leckt && !check.found_leak() {
                    still.push(format!("{case:?}: {}", check.sentence()));
                }
                if !leckt && check.found_leak() {
                    falscher_alarm.push(format!("{case:?}: {}", check.sentence()));
                }
            }
        }
    }
    assert_eq!(faelle, 26);
    assert!(still.is_empty(), "stille Lecks:\n{}", still.join("\n"));
    assert!(
        falscher_alarm.is_empty(),
        "falsche Alarme:\n{}",
        falscher_alarm.join("\n")
    );
}

/// Ein Text, der in der Ausgabe **beides** ist: an einer Stelle wörtlich, an
/// einer anderen nur ohne Leerraum. [`redact_pdf::LeakCheck::literal`] ist
/// dann `true` (die Marke ist ein Oder über alle Fundstellen), und die
/// Entscheidung ist damit richtig — die wörtliche Stelle ist das Leck.
///
/// Vorlage: Zeile A setzt die IBAN in Stücken (im Strom steht **kein**
/// Leerzeichen, der Extraktor macht aus den Lücken welche), Zeile B setzt
/// dieselbe Schreibweise mit echten Leerzeichen. A wird geschwärzt und das
/// Rechteck geht daneben, B ist abgewählt.
/// Mutation (`literal` im Lauf ignoriert): rot.
#[test]
fn zf_q4_1_woertlich_und_gequetscht_zugleich_ist_ein_leck() {
    let out = tmp("beides").join("out.pdf");
    let mut page = kerned_line(700.0, "A");
    page.push(TextItem::new(72.0, 640.0, 10.0, format!("B {SPACED}")));
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&build_pdf(&[page]), "beides.pdf");
    app.state
        .regions
        .push(text_region(0, empty_corner(2), SPACED));
    app.state.regions.push(text_region(0, over(1), PLAIN));
    assert!(app.state.set_enabled(1, false));
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.outcome(1), HitOutcome::Disabled);
    app.state.export(&out, None).expect("Export");

    let bytes = std::fs::read(&out).unwrap();
    let probe = redact_pdf::leaks_many_within(&bytes, &[SPACED], u64::MAX);
    let (mit, ohne): (Vec<_>, Vec<_>) = probe.findings[0]
        .iter()
        .partition(|f| !f.contains("ohne Leerraum"));
    println!("wörtlich: {} · nur gequetscht: {}", mit.len(), ohne.len());
    assert!(!mit.is_empty() && !ohne.is_empty(), "beides muss vorkommen");
    assert!(probe.literal[0], "die Marke ist ein Oder über alle Funde");

    let plan = app.state.plan_export_check(&summary);
    assert_eq!(plan.needles, vec![SPACED.to_string()]);
    assert_eq!(
        plan.kept_forms,
        vec![true],
        "die abgewählte Zeile deckt sie"
    );
    let check = plan.run(&out);
    println!("Satz: {}", check.sentence());
    assert!(
        check.found_leak(),
        "wörtlich in der Ausgabe ist ein Leck: {}",
        check.sentence()
    );
}

/// Die IBAN in Stücken gesetzt — im Strom steht kein Leerzeichen, der
/// Extraktor macht aus den Lücken welche.
fn kerned_line(y: f64, prefix: &str) -> Vec<TextItem> {
    let mut items = vec![TextItem::new(72.0, y, 10.0, prefix)];
    let mut x = 100.0;
    for group in ["DE89", "3704", "0044", "0532", "0130", "00"] {
        items.push(TextItem::new(x, y, 10.0, group));
        x += 30.0;
    }
    items
}

/// Fehlt der Eintrag in `kept_forms`, ist jeder Fund ein Leck — die sichere
/// Seite. Ein Plan, den ein Test von Hand baut, trägt kein `kept_forms`.
///
/// Die andere Vorgabe daneben (`found.literal` fehlt → „gilt als wörtlich“)
/// ist von außen **nicht** erreichbar: [`redact_pdf::leaks_many_within`]
/// füllt `literal` immer in der Länge von `findings`. Die Mutation
/// `unwrap_or(true)` → `unwrap_or(false)` an dieser Stelle lässt die ganze
/// Prüfung von `redact-gui` grün (337 Tests) — sie ist unerreichbare Vorsicht,
/// kein prüfbares Verhalten.
/// Mutation (`kept_forms` … `unwrap_or(false)` → `unwrap_or(true)`): rot.
#[test]
fn zf_q4_1_ohne_marke_gilt_der_fund_als_woertlich() {
    let out = tmp("ohne-marke").join("out.pdf");
    // Nur die Schreibweise **ohne** Leerraum steht in der Datei: der Begriff
    // mit Leerraum trifft damit nur gequetscht.
    let bytes = build_pdf(&[vec![TextItem::new(72.0, 700.0, 10.0, format!("A {PLAIN}"))]]);
    std::fs::write(&out, &bytes).unwrap();
    let probe = redact_pdf::leaks_many_within(&bytes, &[SPACED], u64::MAX);
    assert!(
        !probe.findings[0].is_empty() && !probe.literal[0],
        "{probe:?}"
    );

    let plan = ExportCheckPlan {
        needles: vec![SPACED.to_string()],
        kept_forms: Vec::new(),
        ..ExportCheckPlan::default()
    };
    let check = plan.run(&out);
    println!("Satz: {}", check.sentence());
    assert_eq!(check.leaking, vec![SPACED.to_string()]);
    assert_eq!(check.kept, 0);
}

/// Die beiden Vorgaben der Entscheidung, unmittelbar geprüft.
///
/// [`crate::state::is_leak`] entscheidet je Fund; `None` heißt „diese Angabe
/// fehlt“. Beide Vorgaben müssen auf der sicheren Seite stehen — im Zweifel
/// ist der Fund ein Leck. Die eine (`kept_forms` fehlt) erreicht ein Test von
/// außen ([`zf_q4_1_ohne_marke_gilt_der_fund_als_woertlich`]); die andere
/// (`literal` fehlt) nicht, weil [`redact_pdf::leaks_many_within`] die Marke
/// immer in der Länge von `findings` füllt. Ungeprüft bleiben muss sie
/// deshalb nicht.
///
/// Mutation (`literal.unwrap_or(true)` → `unwrap_or(false)`): rot.
/// Mutation (`kept_form.unwrap_or(false)` → `unwrap_or(true)`): rot.
#[test]
fn zf_q4_1_die_beiden_vorgaben_stehen_auf_der_sicheren_seite() {
    use crate::state::is_leak;
    // Die beiden Lagen, die im Lauf wirklich vorkommen.
    assert!(
        is_leak(Some(true), Some(true)),
        "wörtlich ist immer ein Leck"
    );
    assert!(!is_leak(Some(false), Some(true)), "nur gequetscht, gedeckt");
    assert!(
        is_leak(Some(false), Some(false)),
        "nur gequetscht, ungedeckt"
    );
    assert!(is_leak(Some(true), Some(false)));
    // Und die Vorgaben.
    assert!(
        is_leak(None, Some(true)),
        "ohne Marke gilt der Fund als wörtlich"
    );
    assert!(is_leak(None, None));
    assert!(is_leak(Some(false), None), "ohne Eintrag deckt keine Zeile");
}

// --- Befund Q4-1: wörtlich gedeckt heißt gar nicht erst gesucht ------------

/// Baut die Lage des Befunds: Seite 1 trägt die IBAN und ist zum Schwärzen
/// angehakt, das Rechteck geht daneben; Seite 2 trägt **wörtlich denselben**
/// Text und ist abgewählt.
fn woertlich_gedeckt() -> (RedactApp, PathBuf) {
    let out = tmp("woertlich").join("out.pdf");
    let bytes = build_pdf(&[
        vec![TextItem::new(72.0, 700.0, 10.0, format!("Seite1 {SPACED}"))],
        vec![TextItem::new(72.0, 700.0, 10.0, format!("Seite2 {SPACED}"))],
    ]);
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&bytes, "zwei-seiten.pdf");
    app.state
        .regions
        .push(text_region(0, empty_corner(1), SPACED));
    app.state.regions.push(text_region(1, over(0), SPACED));
    assert!(app.state.set_enabled(1, false));
    (app, out)
}

/// **Befund Q4-1, behoben — so weit die Oberfläche kommt.** Die Schwärzung
/// auf Seite 1 geht daneben, die IBAN steht dort weiter im Seitentext — und
/// weil eine **abgewählte** Zeile auf Seite 2 denselben Text wörtlich trägt,
/// bekommt kein Urteil (seit Fix-Runde 7 wird er gesucht — und gefunden, denn
/// die abgewählte Zeile steht da). Neu war in Fix-Runde 5, **was die Zeile
/// darüber sagt**:
/// nicht mehr „zählen deshalb nicht als Leck“ (ein Urteil, das niemand
/// gefällt hat), sondern „über sie sagt diese Prüfung nichts“ — und das
/// bleibt als Warnung stehen.
///
/// Warum nicht gesucht wird, steht bei [`AppState::plan_export_check`]:
/// beide Vorkommen sind Zeichen für Zeichen gleich, ein wörtlicher Fund
/// wäre also von der bewusst stehen gelassenen Zeile nicht zu unterscheiden
/// — und blind zu suchen brächte den Fehlalarm zurück, den der Test
/// [`zf_q4_1_woertlich_gedeckt_und_getroffen_ist_kein_alarm`] festhält.
#[test]
fn zf_q4_1_woertlich_gedeckt_sagt_die_zeile_es() {
    let (app, out) = woertlich_gedeckt();
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.outcome(1), HitOutcome::Disabled);
    app.state.export(&out, None).expect("Export");

    // Das Orakel: die IBAN steht im Seitentext **von Seite 1** — dort, wo
    // die Schwärzung angehakt war.
    let bytes = std::fs::read(&out).unwrap();
    let hits = redact_pdf::leaks(&bytes, SPACED);
    let seite1: Vec<&String> = hits.iter().filter(|h| h.starts_with("Seite 1 ")).collect();
    println!("Orakel Seite 1: {seite1:?}");
    assert!(
        !seite1.is_empty(),
        "die Vorlage muss auf Seite 1 lecken: {hits:?}"
    );

    let plan = app.state.plan_export_check(&summary);
    assert_eq!(plan.kept_literal, vec![true], "{plan:?}");
    let check = plan.run(&out);
    println!("Satz: {}", check.sentence());
    println!("Warnung: {:?}", check.warning());
    assert!(!check.found_leak());
    // Seit Fix-Runde 7 wird gesucht — und **gefunden**; nur zuzuordnen ist
    // der Fund nicht (Befund R4-1).
    assert_eq!(check.checked, 1);
    assert_eq!(check.kept, 1);
    assert_eq!(check.unsearched, 1, "gefunden, aber kein Urteil");
    assert_eq!(check.vanished(), 0);
    let sentence = check.sentence();
    assert!(
        sentence.contains(
            "1 Text(e) stehen wörtlich auch in einer abgewählten, gelöschten oder geschützten Zeile: \
             über sie sagt diese Prüfung nichts — ob dort eine Schwärzung danebenging, \
             bleibt offen."
        ),
        "{sentence}"
    );
    assert!(
        !sentence.contains("zählen deshalb nicht als Leck"),
        "das Urteil hat niemand gefällt: {sentence}"
    );
    // Der Vorbehalt steht vorn — vor dem Ergebnis.
    assert!(
        sentence.find("stehen wörtlich auch").unwrap()
            < sentence
                .find("0 gesuchte Text(e) stehen nicht mehr in der Ausgabe.")
                .unwrap(),
        "{sentence}"
    );
    let warning = check.warning().expect("das gehört in die Warnungen");
    assert!(
        warning.contains("über 1 Text(e) sagt sie nichts"),
        "{warning}"
    );
}

/// Die Gegenrichtung — und die Grenze für jede Korrektur von Befund Q4-1:
/// trifft das Rechteck auf Seite 1, dann ist der einzige Rest die **bewusst**
/// stehen gelassene Zeile auf Seite 2. Hier darf kein Alarm stehen (Befund 5
/// aus Fix-Runde 4). Blind wieder zu suchen wäre also die falsche Korrektur;
/// der Unterschied zu Q4-1 liegt nicht in der Datei, sondern darin, ob die
/// Schwärzung auf Seite 1 etwas entfernt hat.
#[test]
fn zf_q4_1_woertlich_gedeckt_und_getroffen_ist_kein_alarm() {
    let out = tmp("woertlich-getroffen").join("out.pdf");
    let bytes = build_pdf(&[
        vec![TextItem::new(72.0, 700.0, 10.0, format!("Seite1 {SPACED}"))],
        vec![TextItem::new(72.0, 700.0, 10.0, format!("Seite2 {SPACED}"))],
    ]);
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&bytes, "zwei-seiten.pdf");
    app.state.regions.push(text_region(0, over(0), SPACED));
    app.state.regions.push(text_region(1, over(0), SPACED));
    assert!(app.state.set_enabled(1, false));
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");
    let written = std::fs::read(&out).unwrap();
    let hits = redact_pdf::leaks(&written, SPACED);
    assert!(
        !hits.iter().any(|h| h.starts_with("Seite 1 ")),
        "Seite 1 ist geschwärzt: {hits:?}"
    );
    assert!(
        hits.iter().any(|h| h.starts_with("Seite 2 ")),
        "Seite 2 steht bewusst da: {hits:?}"
    );
    let check = app.state.plan_export_check(&summary).run(&out);
    println!("Satz: {}", check.sentence());
    assert!(!check.found_leak(), "kein Alarm: {}", check.sentence());
    assert_eq!(check.kept, 1);
}

/// **Befund Q4-1, die richtige Erwartung.** Dieselbe Lage: die Datei zeigt
/// die IBAN auf der Seite, deren Zeile angehakt war. Mindestens eines von
/// beidem muss die Oberfläche tun — suchen (und den Fund melden) oder sagen,
/// dass sie über diese Zeile nichts weiß. Heute tut sie keines von beidem.
#[test]
fn zf_q4_1_woertlich_gedeckt_muesste_gesagt_werden() {
    let (app, out) = woertlich_gedeckt();
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");
    let check = app.state.plan_export_check(&summary).run(&out);
    assert!(
        check.found_leak() || check.warning().is_some(),
        "die Datei leckt auf Seite 1, und die Zeile sagt nichts: {}",
        check.sentence()
    );
}

// ===========================================================================
// 2 — Die Decke zählt Schreibweisen
// ===========================================================================

/// Schreibweisen **einer** Normalform: in `base` an bis zu drei Stellen ein
/// Leerzeichen eingesetzt. Jede ist ein eigener Text, alle haben dieselbe
/// Normalform ([`redact_pdf::squeeze`]).
fn many_spellings(base: &str, count: usize) -> Vec<String> {
    let mut out = Vec::new();
    for i in 1..base.len() {
        for j in i + 1..base.len() {
            for k in j + 1..base.len() {
                let mut s = String::new();
                for (pos, ch) in base.chars().enumerate() {
                    if pos == i || pos == j || pos == k {
                        s.push(' ');
                    }
                    s.push(ch);
                }
                out.push(s);
                if out.len() == count {
                    return out;
                }
            }
        }
    }
    panic!("zu wenige Schreibweisen: {}", out.len());
}

/// **Die Decke füllen und ein Leck dahinter verstecken.** 1 000 präparierte
/// Schreibweisen **eines** Textes (der in der Datei nicht vorkommt) stehen
/// vor dem einen Begriff, der wirklich leckt. Gesucht werden die ersten
/// 1 000; der echte fällt heraus, und gefunden wird nichts.
///
/// Das ist keine Lücke, sondern eine Decke — sie muss aber **gesagt** werden.
/// Sie wird gesagt: Satz und Warnung nennen Zahl und Grenze. Der Satz
/// **beginnt** allerdings mit der Entwarnung; siehe Befund Q4-3.
/// Mutation (`skipped` im Satz weg): rot.
#[test]
fn zf_q4_2_tausend_schreibweisen_fuellen_die_decke_und_der_satz_sagt_es() {
    let out = tmp("decke").join("out.pdf");
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&three_spellings(), "drei.pdf");
    // Tausend Schreibweisen einer Normalform, die in der Datei nicht steht.
    let filler = many_spellings("ZZ99887766554433221100", redact_core::MAX_CHECK_NEEDLES);
    for (i, text) in filler.iter().enumerate() {
        app.state.regions.push(text_region(0, spot(i), text));
    }
    // Und ganz hinten das, was wirklich in der Datei steht und stehen bleibt.
    app.state
        .regions
        .push(text_region(0, empty_corner(7), SPACED));
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");

    let bytes = std::fs::read(&out).unwrap();
    assert!(
        !redact_pdf::leaks(&bytes, SPACED).is_empty(),
        "die Vorlage muss lecken"
    );

    let plan = app.state.plan_export_check(&summary);
    assert_eq!(plan.needles.len(), redact_core::MAX_CHECK_NEEDLES + 1);
    assert_eq!(plan.needles.last().map(String::as_str), Some(SPACED));
    let started = std::time::Instant::now();
    let check = plan.run(&out);
    println!("1 001 Schreibweisen: {:?}", started.elapsed());
    println!("Satz: {}", check.sentence());

    assert_eq!(check.checked, redact_core::MAX_CHECK_NEEDLES);
    assert_eq!(check.skipped, 1, "der echte Begriff fällt heraus");
    assert!(!check.found_leak(), "gesucht wurde er nicht");
    // Gesagt wird es — mit Zahl und Grenze, und auch in den Warnungen (die
    // Statuszeile überschreibt die nächste Aktion).
    assert!(
        check
            .sentence()
            .contains("1 weitere Text(e) wurden nicht gesucht")
            && check.sentence().contains("höchstens 1000 Begriffe"),
        "{}",
        check.sentence()
    );
    let warning = check.warning().expect("die Decke gehört in die Warnungen");
    assert!(warning.contains("Nachprüfung unvollständig"), "{warning}");
}

/// **Befund Q4-3, die richtige Erwartung.** Derselbe Lauf: der Satz beginnt
/// mit „1000 gesuchte Text(e) stehen nicht mehr in der Ausgabe“ und bringt
/// den Vorbehalt erst danach. Genau diese Reihenfolge nennt der
/// Doc-Kommentar von [`ExportCheck::warning`] selbst als untauglich („das
/// läse sich als Warnung wie eine Entwarnung“) — abgestellt hat Fix-Runde 5
/// sie nur in der Warnungsliste, nicht in der Statuszeile, die der Nutzer
/// zuerst liest.
#[test]
fn zf_q4_2_der_vorbehalt_muesste_vor_der_entwarnung_stehen() {
    let mut check = ExportCheck {
        limit: redact_core::MAX_CHECK_NEEDLES,
        checked: 1000,
        ..ExportCheck::default()
    };
    check.skipped = 1;
    let sentence = check.sentence();
    let entwarnung = sentence.find("stehen nicht mehr in der Ausgabe").unwrap();
    let vorbehalt = sentence.find("wurden nicht gesucht").unwrap();
    assert!(vorbehalt < entwarnung, "{sentence}");

    check.skipped = 0;
    check.unchecked = vec!["Objekt 7 0: nicht entpackt".into()];
    let sentence = check.sentence();
    let entwarnung = sentence.find("stehen nicht mehr in der Ausgabe").unwrap();
    let vorbehalt = sentence.find("wurden nicht geprüft").unwrap();
    assert!(vorbehalt < entwarnung, "{sentence}");
}

/// Was 1 000 Schreibweisen **derselben** Normalform kosten und was 1 000
/// verschiedene Texte kosten. Die Decke deckelt die Zahl der Muster im
/// Automaten — und die ist in beiden Fällen dieselbe Größenordnung, also
/// zählt sie die richtige Einheit.
/// `cargo test -p redact-gui zf_q4_2_mess -- --ignored --nocapture`
#[test]
#[ignore]
fn zf_q4_2_mess_schreibweisen_gegen_verschiedene_texte() {
    let out = tmp("mess").join("out.pdf");
    std::fs::write(&out, three_spellings()).unwrap();
    let n = redact_core::MAX_CHECK_NEEDLES;

    // Beide Male ohne Fund, damit nur die Suche gemessen wird.
    let gleiche = many_spellings("ZZ99887766554433221100", n);
    let verschiedene: Vec<String> = (0..n).map(|i| format!("Geheimnis Nummer {i:06}")).collect();
    for (name, needles) in [
        ("Schreibweisen", gleiche),
        ("verschiedene Texte", verschiedene),
    ] {
        let plan = ExportCheckPlan {
            kept_forms: vec![false; needles.len()],
            needles,
            ..ExportCheckPlan::default()
        };
        let started = std::time::Instant::now();
        let check = plan.run(&out);
        println!(
            "{name}: {} Begriffe, {:?}, {} Fund(e)",
            check.checked,
            started.elapsed(),
            check.leaking.len()
        );
    }
}

// ===========================================================================
// 3 — Die Sätze
// ===========================================================================

fn check_with(f: impl FnOnce(&mut ExportCheck)) -> ExportCheck {
    let mut check = ExportCheck {
        limit: redact_core::MAX_CHECK_NEEDLES,
        ..ExportCheck::default()
    };
    f(&mut check);
    check
}

/// Jeder Satz, den [`ExportCheck::sentence`] bauen kann, an seiner Lage
/// geprüft: 0/1/mehrere Funde, mit und ohne `kept`, mit und ohne `skipped`,
/// mit 1/3/4/51 ungeprüften Stellen, Handregionen, unlesbar.
#[test]
fn zf_q4_3_jeder_satz_gegen_seine_lage() {
    let places = |n: usize| -> Vec<String> {
        (0..n)
            .map(|i| format!("Objekt {i} 0: nicht entpackt"))
            .collect()
    };

    // 0 Funde, nichts weiter.
    let c = check_with(|c| c.checked = 3);
    println!("[0 Funde] {}", c.sentence());
    assert_eq!(
        c.sentence(),
        "Nachprüfung: 3 gesuchte Text(e) stehen nicht mehr in der Ausgabe. \
         Geprüft ist genau diese Liste, nicht die Datei."
    );
    assert!(c.warning().is_none());

    // 1 Fund.
    let c = check_with(|c| {
        c.checked = 3;
        c.leaking = vec!["A".into()];
    });
    println!("[1 Fund] {}", c.sentence());
    assert!(c.sentence().starts_with(
        "Nachprüfung: 1 von 3 gesuchten Text(en) steht NOCH in der Ausgabe — diese Datei ist \
         nicht geschwärzt"
    ));
    assert_eq!(c.warning().as_deref(), Some(c.sentence().as_str()));

    // Mehrere Funde — dieselbe Zeile, nur die Zahl wächst.
    let c = check_with(|c| {
        c.checked = 3;
        c.leaking = vec!["A".into(), "B".into()];
    });
    println!("[2 Funde] {}", c.sentence());
    assert!(c
        .sentence()
        .starts_with("Nachprüfung: 2 von 3 gesuchten Text(en) steht NOCH"));

    // Nichts gesucht, nichts da.
    let c = check_with(|_| {});
    println!("[nichts] {}", c.sentence());
    assert!(c.sentence().starts_with(
        "Nachprüfung: keine geschwärzte Zeile mit bekanntem Text — es wurde nichts nachgeprüft."
    ));

    // Nichts gesucht, weil alles gedeckt ist.
    let c = check_with(|c| c.kept = 2);
    println!("[nur kept] {}", c.sentence());
    assert!(c
        .sentence()
        .starts_with("Nachprüfung: es wurde nichts gesucht."));
    assert!(c.sentence().contains("2 Text(e) decken sich"));

    // Handregionen.
    let c = check_with(|c| {
        c.checked = 1;
        c.without_text = 2;
    });
    println!("[Hand] {}", c.sentence());
    assert!(c
        .sentence()
        .ends_with("2 Rechteck(e) ohne bekannten Text — dafür bleibt die Sichtprüfung."));

    // 1, 3, 4 und 51 ungeprüfte Stellen: höchstens drei mit Namen.
    for n in [1usize, 3, 4, 51] {
        let c = check_with(|c| {
            c.checked = 1;
            c.unchecked = places(n);
        });
        let sentence = c.sentence();
        println!("[{n} Stellen] {sentence}");
        assert!(sentence.contains(&format!("{n} Stelle(n) wurden nicht geprüft")));
        let named = sentence.matches("nicht entpackt").count();
        assert_eq!(named, n.min(3), "{sentence}");
        if n > 3 {
            assert!(
                sentence.contains(&format!("… und {} weitere", n - 3)),
                "{sentence}"
            );
        }
        assert_eq!(c.warning().as_deref(), Some(sentence.as_str()));
    }

    // Unlesbar — dann gibt es keine Aussage.
    let c = check_with(|c| {
        c.unreadable = Some("Datei fehlt".into());
        c.without_text = 1;
    });
    println!("[unlesbar] {}", c.sentence());
    assert!(c.sentence().starts_with(
        "Nachprüfung: die geschriebene Datei ließ sich nicht zurücklesen (Datei fehlt) — es \
         wurde nichts nachgeprüft."
    ));
}

// --- Befund Q4-2: die Warnung nennt bei „unlesbar“ die falsche Ursache -----

/// Ein Lauf über eine Datei, die es nicht gibt: `run_within` trägt alle
/// Begriffe als `skipped` ein.
fn unlesbar() -> ExportCheck {
    let plan = ExportCheckPlan {
        needles: vec![SPACED.to_string(), PLAIN.to_string()],
        kept_forms: vec![false, false],
        ..ExportCheckPlan::default()
    };
    plan.run(&tmp("unlesbar").join("gibt-es-nicht.pdf"))
}

/// **Befund Q4-2, die Gegenprobe.** Die Lage, die in die Irre führte, ist
/// unverändert: eine unlesbare Ausgabe trägt **jeden** Begriff als `skipped`
/// ein. Nur die Reihenfolge in [`ExportCheck::warning`] entscheidet jetzt
/// nach der Ursache — die Decke („höchstens 1000 Begriffe“) kommt nicht mehr
/// vor, und die Warnung ist wörtlich der Satz der Statuszeile.
#[test]
fn zf_q4_3_unlesbar_nennt_die_decke_nicht_mehr() {
    let check = unlesbar();
    println!("Satz:    {}", check.sentence());
    println!("Warnung: {:?}", check.warning());
    assert!(check.unreadable.is_some());
    assert_eq!(check.skipped, 2, "alle Begriffe gelten als „nicht gesucht“");
    let warning = check.warning().expect("es gibt eine Warnung");
    assert!(
        !warning.contains("höchstens 1000 Begriffe je Nachprüfung"),
        "die Decke ist nicht der Grund: {warning}"
    );
    assert_eq!(warning, check.sentence(), "derselbe Grund wie in der Zeile");
}

/// **Befund Q4-2, die richtige Erwartung.** Die Warnung, die stehen bleibt,
/// muss denselben Grund nennen wie der Satz: die Datei ließ sich nicht
/// zurücklesen. Die Decke hat damit nichts zu tun.
#[test]
fn zf_q4_3_unlesbar_muesste_den_wahren_grund_nennen() {
    let check = unlesbar();
    let warning = check.warning().expect("es gibt eine Warnung");
    assert!(
        warning.contains("zurücklesen"),
        "die Warnung nennt einen Grund, der nicht stimmt: {warning}"
    );
}

// --- Die Zusage über die Zahl der ungeprüften Stellen ----------------------

/// Eine Datei mit `pages` gepackten Seitenströmen — jeder für sich schon zu
/// groß für das Budget unten.
fn many_packed_streams(pages: usize) -> Vec<u8> {
    let items: Vec<Vec<TextItem>> = (0..pages)
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
    bytes
}

/// **Befund Q4-4 (Doku), richtiggestellt.** Der Doc-Kommentar von
/// `MAX_NAMED_PLACES` sagte: „[`redact_pdf::LeakCheck::unchecked`] darf bis
/// zu 51 Zeilen tragen (50 einzelne Ströme plus eine Summenzeile).“ Das gilt
/// je **Budget und Grund**, und davon gibt es mehrere.
///
/// Gemessen (60 zu große Ströme, Budget 64 Byte): **52** Zeilen — 50 Ströme
/// der Rohsicht, ihre Summenzeile, und „Objektgraph (Sichten 3–7) nicht
/// durchsucht“. Damit ist die 51 widerlegt. Die Obergrenze folgt aus
/// `redact_pdf`s `MAX_UNCHECKED = 50`: die Decke gilt je **Zähler**, und es
/// sind drei (nicht entpackte Ströme der Rohsicht, dieselben der Objektsicht,
/// Stellen aus anderem Grund — etwa die Verschachtelungstiefe), jeder mit
/// eigener Summenzeile, dazu die Zeile über Sicht 7: **154**.
///
/// Folgenlos für die Anzeige (genannt werden ohnehin drei), aber die Zahl war
/// die einzige Begründung dafür, warum überhaupt gekürzt wird.
///
/// Und die zweite Hälfte von Befund Q4-3: mit drei genannten Stellen war der
/// Satz **804 Zeichen** lang. Gekürzt auf Ort und Grund sind es **411**.
#[test]
fn zf_q4_3_die_zahl_der_ungepruefeten_stellen_sprengt_die_zusage() {
    let out = tmp("stellen").join("viele.pdf");
    std::fs::write(&out, many_packed_streams(60)).unwrap();
    let laufen = |budget: u64| {
        ExportCheckPlan {
            needles: vec![SPACED.to_string()],
            kept_forms: vec![false],
            max_decompressed_bytes: budget,
            ..ExportCheckPlan::default()
        }
        .run(&out)
    };

    // Budget 64: der Lader lehnt ab, nur die Rohsicht meldet.
    let check = laufen(64);
    println!("Budget 64: {} ungeprüfte Stellen", check.unchecked.len());
    for line in check.unchecked.iter().take(3) {
        println!("   {line}");
    }
    let sentence = check.sentence();
    println!("Satz ({} Zeichen): {sentence}", sentence.chars().count());
    assert!(
        check.unchecked.len() > 51,
        "die Zusage im Doc-Kommentar wäre gehalten: {}",
        check.unchecked.len()
    );
    // Genannt werden trotzdem nur drei, der Rest gezählt.
    assert!(sentence.contains(&format!("… und {} weitere", check.unchecked.len() - 3)));
    // Befund Q4-3: 804 Zeichen waren es mit den vollen Stellen.
    assert!(
        sentence.chars().count() < 450,
        "die Statuszeile ist wieder unlesbar lang ({} Zeichen): {sentence}",
        sentence.chars().count()
    );
    // Gekürzt heißt: Ort und Grund bleiben, die Buchhaltung geht.
    assert!(sentence.contains(": nicht entpackt — "), "{sentence}");
    assert!(!sentence.contains("des Budgets"), "{sentence}");

    // Ein Budget, das die Vorprüfung durchlässt: die Objektsicht meldet ihre
    // eigenen Stellen dazu — deutlich über 51.
    // Die Gegenrichtung: mit einem Budget, das reicht, bleibt nichts übrig —
    // die 52 sind das Budget, nicht die Datei.
    let genug = laufen(120_000);
    println!(
        "Budget 120 000: {} ungeprüfte Stellen",
        genug.unchecked.len()
    );
    assert!(genug.unchecked.is_empty(), "{:?}", genug.unchecked);
    assert!(!genug.incomplete());
    assert!(genug.warning().is_none(), "{:?}", genug.warning());
}
