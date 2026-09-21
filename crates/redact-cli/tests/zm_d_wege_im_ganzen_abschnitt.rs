//! Gegenprüfung D der Runde 9: **was der Abschnitt über seine eigene Regel
//! verschweigt.**
//!
//! Der Vorspann des Abschnitts `## Unveröffentlicht` stellt die Regel für
//! **jede** Messzahl auf und schließt mit: „Wo eine Zahl keinen dieser Wege
//! geht, gehört sie gestrichen und nicht verschoben." Geprüft wird sie von
//! `zj_e_etikett_der_messzahl` aber nur über die **beiden jüngsten**
//! Fix-Runden.
//!
//! Dieser Test nimmt denselben Schnitt der Sätze, dieselbe Messgrößen-Suche
//! und dasselbe `weg()` (wörtlich aus `zj_e` übernommen, damit kein
//! Unterschied aus der Maschinerie kommt) und legt sie über den **ganzen**
//! Abschnitt. Ergebnis: es bleiben Sätze ohne Weg übrig — Register #53, in
//! `zj_e`s Modulkopf mit der Zahl **26** benannt und im Register geführt.
//!
//! Der CHANGELOG-Abschnitt selbst sagt davon nichts. Sein Vorspann sagt nur,
//! dass „jede Zahl der beiden jüngsten Abschnitte" gebunden sei — die Regel
//! darüber gilt dem Wortlaut nach für alles, was darunter steht.
//!
//! Dieser Test ist ein **Wächter über einen benannten Rest**: er hält die
//! Zahl fest und wird rot, sobald sie sich ändert — in beide Richtungen.
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-cli --test zm_d_wege_im_ganzen_abschnitt`

use std::path::PathBuf;

/// So viele Sätze des ganzen Abschnitts nennen keinen Weg — die Zahl, die
/// `zj_e`s Modulkopf als Befund der Fix-Runde 8 nennt.
// Die Fix-Runde 9 hat einen davon geschlossen: sie hat die Merkmalsliste von
// `weg()` um die sechs Einträge gekürzt, die keinen Fall im Block hatten, und
// dabei eines gestrichen, das falsch war (eine Kommandozeile im Satz zu
// erwähnen ist nicht dasselbe, wie die Zahl von dort zu haben). Der Rest ist
// damit 25 — nachgemessen mit genau diesem Test, nicht abgeschätzt.
const BENANNTER_REST: usize = 25;

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

fn glatt(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Der **ganze** Abschnitt `## Unveröffentlicht` — der Geltungsbereich der
/// Regel, wie ihr Vorspann ihn aufspannt (Schnitt wörtlich aus
/// `zl_e_gegenwartszahl_ohne_weg`).
fn ganzer_abschnitt() -> String {
    let text = lf(&std::fs::read_to_string(repo_root().join("CHANGELOG.md")).expect("CHANGELOG"));
    let marke = "\n## Unveröffentlicht";
    let von = text.find(marke).expect("der Abschnitt") + 1;
    let bis = text[von + marke.len()..]
        .find("\n## ")
        .map(|i| von + marke.len() + i)
        .unwrap_or(text.len());
    assert!(
        text[von..bis].contains("\n### Fix-Runde 8"),
        "der Schnitt greift nicht mehr"
    );
    text[von..bis].to_string()
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
            // Kein Schnitt hinter einer Abkürzung aus **einem Buchstaben**
            // („z. B.“, „d. h.“, „u. a.“).
            //
            // Hier stand zusätzlich „keiner hinter einer Ziffer" und die
            // Abkürzungsfrage ohne die Prüfung, ob davor ein BUCHSTABE steht.
            // Damit fiel der Schnitt auch hinter „… beide mit Rückgabewert 3.
            // Dieselbe Datei hat neun Objekte …" aus: zwei Sätze verschmolzen,
            // und ein Weg, der im ersten stand, deckte eine Zahl im zweiten.
            // Sieben solche Stellen hatte der Block. Die Ziffernregel ist
            // ersatzlos weg — `beginnt_neu` verlangt ohnehin einen
            // Großbuchstaben, eine Anführung oder eine Auszeichnung, und
            // `0.6.0` trägt gar kein „. ".
            let davor_abkuerzung =
                i >= 2 && bytes[i - 1].is_alphabetic() && !bytes[i - 2].is_alphanumeric();
            if beginnt_neu && !davor_abkuerzung {
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
    const BINARY: [&str; 7] = [
        "gebauten Binary",
        "gebauten Binaries",
        "target/release/redact-rs",
        "am Binary",
        // Die Spitze eines **Kind**prozesses kann nur ein Kindprozess geliefert
        // haben, und der Kindprozess dieses Projekts ist das gebaute Binary.
        "RUSAGE_CHILDREN",
        "VmHWM` des Kindprozesses",
        "Spitze des Kindprozesses",
    ];
    // Hier stand auch „`redact-rs " — eine Kommandozeile im Satz. Das war
    // unsolide und ist gestrichen: eine Kommandozeile zu **erwähnen** ist nicht
    // dasselbe, wie die Zahl von dort zu haben. Der Gegenbeweis steht als Probe
    // in [`die_merkmale_des_weges_sind_einzeln_belegt`].
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

/// **Der benannte Rest, gezählt.** Über den ganzen Abschnitt gelesen bleiben
/// Sätze mit Zeit- oder Speicherzahl übrig, die keinen der drei Wege nennen.
#[test]
fn ueber_den_ganzen_abschnitt_bleiben_saetze_ohne_weg() {
    let block = ganzer_abschnitt();
    let mut ohne: Vec<String> = Vec::new();
    let mut mit = 0usize;
    for absatz in absaetze(&block) {
        for satz in saetze(&absatz) {
            let groessen = traegt_mess_groesse(&satz);
            if groessen.is_empty() {
                continue;
            }
            match weg(&satz) {
                Some(_) => mit += 1,
                None => ohne.push(format!("[{}] {satz}", groessen.join(", "))),
            }
        }
    }
    eprintln!("Sätze mit Weg: {mit}; ohne Weg: {}", ohne.len());
    for satz in &ohne {
        eprintln!("  OHNE WEG: {}", satz.chars().take(160).collect::<String>());
    }
    assert!(
        mit > 10,
        "die Maschinerie greift nicht mehr — nur {mit} Sätze mit Weg"
    );
    assert_eq!(
        ohne.len(),
        BENANNTER_REST,
        "der benannte Rest (Register #53) hat sich geändert: {} statt {BENANNTER_REST}",
        ohne.len()
    );
}
