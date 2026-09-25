//! Das Gitter muss **fertig werden** — auch mit Koordinaten aus einer
//! präparierten Datei.
//!
//! # Der Befund (Regression aus v0.6.0)
//!
//! [`RectGrid::touching_into`] entschied mit
//!
//! ```text
//! if (x1 - x0 + 1.0) * (y1 - y0 + 1.0) > MAX_CELLS_PER_RECT { … Rückfallebene … }
//! ```
//!
//! ob es die Zellen einzeln absucht oder gleich alles liefert. Fließt `NaN` in
//! das Produkt, ist der Vergleich **falsch** — die billige Rückfallebene wird
//! übersprungen. Und weil `∞ as i64` auf `i64::MAX` sättigt, läuft danach
//! `for x in x0 as i64..=x1 as i64` von einem endlichen Wert bis
//! 9 223 372 036 854 775 807. An der Kommandozeile: eine Datei, die nie fertig
//! wird.
//!
//! # Woher das `NaN` kommt
//!
//! Nicht aus dem Rechteck — das ist in allen vier Koordinaten endlich. Es
//! entsteht **im Gitter selbst**: `floor_cell` rechnet `(coord − origin) /
//! cell`, und liegen Ursprung und Koordinate weit genug auseinander, überläuft
//! schon die Differenz nach ∞. Der Ursprung stammt aus den
//! Schwärzungsbereichen, das abgefragte Rechteck aus dem PDF — beide dürfen
//! also unabhängig voneinander extrem sein. Gemessen: `x0 = 0`, `x1 = ∞`,
//! `y0 = y1 = ∞`, Spanne `∞ − ∞ = NaN`, Wächter `NaN > 64.0 == false`.
//!
//! Der Kommentar an `insert` behauptete ausdrücklich, das Produkt sei „nie
//! NaN". Für `insert` stimmt das (eigenes Rechteck, `ll >= origin`); für
//! `touching_into` mit einem fremden Rechteck nicht. Das ist dieselbe Wurzel
//! wie in `unbrauchbare_koordinaten.rs`: eine Aussage, die an einer Stelle gilt
//! und an der nächsten stillschweigend mitgenommen wird.
//!
//! # Wie hier auf „hängt nicht" geprüft wird
//!
//! Ein hängender Test hängt — er wird nicht rot. Die Abfrage läuft deshalb in
//! einem eigenen Faden, und der Test wartet mit Frist. Zurückgedreht ist die
//! Korrektur damit ein sauberes Rot statt einer Zeitüberschreitung des ganzen
//! Laufs.

use redact_core::conflict::RectGrid;
use redact_core::Rect;
use std::sync::mpsc;
use std::time::Duration;

/// Frist für eine Abfrage, die in Mikrosekunden fertig sein muss.
const FRIST: Duration = Duration::from_secs(10);

/// Führt `f` in einem eigenen Faden aus und gibt auf, wenn es hängt.
fn mit_frist<T: Send + 'static>(was: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(FRIST) {
        Ok(wert) => wert,
        Err(_) => panic!("{was}: nach {FRIST:?} nicht fertig — die Schleife läuft ins Leere"),
    }
}

/// Das Paar aus dem Befund: Ursprung extrem negativ, Abfrage extrem positiv.
///
/// Der Prüfer hat gemessen, dass **beide** extrem sein müssen; extrem in nur
/// einer Größe hängt nicht. Genau deshalb steht hier das Paar und nicht ein
/// einzelnes Rechteck.
fn paar() -> (Rect, Rect) {
    // Der eingetragene Bereich legt den Ursprung fest.
    let bereich = Rect::new(-1e308, -1e308, -1e308 + 1.0, -1e308 + 1.0);
    // Das abgefragte Zeichenrechteck: in x wird nur die obere Kante ∞,
    // in y werden beide Kanten ∞ — daraus wird die Spanne NaN.
    let zeichen = Rect::new(-1e308 + 2e8, 1e308, 1e308, 1.5e308);
    (bereich, zeichen)
}

/// **Der Befund.** Die Abfrage wird fertig.
#[test]
fn eine_abfrage_mit_ueberlaufenden_zellenindizes_wird_fertig() {
    let (bereich, zeichen) = paar();
    let treffer = mit_frist("touching_into", move || {
        let mut grid = RectGrid::new([&bereich].into_iter());
        grid.insert(0, &bereich);
        let mut out = Vec::new();
        grid.touching_into(&zeichen, &mut out);
        out
    });
    // Die Rückfallebene liefert alles Eingetragene — mehr Kandidaten, nie
    // weniger. Der genaue Vergleich entscheidet danach der Aufrufer.
    assert_eq!(
        treffer,
        vec![0],
        "die Rückfallebene hat den eingetragenen Bereich verloren"
    );
}

/// Dieselbe Falle in der Gegenrichtung: das **eingetragene** Rechteck ist das
/// extreme. Hier trug die alte Begründung — aber nur, solange der Ursprung aus
/// denselben Rechtecken kommt. Wer das Gitter mit einer Rechteckmenge baut und
/// eine andere einträgt, hat dieselbe Lage.
#[test]
fn auch_das_eintragen_wird_fertig() {
    let (bereich, zeichen) = paar();
    let cells = mit_frist("insert", move || {
        // Ursprung aus dem einen Rechteck, eingetragen wird das andere.
        let mut grid = RectGrid::new([&bereich].into_iter());
        grid.insert(0, &zeichen);
        let mut out = Vec::new();
        grid.touching_into(&zeichen, &mut out);
        out
    });
    assert_eq!(cells, vec![0], "der Eintrag ist verlorengegangen");
}

/// Und die Gegenprobe: gewöhnliche Rechtecke werden weiterhin über die Zellen
/// getrennt, nicht über die Rückfallebene. Ohne sie wäre alles oben auch dann
/// grün, wenn das Gitter gar keins mehr wäre.
#[test]
fn gewoehnliche_rechtecke_trennt_das_gitter_weiterhin() {
    let bereiche: Vec<Rect> = (0..50)
        .map(|i| Rect::new(0.0, i as f64 * 12.0, 200.0, i as f64 * 12.0 + 10.0))
        .collect();
    let mut grid = RectGrid::new(bereiche.iter());
    for (i, r) in bereiche.iter().enumerate() {
        grid.insert(i, r);
    }
    // Ein Zeichen mitten im Bereich 7 sieht nur dessen Nachbarschaft.
    let mut out = Vec::new();
    grid.touching_into(&Rect::new(100.0, 86.0, 104.0, 94.0), &mut out);
    assert!(out.contains(&7), "der überlappende Bereich fehlt: {out:?}");
    assert!(
        out.len() < bereiche.len(),
        "das Gitter liefert alles — es trennt nicht mehr: {} von {}",
        out.len(),
        bereiche.len()
    );
}

// ---------------------------------------------------------------------------
// MAX_CELLS_PER_RECT — von beiden Seiten festgenagelt
// ---------------------------------------------------------------------------

/// Baut ein Gitter mit Zellenmaß 1 x 1 und Ursprung (0, 0), trägt `rect` ein
/// und sagt, ob es in der Rückfallebene gelandet ist.
///
/// Erkennbar ist das an einer Abfrage **weit weg**: was in einer Zelle steht,
/// wird dort nicht gefunden; was in `everywhere` steht, wird immer geliefert.
fn landet_in_der_rueckfallebene(rect: Rect) -> bool {
    // Einheitsrechtecke legen Zellenmaß 1 x 1 und Ursprung (0, 0) fest.
    let mass = [Rect::new(0.0, 0.0, 1.0, 1.0)];
    let mut grid = RectGrid::new(mass.iter());
    grid.insert(0, &rect);
    let mut out = Vec::new();
    grid.touching_into(&Rect::new(1e6, 1e6, 1e6 + 1.0, 1e6 + 1.0), &mut out);
    out.contains(&0)
}

/// **Die Schwelle selbst.** Bei Zellenmaß 1 belegt ein Rechteck von `0` bis `k`
/// genau `k + 1` Zellen je Achse. `MAX_CELLS_PER_RECT` ist 64:
///
/// * 8 x 8 Zellen (`0..7` je Achse) sind genau 64 — das geht noch ins Gitter;
/// * 9 x 8 Zellen (`0..8` mal `0..7`) sind 72 — das wandert in die
///   Rückfallebene.
///
/// Diese Schwelle hielt bisher **kein** Test: ein Prüfer konnte sie auf
/// 4 000 000 setzen, ohne dass etwas rot wurde. Die beiden Schwellen darüber
/// (`BLOCK_COVERAGE_THRESHOLD`, `CONTAINMENT_THRESHOLD`) sind von beiden Seiten
/// gehalten; diese jetzt auch.
#[test]
fn max_cells_per_rect_liegt_bei_64_zellen() {
    assert!(
        !landet_in_der_rueckfallebene(Rect::new(0.0, 0.0, 7.0, 7.0)),
        "genau 64 Zellen (8 x 8) gehören ins Gitter — die Schwelle liegt zu tief"
    );
    assert!(
        landet_in_der_rueckfallebene(Rect::new(0.0, 0.0, 8.0, 7.0)),
        "72 Zellen (9 x 8) gehören in die Rückfallebene — die Schwelle liegt zu hoch"
    );
    // Und in der anderen Achse dasselbe, damit nicht nur x geprüft ist.
    assert!(
        landet_in_der_rueckfallebene(Rect::new(0.0, 0.0, 7.0, 8.0)),
        "72 Zellen (8 x 9) gehören in die Rückfallebene"
    );
}

/// Ein Rechteck mit unbrauchbaren Koordinaten gehört ebenfalls in die
/// Rückfallebene — dort wird es bei **jeder** Abfrage mitgeprüft, statt
/// unauffindbar in einer Zelle zu fehlen.
#[test]
fn unbrauchbare_koordinaten_landen_in_der_rueckfallebene() {
    for (name, r) in [
        ("NaN", Rect::new(f64::NAN, f64::NAN, f64::NAN, f64::NAN)),
        (
            "unendlich",
            Rect::new(
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::INFINITY,
            ),
        ),
    ] {
        assert!(
            landet_in_der_rueckfallebene(r),
            "{name}: nicht in der Rückfallebene — der Eintrag wäre unauffindbar"
        );
    }
}
