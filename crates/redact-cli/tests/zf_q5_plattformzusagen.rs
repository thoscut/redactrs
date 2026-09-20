//! Gegenprüfung Q5 der Fix-Runde 5: **die Regel, die das lokale Tor nicht
//! fahren kann.**
//!
//! Zweimal war der Windows-Job der CI rot, und beide Male durch einen *Test*,
//! nicht durch das Programm: die Speichermessung der Bombentests las auf jedem
//! Ziel `/proc/self/status`. Das lokale Tor fährt für Windows nur
//! `cargo clippy --target x86_64-pc-windows-gnu`; Clippy prüft (mit
//! `--all-targets`) auch den Testcode, aber es **führt** ihn nicht aus. Der
//! Fehler war kein Typfehler, sondern ein Laufzeitfehler — Clippy konnte ihn
//! nicht sehen.
//!
//! Nachgemessen, ob sich die Lücke schließen lässt, indem das Tor die Tests
//! für Windows wenigstens **baut**:
//!
//! ```text
//! $ cargo test --no-run --target x86_64-pc-windows-gnu -p redact-cli
//! error: error calling dlltool 'x86_64-w64-mingw32-dlltool':
//!        No such file or directory (os error 2)
//! ```
//!
//! Mit `llvm-dlltool` (liegt in `/usr/bin`) als `x86_64-w64-mingw32-dlltool`
//! im Pfad kommt der Bau weiter und bleibt am nächsten Werkzeug stehen:
//!
//! ```text
//! error: linker `x86_64-w64-mingw32-gcc` not found
//! ```
//!
//! Der `windows-gnu`-Zweig braucht die mingw-Laufzeit (`libmingwex.a` und
//! Genossen); die Rust-Toolchain bringt nur `crt2.o` und `dllcrt2.o` mit
//! (`lib/rustlib/x86_64-pc-windows-gnu/lib/self-contained/`), `apt list
//! --installed | grep mingw` findet nichts, und `/var/cache/apt/archives`
//! ist leer — ohne Netz ist hier nichts nachzuinstallieren. Ein
//! `cargo test --no-run` für Windows ist im lokalen Tor also **nicht** zu
//! haben.
//!
//! # Die Regel
//!
//! Was zu haben ist, ist eine Regel **im Baum**, die genau die Fehlerklasse
//! ausschließt, die zweimal durchkam: eine Stelle, die eine Einrichtung des
//! ausführenden Systems beim Namen nennt, muss entweder
//!
//! * hinter einem Plattform-`cfg` stehen (`#![cfg(target_os = "linux")]`,
//!   `#[cfg(unix)]`, `cfg!(target_os = "linux")`) — dann entsteht sie auf
//!   anderen Zielen gar nicht —, **oder**
//! * ihr Ergebnis als *optional* behandeln (`.ok()`, `unwrap_or…`,
//!   `is_err()`, `map_or`, `if let Ok(…)`, ein `match` mit `Err(…)`) — dann
//!   fehlt auf anderen Zielen die Zahl und nicht der Test.
//!
//! Verboten ist die dritte Fassung, die viermal rot war: die Einrichtung
//! ungeschützt nennen und ihr Ergebnis als gegeben nehmen (`.expect(…)`,
//! `.unwrap()`).
//!
//! # Was die Gegenprüfung der Runde 6 an der ersten Fassung fand
//!
//! Neun Proben, einzeln an den Baum gehängt und gefahren
//! (`gegen/r5/plattform.sh`): **ein Fehlalarm** und **sechs Lücken**.
//!
//! | Probe | erste Fassung | jetzt |
//! |---|---|---|
//! | `read_to_string("/proc/self/status").expect(…)` | rot | rot |
//! | dieselbe Zeile, ein **fremdes** `.ok()` acht Zeilen weiter | grün | rot |
//! | `Path::new("/proc").join("self")` — ohne Schrägstrich | grün | rot |
//! | `format!("/proc/{}/status", …)` | rot | rot |
//! | `"/proc/self/status"` in einem **Blockkommentar** | rot | grün |
//! | `read_to_string("C:\\Windows\\win.ini").expect(…)` | grün | rot |
//! | `read_to_string("/etc/localtime").expect(…)` | grün | rot |
//! | `use std::os::unix::fs::PermissionsExt;` ohne `cfg` | grün | rot |
//! | `Command::new("mkfifo")…expect(…)` | grün | rot |
//!
//! Drei Verschärfungen stecken darin:
//!
//! 1. **Der Ausweg zählt nur in derselben Anweisung.** Ein `.ok()` acht Zeilen
//!    weiter gehört einer anderen Anweisung und rettet nichts. Die Anweisung
//!    wird über die Klammerbilanz geschnitten, damit ein `match … { Ok(…) =>
//!    …, Err(e) => … };` ganz dazugehört — das ist der Weg, den
//!    `belege.rs::die_zahl_der_pruefungen_steht_im_readme` für `python3`
//!    richtig geht.
//! 2. **Kommentare sind wirklich ausgenommen**, auch `/* … */` über mehrere
//!    Zeilen. Vorher fiel nur die Zeile weg, die mit `//` *begann*.
//! 3. **Ein fremdes Programm ist eine Einrichtung des Systems.**
//!    `Command::new("mkfifo")` hängt am `PATH`, nicht am Zielsystem —
//!    `#[cfg(unix)]` sagt darüber nichts. Auf einem schlanken Unix-Bild
//!    (Container ohne `coreutils`/`util-linux`) panickt ein Test, der das
//!    Startergebnis als gegeben nimmt. Für diese Marke rettet ein `cfg`
//!    deshalb **nicht**; nur ein Ausweg zählt.
//!
//! Die Gegenprüfung fand neun Stellen im Baum, die die geschärfte Regel
//! verletzen und nicht `redact-cli` gehören. Drei davon sind behoben: zweimal
//! `std::os::unix::fs::symlink` ohne `cfg` — kein Laufzeit-, sondern ein
//! **Bau**fehler auf Windows, wo der CI-Job `cargo clippy --workspace
//! --all-targets` und `cargo test --workspace` fährt — und einmal ein
//! `/proc/self/status` mit `.expect(…)`, das ebendort zur Laufzeit panickt.
//! Sechs bleiben: derselbe `mkfifo`-Fall, der an `cfg(unix)` hängt, aber am
//! `PATH` scheitert. Sie stehen namentlich in [`ALTLASTEN`]; die Liste muss
//! genau aufgehen — eine neue Verletzung macht den Test rot, eine behobene
//! ebenso.
//!
//! Der Test liest den Quelltext, nicht das Programm — deshalb steht er hier
//! und braucht kein Windows.
//!
//! Die neun Proben stehen als Quelltext-Schnipsel in
//! [`die_neun_proben_der_gegenpruefung`] — dort, wo die Regel sie beißt, und
//! nicht als Anhängsel an einer fremden Datei. Drei Gegenproben stehen daneben:
//! derselbe Zugriff unter `cfg`, mit `.ok()` in derselben Anweisung, und der
//! `python3`-Weg mit `match … Err(e) => …`.
//!
//! Mutationsnachweis: jede der drei Verschärfungen einzeln zurückgenommen
//! (Ausweg im 8-Zeilen-Fenster statt in der Anweisung; Kommentare nur
//! zeilenweise; `Command::new` unter `Schutz::CfgOderAusweg`) →
//! `die_neun_proben_der_gegenpruefung` ist jeweils rot und nennt die Probe.
//! Dazu: `ALTLASTEN` um einen Eintrag gekürzt → rot, um einen erfundenen
//! erweitert → rot.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Rettet ein Plattform-`cfg` die Stelle, oder hilft nur ein Ausweg?
#[derive(Clone, Copy, PartialEq, Eq)]
enum Schutz {
    /// Die Einrichtung gibt es auf anderen Zielen nicht — unter einem
    /// `cfg` entsteht die Stelle dort gar nicht, und das genügt.
    CfgOderAusweg,
    /// Die Einrichtung hängt **nicht** am Zielsystem, sondern an dem, was auf
    /// dem Rechner installiert ist. `#[cfg(unix)]` sagt nichts darüber; nur
    /// ein Ausweg trägt.
    NurAusweg,
}

/// Einrichtungen des ausführenden Systems — als **Zeichenkettenliteral** oder
/// als Pfad im Quelltext gesucht, damit ein Satz im Fließtext eines Kommentars
/// nicht mitzählt (Kommentare werden ohnehin vorher ausgeblendet).
const HEIKEL: &[(&str, Schutz)] = &[
    // Linux-Dateisysteme. Mit *und* ohne Schrägstrich: `Path::new("/proc")`
    // nennt dieselbe Einrichtung wie `"/proc/self/status"`.
    ("\"/proc/", Schutz::CfgOderAusweg),
    ("\"/proc\"", Schutz::CfgOderAusweg),
    ("\"/sys/", Schutz::CfgOderAusweg),
    ("\"/sys\"", Schutz::CfgOderAusweg),
    ("\"/dev/", Schutz::CfgOderAusweg),
    ("\"/dev\"", Schutz::CfgOderAusweg),
    // Unix-Systemverzeichnisse jenseits von Linux.
    ("\"/etc/", Schutz::CfgOderAusweg),
    ("\"/etc\"", Schutz::CfgOderAusweg),
    ("\"/var/", Schutz::CfgOderAusweg),
    ("\"/usr/", Schutz::CfgOderAusweg),
    // Windows-Systemorte. Ein Benutzerpfad (`C:\Users\…`) ist keine
    // Einrichtung des Systems, sondern ein Beispiel — deshalb nur diese drei.
    ("\"C:\\\\Windows", Schutz::CfgOderAusweg),
    ("\"C:\\\\Program", Schutz::CfgOderAusweg),
    ("%SystemRoot%", Schutz::CfgOderAusweg),
    // Schalen-Eingebautes.
    ("\"ulimit\"", Schutz::CfgOderAusweg),
    // Plattform-APIs: `std::os::unix::…` gibt es auf Windows nicht, der Bau
    // bricht dort ab. Ein `cfg` nimmt die Stelle heraus und genügt.
    ("std::os::unix::", Schutz::CfgOderAusweg),
    ("std::os::windows::", Schutz::CfgOderAusweg),
    // Ein fremdes Programm hängt am `PATH`, nicht am Ziel. `Command::new` mit
    // einem *Literal* meint ein Programm, das installiert sein muss;
    // `Command::new(bin())` (das eigene Binary) trifft die Marke nicht.
    ("Command::new(\"", Schutz::NurAusweg),
];

/// Ein Plattform-`cfg`: dann entsteht die Stelle auf anderen Zielen nicht.
const CFG: &[&str] = &[
    "#![cfg(target_os",
    "#![cfg(unix",
    "#![cfg(windows",
    "#![cfg(target_family",
    "#[cfg(target_os",
    "#[cfg(unix",
    "#[cfg(windows",
    "#[cfg(target_family",
    "cfg!(target_os",
    "cfg!(unix",
    "cfg!(windows",
    "cfg!(target_family",
];

/// Das Ergebnis wird als *optional* behandelt: fehlt die Einrichtung, fehlt
/// die Zahl und nicht der Test. Gesucht wird nur in **derselben Anweisung**.
const OPTIONAL: &[&str] = &[
    ".ok()",
    "unwrap_or",
    ".is_ok()",
    ".is_err()",
    "map_or",
    "if let Ok(",
    "let Ok(",
    "Err(_)",
    "Err(e)",
    "Err(err)",
    "Ok(v) =>",
    ".unwrap_or_default()",
];

/// Wie viele Zeilen über der Stelle nach einem Plattform-`cfg` gesucht wird.
///
/// Nur für das `cfg`: es wirkt auf alles unter sich, ein Attribut über dem
/// umgebenden Block ist deshalb eine echte Deckung. Der **Ausweg** zählt
/// dagegen nur in derselben Anweisung (siehe [`anweisung`]).
const NAHE: usize = 8;

/// Alle `*.rs` unter `crates/`.
fn rust_dateien(wurzel: &Path, out: &mut Vec<PathBuf>) {
    let Ok(eintraege) = std::fs::read_dir(wurzel) else {
        return;
    };
    for eintrag in eintraege.flatten() {
        let pfad = eintrag.path();
        if pfad.is_dir() {
            if pfad.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_dateien(&pfad, out);
        } else if pfad.extension().is_some_and(|e| e == "rs") {
            out.push(pfad);
        }
    }
}

/// Der Quelltext **ohne Kommentare**: jedes Zeichen eines `//`- oder
/// `/* … */`-Kommentars wird durch ein Leerzeichen ersetzt, die Zeilenzahl
/// bleibt.
///
/// Das ist die zweite Verschärfung: vorher fiel nur eine Zeile weg, die mit
/// `//` *begann* — ein Blockkommentar, der `"/proc/self/status"` erklärt, war
/// ein Fehlalarm (Probe P5 der Gegenprüfung).
fn ohne_kommentare(text: &str) -> String {
    let zeichen: Vec<char> = text.chars().collect();
    let mut aus = String::with_capacity(text.len());
    let mut i = 0;
    let mut tiefe = 0usize; // Blockkommentare dürfen in Rust schachteln.
    while i < zeichen.len() {
        let c = zeichen[i];
        let naechstes = zeichen.get(i + 1).copied().unwrap_or('\0');
        if tiefe > 0 {
            if c == '/' && naechstes == '*' {
                tiefe += 1;
                aus.push_str("  ");
                i += 2;
                continue;
            }
            if c == '*' && naechstes == '/' {
                tiefe -= 1;
                aus.push_str("  ");
                i += 2;
                continue;
            }
            aus.push(if c == '\n' { '\n' } else { ' ' });
            i += 1;
            continue;
        }
        match c {
            '/' if naechstes == '*' => {
                tiefe = 1;
                aus.push_str("  ");
                i += 2;
            }
            '/' if naechstes == '/' => {
                while i < zeichen.len() && zeichen[i] != '\n' {
                    aus.push(' ');
                    i += 1;
                }
            }
            '"' => {
                // Zeichenketten bleiben stehen: in ihnen *steht* die
                // Einrichtung, die gesucht wird.
                aus.push('"');
                i += 1;
                while i < zeichen.len() {
                    let z = zeichen[i];
                    aus.push(z);
                    i += 1;
                    if z == '\\' {
                        if let Some(&n) = zeichen.get(i) {
                            aus.push(n);
                            i += 1;
                        }
                        continue;
                    }
                    if z == '"' {
                        break;
                    }
                }
            }
            _ => {
                aus.push(c);
                i += 1;
            }
        }
    }
    aus
}

/// Eine Zeile ohne ihre Zeichenketten — zum Zählen von Klammern.
fn ohne_zeichenketten(zeile: &str) -> String {
    let zeichen: Vec<char> = zeile.chars().collect();
    let mut aus = String::with_capacity(zeile.len());
    let mut i = 0;
    while i < zeichen.len() {
        if zeichen[i] != '"' {
            aus.push(zeichen[i]);
            i += 1;
            continue;
        }
        i += 1;
        while i < zeichen.len() {
            if zeichen[i] == '\\' {
                i += 2;
                continue;
            }
            if zeichen[i] == '"' {
                i += 1;
                break;
            }
            i += 1;
        }
        aus.push(' ');
    }
    aus
}

/// Endet die Anweisung mit dieser Zeile?
fn schliesst(zeile: &str) -> bool {
    let s = zeile.trim_end();
    s.ends_with(';') || s.ends_with('{') || s.ends_with('}') || s.is_empty()
}

/// **Die Anweisung**, in der Zeile `i` steht — nicht ein Fenster aus `N`
/// Zeilen darum herum.
///
/// Rückwärts bis hinter das Ende der vorigen Anweisung, vorwärts bis die
/// Klammerbilanz aufgeht **und** die Zeile mit `;` (oder `,`) endet. Damit
/// gehört `let out = match Command::new("python3") … { Ok(o) => o, Err(e) =>
/// { … } };` ganz dazu, ein `.ok()` in der *nächsten* Zeile dagegen nicht.
fn anweisung(zeilen: &[&str], i: usize) -> String {
    let mut von = i;
    while von > 0 {
        let vorige = ohne_zeichenketten(zeilen[von - 1]);
        let vorige = vorige.trim();
        if vorige.is_empty() || vorige.starts_with("#[") || schliesst(vorige) {
            break;
        }
        von -= 1;
    }
    let mut tiefe: i32 = 0;
    let mut bis = von;
    for (nr, zeile) in zeilen.iter().enumerate().skip(von) {
        let nackt = ohne_zeichenketten(zeile);
        for c in nackt.chars() {
            match c {
                '(' | '[' | '{' => tiefe += 1,
                ')' | ']' | '}' => tiefe -= 1,
                _ => {}
            }
        }
        bis = nr;
        let getrimmt = nackt.trim_end();
        if nr >= i && tiefe <= 0 && (getrimmt.ends_with(';') || getrimmt.ends_with(',')) {
            break;
        }
        // Die schließende Klammer des umgebenden Blocks: weiter gehört die
        // Anweisung nicht (ein Rumpf ohne `;` endet so).
        if nr >= i && tiefe < 0 {
            break;
        }
        // Eine Anweisung über mehr als 40 Zeilen gibt es hier nicht; ohne
        // Abbruch liefe eine unbalancierte Datei bis ans Ende.
        if nr >= von + 40 && nr >= i {
            break;
        }
    }
    zeilen[von..=bis.min(zeilen.len() - 1)].join("\n")
}

/// Die Zeile des Kopfes der Funktion, in der `zeile` steht (0-basiert).
fn funktionskopf(zeilen: &[&str], zeile: usize) -> Option<usize> {
    (0..=zeile).rev().find(|&i| {
        let s = zeilen[i].trim_start();
        s.starts_with("fn ")
            || s.starts_with("pub fn ")
            || s.starts_with("pub(crate) fn ")
            || s.starts_with("mod ")
            || s.starts_with("pub mod ")
    })
}

/// Trägt die Funktion (oder das Modul), in der die Stelle steht, ein
/// Plattform-`cfg` als Attribut?
fn kopf_hat_cfg(zeilen: &[&str], kopf: usize) -> bool {
    for i in (0..kopf).rev() {
        let s = zeilen[i].trim_start();
        if CFG.iter().any(|c| s.starts_with(c)) {
            return true;
        }
        if s.starts_with("///") || s.starts_with("//") || s.starts_with("#[") || s.is_empty() {
            continue;
        }
        return false;
    }
    false
}

/// **Altlasten**: Stellen, die die Regel heute verletzt und die **nicht mir
/// gehören** (Fix-Runde 7, Agent E besitzt nur `redact-cli`).
///
/// Jede steht mit Datei, Marke und Grund hier, damit die Regel scharf ist,
/// ohne dass sie eine fremde Datei ändern müsste. Der Test verlangt die Liste
/// **genau**: eine neue Verletzung macht ihn rot, und eine behobene ebenso —
/// dann ist die Zeile hier zu streichen.
const ALTLASTEN: &[(&str, &str, &str)] = &[
    // Sechsmal derselbe Fall: `Command::new("mkfifo")…status().expect(…)`
    // unter `#[cfg(unix)]`. `cfg(unix)` sagt nichts über den `PATH`; auf einem
    // schlanken Unix-Bild ohne `util-linux` panickt der Test, statt sich — wie
    // `belege.rs` es für `python3` vormacht — mit einem Hinweis zu begnügen.
    (
        "crates/redact-booking/tests/loader_tests.rs",
        "Command::new(\"",
        "mkfifo ohne Ausweg",
    ),
    (
        "crates/redact-cli/tests/check_leaks.rs",
        "Command::new(\"",
        "mkfifo ohne Ausweg",
    ),
    (
        "crates/redact-cli/tests/hardening.rs",
        "Command::new(\"",
        "mkfifo ohne Ausweg",
    ),
    (
        "crates/redact-core/src/read.rs",
        "Command::new(\"",
        "mkfifo ohne Ausweg",
    ),
    (
        "crates/redact-gui/src/app.rs",
        "Command::new(\"",
        "mkfifo ohne Ausweg",
    ),
    (
        "crates/redact-patterns/tests/config_limits.rs",
        "Command::new(\"",
        "mkfifo ohne Ausweg",
    ),
    // Die drei Stellen der Oberfläche, die Agent E hier nur melden konnte,
    // sind behoben und deshalb gestrichen: zweimal `std::os::unix::fs::symlink`
    // ohne `cfg` (ein **Bau**fehler auf Windows, wo der CI-Job `cargo clippy
    // --workspace --all-targets` fährt) und einmal `proc_status()`, das
    // `/proc/self/status` ohne Ausweg las (ein Laufzeitfehler ebendort). Der
    // Symlink steht jetzt unter `#[cfg(unix)]`, `proc_status` samt seinem
    // Messtest unter `#[cfg(target_os = "linux")]`.
];

/// Ein Pfad mit `/` als Trenner, egal auf welchem System er entstand.
///
/// Unter Windows liefert [`std::path::Display`] Backslashes, [`ALTLASTEN`] nennt
/// seine Dateien aber mit Schrägstrichen. Ohne diese Vereinheitlichung passt
/// dort **kein** Eintrag: jede Altlast gilt als neue Verletzung, und der Test
/// ist ausgerechnet auf der Plattform rot, gegen deren Annahmen er wacht. Genau
/// so war der Windows-Job der CI zum vierten Mal rot — der Prüfer machte
/// selbst die Annahme, die er anderen verbietet.
///
/// Gebunden von [`ein_pfad_mit_backslashes_findet_seine_altlast`]; Mutation
/// (die `replace`-Zeile entfernt): rot, auch auf Linux.
fn mit_schraegstrichen(pfad: &str) -> String {
    pfad.replace('\\', "/").replace("../../", "")
}

/// Eine gefundene Verletzung: Datei (relativ), Zeile, Marke, Quelltext.
struct Verstoss {
    datei: String,
    zeile: usize,
    marke: &'static str,
    text: String,
}

/// Alle Verletzungen der Regel im Baum — samt der Zahl der untersuchten
/// Stellen.
fn verstoesse() -> (Vec<Verstoss>, usize) {
    let crates = repo_root().join("crates");
    let mut dateien = Vec::new();
    rust_dateien(&crates, &mut dateien);
    assert!(
        dateien.len() > 50,
        "der Baum wurde nicht gefunden — nur {} Dateien unter {}",
        dateien.len(),
        crates.display()
    );

    let mut geprueft = 0usize;
    let mut gefunden: Vec<Verstoss> = Vec::new();

    for datei in &dateien {
        // Diese Datei selbst nicht: sie *nennt* die Namen, um nach ihnen zu
        // suchen. Sonst prüfte die Regel ihren eigenen Wortlaut.
        if datei
            .file_name()
            .is_some_and(|n| n == "zf_q5_plattformzusagen.rs")
        {
            continue;
        }
        let roh = std::fs::read_to_string(datei)
            .unwrap_or_else(|e| panic!("{} lesbar: {e}", datei.display()))
            .replace("\r\n", "\n");
        let text = ohne_kommentare(&roh);
        let zeilen: Vec<&str> = text.lines().collect();
        let datei_gedeckt = zeilen.iter().any(|z| {
            let s = z.trim_start();
            s.starts_with("#![cfg(target_os")
                || s.starts_with("#![cfg(unix")
                || s.starts_with("#![cfg(windows")
                || s.starts_with("#![cfg(target_family")
        });
        let relativ = mit_schraegstrichen(
            &datei
                .strip_prefix(repo_root())
                .unwrap_or(datei)
                .display()
                .to_string(),
        );

        for (i, zeile) in zeilen.iter().enumerate() {
            let Some((marke, schutz)) = HEIKEL
                .iter()
                .find(|(t, _)| zeile.contains(t))
                .map(|(t, s)| (*t, *s))
            else {
                continue;
            };
            geprueft += 1;
            let satz = anweisung(&zeilen, i);
            let hat_ausweg = OPTIONAL.iter().any(|o| satz.contains(o));
            if hat_ausweg {
                continue;
            }
            if schutz == Schutz::CfgOderAusweg {
                // Ein `cfg` **darf** über der Stelle stehen, nicht nur in ihrer
                // Anweisung: `#[cfg(unix)] { use std::os::unix::…; … }` nimmt
                // den ganzen Block heraus. Verschärft wurde der **Ausweg**,
                // nicht das `cfg` — ein `cfg` wirkt wirklich auf alles unter
                // sich, ein `.ok()` nur auf seinen eigenen Ausdruck.
                let von = i.saturating_sub(NAHE);
                let davor = zeilen[von..=i].join("\n");
                let unter_cfg = datei_gedeckt
                    || CFG.iter().any(|c| davor.contains(c))
                    || funktionskopf(&zeilen, i).is_some_and(|k| kopf_hat_cfg(&zeilen, k));
                if unter_cfg {
                    continue;
                }
            }
            gefunden.push(Verstoss {
                datei: relativ.clone(),
                zeile: i + 1,
                marke,
                text: zeile.trim_start().to_string(),
            });
        }
    }
    (gefunden, geprueft)
}

/// **Der Prüfer darf nicht selbst plattformabhängig sein.** Ein Pfad, wie
/// Windows ihn schreibt, muss seine Altlast finden.
///
/// Der Test steht hier, weil er auf **jedem** System läuft: er baut die
/// Windows-Schreibweise selbst und schickt sie durch dieselbe Funktion, die der
/// Prüfer benutzt. Ohne sie war der Windows-Job der CI rot und nannte alle
/// sechs `mkfifo`-Stellen als neue Verletzung, obwohl sie namentlich in
/// [`ALTLASTEN`] stehen.
#[test]
fn ein_pfad_mit_backslashes_findet_seine_altlast() {
    for (datei, _, _) in ALTLASTEN {
        assert!(
            !datei.contains('\\'),
            "ALTLASTEN nennt seine Dateien mit Schrägstrichen: {datei}"
        );
        let wie_windows = datei.replace('/', "\\");
        assert_eq!(
            mit_schraegstrichen(&wie_windows),
            *datei,
            "die Windows-Schreibweise findet ihre Altlast nicht"
        );
    }
    // Und der Weg, den `strip_prefix` offenlässt, wird weiter gekürzt.
    assert_eq!(
        mit_schraegstrichen("..\\..\\crates\\redact-cli\\src\\main.rs"),
        "crates/redact-cli/src/main.rs"
    );
}

/// **Die Regel.** Jede Stelle im Baum, die eine Einrichtung des ausführenden
/// Systems beim Namen nennt, steht unter einem Plattform-`cfg` **oder**
/// behandelt ihr Ergebnis als optional — in **derselben Anweisung**.
///
/// Verletzungen, die nicht `redact-cli` gehören, stehen namentlich in
/// [`ALTLASTEN`]; die Liste muss genau aufgehen.
#[test]
fn keine_systemeinrichtung_ohne_cfg_oder_ohne_ausweg() {
    let (gefunden, geprueft) = verstoesse();

    // Eine Regel, die nichts findet, prüft nichts. Der Baum nennt
    // `/proc/self/status` an mehreren Stellen — bleibt der Zähler klein, ist
    // die Suche kaputt und nicht der Baum sauber.
    assert!(
        geprueft >= 20,
        "nur {geprueft} Stelle(n) untersucht — die Suche greift nicht mehr"
    );

    let mut offen: Vec<String> = Vec::new();
    let mut getroffen = vec![false; ALTLASTEN.len()];
    for v in &gefunden {
        match ALTLASTEN
            .iter()
            .position(|(d, m, _)| *d == v.datei && *m == v.marke)
        {
            Some(k) => getroffen[k] = true,
            None => offen.push(format!("{}:{}  {}", v.datei, v.zeile, v.text)),
        }
    }
    assert!(
        offen.is_empty(),
        "{} Stelle(n) nennen eine Einrichtung des ausführenden Systems, ohne \
         Plattform-cfg und ohne Ausweg in derselben Anweisung — genau daran war \
         der Windows-Job der CI viermal rot:\n{}",
        offen.len(),
        offen.join("\n")
    );

    let erledigt: Vec<&str> = ALTLASTEN
        .iter()
        .zip(&getroffen)
        .filter(|(_, &t)| !t)
        .map(|((d, _, _), _)| *d)
        .collect();
    assert!(
        erledigt.is_empty(),
        "{} Altlast(en) sind behoben — bitte die Zeile(n) in ALTLASTEN streichen, \
         sonst deckt die Liste mehr, als es gibt:\n{}",
        erledigt.len(),
        erledigt.join("\n")
    );
}

/// Die Regel gegen sich selbst: die neun Proben der Gegenprüfung, hier als
/// Quelltext-Schnipsel statt als Anhängsel an eine fremde Datei.
///
/// Das ist der Mutationsnachweis **im Baum**: wird eine der drei
/// Verschärfungen zurückgenommen, wird dieser Test rot und nennt die Probe.
#[test]
fn die_neun_proben_der_gegenpruefung() {
    // (Name, Rumpf, erwartet: ist es ein Verstoß?)
    const PROBEN: &[(&str, &str, bool)] = &[
        (
            "nackt",
            "fn f() {\n    let _p = std::fs::read_to_string(\"/proc/self/status\").expect(\"nur Linux\");\n}\n",
            true,
        ),
        (
            "fremdes .ok() acht Zeilen weiter",
            "fn f() {\n    let _p = std::fs::read_to_string(\"/proc/self/status\").expect(\"nur Linux\");\n    let _q = \"7\".parse::<u32>().ok();\n}\n",
            true,
        ),
        (
            "Pfad ohne Schrägstrich",
            "fn f() {\n    let p = std::path::Path::new(\"/proc\").join(\"self\").join(\"status\");\n    let _p = std::fs::read_to_string(p).expect(\"nur Linux\");\n}\n",
            true,
        ),
        (
            "format!",
            "fn f() {\n    let _p = std::fs::read_to_string(format!(\"/proc/{}/status\", std::process::id())).expect(\"nur Linux\");\n}\n",
            true,
        ),
        (
            "Blockkommentar",
            "fn f() {\n    /* Hinweis: \"/proc/self/status\" gibt es nur unter Linux. */\n    let _p = 1;\n}\n",
            false,
        ),
        (
            "Windows-Systempfad",
            "fn f() {\n    let _p = std::fs::read_to_string(\"C:\\\\Windows\\\\win.ini\").expect(\"nur Windows\");\n}\n",
            true,
        ),
        (
            "/etc",
            "fn f() {\n    let _p = std::fs::read_to_string(\"/etc/localtime\").expect(\"nur Unix\");\n}\n",
            true,
        ),
        (
            "Unix-API ohne cfg",
            "fn f() {\n    use std::os::unix::fs::PermissionsExt;\n    let m = std::fs::metadata(\"Cargo.toml\").expect(\"da\");\n    let _b = m.permissions().mode();\n}\n",
            true,
        ),
        (
            "fremdes Programm",
            "fn f() {\n    let _s = std::process::Command::new(\"mkfifo\").arg(\"x\").status().expect(\"mkfifo startbar\");\n}\n",
            true,
        ),
        // Die Gegenrichtung: genau so ist es richtig (der `python3`-Weg aus
        // `belege.rs`), und genau so darf die Regel nicht anschlagen.
        (
            "fremdes Programm mit Ausweg in derselben Anweisung",
            "fn f() {\n    let out = match std::process::Command::new(\"python3\").arg(\"-V\").output() {\n        Ok(out) => out,\n        Err(e) => {\n            eprintln!(\"kein python3 ({e})\");\n            return;\n        }\n    };\n    let _ = out;\n}\n",
            false,
        ),
        (
            "Linux-Einrichtung mit .ok() in derselben Anweisung",
            "fn f() {\n    let _p = std::fs::read_to_string(\"/proc/self/status\")\n        .ok()\n        .unwrap_or_default();\n}\n",
            false,
        ),
        (
            "Linux-Einrichtung unter cfg",
            "#[cfg(target_os = \"linux\")]\nfn f() {\n    let _p = std::fs::read_to_string(\"/proc/self/status\").expect(\"nur Linux\");\n}\n",
            false,
        ),
    ];

    let mut falsch: Vec<String> = Vec::new();
    for (name, rumpf, erwartet) in PROBEN {
        let text = ohne_kommentare(&rumpf.replace("\r\n", "\n"));
        let zeilen: Vec<&str> = text.lines().collect();
        let mut verstoss = false;
        for (i, zeile) in zeilen.iter().enumerate() {
            let Some((_, schutz)) = HEIKEL
                .iter()
                .find(|(t, _)| zeile.contains(t))
                .map(|(t, s)| (*t, *s))
            else {
                continue;
            };
            let satz = anweisung(&zeilen, i);
            if OPTIONAL.iter().any(|o| satz.contains(o)) {
                continue;
            }
            let von = i.saturating_sub(NAHE);
            let davor = zeilen[von..=i].join("\n");
            if schutz == Schutz::CfgOderAusweg
                && (CFG.iter().any(|c| davor.contains(c))
                    || funktionskopf(&zeilen, i).is_some_and(|k| kopf_hat_cfg(&zeilen, k)))
            {
                continue;
            }
            verstoss = true;
        }
        if verstoss != *erwartet {
            falsch.push(format!(
                "Probe „{name}“: erwartet {}, bekommen {}",
                if *erwartet { "Verstoß" } else { "sauber" },
                if verstoss { "Verstoß" } else { "sauber" }
            ));
        }
    }
    assert!(
        falsch.is_empty(),
        "{} Probe(n) der Gegenprüfung gehen nicht auf:\n{}",
        falsch.len(),
        falsch.join("\n")
    );
}

/// Jede Altlast trägt ihren Grund, und der Grund stimmt noch: die Marke steht
/// wirklich in der Datei.
///
/// Ohne diesen Test bliebe eine Zeile stehen, deren Fall es längst nicht mehr
/// gibt — und deckte beim nächsten Mal etwas anderes.
#[test]
fn jede_altlast_traegt_ihren_grund_und_ihren_fall() {
    for (datei, marke, grund) in ALTLASTEN {
        assert!(
            grund.len() > 12,
            "{datei} steht ohne belastbare Begründung in ALTLASTEN"
        );
        let text = std::fs::read_to_string(repo_root().join(datei))
            .unwrap_or_else(|e| panic!("{datei} lesbar: {e}"));
        assert!(
            text.contains(marke),
            "{datei} enthält „{marke}“ nicht mehr — Eintrag in ALTLASTEN streichen"
        );
    }
    // Der `mkfifo`-Fall ist der große: er hängt am `PATH`, nicht am Ziel.
    let mkfifo = ALTLASTEN
        .iter()
        .filter(|(_, m, _)| *m == "Command::new(\"")
        .count();
    assert!(
        mkfifo >= 5,
        "nur {mkfifo} `mkfifo`-Altlast(en) — dann ist die Begründung neu zu schreiben"
    );
    for (datei, marke, _) in ALTLASTEN {
        if *marke != "Command::new(\"" {
            continue;
        }
        let text = std::fs::read_to_string(repo_root().join(datei)).expect("lesbar");
        assert!(
            text.contains("Command::new(\"mkfifo\")"),
            "{datei} startet kein `mkfifo` mehr — Eintrag in ALTLASTEN streichen"
        );
    }
}
