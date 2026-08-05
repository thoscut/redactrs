//! `dedup` sortierte mit einer Ordnung, die keine ist — und Rust beendet den
//! Prozess dafür.
//!
//! # Der Befund
//!
//! Die Vergleichsfunktion in [`redact_core::resolve_conflicts`] lautete
//!
//! ```text
//! ra.rect.ll.x.partial_cmp(&rb.rect.ll.x).unwrap_or(Ordering::Equal).then(a.cmp(&b))
//! ```
//!
//! Mit einem NaN unter den Kanten ist das **keine totale Ordnung**: NaN gilt
//! gegenüber *jedem* Wert als gleich, entschieden wird dann nach dem Index.
//! Drei Regionen genügen als Gegenbeispiel — x=5,0 an Index 0, NaN an Index 1,
//! x=1,0 an Index 2:
//!
//! * 0 vs. 1: `partial_cmp` liefert `None`, also `Equal`, dann entscheidet
//!   `0 < 1` ⇒ **a < NaN**
//! * 1 vs. 2: ebenso `Equal`, dann `1 < 2` ⇒ **NaN < b**
//! * 0 vs. 2: `5,0 > 1,0` ⇒ **a > b**
//!
//! Aus `a < NaN` und `NaN < b` müsste `a < b` folgen. Es folgt das Gegenteil.
//!
//! Seit Rust 1.81 erkennt `slice::sort_by` das und **beendet den Prozess mit
//! einer Panik** („user-provided comparison function does not correctly
//! implement a total order"). Das ist kein Schönheitsfehler: eine präparierte
//! Datei bringt damit das Werkzeug zum Absturz, mitten in der Verarbeitung,
//! ohne Ausgabe und ohne Audit-Log.
//!
//! # Wie NaN dorthin kommt
//!
//! Nicht theoretisch. `redact_pdf::PdfExtractor` rechnet Zeichenrechtecke über
//! die Textmatrix aus; ein `cm`-Wert jenseits von `f32` wird beim
//! Multiplizieren zu ±∞, und ∞ mal 0 ist NaN. Diese Rechtecke werden zu
//! Treffern, Treffer zu [`Region`]en, und alle nicht blockierten Regionen
//! landen in `dedup`. Dieselbe Datei liefert dort mühelos die zwanzig
//! Regionen, ab denen der Sortierer die Verletzung auch bemerkt.
//!
//! # Die Behebung
//!
//! `f64::total_cmp` — die IEEE-754-Totalordnung. Sie ordnet NaN an ein festes
//! Ende und ist damit reflexiv, antisymmetrisch und transitiv. Für alle
//! endlichen Werte, und das sind alle Werte eines gewöhnlichen Dokuments,
//! entscheidet sie genau wie `partial_cmp`; die Reihenfolge des Ergebnisses
//! ändert sich also für kein reales PDF.

use redact_core::{resolve_conflicts, Point, Rect, Region, Source};

fn kandidat(page: usize, rect: Rect) -> Region {
    Region::new(
        page,
        rect,
        Some("DE89370400440532013000".into()),
        Source::Pattern {
            pattern_id: "iban_de".into(),
            confidence: 0.99,
        },
    )
}

/// Ein Rechteck, dessen linke Kante NaN ist — sonst gewöhnlich.
fn nan_rect(y: f64) -> Rect {
    Rect {
        ll: Point::new(f64::NAN, y),
        ur: Point::new(f64::NAN, y + 10.0),
    }
}

/// Wie viele Regionen der Sortierer braucht, bis er die Verletzung bemerkt.
///
/// Gemessen: unter 21 Elementen fällt der Sortierer in eine Einfügesortierung,
/// die die Verletzung nicht prüft. Ab 21 bricht er ab. Die Zahl ist ein Detail
/// der Standardbibliothek und darf sich ändern — deshalb steht hier reichlich
/// Abstand nach oben, nicht die gemessene Untergrenze.
const GENUG: usize = 40;

/// **Der Befund.** Eine Trefferliste mit NaN-Kanten darf den Prozess nicht
/// beenden.
///
/// Der Aufbau ist genau das Gegenbeispiel aus dem Modulkopf, nur oft genug
/// wiederholt: absteigende x-Werte, dazwischen NaN. Absteigend, damit der
/// Sortierer wirklich umordnen muss — eine bereits sortierte Folge fasst er
/// gar nicht erst an.
#[test]
fn eine_trefferliste_mit_nan_kanten_stuerzt_nicht_ab() {
    let mut regions = Vec::with_capacity(GENUG);
    let mut brauchbar = 0usize;
    for i in 0..GENUG {
        let y = 700.0 - i as f64;
        if i % 3 == 1 {
            regions.push(kandidat(0, nan_rect(y)));
        } else {
            regions.push(kandidat(
                0,
                Rect::new((GENUG - i) as f64, y, 100.0, y + 10.0),
            ));
            brauchbar += 1;
        }
    }

    let res = resolve_conflicts(regions);

    // Die brauchbaren Regionen sind je 1 Punkt gegeneinander versetzt und
    // überdecken einander damit zu höchstens 80 % — keine darf wegfallen.
    assert_eq!(
        res.redact.iter().filter(|r| r.rect.is_usable()).count(),
        brauchbar,
        "die brauchbaren Treffer sind nicht mehr vollzählig"
    );
}

/// Dasselbe über mehrere Seiten: der Seitenvergleich steht vor dem
/// Kantenvergleich, darf die Verletzung also nicht verdecken.
#[test]
fn nan_kanten_ueber_mehrere_seiten_stuerzen_nicht_ab() {
    let mut regions = Vec::with_capacity(3 * GENUG);
    let mut brauchbar = 0usize;
    for page in 0..3 {
        for i in 0..GENUG {
            let y = 700.0 - i as f64;
            if i % 3 == 1 {
                regions.push(kandidat(page, nan_rect(y)));
            } else {
                regions.push(kandidat(
                    page,
                    Rect::new((GENUG - i) as f64, y, 100.0, y + 10.0),
                ));
                brauchbar += 1;
            }
        }
    }

    let res = resolve_conflicts(regions);
    assert_eq!(
        res.redact.iter().filter(|r| r.rect.is_usable()).count(),
        brauchbar,
        "die brauchbaren Treffer sind nicht mehr vollzählig"
    );
}

/// **Gegenprobe.** Ein gewöhnliches Dokument ordnet weiterhin genauso.
///
/// `total_cmp` unterscheidet sich von `partial_cmp` bei endlichen Werten an
/// genau einer Stelle: `-0.0 < 0.0`. Auch das ist hier geprüft — es darf die
/// Reihenfolge zweier Treffer an derselben Kante nicht umdrehen, denn bei
/// Gleichstand soll die Eingabereihenfolge entscheiden.
#[test]
fn gewoehnliche_kanten_ordnen_unveraendert() {
    // Eine Spalte wie auf einem Kontoauszug: gleiche x-Spanne, verschiedene y.
    // Der zweite Eintrag ist im ersten enthalten und muss wegfallen.
    let res = resolve_conflicts(vec![
        kandidat(0, Rect::new(90.0, 690.0, 200.0, 720.0)),
        kandidat(0, Rect::new(100.0, 700.0, 106.0, 710.0)),
        kandidat(0, Rect::new(90.0, 600.0, 200.0, 630.0)),
    ]);
    assert_eq!(res.redact.len(), 2, "die Deduplizierung entscheidet anders");
    assert_eq!(res.redact[0].rect, Rect::new(90.0, 690.0, 200.0, 720.0));
    assert_eq!(res.redact[1].rect, Rect::new(90.0, 600.0, 200.0, 630.0));

    // Minus-Null gegen Null: derselbe Punkt, verschiedenes Bitmuster. Bei
    // Gleichstand entscheidet die Eingabereihenfolge — der erste bleibt vorn.
    let res = resolve_conflicts(vec![
        kandidat(0, Rect::new(-0.0, 100.0, 50.0, 110.0)),
        kandidat(0, Rect::new(0.0, 200.0, 50.0, 210.0)),
    ]);
    assert_eq!(res.redact.len(), 2);
    assert_eq!(res.redact[0].rect.ll.y, 100.0, "Reihenfolge gedreht");
}
