//! Gegenprüfung G5 (Fix-Runde 3): die Nachprüfung nach dem Export —
//! Decke in Begriffen, eine Entscheidung je Text, `saturating_add` an den
//! Seitenanzeigen. Kindmodul der Crate-Wurzel; `RedactApp::export_to` ist
//! privat, deshalb läuft hier der öffentliche Weg
//! (`AppState::export` → `plan_export_check` → `ExportCheckPlan::run`).
//!
//! Tests mit `#[ignore = "Befund …"]` tragen die **richtige** Erwartung und
//! sind bis zur Korrektur rot; `cargo test -p redact-gui g5 -- --ignored`
//! zeigt es.

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Instant;

use lopdf::dictionary;
use redact_core::{Rect, Region, Source, MAX_CHECK_NEEDLES};
use redact_pdf::testing::{build_pdf, TextItem};
use redact_pipeline::Config;

use crate::state::{AnnotatedRegion, ExportCheckPlan, HitOutcome};
use crate::RedactApp;

// --------------------------------------------------------------- Hilfsmittel

fn iban_only() -> Config {
    Config {
        patterns: vec!["iban_de".to_string()],
        ..Config::default()
    }
}

fn no_patterns() -> Config {
    Config {
        patterns: Vec::new(),
        ..Config::default()
    }
}

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zc-g5-{tag}-{}-{:?}",
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
            reason: "G5".into(),
        },
    ))
}

/// Ein Rechteck, unter dem auf keiner der hier gebauten Seiten Text liegt —
/// die Zeile gilt als geschwärzt, entfernt aber nichts: ein echtes Leck.
fn empty_corner() -> Rect {
    Rect::new(400.0, 20.0, 500.0, 40.0)
}

/// Lauter **verschiedene**, nicht überlappende kleine Rechtecke in derselben
/// leeren Ecke — deckungsgleiche Rechtecke gleicher Herkunft gälten sonst als
/// „doppelt“ und nicht als geschwärzt.
fn distinct_corner(i: usize) -> Rect {
    let x = 400.0 + (i % 50) as f64 * 2.0;
    let y = 20.0 + (i / 50) as f64 * 2.0;
    Rect::new(x, y, x + 1.5, y + 1.5)
}

/// Gültige deutsche IBAN Nummer `i` (BLZ 37040044, Konto = i), Prüfziffer
/// nach ISO 7064 — damit der Validator des Musters sie annimmt.
fn valid_iban(i: usize, spaced: bool) -> String {
    let bban = format!("37040044{i:010}");
    // „DE00“ ans Ende: D=13, E=14, dann „00“.
    let digits = format!("{bban}131400");
    let rem = digits
        .bytes()
        .fold(0u32, |acc, b| (acc * 10 + (b - b'0') as u32) % 97);
    let check = 98 - rem;
    let plain = format!("DE{check:02}{bban}");
    if !spaced {
        return plain;
    }
    plain
        .as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join(" ")
}

fn terms(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("Begriff{i:04}Xy")).collect()
}

fn vm_hwm_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1).map(|v| v.parse().unwrap_or(0)))
        })
        .unwrap_or(0)
}

// ===========================================================================
// A — Die Decke schneidet in Listenreihenfolge
// ===========================================================================

/// 1 000 echte Mustertreffer (gültige IBANs, per Analyse) füllen die Decke;
/// ein echtes Leck an Position 1 001 wird **nicht** gesucht. Die Zeile sagt
/// es („1 weitere Text(e) wurden nicht gesucht“), aber der Kopf lautet
/// „1000 gesuchte Text(e) stehen nicht mehr in der Ausgabe“ — kein Alarm,
/// keine Warnung. Steht dasselbe Leck vorn in der Liste, schlägt die
/// Prüfung an. Mutation `.min(limit)` weg: der erste Teil wird rot
/// (`leaking` nicht leer, `skipped` 0).
#[test]
fn g5a_die_decke_schneidet_in_listenreihenfolge_ein_leck_dahinter_bleibt_ungeprueft() {
    let out = tmp("a-decke").join("out.pdf");
    let per_page = 40;
    let pages = MAX_CHECK_NEEDLES / per_page;
    let mut sheets: Vec<Vec<TextItem>> = (0..pages)
        .map(|p| {
            (0..per_page)
                .map(|l| {
                    TextItem::new(
                        72.0,
                        800.0 - l as f64 * 18.0,
                        10.0,
                        format!("IBAN: {}", valid_iban(p * per_page + l, true)),
                    )
                })
                .collect()
        })
        .collect();
    // Das Leck: ein Text, den keine Schwärzung erreicht.
    sheets[0].push(TextItem::new(72.0, 825.0, 10.0, "Musterbank AG"));
    let pdf = build_pdf(&sheets);

    let mut app = RedactApp::new(iban_only());
    app.open_bytes_and_analyze(&pdf, "tausend.pdf");
    assert_eq!(
        app.state.regions.len(),
        MAX_CHECK_NEEDLES,
        "die Analyse muss genau {MAX_CHECK_NEEDLES} IBANs finden: {}",
        app.state.status
    );

    // Hinten: die Zeile, die das Leck trägt.
    app.state
        .regions
        .push(text_region(0, empty_corner(), "Musterbank"));
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(MAX_CHECK_NEEDLES), HitOutcome::Redacted);
    let t = Instant::now();
    app.state.export(&out, None).expect("Export");
    let export_time = t.elapsed();

    let plan = app.state.plan_export_check(&summary);
    assert_eq!(
        plan.needles.len(),
        MAX_CHECK_NEEDLES + 1,
        "{}",
        plan.needles.len()
    );
    assert_eq!(plan.needles.last().map(String::as_str), Some("Musterbank"));
    let t = Instant::now();
    let check = plan.run(&out);
    let check_time = t.elapsed();
    println!("Export {export_time:?}; Nachprüfung {check_time:?}");
    println!("Statuszeile hinten: {}", check.sentence());

    // Das Orakel: das Leck steht in der Datei.
    let bytes = std::fs::read(&out).unwrap();
    assert!(!redact_pdf::leaks(&bytes, "Musterbank").is_empty());

    // Die Nachprüfung sieht es nicht — und sagt das.
    assert_eq!(check.checked, MAX_CHECK_NEEDLES);
    assert_eq!(check.skipped, 1);
    assert!(check.leaking.is_empty(), "{:?}", check.leaking);
    assert!(!check.found_leak());
    let sentence = check.sentence();
    assert!(
        sentence.contains("1 weitere Text(e) wurden nicht gesucht"),
        "{sentence}"
    );
    assert!(
        sentence.contains(&format!(
            "{MAX_CHECK_NEEDLES} gesuchte Text(e) stehen nicht mehr"
        )),
        "{sentence}"
    );

    // Vorn: dieselbe Zeile an Position 0 — jetzt wird sie gefunden.
    let leak = app.state.regions.pop().unwrap();
    app.state.regions.insert(0, leak);
    let summary = app.state.hit_summary();
    let plan = app.state.plan_export_check(&summary);
    assert_eq!(plan.needles.first().map(String::as_str), Some("Musterbank"));
    let check = plan.run(&out);
    println!("Statuszeile vorn:   {}", check.sentence());
    assert_eq!(check.leaking, vec!["Musterbank".to_string()]);
    assert_eq!(check.skipped, 1);
}

// ===========================================================================
// B — Eine Entscheidung je Text: die Entscheidung vergleicht Zeichenketten,
//     die Suche vergleicht ohne Leerraum
// ===========================================================================

/// **Befund G5-B1.** Dieselbe IBAN, einmal mit Leerzeichen („DE89 3704 …“),
/// einmal ohne („DE89370400…“) — zwei Mustertreffer, beide echt. Die ohne
/// Leerraum bleibt bewusst stehen. `plan_export_check` vergleicht Texte
/// wörtlich (`kept_texts.contains`), also gilt die Entscheidung nicht für
/// die andere Schreibweise; `leaks_many` sucht die Schreibweise mit
/// Leerraum aber **auch ohne Leerraum** (`Needle::squeezed`) — und findet
/// die stehen gelassene Zeile: „steht NOCH in der Ausgabe — darf so nicht
/// weitergegeben werden“ über eine Datei, die genau so gewollt war.
/// Genau der Fehlalarm, den Befund 5 abstellen sollte, eine Schreibweise
/// weiter.
#[test]
fn g5b_dieselbe_iban_ohne_leerraum_abgewaehlt_ist_kein_leck() {
    let out = tmp("b-leerraum").join("out.pdf");
    let spaced = valid_iban(532_013_000, true);
    let plain = valid_iban(532_013_000, false);
    assert_eq!(spaced, "DE89 3704 0044 0532 0130 00");
    assert_eq!(plain, "DE89370400440532013000");
    let pdf = build_pdf(&[
        vec![TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {spaced}"))],
        vec![TextItem::new(72.0, 700.0, 10.0, format!("Konto {plain}"))],
    ]);
    let mut app = RedactApp::new(iban_only());
    app.open_bytes_and_analyze(&pdf, "zwei.pdf");
    assert_eq!(app.state.regions.len(), 2, "{:?}", app.state.regions);
    assert_eq!(
        app.state.regions[0].region.text.as_deref(),
        Some(spaced.as_str())
    );
    assert_eq!(
        app.state.regions[1].region.text.as_deref(),
        Some(plain.as_str())
    );

    // Seite 2 bleibt bewusst stehen.
    assert!(app.state.set_enabled(1, false));
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(1), HitOutcome::Disabled);
    app.state.export(&out, None).expect("Export");

    let plan = app.state.plan_export_check(&summary);
    // Seit Fix-Runde 5 fällt die Entscheidung nicht mehr im Plan: gesucht
    // wird die geschwärzte Schreibweise, und der Plan merkt sich nur, dass
    // eine abgewählte Zeile dieselbe Normalform trägt.
    assert_eq!(plan.needles, vec![spaced.clone()], "{plan:?}");
    assert_eq!(plan.kept_forms, vec![true], "{plan:?}");
    assert_eq!(plan.kept, 0, "{plan:?}");
    let check = plan.run(&out);
    println!("Statuszeile: {}", check.sentence());
    // Getroffen hat nur die Fassung ohne Leerraum — das kann die abgewählte
    // Zeile sein, also kein Leck. Mutation (`kept_forms` im Lauf nicht
    // beachtet): der Fehlalarm aus Runde 4 ist zurück, rot.
    assert_eq!(check.kept, 1, "{check:?}");
    assert_eq!(check.checked, 1, "{check:?}");
    // Das Orakel je Schreibweise: die geschwärzte ist weg (byteweise), nur
    // die stehen gelassene ist noch da.
    let bytes = std::fs::read(&out).unwrap();
    assert!(!redact_pdf::leaks(&bytes, &plain).is_empty());

    assert!(
        !check.found_leak(),
        "Fehlalarm — die stehen gelassene Schreibweise gilt als Leck: {}",
        check.sentence()
    );
}

/// Beobachtung (kein Befund, niedrig): ein Text, der Teilstring eines
/// stehen gelassenen ist — „DE89“ geschwärzt, die ganze IBAN abgewählt —
/// gilt als Leck. Die Entscheidung greift nur bei wörtlich gleichem Text.
#[test]
fn g5b_teilstring_eines_abgewaehlten_textes_gilt_als_leck() {
    let out = tmp("b-teil").join("out.pdf");
    let iban = valid_iban(532_013_000, true);
    let pdf = build_pdf(&[vec![TextItem::new(
        72.0,
        700.0,
        10.0,
        format!("IBAN: {iban}"),
    )]]);
    let mut app = RedactApp::new(iban_only());
    app.open_bytes_and_analyze(&pdf, "eins.pdf");
    assert_eq!(app.state.regions.len(), 1);
    assert!(app.state.set_enabled(0, false));
    app.state
        .regions
        .push(text_region(0, empty_corner(), "DE89"));
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");
    let plan = app.state.plan_export_check(&summary);
    assert_eq!(plan.needles, vec!["DE89".to_string()]);
    assert_eq!(plan.kept, 0);
    let check = plan.run(&out);
    println!("Statuszeile: {}", check.sentence());
    assert_eq!(check.leaking, vec!["DE89".to_string()]);
}

/// Derselbe Text auf Seite 1 geschwärzt (Rechteck trifft nichts: echtes
/// Leck) und auf Seite 2 abgewählt: nicht gesucht, kein Alarm — das echte
/// Leck auf Seite 1 ist damit unprüfbar.
///
/// **Seit Befund Q4-1 sagt die Zeile das auch.** Vorher stand hier „1 Text(e)
/// decken sich … und zählen deshalb nicht als Leck“ — ein Urteil, das die
/// Prüfung nicht gefällt hat, und [`ExportCheck::warning`] gab `None`. Jetzt
/// steht da, dass sie über diesen Text **nichts** sagt, und es bleibt als
/// Warnung stehen. Das Orakel wüsste es besser: es nennt die Seite. Die
/// Oberfläche kann daraus nichts machen, solange die Seite nur im Text der
/// Fundmeldung steht (siehe Vertrag im Bericht).
#[test]
fn g5b_ein_abgewaehlter_text_auf_seite_2_macht_das_leck_auf_seite_1_unpruefbar() {
    let out = tmp("b-seiten").join("out.pdf");
    let pdf = build_pdf(&[
        vec![TextItem::new(72.0, 700.0, 10.0, "Kunde: Musterbank")],
        vec![TextItem::new(72.0, 700.0, 10.0, "Kunde: Musterbank")],
    ]);
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&pdf, "zwei.pdf");
    app.state
        .regions
        .push(text_region(0, empty_corner(), "Musterbank"));
    app.state.regions.push(text_region(
        1,
        Rect::new(72.0, 690.0, 300.0, 712.0),
        "Musterbank",
    ));
    assert!(app.state.set_enabled(1, false));
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.outcome(1), HitOutcome::Disabled);
    app.state.export(&out, None).expect("Export");

    let plan = app.state.plan_export_check(&summary);
    assert!(plan.needles.is_empty(), "{plan:?}");
    assert_eq!(plan.kept, 1);
    let check = plan.run(&out);
    println!("Statuszeile: {}", check.sentence());
    assert!(!check.found_leak());
    assert_eq!(check.unsearched, 1, "{check:?}");
    assert!(check.sentence().contains(
        "1 Text(e) stehen wörtlich auch in einer abgewählten oder geschützten Zeile: über sie \
         sagt diese Prüfung nichts — ob dort eine Schwärzung danebenging, bleibt offen."
    ));
    let warning = check
        .warning()
        .expect("„nichts gesagt“ ist keine Entwarnung");
    assert!(
        warning.contains("über 1 Text(e) sagt sie nichts"),
        "{warning}"
    );

    // Das Orakel nennt die Seite des Lecks.
    let hits = redact_pdf::leaks(&std::fs::read(&out).unwrap(), "Musterbank");
    println!("Orakel: {hits:?}");
    assert!(hits.iter().any(|h| h.contains("Seite 1")), "{hits:?}");
}

/// Leerer und nur-Leerraum-Text zählt als „ohne Text“; 1 000 gleiche Texte
/// sind ein Begriff (die Decke greift nicht); `kept` zählt je Text, nicht je
/// Zeile. Mutation `seen` weg: 1 000 Begriffe statt 1, und `kept` 3 statt 1.
#[test]
fn g5b_leerer_text_und_tausend_gleiche() {
    let out = tmp("b-gleich").join("out.pdf");
    let pdf = build_pdf(&[vec![TextItem::new(72.0, 700.0, 10.0, "Kunde: Musterbank")]]);
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&pdf, "eins.pdf");
    let r = &mut app.state.regions;
    r.push(AnnotatedRegion::new(Region::new(
        0,
        distinct_corner(0),
        Some(String::new()),
        Source::Manual {
            reason: "G5".into(),
        },
    )));
    r.push(text_region(0, distinct_corner(1), "   "));
    for i in 0..MAX_CHECK_NEEDLES {
        r.push(text_region(0, distinct_corner(2 + i), "Musterbank"));
    }
    // Drei abgewählte und zwei geschwärzte Zeilen eines zweiten Texts.
    for i in 0..5 {
        r.push(text_region(
            0,
            distinct_corner(2 + MAX_CHECK_NEEDLES + i),
            "Zweiter",
        ));
    }
    let n = r.len();
    for index in n - 5..n - 2 {
        assert!(app.state.set_enabled(index, false));
    }
    let summary = app.state.hit_summary();
    assert_eq!(summary.redacted, n - 3, "{summary:?}");
    app.state.export(&out, None).expect("Export");
    let plan = app.state.plan_export_check(&summary);
    assert_eq!(plan.without_text, 2, "{plan:?}");
    assert_eq!(
        plan.needles,
        vec!["Musterbank".to_string()],
        "{}",
        plan.needles.len()
    );
    assert_eq!(plan.kept, 1, "{plan:?}");
    let check = plan.run(&out);
    assert_eq!(check.checked, 1);
    assert_eq!(check.skipped, 0);
    assert_eq!(check.leaking, vec!["Musterbank".to_string()]);
    let s = check.sentence();
    assert!(s.contains("2 Rechteck(e) ohne bekannten Text"), "{s}");
    assert!(
        s.contains("1 Text(e) stehen wörtlich auch in einer abgewählten"),
        "{s}"
    );
}

// ===========================================================================
// C — `saturating_add` im Detailbereich der Seitenleiste
// ===========================================================================

/// Der vierte `saturating_add` (sidebar.rs, Detailbereich) hat bisher keinen
/// Test: `absurd_page_numbers_do_not_panic_in_labels` erreicht nur die drei
/// in state.rs. Hier wird die Seitenleiste ohne Bildschirm gezeichnet, mit
/// einer ausgewählten Region auf Seite `usize::MAX`. Mutation
/// `page.saturating_add(1)` → `page + 1` in sidebar.rs: Panic im Debug-Build.
#[test]
fn g5c_der_detailbereich_der_seitenleiste_zeichnet_seite_usize_max() {
    let mut app = RedactApp::new(no_patterns());
    app.open_bytes_and_analyze(&redact_pdf::testing::minimal_pdf("x"), "eins.pdf");
    app.state
        .regions
        .push(text_region(usize::MAX, empty_corner(), "Musterbank"));
    app.state.selected_region = Some(0);
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::MissingPage);
    let state = RefCell::new(app.state);
    let ctx = egui::Context::default();
    for _ in 0..2 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::new(1280.0, 700.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::SidePanel::left("sidebar")
                    .default_width(320.0)
                    .show(ctx, |ui| {
                        let _ = crate::sidebar::show(ui, &mut state.borrow_mut(), &summary);
                    });
            },
        );
    }
}

// ===========================================================================
// A2 — Die Nachprüfung läuft mit der Entpackgrenze des Ladens und sagt,
//      was sie nicht geprüft hat
// ===========================================================================

/// **Befund G5-A2 (Aufrufer).** Der Plan trägt das Byte-Budget, mit dem die
/// Oberfläche das Dokument geladen hat (`Config::limits.max_decompressed_bytes`
/// — dieselbe Zahl wie `--max-decompressed-mb`), und reicht es an
/// `leaks_many_within`. Nicht die Vorgabe, sondern die Zahl aus **dieser**
/// `Config`: mit einer engeren Grenze trägt der Plan die engere. Mutation
/// (Vorgabe statt `self.config.limits`): der zweite Teil wird rot.
#[test]
fn g5a2_der_plan_traegt_die_entpackgrenze_des_ladens() {
    let app = RedactApp::new(iban_only());
    let summary = app.state.hit_summary();
    let plan = app.state.plan_export_check(&summary);
    assert_eq!(
        plan.max_decompressed_bytes,
        app.state.config.limits.max_decompressed_bytes
    );
    assert_eq!(
        plan.max_decompressed_bytes,
        redact_pdf::document::Limits::default().max_decompressed_bytes
    );

    let tight = 3 * 1024 * 1024;
    let app = RedactApp::new(Config {
        limits: redact_pdf::document::Limits {
            max_decompressed_bytes: tight,
            ..redact_pdf::document::Limits::default()
        },
        ..iban_only()
    });
    let summary = app.state.hit_summary();
    let plan = app.state.plan_export_check(&summary);
    assert_eq!(plan.max_decompressed_bytes, tight, "{plan:?}");
}

/// **Befund G5-A2 (Satz).** Was `leaks_many_within` nicht durchsucht hat,
/// steht im Satz — „nicht gefunden“ ist dann keine Aussage — und zählt für
/// die Warnungen wie ein Fund, ohne als „steht NOCH“ zu gelten.
///
/// **Fix-Runde 5:** Der Satz nennt die Ursache nicht mehr selbst. Er sagte
/// „nicht geprüft (Entpackgrenze)“, und die Entpackgrenze ist längst nicht
/// die einzige: die Objektsicht meldet auch „Verschachtelungstiefe 32
/// erreicht“, die Vorprüfung des Laders ihre eigene Ablehnung. Jetzt zählt
/// der Satz die Stellen und **nennt sie**, jede mit ihrem eigenen Grund —
/// hier zwei verschiedene, und beide müssen bis in die Statuszeile und in
/// die Warnung kommen. Mutation (`unchecked_places()` weg oder auf eine
/// feste Ursache zurück): rot.
#[test]
fn g5a2_nicht_geprueft_steht_im_satz_und_zaehlt_als_warnung() {
    let tief = "Objekt 5 0 /Kids[0]: nicht durchsucht — Verschachtelungstiefe 32 erreicht; \
                was tiefer liegt, hat keine Sicht gelesen";
    let check = crate::state::ExportCheck {
        checked: 2,
        unchecked: vec![
            "Objekt 7 0 (Bildstrom): über der Entpackgrenze".to_string(),
            tief.to_string(),
        ],
        ..Default::default()
    };
    let sentence = check.sentence();
    println!("{sentence}");
    assert!(
        sentence.contains("2 Stelle(n) wurden nicht geprüft — die Antwort ist unvollständig:"),
        "{sentence}"
    );
    // Der Satz behauptet keine Ursache mehr — die Stellen nennen ihre.
    assert!(
        !sentence.contains("nicht geprüft (Entpackgrenze)"),
        "{sentence}"
    );
    assert!(
        sentence.contains("Objekt 7 0 (Bildstrom): über der Entpackgrenze"),
        "{sentence}"
    );
    // Gekürzt auf Ort und Grund — die Erklärung dahinter bleibt in
    // `unchecked` (Befund Q4-3: der Satz war mit drei Stellen 804 Zeichen).
    assert!(
        sentence.contains(
            "Objekt 5 0 /Kids[0]: nicht durchsucht — Verschachtelungstiefe 32 \
                           erreicht"
        ),
        "{sentence}"
    );
    assert!(
        !sentence.contains("was tiefer liegt"),
        "gekürzt: {sentence}"
    );
    assert!(check.unchecked.contains(&tief.to_string()));
    assert!(!sentence.contains("steht NOCH"), "{sentence}");
    assert!(!check.found_leak());
    assert!(check.incomplete());
    let warning = check.warning().expect("unvollständig ist eine Warnung");
    assert!(warning.contains("nicht geprüft"), "{warning}");
    assert!(warning.contains("über der Entpackgrenze"), "{warning}");
    assert!(
        warning.contains("Verschachtelungstiefe 32 erreicht"),
        "{warning}"
    );

    // Mehr Stellen, als der Satz nennt: drei mit Namen, der Rest gezählt.
    let viele = crate::state::ExportCheck {
        checked: 1,
        unchecked: (0..5).map(|i| format!("Objekt {i} 0: Grund {i}")).collect(),
        ..Default::default()
    };
    let s = viele.sentence();
    println!("{s}");
    assert!(s.contains("5 Stelle(n) wurden nicht geprüft"), "{s}");
    assert!(s.contains("Objekt 2 0: Grund 2"), "{s}");
    assert!(!s.contains("Objekt 3 0: Grund 3"), "{s}");
    assert!(s.contains("… und 2 weitere"), "{s}");

    // Vollständig geprüft, nichts gefunden, nichts übersprungen: kein Satz,
    // keine Warnung.
    let clean = crate::state::ExportCheck {
        checked: 2,
        ..Default::default()
    };
    assert!(
        !clean.sentence().contains("nicht geprüft"),
        "{}",
        clean.sentence()
    );
    assert!(!clean.incomplete());
    assert_eq!(clean.warning(), None);

    // Ein Fund bleibt ein Fund, und der Satz nennt beides.
    let both = crate::state::ExportCheck {
        checked: 2,
        leaking: vec!["Musterbank".to_string()],
        unchecked: vec!["Objekt 7 0 (Bildstrom): über der Entpackgrenze".to_string()],
        ..Default::default()
    };
    let sentence = both.sentence();
    assert!(
        sentence.contains("1 von 2 gesuchten Text(en) steht NOCH"),
        "{sentence}"
    );
    assert!(
        sentence.contains("1 Stelle(n) wurden nicht geprüft"),
        "{sentence}"
    );
    assert_eq!(both.warning().as_deref(), Some(sentence.as_str()));
}

/// Die unlesbare Ausgabe trägt kein `unchecked` — dort gibt es ohnehin
/// keine Aussage, und der Satz dafür steht schon.
#[test]
fn g5a2_der_lauf_reicht_unchecked_durch() {
    let out = tmp("a2-lauf").join("out.pdf");
    let mut app = RedactApp::new(iban_only());
    app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");
    let plan = app.state.plan_export_check(&summary);
    assert!(!plan.needles.is_empty());
    let check = plan.clone().run(&out);
    // Der Platzhalter prüft alles; eine echte Grenze trägt hier Sätze ein.
    assert!(check.unchecked.is_empty(), "{:?}", check.unchecked);
    assert!(!check.incomplete());
    let missing = plan.run(&out.with_extension("fehlt.pdf"));
    assert!(missing.unreadable.is_some());
    assert!(missing.unchecked.is_empty());
}

// ===========================================================================
// Messung — `cargo test --release -p redact-gui g5_mess -- --ignored --nocapture`
// ===========================================================================

/// „Die Bytes deckelt `--max-decompressed-mb`“ — die Vorgabe ist 1 024 MB.
/// Ein Bildstrom (Flate, Grau, Nullen), der die Ladegrenze passiert, geht
/// unverändert in die Ausgabe; `leaks_many` packt ihn dort **ohne** Grenze
/// aus (zweimal: Rohblock und Objektgraph) und läuft mit jedem Begriff in
/// jeder Kodierung darüber. Gemessen an `G5_MB` MiB (Vorgabe 16) mit 1 und
/// mit 1 000 Begriffen; die Spitze des Speichers (VmHWM) dazu.
#[test]
#[ignore]
fn g5_mess_bildstrom_unter_der_entpackdecke() {
    let mb: usize = std::env::var("G5_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);
    let side = 4096usize;
    let rows = mb * 1024 * 1024 / side;
    let pdf = {
        let base = build_pdf(&[vec![
            TextItem::new(72.0, 700.0, 10.0, "IBAN: DE89 3704 0044 0532 0130 00"),
            TextItem::new(72.0, 680.0, 10.0, "IBAN: DE02 1203 0000 0000 2020 51"),
        ]]);
        let mut doc = lopdf::Document::load_mem(&base).unwrap();
        let mut image = lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => side as i64,
                "Height" => rows as i64,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
            },
            vec![0u8; side * rows],
        );
        image.compress().unwrap();
        let image_id = doc.add_object(image);
        let page_id = *doc.get_pages().get(&1).unwrap();
        let resources_id = match doc
            .get_object(page_id)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Resources")
        {
            Ok(lopdf::Object::Reference(id)) => *id,
            other => panic!("Resources: {other:?}"),
        };
        doc.get_object_mut(resources_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("XObject", dictionary! { "Im1" => image_id });
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    };
    println!(
        "Eingabe: {} kB auf der Platte, Bildstrom {mb} MiB entpackt",
        pdf.len() / 1024
    );

    let mut app = RedactApp::new(iban_only());
    app.open_bytes_and_analyze(&pdf, "bild.pdf");
    assert!(app.state.is_loaded(), "{}", app.state.status);
    assert_eq!(app.state.regions.len(), 2);
    let summary = app.state.hit_summary();
    let out = tmp("mess").join("out.pdf");
    let t = Instant::now();
    app.state.export(&out, None).expect("Export");
    println!(
        "Export {:?}; Ausgabe {} kB",
        t.elapsed(),
        std::fs::metadata(&out).unwrap().len() / 1024
    );
    let base = app.state.plan_export_check(&summary);
    assert_eq!(base.needles.len(), 2);

    let before = vm_hwm_kb();
    let t = Instant::now();
    let one = ExportCheckPlan {
        needles: base.needles[..1].to_vec(),
        ..base.clone()
    }
    .run(&out);
    let one_time = t.elapsed();
    assert!(!one.found_leak(), "{}", one.sentence());
    println!(
        "1 Begriff: {one_time:?}; VmHWM {} MB → {} MB",
        before / 1024,
        vm_hwm_kb() / 1024
    );

    let t = Instant::now();
    let many = ExportCheckPlan {
        needles: terms(MAX_CHECK_NEEDLES),
        ..base
    }
    .run(&out);
    let many_time = t.elapsed();
    assert_eq!(many.checked, MAX_CHECK_NEEDLES);
    println!(
        "{MAX_CHECK_NEEDLES} Begriffe: {many_time:?}; VmHWM {} MB; hochgerechnet auf 1 024 MiB: {:.0} s",
        vm_hwm_kb() / 1024,
        many_time.as_secs_f64() * 1024.0 / mb as f64
    );
}
