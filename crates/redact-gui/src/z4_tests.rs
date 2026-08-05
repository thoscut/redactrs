//! Prüfrunde „Bedienung“ (Z4) — die frisch gebauten Tastaturwege und ihre
//! Zusammenspiele.
//!
//! Kindmodul von [`crate::app`], damit `apply_key_commands`, `key_commands`,
//! `RedactApp::silent` und `apply_tool_action` erreichbar sind.
//!
//! Belege über die **geschriebene Datei** gehen ausschließlich über
//! [`redact_pdf::leaks`].

use super::*;

use redact_core::{MatchType, Rect, Region, Source};

use crate::state::{AnnotatedRegion, HitOutcome};

// --------------------------------------------------------------- Hilfsmittel

fn iban_only() -> Config {
    Config {
        patterns: vec!["iban_de".to_string()],
        ..Config::default()
    }
}

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "z4-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn keys(f: impl FnOnce(&mut KeyState)) -> KeyState {
    let mut k = KeyState::default();
    f(&mut k);
    k
}

fn press(app: &mut RedactApp, k: KeyState) {
    let commands = key_commands(k, app.state.selected_region.is_some());
    app.apply_key_commands(&commands);
}

fn ctrl_r(app: &mut RedactApp) {
    press(
        app,
        keys(|k| {
            k.ctrl = true;
            k.key_r = true;
        }),
    );
}

fn ctrl_arrow(app: &mut RedactApp, dir: char, shift: bool) {
    press(
        app,
        keys(|k| {
            k.ctrl = true;
            k.shift = shift;
            match dir {
                'l' => k.left = true,
                'r' => k.right = true,
                'u' => k.up = true,
                'd' => k.down = true,
                _ => unreachable!(),
            }
        }),
    );
}

fn ctrl_z(app: &mut RedactApp) {
    press(
        app,
        keys(|k| {
            k.ctrl = true;
            k.key_z = true;
        }),
    );
}

fn assert_gone(path: &std::path::Path, needle: &str) {
    let bytes = std::fs::read(path).expect("Ausgabe lesbar");
    let hits = redact_pdf::leaks(&bytes, needle);
    assert!(
        hits.is_empty(),
        "„{needle}“ steht noch in {}: {hits:#?}",
        path.display()
    );
}

fn assert_present(path: &std::path::Path, needle: &str) {
    let bytes = std::fs::read(path).expect("Ausgabe lesbar");
    assert!(
        !redact_pdf::leaks(&bytes, needle).is_empty(),
        "„{needle}“ fehlt in {} — dann prüft `assert_gone` nichts",
        path.display()
    );
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

// ===========================================================================
// Z1 — Anlegen ⇒ Größe ⇒ Seitenwechsel ⇒ Rückgängig
// ===========================================================================

/// Die volle Kette aus der Aufgabenstellung, mit der Frage: verspricht die
/// Kopfzeile am Ende dasselbe, was der Export tut?
#[test]
fn z1_anlegen_groesse_seitenwechsel_rueckgaengig() {
    let mut app = demo_app();
    app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    assert!(app.state.regions.is_empty());

    // 1. Anlegen.
    ctrl_r(&mut app);
    assert_eq!(app.state.regions.len(), 1);
    assert_eq!(app.state.selected_region, Some(0));
    let angelegt = app.state.regions[0].region.rect;

    // 2. Größe ändern — zwei Anschläge, eine Sitzung.
    ctrl_arrow(&mut app, 'r', true);
    ctrl_arrow(&mut app, 'u', true);
    let gross = app.state.regions[0].region.rect;
    assert!((gross.width() - angelegt.width() - 10.0).abs() < 1e-6);
    assert!((gross.height() - angelegt.height() - 10.0).abs() < 1e-6);
    assert_eq!((gross.ll.x, gross.ll.y), (angelegt.ll.x, angelegt.ll.y));

    // 3. Seitenwechsel — mit Bild ab, denn der Pfeil schiebt ja.
    press(&mut app, keys(|k| k.page_down = true));
    assert_eq!(app.state.current_page, 1);
    assert_eq!(
        app.state.selected_region,
        Some(0),
        "die Auswahl überlebt den Seitenwechsel"
    );

    // 4. Größe ändern auf der falschen Seite ändert nichts …
    ctrl_arrow(&mut app, 'r', true);
    assert_eq!(
        app.state.regions[0].region.rect, gross,
        "auf einer anderen Seite darf Strg+Pfeil nichts ändern"
    );
    let status = app.state.status.clone();
    assert!(status.contains("Seite 1"), "Statuszeile: {status}");

    // 5. Rückgängig: erst die Größe, dann das Rechteck selbst.
    ctrl_z(&mut app);
    assert_eq!(app.state.regions.len(), 1, "Schritt 1 zurück");
    assert_eq!(
        app.state.regions[0].region.rect, angelegt,
        "die Größenänderung ist ein Schritt, nicht zwei"
    );
    ctrl_z(&mut app);
    assert!(
        app.state.regions.is_empty(),
        "Schritt 2 zurück: das Rechteck ist weg"
    );
    assert_eq!(app.state.hit_summary().redacted, 0);
}

/// **Der abgelehnte Anschlag darf keinen Schritt „Rückgängig“ kosten** — und
/// dieselbe Frage für die neue Richtung: wer auf der falschen Seite Strg+Pfeil
/// drückt, hat nichts geändert.
#[test]
fn z1b_ein_abgelehnter_anschlag_kostet_keinen_schritt() {
    let mut app = demo_app();
    app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    ctrl_r(&mut app);
    let angelegt = app.state.regions[0].region.rect;

    // Seitenwechsel, dann zehn abgelehnte Anschläge.
    press(&mut app, keys(|k| k.page_down = true));
    for _ in 0..10 {
        ctrl_arrow(&mut app, 'r', true);
    }
    // Ein einziges Rückgängig muss zurück auf „kein Rechteck“ führen.
    ctrl_z(&mut app);
    assert!(
        app.state.regions.is_empty(),
        "zehn abgelehnte Anschläge haben Verlaufsschritte erzeugt: {:?}",
        app.state.regions
    );
    let _ = angelegt;
}

/// **Die Wortwahl der Absage — behoben.** `resize_selected` benutzte dieselbe
/// Meldung wie `move_selected`: sie begann mit „Nicht verschoben“, obwohl gar
/// nicht verschoben werden sollte. Jetzt nennt sie das Verb, um das es ging
/// (siehe [`crate::state::NudgeKind`], geprüft in
/// `rev8_tests::r5_die_absage_nennt_das_verb_um_das_es_ging`).
#[test]
fn z1c_die_absage_beim_groesse_aendern_spricht_vom_groesse_aendern() {
    let mut app = demo_app();
    app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    ctrl_r(&mut app);
    press(&mut app, keys(|k| k.page_down = true));
    ctrl_arrow(&mut app, 'r', false);
    let status = app.state.status.clone();
    assert!(
        status.starts_with("Größe nicht geändert"),
        "unerwarteter Wortlaut: {status}"
    );
}

// ===========================================================================
// Z2 — Größe ändern an einem geschützten Treffer
// ===========================================================================

/// Ein durch die Negativliste **geschützter** Treffer, dessen Größe mit
/// Strg+Pfeil geändert wird: er wird zur manuellen Region, überstimmt damit den
/// Schutz — und das muss gesagt werden **und** im Export ankommen.
#[test]
fn z2_groesse_aendern_an_einem_geschuetzten_treffer() {
    let mut app = demo_app();
    app.state.analyze().expect("Analyse läuft");
    // Ein Treffer auf Seite 0 — die IBAN.
    let index = app
        .state
        .regions
        .iter()
        .position(|a| a.region.page == 0)
        .expect("ein Treffer auf Seite 0");
    let hit = app.state.regions[index].region.rect;

    // Schutzeintrag darüber.
    app.state.regions.insert(
        0,
        AnnotatedRegion::new(Region::new(
            0,
            Rect::new(
                hit.ll.x - 2.0,
                hit.ll.y - 2.0,
                hit.ur.x + 2.0,
                hit.ur.y + 2.0,
            ),
            Some("Schutz".into()),
            Source::Booking {
                booking_id: "b001".into(),
                match_type: MatchType::Negative,
            },
        )),
    );
    let index = index + 1;
    assert_eq!(app.state.hit_summary().outcome(index), HitOutcome::Blocked);

    // Auswählen und die Größe ändern.
    app.state.selected_region = Some(index);
    app.state.current_page = 0;
    ctrl_arrow(&mut app, 'r', false);

    assert_eq!(
        app.state.status,
        crate::state::PROTECTION_OVERRIDDEN,
        "die Umkehrung der Wirkung muss angesagt werden"
    );
    assert_eq!(
        app.state.hit_summary().outcome(index),
        HitOutcome::Redacted,
        "aus geschützt wird geschwärzt"
    );

    // Und der Export tut es auch.
    let dir = tmp("z2");
    let out = dir.join("out.pdf");
    app.state.export(&out, None).expect("Export läuft");
    assert_gone(&out, "DE89 3704 0044 0532 0130 00");
    assert_present(&out, "Musterbank");

    // Strg+Z stellt Rechteck **und** Schutz wieder her.
    ctrl_z(&mut app);
    let wieder = app
        .state
        .regions
        .iter()
        .position(|a| a.region.rect == hit)
        .expect("das ursprüngliche Rechteck ist zurück");
    assert_eq!(
        app.state.hit_summary().outcome(wieder),
        HitOutcome::Blocked,
        "und der Schutz gilt wieder"
    );
}

// ===========================================================================
// Z3 — Strg+R auf einer geheilten Seite
// ===========================================================================

/// Zweiseitiges PDF, dessen **zweite** Seite eine unbrauchbare MediaBox
/// `[0 0 0 0]` trägt. Der Text liegt dort, wo Strg+R sein Rechteck hinlegt.
fn healed_pdf() -> Vec<u8> {
    use redact_pdf::testing::TextItem;
    let bytes = redact_pdf::testing::build_pdf(&[
        vec![TextItem::new(72.0, 700.0, 10.0, "Seite 1 ohne Geheimnis")],
        vec![TextItem::new(
            220.0,
            410.0,
            10.0,
            "IBAN: DE89 3704 0044 0532 0130 00",
        )],
    ]);
    let mut doc = lopdf::Document::load_mem(&bytes).expect("ladbar");
    let page_ids: Vec<lopdf::ObjectId> = doc.get_pages().values().copied().collect();
    let second = page_ids[1];
    doc.get_object_mut(second)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "MediaBox",
            vec![
                lopdf::Object::Integer(0),
                lopdf::Object::Integer(0),
                lopdf::Object::Integer(0),
                lopdf::Object::Integer(0),
            ],
        );
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    out
}

/// **Die Leitfrage auf der geheilten Seite.** Die Oberfläche rechnet dort mit
/// A4, legt das Rechteck in dessen Mitte und verspricht „1 wird geschwärzt“.
/// Tut der Export dasselbe?
#[test]
fn z3_strg_r_auf_einer_geheilten_seite() {
    let mut app = RedactApp::silent(Config::default());
    app.state
        .load_bytes(&healed_pdf(), Some(PathBuf::from("geheilt.pdf")))
        .expect("ladbar");
    assert_eq!(
        app.state.healed_pages,
        vec![1],
        "Seite 2 muss als geheilt gelten"
    );
    assert!(
        app.state.warnings.iter().any(|w| w.contains("Seite 2")),
        "die Heilung muss angesagt werden: {:?}",
        app.state.warnings
    );
    let sheet = app.state.page_box(1).expect("Seite 2");
    assert!(sheet.width() > 500.0, "geheilt auf A4: {sheet:?}");

    // Auf die geheilte Seite und ein Rechteck anlegen.
    app.state.set_page(1);
    ctrl_r(&mut app);
    assert_eq!(app.state.regions.len(), 1);
    let rect = app.state.regions[0].region.rect;
    assert_eq!(app.state.regions[0].region.page, 1);

    let summary = app.state.hit_summary();
    assert_eq!(
        summary.redacted,
        1,
        "die Kopfzeile verspricht eine Schwärzung: {}",
        summary.headline()
    );
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);

    let dir = tmp("z3");
    let out = dir.join("out.pdf");
    let outcome = app.state.export(&out, None).expect("Export läuft");
    // Der Beleg über die geschriebene Datei — nicht über den eigenen Extraktor.
    assert_gone(&out, "DE89 3704 0044 0532 0130 00");
    assert!(
        outcome.removed_glyphs > 0,
        "kein Zeichen entfernt, Rechteck {rect:?}, Bericht {outcome:?}"
    );
}

/// Gegenprobe: auf derselben Datei ohne Schwärzung steht die IBAN noch drin —
/// sonst prüfte der Test darüber nichts.
#[test]
fn z3b_die_gegenprobe_zur_geheilten_seite() {
    let bytes = healed_pdf();
    assert!(
        !redact_pdf::leaks(&bytes, "DE89 3704 0044 0532 0130 00").is_empty(),
        "die Ausgangsdatei muss die IBAN enthalten"
    );
}

// ===========================================================================
// Z4 — Strg+R bei 96 000 Regionen
// ===========================================================================

fn viele_regionen(n: usize) -> Vec<AnnotatedRegion> {
    (0..n)
        .map(|i| {
            let y = 10.0 + (i % 800) as f64;
            let x = 10.0 + (i / 800) as f64 * 4.0;
            AnnotatedRegion::new(Region::new(
                0,
                Rect::new(x, y, x + 3.0, y + 0.5),
                Some(format!("t{i}")),
                Source::Pattern {
                    pattern_id: "iban_de".into(),
                    confidence: 0.9,
                },
            ))
        })
        .collect()
}

/// Strg+R darf bei voller Trefferliste nicht sekundenlang stehen.
#[test]
fn z4_strg_r_bei_96000_regionen() {
    let mut app = demo_app();
    app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    app.state.regions = viele_regionen(96_000);

    let t0 = std::time::Instant::now();
    ctrl_r(&mut app);
    let anlegen = t0.elapsed();
    assert_eq!(app.state.regions.len(), 96_001);

    let t1 = std::time::Instant::now();
    ctrl_arrow(&mut app, 'r', true);
    let groesse = t1.elapsed();

    let t2 = std::time::Instant::now();
    let summary = app.state.hit_summary();
    let bilanz = t2.elapsed();

    eprintln!(
        "Z4 Messung: Strg+R {anlegen:?}, Strg+Pfeil {groesse:?}, Bilanz {bilanz:?}, \
         redacted {}",
        summary.redacted
    );
    assert!(
        anlegen < std::time::Duration::from_millis(1500),
        "Strg+R dauerte {anlegen:?}"
    );
    assert!(
        groesse < std::time::Duration::from_millis(1500),
        "Strg+Pfeil dauerte {groesse:?}"
    );
}

/// Dasselbe an einem **Muster**treffer: dort ruft `set_region_rect` beim ersten
/// Anschlag zusätzlich `hit_summary`, um `PROTECTION_OVERRIDDEN` entscheiden zu
/// können.
#[test]
fn z4b_strg_pfeil_am_mustertreffer_bei_96000_regionen() {
    let mut app = demo_app();
    app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    app.state.regions = viele_regionen(96_000);
    app.state.selected_region = Some(0);

    let t0 = std::time::Instant::now();
    ctrl_arrow(&mut app, 'r', true);
    let erster = t0.elapsed();
    let t1 = std::time::Instant::now();
    ctrl_arrow(&mut app, 'r', true);
    let zweiter = t1.elapsed();
    eprintln!("Z4b Messung: erster Anschlag {erster:?}, zweiter {zweiter:?}");
    assert!(
        erster < std::time::Duration::from_millis(2000),
        "erster Anschlag dauerte {erster:?}"
    );
}

// ===========================================================================
// Z5 — Die Zeigerlösung bei deckungsgleichen Regionen
// ===========================================================================

/// Die frühere, **suchende** Zuordnung — als Vergleichsmaßstab.
fn suchende_outcomes(state: &AppState) -> Vec<HitOutcome> {
    let resolution = state.resolution();
    state
        .regions
        .iter()
        .map(|entry| {
            if entry.is_blocking() {
                return HitOutcome::Protecting;
            }
            if !entry.enabled {
                return HitOutcome::Disabled;
            }
            if state.is_off_page(&entry.region) {
                return match state.page_box(entry.region.page) {
                    Some(_) => HitOutcome::OffPage,
                    None => HitOutcome::MissingPage,
                };
            }
            if resolution.redact.contains(&entry.region) {
                return HitOutcome::Redacted;
            }
            if resolution
                .blocked
                .iter()
                .any(|b| b.page == entry.region.page && b.rect == entry.region.rect)
            {
                return HitOutcome::Blocked;
            }
            HitOutcome::Duplicate
        })
        .collect()
}

/// **Zwei deckungsgleiche manuelle Rechtecke neben einem geschützten
/// Mustertreffer derselben Fläche.**
///
/// So entsteht die Lage: zweimal Strg+R auf derselben Seite legt zwei
/// buchstäblich gleiche Regionen an (gleiche Seite, gleiches Rechteck, gleiche
/// Herkunft, gleicher Grund). Liegt dort zusätzlich ein Mustertreffer, den die
/// Negativliste deckt, dann verschiebt der Duplikat-Eintrag den
/// `blocked`-Zeiger um eins.
#[test]
fn z5_zwei_gleiche_rechtecke_verschieben_den_blockade_zeiger() {
    let mut state = AppState::new();
    state
        .load_bytes(
            &redact_pdf::testing::minimal_pdf("nichts"),
            Some(PathBuf::from("x.pdf")),
        )
        .expect("ladbar");
    let r = Rect::new(100.0, 100.0, 300.0, 140.0);

    // Der Schutzeintrag.
    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        Rect::new(90.0, 90.0, 310.0, 150.0),
        Some("Schutz".into()),
        Source::Booking {
            booking_id: "b001".into(),
            match_type: MatchType::Negative,
        },
    )));
    // Zweimal Strg+R: identische manuelle Regionen.
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
    // Und ein Mustertreffer derselben Fläche, den der Schutz deckt.
    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        r,
        Some("DE89".into()),
        Source::Pattern {
            pattern_id: "iban_de".into(),
            confidence: 0.9,
        },
    )));

    let zeiger = state.hit_summary().outcomes;
    let suchend = suchende_outcomes(&state);

    eprintln!("Z5 Zeiger:  {zeiger:?}");
    eprintln!("Z5 Suchend: {suchend:?}");

    // Zeile 0 ist der Schutz, Zeile 1 das erste Rechteck — beides richtig.
    assert_eq!(zeiger[0], HitOutcome::Protecting);
    assert_eq!(zeiger[1], HitOutcome::Redacted);

    // **Der Befund, behoben.** Zeile 2 ist die zweite, deckungsgleiche
    // Handregion. Sie ist ein Duplikat: sie ging als Kandidat in die
    // Auflösung, `dedup` warf sie heraus, und blockiert werden kann sie gar
    // nicht (manuelle Regionen überstimmen die Negativliste). Der
    // `blocked`-Zeiger verglich aber nur Seite und Rechteck — und die stimmen
    // mit dem blockierten Mustertreffer überein; also verbrauchte die
    // Duplikatzeile dessen Platz. Seit `AnnotatedRegion::is_blockable` fragt
    // der Zeiger zusätzlich, ob die Zeile überhaupt blockierbar ist.
    assert_eq!(
        zeiger[2],
        HitOutcome::Duplicate,
        "eine Handregion kann nicht blockiert sein: {zeiger:?}"
    );
    // Zeile 3 ist der wirklich blockierte Mustertreffer — und heißt jetzt auch so.
    assert_eq!(zeiger[3], HitOutcome::Blocked, "{zeiger:?}");
    assert_eq!(zeiger[3].note(), "geschützt durch Ihre Liste");

    // **Die Wahrheit** steht in der Auflösung selbst: blockiert ist genau ein
    // Eintrag, und das ist der Mustertreffer mit seinem Text.
    let blocked = state.blocked_regions();
    assert_eq!(blocked.len(), 1);
    assert_eq!(
        blocked[0].blocked_reason.as_deref().unwrap_or(""),
        state.regions[3].region.reason(),
        "Audit-Log und Anzeige nennen jetzt dieselbe Zeile"
    );

    // Die frühere, suchende Fassung lag hier anders daneben: sie zählte die
    // Duplikatzeile als „wird geschwärzt“.
    assert_eq!(suchend[2], HitOutcome::Redacted);
    assert_eq!(suchend[3], HitOutcome::Blocked);
}

/// Der Export bleibt in dieser Lage trotzdem richtig: geschwärzt wird genau
/// **eine** Fläche, und die Zusage der Kopfzeile stimmt.
#[test]
fn z5b_die_bilanz_und_der_export_bleiben_einig() {
    let mut state = AppState::new();
    state
        .load_bytes(
            &redact_pdf::testing::demo_statement(),
            Some(PathBuf::from("demo.pdf")),
        )
        .expect("ladbar");
    let r = Rect::new(70.0, 730.0, 320.0, 748.0);
    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        Rect::new(60.0, 720.0, 330.0, 755.0),
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
        Some("DE89 3704 0044 0532 0130 00".into()),
        Source::Pattern {
            pattern_id: "iban_de".into(),
            confidence: 0.9,
        },
    )));

    let summary = state.hit_summary();
    assert_eq!(
        summary.redacted,
        state.enabled_redactions().len(),
        "Kopfzeile und Exportliste müssen dieselbe Zahl nennen: {}",
        summary.headline()
    );

    let dir = tmp("z5b");
    let out = dir.join("out.pdf");
    state.export(&out, None).expect("Export läuft");
    assert_gone(&out, "DE89 3704 0044 0532 0130 00");
}

// ===========================================================================
// Z6 — MissingPage und die Absage der Pfeiltasten
// ===========================================================================

/// Eine Zeile auf einer Seite, die es nicht gibt, ist mit den Pfeiltasten
/// nicht zu erreichen — die Absage schickt aber trotzdem dorthin.
#[test]
fn z6_die_absage_verweist_auf_eine_seite_die_es_nicht_gibt() {
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
    ctrl_arrow(&mut app, 'r', false);
    let status = app.state.status.clone();
    assert!(
        status.contains("Seite 8"),
        "die Absage nennt Seite 8: {status}"
    );
    assert_eq!(
        app.state.page_count(),
        2,
        "…die es in einem zweiseitigen Dokument nicht gibt"
    );
    // **Behoben**: sie rät nicht mehr, dorthin zu blättern, sondern nennt die
    // Seitenzahl des Dokuments und einen gangbaren Ausweg. Siehe
    // `crate::state::selection_on_missing_page` und
    // `rev8_tests::r6_die_absage_schickt_nicht_zu_einer_seite_die_es_nicht_gibt`.
    assert!(
        !status.contains("Zu ihr blättern"),
        "der falsche Rat steht noch da: {status}"
    );
    assert!(status.contains("es hat 2 Seite(n)"), "{status}");
}

// ===========================================================================
// Z7 — Strg+R ohne Dokument, und die Tabulatorkette rückwärts
// ===========================================================================

/// Ohne Dokument gibt es keine Seite — und das muss gesagt werden.
#[test]
fn z7_strg_r_ohne_dokument_sagt_warum() {
    let mut app = RedactApp::silent(Config::default());
    ctrl_r(&mut app);
    assert!(app.state.regions.is_empty());
    assert!(
        app.state.status.contains("Kein Dokument"),
        "Statuszeile: {}",
        app.state.status
    );
}

/// **Umschalt+Tab** muss dieselbe Kette rückwärts durchlaufen — auch über
/// ausgegraute Knöpfe hinweg.
#[test]
fn z7b_umschalt_tab_geht_rueckwaerts_durch_die_leiste() {
    let context = crate::toolbar::ToolContext {
        loaded: true,
        can_undo: true,
        can_redo: false, // „Wiederholen“ ist grau — der Alltagsfall
        first_page: false,
        last_page: false,
        can_zoom_in: true,
        can_zoom_out: true,
        can_find_anything: true,
    };
    let ctx = egui::Context::default();
    let draw = |events: Vec<egui::Event>| {
        let mut clicked = None;
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                clicked = tool_row(ui, &context);
            });
        });
        clicked
    };
    let _ = draw(Vec::new());
    let mut chain = Vec::new();
    for _ in 0..crate::toolbar::buttons().len() + 2 {
        let _ = draw(vec![tab_event(true)]);
        if let Some(action) = draw(vec![space_event()]) {
            chain.push(
                crate::toolbar::buttons()
                    .iter()
                    .find(|b| b.action == action)
                    .map(|b| b.text.to_string())
                    .unwrap(),
            );
        }
    }
    eprintln!("Z7b Kette rückwärts: {chain:?}");
    assert!(
        !chain.contains(&"Wiederholen".to_string()),
        "der graue Knopf bekommt auch rückwärts keinen Fokus: {chain:?}"
    );
    for name in ["Öffnen", "Exportieren", "Rechteck", "Zurück"] {
        assert!(
            chain.contains(&name.to_string()),
            "„{name}“ muss rückwärts erreichbar sein: {chain:?}"
        );
    }
}

fn tab_event(shift: bool) -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            shift,
            ..Default::default()
        },
    }
}

fn space_event() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Space,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    }
}

// ===========================================================================
// Z8 — Die geheilte Seite: Oberfläche warnt, Kommandozeile schweigt
// ===========================================================================

/// **Beide Programme sollen dasselbe sagen** — `cli_and_gui_agree.rs`
/// vergleicht das Audit-Log Feld für Feld, und `warnings` ist eines davon.
///
/// Auf einer Seite mit unbrauchbarer MediaBox tun sie es nicht: die
/// Oberfläche stellt [`crate::state::healed_page_warning`] an den Anfang
/// ihrer Warnungsliste (und damit in ihr Audit-Log), `redact_pipeline::run`
/// kennt den Satz überhaupt nicht. Gerechnet wird in beiden Fällen mit A4 —
/// nur erfährt es auf der Kommandozeile niemand.
#[test]
fn z8_die_geheilte_seite_warnt_nur_in_der_oberflaeche() {
    let bytes = healed_pdf();
    let dir = tmp("z8");
    let input = dir.join("geheilt.pdf");
    std::fs::write(&input, &bytes).unwrap();

    // --- Oberfläche ---
    let mut app = RedactApp::silent(iban_only());
    app.state
        .load_bytes(&bytes, Some(input.clone()))
        .expect("ladbar");
    app.state.analyze().expect("Analyse");
    let gui_out = dir.join("gui.pdf");
    let gui_outcome = app.state.export(&gui_out, None).expect("Export");

    // --- Kommandozeile, dieselbe Config ---
    let cli_out = dir.join("cli.pdf");
    let config = Config {
        input: input.clone(),
        output: Some(cli_out.clone()),
        force: true,
        ..iban_only()
    };
    let cli_outcome = redact_pipeline::run(&config).expect("Lauf");

    let satz = crate::state::healed_page_warning(1);
    assert!(
        gui_outcome.warnings.contains(&satz),
        "die Oberfläche warnt: {:?}",
        gui_outcome.warnings
    );
    assert!(
        !cli_outcome
            .warnings
            .iter()
            .any(|w| w.contains("MediaBox") || w.contains("unbrauchbare Seitengröße")),
        "**Befund**: die Kommandozeile schweigt dazu — {:?}",
        cli_outcome.warnings
    );
    // Gerechnet wird trotzdem gleich: beide schwärzen dieselbe Stelle.
    assert_eq!(gui_outcome.removed_glyphs, cli_outcome.removed_glyphs);
    assert!(gui_outcome.removed_glyphs > 0);
    assert_gone(&gui_out, "DE89 3704 0044 0532 0130 00");
    assert_gone(&cli_out, "DE89 3704 0044 0532 0130 00");
}
