//! Gegenprüfung C der Runde 9: **die Maschine, die die Zahlen der Doku an
//! Läufe bindet** — und was in der Fix-Runde 9 daraus geworden ist.
//!
//! Der Kernanspruch hat gehalten: 183 Zahlen des geprüften Blocks wurden
//! einzeln geändert (135 Ziffernfolgen, 48 Zahlwörter), und **keine** blieb
//! unbemerkt. In der Fix-Runde 7 waren es noch 58 von 76, die grün blieben.
//!
//! Gefunden wurden fünf Löcher daneben, und dieses hier ist das schwerste:
//! **eine Zusage in Fettschrift war für den Wächter keine Zahl.** `traegt_zahl`
//! ließ Ordnungszahlen und „ein/eine/einen“ mit Absicht aus — im Fließtext sind
//! sie ein Platz in einer Liste oder der unbestimmte Artikel. Genau dort
//! standen aber drei der vier Sicherheitspunkte der Runde 8: „als **erste**
//! Anweisung", „genau **einen** Namen“, „**eine** Stelle“. Jede ließ sich
//! umdrehen (`erste` → `letzte`, `einen` → `jeden`, `eine` → `jede`), ohne dass
//! ein Test es merkte — nach der Mutation sagte die Doku das Gegenteil des
//! Fixes.
//!
//! ## Warum diese Datei jetzt den Quelltext liest
//!
//! Sie tat es nicht. Sie hielt **wörtliche Kopien** der Helfer, die sie prüft
//! (`saetze`, `sieht_zahl`, `hoechste_rundennummer`, die Weglisten) — und eine
//! Kopie prüft ihr eigenes Abbild, sobald das Original sich bewegt: sie blieb
//! rot, nachdem das Original schon stimmte, und hätte ebenso gut grün bleiben
//! können, während es falsch ist. Das ist dieselbe Scheintest-Klasse, gegen die
//! dieses Projekt seit der Runde 3 arbeitet, diesmal auf der Prüferseite — und
//! sie traf in der Runde 9 **drei** Belegdateien.
//!
//! Deshalb hängt jede Zusicherung hier an den Stellen des Quelltexts, ohne die
//! die Korrektur nicht mehr da wäre. Das ist kein Ersatz für einen Lauf: den
//! Lauf tun `belege`, `zi_e`, `zj_e` und `zl_e` an der echten Doku. Diese Datei
//! sorgt dafür, dass niemand die Korrektur still zurücknimmt.
//!
//! ```text
//! flock /tmp/redactrs-cargo.lock cargo test -p redact-cli --test zm_c_bindungsmaschine
//! ```

use std::path::{Path, PathBuf};

fn wurzel() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("Wurzel")
        .to_path_buf()
}

fn quelltext(datei: &str) -> String {
    std::fs::read_to_string(wurzel().join(datei))
        .unwrap_or_else(|e| panic!("{datei}: {e}"))
        .replace("\r\n", "\n")
}

/// Verlangt jede dieser Stellen im Quelltext — und sagt bei jeder, was ohne sie
/// wieder offen wäre.
fn verlangt(datei: &str, stellen: &[(&str, &str)]) {
    let quelle = quelltext(datei);
    let fehlend: Vec<String> = stellen
        .iter()
        .filter(|(stelle, _)| !quelle.contains(stelle))
        .map(|(stelle, folge)| format!("  fehlt in {datei}: {stelle}\n      → dann {folge}"))
        .collect();
    assert!(
        fehlend.is_empty(),
        "{} Stelle(n) der Korrektur sind aus {datei} verschwunden:\n{}",
        fehlend.len(),
        fehlend.join("\n")
    );
}

// ===========================================================================
// C1 — die Reihe der Zahlwörter und die Zusage in Fettschrift
// ===========================================================================

/// **Die Reihe reicht über „zwölf“ hinaus, und „null“/„eins“ gehören dazu.**
///
/// Die Liste endete bei „zwölf“ und hatte „siebzehn“ von Hand nachgetragen —
/// sie wuchs also dort, wo jemand hinsah. „Rückgabewert null“ ist dieselbe Art
/// Zusage wie „Rückgabewert 3“ zwei Absätze weiter, und sie stand ungebunden
/// da. „ein“/„eine“ fehlen weiter mit Absicht: das ist der unbestimmte Artikel.
#[test]
fn die_reihe_der_zahlwoerter_ist_vollstaendig() {
    verlangt(
        "crates/redact-cli/tests/belege.rs",
        &[
            ("\"null\",", "„Rückgabewert null“ ist wieder keine Zahl"),
            ("\"eins\",", "„eins“ ist wieder keine Zahl"),
            ("\"dreizehn\",", "die Reihe endet wieder mitten drin"),
            ("\"neunzehn\",", "die Reihe endet wieder mitten drin"),
            ("\"siebzig\",", "die Zehner fehlen wieder"),
            (
                "const WEITERE:",
                "„hundert“/„tausend“/„Dutzend“ fehlen wieder",
            ),
        ],
    );
}

/// **Ein Zahlwort in `**…**` ist eine Zusage, keine Floskel.**
///
/// Das schwerste Loch dieser Runde. Und die Regel muss zweierlei treffen: die
/// Auszeichnung wird über den **Block** gerechnet (ein Lauf kann mehrere Wörter
/// umfassen — „genau **einen** Namen“), und ein **langer** Lauf zählt nicht,
/// weil die Listenpunkte dieser Doku mit einer fetten Überschrift beginnen, in
/// der „Eine“ der Artikel ist.
#[test]
fn eine_zusage_in_fettschrift_ist_eine_zahl() {
    verlangt(
        "crates/redact-cli/tests/belege.rs",
        &[
            (
                "fn auszeichnung(text: &str) -> Vec<bool> {",
                "die Auszeichnung wird nicht mehr über den Block gerechnet, und \
                 „genau **einen** Namen“ fällt durch",
            ),
            (
                "const FETT_WORTE: usize = 3;",
                "eine fette Überschrift gilt wieder als Betonung, und jedes „Eine“ \
                 an ihrem Anfang verlangt eine Ausnahme",
            ),
            (
                "if ausgezeichnet {",
                "Ordnungszahlen und der Artikel zählen wieder nie — dann steht \
                 „als **erste** Anweisung“ wieder ungebunden da",
            ),
            (
                "const ORDNUNGSSTAEMME:",
                "„**erste**“ ist wieder keine Zahl",
            ),
            (
                "const ARTIKELZAHLEN:",
                "„**einen**“ und „**eine**“ sind wieder keine Zahl",
            ),
        ],
    );
}

/// **Die Endung zählt nur mit einem Zahlwort davor.**
///
/// Die Gegenrichtung: `ohne_mal.ends_with(w)` allein hielt „Jahrhundert“ und
/// „Jahrtausend“ für Zahlen. Das machte den Wächter strenger und nicht
/// schwächer — falsch war es trotzdem, und ein falscher Treffer verlangt
/// irgendwann eine Ausnahme, die dann etwas anderes mitdeckt.
#[test]
fn die_endung_zaehlt_nur_mit_einem_zahlwort_davor() {
    verlangt(
        "crates/redact-cli/tests/belege.rs",
        &[(
            "ZAHLWORTE.contains(&vorn) || WEITERE.contains(&vorn) || vorn == \"ein\"",
            "„Jahrhundert“ gilt wieder als Zahl",
        )],
    );
}

// ===========================================================================
// C2 — die Wegliste und die Satztrennung
// ===========================================================================

/// **Die Wegliste ist wieder so kurz, wie ein Lauf sie deckt.**
///
/// Die Runde 8 hatte sechs Merkmale hinzugefügt; **keines** kam im Block vor,
/// den dieser Wächter liest, und eines war falsch: eine Kommandozeile im Satz
/// zu **erwähnen** ist nicht dasselbe, wie die Zahl von dort zu haben. Der
/// Gegenbeweis stand in einem Satz, der `redact-rs --gui` nennt und im selben
/// Atemzug sagt, dass die Zahl nicht von dort kommt.
#[test]
fn die_wegliste_traegt_kein_totes_merkmal() {
    let quelle = quelltext("crates/redact-cli/tests/zj_e_etikett_der_messzahl.rs");
    for tot in [
        "\"`redact-rs ",
        "\"RUSAGE_CHILDREN\"",
        "\"VmHWM` des Kindprozesses\"",
        "\"Spitze des Kindprozesses\"",
        "\"_mess_\"",
        "\"::mess_\"",
    ] {
        assert!(
            !quelle.contains(tot),
            "das Merkmal {tot} ist wieder in `zj_e::weg` — es hat keinen Fall im \
             geprüften Block, und „`redact-rs “ macht aus einer an der Oberfläche \
             erhobenen Zahl eine Zahl des gebauten Binaries"
        );
    }
    verlangt(
        "crates/redact-cli/tests/zj_e_etikett_der_messzahl.rs",
        &[(
            "fn die_merkmale_des_weges_sind_einzeln_belegt()",
            "die Merkmale, die bleiben, stehen wieder ohne Lauf da",
        )],
    );
}

/// **Die Satztrennung schneidet hinter einer Ziffer.**
///
/// `davor_einzeln` prüfte nur, dass das Zeichen **zwei** vor dem Punkt nicht
/// alphanumerisch ist — bei „… Rückgabewert 3. Dieselbe Datei …“ traf das zu
/// (Ziffer, davor ein Leerzeichen), der Schnitt fiel aus, und zwei Sätze
/// verschmolzen. Ein Weg im ersten deckte dann eine Zahl im zweiten: derselbe
/// weglose Satz war nach „… endete mit einem Fehler.“ rot und nach „… endete
/// mit Rückgabewert 3." grün.
#[test]
fn die_satztrennung_verlangt_einen_buchstaben_vor_dem_punkt() {
    for datei in [
        "crates/redact-cli/tests/zj_e_etikett_der_messzahl.rs",
        "crates/redact-cli/tests/zl_e_gegenwartszahl_ohne_weg.rs",
    ] {
        let quelle = quelltext(datei);
        assert!(
            quelle.contains("bytes[i - 1].is_alphabetic() && !bytes[i - 2].is_alphanumeric()"),
            "{datei}: die Abkürzungsregel verlangt keinen Buchstaben mehr vor dem \
             Punkt — dann verschmelzen „… Rückgabewert 3.“ und der Satz danach"
        );
        assert!(
            !quelle.contains("let davor_ziffer"),
            "{datei}: die Ziffernregel ist wieder da — sie unterdrückte den Schnitt \
             an echten Satzenden, und `beginnt_neu` verlangt ohnehin einen \
             Großbuchstaben"
        );
    }
}

// ===========================================================================
// C3 — der Abschnittsschnitt und die Rundenzahl
// ===========================================================================

/// **Der Schnitt des Abschnitts überspringt eingezäunte Codeblöcke.**
///
/// `find("\n## ")` endete an der ersten solchen Zeile — auch mitten in einem
/// Codeblock. Weit genug hinten passierte das **still**: alle Tests blieben
/// grün, während der geprüfte Abschnitt um Hunderte Zeilen kürzer war. Ein
/// Wächter, der weniger liest, als er sagt, meldet nichts.
#[test]
fn der_abschnittsschnitt_ueberspringt_codebloecke() {
    verlangt(
        "crates/redact-cli/tests/zl_e_gegenwartszahl_ohne_weg.rs",
        &[
            (
                "gestutzt.starts_with(\"```\")",
                "eine ```-Zeile beendet den Schnitt wieder nicht, aber eine `## `-Zeile \
                 **in** einem Codeblock schon",
            ),
            (
                "} else if pos > von && !im_zaun && zeile.starts_with(\"## \") {",
                "der Schnitt endet wieder an der ersten `## `-Zeile, egal wo sie steht",
            ),
        ],
    );
}

/// **Die Rundenzahl wird gezählt, nicht maximiert.**
///
/// Die Runde 8 leitete sie als **höchste** Rundennummer ab. Das ergibt heute
/// dasselbe — aber nur, solange die Zählung lückenlos ist. Springt sie
/// (`10, 8, 7, 6`), sagt `max` zehn, wo sechs numerierte Überschriften stehen,
/// und verlangt vom Vorspann eine falsche Zahl. Gezählt werden die
/// Überschriften plus zwei: die Runden 1 und 2 sind älter als diese Benennung.
#[test]
fn die_rundenzahl_wird_gezaehlt() {
    let quelle = quelltext("crates/redact-cli/tests/zi_e_lage_der_messzahlen.rs");
    assert!(
        quelle.contains("let runden = text.matches(\"\\n### Fix-Runde \").count() + 2;"),
        "die Rundenzahl wird nicht mehr gezählt — steht dort wieder `max`, hält \
         sie eine Lücke in der Zählung nicht"
    );
    assert!(
        !quelle.contains(".max()"),
        "`max` ist wieder da: es ersetzt kein Zählen"
    );

    // Und die Zählung geht am echten CHANGELOG auf — sonst prüft dieser Test
    // eine Regel, die die Datei selbst nicht einhält.
    let text = quelltext("CHANGELOG.md");
    let numeriert = text.matches("\n### Fix-Runde ").count();
    let wort = [
        "", "Eine", "Zwei", "Drei", "Vier", "Fünf", "Sechs", "Sieben", "Acht", "Neun", "Zehn",
        "Elf", "Zwölf",
    ]
    .get(numeriert + 2)
    .copied()
    .unwrap_or_default();
    assert!(
        !wort.is_empty(),
        "{} Fix-Runden — für so viele gibt es hier kein Zahlwort",
        numeriert + 2
    );
    assert!(
        text.contains(&format!("{wort} Fix-Runden")),
        "der Vorspann sagt nicht „{wort} Fix-Runden“, obwohl {numeriert} numerierte \
         Überschriften dastehen"
    );
}
