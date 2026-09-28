//! Ein Schwärzungsbereich mit unbrauchbaren Koordinaten darf **nichts** tun —
//! und muss es sagen.
//!
//! # Der Befund und warum er hier landet
//!
//! `Rect::covered_fraction` meldete gegen ein NaN-Rechteck `1.0`
//! (`f64::min`/`max` schlucken NaN, siehe
//! `redact-core/tests/unbrauchbare_koordinaten.rs`). Für [`hidden_flags`] hieß
//! das: ein Bereich mit unbrauchbaren Koordinaten überdeckt **jedes Zeichen der
//! Seite**. Aufgefallen ist es beim Umbau ebendieser Funktion — `per_redaction`
//! meldete `[10, 4]` statt `[0, 4]` —, und aufgehalten hat es seither ein
//! Torwächter im Aufrufer (`touches`, `redact-pdf/src/redact.rs`).
//!
//! Ein Torwächter im Aufrufer ist für den nächsten Aufrufer eine Mine. Die
//! Regel steht deshalb jetzt im Typ. Dieser Test misst nach, dass sie am
//! **ganzen Weg** trägt, und zwar an der Stelle, an der die Richtungsfrage
//! wirklich weh tut: bei der Schwärzung selbst.
//!
//! # Was hier gefordert wird
//!
//! Für einen unbrauchbaren Bereich gilt dreierlei, und alle drei zusammen:
//!
//! 1. **kein Zeichen entfernt** — sonst verschwände der Text der ganzen Seite;
//! 2. **kein Deck-Rechteck gezeichnet** — sonst stünde `NaN NaN NaN NaN re` im
//!    Content-Stream, was kein Betrachter zeichnet und was die Ausgabedatei
//!    ungültig macht;
//! 3. **beides zusammen**, denn nur 1. allein wäre der stillste Schaden: alles
//!    weg, nichts schwarz.
//!
//! Erreichbar ist der Fall von der Kommandozeile aus mit `--padding nan` —
//! `clap` nimmt „nan“ als `f64` an, und `Rect::expanded` macht daraus ein
//! NaN-Rechteck. Deshalb steht dieser Weg unten als eigener Fall.

use redact_core::{Action, Point, Rect, Redaction, Region, Source};
use redact_pdf::testing::{build_pdf, TextItem};
use redact_pdf::{load_from_bytes, PdfRedactor};

const IBAN: &str = "DE89 3704 0044 0532 0130 00";
const NAME: &str = "Kontoinhaber Max Mustermann";

fn manual(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
            rect,
            None,
            Source::Manual {
                reason: "Test".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Eine Seite mit zwei Zeilen: IBAN bei y = 700, Name bei y = 650.
fn seite() -> lopdf::Document {
    let bytes = build_pdf(&[vec![
        TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {IBAN}")),
        TextItem::new(72.0, 650.0, 10.0, NAME),
    ]]);
    load_from_bytes(&bytes).expect("PDF ladbar")
}

/// Alle Bauarten unbrauchbarer Rechtecke — dieselben wie im Kerntest.
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
            "eine Ecke NaN",
            Rect {
                ll: Point::new(60.0, 690.0),
                ur: Point::new(n, n),
            },
        ),
        (
            "unendlich gross",
            Rect {
                ll: Point::new(m, m),
                ur: Point::new(p, p),
            },
        ),
    ]
}

/// Der ausgepackte Content-Stream der ersten Seite.
fn seiteninhalt(doc: &lopdf::Document) -> String {
    let page_id = *doc.get_pages().values().next().expect("eine Seite");
    let data = doc.get_page_content(page_id).expect("Content lesbar");
    String::from_utf8_lossy(&data).into_owned()
}

/// **Der Befund.** Ein unbrauchbarer Bereich entfernt kein Zeichen und
/// zeichnet kein Rechteck.
#[test]
fn ein_unbrauchbarer_bereich_tut_nichts() {
    for (name, kaputt) in unbrauchbare() {
        let mut doc = seite();
        let report = PdfRedactor::new()
            .apply_with_report(&mut doc, &[manual(kaputt)])
            .expect("Schwärzung läuft");

        assert_eq!(
            report.per_redaction[0], 0,
            "{name}: hat {} Zeichen entfernt — ein Bereich ohne brauchbare \
             Koordinaten überdeckt die ganze Seite",
            report.per_redaction[0]
        );
        assert_eq!(
            report.removed_glyphs, 0,
            "{name}: Zeichen aus dem Strom entfernt"
        );
        assert_eq!(
            report.drawn_rects, 0,
            "{name}: ein Deck-Rechteck gezeichnet, das kein Betrachter zeichnen kann"
        );

        // Der Text steht noch da — hier ist das die *richtige* Antwort: der
        // Bereich hat nie gesagt, wo er liegt.
        let inhalt = seiteninhalt(&doc);
        assert!(inhalt.contains("IBAN"), "{name}: Text ohne Anlass entfernt");
        assert!(inhalt.contains("Kontoinhaber"), "{name}: Text ohne Anlass");
    }
}

/// Kein `NaN` und kein `inf` in der Ausgabedatei. Ein PDF-Real kennt beides
/// nicht; ein Betrachter, der auf `NaN NaN NaN NaN re` trifft, überspringt den
/// Operator im besten Fall und bricht im schlechteren die Seite ab.
#[test]
fn in_die_ausgabe_gerat_keine_unzahl() {
    for (name, kaputt) in unbrauchbare() {
        let mut doc = seite();
        PdfRedactor::new()
            .apply_with_report(&mut doc, &[manual(kaputt)])
            .expect("Schwärzung läuft");

        let inhalt = seiteninhalt(&doc);
        for wort in ["NaN", "nan", "inf", "Inf"] {
            assert!(
                !inhalt.contains(wort),
                "{name}: „{wort}“ steht im Content-Stream:\n{inhalt}"
            );
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("speicherbar");
        let datei = String::from_utf8_lossy(&bytes);
        assert!(
            !datei.contains("NaN") && !datei.contains("inf"),
            "{name}: „NaN“/„inf“ steht in der Ausgabedatei"
        );
    }
}

/// **Der erreichbare Weg.** `redact-rs --padding nan` macht aus einem völlig
/// gewöhnlichen Bereich einen unbrauchbaren. Vorher entfernte das den Text der
/// ganzen Seite, sobald der Torwächter im Aufrufer fehlte, und schrieb in
/// jedem Fall `NaN NaN NaN NaN re` in die Ausgabe.
#[test]
fn ein_rand_von_nan_macht_die_schwaerzung_wirkungslos_aber_nicht_gefaehrlich() {
    for rand in [f64::NAN, f64::INFINITY] {
        let mut doc = seite();
        // Ein Rechteck, das ohne den Rand genau die IBAN-Zeile träfe.
        let report = PdfRedactor::with_padding(rand)
            .apply_with_report(&mut doc, &[manual(Rect::new(70.0, 698.0, 260.0, 712.0))])
            .expect("Schwärzung läuft");

        assert_eq!(report.drawn_rects, 0, "Rand {rand}: Rechteck gezeichnet");
        assert_eq!(report.removed_glyphs, 0, "Rand {rand}: Zeichen entfernt");
        let inhalt = seiteninhalt(&doc);
        assert!(
            !inhalt.contains("NaN") && !inhalt.contains("inf"),
            "Rand {rand}: Unzahl im Content-Stream:\n{inhalt}"
        );
    }
}

/// **Die Gegenprobe.** Ein gewöhnlicher Bereich mit gewöhnlichem Rand wirkt
/// weiterhin — ohne diesen Test wären alle Zusagen oben auch dann erfüllt,
/// wenn die Schwärzung gar nichts mehr täte.
#[test]
fn ein_gewoehnlicher_bereich_wirkt_weiterhin() {
    let mut doc = seite();
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[manual(Rect::new(70.0, 698.0, 260.0, 712.0))])
        .expect("Schwärzung läuft");

    assert!(
        report.per_redaction[0] > 0,
        "die IBAN-Zeile wurde nicht getroffen"
    );
    assert_eq!(report.drawn_rects, 1, "kein Deck-Rechteck gezeichnet");
    let inhalt = seiteninhalt(&doc);
    assert!(
        !inhalt.contains("3704"),
        "die IBAN steht noch im Strom:\n{inhalt}"
    );
    assert!(
        inhalt.contains("Kontoinhaber"),
        "die zweite Zeile wurde mit entfernt"
    );
}

/// Und die Probe darauf, dass ein unbrauchbarer Bereich einen *gültigen*
/// daneben nicht mit herunterzieht: beide zusammen in einem Lauf.
#[test]
fn ein_unbrauchbarer_bereich_stoert_den_gueltigen_daneben_nicht() {
    let mut doc = seite();
    let kaputt = Rect {
        ll: Point::new(f64::NAN, f64::NAN),
        ur: Point::new(f64::NAN, f64::NAN),
    };
    let report = PdfRedactor::new()
        .apply_with_report(
            &mut doc,
            &[manual(kaputt), manual(Rect::new(70.0, 698.0, 260.0, 712.0))],
        )
        .expect("Schwärzung läuft");

    assert_eq!(
        report.per_redaction[0], 0,
        "der unbrauchbare Bereich wirkte"
    );
    assert!(
        report.per_redaction[1] > 0,
        "der gültige Bereich wirkte nicht"
    );
    assert_eq!(
        report.drawn_rects, 1,
        "genau ein Rechteck gehört gezeichnet"
    );
    assert!(
        !seiteninhalt(&doc).contains("3704"),
        "die IBAN steht noch da"
    );
}
