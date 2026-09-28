//! Ein Rechteck mit unbrauchbaren Koordinaten bezeichnet **keinen** Bereich der
//! Ebene — und darf deshalb auch keinen überdecken.
//!
//! # Der Befund
//!
//! `f64::min` und `f64::max` schlucken einen NaN-Operanden und liefern den
//! anderen zurück. In [`Rect::intersection_area`] wird daraus
//!
//! ```text
//! w = (self.ur.x.min(NaN) - self.ll.x.max(NaN)).max(0.0)   // = self.width()
//! ```
//!
//! also die **volle Fläche von `self`**. [`Rect::covered_fraction`] teilt sie
//! durch dieselbe Fläche und meldet `1.0`: ein Rechteck mit unbrauchbaren
//! Koordinaten „überdeckt“ jedes andere vollständig. Dazu passend überstand ein
//! solches Rechteck den Entartungsfilter — [`Rect::is_empty`] rechnet mit
//! `width() <= 0.0`, und jeder Vergleich mit NaN ist falsch.
//!
//! # Die Richtung
//!
//! Für die Aufrufer ist „überdeckt alles“ nicht einheitlich die gefährliche
//! Antwort: bei der Negativliste (`resolve_conflicts`) blockiert es eine
//! Schwärzung — ein **Leck**; beim Deck-Rechteck (`hidden_flags`) entfernte es
//! den Text der ganzen Seite — **Datenverlust**, und zwar ohne dass ein
//! sichtbares Rechteck darüber läge. Eine Zahl kann diesen Streit nicht
//! schlichten.
//!
//! Deshalb steht hier nicht die Frage „was ist sicherer“, sondern die Frage
//! „was ist wahr“: ein Rechteck ohne brauchbare Koordinaten bezeichnet die
//! leere Menge, und die leere Menge überdeckt 0 % von allem. Die
//! Sicherheitsfrage wird eine Ebene höher entschieden — von [`Rect::is_empty`],
//! die einen solchen Bereich in denselben Topf wirft wie ein entartetes
//! Rechteck. Wer ihn nicht auswerten kann, zeichnet ihn auch nicht und meldet
//! ihn als wirkungslos.
//!
//! Der Nachweis dafür, dass es dabei bleibt, steht in
//! `redact-pdf/tests/unbrauchbares_deckrechteck.rs`.

use redact_core::{resolve_conflicts, Point, Rect, Region, Source};

/// Alle Bauarten unbrauchbarer Koordinaten, mit Namen für die Fehlermeldung.
///
/// `-inf`/`+inf` gehören dazu, weil sie sich in NaN verwandeln, sobald jemand
/// eine Breite ausrechnet: `inf - inf` ist NaN. Ein Rechteck, dessen beide
/// x-Werte `+inf` sind, hat heute die Breite NaN.
fn unbrauchbare() -> Vec<(&'static str, Rect)> {
    let n = f64::NAN;
    let p = f64::INFINITY;
    let m = f64::NEG_INFINITY;
    vec![
        (
            "alles NaN",
            Rect {
                ll: Point::new(n, n),
                ur: Point::new(n, n),
            },
        ),
        (
            "nur ll.x NaN",
            Rect {
                ll: Point::new(n, 0.0),
                ur: Point::new(10.0, 10.0),
            },
        ),
        (
            "nur ur.y NaN",
            Rect {
                ll: Point::new(0.0, 0.0),
                ur: Point::new(10.0, n),
            },
        ),
        (
            "unendlich gross",
            Rect {
                ll: Point::new(m, m),
                ur: Point::new(p, p),
            },
        ),
        (
            "beide Kanten +inf (Breite wird NaN)",
            Rect {
                ll: Point::new(p, p),
                ur: Point::new(p, p),
            },
        ),
    ]
}

/// Ein gewöhnliches Zeichenrechteck: 6 x 10 Punkt, wie ein 10-pt-Glyph.
fn glyph() -> Rect {
    Rect::new(100.0, 700.0, 106.0, 710.0)
}

// ---------------------------------------------------------------------------
// Die Regel selbst
// ---------------------------------------------------------------------------

/// **Der Befund.** Ein unbrauchbares Rechteck überdeckt nichts.
#[test]
fn ein_unbrauchbares_rechteck_ueberdeckt_nichts() {
    let g = glyph();
    for (name, kaputt) in unbrauchbare() {
        assert_eq!(
            g.covered_fraction(&kaputt),
            0.0,
            "{name}: überdeckt angeblich einen Anteil des Zeichens"
        );
        assert_eq!(
            g.intersection_area(&kaputt),
            0.0,
            "{name}: hat angeblich eine Schnittfläche mit dem Zeichen"
        );
        // Und in der Gegenrichtung — dort war die Antwort schon vorher 0,
        // aber über einen anderen Weg (NaN/NaN). Sie darf nicht NaN werden.
        assert_eq!(
            kaputt.covered_fraction(&g),
            0.0,
            "{name}: wird angeblich vom Zeichen überdeckt"
        );
        assert_eq!(kaputt.intersection_area(&g), 0.0, "{name}: Gegenrichtung");
    }
}

/// [`Rect::covered_fraction`] verspricht „0.0 … 1.0“. NaN ist keins von beidem
/// und wäre für jeden Vergleich die falsche Auskunft — `NaN >= schwelle` ist
/// falsch, `NaN < schwelle` aber auch.
///
/// Der zweite Teil ist der Weg, den eine Prüfung der *Eingabe* nicht sieht:
/// **endliche Koordinaten, unendliche Fläche.** Ein Rechteck mit 1e308 Punkt
/// Kantenlänge ist in allen vier Koordinaten endlich, seine Fläche läuft aber
/// über — und `∞ / ∞` ist NaN.
#[test]
fn covered_fraction_liefert_nie_nan() {
    let g = glyph();
    for (name, kaputt) in unbrauchbare() {
        for (a, b) in [(g, kaputt), (kaputt, g), (kaputt, kaputt)] {
            let f = a.covered_fraction(&b);
            assert!(
                (0.0..=1.0).contains(&f),
                "{name}: covered_fraction lieferte {f}"
            );
        }
    }

    // Endliche Koordinaten, überlaufende Fläche.
    let riesig = Rect::new(1e308, 1e308, 1.5e308, 1.5e308);
    assert!(
        riesig.is_usable(),
        "der Fall soll gerade *nicht* unbrauchbar sein"
    );
    assert!(
        riesig.area().is_infinite(),
        "der Fall trifft nicht mehr: die Fläche läuft nicht über"
    );
    for (was, a, b) in [
        ("mit sich selbst", riesig, riesig),
        ("gegen ein Zeichen", riesig, g),
        ("ein Zeichen dagegen", g, riesig),
    ] {
        let f = a.covered_fraction(&b);
        assert!(
            (0.0..=1.0).contains(&f),
            "überlaufende Fläche, {was}: covered_fraction lieferte {f}"
        );
    }
}

/// Der Entartungsfilter muss ein unbrauchbares Rechteck fangen. Das ist die
/// Stelle, an der die *Sicherheits*frage entschieden wird: was hier hängen
/// bleibt, wird weder gezeichnet noch als Erfolg verbucht.
#[test]
fn ein_unbrauchbares_rechteck_ist_leer() {
    for (name, kaputt) in unbrauchbare() {
        assert!(kaputt.is_empty(), "{name}: gilt als brauchbares Rechteck");
        // Und mit `--padding` bleibt es leer — dort kommt es in
        // `redact_pdf` und `redact_pipeline` an.
        assert!(
            kaputt.expanded(1.0).is_empty(),
            "{name}: mit Rand nicht mehr leer"
        );
    }
    // Ein NaN-Rand macht aus einem gültigen Rechteck ein unbrauchbares —
    // `--padding nan` ist von der Kommandozeile aus erreichbar.
    assert!(
        glyph().expanded(f64::NAN).is_empty(),
        "ein Rand von NaN ergibt angeblich ein brauchbares Rechteck"
    );
}

/// `is_empty()` und `area()` müssen dasselbe sagen — sonst rechnet
/// [`Rect::covered_fraction`] mit einer Fläche, die es laut Filter nicht gibt.
#[test]
fn leer_und_flaechenlos_sind_dasselbe() {
    let mut faelle = unbrauchbare();
    faelle.push(("gewöhnlich", glyph()));
    faelle.push(("entartet (Höhe 0)", Rect::new(0.0, 5.0, 10.0, 5.0)));
    faelle.push(("verdreht", Rect::new(10.0, 10.0, 0.0, 0.0).normalized()));
    for (name, r) in faelle {
        assert_eq!(
            r.is_empty(),
            r.area() == 0.0,
            "{name}: is_empty()={} aber area()={}",
            r.is_empty(),
            r.area()
        );
    }
}

/// Berührung und Enthaltensein sagen dasselbe wie die Fläche.
#[test]
fn ein_unbrauchbares_rechteck_beruehrt_nichts() {
    let g = glyph();
    for (name, kaputt) in unbrauchbare() {
        assert!(!g.intersects(&kaputt), "{name}: berührt angeblich");
        assert!(
            !kaputt.intersects(&g),
            "{name}: berührt angeblich (Gegenr.)"
        );
        assert!(
            !kaputt.contains(g.center()),
            "{name}: enthält angeblich den Zeichenmittelpunkt"
        );
    }
}

// ---------------------------------------------------------------------------
// Die Aufrufer — die Richtungsfrage, an den beiden Enden nachgemessen
// ---------------------------------------------------------------------------

fn kandidat(rect: Rect) -> Region {
    Region::new(
        0,
        rect,
        Some("DE89370400440532013000".into()),
        Source::Pattern {
            pattern_id: "iban_de".into(),
            confidence: 0.99,
        },
    )
}

fn negativ(rect: Rect) -> Region {
    Region::new(
        0,
        rect,
        Some("Eigene Buchung".into()),
        Source::Booking {
            booking_id: "B-1".into(),
            match_type: redact_core::MatchType::Negative,
        },
    )
}

/// **Die gefährliche Richtung der Negativliste.** Ein Negativeintrag mit
/// unbrauchbaren Koordinaten blockierte bisher **jeden** Treffer der Seite —
/// die Schwärzung unterbliebe, die IBAN stünde in der Ausgabe.
#[test]
fn ein_unbrauchbarer_negativeintrag_blockiert_nichts() {
    for (name, kaputt) in unbrauchbare() {
        let res = resolve_conflicts(vec![kandidat(glyph()), negativ(kaputt)]);
        assert_eq!(
            res.redact.len(),
            1,
            "{name}: der Treffer wurde von einem unbrauchbaren Negativeintrag blockiert"
        );
        assert!(res.blocked.is_empty(), "{name}: blockiert gemeldet");
    }
}

/// Und die Gegenprobe: ein Negativeintrag mit *brauchbaren* Koordinaten
/// blockiert weiterhin. Ohne diese Zeile wäre der Test oben auch dann grün,
/// wenn die Negativliste gar nicht mehr wirkte.
#[test]
fn ein_gewoehnlicher_negativeintrag_blockiert_weiterhin() {
    let res = resolve_conflicts(vec![kandidat(glyph()), negativ(glyph().expanded(2.0))]);
    assert!(
        res.redact.is_empty(),
        "die Negativliste blockiert nicht mehr"
    );
    assert_eq!(res.blocked.len(), 1);
}

/// **Die andere gefährliche Richtung.** `dedup` wirft eine Region weg, die in
/// einer anderen derselben Herkunft vollständig enthalten ist. Eine Region mit
/// unbrauchbaren Koordinaten enthielt bisher rechnerisch alles — und schluckte
/// damit die echten Treffer, sofern sie zuerst an der Reihe war.
#[test]
fn eine_unbrauchbare_region_schluckt_keine_andere() {
    for (name, kaputt) in unbrauchbare() {
        // Beide aus derselben Quelle, damit `dedup` überhaupt vergleicht.
        let echt = kandidat(glyph());
        let res = resolve_conflicts(vec![kandidat(kaputt), echt.clone()]);
        assert!(
            res.redact.iter().any(|r| r.rect == echt.rect),
            "{name}: der echte Treffer wurde von einer unbrauchbaren Region geschluckt"
        );
    }
}

/// Gegenprobe zu `dedup`: eine wirklich umschließende Region schluckt weiterhin.
#[test]
fn eine_umschliessende_region_schluckt_weiterhin() {
    let res = resolve_conflicts(vec![
        kandidat(Rect::new(90.0, 690.0, 200.0, 720.0)),
        kandidat(glyph()),
    ]);
    assert_eq!(
        res.redact.len(),
        1,
        "die enthaltene Region wurde nicht entfernt"
    );
}
