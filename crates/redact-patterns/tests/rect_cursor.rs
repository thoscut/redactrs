//! Was der Glyphen-Zeiger am Ergebnis ändern darf: nichts.
//!
//! # Warum es diese Datei gibt
//!
//! `find_matches` fragte für **jeden** Treffer `TextRun::rect_for_byte_range`,
//! und diese Funktion zählt Byte für Byte von Glyphe 0 los. Bei M Treffern in
//! einer Zeile aus G Glyphen sind das rund G·M/2 Schritte. Gemessen an einer
//! Zeile aus lauter IBANs: 16 000 Treffer 1,233 s, 32 000 Treffer 6,145 s —
//! die doppelte Eingabe kostete das Fünffache. Die Kontrolle nagelt die
//! Ursache fest: dieselbe Zeichenzahl auf viele Zeilen verteilt kostete einen
//! Bruchteil. Es lag an der Zeilen*länge*, nicht an der Glyphenzahl.
//!
//! Seither wandert ein [`redact_core::geometry::GlyphCursor`] je Muster und
//! Lauf **einmal** durch die Glyphen.
//!
//! # Was hier geprüft wird
//!
//! `rect_for_byte_range` bestimmt, *wo* das schwarze Rechteck landet. Eine
//! schnelle falsche Koordinate schwärzt den falschen Text und lässt den
//! richtigen stehen — das wäre das schlechteste Ergebnis. Deshalb steht der
//! Gleichheitsnachweis vor der Geschwindigkeit:
//!
//! * [`der_zeiger_aendert_kein_einziges_rechteck`] fährt die **alte** Fassung
//!   (hier ausgeschrieben) und die neue über dieselben Läufe und vergleicht die
//!   Regionen bitgenau.
//! * [`ein_muster_mit_target_im_look_ahead_laeuft_rueckwaerts`] hält den Fall
//!   fest, wegen dem der Zeiger die Aufsteigerei **prüft** statt sie
//!   vorauszusetzen.
//! * [`die_suche_waechst_linear_mit_der_trefferzahl`] misst nach, dass die
//!   Kurve wirklich gerade geworden ist.
//!
//! Die Messreihe bis 32 000 Treffer steht in [`messreihe`] (`#[ignore]`, weil
//! sie Minuten dauert und eine Wanduhr keine Zusicherung trägt).

use std::time::{Duration, Instant};

use fancy_regex::{Regex, RegexBuilder};
use redact_core::{Glyph, Rect, Region, Result, Source, TextRun};
use redact_patterns::{
    builtin_patterns, validate_bic, validate_creditor_id, validate_iban, validate_luhn, PatternDef,
    PatternMatcher, Validator, BACKTRACK_LIMIT, CONTEXT_GROUP, DEFAULT_MIN_CONFIDENCE,
    TARGET_GROUP,
};

// ---------------------------------------------------------------------------
// Die Fassung von vorher, ausgeschrieben.
// ---------------------------------------------------------------------------

/// Der Stand von `TextRun::rect_for_byte_range` **vor** dem Zeiger.
///
/// Absichtlich eine eigene Kopie: die Methode geht seit der Änderung selbst
/// durch den Zeiger und wäre als Vergleichsmaßstab wertlos.
fn referenz_rect(run: &TextRun, start: usize, end: usize) -> Option<Rect> {
    if start >= end {
        return None;
    }
    let mut byte = 0usize;
    let mut acc: Option<Rect> = None;
    for glyph in &run.glyphs {
        let len = glyph.ch.len_utf8();
        if byte >= start && byte < end {
            acc = Some(match acc {
                Some(r) => r.union(&glyph.rect),
                None => glyph.rect,
            });
        }
        byte += len;
        if byte >= end {
            break;
        }
    }
    acc
}

/// Konfidenz, auf die ein bestandener Validator anhebt — wie im Matcher.
const VALIDATED_CONFIDENCE: f32 = 0.99;

/// `matcher::trim_range`, ausgeschrieben.
fn referenz_trim(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let slice = text.get(start..end)?;
    let trimmed = slice.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lead = slice.len() - slice.trim_start().len();
    Some((start + lead, start + lead + trimmed.len()))
}

/// `matcher::check`, ausgeschrieben.
fn referenz_check(def: &PatternDef, text: &str, has_context: bool) -> Option<f32> {
    let confidence = match def.confidence_without_context {
        Some(weak) if !has_context => weak,
        _ => def.confidence,
    };
    let ok = match def.validator {
        None => return Some(confidence),
        Some(Validator::Iban) => validate_iban(text),
        Some(Validator::CreditorId) => validate_creditor_id(text),
        Some(Validator::Bic) => validate_bic(text),
        Some(Validator::Luhn) => validate_luhn(text),
    };
    if !ok {
        return None;
    }
    Some(confidence.max(VALIDATED_CONFIDENCE))
}

/// `PatternMatcher::find_matches`, **so wie es vor dem Zeiger aussah**: eine
/// neue Zählung von Glyphe 0 für jeden einzelnen Treffer.
fn referenz_find_matches(
    defs: &[PatternDef],
    min_confidence: f32,
    runs: &[TextRun],
) -> Result<Vec<Region>> {
    let compiled: Vec<(usize, Regex)> = defs
        .iter()
        .enumerate()
        .filter(|(_, d)| d.enabled)
        .map(|(i, d)| {
            (
                i,
                RegexBuilder::new(&d.regex)
                    .backtrack_limit(BACKTRACK_LIMIT)
                    .build()
                    .expect("Regex der Referenz"),
            )
        })
        .collect();

    let mut regions = Vec::new();
    for run in runs {
        for (index, regex) in &compiled {
            let def = &defs[*index];
            for found in regex.captures_iter(&run.text) {
                let caps = found.map_err(|e| {
                    redact_core::RedactError::Pattern(format!("Referenz {}: {e}", def.id))
                })?;
                let whole = caps.get(0).expect("Gesamttreffer");
                let m = caps.name(TARGET_GROUP).unwrap_or(whole);
                let Some((start, end)) = referenz_trim(&run.text, m.start(), m.end()) else {
                    continue;
                };
                let text = &run.text[start..end];
                let has_context = caps.name(CONTEXT_GROUP).is_some();
                let Some(confidence) = referenz_check(def, text, has_context) else {
                    continue;
                };
                if confidence < min_confidence {
                    continue;
                }
                let Some(rect) = referenz_rect(run, start, end) else {
                    continue;
                };
                regions.push(Region::new(
                    run.page,
                    rect,
                    Some(text.to_string()),
                    Source::Pattern {
                        pattern_id: def.id.clone(),
                        confidence,
                    },
                ));
            }
        }
    }
    Ok(regions)
}

// ---------------------------------------------------------------------------
// Testdaten
// ---------------------------------------------------------------------------

/// Xorshift64* — deterministisch und ohne neue Abhängigkeit.
struct Zufall(u64);

impl Zufall {
    fn neu(saat: u64) -> Self {
        Self(saat | 1)
    }
    fn zahl(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn bis(&mut self, n: usize) -> usize {
        (self.zahl() % n.max(1) as u64) as usize
    }
}

/// Baut einen Lauf aus Text — mit Zellen, wie sie aus dem Extraktor kommen.
///
/// Mehrbyte-Zeichen bekommen eine breitere Zelle, und je vier Zeichen teilen
/// sich einmal eine Zelle (so behandelt `redact-pdf` die Teilzeichen einer
/// Ligatur: ein Code, mehrere Zeichen, kein eigener Vorschub).
fn lauf(page: usize, text: &str) -> TextRun {
    let mut glyphs = Vec::new();
    let mut x = 0.0f64;
    for (i, ch) in text.chars().enumerate() {
        let breite = 3.0 + ch.len_utf8() as f64;
        // Jedes vierte Zeichen sitzt auf der Zelle seines Vorgängers.
        if i % 4 == 3 && !glyphs.is_empty() {
            let vorher: &Glyph = glyphs.last().expect("nicht leer");
            let r = vorher.rect;
            glyphs.push(Glyph { ch, rect: r });
        } else {
            glyphs.push(Glyph {
                ch,
                rect: Rect::new(x, 1.5, x + breite, 11.0),
            });
            x += breite;
        }
    }
    TextRun::new(page, glyphs)
}

/// Eine Region, heruntergebrochen auf das, was verglichen wird: Seite,
/// Rechteck als Bitmuster, Text, Muster-ID, Konfidenz als Bitmuster.
type Vergleichbar = (usize, [u64; 4], Option<String>, String, u32);

/// Bitmuster eines Rechtecks — `==` auf `f64` wäre bei `-0.0`/`0.0` und `NaN`
/// die falsche Auskunft. Verlangt ist *dasselbe* Rechteck.
fn bits(regionen: &[Region]) -> Vec<Vergleichbar> {
    regionen
        .iter()
        .map(|r| {
            let (id, conf) = match &r.source {
                Source::Pattern {
                    pattern_id,
                    confidence,
                } => (pattern_id.clone(), confidence.to_bits()),
                other => panic!("nur Pattern-Treffer erwartet, nicht {other:?}"),
            };
            (
                r.page,
                [
                    r.rect.ll.x.to_bits(),
                    r.rect.ll.y.to_bits(),
                    r.rect.ur.x.to_bits(),
                    r.rect.ur.y.to_bits(),
                ],
                r.text.clone(),
                id,
                conf,
            )
        })
        .collect()
}

/// Gültige deutsche IBANs — sie müssen die mod-97-Prüfung bestehen, sonst
/// verwirft der Matcher sie und die ganze Messung liefe ins Leere.
fn iban_de(kontonummer: u64) -> String {
    let bban = format!("{:08}{:010}", 37_040_044u64, kontonummer % 10_000_000_000);
    // „DE00" ans Ende, Buchstaben als Zahl (D=13, E=14), dann 98 - rest.
    let mut rest = 0u64;
    for c in bban.chars().chain("131400".chars()) {
        rest = (rest * 10 + c.to_digit(10).expect("Ziffer") as u64) % 97;
    }
    let pruef = 98 - rest;
    format!("DE{pruef:02}{bban}")
}

// ---------------------------------------------------------------------------
// (1) Der Gleichheitsnachweis
// ---------------------------------------------------------------------------

/// **Der Gleichheitsnachweis.** Alte und neue Fassung über viele zufällige
/// Läufe: dieselben Regionen, bitgenau.
///
/// Abgedeckt sind die Stellen, an denen sich ein Zeiger verzählen kann:
/// Treffer ganz am Anfang und ganz am Ende einer Zeile, unmittelbar
/// aufeinanderfolgende Treffer, Mehrbyte-Zeichen (Umlaute, „€", ein
/// Vierbyte-Zeichen), Ligaturgruppen (mehrere Glyphen auf einer Zelle) und
/// mehrere Muster über demselben Lauf.
#[test]
fn der_zeiger_aendert_kein_einziges_rechteck() {
    // Bausteine: Treffer, Beinahe-Treffer und Zeichen, die die Byte-Zählung
    // von der Zeichenzählung trennen.
    let bausteine: &[&str] = &[
        "DE89370400440532013000",
        "DE89 3704 0044 0532 0130 00",
        "Kto. 4711 0815",
        "BLZ 50010517",
        "Steuer-ID: 12345678901",
        "max.mustermann@example.org",
        "Telefon: +49 30 123456789",
        "COBADEFFXXX",
        " ",
        "  ",
        "ä",
        "Ümläute",
        "€ 1.234,56",
        "𝄞",
        "ﬁnanzieren",
        "Überweisung",
        "-",
        "x",
        "0049 30 1234567",
        "DE98ZZZ09999999999",
    ];

    let matcher = PatternMatcher::new(&[]).expect("alle eingebauten Muster");
    let defs = builtin_patterns();

    let mut r = Zufall::neu(0x9E37_79B9_7F4A_7C15);
    let mut regionen_gesamt = 0usize;

    for fall in 0..400 {
        // Mal eine Zeile, mal mehrere — die Reihenfolge der Ausgabe hängt
        // an beidem.
        let zeilen = 1 + r.bis(3);
        let runs: Vec<TextRun> = (0..zeilen)
            .map(|seite| {
                let stuecke = 1 + r.bis(14);
                let mut text = String::new();
                for _ in 0..stuecke {
                    text.push_str(bausteine[r.bis(bausteine.len())]);
                }
                lauf(seite, &text)
            })
            .collect();

        let alt = referenz_find_matches(&defs, DEFAULT_MIN_CONFIDENCE, &runs).expect("Referenz");
        let neu = matcher.find_matches(&runs).expect("Zeiger");
        assert_eq!(
            bits(&alt),
            bits(&neu),
            "Fall {fall}: {:?}",
            runs.iter().map(|r| &r.text).collect::<Vec<_>>()
        );
        regionen_gesamt += neu.len();
    }

    // Gegenprobe: es wurden wirklich Rechtecke verglichen und nicht 400 leere
    // Listen gegeneinandergehalten.
    assert!(
        regionen_gesamt > 500,
        "nur {regionen_gesamt} Regionen — die Testdaten treffen nichts"
    );
    println!("400 Fälle, {regionen_gesamt} Regionen, bitgenau identisch");
}

/// Dieselbe Gleichheit für die Grenzfälle der Zeile: ganz vorn, ganz hinten,
/// lückenlos hintereinander — hier ausgeschrieben statt gewürfelt.
#[test]
fn treffer_am_rand_und_dicht_an_dicht() {
    let matcher = PatternMatcher::new(&[]).expect("alle eingebauten Muster");
    let defs = builtin_patterns();

    let a = iban_de(1);
    let b = iban_de(2);
    let zeilen: Vec<String> = vec![
        // Treffer beginnt am ersten Byte.
        format!("{a} Rest der Zeile"),
        // Treffer endet am letzten Byte.
        format!("Vorspann {a}"),
        // Nur der Treffer.
        a.clone(),
        // Zwei Treffer ohne Lücke — der Zeiger steht nach dem ersten genau am
        // Anfang des zweiten.
        format!("{a} {b}"),
        // Mit Mehrbyte-Zeichen davor: Byte- und Zeichenzählung laufen
        // auseinander.
        format!("Überweisung an {a}"),
        format!("𝄞ﬁä{a}€"),
        // Der Zeiger muss zwischen zwei Mustern zurück: E-Mail vorn, IBAN
        // hinten, und die Muster laufen in fester Reihenfolge über die Zeile.
        format!("max.mustermann@example.org zahlt an {a}"),
        // Viele Treffer dicht an dicht.
        format!("{a} {b} {a} {b} {a}"),
        // Kontext-Muster am Zeilenanfang und -ende.
        "Kto. 4711 0815".to_string(),
        "BLZ 50010517".to_string(),
    ];

    for zeile in &zeilen {
        let runs = [lauf(0, zeile)];
        let alt = referenz_find_matches(&defs, DEFAULT_MIN_CONFIDENCE, &runs).expect("Referenz");
        let neu = matcher.find_matches(&runs).expect("Zeiger");
        assert!(!neu.is_empty(), "{zeile:?} trifft nichts");
        assert_eq!(bits(&alt), bits(&neu), "{zeile:?}");
    }
}

// ---------------------------------------------------------------------------
// (2) Die Voraussetzung — und warum sie nicht vorausgesetzt wird
// ---------------------------------------------------------------------------

/// Was `captures_iter` von `fancy-regex` zusichert: die **Gesamttreffer**
/// (Gruppe 0) kommen aufsteigend und überschneidungsfrei.
///
/// Das ist keine Vermutung, sondern die Bauart des Iterators: er sucht ab dem
/// Ende des vorigen Treffers weiter (`captures_from_pos(text, last_end)`).
/// Hier wird es an den eingebauten Mustern nachgemessen, damit ein
/// Versionssprung der Bibliothek nicht still daran rüttelt.
#[test]
fn gesamttreffer_kommen_aufsteigend_und_ueberschneidungsfrei() {
    let text = "IBAN DE89 3704 0044 0532 0130 00 Kto. 4711 0815 BLZ 50010517 \
                a@b.de Tel. +49 30 123456789 Steuer-ID 12345678901 COBADEFFXXX "
        .repeat(20);
    let mut geprueft = 0usize;
    for def in builtin_patterns() {
        let re = RegexBuilder::new(&def.regex)
            .backtrack_limit(BACKTRACK_LIMIT)
            .build()
            .expect("eingebautes Muster");
        let mut ende = 0usize;
        for caps in re.captures_iter(&text) {
            let m = caps.expect("Suche").get(0).expect("Gesamttreffer");
            assert!(
                m.start() >= ende,
                "{}: Treffer {}..{} überschneidet den vorigen (endete bei {ende})",
                def.id,
                m.start(),
                m.end()
            );
            ende = m.end();
            geprueft += 1;
        }
    }
    assert!(geprueft > 100, "nur {geprueft} Treffer geprüft");
}

/// **Und was `captures_iter` nicht zusichert.**
///
/// Geschwärzt wird nicht der Gesamttreffer, sondern die Gruppe `target` —
/// und die darf in einem Look-around stehen. Dann liegt sie außerhalb des
/// Gesamttreffers, und die Bereiche laufen rückwärts. Über
/// `--patterns-config` bringt so ein Muster jeder mit.
///
/// Das Muster unten ist genau so eins: die erste Alternative fängt `target`
/// sieben Zeichen **hinter** dem Treffer ein, die zweite gar nicht (dann gilt
/// der Gesamttreffer). Auf `"AQxxxxZ"` kommt erst 6..7, dann 1..2.
///
/// Der Zeiger darf daran nicht scheitern — deshalb prüft er, statt
/// vorauszusetzen. Der Test hält beides fest: dass der Rücklauf wirklich
/// auftritt (sonst wäre er ein grüner Haken ohne Aussage), und dass beide
/// Fassungen dieselben Rechtecke liefern.
#[test]
fn ein_muster_mit_target_im_look_ahead_laeuft_rueckwaerts() {
    const REGEX: &str = r"(?:A(?=.....(?<target>Z))|Q)";
    let text = "AQxxxxZ AQxxxxZ AQxxxxZ";

    // (a) Der Rücklauf ist echt.
    let re = Regex::new(REGEX).expect("Muster");
    let mut bereiche = Vec::new();
    for caps in re.captures_iter(text) {
        let caps = caps.expect("Suche");
        let whole = caps.get(0).expect("Gesamttreffer");
        let m = caps.name(TARGET_GROUP).unwrap_or(whole);
        bereiche.push((m.start(), m.end()));
    }
    assert!(
        bereiche.windows(2).any(|w| w[1].0 < w[0].0),
        "das Muster läuft gar nicht rückwärts: {bereiche:?} — dann prüft dieser \
         Test nichts mehr"
    );

    // (b) Und beide Fassungen liefern trotzdem dasselbe.
    let def = PatternDef {
        id: "rueckwaerts".to_string(),
        regex: REGEX.to_string(),
        description: String::new(),
        confidence: 0.9,
        confidence_without_context: None,
        enabled: true,
        validator: None,
    };
    let runs = [lauf(0, text)];
    let matcher = PatternMatcher::with_defs(vec![def.clone()]).expect("Muster");
    let alt = referenz_find_matches(&[def], DEFAULT_MIN_CONFIDENCE, &runs).expect("Referenz");
    let neu = matcher.find_matches(&runs).expect("Zeiger");
    assert!(!neu.is_empty());
    assert_eq!(bits(&alt), bits(&neu));
}

/// Ein eigenes Muster, dessen `target` im Look-**behind** steht: dort liegt
/// der Bereich *vor* dem Gesamttreffer. Auch das muss dieselben Rechtecke
/// liefern.
#[test]
fn ein_muster_mit_target_im_look_behind_liefert_dieselben_rechtecke() {
    let def = PatternDef {
        id: "davor".to_string(),
        regex: r"(?<=(?<target>[0-9]{4}))-EUR".to_string(),
        description: String::new(),
        confidence: 0.9,
        confidence_without_context: None,
        enabled: true,
        validator: None,
    };
    let text = "1234-EUR ä5678-EUR 9012-EUR€";
    let runs = [lauf(0, text)];
    let matcher = PatternMatcher::with_defs(vec![def.clone()]).expect("Muster");
    let alt = referenz_find_matches(&[def], DEFAULT_MIN_CONFIDENCE, &runs).expect("Referenz");
    let neu = matcher.find_matches(&runs).expect("Zeiger");
    assert_eq!(neu.len(), 3, "{neu:?}");
    assert_eq!(bits(&alt), bits(&neu));
}

// ---------------------------------------------------------------------------
// (3) Und erst jetzt: die Geschwindigkeit
// ---------------------------------------------------------------------------

/// Wie viele Messläufe je Größe — siehe `backtracking.rs`, dort steht die
/// Begründung für den Bestwert statt einer Einzelmessung.
const MESSLAEUFE: usize = 3;

fn ibanzeile(n: usize) -> String {
    let mut s = String::with_capacity(n * 28);
    for i in 0..n {
        s.push_str(&iban_de(i as u64));
        s.push(' ');
    }
    s
}

fn bestzeit(f: impl Fn() -> usize) -> (Duration, usize) {
    let mut beste = Duration::MAX;
    let mut treffer = 0;
    for _ in 0..MESSLAEUFE {
        let start = Instant::now();
        treffer = f();
        beste = beste.min(start.elapsed());
    }
    (beste, treffer)
}

/// **Die Kurve ist gerade geworden** — gemessen als Verhältnis zur alten
/// Fassung, nicht als Verhältnis zweier getrennt gestoppter Zeiten.
///
/// Der erste Anlauf verglich die Zeit bei 4 000 Treffern mit der bei 8 000
/// und verlangte einen Faktor unter 2,6. Das flatterte: zwei Agenten haben
/// unabhängig Werte zwischen 1,72 und 3,84 gemessen — allein grün, unter
/// paralleler Last rot. Zwei *getrennt* gestoppte Zeiten driften eben
/// auseinander, und eine Zusicherung, die in einem von fünf Läufen ohne
/// Grund fehlschlägt, ist keine: `cargo test` bricht nach dem ersten roten
/// Testbinary ab, ein Flattern verdeckt also die halbe Suite.
///
/// Deshalb steht hier ein Verhältnis, das Fremdlast **beide Seiten
/// gleichzeitig** trifft: dieselbe Eingabe, einmal durch die alte Fassung
/// (`referenz_find_matches`, in dieser Datei ausgeschrieben) und einmal
/// durch die neue, abwechselnd gemessen. Der Unterschied ist keine
/// Feinheit — gemessen wurden 66-fach bei 8 000 Treffern und 543-fach bei
/// 32 000 —, die Schranke von 8 lässt also eine Größenordnung Luft.
///
/// Gegenprobe eingebaut: beide Fassungen müssen dieselbe Trefferzahl
/// liefern. Sonst wäre der Test auch dann grün, wenn die neue nur nichts
/// mehr findet.
#[test]
fn die_suche_ist_um_groessenordnungen_schneller_als_die_alte_fassung() {
    let matcher = PatternMatcher::new(&["iban_de".to_string()]).expect("iban_de");
    // Dieselbe Musterauswahl wie der Matcher: nur `iban_de` bleibt an.
    let defs: Vec<PatternDef> = builtin_patterns()
        .into_iter()
        .map(|mut d| {
            d.enabled = d.id == "iban_de";
            d
        })
        .collect();
    let laeufe = [lauf(0, &ibanzeile(8_000))];

    let mut t_alt = Duration::MAX;
    let mut t_neu = Duration::MAX;
    let (mut n_alt, mut n_neu) = (0usize, 0usize);
    // Abwechselnd, nicht nacheinander: ein Lastberg trifft sonst nur die
    // Seite, die gerade an der Reihe ist.
    for _ in 0..MESSLAEUFE {
        let start = Instant::now();
        n_alt = referenz_find_matches(&defs, DEFAULT_MIN_CONFIDENCE, &laeufe)
            .expect("Referenz")
            .len();
        t_alt = t_alt.min(start.elapsed());

        let start = Instant::now();
        n_neu = matcher.find_matches(&laeufe).expect("neu").len();
        t_neu = t_neu.min(start.elapsed());
    }

    assert_eq!(n_alt, 8_000, "die alte Fassung trifft nicht wie erwartet");
    assert_eq!(
        n_neu, n_alt,
        "die neue Fassung findet etwas anderes als die alte"
    );

    let faktor = t_alt.as_secs_f64() / t_neu.as_secs_f64().max(1e-9);
    println!(
        "8 000 Treffer in einer Zeile: alt {t_alt:?}, neu {t_neu:?} — \
         Faktor {faktor:.1} (Bestwert aus je {MESSLAEUFE} Läufen, abwechselnd)"
    );
    assert!(
        faktor > 8.0,
        "die neue Fassung ist nur {faktor:.1}-fach schneller als die alte — \
         das reicht nicht, um quadratisch von linear zu unterscheiden \
         (alt {t_alt:?}, neu {t_neu:?})"
    );
}

/// Die Kontrolle des Prüfers, als Test: **dieselbe Zeichenzahl** auf viele
/// Zeilen verteilt kostete früher einen Bruchteil dessen, was sie in einer
/// Zeile kostete. Genau daran hing der Befund — nicht an der Glyphenzahl.
///
/// Jetzt dürfen beide nicht mehr weit auseinanderliegen. Zugesichert wird
/// eine großzügige Schranke: die eine lange Zeile darf nicht mehr als das
/// Dreifache der vielen kurzen kosten (vorher war es ein Vielfaches, das mit
/// der Zeilenlänge weiterwuchs).
#[test]
fn eine_lange_zeile_kostet_nicht_mehr_als_viele_kurze() {
    let matcher = PatternMatcher::new(&["iban_de".to_string()]).expect("iban_de");
    const N: usize = 8_000;

    let eine = [lauf(0, &ibanzeile(N))];
    let viele: Vec<TextRun> = (0..N)
        .map(|i| lauf(0, &format!("{} ", iban_de(i as u64))))
        .collect();

    let (t_eine, n_eine) = bestzeit(|| matcher.find_matches(&eine).expect("eine").len());
    let (t_viele, n_viele) = bestzeit(|| matcher.find_matches(&viele).expect("viele").len());
    assert_eq!(n_eine, N);
    assert_eq!(n_viele, N);

    let faktor = t_eine.as_secs_f64() / t_viele.as_secs_f64().max(1e-9);
    println!("{N} Treffer: eine Zeile {t_eine:?}, {N} Zeilen {t_viele:?} — Faktor {faktor:.2}");
    assert!(
        faktor < 3.0,
        "die eine lange Zeile kostet das {faktor:.1}-fache der vielen kurzen — \
         die Zeilenlänge schlägt wieder durch"
    );
}

/// Die Messreihe vorher/nachher bis 32 000 Treffer.
///
/// `#[ignore]`, weil die alte Fassung dafür Minuten braucht und eine Wanduhr
/// keine Zusicherung trägt. Aufruf:
///
/// ```text
/// cargo test --release -p redact-patterns --test rect_cursor -- --ignored --nocapture
/// ```
#[test]
#[ignore = "Messreihe, dauert Minuten — von Hand aufrufen"]
fn messreihe() {
    let matcher = PatternMatcher::new(&["iban_de".to_string()]).expect("iban_de");
    let defs: Vec<PatternDef> = builtin_patterns()
        .into_iter()
        .filter(|d| d.id == "iban_de")
        .collect();

    println!(
        "{:>7} | {:>8} | {:>7} | {:>10} {:>7} | {:>10} {:>7}",
        "IBANs", "Glyphen", "Treffer", "alt", "x", "neu", "x"
    );
    let mut alt_vorher: Option<f64> = None;
    let mut neu_vorher: Option<f64> = None;
    for n in [2_000usize, 4_000, 8_000, 16_000, 32_000] {
        let runs = [lauf(0, &ibanzeile(n))];
        let glyphen = runs[0].glyphs.len();

        let start = Instant::now();
        let alt = referenz_find_matches(&defs, DEFAULT_MIN_CONFIDENCE, &runs).expect("alt");
        let t_alt = start.elapsed().as_secs_f64();

        let start = Instant::now();
        let neu = matcher.find_matches(&runs).expect("neu");
        let t_neu = start.elapsed().as_secs_f64();

        assert_eq!(bits(&alt), bits(&neu), "n = {n}");
        assert_eq!(neu.len(), n);

        println!(
            "{n:>7} | {glyphen:>8} | {:>7} | {t_alt:>9.3}s {:>7} | {t_neu:>9.3}s {:>7}",
            neu.len(),
            alt_vorher.map_or("—".to_string(), |v| format!("{:.2}", t_alt / v)),
            neu_vorher.map_or("—".to_string(), |v| format!("{:.2}", t_neu / v)),
        );
        alt_vorher = Some(t_alt);
        neu_vorher = Some(t_neu);
    }
}
