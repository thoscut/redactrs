//! Gegenprüfung E (Nachbesserung der Fix-Runde 7): **das Etikett der
//! Messzahl** — nicht ob eine Zahl gebunden ist, sondern ob sie sagt, woher
//! sie kommt.
//!
//! `belege.rs` prüft mit `jede_zahl_der_letzten_runden_ist_gebunden`, dass
//! keine Zahl im Block der beiden letzten Fix-Runden *ungebunden* dasteht.
//! Gebunden heißt dort: der Wortlaut des Satzes wird aus einer Konstanten in
//! `messwerte` zusammengesetzt und muss so im CHANGELOG stehen. Das bindet
//! den **Wert**. Es bindet nicht die **Herkunft**.
//!
//! Die Nachbesserung dieser Runde hat in den Vorspann des CHANGELOG eine
//! Regel geschrieben, die genau über die Herkunft spricht:
//!
//! > * Eine Messzahl stammt aus einem Lauf des **gebauten Binaries**
//! >   (`target/release/redact-rs`) …
//! > * Zeigt die Kommandozeile die Größe nicht, … dann sagt **der Satz
//! >   selbst**, dass im Testprozess gemessen wurde, und nennt das Profil.
//! >   Ein zweiter Weg, kein Schlupfloch: still eine Debug-Zahl als
//! >   Binary-Zahl auszugeben ist keiner.
//! > * Eine Zahl, die den Zustand **vor** einer Korrektur beziffert, … sagt
//! >   deshalb, dass sie den alten Zustand beschreibt, und nennt den Stand …
//!
//! Diese Regel prüft kein Test. Dieser hier tut es: er nimmt denselben Block
//! wie `belege.rs`, sucht jeden Satz, der eine **Zeit- oder Speicherzahl**
//! trägt, und verlangt, dass derselbe Satz einen der drei Wege nennt.
//!
//! Der Maßstab ist dabei absichtlich großzügig — es genügt *irgendein*
//! Merkmal des Weges im selben Satz. Was hier auffällt, fällt deshalb nicht
//! an einer engen Lesart auf.
//!
//! Eng ist er an **einer** Stelle, und zwar an der, an der die Großzügigkeit
//! die Regel aufgehoben hätte: ein Profil („Release“) ist kein Ort. Siehe
//! [`weg`].
//!
//! **Das Fenster: die beiden jüngsten Fix-Runden, und das mit Grund.**
//! `zl_e_gegenwartszahl_ohne_weg` liest inzwischen den ganzen Abschnitt
//! `## Unveröffentlicht` — es fragt aber nur nach Sätzen, die von **heute**
//! sprechen, und eine Gegenwartszahl muss heute messbar sein. Dieser Test
//! fragt nach *jeder* Zeit- und Speicherzahl, auch nach denen der Runden 3
//! bis 6. Über den ganzen Abschnitt gelesen melden 26 Sätze keinen Weg —
//! gemessen in der Fix-Runde 8 mit genau diesem Schnitt. Das ist ein Befund
//! und keine Nachlässigkeit dieses Tests: die Läufe hinter jenen Zahlen
//! liegen Runden zurück, mancher Stand ist nicht mehr auszuchecken, und die
//! Regel des Vorspanns ist jünger als sie. Er steht im Aufgabenregister und
//! nicht in einer stillen Ausnahme; wer ihn abarbeitet, weitet hier den
//! Schnitt auf den Abschnitt und nimmt die 26 Sätze der Reihe nach.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("Wurzel")
        .to_path_buf()
}

fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Zeilenumbrüche und Einrückung glätten — wie `belege::glatt`.
fn glatt(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Der Block der beiden letzten Fix-Runden — derselbe Schnitt wie in
/// `belege::changelog_block`.
fn changelog_block() -> String {
    let text = lf(&std::fs::read_to_string(repo_root().join("CHANGELOG.md")).expect("CHANGELOG"));
    let ueberschriften: Vec<usize> = text
        .match_indices("\n### Fix-Runde ")
        .map(|(i, _)| i)
        .collect();
    assert!(
        ueberschriften.len() >= 3,
        "weniger als drei Fix-Runden im CHANGELOG"
    );
    text[ueberschriften[0]..ueberschriften[2]].to_string()
}

/// Ein Absatz des Blocks, geglättet: Listenpunkte und Absätze einzeln.
fn absaetze(block: &str) -> Vec<String> {
    block
        .split("\n\n")
        .map(glatt)
        .filter(|a| !a.is_empty())
        .collect()
}

/// Die Sätze eines Absatzes. Getrennt wird an „. “, aber nur, wenn danach
/// wirklich ein neuer Satz beginnt (Großbuchstabe, Anführung, Auszeichnung) —
/// sonst zerfällt `0.6.0` oder `z. B.` in Stücke.
fn saetze(absatz: &str) -> Vec<String> {
    let bytes: Vec<char> = absatz.chars().collect();
    let mut aus = Vec::new();
    let mut anfang = 0usize;
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == '.' && bytes[i + 1] == ' ' {
            let beginnt_neu = bytes
                .get(i + 2)
                .is_some_and(|c| c.is_uppercase() || "„»*`(".contains(*c));
            // Kein Schnitt hinter einer Ziffer (`0. 6`) und keiner hinter
            // einem einzelnen Buchstaben (`z. B.`).
            let davor_ziffer = i > 0 && bytes[i - 1].is_ascii_digit();
            let davor_einzeln = i >= 2 && !bytes[i - 2].is_alphanumeric();
            if beginnt_neu && !davor_ziffer && !davor_einzeln {
                aus.push(bytes[anfang..=i].iter().collect::<String>());
                anfang = i + 2;
            }
        }
        i += 1;
    }
    if anfang < bytes.len() {
        aus.push(bytes[anfang..].iter().collect::<String>());
    }
    aus.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Trägt dieser Satz eine **Zeit- oder Speicherzahl**? Also die Art Zahl, die
/// überhaupt von einem Profil und einem Ort abhängt.
///
/// Gesucht wird eine Ziffernfolge (mit deutschem Dezimalkomma und schmalem
/// Trenner) unmittelbar vor einer Einheit.
fn traegt_mess_groesse(satz: &str) -> Vec<String> {
    const EINHEITEN: [&str; 8] = [" s", " ms", " MB", " kB", " GB", " GiB", " MiB", " GB/s"];
    let zeichen: Vec<char> = satz.chars().collect();
    let mut treffer = Vec::new();
    for (i, c) in zeichen.iter().enumerate() {
        if !c.is_ascii_digit() {
            continue;
        }
        // Ende der Zahl suchen: Ziffern, Komma, Trennzeichen.
        let mut j = i;
        while j < zeichen.len()
            && (zeichen[j].is_ascii_digit() || ",. \u{202f}\u{a0}".contains(zeichen[j]))
        {
            // Ein Leerzeichen zählt nur als Tausendertrenner, wenn danach
            // wieder eine Ziffer kommt.
            if " \u{202f}\u{a0}".contains(zeichen[j])
                && !zeichen.get(j + 1).is_some_and(|c| c.is_ascii_digit())
            {
                break;
            }
            j += 1;
        }
        // Der Anfang muss der Anfang der Zahl sein.
        if i > 0 && (zeichen[i - 1].is_ascii_digit() || zeichen[i - 1] == ',') {
            continue;
        }
        let rest: String = zeichen[j..].iter().collect();
        for e in EINHEITEN {
            let passt = rest.starts_with(e)
                && rest[e.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric());
            if passt {
                let zahl: String = zeichen[i..j].iter().collect();
                treffer.push(format!("{zahl}{e}"));
                break;
            }
        }
    }
    treffer
}

/// Welchen der drei erlaubten Wege nennt dieser Satz — wenn überhaupt einen?
///
/// Großzügig gelesen: **ein** Merkmal genügt. Zwei Stellen sind trotzdem eng,
/// weil hier sonst genau das durchgeht, was die Regel verbietet:
///
/// * **Ein Profil ist kein Ort.** „(Release, …)“ stand einmal in der
///   Binary-Liste. Ein im Testprozess erhobener Wert, der nur sein Profil
///   nennt, hätte damit als Zahl des gebauten Binaries gegolten — das stille
///   Umetikett, das der Vorspann ausdrücklich ausschließt („still eine
///   Debug-Zahl als Binary-Zahl auszugeben ist keiner“). `Release` zählt
///   deshalb nur noch als **Profil** beim zweiten Weg.
/// * **Der Testprozess wird zuerst gefragt.** Nennt ein Satz ihn, ist er im
///   Testprozess gemessen, und dann entscheidet allein, ob auch das Profil
///   dasteht. Stünde die Binary-Frage davor, machte ein „am Binary“ in einem
///   Nebensatz die Prüfung des Profils überflüssig.
fn weg(satz: &str) -> Option<&'static str> {
    const BINARY: [&str; 8] = [
        "gebauten Binary",
        "gebauten Binaries",
        "target/release/redact-rs",
        "am Binary",
        // Eine Kommandozeile ist ein Lauf des Binaries, und die Spitze eines
        // KINDprozesses kann nur ein Kindprozess geliefert haben.
        "`redact-rs ",
        "RUSAGE_CHILDREN",
        "VmHWM` des Kindprozesses",
        "Spitze des Kindprozesses",
    ];
    // Ein benannter Messtest ist ein aufgezeichneter Lauf **im Testprozess** —
    // das Profil verlangt der zweite Weg zusätzlich, wie bei jedem anderen
    // Merkmal dieser Liste.
    const TESTPROZESS: [&str; 5] = [
        "Testprozess",
        "im Testlauf",
        "eigener Prozess",
        "_mess_",
        "::mess_",
    ];
    const ALTSTAND: [&str; 6] = [
        "Stand `",
        "ungedeckelt",
        "vor dieser Änderung",
        "vorher",
        "widerlegt",
        "damals",
    ];

    if TESTPROZESS.iter().any(|m| satz.contains(m)) {
        // Der zweite Weg verlangt zusätzlich das Profil.
        if satz.contains("Debug") || satz.contains("Release") {
            return Some("Testprozess + Profil");
        }
        return None;
    }
    if BINARY.iter().any(|m| satz.contains(m)) {
        return Some("Binary");
    }
    if ALTSTAND.iter().any(|m| satz.contains(m)) {
        return Some("alter Stand");
    }
    None
}

/// **Die Regel des Vorspanns, angewandt.** Jeder Satz des Blocks, der eine
/// Zeit- oder Speicherzahl trägt, nennt seinen Weg.
#[test]
fn jede_zeit_und_speicherzahl_nennt_ihren_weg() {
    let block = changelog_block();
    let mut ohne_weg: Vec<String> = Vec::new();
    let mut mit_weg = 0usize;

    for absatz in absaetze(&block) {
        for satz in saetze(&absatz) {
            let groessen = traegt_mess_groesse(&satz);
            if groessen.is_empty() {
                continue;
            }
            match weg(&satz) {
                Some(_) => mit_weg += 1,
                None => ohne_weg.push(format!("[{}] {satz}", groessen.join(", "))),
            }
        }
    }

    assert!(
        mit_weg + ohne_weg.len() >= 8,
        "nur {} Satz/Sätze mit einer Zeit- oder Speicherzahl gefunden — der \
         Schnitt oder die Satztrennung greift nicht mehr",
        mit_weg + ohne_weg.len()
    );

    assert!(
        ohne_weg.is_empty(),
        "{} Satz/Sätze im Block tragen eine Zeit- oder Speicherzahl, ohne zu \
         sagen, woher sie kommt — weder ein Lauf am gebauten Binary noch \
         „Testprozess“ mit Profil noch ein alter Stand steht im Satz. Der \
         Vorspann dieses Abschnitts verspricht genau das für jede Messzahl:\n\n{}\n\n\
         ({mit_weg} Satz/Sätze nennen ihren Weg.)",
        ohne_weg.len(),
        ohne_weg.join("\n\n")
    );
}

/// **Die Gegenprobe zum Vorspann selbst.** Die Regel steht nicht nur als
/// Prosa da: der Satz, der sie trägt, sagt auch, dass ein stilles Umetikett
/// kein erlaubter Weg ist. Bricht diese Formulierung weg, prüft der Test
/// darüber eine Regel, die niemand mehr aufgestellt hat.
#[test]
fn der_vorspann_stellt_die_regel_wirklich_auf() {
    let text = lf(&std::fs::read_to_string(repo_root().join("CHANGELOG.md")).expect("CHANGELOG"));
    let vorspann = glatt(&text[..text.find("\n### Fix-Runde ").expect("erste Fix-Runde")]);
    for satzteil in [
        "Eine Messzahl stammt aus einem Lauf des **gebauten Binaries**",
        "dann sagt **der Satz selbst**, dass im Testprozess gemessen wurde, und nennt das Profil",
        "still eine Debug-Zahl als Binary-Zahl auszugeben ist keiner",
        "Wo eine Zahl keinen dieser Wege geht, gehört sie gestrichen und nicht verschoben",
    ] {
        assert!(
            vorspann.contains(satzteil),
            "der Vorspann trägt „{satzteil}“ nicht mehr — die Regel, gegen die \
             `jede_zeit_und_speicherzahl_nennt_ihren_weg` prüft, steht dann nirgends"
        );
    }
}
