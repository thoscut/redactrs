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
    // 1 800 Zeichen, nicht 900: der Rumpf von `changelog_block` traegt seit
    // dieser Runde die Begruendung des Schnitts als Kommentar, und die
    // entscheidende Zeile stand sonst jenseits des Fensters — der Nachbau haette
    // eine geschlossene Luecke fuer offen gehalten.
    let rumpf = &quelle[ab..ab + 1_800.min(quelle.len() - ab)];
    // Lücke 1 IST geschlossen: `changelog_block` schneidet seit dieser Runde ab
    // `## Unveröffentlicht`, nicht mehr ab der ersten `### Fix-Runde`. Der
    // Nachbau hier liest das aus dem Quelltext, statt es anzunehmen — fällt das
    // Original zurück, wird diese Zusicherung rot.
    assert!(
        rumpf.contains("glatt(&text[anfang..ueberschriften[2]])")
            && rumpf.contains("## Unveröffentlicht"),
        "`changelog_block` schneidet nicht mehr ab `## Unveröffentlicht` — die \
         Lücke, die diese Datei belegt hat, ist wieder offen"
    );

    let ueberschriften: Vec<usize> = text
        .match_indices("\n### Fix-Runde ")
        .map(|(i, _)| i)
        .collect();
    assert!(
        ueberschriften.len() >= 3,
        "weniger als drei Fix-Runden im CHANGELOG"
    );
    let anfang = text
        .find("\n## Unveröffentlicht")
        .map_or(ueberschriften[0], |i| i + 1);
    (anfang, ueberschriften[2])
}

/// Der Vorspann des Abschnitts `## Unveröffentlicht`: von seiner Überschrift
/// bis zur ersten `### Fix-Runde`.
fn vorspann(text: &str) -> (usize, usize) {
    // Dieselbe Kante wie der Schnitt in `belege::changelog_block`: HINTER dem
    // Zeilenumbruch. Ein Unterschied von einem Byte liesse den Vergleich unten
    // scheitern, ohne dass an der Sache etwas faul waere.
    let von = text
        .find("\n## Unveröffentlicht")
        .map(|i| i + 1)
        .expect("Abschnitt `## Unveröffentlicht` steht im CHANGELOG");
    let bis = text[von..]
        .find("\n### Fix-Runde ")
        .map(|i| von + i)
        .expect("nach dem Vorspann kommt eine Fix-Runde");
    (von, bis)
}

/// Die Wortliste aus `belege::traegt_zahl` — **gelesen**, nicht abgeschrieben:
/// eine zweite Abschrift würde still auseinanderlaufen.
fn zahlworte_des_waechters() -> (Vec<String>, Vec<String>) {
    let text = belege_quelltext();
    let ab = text
        .find("fn traegt_zahl(")
        .expect("`fn traegt_zahl` steht in belege.rs");
    let eine_liste = |name: &str| -> Vec<String> {
        let liste = text[ab..]
            .find(name)
            .map(|i| ab + i)
            .unwrap_or_else(|| panic!("`{name}` steht in `traegt_zahl`"));
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
        aus
    };
    let reihe = eine_liste("const ZAHLWORTE: [&str;");
    let weitere = eine_liste("const WEITERE: [&str;");
    assert!(
        reihe.len() >= 20,
        "die Wortliste des Wächters ist geschrumpft — {} Wörter",
        reihe.len()
    );
    (reihe, weitere)
}

/// Das Kriterium des Wächters, aus seinen **gelesenen** Listen nachgebaut:
/// Ziffer, Wort aus der Reihe (auch als `…mal`-Form), oder ein Wort, das auf
/// eines der Wörter der zweiten Liste endet („zweihundert“).
fn sieht_zahl(reihe: &[String], weitere: &[String], wort: &str) -> bool {
    let kern: String = wort
        .chars()
        .filter(|c| !"*`„“»«().,;:—–!?[]…\u{202f}".contains(*c))
        .collect();
    if kern.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    let klein = kern.to_lowercase();
    let ohne_mal = klein.strip_suffix("mal").unwrap_or(&klein);
    reihe.iter().any(|w| w == ohne_mal)
        || weitere.iter().any(|w| w == ohne_mal)
        || weitere
            .iter()
            .any(|w| ohne_mal.len() > w.len() && ohne_mal.ends_with(w.as_str()))
}

// ---------------------------------------------------------------------------
// Lücke 1 — GESCHLOSSEN: der Vorspann der Release-Notizen wird mitgeprüft
// ---------------------------------------------------------------------------

/// **Der Vorspann liegt IM geprüften Block.** Vorher lag er davor.
///
/// Diese Datei hat die Lücke belegt: `changelog_block` schnitt ab der ersten
/// `### Fix-Runde`, und der Vorspann von `## Unveröffentlicht` blieb außen —
/// obwohl dort die Zahlen stehen, die den ganzen Abschnitt zusammenfassen
/// („sieben Fix-Runden“, „vier stille Lecks“). Eine Zahl, die eine
/// Zusammenfassung trägt, ist so viel eine Zusage wie eine im Text.
///
/// Der Schnitt beginnt jetzt am Abschnitt. Der Test steht weiter hier, nur mit
/// gedrehter Aussage: er hält die Lücke ZU. Fällt der Schnitt zurück, wird
/// `schnitt_wie_belege` rot (es liest den Quelltext von `belege.rs`), und wenn
/// jemand den Vorspann verschiebt, wird diese Zusicherung rot.
#[test]
fn der_vorspann_der_release_notizen_liegt_im_geprueften_block() {
    let text = changelog();
    let (vor_von, vor_bis) = vorspann(&text);
    let (block_von, block_bis) = schnitt_wie_belege(&text);

    assert!(
        block_von <= vor_von && vor_bis <= block_bis,
        "der Vorspann ({vor_von}..{vor_bis}) liegt nicht im geprüften Block \
         ({block_von}..{block_bis}) — die Lücke ist wieder offen"
    );

    let vor = glatt(&text[vor_von..vor_bis]);
    assert!(
        vor.chars().any(|c| c.is_ascii_digit()),
        "der Vorspann trägt keine Ziffer mehr — dann prüft dieser Test nichts: {vor}"
    );
    // **Abgeleitet, nicht abgeschrieben.** Hier stand „Sieben Fix-Runden“ als
    // Literal — und wurde in der Runde 8 rot, weil der Vorspann richtig
    // weiterzählte. Ein Literal prüft, dass sich nichts ändert; gefragt ist,
    // dass der Vorspann **mitzählt**.
    //
    // Gezählt wird nicht die Zahl der Überschriften, sondern die **höchste
    // Rundennummer**: die Runden 1 und 2 sind älter als diese Benennung und
    // stehen unter eigenen Überschriften. Sechs Überschriften, acht Runden —
    // wer die Überschriften zählt, zählt am Vorspann vorbei.
    let runden = text
        .match_indices("\n### Fix-Runde ")
        .filter_map(|(i, m)| {
            text[i + m.len()..]
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .and_then(|z| z.parse::<usize>().ok())
        })
        .max()
        .expect("mindestens eine numerierte Fix-Runde");
    let wort = [
        "", "Eine", "Zwei", "Drei", "Vier", "Fünf", "Sechs", "Sieben", "Acht", "Neun", "Zehn",
        "Elf", "Zwölf",
    ]
    .get(runden)
    .copied()
    .unwrap_or_default();
    assert!(
        !wort.is_empty(),
        "{runden} Fix-Runden — für so viele hat dieser Test kein Zahlwort"
    );
    assert!(
        vor.contains(&format!("{wort} Fix-Runden")),
        "der Vorspann zählt die Runden nicht mit: die Datei hat {runden} \
         `### Fix-Runde`-Überschriften, der Vorspann sagt nicht „{wort} \
         Fix-Runden“: {vor}"
    );

    // Und die pauschale Zusage, die hier stand, ist weg: sie war falsch (alle
    // drei Messzahlen der Runde 7 stammten aus einem Debug-Testprozess). An
    // ihrer Stelle steht ein Maßstab, der sagt, woher eine Zahl kommen muss.
    // Nicht am Vorkommen, sondern an der AUSSAGE prüfen: der Vorspann ZITIERT
    // die alte pauschale Zusage, um zu sagen, dass sie keine war. Ein
    // `!contains` schlug hier auf das Zitat an — dieselbe Fehlerklasse, an der
    // in dieser Schleife schon die Plattformregel hing (sie las ihr eigenes
    // Zitat als Verwendung). Geprüft wird deshalb, dass die Widerlegung
    // dabeisteht und dass der Maßstab, der an ihre Stelle getreten ist, dasteht.
    if let Some(i) = vor.find("jede Angabe hier stammt aus einem Lauf des gebauten Binaries") {
        let danach = &vor[i..];
        assert!(
            danach.contains("war keine"),
            "die alte pauschale Zusage steht da, ohne widerlegt zu werden: {danach}"
        );
    }
    assert!(
        vor.contains("Eine Messzahl stammt aus einem Lauf des **gebauten Binaries**"),
        "der Maßstab für eine Messzahl fehlt im Vorspann: {vor}"
    );
}

/// **Eine erfundene Messzahl im Vorspann landet jetzt im Block.**
///
/// Die Gegenprobe zur Lage: vorher entging eine Zahl im Vorspann der Prüfung
/// vollständig. Der Test baut sie in eine Kopie des Textes ein und verlangt,
/// dass der Schnitt sie erfasst — damit `jede_zahl_der_letzten_runden_ist_gebunden`
/// sie sehen KANN.
#[test]
fn eine_erfundene_messzahl_im_vorspann_liegt_im_block() {
    let text = changelog();
    let (vor_von, vor_bis) = vorspann(&text);
    let erfunden = "Dieser Satz nennt 4 711 MB und ist an nichts gebunden.";

    let mut gefaelscht = String::with_capacity(text.len() + erfunden.len() + 2);
    gefaelscht.push_str(&text[..vor_bis]);
    gefaelscht.push('\n');
    gefaelscht.push_str(erfunden);
    gefaelscht.push('\n');
    gefaelscht.push_str(&text[vor_bis..]);

    let (block_von, block_bis) = schnitt_wie_belege(&gefaelscht);
    let block = glatt(&gefaelscht[block_von..block_bis]);
    assert!(
        block.contains("4 711 MB"),
        "die erfundene Zahl liegt außerhalb des geprüften Blocks — dann sieht der \
         Wächter sie nicht"
    );
    assert!(
        block_von <= vor_von,
        "der Block beginnt hinter dem Vorspann"
    );
}

// ---------------------------------------------------------------------------
// Lücke 2 — GESCHLOSSEN: die Reihe reicht jetzt über „zwölf“ hinaus
// ---------------------------------------------------------------------------

/// **Die Lücke, und was an ihre Stelle getreten ist.** Die Wortliste des
/// Wächters endete bei „zwölf“ und hatte „siebzehn“ von Hand nachgetragen — sie
/// wuchs also genau dort, wo jemand hingesehen hatte. Fünf ausgeschriebene
/// Mengen, die eine Runde 8 ohne Weiteres schreiben würde, waren für ihn keine
/// Zahlen: „dreizehn“, „zwanzig“, „hundert“, „tausend“, „Dutzend“. Und der
/// Wächter ist die **Gegenrichtung** zur Bindung: er behauptet, im Block der
/// beiden letzten Fix-Runden stehe keine unbedeckte Zahl. Was er nicht als
/// Zahl erkennt, meldet er nicht — die Lücke war also nicht, dass er zu viel
/// meldet, sondern dass er stillschweigend zu wenig prüft.
///
/// Seit dieser Runde steht die Reihe vollständig da — bis „neunzehn“, die
/// Zehner, „hundert“, „tausend“, „Dutzend“ —, und die `…mal`-Formen entstehen
/// aus derselben Reihe statt aus einer zweiten Liste. Dieser Test hält das
/// fest, und zwar am **Kriterium**, nicht an der Liste: gelesen werden beide
/// Listen aus `belege.rs`, nachgebaut wird der Vergleich.
///
/// Mutation, die ihn rot macht: in `traegt_zahl` die Reihe wieder bei „zwölf“
/// enden lassen (oder `WEITERE` leeren).
#[test]
fn die_reihe_des_waechters_reicht_ueber_zwoelf_hinaus() {
    let (reihe, weitere) = zahlworte_des_waechters();
    let sieht = |w: &str| sieht_zahl(&reihe, &weitere, w);

    // Was er kennen muss — sonst prüfte dieser Test die falsche Liste.
    for bekannt in ["zwei", "zwölf", "siebzehn", "siebenmal"] {
        assert!(
            sieht(bekannt),
            "„{bekannt}“ sieht der Wächter nicht — das ist nicht das Kriterium \
             aus `traegt_zahl`"
        );
    }

    // Und was die Lücke war.
    let blind: Vec<&str> = ["dreizehn", "zwanzig", "hundert", "tausend", "Dutzend"]
        .into_iter()
        .filter(|w| !sieht(w))
        .collect();
    assert!(
        blind.is_empty(),
        "der Wächter sieht diese ausgeschriebenen Mengen weiter nicht: {blind:?} — \
         dann steht im Block der beiden letzten Fix-Runden womöglich eine \
         unbedeckte Zahl, und `jede_zahl_der_letzten_runden_ist_gebunden` sagt \
         trotzdem Ja"
    );

    // Derselbe Satz, an dem die Lücke vorgeführt wurde: jedes seiner fünf
    // Mengenwörter ist jetzt eine Zahl.
    let satz = "Der Redaktor braucht dafür nur noch zwanzig Millisekunden statt \
                dreizehn Sekunden, und die Spitze liegt bei einem Dutzend MB statt \
                bei tausend.";
    let gesehen: Vec<&str> = satz.split_whitespace().filter(|w| sieht(w)).collect();
    assert_eq!(
        gesehen,
        vec!["zwanzig", "dreizehn", "Dutzend", "tausend."],
        "genau die Mengenwörter dieses Satzes, keines mehr und keines weniger"
    );
}
