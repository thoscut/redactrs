//! Ein Vorbehalt, der zwanzigtausend Zeilen lang ist, wird nicht gelesen.
//!
//! # Der Befund (gemessen am gebauten Binary, vor der Änderung)
//!
//! `healed_page_warnings` erzeugte **je Seite** einen vollständigen Satz von
//! rund 410 Zeichen und wiederholte darin 350 Zeichen Folgetext. An einer Datei
//! mit 20 000 entarteten Seiten (5,4 MB, `/MediaBox [0 0 0 0]` auf jeder Seite):
//!
//! ```text
//! $ redact-rs v20000.pdf -o v20000_out.pdf -f --audit-log v20000.log
//! # stderr: 20 000 Zeilen à 418 Byte
//! # Audit-Log: 19 378 982 Byte  (dieselbe Datei mit gesunder MediaBox: 11 130 068)
//! ```
//!
//! Kein Absturz, kein Hänger, RSS 179 MB — kein Sicherheitsfehler. Aber die
//! Nachbarn in `redact_pipeline::audit::Effects::warnings` sagen dasselbe seit
//! jeher in einem Satz mit Seitenliste („{n} von {total} … (Seite {liste})“).
//!
//! # Und die Entdopplung war quadratisch
//!
//! `push_warnings` verglich jede neue Warnung mit **jeder** schon eingetragenen
//! (`target.contains`). Gemessen (`cargo test --release`, Warnungen à 130
//! Zeichen): 10 000 Stück 0,148 s, 20 000 Stück 0,359 s, 40 000 Stück 2,528 s.
//! Dieselbe Bauart, die in dieser Runde an drei anderen Stellen durch ein Set
//! ersetzt wurde.

use std::time::{Duration, Instant};

use redact_core::Rect;
use redact_pdf::document::SaneBox;
use redact_pdf::testing::TextItem;
use redact_pipeline::{healed_page_warnings, push_warnings, Config, Outcome};

/// Die IBAN, an der gemessen wird.
const IBAN: &str = "DE89 3704 0044 0532 0130 00";

/// Ein gesundes Blatt: nichts zu melden.
fn gesund() -> SaneBox {
    SaneBox {
        rect: Rect::new(0.0, 0.0, 595.276, 841.89),
        replaced: None,
    }
}

/// Ein geheiltes Blatt — `roh` ist die Angabe, die in der Datei stand.
fn geheilt(roh: [f64; 4]) -> SaneBox {
    SaneBox {
        rect: Rect::new(0.0, 0.0, 595.276, 841.89),
        replaced: Some(Rect::new(roh[0], roh[1], roh[2], roh[3])),
    }
}

/// **Der Befund.** Zwanzigtausend gleich beanstandete Seiten ergeben *einen*
/// Satz mit Seitenliste statt zwanzigtausend Sätzen.
#[test]
fn viele_gleiche_seiten_ergeben_einen_satz() {
    let boxen: Vec<SaneBox> = (0..20_000).map(|_| geheilt([0.0, 0.0, 0.0, 0.0])).collect();
    let saetze = healed_page_warnings(&boxen);

    assert_eq!(saetze.len(), 1, "ein Satz, nicht zwanzigtausend");
    // Zeichenweise abgeschnitten, nicht byteweise: die Meldung eines
    // fehlgeschlagenen Tests darf nicht selbst mitten in einem Umlaut brechen.
    let anfang: String = saetze[0].chars().take(80).collect();
    assert!(
        saetze[0].starts_with("Seite 1, 2, 3, "),
        "die Seitenliste steht vorn: {anfang}"
    );
    assert!(
        saetze[0].contains(", 20000: Unbrauchbare MediaBox (0 x 0), A4 angenommen."),
        "die letzte Seite und die Rohangabe gehören dazu"
    );
    assert!(
        saetze[0].contains("Betrifft 20000 von 20000 Seite(n)."),
        "und die Zahl im Stil der Nachbarn in audit.rs — Satz beginnt mit: {anfang}"
    );

    // Vorher: 20 000 × rund 410 Zeichen. Die Schranke ist großzügig — sie soll
    // nicht die Zeichenzahl festschreiben, sondern eine Rückkehr zum Satz je
    // Seite auffangen.
    let zeichen: usize = saetze.iter().map(String::len).sum();
    assert!(
        zeichen < 200_000,
        "zusammengefasst sind es {zeichen} Zeichen — je Seite wären es über 8 Millionen"
    );
}

/// **Die Gegenrichtung der Zusammenfassung.** Seiten mit *verschiedenen*
/// Angaben werden nicht in einen Satz gezogen: darin steht die Rohangabe der
/// Datei, und eine davon wäre dann falsch wiedergegeben.
#[test]
fn verschiedene_beanstandungen_bleiben_getrennt() {
    let boxen = vec![
        geheilt([0.0, 0.0, 0.0, 0.0]),
        gesund(),
        geheilt([0.0, 0.0, 300_000.0, 300_000.0]),
        geheilt([0.0, 0.0, 0.0, 0.0]),
    ];
    let saetze = healed_page_warnings(&boxen);

    assert_eq!(saetze.len(), 2, "zwei Beanstandungen: {saetze:?}");
    assert!(
        saetze[0].starts_with("Seite 1, 4: Unbrauchbare MediaBox (0 x 0), A4 angenommen."),
        "erste Nennung zuerst, beide Seiten in einem Satz: {}",
        saetze[0]
    );
    assert!(
        saetze[0].contains("Betrifft 2 von 4 Seite(n)."),
        "gezählt wird gegen alle Seiten des Dokuments: {}",
        saetze[0]
    );
    assert!(
        saetze[1].starts_with("Seite 3: Unbrauchbare MediaBox (300000 x 300000), A4 angenommen."),
        "die andere Angabe steht für sich: {}",
        saetze[1]
    );
}

/// Die gesunde Datei bleibt stumm — die Grenze fasst gewöhnliche Blätter nicht
/// an.
#[test]
fn ohne_beanstandung_kein_satz() {
    assert!(healed_page_warnings(&[gesund(), gesund(), gesund()]).is_empty());
    assert!(healed_page_warnings(&[]).is_empty());
}

/// **Die ganze Kette**, nicht nur die Funktion: eine dreiseitige Datei, auf der
/// zwei Seiten dieselbe kaputte Angabe tragen, ergibt in `Outcome::warnings`
/// einen Satz mit beiden Seitenzahlen.
#[test]
fn die_kette_fasst_ebenfalls_zusammen() {
    let inhalt: Vec<Vec<TextItem>> = (0..3)
        .map(|i| {
            vec![TextItem::new(
                72.0,
                700.0,
                12.0,
                format!("Seite {i}: IBAN {IBAN}"),
            )]
        })
        .collect();
    let bytes = redact_pdf::testing::build_pdf(&inhalt);
    let mut doc = lopdf::Document::load_mem(&bytes).expect("ladbar");
    let ids: Vec<lopdf::ObjectId> = doc.get_pages().values().copied().collect();
    for seite in [0usize, 2] {
        doc.get_object_mut(ids[seite])
            .expect("Seite vorhanden")
            .as_dict_mut()
            .expect("Seite ist ein Dictionary")
            .set(
                "MediaBox",
                vec![
                    lopdf::Object::Real(0.0),
                    lopdf::Object::Real(0.0),
                    lopdf::Object::Real(0.0),
                    lopdf::Object::Real(0.0),
                ],
            );
    }
    let mut kaputt = Vec::new();
    doc.save_to(&mut kaputt).expect("speicherbar");

    let dir = std::env::temp_dir().join(format!("redactrs_zag_kette_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("Arbeitsverzeichnis anlegbar");
    let input = dir.join("ein.pdf");
    std::fs::write(&input, &kaputt).expect("schreibbar");
    let outcome: Outcome = redact_pipeline::run(&Config {
        input,
        output: Some(dir.join("aus.pdf")),
        force: true,
        ..Config::default()
    })
    .expect("Lauf gelingt");

    let saetze: Vec<&String> = outcome
        .warnings
        .iter()
        .filter(|w| w.contains("Unbrauchbare MediaBox"))
        .collect();
    assert_eq!(saetze.len(), 1, "ein Satz für beide Seiten: {saetze:?}");
    assert!(
        saetze[0].starts_with("Seite 1, 3: "),
        "und er nennt beide, 1-basiert: {}",
        saetze[0]
    );
    assert!(saetze[0].contains("Betrifft 2 von 3 Seite(n)."));
}

// ---------------------------------------------------------------------------
// push_warnings
// ---------------------------------------------------------------------------

/// Die alte Fassung, wörtlich — sie ist der Vergleichsmaßstab für
/// [`die_entdopplung_ist_nicht_mehr_quadratisch`].
fn push_warnings_alt(target: &mut Vec<String>, warnings: Vec<String>) {
    for warning in warnings {
        if !target.contains(&warning) {
            target.push(warning);
        }
    }
}

/// Erst die Zusicherung, dann die Zeit: das Set darf sich nicht anders
/// verhalten als die lineare Suche. Erste Nennung gewinnt, spätere Dubletten
/// fallen weg — auch gegen bereits eingetragene.
#[test]
fn die_entdopplung_verhaelt_sich_wie_vorher() {
    let faelle: Vec<(Vec<&str>, Vec<&str>)> = vec![
        (vec![], vec!["a", "b", "a", "c", "b"]),
        (vec!["a", "b"], vec!["b", "c", "a", "d"]),
        (vec!["a"], vec![]),
        (vec![], vec![]),
        (vec!["a"], vec!["a", "a", "a"]),
    ];
    for (vorhanden, neu) in faelle {
        let als_string = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        let mut alt = als_string(&vorhanden);
        let mut neu_ziel = als_string(&vorhanden);
        push_warnings_alt(&mut alt, als_string(&neu));
        push_warnings(&mut neu_ziel, als_string(&neu));
        assert_eq!(
            alt, neu_ziel,
            "vorhanden {vorhanden:?}, neu {neu:?} — die Reihenfolge muss dieselbe bleiben"
        );
    }
}

/// Bestwert aus mehreren Läufen, siehe [`die_entdopplung_ist_nicht_mehr_quadratisch`].
const MESSLAEUFE: usize = 3;

/// **Die Kurve ist gerade geworden** — gemessen als Verhältnis zur alten
/// Fassung, nicht als Verhältnis zweier getrennt gestoppter Zeiten.
///
/// Zwei getrennt gestoppte Zeiten driften unter Fremdlast auseinander, und eine
/// Zusicherung, die in einem von fünf Läufen ohne Grund fehlschlägt, ist keine
/// (`cargo test` bricht nach dem ersten roten Testbinary ab). Deshalb dieselbe
/// Eingabe, abwechselnd durch beide Fassungen, Bestwert aus je
/// [`MESSLAEUFE`] Läufen — Fremdlast trifft dann beide Seiten.
///
/// Gemessen mit `--release` und 20 000 Warnungen: alt 0,359 s, neu 0,005 s.
/// Die Schranke von 8 lässt eine Größenordnung Luft und trägt auch im
/// Debug-Bau.
///
/// Gegenprobe eingebaut: beide Fassungen müssen dasselbe Ergebnis liefern.
/// Sonst wäre der Test auch dann grün, wenn die neue Fassung nur nichts mehr
/// einträgt.
#[test]
fn die_entdopplung_ist_nicht_mehr_quadratisch() {
    let quelle: Vec<String> = (0..20_000)
        .map(|i| {
            format!(
                "Seite {i}: irgendein Satz, der lang genug ist, dass ein Vergleich \
                 nicht schon am ersten Zeichen entscheidet."
            )
        })
        .collect();

    let mut t_alt = Duration::MAX;
    let mut t_neu = Duration::MAX;
    let (mut r_alt, mut r_neu) = (Vec::new(), Vec::new());
    for _ in 0..MESSLAEUFE {
        let mut ziel = Vec::new();
        let start = Instant::now();
        push_warnings_alt(&mut ziel, quelle.clone());
        t_alt = t_alt.min(start.elapsed());
        r_alt = ziel;

        let mut ziel = Vec::new();
        let start = Instant::now();
        push_warnings(&mut ziel, quelle.clone());
        t_neu = t_neu.min(start.elapsed());
        r_neu = ziel;
    }

    assert_eq!(r_alt.len(), 20_000, "die alte Fassung trägt alles ein");
    assert_eq!(r_neu, r_alt, "die neue Fassung trägt etwas anderes ein");

    let faktor = t_alt.as_secs_f64() / t_neu.as_secs_f64().max(1e-9);
    println!(
        "20 000 Warnungen: alt {t_alt:?}, neu {t_neu:?} — Faktor {faktor:.1} \
         (Bestwert aus je {MESSLAEUFE} Läufen, abwechselnd)"
    );
    assert!(
        faktor > 8.0,
        "die neue Fassung ist nur {faktor:.1}-fach schneller als die alte — \
         das reicht nicht, um quadratisch von linear zu unterscheiden \
         (alt {t_alt:?}, neu {t_neu:?})"
    );
}
