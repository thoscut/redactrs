//! Präzision und Trefferquote der eingebauten Patterns an echten Auszugszeilen.
//!
//! # Warum es diese Datei gibt
//!
//! Gemessen am 30-zeiligen Beispielauszug in [`AUSZUG`] lieferte der
//! Standardlauf vorher 40 Schwärzungen, davon 30 Fehltreffer (75 %):
//!
//! | Pattern      | Treffer | davon falsch | nachher |
//! |--------------|---------|--------------|---------|
//! | `date_de`    | 11      | 11 (100 %)   | aus     |
//! | `amount_eur` | 10      | 10 (100 %)   | aus     |
//! | `konto_nr`   |  8      |  6 (75 %)    | 3, 0 falsch |
//! | `blz`        |  4      |  2 (50 %)    | 2, 0 falsch |
//! | `phone_de`   |  3      |  1 (33 %)    | 2, 0 falsch |
//! | Rest         |  4      |  0           | 5, 0 falsch |
//!
//! Alle vier achtstelligen Token trafen `konto_nr` **und** `blz` — dieselbe
//! Stelle wurde doppelt geschwärzt, ohne dass eine der beiden Deutungen
//! erkennbar besser war. `phone_de` schlug mitten in einer gruppierten IBAN an
//! („0532 0130 00"), `konto_nr` auf den Ziffern zweier Telefonnummern. Ein
//! Auszug, auf dem jedes Datum und jeder Betrag geschwärzt ist, ist
//! unbrauchbar — und die echten Treffer gehen in der Masse unter.
//!
//! Die Tests hier halten beide Seiten fest: zu jedem Pattern eine Tabelle aus
//! Positiv- *und* Negativzeilen (Präzision **und** Trefferquote), und für den
//! ganzen Auszug eine Obergrenze für die Fehltrefferquote.

use redact_core::{Glyph, Rect, Region, Source, TextRun};
use redact_patterns::PatternMatcher;

// --------------------------------------------------------------- Hilfsmittel

fn run(page: usize, text: &str) -> TextRun {
    let glyphs = text
        .chars()
        .enumerate()
        .map(|(i, ch)| Glyph {
            ch,
            rect: Rect::new(i as f64 * 5.0, 0.0, (i as f64 + 1.0) * 5.0, 10.0),
        })
        .collect();
    TextRun::new(page, glyphs)
}

fn matcher_for(ids: &[&str]) -> PatternMatcher {
    let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
    PatternMatcher::new(&ids).unwrap()
}

fn texts(regions: &[Region]) -> Vec<&str> {
    regions
        .iter()
        .map(|r| r.text.as_deref().unwrap_or(""))
        .collect()
}

fn pattern_id(region: &Region) -> &str {
    match &region.source {
        Source::Pattern { pattern_id, .. } => pattern_id,
        other => panic!("falsche Quelle: {other:?}"),
    }
}

/// Eine Zeile aus einem echten Kontoauszug und das, was das Pattern bei
/// voreingestelltem Mindestvertrauen daraus schwärzen soll.
///
/// Leeres `erwartet` heißt: hier darf nichts geschwärzt werden.
struct Fall {
    zeile: &'static str,
    erwartet: &'static [&'static str],
}

/// Prüft die Tabelle eines Patterns und meldet Präzision und Trefferquote.
///
/// Beide werden über *alle* Zeilen gerechnet, Positiv- wie Negativzeilen:
/// Präzision = richtige Treffer / alle Treffer, Trefferquote = gefundene
/// erwartete Stellen / alle erwarteten Stellen.
fn pruefe(id: &str, tabelle: &[Fall]) {
    let m = matcher_for(&[id]);
    let mut treffer = 0usize;
    let mut richtige = 0usize;
    let mut erwartete = 0usize;
    let mut abweichungen = Vec::new();

    for fall in tabelle {
        let gefunden = m.find_matches(&[run(0, fall.zeile)]).unwrap();
        let gefunden = texts(&gefunden);
        treffer += gefunden.len();
        erwartete += fall.erwartet.len();
        richtige += gefunden
            .iter()
            .filter(|t| fall.erwartet.contains(t))
            .count();
        if gefunden != fall.erwartet.to_vec() {
            abweichungen.push(format!(
                "  {:?}\n    erwartet: {:?}\n    gefunden: {:?}",
                fall.zeile, fall.erwartet, gefunden
            ));
        }
    }

    let praezision = if treffer == 0 {
        1.0
    } else {
        richtige as f64 / treffer as f64
    };
    let quote = if erwartete == 0 {
        1.0
    } else {
        richtige as f64 / erwartete as f64
    };
    println!(
        "{id}: {} Zeilen, Präzision {:.0} % ({richtige}/{treffer}), \
         Trefferquote {:.0} % ({richtige}/{erwartete})",
        tabelle.len(),
        praezision * 100.0,
        quote * 100.0
    );
    assert!(
        abweichungen.is_empty(),
        "{id} weicht ab:\n{}",
        abweichungen.join("\n")
    );
}

// ------------------------------------------------------ Tabellen je Pattern

#[test]
fn iban_de_tabelle() {
    pruefe(
        "iban_de",
        &[
            Fall {
                zeile: "IBAN: DE89 3704 0044 0532 0130 00",
                erwartet: &["DE89 3704 0044 0532 0130 00"],
            },
            Fall {
                zeile: "Empfaenger-IBAN: DE02 1203 0000 0000 2020 51",
                erwartet: &["DE02 1203 0000 0000 2020 51"],
            },
            Fall {
                zeile: "Ueberweisung auf DE89370400440532013000 ausgefuehrt",
                erwartet: &["DE89370400440532013000"],
            },
            // Prüfziffer verdreht — die Prüfsumme fängt es ab.
            Fall {
                zeile: "IBAN: DE88 3704 0044 0532 0130 00",
                erwartet: &[],
            },
            // Bekannte Lücke: die Gläubiger-ID einer Lastschrift ist keine
            // IBAN und wird (noch) von keinem Pattern erfasst.
            Fall {
                zeile: "Glaeubiger-ID: DE98ZZZ09999999999",
                erwartet: &[],
            },
            Fall {
                zeile: "Musterbank AG - Kontoauszug Nr. 1/2026",
                erwartet: &[],
            },
        ],
    );
}

#[test]
fn konto_nr_tabelle() {
    pruefe(
        "konto_nr",
        &[
            Fall {
                zeile: "Kontonummer: 532013000",
                erwartet: &["532013000"],
            },
            Fall {
                zeile: "Konto-Nr. 1234567 bei der Musterbank",
                erwartet: &["1234567"],
            },
            // Alte, gruppiert gedruckte Kontonummer neben einer BLZ.
            Fall {
                zeile: "Kto. 4711 0815 BLZ 50010517  Rechnung Nr. 2026-0042",
                erwartet: &["4711 0815"],
            },
            Fall {
                zeile: "Vertragskonto 30012345 Abschlag Strom",
                erwartet: &["30012345"],
            },
            // Ziffernketten ohne Kontobezug — der frühere Hauptfehler.
            Fall {
                zeile: "Lohn/Gehalt 01/2026 Personalnummer 8891234",
                erwartet: &[],
            },
            Fall {
                zeile: "27.01.2026  Dauerauftrag Versicherung Police 90123456",
                erwartet: &[],
            },
            Fall {
                zeile: "Bankleitzahl: 37040044",
                erwartet: &[],
            },
            Fall {
                zeile: "Telefon: +49 30 123456789",
                erwartet: &[],
            },
            Fall {
                zeile: "IBAN: DE89 3704 0044 0532 0130 00",
                erwartet: &[],
            },
            Fall {
                zeile: "Musterbank AG - Kontoauszug Nr. 1/2026",
                erwartet: &[],
            },
        ],
    );
}

#[test]
fn blz_tabelle() {
    pruefe(
        "blz",
        &[
            Fall {
                zeile: "Bankleitzahl: 37040044",
                erwartet: &["37040044"],
            },
            Fall {
                zeile: "Kto. 4711 0815 BLZ 50010517  Rechnung Nr. 2026-0042",
                erwartet: &["50010517"],
            },
            Fall {
                zeile: "BLZ 20040000 (Commerzbank)",
                erwartet: &["20040000"],
            },
            // Achtstellig, aber keine Bankleitzahl.
            Fall {
                zeile: "Kontonummer: 87654321",
                erwartet: &[],
            },
            Fall {
                zeile: "Vertragskonto 30012345 Abschlag Strom",
                erwartet: &[],
            },
            Fall {
                zeile: "27.01.2026  Dauerauftrag Versicherung Police 90123456",
                erwartet: &[],
            },
        ],
    );
}

#[test]
fn bic_tabelle() {
    pruefe(
        "bic",
        &[
            Fall {
                zeile: "BIC: COBADEFFXXX",
                erwartet: &["COBADEFFXXX"],
            },
            Fall {
                zeile: "Empfaengerbank BIC DEUTDEFF",
                erwartet: &["DEUTDEFF"],
            },
            // Großgeschriebene Wörter fallen durch die Länderprüfung.
            Fall {
                zeile: "RECHNUNG ABSENDER MAHNUNGS",
                erwartet: &[],
            },
            Fall {
                zeile: "Musterbank AG - Kontoauszug Nr. 1/2026",
                erwartet: &[],
            },
        ],
    );
}

#[test]
fn steuer_id_tabelle() {
    pruefe(
        "steuer_id",
        &[
            Fall {
                zeile: "Steuer-ID: 12345678901",
                erwartet: &["12345678901"],
            },
            Fall {
                zeile: "Steuernummer 98765432109",
                erwartet: &["98765432109"],
            },
            Fall {
                zeile: "Steuerliche Identifikationsnummer: 11122233344",
                erwartet: &["11122233344"],
            },
            // Elf nackte Ziffern sind genauso gut eine Vorgangsnummer.
            Fall {
                zeile: "Vorgangsnummer 20260105001 gebucht",
                erwartet: &[],
            },
            Fall {
                zeile: "29.01.2026  Gutschrift Erstattung Finanzamt   215,30 EUR",
                erwartet: &[],
            },
        ],
    );
}

#[test]
fn email_tabelle() {
    pruefe(
        "email",
        &[
            Fall {
                zeile: "Kontakt: max.mustermann@example.org",
                erwartet: &["max.mustermann@example.org"],
            },
            Fall {
                zeile: "Rueckfragen an service@musterbank.de bitte",
                erwartet: &["service@musterbank.de"],
            },
            Fall {
                zeile: "Verwendungszweck: Rechnung@Januar",
                erwartet: &[],
            },
            Fall {
                zeile: "05.01.2026  Ueberweisung an Musterfirma GmbH",
                erwartet: &[],
            },
        ],
    );
}

#[test]
fn phone_de_tabelle() {
    pruefe(
        "phone_de",
        &[
            Fall {
                zeile: "Telefon: +49 30 123456789",
                erwartet: &["+49 30 123456789"],
            },
            Fall {
                zeile: "Filiale Berlin-Mitte, Telefon 030 987654321",
                erwartet: &["030 987654321"],
            },
            Fall {
                zeile: "Mobil 0170 1234567",
                erwartet: &["0170 1234567"],
            },
            // Der frühere Fehltreffer: die Ziffernblöcke einer gruppierten
            // IBAN sehen aus wie eine Nummer mit Vorwahl.
            Fall {
                zeile: "IBAN: DE89 3704 0044 0532 0130 00",
                erwartet: &[],
            },
            Fall {
                zeile: "18.01.2026  Lastschrift Stadtwerke   89,90 EUR",
                erwartet: &[],
            },
        ],
    );
}

#[test]
fn credit_card_tabelle() {
    pruefe(
        "credit_card",
        &[
            Fall {
                zeile: "Kreditkarte 4539 1488 0343 6467 belastet",
                erwartet: &["4539 1488 0343 6467"],
            },
            // Eine Ziffer verdreht — Luhn schlägt an.
            Fall {
                zeile: "Kreditkarte 4539 1488 0343 6468 belastet",
                erwartet: &[],
            },
            Fall {
                zeile: "IBAN: DE89 3704 0044 0532 0130 00",
                erwartet: &[],
            },
            Fall {
                zeile: "25.01.2026  Kartenzahlung Supermarkt Filiale 4021",
                erwartet: &[],
            },
        ],
    );
}

/// Standardmäßig aus — die Tabelle belegt trotzdem, was `--patterns date_de`
/// leistet, damit die Abschaltung eine Produktentscheidung bleibt und kein
/// Vorwand für ein kaputtes Pattern ist.
#[test]
fn date_de_tabelle() {
    pruefe(
        "date_de",
        &[
            Fall {
                zeile: "05.01.2026  Ueberweisung an Musterfirma GmbH",
                erwartet: &["05.01.2026"],
            },
            Fall {
                zeile: "Auszug vom 01.01.2026 bis 31.01.2026",
                erwartet: &["01.01.2026", "31.01.2026"],
            },
            // Bekannte Lücke: einstellige Tage/Monate ohne führende Null.
            Fall {
                zeile: "Wertstellung 5.1.2026",
                erwartet: &[],
            },
            Fall {
                zeile: "Alter Kontostand: 2.480,15 EUR",
                erwartet: &[],
            },
        ],
    );
}

/// Ebenfalls standardmäßig aus, siehe [`date_de_tabelle`].
#[test]
fn amount_eur_tabelle() {
    pruefe(
        "amount_eur",
        &[
            Fall {
                zeile: "05.01.2026  Ueberweisung   1.234,56 EUR",
                erwartet: &["1.234,56 EUR"],
            },
            Fall {
                zeile: "Neuer Kontostand: 5.795,60 EUR",
                erwartet: &["5.795,60 EUR"],
            },
            // Bekannte Lücke: ausgeschriebene Währung.
            Fall {
                zeile: "Gebuehr 4,90 Euro",
                erwartet: &[],
            },
            Fall {
                zeile: "Kontonummer: 532013000",
                erwartet: &[],
            },
        ],
    );
}

#[test]
fn iban_intl_tabelle() {
    pruefe(
        "iban_intl",
        &[
            Fall {
                zeile: "IBAN GB82 WEST 1234 5698 7654 32",
                erwartet: &["GB82 WEST 1234 5698 7654 32"],
            },
            Fall {
                zeile: "IBAN: DE89 3704 0044 0532 0130 00",
                erwartet: &["DE89 3704 0044 0532 0130 00"],
            },
            Fall {
                zeile: "Musterbank AG - Kontoauszug Nr. 1/2026",
                erwartet: &[],
            },
        ],
    );
}

// ------------------------------------------------- Fehltreffer am Gesamtlauf

/// Ein realistischer Kontoauszug, 30 Zeilen.
///
/// Er enthält bewusst die Ziffernketten, die früher alles geschwärzt haben:
/// Personal-, Policen-, Vertrags-, Rechnungs- und Telefonnummern.
const AUSZUG: &[&str] = &[
    "Musterbank AG - Kontoauszug Nr. 1/2026",
    "Kontoinhaber: Max Mustermann",
    "Musterstrasse 12, 10115 Berlin",
    "IBAN: DE89 3704 0044 0532 0130 00",
    "BIC: COBADEFFXXX",
    "Kontonummer: 532013000",
    "Bankleitzahl: 37040044",
    "Auszug vom 01.01.2026 bis 31.01.2026",
    "Alter Kontostand: 2.480,15 EUR",
    "Datum       Vorgang                                     Betrag",
    "05.01.2026  Ueberweisung an Musterfirma GmbH       1.234,56 EUR",
    "            Kto. 4711 0815 BLZ 50010517  Rechnung Nr. 2026-0042",
    "12.01.2026  Gehalt Arbeitgeber XY AG                3.500,00 EUR",
    "            Lohn/Gehalt 01/2026 Personalnummer 8891234",
    "18.01.2026  Lastschrift Stadtwerke Musterstadt         89,90 EUR",
    "            Vertragskonto 30012345 Abschlag Strom",
    "23.01.2026  Ueberweisung an Musterfirma GmbH          420,00 EUR",
    "            Verwendungszweck: Miete Februar 2026",
    "25.01.2026  Kartenzahlung Supermarkt Filiale 4021       67,45 EUR",
    "27.01.2026  Dauerauftrag Versicherung Police 90123456   58,00 EUR",
    "29.01.2026  Gutschrift Erstattung Finanzamt            215,30 EUR",
    "31.01.2026  Entgeltabrechnung Kontofuehrung              4,90 EUR",
    "Neuer Kontostand: 5.795,60 EUR",
    "Empfaenger-IBAN: DE02 1203 0000 0000 2020 51",
    "Glaeubiger-ID: DE98ZZZ09999999999",
    "Kontakt: max.mustermann@example.org",
    "Telefon: +49 30 123456789",
    "Steuer-ID: 12345678901",
    "Filiale Berlin-Mitte, Telefon 030 987654321",
    "Seite 1 von 1 - erstellt am 01.02.2026",
];

/// Das Schutzgut des Auszugs: was ein Standardlauf schwärzen *soll*.
const SCHUTZGUT: &[(&str, &str)] = &[
    ("iban_de", "DE89 3704 0044 0532 0130 00"),
    ("bic", "COBADEFFXXX"),
    ("konto_nr", "532013000"),
    ("blz", "37040044"),
    ("konto_nr", "4711 0815"),
    ("blz", "50010517"),
    ("konto_nr", "30012345"),
    ("iban_de", "DE02 1203 0000 0000 2020 51"),
    ("email", "max.mustermann@example.org"),
    ("phone_de", "+49 30 123456789"),
    ("steuer_id", "12345678901"),
    ("phone_de", "030 987654321"),
];

/// Obergrenze für den Anteil der Fehltreffer am Standardlauf.
///
/// Der Befund lag bei 75 % (30 von 40). Die Grenze ist bewusst scharf: sobald
/// ein Pattern wieder anfängt, den Inhalt des Auszugs mitzuschwärzen, fällt
/// dieser Test.
const MAX_FEHLTREFFERQUOTE: f64 = 0.10;

fn auszug_runs() -> Vec<TextRun> {
    AUSZUG
        .iter()
        .enumerate()
        .map(|(i, zeile)| run(i, zeile))
        .collect()
}

#[test]
fn standardlauf_haelt_die_fehltrefferquote_ein() {
    let regions = PatternMatcher::new(&[])
        .unwrap()
        .find_matches(&auszug_runs())
        .unwrap();

    let gefunden: Vec<(&str, &str)> = regions
        .iter()
        .map(|r| (pattern_id(r), r.text.as_deref().unwrap_or("")))
        .collect();

    let fehltreffer: Vec<&(&str, &str)> =
        gefunden.iter().filter(|t| !SCHUTZGUT.contains(t)).collect();
    let quote = fehltreffer.len() as f64 / gefunden.len() as f64;
    println!(
        "Standardlauf: {} Schwärzungen auf {} Zeilen, {} Fehltreffer ({:.0} %)",
        gefunden.len(),
        AUSZUG.len(),
        fehltreffer.len(),
        quote * 100.0
    );
    assert!(
        quote <= MAX_FEHLTREFFERQUOTE,
        "Fehltrefferquote {:.0} % über der Grenze von {:.0} %: {fehltreffer:?}",
        quote * 100.0,
        MAX_FEHLTREFFERQUOTE * 100.0
    );

    // Und die Gegenrichtung: nichts vom Schutzgut darf fehlen.
    let fehlend: Vec<&(&str, &str)> = SCHUTZGUT.iter().filter(|s| !gefunden.contains(s)).collect();
    assert!(fehlend.is_empty(), "nicht geschwärzt: {fehlend:?}");
}

/// Kein Datum und kein Betrag des Auszugs wird im Standardlauf angefasst.
#[test]
fn standardlauf_laesst_datum_und_betrag_stehen() {
    let regions = PatternMatcher::new(&[])
        .unwrap()
        .find_matches(&auszug_runs())
        .unwrap();
    for r in &regions {
        let id = pattern_id(r);
        assert!(
            id != "date_de" && id != "amount_eur",
            "{id} schwärzt {:?}",
            r.text
        );
    }
}

/// Dieselben Zeilen wie im Demo-PDF (`redact_pdf::testing::demo_statement`).
/// Das Crate hängt bewusst nicht von `redact-pdf` ab, deshalb die Kopie.
const DEMO_STATEMENT: &[&str] = &[
    "Musterbank AG - Kontoauszug",
    "Kontoinhaber: Max Mustermann",
    "IBAN: DE89 3704 0044 0532 0130 00",
    "BIC: COBADEFFXXX",
    "Kontonummer: 532013000",
    "Buchungen:",
    "05.01.2026  Ueberweisung an Musterfirma GmbH   1.234,56 EUR",
    "12.01.2026  Gehalt Arbeitgeber XY              3.500,00 EUR",
    "18.01.2026  Lastschrift Stadtwerke              89,90 EUR",
    "23.01.2026  Ueberweisung an Musterfirma GmbH     420,00 EUR",
    "Seite 2",
    "Empfaenger-IBAN: DE02 1203 0000 0000 2020 51",
    "Kontakt: max.mustermann@example.org",
    "Telefon: +49 30 123456789",
    "Steuer-ID: 12345678901",
];

/// Am Demo-PDF bleibt vom Standardlauf genau das Schutzgut übrig: vorher
/// 16 Schwärzungen, davon 10 falsch (8× Datum/Betrag, die Ziffern einer
/// Telefonnummer als `konto_nr`, ein IBAN-Block als `phone_de`) — und die
/// Steuer-ID blieb stehen, weil `steuer_id` aus war. Jetzt 7 Treffer, alle
/// richtig, Steuer-ID eingeschlossen.
#[test]
fn demo_statement_wird_nur_beim_schutzgut_geschwaerzt() {
    let runs: Vec<TextRun> = DEMO_STATEMENT
        .iter()
        .enumerate()
        .map(|(i, zeile)| run(i, zeile))
        .collect();
    let regions = PatternMatcher::new(&[])
        .unwrap()
        .find_matches(&runs)
        .unwrap();
    let gefunden: Vec<(&str, &str)> = regions
        .iter()
        .map(|r| (pattern_id(r), r.text.as_deref().unwrap_or("")))
        .collect();
    assert_eq!(
        gefunden,
        vec![
            ("iban_de", "DE89 3704 0044 0532 0130 00"),
            ("bic", "COBADEFFXXX"),
            ("konto_nr", "532013000"),
            ("iban_de", "DE02 1203 0000 0000 2020 51"),
            ("email", "max.mustermann@example.org"),
            ("phone_de", "+49 30 123456789"),
            ("steuer_id", "12345678901"),
        ]
    );
}

/// Die Verdachtsfälle sind nicht verschwunden — sie sind eine Schwelle
/// entfernt. Wer sie sehen will, senkt `min_confidence`.
#[test]
fn gesenkte_schwelle_bringt_die_verdachtsfaelle_zurueck() {
    let streng = PatternMatcher::new(&[])
        .unwrap()
        .find_matches(&auszug_runs())
        .unwrap();
    let locker = PatternMatcher::new(&[])
        .unwrap()
        .with_min_confidence(0.25)
        .unwrap()
        .find_matches(&auszug_runs())
        .unwrap();
    assert!(
        locker.len() > streng.len(),
        "{} vs. {}",
        locker.len(),
        streng.len()
    );
    let texte = texts(&locker);
    // Personalnummer und Policennummer: früher blind geschwärzt, jetzt eine
    // bewusste Entscheidung.
    assert!(texte.contains(&"8891234"), "{texte:?}");
    assert!(texte.contains(&"90123456"), "{texte:?}");
}
