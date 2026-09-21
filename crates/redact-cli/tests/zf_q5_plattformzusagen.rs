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
//! # Die vierte Verschärfung: Zeichenkette oder Verwendung?
//!
//! Kommentare waren ausgenommen, Zeichenketten nicht — und daran schlug der
//! Prüfer in dieser Runde auf einen **gebundenen Satz** an: `belege.rs` hält
//! Sätze der Doku fest, und einer davon nannte den Namen des Unix-APIs, das
//! oben behoben wurde. Umgangen wurde das, indem der Name aus dem Satz
//! verschwand — die Doku beugte sich dem Prüfer, und das ist der falsche Weg
//! herum.
//!
//! Zeichenketten *blind* auszunehmen wäre der falsche Ausweg zurück: dann
//! wächst die blinde Fläche des Prüfers. Eine Verwendung kann sich nicht in
//! einer Zeichenkette verstecken — eine Zeichenkette wird nicht ausgeführt —,
//! aber ein Pfad, den `Command::new("…")` oder `Path::new("…")` weitergibt,
//! **steht** in einem Literal und ist sehr wohl eine Verwendung. Ein Prüfer,
//! der `"/proc/self/status"` in `read_to_string("/proc/self/status")` nicht mehr
//! sieht, prüft nichts mehr.
//!
//! Unterschieden wird deshalb nach der **Stelle** der Marke im Literal
//! ([`ist_verwendung`]):
//!
//! | Marke steht … | Beispiel | gilt als |
//! |---|---|---|
//! | im Code | `use std::os::unix::fs::…;` | Verwendung |
//! | am Anfang des Inhalts | `Path::new("%SystemRoot%")` | Verwendung |
//! | mitten im Inhalt | `format!("… {}mal std::os::unix::fs::symlink …", n)` | Erwähnung |
//!
//! Das ist die kleinste Regel, die beide Seiten trifft: eine Marke, vor der im
//! selben Literal noch Text steht, ist Teil eines Satzes — Doku,
//! Fehlermeldung, gebundener Satz. Eine Marke, mit der das Literal *anfängt*,
//! **ist** die Einrichtung, die weitergegeben wird. Und die Marken, die selbst
//! mit einem Anführungszeichen beginnen (`"/proc/`, `"C:\\Windows`), treffen
//! ohnehin nur den Anfang eines Literals.
//!
//! # Die fünfte Verschärfung: die Karte muss stimmen — in **beide** Richtungen
//!
//! Die erste Fassung dieser Karte zählte Anführungszeichen, Zeile für Zeile,
//! mit einem Übertrag nur für die Fortsetzung mit `\` am Zeilenende. Damit war
//! die Blindstelle **verschoben, nicht geschlossen** — und der Prüfer lag
//! danach in *beide* Richtungen falsch:
//!
//! * Ein gewöhnliches Zeichenliteral `'"'` kippte die Zählung. Alles dahinter
//!   galt als Inhalt einer Zeichenkette, also als Erwähnung — eine **Verwendung**
//!   von `std::os::unix::` verschwand. Die alte, grobe Fassung (`zeile.contains`)
//!   hatte sie gesehen. Gerade diese Klasse bricht auf Windows nicht zur
//!   Laufzeit, sondern beim **Bau**, und war in der Runde 7 zwei von drei
//!   echten Befunden.
//! * Dieselbe Naivität erzeugte **Fehlalarme**: ein `'"'` ließ den
//!   Kommentarschnitt entgleisen, danach schlug die Regel auf reinem
//!   Kommentartext an; und ein Quelltext-Schnipsel in einer rohen Zeichenkette
//!   (`r"…"`, `r#"…"#`, die keine Maskierung kennt) war **immer** ein Verstoß.
//!   Eine Grenze, die gewöhnlichen Quelltext ablehnt, ist genauso ein Fehler
//!   wie eine Lücke.
//!
//! Beides kommt von derselben Ursache: Rust lässt sich nicht durch Zählen von
//! Anführungszeichen zerlegen. Deshalb geht jetzt **ein** Durchgang über die
//! Datei ([`lagen`]) und legt für jedes Byte fest, was dort steht — Kommentar
//! (auch geschachtelt), Inhalt einer Zeichenkette (auch roh, auch über mehrere
//! Zeilen), Zeichenliteral oder Code. Aus demselben Durchgang kommen alle drei
//! Sichten, die die Regel braucht ([`zerlegt`]): der Text ohne Kommentare, die
//! Literalkarte und das **nackte** Abbild jeder Zeile, an dem Klammern gezählt
//! und der Ausweg gesucht wird. Der Übertrag von Hand fällt damit weg: eine
//! Zeichenkette über mehrere Zeilen ist von selbst richtig gezählt, und ein
//! `'('` in einem Zeichenliteral ist keine offene Klammer mehr.
//!
//! Alle Seiten stehen als Proben im Baum — **paarweise**, denn ein Prüfer, der
//! nur in einer Richtung geprobt wird, lässt die andere offen:
//!
//! | Probe | vorher | jetzt |
//! |---|---|---|
//! | `Path::new("%SystemRoot%\\win.ini")` — Verwendung | rot | rot |
//! | derselbe Name mitten in einem gebundenen Satz | rot | grün |
//! | derselbe Satz über drei Zeilen fortgesetzt | rot | grün |
//! | Verwendung hinter einem `'"'` | **grün** | rot |
//! | Verwendung hinter `'\''` und `'\\'` | rot | rot |
//! | Kommentartext hinter einem `'"'` | **rot** | grün |
//! | Schnipsel in `r#"…"#` | **rot** | grün |
//! | derselbe Schnipsel mehrzeilig | **rot** | grün |
//!
//! # Was davon im Baum steht
//!
//! Die Gegenprüfung fand neun Stellen im Baum, die die geschärfte Regel
//! verletzen und nicht `redact-cli` gehören. Drei davon sind in der Runde 7
//! behoben: zweimal `std::os::unix::fs::symlink` ohne `cfg` — kein Laufzeit-,
//! sondern ein **Bau**fehler auf Windows, wo der CI-Job `cargo clippy
//! --workspace --all-targets` und `cargo test --workspace` fährt — und einmal
//! ein `/proc/self/status` mit `.expect(…)`, das ebendort zur Laufzeit panickt.
//!
//! Die sechs `mkfifo`-Stellen tragen jetzt den Ausweg, den `belege.rs` für
//! `python3` vormacht. Der erste Zuschnitt dieses Auswegs deckte allerdings
//! genau **einen** Fall — `mkfifo` lässt sich nicht *starten* (`Err`) — und
//! damit nicht den Anlassfall des Befundes: auf einem BusyBox-Bild steht der
//! Name als Symlink im `PATH`, das Applet fehlt, `status()` liefert
//! `Ok(exit status: 127)`, und der harte `assert!(ok.success(), …)` dahinter
//! ließ den Lauf an allen fünf Stellen platzen, obwohl am Programm nichts
//! falsch war. Die Begründung dafür war sachlich falsch: scheitert `mkfifo`,
//! gibt es **keine** Pipe — es ist genauso nichts zu prüfen wie bei fehlendem
//! Werkzeug. Deshalb überspringt jetzt **jeder** Ausgang außer null, mit
//! Hinweis auf `stderr`.
//!
//! Fünf Stellen nennt [`BEHOBENE_MKFIFO`] namentlich, und
//! [`die_fuenf_behobenen_mkfifo_stellen_haben_ihren_ausweg_und_sagen_ihn`] hält
//! sie fest — samt der Zeile, die sagt, was ein übersprungener Test *nicht*
//! geprüft hat. Ein stiller Übersprung wäre aus einer Prüfung eine Entwarnung
//! geworden. Weil eine Textprüfung des Quelltexts das nicht halten kann (ein
//! `Err(e) => panic!(…)` erfüllt jede Zeichenkettenprüfung und auch diese
//! Regel), fährt
//! `check_leaks::eine_mkfifo_attrappe_mit_ausgang_127_bricht_den_lauf_nicht`
//! die Stelle **wirklich**: mit einem `PATH`, in dem `mkfifo` ein Skript ist,
//! das mit 127 endet.
//!
//! Was noch offen ist, steht namentlich in [`ALTLASTEN`]; die Liste muss genau
//! aufgehen — eine neue Verletzung macht den Test rot, eine behobene ebenso,
//! und leer sein darf sie auch.
//!
//! Der Test liest den Quelltext, nicht das Programm — deshalb steht er hier
//! und braucht kein Windows.
//!
//! Die siebzehn Proben stehen als Quelltext-Schnipsel in
//! [`die_proben_der_gegenpruefung`] — dort, wo die Regel sie beißt, und nicht
//! als Anhängsel an einer fremden Datei (neun aus der Runde 6, drei für die
//! Zeichenketten der Runde 7, fünf für die Literalkarte dieser Runde). Drei
//! Gegenproben stehen daneben: derselbe Zugriff unter `cfg`, mit `.ok()` in
//! derselben Anweisung, und der `python3`-Weg mit `match … Err(e) => …`.
//!
//! Mutationsnachweis: jede Verschärfung einzeln zurückgenommen (Ausweg im
//! 8-Zeilen-Fenster statt in der Anweisung; Kommentare nur zeilenweise;
//! `Command::new` unter `Schutz::CfgOderAusweg`; [`ist_verwendung`] einmal ohne
//! die Erwähnung und einmal blind gegen jedes Literal; in [`lagen`] die
//! Zeichenliterale und dann die rohen Zeichenketten übergangen) →
//! `die_proben_der_gegenpruefung` ist jeweils rot und nennt die Probe.
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

/// Wie viele Zeilen **nach** dem Start eines fremden Werkzeugs daraufhin
/// gelesen werden, dass dort keine Behauptung über seinen Ausgang steht
/// (siehe [`die_fuenf_behobenen_mkfifo_stellen_haben_ihren_ausweg_und_sagen_ihn`]).
///
/// Zwölf Zeilen: der Ausweg selbst braucht fünf bis acht, der harte `assert`
/// stand unmittelbar dahinter.
const FENSTER: usize = 12;

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

/// Was an einer Stelle des Quelltexts steht.
///
/// Eine Zeile Rust lässt sich **nicht** durch Zählen von Anführungszeichen
/// zerlegen, und die erste Fassung dieses Prüfers tat genau das. Sie lag
/// dadurch in *beide* Richtungen falsch:
///
/// * Ein gewöhnliches Zeichenliteral `'"'` kippte die Zählung. Alles dahinter
///   galt als Inhalt einer Zeichenkette, also als *Erwähnung* — eine
///   Verwendung von `std::os::unix::` verschwand. Das ist die Klasse, die auf
///   Windows nicht zur Laufzeit, sondern beim **Bau** bricht, und in der Runde
///   7 zwei von drei echten Befunden war.
/// * Eine rohe Zeichenkette (`r"…"`, `r#"…"#`) kennt keine Maskierung; ihr
///   Inhalt zerfiel dem Zähler in Stücke. Ein Quelltext-Schnipsel darin galt
///   deshalb **immer** als Verstoß — ein Fehlalarm auf gewöhnlichem Quelltext.
///
/// Deshalb geht **ein** Durchgang über die Datei und legt für jedes Byte fest,
/// was dort steht ([`lagen`]). Er kennt Zeilen- und (schachtelbare)
/// Blockkommentare, gewöhnliche und rohe Zeichenketten samt `b`-Vorsatz,
/// Zeichenliterale — und den Unterschied zwischen einem Zeichenliteral (`'a'`)
/// und einer Lebenszeit (`'a`). Aus demselben Durchgang kommen alle drei
/// Sichten, die die Regel braucht: der Quelltext ohne Kommentare, die
/// Literalkarte und das **nackte** Abbild jeder Zeile.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lage {
    /// Code — dazu gehören auch die Begrenzer einer Zeichenkette (`"`, `r#"`,
    /// `"#`), denn eine Marke wie `"/proc/` beginnt mit dem öffnenden
    /// Anführungszeichen.
    Code,
    /// Kommentar.
    Kommentar,
    /// Ein Zeichenliteral samt seiner Begrenzer. Für die Regel zählt es wie
    /// Code (eine Marke passt in kein einzelnes Zeichen); beim Zählen von
    /// Klammern wird es übersprungen, denn `'('` ist keine offene Klammer.
    Zeichen,
    /// Im *Inhalt* einer Zeichenkette, als `k`-tes Zeichen dieses Inhalts.
    Inhalt(usize),
}

/// Die Länge in Bytes des Zeichens, das bei `i` beginnt (UTF-8).
fn zeichenlaenge(b: &[u8], i: usize) -> usize {
    let n = match b[i] {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        // Ein Folgebyte kann hier nicht anfangen; ein Byte weiter ist die
        // einzige Antwort, die nicht stehenbleibt.
        _ => 1,
    };
    n.min(b.len() - i)
}

/// Beginnt bei `i` eine **rohe** Zeichenkette (`r"…"`, `r#"…"#`, `br##"…"##`)?
///
/// Zurück kommt der Anfang ihres Inhalts und die Zahl der Rauten.
fn roher_anfang(b: &[u8], i: usize) -> Option<(usize, usize)> {
    // Kein Bestandteil eines Namens: das `r` in `for` fängt keine rohe
    // Zeichenkette an.
    if i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_') {
        return None;
    }
    let mut j = i;
    if b[j] == b'b' {
        j += 1;
    }
    if b.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let mut rauten = 0usize;
    while b.get(j) == Some(&b'#') {
        rauten += 1;
        j += 1;
    }
    if b.get(j) == Some(&b'"') {
        Some((j + 1, rauten))
    } else {
        None
    }
}

/// Stehen ab `i` genau `rauten` Rauten? Damit endet eine rohe Zeichenkette.
fn rauten_folgen(b: &[u8], i: usize, rauten: usize) -> bool {
    (0..rauten).all(|k| b.get(i + k) == Some(&b'#'))
}

/// Das Ende des Zeichenliterals, das bei `i` beginnt — oder `None`, wenn dort
/// eine **Lebenszeit** steht (`'a`, `'static`, `'_`).
///
/// Das ist die Unterscheidung, an der ein Zähler von Anführungszeichen
/// zerbricht: `'"'` ist ein Zeichen und öffnet keine Zeichenkette, `&'a str`
/// dagegen ist gar kein Literal.
fn zeichenliteral_ende(b: &[u8], i: usize) -> Option<usize> {
    if i + 1 >= b.len() {
        return None;
    }
    if b[i + 1] == b'\\' {
        // Maskiert: `'\''`, `'\\'`, `'\n'`, `'\x41'`, `'\u{1F600}'`. Das
        // maskierte Zeichen kann selbst ein `'` sein, deshalb erst dahinter
        // suchen. Länger als `'\u{10FFFF}'` wird kein Zeichenliteral.
        let mut j = i + 3;
        while j < b.len() && j <= i + 12 {
            if b[j] == b'\'' {
                return Some(j + 1);
            }
            j += 1;
        }
        return None;
    }
    let l = zeichenlaenge(b, i + 1);
    if b.get(i + 1 + l) == Some(&b'\'') {
        Some(i + 1 + l + 1)
    } else {
        None
    }
}

/// Für **jedes Byte** des Quelltexts: was steht dort?
///
/// Der ganze Text auf einmal, nicht Zeile für Zeile — eine Zeichenkette über
/// mehrere Zeilen (eine rohe oder eine mit `\` fortgesetzte) ist damit von
/// selbst richtig gezählt, und der Übertrag von Hand, den die erste Fassung
/// brauchte, fällt weg.
fn lagen(text: &str) -> Vec<Lage> {
    /// Eintragen — aber ein Zeilenumbruch bleibt immer `Code`: die
    /// Zeilengrenzen müssen stehen bleiben, auch mitten in einer mehrzeiligen
    /// Zeichenkette.
    fn setze(aus: &mut [Lage], b: &[u8], von: usize, bis: usize, was: Lage) {
        for k in von..bis {
            if b[k] != b'\n' {
                aus[k] = was;
            }
        }
    }

    let b = text.as_bytes();
    let mut aus = vec![Lage::Code; b.len()];
    let mut i = 0usize;
    while i < b.len() {
        // Blockkommentar — in Rust schachtelbar.
        if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            let von = i;
            let mut tiefe = 1usize;
            i += 2;
            while i < b.len() && tiefe > 0 {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    tiefe += 1;
                    i += 2;
                } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    tiefe -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            setze(&mut aus, b, von, i, Lage::Kommentar);
            continue;
        }
        // Zeilenkommentar.
        if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
            let von = i;
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            setze(&mut aus, b, von, i, Lage::Kommentar);
            continue;
        }
        // Zeichenliteral — oder eine Lebenszeit, die nur ihr `'` ist.
        if b[i] == b'\'' {
            match zeichenliteral_ende(b, i) {
                Some(ende) => {
                    setze(&mut aus, b, i, ende, Lage::Zeichen);
                    i = ende;
                }
                None => i += 1,
            }
            continue;
        }
        // Rohe Zeichenkette: keine Maskierung, Ende erst bei `"` samt Rauten.
        if let Some((inhalt, rauten)) = roher_anfang(b, i) {
            i = inhalt;
            let mut k = 0usize;
            while i < b.len() {
                if b[i] == b'"' && rauten_folgen(b, i + 1, rauten) {
                    i += 1 + rauten;
                    break;
                }
                let l = zeichenlaenge(b, i);
                setze(&mut aus, b, i, i + l, Lage::Inhalt(k));
                i += l;
                k += 1;
            }
            continue;
        }
        // Gewöhnliche Zeichenkette (auch `b"…"`).
        if b[i] == b'"' {
            i += 1;
            let mut k = 0usize;
            while i < b.len() {
                if b[i] == b'"' {
                    i += 1;
                    break;
                }
                if b[i] == b'\\' {
                    // Maskierung: der Rückstrich und das maskierte Zeichen
                    // gehören zum Inhalt — auch der Zeilenumbruch einer
                    // Fortsetzung.
                    setze(&mut aus, b, i, i + 1, Lage::Inhalt(k));
                    i += 1;
                    if i < b.len() {
                        let l = zeichenlaenge(b, i);
                        setze(&mut aus, b, i, i + l, Lage::Inhalt(k + 1));
                        i += l;
                    }
                    k += 2;
                    continue;
                }
                let l = zeichenlaenge(b, i);
                setze(&mut aus, b, i, i + l, Lage::Inhalt(k));
                i += l;
                k += 1;
            }
            continue;
        }
        i += 1;
    }
    aus
}

/// Eine Zeile in den drei Sichten, die die Regel braucht.
struct Zeile {
    /// Die Zeile ohne Kommentare — Zeichenketten bleiben stehen, denn in ihnen
    /// *steht* die Einrichtung, die gesucht wird.
    text: String,
    /// Für jedes Byte: steht es im Inhalt einer Zeichenkette, und als
    /// wievieltes Zeichen dieses Inhalts? (Siehe [`ist_verwendung`].)
    karte: Vec<Option<usize>>,
    /// Nur Code: Zeichenketten, Zeichenliterale und Kommentare sind durch
    /// Leerzeichen ersetzt. Daran werden Klammern gezählt und der Ausweg
    /// gesucht — ein `.ok()` **in** einer Zeichenkette ist keiner.
    nackt: String,
}

/// Den Quelltext in Zeilen zerlegen — in einem Durchgang ([`lagen`]).
///
/// Die Ersetzungen halten die Byte-Länge, deshalb passen Karte, Text und
/// nacktes Abbild Byte für Byte aufeinander.
fn zerlegt(roh: &str) -> Vec<Zeile> {
    if roh.is_empty() {
        return Vec::new();
    }
    let lage = lagen(roh);
    let b = roh.as_bytes();
    let mut aus = Vec::new();
    let mut von = 0usize;
    loop {
        let bis = match b[von..].iter().position(|c| *c == b'\n') {
            Some(n) => von + n,
            None => b.len(),
        };
        let scheibe = &lage[von..bis];
        let text: Vec<u8> = b[von..bis]
            .iter()
            .zip(scheibe)
            .map(|(c, l)| if *l == Lage::Kommentar { b' ' } else { *c })
            .collect();
        let nackt: Vec<u8> = b[von..bis]
            .iter()
            .zip(scheibe)
            .map(|(c, l)| if *l == Lage::Code { *c } else { b' ' })
            .collect();
        aus.push(Zeile {
            text: String::from_utf8(text).expect("ganze Zeichen ersetzt"),
            karte: scheibe
                .iter()
                .map(|l| match l {
                    Lage::Inhalt(k) => Some(*k),
                    _ => None,
                })
                .collect(),
            nackt: String::from_utf8(nackt).expect("ganze Zeichen ersetzt"),
        });
        if bis >= b.len() {
            break;
        }
        von = bis + 1;
        // Ein Text, der mit `\n` endet, hat keine leere Schlusszeile — genau
        // wie [`str::lines`].
        if von == b.len() {
            break;
        }
    }
    aus
}

/// Ist die Marke, die bei Byte `p` beginnt, eine **Verwendung** — oder nur eine
/// **Erwähnung** in einem Text?
///
/// Das ist die Blindstelle, die diese Runde schließt. Kommentare waren schon
/// ausgenommen ([`zerlegt`]), Zeichenketten nicht: der gebundene Satz
/// in `belege.rs`, der einen behobenen Befund beschreibt, schlug an, weil er
/// den Namen des Unix-APIs nannte.
///
/// Zeichenketten *blind* auszunehmen wäre der falsche Ausweg: ein Pfad, den
/// `Command::new("mkfifo")` oder `Path::new("%SystemRoot%")` weitergibt, steht
/// in einem Literal und ist sehr wohl eine Verwendung. Die Unterscheidung
/// hängt deshalb an der **Stelle** der Marke im Literal:
///
/// * Code (`None`) — oder das öffnende Anführungszeichen, mit dem eine Marke
///   wie `"/proc/` selbst beginnt: **Verwendung**.
/// * am Anfang des Inhalts (`Some(0)`) — das Literal *ist* die Einrichtung,
///   `Path::new("%SystemRoot%")`: **Verwendung**.
/// * mitten im Inhalt (`Some(k)`, `k > 0`) — vor der Marke steht Text, sie ist
///   also Teil eines Satzes: **Erwähnung**.
///
/// Gebunden von den Proben „Verwendung im Zeichenketten-Argument“ (muss rot
/// machen) und „Erwähnung im gebundenen Satz“ (darf nicht rot machen) in
/// [`die_proben_der_gegenpruefung`].
fn ist_verwendung(karte: &[Option<usize>], p: usize) -> bool {
    !matches!(karte.get(p), Some(Some(k)) if *k > 0)
}

/// Die erste Marke der Zeile, die dort wirklich **verwendet** wird.
///
/// Die Reihenfolge ist die von [`HEIKEL`] — wie vorher; neu ist nur, dass eine
/// Marke nicht zählt, wenn *jedes* ihrer Vorkommen in der Zeile bloß eine
/// Erwähnung ist.
fn verwendete_marke(zeile: &str, karte: &[Option<usize>]) -> Option<(&'static str, Schutz)> {
    HEIKEL
        .iter()
        .find(|(t, _)| {
            zeile
                .match_indices(*t)
                .any(|(p, _)| ist_verwendung(karte, p))
        })
        .map(|(t, s)| (*t, *s))
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
///
/// Gezählt und gesucht wird auf den **nackten** Zeilen ([`Zeile::nackt`]):
/// `'('` in einem Zeichenliteral ist keine offene Klammer, und ein `.ok()`
/// **in** einer Zeichenkette ist kein Ausweg.
fn anweisung(nackte: &[&str], i: usize) -> String {
    let mut von = i;
    while von > 0 {
        let vorige = nackte[von - 1].trim();
        if vorige.is_empty() || vorige.starts_with("#[") || schliesst(vorige) {
            break;
        }
        von -= 1;
    }
    let mut tiefe: i32 = 0;
    let mut bis = von;
    for (nr, nackt) in nackte.iter().enumerate().skip(von) {
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
    nackte[von..=bis.min(nackte.len() - 1)].join("\n")
}

/// Die Zeile des Kopfes der Funktion, in der `zeile` steht (0-basiert).
fn funktionskopf(nackte: &[&str], zeile: usize) -> Option<usize> {
    (0..=zeile).rev().find(|&i| {
        let s = nackte[i].trim_start();
        s.starts_with("fn ")
            || s.starts_with("pub fn ")
            || s.starts_with("pub(crate) fn ")
            || s.starts_with("mod ")
            || s.starts_with("pub mod ")
    })
}

/// Trägt die Funktion (oder das Modul), in der die Stelle steht, ein
/// Plattform-`cfg` als Attribut?
fn kopf_hat_cfg(nackte: &[&str], kopf: usize) -> bool {
    for i in (0..kopf).rev() {
        let s = nackte[i].trim_start();
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
    // Von den sechs `mkfifo`-Stellen ist hier keine mehr offen: die fünf
    // dieser Runde stehen in [`BEHOBENE_MKFIFO`], und die sechste
    // (`crates/redact-gui/src/app.rs`) trägt ihren Ausweg jetzt ebenfalls —
    // deshalb ist ihre Zeile gestrichen. **Offen bleibt dort etwas, das diese
    // Regel nicht sieht:** hinter dem Ausweg steht weiter ein
    // `assert!(ok.success(), …)`. Ein `mkfifo`, das mit 127 endet (BusyBox
    // ohne das Applet), lässt den Lauf dort also weiter platzen, obwohl keine
    // Pipe entstanden ist. Die Regel prüft den Ausweg, nicht was danach mit
    // ihm geschieht; für die eigenen fünf Stellen hält das
    // [`die_fuenf_behobenen_mkfifo_stellen_haben_ihren_ausweg_und_sagen_ihn`]
    // — `app.rs` gehört der Oberfläche und ist als Vertrag gemeldet.
    // Ein **Beleg der Gegenprüfung** (Register #19), absichtlich rot abgelegt —
    // und dabei selbst ein Windows-Baufehler: `std::os::unix::fs::symlink` ohne
    // `cfg`. Die Datei gehört `redact-gui`; hier steht sie, damit die Regel
    // scharf bleibt, ohne eine fremde Datei zu ändern. Wird sie unter ein
    // `cfg(unix)` gestellt, ist diese Zeile zu streichen — der Test sagt es.
    (
        "crates/redact-gui/tests/zi_c_verworfenes_urteil.rs",
        "std::os::unix::",
        "Unix-API ohne cfg — Baufehler auf Windows, gehört redact-gui",
    ),
    // Die drei Stellen der Oberfläche, die Agent E hier nur melden konnte,
    // sind behoben und deshalb gestrichen: zweimal `std::os::unix::fs::symlink`
    // ohne `cfg` (ein **Bau**fehler auf Windows, wo der CI-Job `cargo clippy
    // --workspace --all-targets` fährt) und einmal `proc_status()`, das
    // `/proc/self/status` ohne Ausweg las (ein Laufzeitfehler ebendort). Der
    // Symlink steht jetzt unter `#[cfg(unix)]`, `proc_status` samt seinem
    // Messtest unter `#[cfg(target_os = "linux")]`.
];

/// Die fünf `mkfifo`-Stellen, die diese Runde behoben hat.
///
/// Sie starten das Werkzeug weiter — aber **mit** Ausweg, und sie sagen auf
/// `stderr`, was sie deshalb nicht geprüft haben. Namentlich hier, weil
/// [`ALTLASTEN`] sie nicht mehr deckt: ohne diese Liste wäre die Korrektur nur
/// so lange da, wie niemand sie zurücknimmt.
const BEHOBENE_MKFIFO: &[&str] = &[
    "crates/redact-booking/tests/loader_tests.rs",
    "crates/redact-cli/tests/check_leaks.rs",
    "crates/redact-cli/tests/hardening.rs",
    "crates/redact-core/src/read.rs",
    "crates/redact-patterns/tests/config_limits.rs",
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
        let zerlegte = zerlegt(&roh);
        let nackte: Vec<&str> = zerlegte.iter().map(|z| z.nackt.as_str()).collect();
        let datei_gedeckt = nackte.iter().any(|z| {
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

        for (i, zeile) in zerlegte.iter().enumerate() {
            let Some((marke, schutz)) = verwendete_marke(&zeile.text, &zeile.karte) else {
                continue;
            };
            geprueft += 1;
            let satz = anweisung(&nackte, i);
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
                let davor = nackte[von..=i].join("\n");
                let unter_cfg = datei_gedeckt
                    || CFG.iter().any(|c| davor.contains(c))
                    || funktionskopf(&nackte, i).is_some_and(|k| kopf_hat_cfg(&nackte, k));
                if unter_cfg {
                    continue;
                }
            }
            gefunden.push(Verstoss {
                datei: relativ.clone(),
                zeile: i + 1,
                marke,
                text: zeile.text.trim_start().to_string(),
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

/// Die Regel gegen sich selbst: die Proben der Gegenprüfung, hier als
/// Quelltext-Schnipsel statt als Anhängsel an eine fremde Datei.
///
/// Siebzehn Proben und drei Gegenproben: neun aus der Runde 6, drei für die
/// Zeichenketten der Runde 7 — und fünf für die Literalkarte, die diese Runde
/// richtigstellt. Die fünf kommen **paarweise**: zwei Verwendungen, die die
/// naive Zählung übersah (hinter `'\"'` und hinter maskierten
/// Zeichenliteralen), und drei Fehlalarme, die sie erzeugte (Kommentartext
/// hinter `'\"'`, ein Schnipsel in einer rohen Zeichenkette, derselbe
/// mehrzeilig). Ein Prüfer, der nur in einer Richtung geprobt wird, lässt die
/// andere offen — daran war die erste Fassung dieser Karte gescheitert.
///
/// Die Zahl stand früher im Namen dieses Tests und veraltete zweimal
/// (‚neun‘, dann ‚zwölf‘); sie steht jetzt nur noch dort, wo ein Test sie hält
/// — unten in der gebundenen Länge der Liste.
///
/// Das ist der Mutationsnachweis **im Baum**: wird eine der Verschärfungen
/// zurückgenommen, wird dieser Test rot und nennt die Probe.
#[test]
fn die_proben_der_gegenpruefung() {
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
        // Die Blindstelle dieser Runde: Zeichenketten. Eine Marke, mit der das
        // Literal *anfängt*, wird weitergegeben — das ist eine Verwendung, auch
        // ohne Schrägstrich am Anfang.
        (
            "Verwendung im Zeichenketten-Argument",
            "fn f() {\n    let p = std::path::Path::new(\"%SystemRoot%\\\\win.ini\");\n    let _t = std::fs::read_to_string(p).expect(\"nur Windows\");\n}\n",
            true,
        ),
        // Und die Gegenseite: derselbe Name mitten in einem Satz, den ein Test
        // an die Doku bindet. Das ist Text *über* die Regel, keine Verwendung —
        // genau der Fehlalarm, der `belege.rs` den Namen aus dem Satz nahm.
        (
            "Erwähnung im gebundenen Satz",
            "fn f() {\n    let _s = format!(\"{} Stellen verletzten die Regel, {}mal std::os::unix::fs::symlink ohne cfg\", 9, 2);\n}\n",
            false,
        ),
        // Und derselbe Satz, wie `belege.rs` ihn wirklich schreibt: ein Literal
        // über drei Zeilen, mit `\` fortgesetzt. Ohne den Übertrag in
        // [`lagen`] sähe der Prüfer die zweite Zeile als Code und
        // schlüge dort wieder an — die Lieferung wäre halb.
        (
            "Erwähnung im fortgesetzten Satz",
            "fn f() {\n    let _s = format!(\n        \"{} Stellen im Baum verletzten die Regel; **{} davon** sind behoben: \\\n         {}mal `std::os::unix::fs::symlink` **ohne** `cfg`\",\n        9, 3, 2\n    );\n}\n",
            false,
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
        // ---- Die Blindstelle, die Befund 2 **verschoben** statt geschlossen
        // hatte: die Literalkarte zählte Anführungszeichen naiv. Ein
        // gewöhnliches `'\"'` kippte die Zählung — danach galt jede Marke als
        // Erwähnung, und die Verwendung verschwand. Betroffen war gerade die
        // Klasse, die auf Windows beim **Bau** bricht.
        (
            "Verwendung hinter einem Zeichenliteral",
            "fn f(p: &std::path::Path) {\n    let _s = format!(\"{}{:?}\", '\"', std::os::unix::fs::symlink(p, p));\n}\n",
            true,
        ),
        // Dieselbe Naivität in der anderen Richtung, Teil 1: ein `'\"'` ließ den
        // Kommentarschnitt entgleisen, und danach schlug die Regel auf reinem
        // Kommentartext an — ein Fehlalarm auf gewöhnlichem Quelltext.
        (
            "Erwähnung im Kommentar hinter einem Zeichenliteral",
            "fn f() {\n    let anfuehrung = '\"';\n    // Hinweis: std::os::unix::fs::symlink gibt es auf Windows nicht.\n    let _ = anfuehrung;\n}\n",
            false,
        ),
        // Teil 2: eine rohe Zeichenkette kennt keine Maskierung. Ein
        // Quelltext-Schnipsel darin war deshalb **immer** ein Verstoß.
        (
            "Schnipsel in einer rohen Zeichenkette",
            "fn f() {\n    let _doku = r#\"so nicht: std::fs::read_to_string(\"/proc/self/status\")\"#;\n    let _ = _doku;\n}\n",
            false,
        ),
        (
            "Schnipsel in einer mehrzeiligen rohen Zeichenkette",
            "fn f() {\n    let _doku = r#\"\n        so nicht:\n            let _ = std::os::unix::fs::symlink(a, b);\n    \"#;\n    let _ = _doku;\n}\n",
            false,
        ),
        // Und die Gegenrichtung der Gegenrichtung: maskierte Zeichenliterale
        // dürfen die Karte **nicht** verschieben — der Verstoß dahinter bleibt
        // sichtbar.
        (
            "Verwendung hinter maskierten Zeichenliteralen",
            "fn f(p: &std::path::Path) {\n    let _s = format!(\"{}{}{:?}\", '\\'', '\\\\', std::os::unix::fs::symlink(p, p));\n}\n",
            true,
        ),
        (
            "Linux-Einrichtung unter cfg",
            "#[cfg(target_os = \"linux\")]\nfn f() {\n    let _p = std::fs::read_to_string(\"/proc/self/status\").expect(\"nur Linux\");\n}\n",
            false,
        ),
    ];

    let mut falsch: Vec<String> = Vec::new();
    for (name, rumpf, erwartet) in PROBEN {
        let zerlegte = zerlegt(&rumpf.replace("\r\n", "\n"));
        let nackte: Vec<&str> = zerlegte.iter().map(|z| z.nackt.as_str()).collect();
        let mut verstoss = false;
        for (i, zeile) in zerlegte.iter().enumerate() {
            let Some((_, schutz)) = verwendete_marke(&zeile.text, &zeile.karte) else {
                continue;
            };
            let satz = anweisung(&nackte, i);
            if OPTIONAL.iter().any(|o| satz.contains(o)) {
                continue;
            }
            let von = i.saturating_sub(NAHE);
            let davor = nackte[von..=i].join("\n");
            if schutz == Schutz::CfgOderAusweg
                && (CFG.iter().any(|c| davor.contains(c))
                    || funktionskopf(&nackte, i).is_some_and(|k| kopf_hat_cfg(&nackte, k)))
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

    // Die Zahl der Proben ist gebunden: siebzehn Proben und drei Gegenproben,
    // wie die Doku dieses Tests sie zählt. Ohne diese Zeile könnte eine Probe
    // still verschwinden.
    assert_eq!(
        PROBEN.len(),
        17 + 3,
        "die Doku nennt siebzehn Proben und drei Gegenproben — die Liste \
         hat aber {} Einträge",
        PROBEN.len()
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
    // Der `mkfifo`-Fall ist der große: er hängt am `PATH`, nicht am Ziel. Eine
    // Zahl steht hier nicht mehr — die Liste darf auf null schrumpfen, und das
    // ist der Sinn der Übung. Wieviel von ihr behoben ist, hält
    // [`BEHOBENE_MKFIFO`] fest.
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

/// Die fünf behobenen `mkfifo`-Stellen bleiben behoben — und bleiben **laut**.
///
/// [`ALTLASTEN`] deckt sie nicht mehr; ohne diesen Test wäre ein Rückfall auf
/// `.expect("mkfifo startbar")` nur eine neue Verletzung unter vielen, und ein
/// stiller Übersprung fällt überhaupt niemandem auf. Genau das ist die
/// Fehlerklasse dieses Projekts: aus einer Prüfung wird eine Entwarnung.
///
/// Fünf Dinge werden gehalten: das Werkzeug wird noch gestartet (sonst gehört
/// die Zeile hier weg), sein Startergebnis geht über ein `match` (der Ausweg in
/// derselben Anweisung), es wird **nicht** als gegeben genommen, der Übersprung
/// druckt eine Zeile — und hinter dem Ausweg steht **kein `assert` auf den
/// Ausgang**. Das letzte ist neu und war die Lücke: ein Ausweg nur für `Err`
/// und dahinter `assert!(ok.success(), …)` lässt den Lauf bei jedem
/// nichtnull-Ausgang platzen — also im Anlassfall ‚BusyBox ohne `mkfifo`‘, wo
/// `status()` `Ok(exit status: 127)` liefert.
///
/// **Was dieser Test nicht kann:** er liest Zeichenketten. Ein
/// `Err(e) => panic!(…)` erfüllt jede davon und auch die Regel. Die Sache
/// selbst — überspringt die Stelle wirklich, statt zu platzen? — fährt
/// `check_leaks::eine_mkfifo_attrappe_mit_ausgang_127_bricht_den_lauf_nicht` mit
/// einem präparierten `PATH` gegen eine der fünf Stellen. Die anderen vier
/// trägt nur dieser Text; das ist als Rest festgehalten und nicht als
/// Entwarnung.
///
/// Mutation (nachgewiesen): in `crates/redact-core/src/read.rs` das `match`
/// durch `.expect("mkfifo startbar")` ersetzt → dieser Test rot, und nur er
/// sowie `keine_systemeinrichtung_ohne_cfg_oder_ohne_ausweg`. Dazu in
/// `check_leaks.rs` der Ausweg zurück auf `Ok(status) => status` samt
/// `assert!(status.success(), …)` → dieser Test rot (`assert` auf den Ausgang)
/// und der Attrappen-Test ebenfalls (Kindlauf mit `exit status: 101`).
#[test]
fn die_fuenf_behobenen_mkfifo_stellen_haben_ihren_ausweg_und_sagen_ihn() {
    for datei in BEHOBENE_MKFIFO {
        let text = std::fs::read_to_string(repo_root().join(datei))
            .unwrap_or_else(|e| panic!("{datei} lesbar: {e}"));
        assert!(
            text.contains("Command::new(\"mkfifo\")"),
            "{datei} startet kein `mkfifo` mehr — dann gehört die Zeile hier weg"
        );
        assert!(
            text.contains("match std::process::Command::new(\"mkfifo\")")
                || text.contains("match Command::new(\"mkfifo\")"),
            "{datei} führt das Startergebnis nicht mehr über ein `match` — dann gibt es \
             keinen Ausweg in derselben Anweisung"
        );
        assert!(
            !text.contains(".expect(\"mkfifo startbar\")"),
            "{datei} nimmt das Startergebnis wieder als gegeben"
        );
        assert!(
            text.contains("kein mkfifo im Pfad"),
            "{datei} überspringt still — ein übersprungener Test muss sagen, was er \
             nicht geprüft hat und warum"
        );
        // Der Ausweg darf nicht gleich dahinter entwertet werden. Gesucht wird
        // im Fenster **nach** dem Start des Werkzeugs: dort gehört keine
        // Behauptung über seinen Ausgang hin.
        let zeilen: Vec<&str> = text.lines().collect();
        for (i, zeile) in zeilen.iter().enumerate() {
            if !zeile.contains("Command::new(\"mkfifo\")") {
                continue;
            }
            let bis = (i + FENSTER).min(zeilen.len());
            for (nr, spaeter) in zeilen[i..bis].iter().enumerate() {
                assert!(
                    !(spaeter.contains("assert") && spaeter.contains("success")),
                    "{datei}:{} behauptet nach dem Start von `mkfifo` etwas über \
                     seinen Ausgang — dann trägt der Ausweg nur den Fall ‚`mkfifo` \
                     lässt sich nicht starten‘ und nicht den Anlassfall (BusyBox \
                     ohne das Applet, `Ok(exit status: 127)`):\n{}",
                    i + nr + 1,
                    spaeter.trim()
                );
            }
        }
    }
    assert!(
        ALTLASTEN
            .iter()
            .all(|(d, _, _)| !BEHOBENE_MKFIFO.contains(d)),
        "eine behobene Stelle steht noch in ALTLASTEN — dann deckt die Liste mehr, \
         als es gibt"
    );
}
