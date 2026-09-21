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
//! bis 6. Über den ganzen Abschnitt gelesen melden **25** Sätze keinen Weg —
//! gemessen in der Fix-Runde 9 mit genau diesem Schnitt, und festgehalten von
//! `zm_d_wege_im_ganzen_abschnitt`, das in beide Richtungen rot wird. Das ist ein Befund
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
    // **Kürzer als in der Fix-Runde 8, und mit Grund.** Dort kamen sechs
    // Merkmale hinzu: eine Kommandozeile im Satz, `RUSAGE_CHILDREN`, der
    // `VmHWM` und die Spitze eines Kindprozesses, und der Name eines
    // `_mess_`-Tests. **Keines** kam im geprüften Block vor — eine Lockerung,
    // die kein Lauf deckt, ist ein Scheintest. Und eines war zusätzlich falsch:
    // eine Kommandozeile zu **erwähnen** ist nicht dasselbe, wie die Zahl von
    // dort zu haben; der Gegenbeweis steht als Probe in
    // [`die_merkmale_des_weges_sind_einzeln_belegt`].
    //
    // Sie kommen zurück, wenn der Schnitt auf den ganzen Abschnitt geweitet
    // wird (Register #53) — dann **mit** ihren Fällen.
    const BINARY: [&str; 4] = [
        "gebauten Binary",
        "gebauten Binaries",
        "target/release/redact-rs",
        "am Binary",
    ];
    const TESTPROZESS: [&str; 3] = ["Testprozess", "im Testlauf", "eigener Prozess"];
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

/// **Jedes Merkmal einzeln, an einem gebauten Satz.**
///
/// Die Merkmalslisten in [`weg`] sind das, woran dieser Test hängt — und sie
/// waren eine Runde lang ungeprüft: keines der in der Fix-Runde 8
/// hinzugekommenen Merkmale kam im Block überhaupt vor, und ein Lauf, der
/// nichts trifft, deckt nichts. Nimmt man sie weg, bleibt alles grün; das ist
/// die Bauart eines Scheintests. Hier stehen sie einzeln, mit dem Satz, der
/// sie auslöst, und mit dem, der sie **nicht** auslösen darf.
///
/// Mutation, die ihn rot macht: ein Merkmal aus `BINARY` oder `TESTPROZESS`
/// streichen — oder „`redact-rs " wieder aufnehmen.
#[test]
fn die_merkmale_des_weges_sind_einzeln_belegt() {
    // (Satz, erwartetes Urteil)
    let proben: [(&str, Option<&str>); 7] = [
        ("Gemessen am gebauten Binary: 0,15 s.", Some("Binary")),
        (
            "Aus einem Lauf des gebauten Binaries: 0,15 s.",
            Some("Binary"),
        ),
        ("`target/release/redact-rs` braucht 0,15 s.", Some("Binary")),
        (
            "Gemessen im Testprozess (Release): 0,15 s.",
            Some("Testprozess + Profil"),
        ),
        (
            "Gemessen im Testprozess (Release), Lauf \
             `zd_orakel_budget::zd_mess_die_alte_suche_je_muster`: 0,163 s.",
            Some("Testprozess + Profil"),
        ),
        // **Der Gegenbeweis.** Der Name eines Messtests allein ist kein Weg: er
        // sagt nicht, wo gemessen wurde. „Testprozess“ sagt es, und dann
        // verlangt der zweite Weg zusätzlich das Profil.
        (
            "Gemessen in `zd_mess_die_alte_suche_je_muster`: 0,163 s.",
            None,
        ),
        // **Der Gegenbeweis, der diese Liste gekostet hat.** Der Satz erwähnt
        // eine Kommandozeile und sagt im selben Atemzug, dass die Zahl NICHT
        // von dort kommt. Galt „`redact-rs " als Merkmal, ging er als Zahl des
        // gebauten Binaries durch — das stille Umetikett, das der Vorspann
        // ausschließt.
        (
            "Gemessen an der Oberfläche: die Statuszeile entsteht heute in 0,42 s, \
             und die Kommandozeile `redact-rs --gui` gibt es dafür nicht.",
            None,
        ),
    ];

    for (satz, erwartet) in proben {
        assert_eq!(
            weg(satz),
            erwartet,
            "der Weg dieses Satzes wird falsch gelesen: „{satz}“"
        );
        // Und die Probe prüft wirklich etwas: jeder dieser Sätze trägt eine
        // Zeit- oder Speicherzahl, fällt also überhaupt unter die Regel.
        assert!(
            !traegt_mess_groesse(satz).is_empty(),
            "die Probe trägt gar keine Messgröße: „{satz}“"
        );
    }
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

    // **Greift der Schnitt?** Hier stand `>= 8` — eine Zahl als Ersatz für die
    // Frage, und sie schlug an, sobald eine Runde ihre Befunde ohne Zeit- und
    // Speicherzahlen beschreiben konnte. Das ist ein Fehlalarm, der den Wächter
    // zwingt, Zahlen zu erfinden. Gefragt ist, ob der Schnitt **die beiden
    // jüngsten Fix-Runden** trägt — und das lässt sich direkt sagen.
    let runden = block.matches("### Fix-Runde ").count();
    assert_eq!(
        runden, 2,
        "der Block trägt {runden} Fix-Runden statt zwei — der Schnitt greift nicht mehr"
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
