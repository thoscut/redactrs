//! Gegenprüfung der Nachbesserung E (Pendel): **die Zahl, die von heute
//! spricht.**
//!
//! `zj_e_etikett_der_messzahl::jede_zeit_und_speicherzahl_nennt_ihren_weg`
//! verlangt für jeden Satz mit einer Zeit- oder Speicherzahl *einen* der drei
//! Wege des Vorspanns. Sein `weg()` liest den dritten Weg („alter Zustand“)
//! aber an sechs losen Wörtern ab — `vorher`, `widerlegt`, `damals`,
//! `ungedeckelt`, `vor dieser Änderung`, ``Stand ` ``. Damit genügt **ein**
//! altes Wort, um einen ganzen Satz freizugeben, auch wenn in demselben Satz
//! Zahlen stehen, die den Zustand von **heute** beziffern.
//!
//! Der Vorspann des CHANGELOG lässt das nicht zu. Sein dritter Weg gilt
//! ausdrücklich nur für Zahlen, die den Zustand **vor** einer Korrektur
//! beziffern („Eine Zahl, die den Zustand **vor** einer Korrektur beziffert,
//! ist danach nicht mehr messbar“). Eine Zahl, die den Zustand von heute
//! beziffert, ist messbar — für sie bleiben nur Weg 1 (Lauf des gebauten
//! Binaries) und Weg 2 (Testprozess **mit Profil**).
//!
//! Dieser Test nimmt denselben Schnitt, dieselbe Satztrennung und dieselben
//! Weg-Merkmale wie `zj_e` (wörtlich kopiert, damit kein Unterschied aus der
//! Maschinerie kommt) und schränkt nur zweierlei ein:
//!
//! * er sieht **nur Zeitzahlen** an (`s`, `ms`, `GB/s`) — eine Dateigröße oder
//!   eine eingestellte Grenze in kB/MB ist keine Messung und hängt an keinem
//!   Profil, sie darf ohne Weg dastehen (siehe
//!   [`der_massstab_beanstandet_gewoehnliche_prosa_nicht`]);
//! * er sieht nur Sätze an, die das Wort **„heute“** führen, also selbst
//!   sagen, dass sie vom jetzigen Baum sprechen.
//!
//! Enger als das geht der Maßstab nicht, und schmaler kann ein Fehlalarm nicht
//! werden: drei Sätze des Blocks führen „heute“ und eine Zeitzahl, zwei nennen
//! ihren Weg, einer nicht.

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

fn glatt(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Der **ganze** Abschnitt `## Unveröffentlicht`.
///
/// Hier stand der Schnitt der beiden letzten Fix-Runden, wie in
/// `belege::changelog_block` und `zj_e::changelog_block`. Der hing am Fenster:
/// kam eine Runde hinzu, fiel die drittletzte heraus — und mit ihr der Satz, an
/// dem dieser Test hing. Die Wächterzahl fiel auf 0, und der Test meldete
/// richtig „der Schnitt greift nicht mehr“. Behoben ist das am richtigen Ort:
/// die Regel, gegen die hier geprüft wird, steht im **Vorspann dieses
/// Abschnitts** und gilt für den ganzen Abschnitt. Also wird der Abschnitt
/// gelesen und nicht sein jüngstes Fenster.
fn changelog_block() -> String {
    let text = lf(&std::fs::read_to_string(repo_root().join("CHANGELOG.md")).expect("CHANGELOG"));
    let marke = "\n## Unveröffentlicht";
    let von = text
        .find(marke)
        .expect("der Abschnitt `## Unveröffentlicht`")
        + 1;
    let bis = text[von + marke.len()..]
        .find("\n## ")
        .map(|i| von + marke.len() + i)
        .unwrap_or(text.len());
    assert!(
        text[von..bis].contains("\n### Fix-Runde "),
        "der Abschnitt enthält keine numerierte Fix-Runde — der Schnitt greift nicht"
    );
    text[von..bis].to_string()
}

fn absaetze(block: &str) -> Vec<String> {
    block
        .split("\n\n")
        .map(glatt)
        .filter(|a| !a.is_empty())
        .collect()
}

/// Satztrennung, wörtlich wie in `zj_e`.
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

/// Ziffernfolge unmittelbar vor einer der Einheiten — die Zahlensuche von
/// `zj_e`, nur mit der übergebenen Einheitenliste.
fn zahlen_vor_einheit(satz: &str, einheiten: &[&str]) -> Vec<String> {
    let zeichen: Vec<char> = satz.chars().collect();
    let mut treffer = Vec::new();
    for (i, c) in zeichen.iter().enumerate() {
        if !c.is_ascii_digit() {
            continue;
        }
        let mut j = i;
        while j < zeichen.len()
            && (zeichen[j].is_ascii_digit() || ",. \u{202f}\u{a0}".contains(zeichen[j]))
        {
            if " \u{202f}\u{a0}".contains(zeichen[j])
                && !zeichen.get(j + 1).is_some_and(|c| c.is_ascii_digit())
            {
                break;
            }
            j += 1;
        }
        if i > 0 && (zeichen[i - 1].is_ascii_digit() || zeichen[i - 1] == ',') {
            continue;
        }
        let rest: String = zeichen[j..].iter().collect();
        for e in einheiten {
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

/// Nur Zeit und Durchsatz. Beides hängt am Profil und an der Maschine; eine
/// Dateigröße und eine eingestellte Grenze hängen an keinem von beiden.
const ZEITEINHEITEN: [&str; 3] = [" ms", " s", " GB/s"];

/// Die Einheitenliste von `zj_e` — für die Gegenprobe am fremden Wächter.
const ZJ_E_EINHEITEN: [&str; 8] = [" s", " ms", " MB", " kB", " GB", " GiB", " MiB", " GB/s"];

/// Weg 1 und Weg 2 des Vorspanns, Merkmale wörtlich wie in `zj_e::weg`. Weg 3
/// fehlt hier mit Absicht: er gilt nur für Zahlen des alten Zustands.
fn weg_fuer_heute(satz: &str) -> Option<&'static str> {
    // Ohne „(Release“/„Release)“ und mit dem Testprozess zuerst — aus demselben
    // Grund wie in `zj_e::weg`: ein **Profil ist kein Ort**. Solange
    // „(Release“ als Merkmal des gebauten Binaries galt, wäre eine im
    // Testprozess erhobene Zahl als Binary-Zahl durchgegangen, und genau das
    // schließt der Vorspann aus.
    const BINARY: [&str; 4] = [
        "gebauten Binary",
        "gebauten Binaries",
        "target/release/redact-rs",
        "am Binary",
    ];
    const TESTPROZESS: [&str; 3] = ["Testprozess", "im Testlauf", "eigener Prozess"];

    if TESTPROZESS.iter().any(|m| satz.contains(m)) {
        if satz.contains("Debug") || satz.contains("Release") {
            return Some("Testprozess + Profil");
        }
        return None;
    }
    if BINARY.iter().any(|m| satz.contains(m)) {
        return Some("Binary");
    }
    None
}

/// Sagt der Satz selbst, dass er vom jetzigen Baum spricht?
fn spricht_von_heute(satz: &str) -> bool {
    satz.contains("heute") || satz.contains("Heute")
}

/// **Befund.** Ein Satz des Blocks führt „heute“ und sechs Zeitzahlen und
/// nennt weder einen Lauf am gebauten Binary noch einen Testprozess mit
/// Profil. Er kommt durch `zj_e` nur, weil in ihm das Wort „widerlegt“ steht —
/// und das gehört einer **anderen** Zahl desselben Satzes, der gestrichenen
/// „65,7 s“.
#[test]
fn jede_gegenwartszahl_nennt_ihren_weg() {
    let block = changelog_block();
    let mut ohne_weg: Vec<String> = Vec::new();
    let mut mit_weg: Vec<String> = Vec::new();

    for absatz in absaetze(&block) {
        for satz in saetze(&absatz) {
            if !spricht_von_heute(&satz) {
                continue;
            }
            let zeiten = zahlen_vor_einheit(&satz, &ZEITEINHEITEN);
            if zeiten.is_empty() {
                continue;
            }
            match weg_fuer_heute(&satz) {
                Some(w) => mit_weg.push(format!("[{w}] {satz}")),
                None => ohne_weg.push(format!("[{}] {satz}", zeiten.join(", "))),
            }
        }
    }

    assert!(
        mit_weg.len() + ohne_weg.len() >= 3,
        "nur {} Satz/Sätze mit „heute“ und einer Zeitzahl gefunden — der \
         Schnitt oder die Satztrennung greift nicht mehr",
        mit_weg.len() + ohne_weg.len()
    );

    assert!(
        ohne_weg.is_empty(),
        "{} Satz/Sätze des Blocks sprechen von **heute** und tragen eine \
         Zeitzahl, ohne Weg 1 (Lauf des gebauten Binaries) oder Weg 2 \
         (Testprozess samt Profil) zu nennen. Weg 3 des Vorspanns steht ihnen \
         nicht offen: er gilt nur für Zahlen, die den Zustand **vor** einer \
         Korrektur beziffern.\n\n{}\n\n\
         ({} Satz/Sätze derselben Art nennen ihren Weg:\n{})",
        ohne_weg.len(),
        ohne_weg.join("\n\n"),
        mit_weg.len(),
        mit_weg
            .iter()
            .map(|s| format!("  - {}", &s[..s.len().min(120)]))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// **Befund.** Dieselbe Bytemenge, zwei Einheiten — genau der Fehler, den der
/// Block zwei Punkte weiter oben für die Bombentabelle geradezieht („MB heißt
/// in diesem Projekt 1024² Byte“).
///
/// `audit_bytes.rs` nennt die Menge, über die jedes Muster der alten Suche
/// läuft, seit dieser Runde in 1024²-MB und schreibt dazu, dass die andere
/// Schreibweise der Fehler war. Der CHANGELOG-Block führt weiter die andere
/// Schreibweise — ohne den Zusatz „dezimal“, den er der Nachbarzahl
/// („205 MB vorher, 138 MB nachher, dort in Dezimal-MB gezählt“) gibt.
#[test]
fn die_mb_je_durchgang_steht_in_beiden_dateien_gleich() {
    let quelle = glatt(&lf(&std::fs::read_to_string(
        repo_root().join("crates/redact-pdf/src/audit_bytes.rs"),
    )
    .expect("audit_bytes.rs")));

    // „läuft jedes Muster über **256 MB** — vier Blöcke à 64 MiB“
    let marke = "läuft jedes Muster über";
    let ab = quelle.find(marke).expect("die Stelle in audit_bytes.rs") + marke.len();
    let stueck: String = quelle[ab..].chars().take(80).collect();
    let quell_mb: usize = stueck
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .expect("MB-Zahl in audit_bytes.rs");
    // „… — vier Blöcke à 64 MiB (…)“: die Zahl der Blöcke steht ausgeschrieben,
    // die Blockgröße in Ziffern.
    let vor_bloecke = stueck.split(" Blöcke à ").next().unwrap_or_default();
    let bloecke: usize = ["zwei", "drei", "vier", "fünf", "sechs"]
        .iter()
        .zip(2usize..)
        .find(|(wort, _)| vor_bloecke.contains(**wort))
        .map(|(_, n)| n)
        .expect("ausgeschriebene Blockzahl in audit_bytes.rs");
    let mib: usize = stueck
        .split(" Blöcke à ")
        .nth(1)
        .map(|s| {
            s.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
        })
        .and_then(|s| s.parse().ok())
        .expect("Blockgröße in audit_bytes.rs");

    // Abgeleitet, nicht abgeschrieben: vier Blöcke à 64 MiB in 1024²-MB.
    assert_eq!(
        quell_mb,
        bloecke * mib,
        "audit_bytes.rs rechnet die Menge nicht mehr in 1024²-MB — dann taugt \
         sie hier nicht als Maßstab"
    );

    let block = glatt(&changelog_block());
    let marke = "Muster je Begriff über ";
    let ab = block.find(marke).expect("die Rechnung im Block") + marke.len();
    let doku_mb: usize = block[ab..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .expect("MB-Zahl im CHANGELOG");

    assert_eq!(
        doku_mb, quell_mb,
        "der CHANGELOG-Block nennt {doku_mb} MB je Durchgang, \
         `crates/redact-pdf/src/audit_bytes.rs` nennt für dieselben \
         {bloecke} Blöcke à {mib} MiB {quell_mb} MB und sagt dazu, die andere \
         Schreibweise sei „dieselbe Menge dezimal gerechnet, also der Fehler“. \
         Der Vorspann desselben Blocks legt fest: MB heißt 1024 Byte zum \
         Quadrat."
    );
}

/// **Gegenprobe zum eigenen Maßstab — die andere Richtung.** Eine Grenze, die
/// gewöhnliche Prosa ablehnt, wäre genauso ein Fehler wie eine Lücke. Der
/// Maßstab dieses Tests beanstandet deshalb nichts, was keine Zeitmessung von
/// heute ist: keine Dateigröße, keine eingestellte Grenze, keine Zahl aus
/// einem alten Zustand, und keinen Satz, der seinen Weg nennt.
#[test]
fn der_massstab_beanstandet_gewoehnliche_prosa_nicht() {
    let harmlos = [
        "Eine Datei von 12 kB mit drei Objekten löst heute keine Warnung mehr aus.",
        "Die Grenze für Eingabedateien liegt heute bei 16 MB.",
        "Das Budget beträgt heute 512 MiB und deckt 100 000 Zuordnungen.",
        "Der Extraktor brauchte 31,1 s, und diese Zahl beschreibt den alten Stand.",
        "Dieselbe Datei kostet heute am gebauten Binary 0,13 s.",
        "Heute sind es im Testprozess (Release) 767–791 ms statt 746 ms.",
    ];
    for satz in harmlos {
        let zeiten = zahlen_vor_einheit(satz, &ZEITEINHEITEN);
        let beanstandet =
            spricht_von_heute(satz) && !zeiten.is_empty() && weg_fuer_heute(satz).is_none();
        assert!(
            !beanstandet,
            "der Maßstab dieses Tests beanstandet gewöhnliche Prosa: „{satz}“ \
             (gefundene Zeitzahlen: {zeiten:?})"
        );
    }

    // Und er trifft, was er treffen soll.
    let treffer = "Heute 5,01 s (1 Begriff) gegen 5,99 s (1 000).";
    assert!(
        spricht_von_heute(treffer)
            && !zahlen_vor_einheit(treffer, &ZEITEINHEITEN).is_empty()
            && weg_fuer_heute(treffer).is_none(),
        "der Maßstab trifft nicht einmal den offensichtlichen Fall"
    );
}

/// **Vertrag, fremde Datei.** Warum dieser Test seine Einheitenliste auf Zeit
/// beschränkt: `zj_e` verlangt einen Messweg auch für eine reine Dateigröße
/// und für eine eingestellte Grenze. Beide hängen an keinem Profil und an
/// keiner Maschine; für sie ist keiner der drei Wege des Vorspanns gemeint.
///
/// Heute schlägt das an nichts an — jeder Satz des Blocks mit einer kB- oder
/// MB-Zahl trägt auch eine Messung. Der nächste gewöhnliche Satz über eine
/// Dateigröße im Block wird aber abgelehnt. Das ist die andere Richtung
/// desselben Fehlers und gehört `zj_e_etikett_der_messzahl.rs`, nicht hier.
#[test]
fn zj_e_verlangt_einen_weg_auch_fuer_eine_dateigroesse() {
    for satz in [
        "Eine Datei von 12 kB mit drei Objekten löst keine Warnung mehr aus.",
        "Die Grenze für Eingabedateien liegt bei 16 MB.",
    ] {
        assert!(
            zahlen_vor_einheit(satz, &ZEITEINHEITEN).is_empty(),
            "„{satz}“ trägt keine Zeitzahl — dieser Test setzt genau das voraus"
        );
        assert!(
            !zahlen_vor_einheit(satz, &ZJ_E_EINHEITEN).is_empty(),
            "die Einheitenliste von `zj_e` findet in „{satz}“ nichts mehr — \
             dann ist dieser Vertrag erledigt"
        );
    }
}
