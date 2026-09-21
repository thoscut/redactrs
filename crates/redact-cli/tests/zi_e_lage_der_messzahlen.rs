//! Gegenprobe zu Register #49 unter der Linse **widerlegen**.
//!
//! Die Korrektur bindet die Messzahlen der Fix-Runde 7 in `belege::messwerte`
//! und stellt fünf daraus erzeugte Sätze in den CHANGELOG-Block der Runde 7.
//! Ihr Wächter ist `belege::jede_zahl_der_letzten_runden_ist_gebunden`: er
//! schneidet einen Block aus `CHANGELOG.md`, deckt ihn mit den gebundenen
//! Sätzen ab und verlangt, dass **keine** Zahl übrig bleibt.
//!
//! Diese Datei fragt nicht, ob die fünf Sätze stimmen — sie stimmen (eigene
//! Läufe, siehe Bericht). Sie fragt, **welchen Fall der Wächter nicht sieht**,
//! und findet zwei:
//!
//! 1. **Die Lage.** Der Schnitt beginnt an der ersten `### Fix-Runde` und
//!    nicht am Abschnitt `## Unveröffentlicht`. Der Vorspann dieses
//!    Abschnitts — genau der Text, den `.github/workflows/release.yml` beim
//!    Veröffentlichen als Release-Notizen herausschneidet (`## <version>` bis
//!    zur nächsten `## `-Überschrift) — liegt damit **außerhalb** der
//!    Prüfung. Dort steht heute die pauschale Zusage „jede Angabe hier stammt
//!    aus einem Lauf des gebauten Binaries, nicht aus dem Quelltext“, und
//!    dort stehen sechs ungebundene Zahlen.
//! 2. **Die Form.** `belege::traegt_zahl` kennt Kardinalzahlen bis „zwölf“
//!    (plus „siebzehn“ und die `…mal`-Formen). „zwanzig“, „dreizehn“,
//!    „hundert“, „tausend“, „Dutzend“ sind für den Wächter keine Zahlen —
//!    eine ausgeschriebene Messzahl im Block bleibt unbemerkt. Genau diese
//!    Klasse hat das Projekt schon einmal getroffen: „siebenmal“ war eine der
//!    siebzehn Mutationen, die niemand rot machte.
//!
//! **Diese Datei ist grün, solange die Lücke offen ist.** Sie hält die Lücke
//! fest, damit sie nicht wieder verlorengeht; sie ist kein Zaun. Wird der
//! Vertrag in `belege.rs` eingetragen (Schnitt am `## `-Abschnitt, Wortliste
//! über zwölf hinaus), wird sie rot — dann hat sie ihre Aufgabe erfüllt und
//! gehört gelöscht. Der Beleg, dass die Zahlen des Vorspanns wirklich
//! ungebunden sind, steht nicht hier, sondern im Lauf des Berichts: mit
//! einem Schnitt ab `## Unveröffentlicht` wird
//! `jede_zahl_der_letzten_runden_ist_gebunden` am **unveränderten**
//! CHANGELOG rot und nennt sechs Zahlen.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Wie `belege::lf` — unter Windows checkt git mit CRLF aus.
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Wie `belege::glatt`.
fn glatt(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn changelog() -> String {
    lf(&std::fs::read_to_string(repo_root().join("CHANGELOG.md")).expect("CHANGELOG.md lesbar"))
}

fn belege_quelltext() -> String {
    lf(
        &std::fs::read_to_string(repo_root().join("crates/redact-cli/tests/belege.rs"))
            .expect("belege.rs lesbar"),
    )
}

/// Der Schnitt aus `belege::changelog_block`, Zeile für Zeile nachgebaut:
/// von der jüngsten `### Fix-Runde` bis zur drittjüngsten.
///
/// Ein Nachbau läuft still auseinander, sobald das Original sich ändert.
/// Deshalb steht hier zuerst die **Bindung an den Quelltext**: die
/// Schnittkante, die dieser Nachbau annimmt, muss die sein, die
/// `changelog_block` wirklich nimmt. Wird der Vertrag eingetragen — Schnitt am
/// `## `-Abschnitt statt an der ersten `### Fix-Runde` —, wird diese
/// Zusicherung rot, und mit ihr jeder Test dieser Datei, der auf ihr aufbaut.
fn schnitt_wie_belege(text: &str) -> (usize, usize) {
    let quelle = belege_quelltext();
    let ab = quelle
        .find("fn changelog_block()")
        .expect("`fn changelog_block` steht in belege.rs");
    let rumpf = &quelle[ab..ab + 900.min(quelle.len() - ab)];
    assert!(
        rumpf.contains("glatt(&text[ueberschriften[0]..ueberschriften[2]])"),
        "`changelog_block` schneidet nicht mehr ab `ueberschriften[0]` — \
         Lücke 1 ist angefasst, dieser Nachbau gilt nicht mehr und diese Datei \
         gehört überarbeitet oder gelöscht"
    );

    let ueberschriften: Vec<usize> = text
        .match_indices("\n### Fix-Runde ")
        .map(|(i, _)| i)
        .collect();
    assert!(
        ueberschriften.len() >= 3,
        "weniger als drei Fix-Runden im CHANGELOG"
    );
    (ueberschriften[0], ueberschriften[2])
}

/// Der Vorspann des Abschnitts `## Unveröffentlicht`: von seiner Überschrift
/// bis zur ersten `### Fix-Runde`.
fn vorspann(text: &str) -> (usize, usize) {
    let von = text
        .find("\n## Unveröffentlicht")
        .expect("Abschnitt `## Unveröffentlicht` steht im CHANGELOG");
    let (block_von, _) = schnitt_wie_belege(text);
    assert!(
        von < block_von,
        "der Vorspann liegt vor der ersten Fix-Runde"
    );
    (von, block_von)
}

/// Die Wortliste aus `belege::traegt_zahl` — **gelesen**, nicht abgeschrieben:
/// eine zweite Abschrift würde still auseinanderlaufen.
fn zahlworte_des_waechters() -> Vec<String> {
    let text = belege_quelltext();
    let ab = text
        .find("fn traegt_zahl(")
        .expect("`fn traegt_zahl` steht in belege.rs");
    let liste = text[ab..]
        .find("const ZAHLWORTE: [&str;")
        .map(|i| ab + i)
        .expect("`ZAHLWORTE` steht in `traegt_zahl`");
    let bis = text[liste..]
        .find("];")
        .map(|i| liste + i)
        .expect("die Liste endet");
    let mut aus = Vec::new();
    let mut rest = &text[liste..bis];
    while let Some(a) = rest.find('"') {
        let nach = &rest[a + 1..];
        let e = nach.find('"').expect("geschlossenes Zeichenkettenliteral");
        aus.push(nach[..e].to_string());
        rest = &nach[e + 1..];
    }
    assert!(
        aus.len() >= 20,
        "die Wortliste des Wächters ist geschrumpft — {} Wörter",
        aus.len()
    );
    aus
}

// ---------------------------------------------------------------------------
// Lücke 1 — die Lage: der Vorspann der Release-Notizen wird nicht geprüft
// ---------------------------------------------------------------------------

/// Der Vorspann von `## Unveröffentlicht` trägt Zahlen, und der geprüfte
/// Block fängt erst dahinter an.
#[test]
fn der_vorspann_der_release_notizen_liegt_ausserhalb_des_geprueften_blocks() {
    let text = changelog();
    let (vor_von, vor_bis) = vorspann(&text);
    let (block_von, block_bis) = schnitt_wie_belege(&text);

    // Der Vorspann liegt vollständig **vor** dem geprüften Block.
    assert!(
        vor_bis <= block_von,
        "der Vorspann reicht in den geprüften Block"
    );
    assert!(block_von < block_bis);

    let vor = glatt(&text[vor_von..vor_bis]);

    // Er trägt Zahlen — und zwar Zusagen über das Programm, nicht bloß eine
    // Fassungsnummer.
    assert!(
        vor.contains("Sieben Fix-Runden"),
        "der Vorspann zählt die Runden nicht mehr: {vor}"
    );
    assert!(
        vor.contains("fallen vier stille Lecks"),
        "der Vorspann zählt die Lecks nicht mehr: {vor}"
    );
    assert!(
        vor.chars().any(|c| c.is_ascii_digit()),
        "der Vorspann trägt keine Ziffer mehr: {vor}"
    );

    // Und er trägt die pauschale Zusage, an der der Block gemessen werden
    // müsste — ungeprüft, weil sie außerhalb liegt. Die fünf Sätze der Runde
    // 7 stammen ausdrücklich aus einem **Debug-Testprozess**, nicht aus dem
    // gebauten Binary; einer („100 000 Zuordnungen“) sogar aus einer
    // Konstanten des Quelltexts.
    assert!(
        vor.contains(
            "jede Angabe hier stammt aus einem Lauf des gebauten Binaries, nicht aus \
                      dem Quelltext"
        ),
        "die pauschale Zusage des Vorspanns lautet anders: {vor}"
    );
    let block = glatt(&text[block_von..block_bis]);
    assert!(
        block.contains("Nachgemessen am Baum dieser Runde (Debug, im Testprozess)"),
        "der Block nennt den Ort der neuen Messungen nicht mehr"
    );
}

/// Eine erfundene Messzahl im Vorspann bleibt dem Schnitt verborgen — sie
/// landet nie im Block, den der Wächter abdeckt.
#[test]
fn eine_erfundene_messzahl_im_vorspann_kommt_im_block_nicht_vor() {
    let text = changelog();
    let (_, vor_bis) = vorspann(&text);

    const ERFUNDEN: &str = "\nDer Redaktor braucht dafür 12,5 s und belegt 99 MB.\n";
    let mut gefaelscht = String::with_capacity(text.len() + ERFUNDEN.len());
    gefaelscht.push_str(&text[..vor_bis]);
    gefaelscht.push_str(ERFUNDEN);
    gefaelscht.push_str(&text[vor_bis..]);

    let (von, bis) = schnitt_wie_belege(&gefaelscht);
    let block = glatt(&gefaelscht[von..bis]);
    assert!(
        !block.contains("12,5 s"),
        "der Schnitt sieht den Vorspann jetzt — Lücke 1 ist geschlossen, \
         diese Datei gehört gelöscht"
    );
    assert!(
        !block.contains("99 MB"),
        "der Schnitt sieht den Vorspann jetzt — Lücke 1 ist geschlossen, \
         diese Datei gehört gelöscht"
    );
}

// ---------------------------------------------------------------------------
// Lücke 2 — die Form: ausgeschriebene Zahlen jenseits von „zwölf“
// ---------------------------------------------------------------------------

/// Die Wortliste des Wächters endet bei „zwölf“. Vier ausgeschriebene
/// Messzahlen, die eine Runde 8 ohne Weiteres schreiben würde, sind für ihn
/// keine Zahlen.
#[test]
fn ausgeschriebene_messzahlen_jenseits_von_zwoelf_sieht_der_waechter_nicht() {
    let liste = zahlworte_des_waechters();
    let hat = |w: &str| liste.iter().any(|x| x == w);

    // Was er kennt — sonst prüfte dieser Test die falsche Liste.
    for bekannt in ["zwei", "zwölf", "siebzehn", "siebenmal"] {
        assert!(
            hat(bekannt),
            "„{bekannt}“ fehlt — das ist nicht die Liste aus `traegt_zahl`"
        );
    }

    // Was er nicht kennt.
    let blind: Vec<&str> = ["dreizehn", "zwanzig", "hundert", "tausend", "Dutzend"]
        .into_iter()
        .filter(|w| !hat(&w.to_lowercase()) && !hat(w))
        .collect();
    assert_eq!(
        blind,
        vec!["dreizehn", "zwanzig", "hundert", "tausend", "Dutzend"],
        "die Wortliste ist gewachsen — Lücke 2 ist (teils) geschlossen, \
         diese Datei gehört überarbeitet oder gelöscht"
    );

    // Und ein Satz aus genau diesen Wörtern trägt für den Wächter keine Zahl.
    // Nachgebaut ist nur das Kriterium, nicht die Liste: Ziffer oder Wort aus
    // der gelesenen Liste.
    let traegt_zahl = |wort: &str| -> bool {
        let kern: String = wort
            .chars()
            .filter(|c| !"*`„“»«().,;:—–!?[]…\u{202f}".contains(*c))
            .collect();
        kern.chars().any(|c| c.is_ascii_digit()) || liste.iter().any(|w| *w == kern.to_lowercase())
    };
    let satz = "Der Redaktor braucht dafür nur noch zwanzig Millisekunden statt \
                dreizehn Sekunden, und die Spitze liegt bei einem Dutzend MB statt \
                bei tausend.";
    let gesehen: Vec<&str> = satz.split(' ').filter(|w| traegt_zahl(w)).collect();
    assert!(
        gesehen.is_empty(),
        "der Wächter sieht in diesem Satz jetzt Zahlen ({gesehen:?}) — \
         Lücke 2 ist geschlossen, diese Datei gehört gelöscht"
    );
}
