//! Prüfrunde 7 — die beiden Quadratiken in der Trefferbilanz, und die Zusage,
//! auf der ihre Beseitigung steht.
//!
//! Kindmodul von [`crate::state`], damit auch die nicht-öffentlichen Teile der
//! Bilanz (`conflict_input`, `counts_for_resolution`) erreichbar sind.
//!
//! ## Worum es geht
//!
//! [`AppState::hit_summary`] und [`AppState::enabled_redactions`] suchten die
//! Zeile zu einem Ergebnis über **Gleichheit**: für jede der n Zeilen ein Zug
//! durch bis zu n Ergebnisplätze, also n²/2 Vergleiche ganzer
//! [`Region`]-Werte. Gemessen bei 96 000 Regionen: 1,6 s bzw. 25,0 s — bei
//! einer Auflösung, die selbst 92 ms braucht.
//!
//! Beide Suchen sind überflüssig, weil [`resolve_conflicts`] seine beiden
//! Listen in **Eingabereihenfolge** zurückgibt. Damit genügt ein Zeiger je
//! Liste, und der Vergleich bleibt derselbe.
//!
//! ## Der Preis, und warum er hier bezahlt wird
//!
//! Diese Reihenfolge ist heute eine Eigenschaft der Implementierung von
//! `resolve_conflicts` (`result.redact.push(cand)` in Kandidatenreihenfolge;
//! `dedup` baut über `regions.drain(..).enumerate()` neu auf und erhält sie).
//! Wird sie beim nächsten Umbau von `dedup` aufgegeben, zeigt die Oberfläche
//! stillschweigend falsche Befunde an — „doppelt“ statt „wird geschwärzt“ und
//! umgekehrt.
//!
//! Deshalb steht sie hier als **Zusicherung mit Test**:
//! [`resolve_conflicts_haelt_die_eingabereihenfolge`] prüft sie an einer
//! Anordnung, die jede Umsortierung sichtbar macht, und
//! [`die_reihenfolgepruefung_schlaegt_an_wenn_die_zusage_faellt`] weist nach,
//! dass diese Prüfung wirklich greift. Der Doc-Kommentar in `redact-core`, der
//! die Zusage auch dort festhält, ist ein Nachzug — das Crate gehört in dieser
//! Runde einem anderen Zuständigkeitsbereich.

use super::*;

use std::time::Instant;

use redact_core::{resolve_conflicts, Action, MatchType, Rect, Region, Source};

// --------------------------------------------------------------- Hilfsmittel

fn pattern(page: usize, x: f64, y: f64, text: &str) -> Region {
    Region::new(
        page,
        Rect::new(x, y, x + 40.0, y + 10.0),
        Some(text.to_string()),
        Source::Pattern {
            pattern_id: "iban_de".to_string(),
            confidence: 0.9,
        },
    )
}

fn manual(page: usize, x: f64, y: f64) -> Region {
    Region::new(
        page,
        Rect::new(x, y, x + 40.0, y + 10.0),
        None,
        Source::Manual {
            reason: "von Hand".to_string(),
        },
    )
}

fn negative(page: usize, x: f64, y: f64) -> Region {
    Region::new(
        page,
        Rect::new(x, y, x + 40.0, y + 10.0),
        Some("Schutz".to_string()),
        Source::Booking {
            booking_id: "B-1".to_string(),
            match_type: MatchType::Negative,
        },
    )
}

/// Ein Zustand **ohne** Dokument: dann bleibt jede Region stehen
/// ([`AppState::is_off_page`] antwortet ohne Seiten mit `false`), und die
/// Bilanz misst genau das, was hier gemessen werden soll.
fn state_with(regions: Vec<Region>) -> AppState {
    let mut state = AppState::new();
    state.regions = regions.into_iter().map(AnnotatedRegion::new).collect();
    state
}

// ---------------------------------------------------------------------------
// (1) Die Zusage: `resolve_conflicts` gibt in Eingabereihenfolge zurück
// ---------------------------------------------------------------------------

/// Ist `folge` eine Teilfolge von `ganz` — gleiche Ordnung, keine Umstellung?
///
/// Als `Result` und nicht als `assert!`, damit
/// [`die_reihenfolgepruefung_schlaegt_an_wenn_die_zusage_faellt`] die Prüfung
/// selbst prüfen kann. Ein Test, dessen Wächter nie rot war, ist keiner.
fn ist_teilfolge<T: PartialEq + std::fmt::Debug>(
    folge: &[T],
    ganz: &[T],
) -> std::result::Result<(), String> {
    let mut next = 0usize;
    for element in folge {
        let Some(at) = ganz[next..].iter().position(|k| k == element) else {
            return Err(format!(
                "„{element:?}“ steht im Ergebnis an Platz {}, aber im Rest der Eingabe \
                 ab Platz {next} nicht mehr — die Reihenfolge ist aufgegeben",
                folge.iter().position(|e| e == element).unwrap_or(0)
            ));
        };
        next += at + 1;
    }
    Ok(())
}

/// **Die Zusage, auf der die beiden Zeiger in [`AppState::hit_summary`]
/// stehen.**
///
/// `resolve_conflicts` gibt `redact` und `blocked` als Teilfolgen der
/// Kandidaten zurück — in Eingabereihenfolge, nicht sortiert.
///
/// Die Anordnung ist so gewählt, dass jede Umsortierung auffällt:
///
/// * die linken Kanten laufen **rückwärts** (240, 200, …). `dedup` sortiert
///   intern nach Seite und linker Kante; gäbe es das Ergebnis in dieser
///   inneren Ordnung zurück, stünde es hier genau verkehrt herum;
/// * zwei Seiten sind **verschränkt** (0, 1, 0, 1, …). Eine Rückgabe nach
///   Seiten gruppiert fiele damit ebenfalls auf;
/// * dazwischen liegen ein blockierter Treffer und ein deckungsgleiches
///   Duplikat, damit auch die Lücken in beiden Listen an der richtigen Stelle
///   sitzen.
#[test]
fn resolve_conflicts_haelt_die_eingabereihenfolge() {
    let schutz = negative(0, 100.0, 500.0);
    // Kandidaten, absichtlich gegen jede innere Sortierordnung angeordnet.
    let kandidaten = vec![
        pattern(0, 240.0, 700.0, "a"),
        pattern(1, 200.0, 700.0, "b"),
        // wird von `schutz` blockiert (deckungsgleich)
        pattern(0, 100.0, 500.0, "c"),
        pattern(1, 160.0, 700.0, "d"),
        // deckungsgleich mit „a“ ⇒ fällt als Duplikat weg
        pattern(0, 240.0, 700.0, "a"),
        pattern(0, 60.0, 700.0, "e"),
        // wird ebenfalls blockiert
        pattern(0, 100.0, 500.0, "f"),
        manual(1, 20.0, 700.0),
    ];

    let mut eingabe = vec![schutz];
    eingabe.extend(kandidaten.iter().cloned());
    let ergebnis = resolve_conflicts(eingabe);

    // Gegenprobe zuerst: die Anordnung tut überhaupt etwas.
    assert_eq!(ergebnis.redact.len(), 5, "erwartet: a, b, d, e, manual");
    assert_eq!(ergebnis.blocked.len(), 2, "erwartet: c, f");

    ist_teilfolge(&ergebnis.redact, &kandidaten)
        .expect("`redact` steht nicht mehr in Eingabereihenfolge");

    let geblockt: Vec<(usize, Rect)> = ergebnis.blocked.iter().map(|b| (b.page, b.rect)).collect();
    let alle: Vec<(usize, Rect)> = kandidaten.iter().map(|r| (r.page, r.rect)).collect();
    ist_teilfolge(&geblockt, &alle).expect("`blocked` steht nicht mehr in Eingabereihenfolge");

    // Und ausgeschrieben, damit ein Fehlschlag sagt, *was* zurückkam:
    let texte: Vec<&str> = ergebnis
        .redact
        .iter()
        .map(|r| r.text.as_deref().unwrap_or("manual"))
        .collect();
    assert_eq!(
        texte,
        vec!["a", "b", "d", "e", "manual"],
        "die Kandidatenreihenfolge ist nicht erhalten"
    );
}

/// **Der Nachweis, dass der Wächter greift.**
///
/// Dieselbe Ergebnismenge, nur umsortiert — einmal nach linker Kante (die
/// innere Ordnung von `dedup`, der wahrscheinlichste Rückfall) und einmal nach
/// Seiten gruppiert. Beide Male muss [`ist_teilfolge`] anschlagen. Ohne diesen
/// Test wäre nicht gezeigt, dass der Test darüber überhaupt etwas hält.
#[test]
fn die_reihenfolgepruefung_schlaegt_an_wenn_die_zusage_faellt() {
    let kandidaten = vec![
        pattern(0, 240.0, 700.0, "a"),
        pattern(1, 200.0, 700.0, "b"),
        pattern(1, 160.0, 700.0, "d"),
        pattern(0, 60.0, 700.0, "e"),
    ];

    // Unverändert: die Prüfung ist zufrieden.
    ist_teilfolge(&kandidaten, &kandidaten).expect("die Eingabe ist ihre eigene Teilfolge");

    // (a) nach linker Kante sortiert — genau das, was ein Umbau von `dedup`
    //     zurückgäbe, der die innere Ordnung durchreicht.
    let mut nach_kante = kandidaten.clone();
    nach_kante.sort_by(|l, r| l.rect.ll.x.partial_cmp(&r.rect.ll.x).unwrap());
    assert_ne!(nach_kante, kandidaten, "die Umsortierung tut nichts");
    assert!(
        ist_teilfolge(&nach_kante, &kandidaten).is_err(),
        "die Prüfung nimmt eine nach linker Kante sortierte Rückgabe hin"
    );

    // (b) nach Seiten gruppiert — der zweite naheliegende Umbau.
    let mut nach_seite = kandidaten.clone();
    nach_seite.sort_by_key(|r| r.page);
    assert_ne!(nach_seite, kandidaten, "die Gruppierung tut nichts");
    assert!(
        ist_teilfolge(&nach_seite, &kandidaten).is_err(),
        "die Prüfung nimmt eine nach Seiten gruppierte Rückgabe hin"
    );
}

// ---------------------------------------------------------------------------
// (2) Die Bilanz rechnet mit den Zeigern dasselbe wie mit den Suchen
// ---------------------------------------------------------------------------

/// Die **frühere** Fassung von [`AppState::hit_summary`], Zeile für Zeile:
/// zwei Listen von Plätzen, und für jede Zeile eine Suche darin.
///
/// Sie steht hier als Vergleichsmaßstab. Verglichen wird dasselbe wie in der
/// neuen Fassung (ganze [`Region`]-Gleichheit bzw. Seite und Rechteck), nur
/// eben suchend; wenn beide auf einer Anordnung dasselbe liefern, hat der
/// Umbau nichts an der Bedeutung geändert.
fn referenz_outcomes(state: &AppState) -> Vec<HitOutcome> {
    let resolution = state.resolution();
    let mut redact: Vec<Option<&Region>> = resolution.redact.iter().map(Some).collect();
    let mut blocked: Vec<Option<&redact_core::BlockedRegion>> =
        resolution.blocked.iter().map(Some).collect();

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
            if let Some(slot) = redact
                .iter_mut()
                .find(|slot| slot.is_some_and(|r| *r == entry.region))
            {
                *slot = None;
                return HitOutcome::Redacted;
            }
            if let Some(slot) = blocked.iter_mut().find(|slot| {
                slot.is_some_and(|b| b.page == entry.region.page && b.rect == entry.region.rect)
            }) {
                *slot = None;
                return HitOutcome::Blocked;
            }
            HitOutcome::Duplicate
        })
        .collect()
}

/// Eine Anordnung, die alle sieben Befunde und alle Sonderfälle enthält, die
/// den beiden Zeigern gefährlich werden könnten: Duplikate, Blockaden,
/// abgewählte Zeilen, Schutzeinträge, ein manueller Treffer, der die
/// Negativliste überstimmt, und deckungsgleiche Rechtecke verschiedener
/// Herkunft.
fn gemischte_zeilen() -> Vec<AnnotatedRegion> {
    let mut zeilen: Vec<AnnotatedRegion> = vec![
        AnnotatedRegion::new(negative(0, 100.0, 500.0)),
        AnnotatedRegion::new(pattern(0, 240.0, 700.0, "a")),
        // blockiert
        AnnotatedRegion::new(pattern(0, 100.0, 500.0, "c")),
        // manuell an derselben Stelle wie der blockierte Treffer: überstimmt
        AnnotatedRegion::new(manual(0, 100.0, 500.0)),
        AnnotatedRegion::new(pattern(1, 200.0, 700.0, "b")),
        // Duplikat von „a“
        AnnotatedRegion::new(pattern(0, 240.0, 700.0, "a")),
        AnnotatedRegion::new(pattern(1, 160.0, 700.0, "d")),
        // ein zweiter blockierter Treffer, deckungsgleich mit dem ersten
        AnnotatedRegion::new(pattern(0, 100.0, 500.0, "f")),
        AnnotatedRegion::new(pattern(0, 60.0, 700.0, "e")),
    ];
    // abgewählt: fällt vor der Auflösung heraus
    zeilen[6].enabled = false;
    zeilen
}

/// **Der Umbau ändert nichts an der Bedeutung.** Zeiger und Suchen liefern
/// dieselben Befunde.
///
/// Gegenprobe eingebaut: die Anordnung muss wirklich alle interessanten
/// Befunde enthalten, sonst wäre der Vergleich wertlos.
#[test]
fn zeiger_und_suche_liefern_dieselbe_bilanz() {
    let mut state = AppState::new();
    state.regions = gemischte_zeilen();

    let neu = state.hit_summary().outcomes;
    let alt = referenz_outcomes(&state);
    assert_eq!(neu, alt, "die Zeigerfassung weicht von der Suchfassung ab");

    for erwartet in [
        HitOutcome::Redacted,
        HitOutcome::Protecting,
        HitOutcome::Disabled,
        HitOutcome::Blocked,
        HitOutcome::Duplicate,
    ] {
        assert!(
            neu.contains(&erwartet),
            "die Anordnung enthält gar keinen Fall {erwartet:?} — der Vergleich \
             prüft dann nichts"
        );
    }
}

/// Dasselbe noch einmal mit **Dokument**, damit auch `OffPage` und
/// `MissingPage` im Vergleich stecken: sie greifen vor den beiden Zeigern und
/// dürfen sie nicht verschieben.
#[test]
fn zeiger_und_suche_stimmen_auch_neben_dem_blatt_ueberein() {
    let mut state = AppState::new();
    state
        .load_bytes(&redact_pdf::testing::demo_statement(), None)
        .expect("Demo-PDF ladbar");
    let seiten = state.page_count();
    let blatt = state.page_box(0).expect("Seite 0");

    state.regions = vec![
        AnnotatedRegion::new(pattern(0, 40.0, 700.0, "a")),
        // ganz rechts neben dem Blatt
        AnnotatedRegion::new(pattern(0, blatt.ur.x + 50.0, 700.0, "neben")),
        // eine Seite, die es nicht gibt
        AnnotatedRegion::new(pattern(seiten + 3, 40.0, 700.0, "fehlt")),
        AnnotatedRegion::new(pattern(0, 40.0, 600.0, "b")),
    ];

    let neu = state.hit_summary().outcomes;
    assert_eq!(neu, referenz_outcomes(&state));
    assert_eq!(
        neu,
        vec![
            HitOutcome::Redacted,
            HitOutcome::OffPage,
            HitOutcome::MissingPage,
            HitOutcome::Redacted,
        ]
    );
}

/// **`HitSummary::redacted` hat nur noch eine Quelle.**
///
/// Die Zahl kam aus `resolution.redact.len()`, die Befunde daneben aus
/// `outcomes` — zwei Quellen für dieselbe Aussage, gelesen an verschiedenen
/// Stellen (Kopfzeile hier, Miniaturspalte und Betrachter dort). Genau dieses
/// Paar war in einer früheren Runde schon einmal auseinandergelaufen.
///
/// **Der ehrliche Stand des Mutationsnachweises**: dreht man die Stelle in
/// [`AppState::hit_summary`] auf `resolution.redact.len()` zurück, bleibt
/// dieser Test grün — und jeder andere auch. Solange die Zuordnung stimmt,
/// liefert die zweite Quelle nirgends etwas anderes; **abweichen kann sie
/// nur.** Genau das ist der Grund, sie zu streichen, und deshalb steht hier
/// die Zusicherung und keine Behauptung über einen roten Test.
///
/// Dass die Abweichung nicht bloß theoretisch ist, zeigt der Nachbar
/// [`zwei_quellen_laufen_auseinander_sobald_die_zuordnung_faellt`]: mit den
/// zwei Zeigern **hängt** die Zuordnung an einer Zusage, und fällt sie, sagen
/// Kopfzeile und Seitenspalte Verschiedenes — wenn die Kopfzeile ihre eigene
/// Quelle behält.
#[test]
fn redacted_ist_die_zahl_der_zeilen_mit_befund_geschwaerzt() {
    let mut state = AppState::new();
    state.regions = gemischte_zeilen();

    let summary = state.hit_summary();
    let aus_outcomes = summary.outcomes.iter().filter(|o| o.is_redacted()).count();
    assert_eq!(summary.redacted, aus_outcomes);
    assert_eq!(summary.redacted, state.resolution().redact.len());
}

/// Kopfzeile und Seitenspalte zählen dieselben Zeilen — auch dort, wo eine
/// Zeile gar nicht erst in die Auflösung geht.
///
/// Die Kopfzeile nimmt `HitSummary::redacted`, die Miniaturspalte
/// [`AppState::redactions_per_page`] (also `outcomes`). Wären das zwei
/// Rechnungen, stünde hier die erste Stelle, an der sie auseinanderlaufen
/// könnten.
#[test]
fn die_kopfzeile_zaehlt_dieselben_zeilen_wie_die_seitenspalte() {
    let mut state = AppState::new();
    state
        .load_bytes(&redact_pdf::testing::demo_statement(), None)
        .expect("Demo-PDF ladbar");
    let blatt = state.page_box(0).expect("Seite 0");

    state.regions = vec![
        AnnotatedRegion::new(pattern(0, 40.0, 700.0, "a")),
        // Neben dem Blatt: geht gar nicht erst in die Auflösung, bekommt aber
        // eine Zeile — und die Kopfzeile darf sie nicht mitzählen.
        AnnotatedRegion::new(pattern(0, blatt.ur.x + 50.0, 700.0, "neben")),
    ];

    let summary = state.hit_summary();
    let spalte: usize = state.redactions_per_page(&summary).iter().sum();
    assert_eq!(
        summary.redacted, spalte,
        "Kopfzeile und Seitenspalte zählen verschieden"
    );
    assert_eq!(summary.redacted, 1);
    assert_eq!(summary.off_page, 1);

    // Und der eigentliche Punkt: die Kopfzeile sagt es auch.
    assert!(
        summary.headline().contains("1 werden geschwärzt"),
        "Kopfzeile: {}",
        summary.headline()
    );
}

/// Die Zuordnung aus [`AppState::hit_summary`], nachgebaut — aber mit einer
/// **übergebenen** Auflösung.
///
/// Nur dafür da, die Zusage einmal absichtlich zu brechen: die echte Fassung
/// nimmt ihre Auflösung selbst und lässt sich von außen nicht belügen.
/// Buchstäblich derselbe Ablauf, damit der Nachweis darunter etwas über die
/// echte Fassung sagt — [`die_nachgebaute_zuordnung_ist_die_echte`] hält das
/// fest.
fn zuordnung_mit(state: &AppState, resolution: &redact_core::Resolution) -> Vec<HitOutcome> {
    let mut next_redact = 0usize;
    let mut next_blocked = 0usize;
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
            if resolution
                .redact
                .get(next_redact)
                .is_some_and(|r| *r == entry.region)
            {
                next_redact += 1;
                return HitOutcome::Redacted;
            }
            if resolution
                .blocked
                .get(next_blocked)
                .is_some_and(|b| b.page == entry.region.page && b.rect == entry.region.rect)
            {
                next_blocked += 1;
                return HitOutcome::Blocked;
            }
            HitOutcome::Duplicate
        })
        .collect()
}

/// Der Nachbau oben ist die echte Zuordnung — sonst wiese der Nachweis
/// darunter nichts nach.
#[test]
fn die_nachgebaute_zuordnung_ist_die_echte() {
    let mut state = AppState::new();
    state.regions = gemischte_zeilen();
    assert_eq!(
        zuordnung_mit(&state, &state.resolution()),
        state.hit_summary().outcomes
    );
}

/// **Warum die Reihenfolgezusage nicht bloß eine Feinheit ist.**
///
/// Wird sie aufgegeben — hier nachgestellt, indem `redact` umgedreht wird —,
/// dann findet nicht mehr jede Zeile ihren Platz. Die Folge ist genau das
/// Muster, das die frühere Runde schon einmal hatte:
///
/// * `outcomes` (Miniaturspalte, Betrachter, Seitenleiste) zeigt **weniger**
///   Schwärzungen, und die betroffenen Zeilen heißen fälschlich „doppelt“;
/// * `resolution.redact.len()` — die frühere zweite Quelle der Kopfzeile —
///   zählt unverändert weiter und verspräche mehr, als gezeichnet wird.
///
/// Der Test hält beide Hälften fest: dass die Zusage **trägt** (die Zuordnung
/// mit der echten Auflösung ist vollständig) und dass ihr Wegfall **sichtbar**
/// wäre. Damit ist gezeigt, wozu
/// [`resolve_conflicts_haelt_die_eingabereihenfolge`] da ist — und warum die
/// Kopfzeile ihre Zahl aus `outcomes` nehmen muss und nicht daneben.
#[test]
fn zwei_quellen_laufen_auseinander_sobald_die_zuordnung_faellt() {
    let mut state = AppState::new();
    // Nur Schwärzungen, damit die Umkehrung wirklich nur die Reihenfolge
    // ändert und keine Duplikate ins Spiel bringt.
    state.regions = (0..5)
        .map(|i| AnnotatedRegion::new(pattern(0, 40.0 + f64::from(i) * 60.0, 700.0, "x")))
        .collect();

    let echt = state.resolution();
    assert_eq!(echt.redact.len(), 5);
    let treffer = |o: &[HitOutcome]| o.iter().filter(|befund| befund.is_redacted()).count();

    // (a) Mit der Zusage: jede Zeile findet ihren Platz.
    assert_eq!(treffer(&zuordnung_mit(&state, &echt)), echt.redact.len());

    // (b) Ohne sie: dieselbe Menge, andere Reihenfolge — und die Bilanz
    //     verliert Zeilen an „doppelt“.
    let verdreht = redact_core::Resolution {
        redact: echt.redact.iter().rev().cloned().collect(),
        blocked: echt.blocked.clone(),
    };
    let kaputt = zuordnung_mit(&state, &verdreht);
    assert!(
        treffer(&kaputt) < verdreht.redact.len(),
        "die Umkehrung fällt gar nicht auf: {kaputt:?}"
    );
    assert!(
        kaputt.contains(&HitOutcome::Duplicate),
        "eine Zeile müsste fälschlich „doppelt“ heißen: {kaputt:?}"
    );
}

// ---------------------------------------------------------------------------
// (3) `enabled_redactions` ist die Bilanz, nicht eine zweite Rechnung
// ---------------------------------------------------------------------------

/// Die **frühere** Fassung von [`AppState::enabled_redactions`]: für jede
/// Region der Auflösung eine Suche durch alle Zeilen.
fn referenz_enabled_redactions(state: &AppState) -> Vec<redact_core::Redaction> {
    state
        .resolution()
        .redact
        .into_iter()
        .map(|region| {
            let action = state
                .regions
                .iter()
                .filter(|a| a.is_blocking() || a.enabled)
                .find(|a| a.region == region)
                .map(|a| a.action.clone())
                .unwrap_or_else(|| state.config.action.clone());
            redact_core::Redaction::new(region, action)
        })
        .collect()
}

/// **Die gestrichene Rechnung liefert dasselbe.** Reihenfolge, Regionen und
/// Schwärzungsart stimmen mit der suchenden Fassung überein — auch dort, wo
/// zwei Zeilen dasselbe Rechteck tragen und nur eine davon angehakt ist (das
/// war Befund B5, und er bleibt behoben).
#[test]
fn abgeleitete_und_suchende_schwaerzungsliste_stimmen_ueberein() {
    let mut state = AppState::new();
    state.regions = gemischte_zeilen();
    // Zwei verschiedene Arten, damit ein Vertauschen auffiele.
    state.regions[1].action = Action::Replace("[IBAN]".to_string());
    state.regions[5].action = Action::Whiteout;

    let neu = state.enabled_redactions();
    let alt = referenz_enabled_redactions(&state);
    assert_eq!(
        neu, alt,
        "die abgeleitete Liste weicht von der gesuchten ab"
    );
    assert!(
        !neu.is_empty(),
        "die Anordnung liefert gar keine Schwärzung"
    );

    // Die Art der **angehakten** Zeile gewinnt, nicht die der ersten
    // deckungsgleichen (Befund B5).
    assert_eq!(neu[0].action, Action::Replace("[IBAN]".to_string()));
}

/// Dieselbe Gegenprobe mit Dokument: eine Zeile neben dem Blatt darf in der
/// Schwärzungsliste nicht auftauchen — und ihre Schwärzungsart erst recht
/// nicht an eine andere Zeile geraten.
#[test]
fn eine_zeile_neben_dem_blatt_steht_in_keiner_schwaerzungsliste() {
    let mut state = AppState::new();
    state
        .load_bytes(&redact_pdf::testing::demo_statement(), None)
        .expect("Demo-PDF ladbar");
    let blatt = state.page_box(0).expect("Seite 0");

    state.regions = vec![
        AnnotatedRegion::new(pattern(0, blatt.ur.x + 50.0, 700.0, "neben")),
        AnnotatedRegion::new(pattern(0, 40.0, 700.0, "a")),
    ];
    state.regions[0].action = Action::Whiteout;
    state.regions[1].action = Action::Blackout;

    let liste = state.enabled_redactions();
    assert_eq!(liste.len(), 1);
    assert_eq!(liste[0].action, Action::Blackout);
    assert_eq!(liste, referenz_enabled_redactions(&state));
}

// ---------------------------------------------------------------------------
// (4) Und erst jetzt: die Geschwindigkeit
// ---------------------------------------------------------------------------

/// Wie viele Messläufe je Größe — Bestwert statt Einzelmessung, aus demselben
/// Grund wie in `redact-patterns/tests/backtracking.rs`.
const MESSLAEUFE: usize = 3;

/// n Treffer in **einer Spalte** — gleiche x-Spanne, verschiedene y.
///
/// Die ungünstigste Anordnung für die Auflösung (siehe `redact-core::conflict`)
/// und zugleich der Regelfall auf einem Kontoauszug: eine IBAN je Zeile.
fn spalte(n: usize) -> Vec<Region> {
    (0..n)
        .map(|i| pattern(0, 60.0, 20.0 + (i as f64) * 12.0, "DE00"))
        .collect()
}

/// **Beide Quadratiken sind weg** — gemessen als Verhältnis zur jeweils
/// früheren Fassung, nicht als Verhältnis zweier getrennt gestoppter Zeiten.
///
/// Warum so und nicht als Wanduhr-Schranke: zwei getrennt gestoppte Zeiten
/// driften unter Fremdlast auseinander, und eine Zusicherung, die in einem von
/// fünf Läufen ohne Grund fehlschlägt, ist keine (dieselbe Begründung wie in
/// `redact-patterns/tests/rect_cursor.rs`). Hier laufen alte und neue Fassung
/// **abwechselnd** auf derselben Eingabe; ein Lastberg trifft dann beide.
///
/// Gemessen (24 000 Regionen, `--release`): `hit_summary` 114,7 ms → 5,3 ms,
/// `enabled_redactions` 1,2 s → 9,8 ms. Die Schranke von 4 lässt also reichlich
/// Luft und bleibt auch auf einer langsamen Maschine unter Last stehen.
///
/// Gegenprobe eingebaut: beide Fassungen müssen dasselbe liefern. Sonst wäre
/// der Test auch dann grün, wenn die neue Fassung nur weniger täte.
#[test]
fn die_bilanz_ist_nicht_mehr_quadratisch() {
    // Groß genug, dass n²/2 sich deutlich von n abhebt, klein genug für einen
    // Debug-Lauf in der regulären Suite.
    const N: usize = 16_000;
    let state = state_with(spalte(N));

    let mut t_alt = std::time::Duration::MAX;
    let mut t_neu = std::time::Duration::MAX;
    for _ in 0..MESSLAEUFE {
        let start = Instant::now();
        let alt = referenz_outcomes(&state);
        t_alt = t_alt.min(start.elapsed());

        let start = Instant::now();
        let neu = state.hit_summary().outcomes;
        t_neu = t_neu.min(start.elapsed());

        assert_eq!(alt, neu, "die beiden Fassungen sind sich uneins");
    }

    let faktor = t_alt.as_secs_f64() / t_neu.as_secs_f64().max(1e-9);
    println!("hit_summary bei {N} Regionen: alt {t_alt:?}, neu {t_neu:?} — Faktor {faktor:.1}");
    assert!(
        faktor > 4.0,
        "die Bilanz ist nur {faktor:.1}-fach schneller als die suchende Fassung — \
         das reicht nicht, um quadratisch von linear zu unterscheiden \
         (alt {t_alt:?}, neu {t_neu:?})"
    );
}

/// Dasselbe für die Schwärzungsliste, die teurere der beiden: sie verglich
/// bei jedem Schritt eine ganze [`Region`] **samt** ihrem `Option<String>`.
#[test]
fn die_schwaerzungsliste_ist_nicht_mehr_quadratisch() {
    const N: usize = 16_000;
    let state = state_with(spalte(N));

    let mut t_alt = std::time::Duration::MAX;
    let mut t_neu = std::time::Duration::MAX;
    for _ in 0..MESSLAEUFE {
        let start = Instant::now();
        let alt = referenz_enabled_redactions(&state);
        t_alt = t_alt.min(start.elapsed());

        let start = Instant::now();
        let neu = state.enabled_redactions();
        t_neu = t_neu.min(start.elapsed());

        assert_eq!(alt, neu, "die beiden Fassungen sind sich uneins");
    }

    let faktor = t_alt.as_secs_f64() / t_neu.as_secs_f64().max(1e-9);
    println!(
        "enabled_redactions bei {N} Regionen: alt {t_alt:?}, neu {t_neu:?} — Faktor {faktor:.1}"
    );
    assert!(
        faktor > 4.0,
        "die Schwärzungsliste ist nur {faktor:.1}-fach schneller als die suchende \
         Fassung (alt {t_alt:?}, neu {t_neu:?})"
    );
}

/// Die Messreihe vorher/nachher bis 96 000 Regionen.
///
/// `#[ignore]`, weil die alte Fassung dafür knapp eine halbe Minute braucht
/// und eine Wanduhr keine Zusicherung trägt — die beiden Tests darüber sind
/// die Zusicherung. Aufruf:
///
/// ```text
/// cargo test --release -p redact-gui -- --ignored --nocapture messreihe
/// ```
#[test]
#[ignore = "Messreihe, dauert Minuten — von Hand aufrufen"]
fn messreihe() {
    println!(
        "{:>7} | {:>10} | {:>10} {:>6} | {:>10} {:>6} | {:>10} {:>6} | {:>10} {:>6}",
        "n",
        "resolution",
        "summary alt",
        "x",
        "summary neu",
        "x",
        "redact. alt",
        "x",
        "redact. neu",
        "x"
    );
    let mut vorher: [Option<f64>; 4] = [None; 4];
    for n in [6_000usize, 12_000, 24_000, 48_000, 96_000] {
        let state = state_with(spalte(n));

        let start = Instant::now();
        let resolution = state.resolution();
        let t_res = start.elapsed().as_secs_f64();
        assert_eq!(resolution.redact.len(), n);

        let start = Instant::now();
        let alt_summary = referenz_outcomes(&state);
        let t_sa = start.elapsed().as_secs_f64();

        let start = Instant::now();
        let neu_summary = state.hit_summary().outcomes;
        let t_sn = start.elapsed().as_secs_f64();

        let start = Instant::now();
        let alt_liste = referenz_enabled_redactions(&state);
        let t_ra = start.elapsed().as_secs_f64();

        let start = Instant::now();
        let neu_liste = state.enabled_redactions();
        let t_rn = start.elapsed().as_secs_f64();

        assert_eq!(alt_summary, neu_summary, "n = {n}");
        assert_eq!(alt_liste, neu_liste, "n = {n}");

        let zeiten = [t_sa, t_sn, t_ra, t_rn];
        let faktoren: Vec<String> = zeiten
            .iter()
            .zip(vorher.iter())
            .map(|(t, v)| v.map_or("—".to_string(), |v: f64| format!("{:.2}", t / v)))
            .collect();
        println!(
            "{n:>7} | {t_res:>9.3}s | {t_sa:>9.3}s {:>6} | {t_sn:>9.3}s {:>6} | \
             {t_ra:>9.3}s {:>6} | {t_rn:>9.3}s {:>6}",
            faktoren[0], faktoren[1], faktoren[2], faktoren[3]
        );
        for (slot, t) in vorher.iter_mut().zip(zeiten) {
            *slot = Some(t);
        }
    }
}
