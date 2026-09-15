//! Die Belege unter `docs/` gegen das gebaute Binary.
//!
//! ## Warum es diesen Test gibt
//!
//! `docs/pruefung.txt` ist die ungekürzte Ausgabe zweier `--check-leaks`-Läufe
//! an der Demo — vor und nach der Schwärzung. `docs/vorher-nachher.md` zitiert
//! daraus die Zeilen `GEFUNDEN (N Fundstelle(n)): …` und `nicht gefunden: …`.
//! Beides entsteht aus `scripts/make-preview.sh`, und beides veraltet still,
//! sobald sich die Nachprüfung ändert: die Belege der letzten Runde stammten
//! aus einer Fassung vor der siebten Sicht von `--check-leaks`, und jede
//! Fundstellenzahl darin war seither um eins zu klein. Niemand hat es gesehen,
//! weil kein Test die Datei mit dem Programm verglich.
//!
//! Dieser Test macht denselben Lauf wie das Skript — `--write-demo`,
//! schwärzen, `--check-leaks` mit den Begriffen aus `BEGRIFFE` in
//! `make-preview.sh` — und verlangt, dass die Belegzeilen und die
//! Rückgabewerte im eingecheckten `pruefung.txt` genau die des gebauten
//! Binaries sind. Ändert sich das Programm, ist der Beleg neu zu erzeugen
//! (`./scripts/make-preview.sh`), und der Test sagt, wann.
//!
//! Verglichen wird der **ganze Berichtsblock** — die `Geprüft:`-Zeile mit der
//! Dateigröße, je Begriff die `GEFUNDEN`/`nicht gefunden`-Zeile und **jede**
//! Fundstellenzeile darunter, samt Byte-Offset, Objektnummer und Ausschnitt.
//!
//! Hier stand bis zur Fix-Runde 5, die Fundstellen ließen sich nicht
//! vergleichen: sie trügen Byte-Offsets, und die hingen am Objektlayout. Das
//! stimmt nicht — dieser Test erzeugt die Demo mit demselben Binary neu, und
//! der Weg ist bitgleich (drei Läufe von `--write-demo` + Schwärzen liefern
//! dieselbe SHA-256-Summe). Die Lücke war teuer: die Gegenprüfung E4 mutierte
//! „1862 Byte“ zu „1863“ und „(Objekt 4 0)“ zu „(Objekt 9 0)“ in
//! `docs/pruefung.txt`, und beide Mutationen blieben grün — obwohl
//! `scripts/check-preview.py` in seinem Kopf zusagt, dieser Test halte
//! `pruefung.txt` am Programm fest.
//!
//! Die Suchbegriffe werden **aus dem Skript gelesen**, nicht hier
//! abgeschrieben — sonst wären es zwei Listen, und dieser Test prüfte die
//! falsche.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Zeilenenden vereinheitlichen.
///
/// Unter Windows checkt git Textdateien mit CRLF aus, wenn `core.autocrlf`
/// gesetzt ist. Die Vergleiche hier suchen Zeilen und Abschnitte mit `\n` —
/// ohne diese Umschrift war der Windows-Job der CI rot, obwohl an den Belegen
/// nichts fehlte. `.gitattributes` hält LF fest; das hier ist der zweite Zaun.
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "redact-belege-{}-{name}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Ein Lauf im Arbeitsverzeichnis `dir`, ohne Einstellungsdatei und ohne
/// Passwort aus der Umgebung — die Umgebung des Testläufers darf den Beleg
/// nicht verschieben.
fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .current_dir(dir)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .expect("Binary startbar")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Die Suchbegriffe aus `BEGRIFFE=( … )` in `scripts/make-preview.sh`, in
/// der Reihenfolge des Skripts.
fn begriffe_aus_dem_skript() -> Vec<String> {
    let skript = std::fs::read_to_string(repo_root().join("scripts/make-preview.sh"))
        .expect("scripts/make-preview.sh lesbar");
    let mut begriffe = Vec::new();
    let mut im_block = false;
    for zeile in skript.lines() {
        let zeile = zeile.trim();
        if zeile == "BEGRIFFE=(" {
            im_block = true;
            continue;
        }
        if im_block {
            if zeile == ")" {
                break;
            }
            if zeile.starts_with('#') {
                continue;
            }
            let begriff = zeile
                .strip_prefix('"')
                .and_then(|z| z.strip_suffix('"'))
                .unwrap_or_else(|| panic!("BEGRIFFE-Zeile ohne Anführungszeichen: {zeile:?}"));
            begriffe.push(begriff.to_string());
        }
    }
    assert!(
        im_block && !begriffe.is_empty(),
        "kein BEGRIFFE-Block in scripts/make-preview.sh gefunden"
    );
    begriffe
}

/// Die Zeilen eines `--check-leaks`-Laufs, die die Doku zitiert: je Begriff
/// genau eine, `GEFUNDEN (N Fundstelle(n)): …` oder `nicht gefunden: …`.
fn belegzeilen(text: &str) -> Vec<String> {
    text.lines()
        .filter(|z| z.starts_with("  GEFUNDEN (") || z.starts_with("  nicht gefunden: "))
        .map(str::to_string)
        .collect()
}

/// Der ganze Berichtsblock eines `--check-leaks`-Laufs: die `Geprüft:`-Zeile
/// mit der Dateigröße, je Begriff seine Belegzeile und **jede**
/// Fundstellenzeile darunter (sechs Leerzeichen eingerückt).
///
/// Die Zeilen des abgedruckten Aufrufs (`      --check-leaks "…" \`) sehen in
/// `pruefung.txt` genauso eingerückt aus wie eine Fundstelle; sie stehen aber
/// **vor** der `Geprüft:`-Zeile, und deshalb beginnt der Block dort.
fn bericht(text: &str) -> Vec<String> {
    text.lines()
        .skip_while(|z| !z.starts_with("Geprüft: "))
        .take_while(|z| !z.starts_with("Ergebnis: "))
        .filter(|z| !z.trim().is_empty())
        .map(str::to_string)
        .collect()
}

/// Der Abschnitt von `pruefung.txt` zwischen der Überschrift `von` und der
/// nächsten Überschrift `bis`.
fn abschnitt<'a>(text: &'a str, von: &str, bis: &str) -> &'a str {
    let start = text
        .find(&format!("\n{von}"))
        .unwrap_or_else(|| panic!("pruefung.txt ohne Abschnitt {von}"));
    let rest = &text[start..];
    let ende = rest
        .find(&format!("\n{bis}"))
        .unwrap_or_else(|| panic!("pruefung.txt: nach {von} fehlt {bis}"));
    &rest[..ende]
}

/// Der in `pruefung.txt` festgehaltene Rückgabewert eines Abschnitts.
fn rueckgabewert(abschnitt: &str) -> i32 {
    abschnitt
        .lines()
        .find_map(|z| z.strip_prefix("Rueckgabewert: "))
        .expect("Abschnitt ohne Rueckgabewert")
        .trim()
        .parse()
        .expect("Rueckgabewert ist eine Zahl")
}

/// `pruefung.txt` nennt je Begriff genau eine Belegzeile, vorher wie
/// nachher, und die Zahlen darin sind die des gebauten Binaries.
#[test]
fn die_fundstellen_in_pruefung_txt_sind_die_des_gebauten_binaries() {
    let begriffe = begriffe_aus_dem_skript();
    let pruefung = lf(
        &std::fs::read_to_string(repo_root().join("docs/pruefung.txt"))
            .expect("docs/pruefung.txt lesbar"),
    );
    let vorher = abschnitt(&pruefung, "VORHER", "NACHHER");
    let nachher = abschnitt(&pruefung, "NACHHER", "DIE VIER SCHRITTE");

    let dir = workdir("fundstellen");
    let demo = run_in(&dir, &["--write-demo", "kontoauszug.pdf"]);
    assert!(demo.status.success(), "--write-demo: {}", stderr(&demo));
    let lauf = run_in(
        &dir,
        &["kontoauszug.pdf", "-o", "kontoauszug_geschwaerzt.pdf"],
    );
    assert!(lauf.status.success(), "Schwärzen: {}", stderr(&lauf));

    let mut args: Vec<&str> = Vec::new();
    for begriff in &begriffe {
        args.push("--check-leaks");
        args.push(begriff);
    }

    for (datei, beleg) in [
        ("kontoauszug.pdf", vorher),
        ("kontoauszug_geschwaerzt.pdf", nachher),
    ] {
        let mut aufruf = vec![datei];
        aufruf.extend_from_slice(&args);
        let out = run_in(&dir, &aufruf);
        let gelaufen = belegzeilen(&stdout(&out));
        let belegt = belegzeilen(beleg);
        assert_eq!(
            gelaufen.len(),
            begriffe.len(),
            "{datei}: je Begriff eine Belegzeile erwartet:\n{}",
            stdout(&out)
        );
        assert_eq!(
            belegt, gelaufen,
            "docs/pruefung.txt ({datei}) sagt anderes als das gebaute Binary — \
             `./scripts/make-preview.sh` neu laufen lassen"
        );

        // Und derselbe Vergleich über den ganzen Block: Dateigröße und jede
        // Fundstellenzeile. Ohne ihn bleiben „1862 Byte“ und „(Objekt 4 0)“
        // ungebunden (Gegenprüfung E4).
        let gelaufen_block = bericht(&stdout(&out));
        let belegter_block = bericht(beleg);
        assert!(
            belegter_block.first().is_some_and(
                |z| z.starts_with(&format!("Geprüft: {datei} (")) && z.ends_with(" Byte)")
            ),
            "docs/pruefung.txt ({datei}) ohne Geprüft-Zeile mit Dateigröße: {belegter_block:?}"
        );
        assert!(
            belegter_block.len() > begriffe.len() + 1,
            "docs/pruefung.txt ({datei}) zitiert keine einzige Fundstellenzeile: \
             {belegter_block:?}"
        );
        assert_eq!(
            belegter_block, gelaufen_block,
            "docs/pruefung.txt ({datei}) sagt anderes als das gebaute Binary — \
             Dateigröße oder Fundstelle weicht ab; \
             `./scripts/make-preview.sh` neu laufen lassen"
        );
        assert_eq!(
            Some(rueckgabewert(beleg)),
            out.status.code(),
            "docs/pruefung.txt ({datei}) nennt einen anderen Rückgabewert"
        );
    }

    // Sonst prüfte der Test zwei leere Listen gegeneinander — und die
    // Aussage der Datei ist genau: vorher alles drin, nachher der Name.
    assert!(
        belegzeilen(vorher)
            .iter()
            .all(|z| z.starts_with("  GEFUNDEN")),
        "VORHER: alle Begriffe stehen in der Demo"
    );
    assert!(
        belegzeilen(nachher)
            .iter()
            .any(|z| z.starts_with("  nicht gefunden: ")),
        "NACHHER: mindestens ein Begriff ist weg"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Die erste Zeile von `pruefung.txt` nennt die Fassung, die den Beleg
/// erzeugt hat — und das muss die gebaute sein. Ein Beleg aus einer älteren
/// Fassung sagt nichts über diese.
#[test]
fn pruefung_txt_stammt_von_dieser_fassung() {
    let pruefung = lf(
        &std::fs::read_to_string(repo_root().join("docs/pruefung.txt"))
            .expect("docs/pruefung.txt lesbar"),
    );
    let version = stdout(&run_in(&repo_root(), &["--version"]));
    let version = version.trim();
    assert!(!version.is_empty(), "--version schweigt");
    let erwartet = format!("Erzeugt von scripts/make-preview.sh mit {version}.");
    assert_eq!(
        pruefung.lines().next().unwrap_or_default(),
        erwartet,
        "docs/pruefung.txt stammt aus einer anderen Fassung — \
         `./scripts/make-preview.sh` neu laufen lassen"
    );
}

/// Die Zahl der Prüfungen aus `scripts/check-preview.py` steht in der README —
/// und zwar die, die das Skript wirklich druckt.
///
/// Gegenprüfung E5 der Fix-Runde 5: die README nannte „36 Prüfungen“, das
/// Skript meldete 54. Eine abgeschriebene Zahl veraltet still. Jetzt liest
/// dieser Test die letzte Zeile des Skriptlaufs (`Alle N Pruefungen
/// bestanden.`) und verlangt genau dieses `N` im Satz der README.
///
/// Ohne `python3` im Pfad ist hier nichts zu prüfen — der Test sagt das und
/// endet grün; im Gate läuft `python3 scripts/check-preview.py docs` ohnehin
/// als eigener Schritt.
///
/// Mutation (nachgewiesen): „54 Prüfungen“ in der README auf „55“ gesetzt →
/// dieser Test rot.
#[test]
fn die_zahl_der_pruefungen_steht_im_readme() {
    let wurzel = repo_root();
    let out = match Command::new("python3")
        .arg(wurzel.join("scripts/check-preview.py"))
        .arg(wurzel.join("docs"))
        .current_dir(&wurzel)
        .stdin(Stdio::null())
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            eprintln!("kein python3 im Pfad ({e}) — die Zahl bleibt hier ungeprüft");
            return;
        }
    };
    let text = stdout(&out);
    assert!(
        out.status.success(),
        "check-preview.py ist nicht grün:\n{text}{}",
        stderr(&out)
    );
    let gemeldet = text
        .lines()
        .find_map(|z| {
            z.strip_prefix("Alle ")
                .and_then(|z| z.strip_suffix(" Pruefungen bestanden."))
        })
        .unwrap_or_else(|| panic!("check-preview.py meldet keine Zahl:\n{text}"));

    let readme = lf(&std::fs::read_to_string(wurzel.join("README.md")).expect("README.md lesbar"));
    let satz = format!("# prüft sie ({gemeldet} Prüfungen");
    assert!(
        readme.contains(&satz),
        "README.md nennt nicht „{satz}…)“ — check-preview.py meldet {gemeldet} Prüfungen"
    );
}

/// **Die Regel hinter dem zweiten Zaun.** Zweimal war der Windows-Job der CI
/// rot, weil ein Test eine Datei des Repositorys wörtlich las und git sie dort
/// mit CRLF ausgecheckt hatte. `.gitattributes` hält für jede Art von Textdatei
/// LF fest; dieser Test hält `.gitattributes` fest.
///
/// Gemessen wird nicht der Inhalt der Regel, sondern ihre **Wirkung**: für jede
/// Endung, die ein Test hier oder in `redact-cli` wörtlich liest, muss eine
/// Zeile `eol=lf` dastehen. Wird eine gestrichen, wird dieser Test rot — und
/// nicht erst der Job auf einem fremden Betriebssystem.
#[test]
fn jede_gelesene_dateiart_wird_mit_lf_ausgecheckt() {
    let attrs = lf(&std::fs::read_to_string(repo_root().join(".gitattributes"))
        .expect(".gitattributes lesbar"));
    for endung in ["md", "txt", "rs", "toml", "yml", "sh", "py"] {
        let zeile = format!("*.{endung} text eol=lf");
        assert!(
            attrs.lines().any(|l| l.trim() == zeile),
            ".gitattributes ohne „{zeile}“ — eine Datei dieser Art wird von einem \
             Test wörtlich gelesen und käme unter Windows mit CRLF an"
        );
    }
}

// ===========================================================================
// Die Messzahlen der Doku — an EINER Stelle
// ===========================================================================
//
// ## Warum es diesen Block gibt
//
// Die Gegenprüfung der Fix-Runde 5 hat siebzehn Zahlen und Sätze in README,
// `SECURITY.md` und `CHANGELOG.md` in **einem** Lauf mutiert — `5,20 s` zu
// `9,99 s`, `675 MB` zu `42 MB`, `2 172 628 kB ≈ 2,1 GiB` zu
// `42 kB ≈ 0,1 GiB`, `24 MB, 0,02 s, Exit 1` zu `77 MB, 9,02 s, Exit 3`,
// `siebenmal` zu einer anderen Zahl, den ganzen Gründe-Satz — und **kein
// einziger Test wurde rot**. Die Runde hatte zwei ungebundene „1 000“
// geschlossen (`the_needle_ceiling_is_one_number_in_code_help_and_docs`) und
// dabei mindestens sieben neue ungebundene Zahlen angelegt.
//
// ## Wie gebunden wird, ohne bei jeder Messung zu brechen
//
// Eine Messung ist keine Konstante: wer auf einer anderen Maschine misst,
// bekommt andere Sekunden, und ein Test, der die Sekunden vorschreibt, wäre
// eine Zusage, die das Projekt nicht geben kann. Gebunden wird deshalb die
// **Stelle**:
//
// * Jede Messzahl steht **hier** einmal als Konstante ([`messwerte`]).
// * Aus diesen Konstanten baut [`messsaetze`] den Satz, wie er in der Doku
//   steht. Der Test verlangt ihn dort wörtlich (Zeilenumbrüche geglättet).
// * Wird die Zahl in der Doku geändert, findet der Test den Satz nicht mehr
//   und nennt Datei und Satz. Wird sie **hier** geändert, ebenso — eine neue
//   Messung ist damit eine Änderung an zwei Stellen, und beide sieht der
//   Leser des Diffs nebeneinander.
// * Was sich **ableiten** lässt, wird abgeleitet und nicht abgeschrieben:
//   das Verhältnis aus zwei Zeiten, die MB aus den gemessenen kB, die
//   Bytegrenze aus [`redact_core::MAX_AUX_FILE_BYTES`], die Zahl der
//   Kodierungen aus einem **Lauf** des gebauten Binaries.
//
// Mutationsnachweis: siehe `die_zahlen_der_doku_sind_gebunden` — jede Stelle
// dieser Liste wurde einzeln in der Doku mutiert, jede machte den Test rot.

/// Jede Zahl, die die Doku als **Messung** nennt — einmal.
///
/// Zahlen ohne Einheit sind ganze Zahlen; Zahlen mit Komma stehen als
/// Zeichenkette, weil die Doku sie mit deutschem Dezimalkomma schreibt und
/// eine Umrechnung nur eine weitere Stelle wäre, an der etwas auseinander
/// laufen kann.
mod messwerte {
    // Kosten des Automaten (Fix-Runde 4/5, `zd_mess_1000_begriffe_kosten_wie_einer`)
    pub const EIN_BEGRIFF_S: &str = "5,20";
    pub const TAUSEND_BEGRIFFE_S: &str = "6,27";
    pub const VERHAELTNIS: &str = "1,21";
    pub const AUTOMAT_1K_MB: &str = "1,0";
    pub const AUTOMAT_10K_MB: &str = "7,1";
    pub const AUTOMAT_100K_MB: &str = "68";
    pub const AUTOMAT_1M_MB: &str = "675";
    pub const AUTOMAT_BAU_S: &str = "25";
    /// Die nachgestellte alte Kostenrechnung (Einzelsuchen mit `memmem`).
    pub const MEMMEM_EINZELSUCHEN: usize = 12_000;
    pub const MEMMEM_S: &str = "89,2";
    pub const MEMMEM_ZWOELF_S: &str = "0,08";
    pub const AUTOMAT_DURCHGANG_S: &str = "0,28";

    // Spitzenspeicher der Bomben (`getrusage(RUSAGE_CHILDREN).ru_maxrss`, kB
    // = 1024 Byte). Die MB-Angaben der Doku werden daraus **abgeleitet**.
    pub const BOMBE_PRUEFEN_KB: u64 = 2_172_628;
    pub const BOMBE_SCHWAERZEN_KB: u64 = 2_172_312;
    pub const BOMBE_OBJSTM_VORGABE_KB: u64 = 1_056_688;
    pub const BOMBE_OBJSTM_KB: u64 = 3_220_164;
    pub const BOMBE_KLEIN_MB: &str = "24";
    pub const BOMBE_KLEIN_S: &str = "0,02";
    pub const BOMBE_S: &str = "18";

    // Vorprüfung: ein Altlast-Filter über der Rohgrenze.
    pub const ALTLAST_ROH_BYTES: u64 = 17_825_795;

    // Tiefe der Objektsicht (Fix-Runde 5, `ze_p1_mess_tiefe_kostet`)
    pub const TIEFE_BREITE: &str = "1 000";
    pub const TIEFE_32_MS: &str = "35,4";
    pub const TIEFE_100_MS: &str = "57,9";

    // Metadatenlauf ohne Tiefengrenze (Fix-Runde 5, `mess_p3_3_grosse_formularbaeume`)
    pub const FELDER: &str = "100 000";
    pub const FELDER_S: &str = "0,40";
    pub const KETTE: &str = "1 000 000";
    pub const KETTE_S: &str = "1,65";

    // UTF-16LE im Automaten (Fix-Runde 5, `ze_p1_mess_kosten_je_kodierung`)
    pub const LE_BEGRIFFE: &str = "200";
    pub const LE_VORHER_MS: &str = "31,2–32,2";
    pub const LE_NACHHER_MS: &str = "29,3–34,4";

    // Spiegel über mehrfach platzierten Formularen (Fix-Runde 5)
    /// Der Text des Spiegels — aus ihm werden die beiden Zahlen des Satzes
    /// „10 Zeichen im Spiegel, 5 in den Glyphen“ abgeleitet.
    pub const SPIEGELTEXT: &str = "AlphaAlpha";
    pub const GLYPHENTEXT: &str = "Alpha";
    pub const PLATZIERUNGEN: &str = "100 000";
    pub const PLATZIERUNGEN_MB: &str = "8,5";
    pub const PLATZIERUNGEN_S: &str = "0,3";

    // Oberfläche (Fix-Runde 5, `ze_p5_1_jede_schreibweise_wird_gesucht`)
    pub const IBAN_FUNDE: &str = "siebenmal";

    // Kein Kernabzug (Fix-Runde 3)
    pub const CORE_BYTES: &str = "454 656";

    // Die Spiegel-Formular-Liste ohne Decke (Fix-Runde 6)
    pub const SPIEGEL_BOMBE_KB: &str = "276";
    pub const SPIEGEL_BOMBE_MB: u64 = 2_306;
    pub const SPIEGEL_BOMBE_S: &str = "41,7";
    pub const SPIEGEL_FAKTOR: &str = "8 400";
    pub const ZUORDNUNGEN: &str = "100 000";
    pub const ZUORDNUNGEN_MB: &str = "16";
}

/// „1 000“ statt „1000“ — Leerzeichen als Tausendertrenner, wie die Doku es
/// schreibt.
fn mit_tausendertrenner(n: u64) -> String {
    let ziffern = n.to_string();
    let mut aus = String::new();
    for (i, z) in ziffern.chars().enumerate() {
        if i > 0 && (ziffern.len() - i).is_multiple_of(3) {
            aus.push(' ');
        }
        aus.push(z);
    }
    aus
}

/// kB (1024 Byte, so meldet es `VmHWM`) in MB (1024² Byte, so rechnet dieses
/// Projekt überall) — die Umrechnung, die die Bombentabelle bis zur
/// Fix-Runde 6 mit 1000 machte, während der Fließtext darüber mit 1024²
/// rechnete.
fn kb_in_mb(kb: u64) -> u64 {
    (kb as f64 / 1024.0).round() as u64
}

fn kb_in_gib(kb: u64) -> String {
    format!("{:.1}", kb as f64 / (1024.0 * 1024.0)).replace('.', ",")
}

fn mb_in_gib(mb: u64) -> String {
    format!("{:.1}", mb as f64 / 1024.0).replace('.', ",")
}

/// Zeilenumbrüche und Einrückung glätten.
fn glatt(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Die Sätze der Doku, in denen eine Messzahl steht — Datei und Wortlaut.
///
/// Der Wortlaut wird aus [`messwerte`] zusammengesetzt; nichts hier ist eine
/// zweite Abschrift einer Zahl.
fn messsaetze() -> Vec<(&'static str, String)> {
    use messwerte as m;
    let mut aus: Vec<(&'static str, String)> = Vec::new();
    let mut satz = |datei: &'static str, text: String| aus.push((datei, text));

    // --- Kosten des Automaten -------------------------------------------
    satz(
        "README.md",
        format!(
            "nachgemessen an einer 64-MiB-Datei: {} s für einen, {} s für 1 000, \
             Verhältnis {}",
            m::EIN_BEGRIFF_S,
            m::TAUSEND_BEGRIFFE_S,
            m::VERHAELTNIS
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "1 Begriff **{} s**, 1 000 Begriffe **{} s** — Verhältnis {}",
            m::EIN_BEGRIFF_S,
            m::TAUSEND_BEGRIFFE_S,
            m::VERHAELTNIS
        ),
    );
    satz(
        "README.md",
        format!(
            "nachgemessen {} MB für 1 000 Begriffe (bis zu {} Muster",
            m::AUTOMAT_1K_MB,
            // Neun Byte-Kodierungen **und** der dekodierte Text je Begriff —
            // die Zahl kommt aus der Liste, nicht aus der Doku.
            mit_tausendertrenner(
                (redact_core::MAX_CHECK_NEEDLES * (BYTE_KODIERUNGEN_ASCII_ERWARTET.len() + 1))
                    as u64
            ),
        ),
    );
    satz(
        "README.md",
        format!(
            "{} MB für 10 000, {} MB für 100 000 und **{} MB** für eine Million — \
             dazu {} s allein für seinen Bau",
            m::AUTOMAT_10K_MB,
            m::AUTOMAT_100K_MB,
            m::AUTOMAT_1M_MB,
            m::AUTOMAT_BAU_S
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "({} MB bei 1 000 Begriffen, {} MB bei 10 000, {} MB bei 100 000, \
             **{} MB** bei einer Million, dazu {} s allein für seinen Bau)",
            m::AUTOMAT_1K_MB,
            m::AUTOMAT_10K_MB,
            m::AUTOMAT_100K_MB,
            m::AUTOMAT_1M_MB,
            m::AUTOMAT_BAU_S
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "**{} Einzelsuchen** mit `memmem` über 64 MiB kosten **{} s**, zwölf \
             davon {} s; derselbe Durchgang mit einem Automaten kostet {} s",
            mit_tausendertrenner(m::MEMMEM_EINZELSUCHEN as u64),
            m::MEMMEM_S,
            m::MEMMEM_ZWOELF_S,
            m::AUTOMAT_DURCHGANG_S
        ),
    );

    // --- Spitzenspeicher der Bomben --------------------------------------
    satz(
        "SECURITY.md",
        format!(
            "`VmHWM` von {} kB, also **{} MB** ≈ {} GiB",
            mit_tausendertrenner(m::BOMBE_PRUEFEN_KB),
            mit_tausendertrenner(kb_in_mb(m::BOMBE_PRUEFEN_KB)),
            kb_in_gib(m::BOMBE_PRUEFEN_KB)
        ),
    );
    satz(
        "SECURITY.md",
        format!(
            "dieselbe Spitze ({} kB = {} MB)",
            mit_tausendertrenner(m::BOMBE_SCHWAERZEN_KB),
            mit_tausendertrenner(kb_in_mb(m::BOMBE_SCHWAERZEN_KB))
        ),
    );
    satz(
        "SECURITY.md",
        format!(
            "Rückgabewert 1, `VmHWM` {} kB = {} MB); wer ihm mit \
             `--max-decompressed-mb 4096` Luft gibt, misst {} kB = **{} MB** ≈ {} GiB",
            mit_tausendertrenner(m::BOMBE_OBJSTM_VORGABE_KB),
            mit_tausendertrenner(kb_in_mb(m::BOMBE_OBJSTM_VORGABE_KB)),
            mit_tausendertrenner(m::BOMBE_OBJSTM_KB),
            mit_tausendertrenner(kb_in_mb(m::BOMBE_OBJSTM_KB)),
            kb_in_gib(m::BOMBE_OBJSTM_KB)
        ),
    );
    satz(
        "SECURITY.md",
        format!(
            "| als Seiteninhalt | {klein} MB, {klein_s} s, **Exit 1** | {gross} MB, {s} s, \
             Exit 0 | | als Objektstrom (`/ObjStm`) | {klein} MB, {klein_s} s, **Exit 1** | \
             {objstm} MB, {s} s, Exit 0 |",
            klein = m::BOMBE_KLEIN_MB,
            klein_s = m::BOMBE_KLEIN_S,
            gross = mit_tausendertrenner(kb_in_mb(m::BOMBE_PRUEFEN_KB)),
            objstm = mit_tausendertrenner(kb_in_mb(m::BOMBE_OBJSTM_KB)),
            s = m::BOMBE_S,
        ),
    );
    satz(
        "SECURITY.md",
        format!(
            "stabil auf sechs Stellen ({} kB bzw. {} kB, also {} MB bzw. {} MB)",
            mit_tausendertrenner(m::BOMBE_PRUEFEN_KB),
            mit_tausendertrenner(m::BOMBE_OBJSTM_KB),
            mit_tausendertrenner(kb_in_mb(m::BOMBE_PRUEFEN_KB)),
            mit_tausendertrenner(kb_in_mb(m::BOMBE_OBJSTM_KB))
        ),
    );
    satz(
        "SECURITY.md",
        format!(
            "mit 16 MB bleibt derselbe Lauf bei {} MB (Rückgabewert 1",
            m::BOMBE_KLEIN_MB
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "`VmHWM` von {} kB = {} MB ≈ {} GiB — dieselbe Spitze wie beim Schwärzen \
             ({} kB = {} MB)",
            mit_tausendertrenner(m::BOMBE_PRUEFEN_KB),
            mit_tausendertrenner(kb_in_mb(m::BOMBE_PRUEFEN_KB)),
            kb_in_gib(m::BOMBE_PRUEFEN_KB),
            mit_tausendertrenner(m::BOMBE_SCHWAERZEN_KB),
            mit_tausendertrenner(kb_in_mb(m::BOMBE_SCHWAERZEN_KB))
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "beide Formen nach {} s bei {} MB ab (Rückgabewert 1); mit \
             `--max-decompressed-mb 4096` — dem Lauf ohne wirksame Grenze — steigt die \
             Spitze auf **{} MB** (Seiteninhalt) bzw. **{} MB** (`/ObjStm`), je rund {} s \
             ({} bzw. {} kB",
            m::BOMBE_KLEIN_S,
            m::BOMBE_KLEIN_MB,
            mit_tausendertrenner(kb_in_mb(m::BOMBE_PRUEFEN_KB)),
            mit_tausendertrenner(kb_in_mb(m::BOMBE_OBJSTM_KB)),
            m::BOMBE_S,
            mit_tausendertrenner(m::BOMBE_PRUEFEN_KB),
            mit_tausendertrenner(m::BOMBE_OBJSTM_KB)
        ),
    );

    // --- Vorprüfung: Altlast-Filter über der Rohgrenze --------------------
    satz(
        "SECURITY.md",
        format!(
            "ist mit {} Bytes zu groß (Grenze {} Bytes)",
            mit_tausendertrenner(m::ALTLAST_ROH_BYTES),
            mit_tausendertrenner(redact_core::MAX_AUX_FILE_BYTES)
        ),
    );

    // --- Tiefe der Objektsicht -------------------------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "Tiefe 99 bei Breite {} kostet {} ms gegen {} ms bei einer Grenze von 100",
            m::TIEFE_BREITE,
            m::TIEFE_32_MS,
            m::TIEFE_100_MS
        ),
    );

    // --- Metadatenlauf ----------------------------------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "Gemessen (Release): {} Felder {} s, eine `/Parent`-Kette aus {} Feldern {} s",
            m::FELDER,
            m::FELDER_S,
            m::KETTE,
            m::KETTE_S
        ),
    );

    // --- UTF-16LE ---------------------------------------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "(8 MiB entpackt, {} Begriffe: {} ms vorher, {} ms nachher)",
            m::LE_BEGRIFFE,
            m::LE_VORHER_MS,
            m::LE_NACHHER_MS
        ),
    );

    // --- Spiegel über mehrfach platzierten Formularen ---------------------
    satz(
        "CHANGELOG.md",
        format!(
            "`/Span <</ActualText ({})>> BDC /Fm0 Do /Fm0 Do EMC`: die Schließung \
             entdoppelte über Objekt-Ids und zählte die Glyphen halb — „{} Zeichen im \
             Spiegel, {} in den Glyphen“",
            m::SPIEGELTEXT,
            m::SPIEGELTEXT.chars().count(),
            m::GLYPHENTEXT.chars().count()
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "höchstens {} Formularplatzierungen unter den Spiegeln einer Seite \
             (Mehrkosten gemessen: {} MB, unter {} s)",
            m::PLATZIERUNGEN,
            m::PLATZIERUNGEN_MB,
            m::PLATZIERUNGEN_S
        ),
    );

    // --- Oberfläche -------------------------------------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "über eine Datei, in der die IBAN noch **{}** stand",
            m::IBAN_FUNDE
        ),
    );

    // --- Kein Kernabzug ---------------------------------------------------
    satz("SECURITY.md", format!("| `core`, {} Byte |", m::CORE_BYTES));
    satz(
        "CHANGELOG.md",
        format!("eine {} Byte große `core`-Datei", m::CORE_BYTES),
    );

    // --- Die Spiegel-Formular-Liste ohne Decke (Fix-Runde 6) --------------
    satz(
        "SECURITY.md",
        format!(
            "höchstens {} Zuordnungen zwischen einem Textspiegel und einer \
             Formularplatzierung beim Aufbau der Liste und ebenso viele beim \
             Aufklappen, zusammen rund {} MB",
            m::ZUORDNUNGEN,
            m::ZUORDNUNGEN_MB
        ),
    );
    satz(
        "SECURITY.md",
        format!(
            "eine Datei von {} kB machte daraus {} MB Spitzenspeicher und {} s \
             (Faktor {})",
            m::SPIEGEL_BOMBE_KB,
            mit_tausendertrenner(m::SPIEGEL_BOMBE_MB),
            m::SPIEGEL_BOMBE_S,
            m::SPIEGEL_FAKTOR
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "eine getaggte Seite von {} kB, die {} s und {} GB Arbeitsspeicher kostete",
            m::SPIEGEL_BOMBE_KB,
            m::SPIEGEL_BOMBE_S,
            mb_in_gib(m::SPIEGEL_BOMBE_MB)
        ),
    );

    // --- Und derselbe Kostensatz im Hilfetext des Binaries ---------------
    satz(
        "--help",
        format!(
            "1 000 kosten kaum mehr als einer (gemessen an 64 MiB: {} s gegen {} s)",
            m::EIN_BEGRIFF_S,
            m::TAUSEND_BEGRIFFE_S
        ),
    );
    satz(
        "--help",
        format!(
            "gemessen {} MB für 1 000 Begriffe, {} MB und {} s allein für den Bau bei \
             einer Million",
            m::AUTOMAT_1K_MB,
            m::AUTOMAT_1M_MB,
            m::AUTOMAT_BAU_S
        ),
    );

    // --- Der Eintrag der Fix-Runde 6 zitiert seine eigenen Zahlen ---------
    satz(
        "CHANGELOG.md",
        format!(
            "das Verhältnis {} aus {} s und {} s, die MB aus den gemessenen kB, die \
             Grenze {} aus `redact_core::MAX_AUX_FILE_BYTES`",
            m::VERHAELTNIS,
            m::TAUSEND_BEGRIFFE_S,
            m::EIN_BEGRIFF_S,
            mit_tausendertrenner(redact_core::MAX_AUX_FILE_BYTES)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "die Tabelle nennt jetzt **{} MB** und **{} MB**",
            mit_tausendertrenner(kb_in_mb(m::BOMBE_PRUEFEN_KB)),
            mit_tausendertrenner(kb_in_mb(m::BOMBE_OBJSTM_KB))
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "„{} Zeichen im Spiegel, {} in den Glyphen“ aus dem Spiegeltext `{}`",
            m::SPIEGELTEXT.chars().count(),
            m::GLYPHENTEXT.chars().count(),
            m::SPIEGELTEXT
        ),
    );

    aus
}

/// **Die Bindung.** Jeder Satz aus [`messsaetze`] steht genau so in seiner
/// Datei — und jeder trägt eine Ziffer, sonst bindet er nichts.
///
/// Mutationsnachweis: siehe Modulkommentar oben.
#[test]
fn die_zahlen_der_doku_sind_gebunden() {
    let wurzel = repo_root();
    let mut inhalt: std::collections::BTreeMap<&str, String> = std::collections::BTreeMap::new();
    let mut fehlend: Vec<String> = Vec::new();

    let saetze = messsaetze();
    assert!(
        saetze.len() >= 20,
        "die Liste ist geschrumpft — {} Sätze",
        saetze.len()
    );

    for (datei, satz) in &saetze {
        // Ein Eintrag ohne Zahl bindet nichts. „siebenmal“ ist eine — die
        // Doku schreibt kleine Zahlen aus, und genau dieses Wort war eine der
        // siebzehn Mutationen, die niemand rot machte.
        const ZAHLWORTE: [&str; 12] = [
            "zwei", "drei", "vier", "fünf", "sechs", "sieben", "acht", "neun", "zehn", "elf",
            "zwölf", "einmal",
        ];
        assert!(
            satz.chars().any(|c| c.is_ascii_digit()) || ZAHLWORTE.iter().any(|w| satz.contains(w)),
            "„{satz}“ trägt keine Zahl und bindet nichts"
        );
        // „--help“ ist keine Datei, sondern der Hilfetext des gebauten
        // Binaries: dieselben Messzahlen stehen dort noch einmal.
        let text = inhalt.entry(datei).or_insert_with(|| {
            if *datei == "--help" {
                glatt(&stdout(&run_in(&wurzel, &["--help"])))
            } else {
                glatt(&lf(&std::fs::read_to_string(wurzel.join(datei))
                    .unwrap_or_else(|e| panic!("{datei}: {e}"))))
            }
        });
        if !text.contains(satz.as_str()) {
            fehlend.push(format!("{datei}: „{satz}“"));
        }
    }

    assert!(
        fehlend.is_empty(),
        "{} Messzahl(en) stehen in der Doku anders als in `messwerte` — \
         entweder wurde die Doku umgeschrieben, ohne neu zu messen, oder die \
         Messung ist neu und `messwerte` ist nicht nachgezogen:\n{}",
        fehlend.len(),
        fehlend.join("\n")
    );
}

/// Was sich ableiten lässt, wird abgeleitet — und die Doku muss die
/// abgeleitete Zahl tragen.
///
/// Fünf Ableitungen: das Verhältnis zweier Zeiten, die MB aus den gemessenen
/// kB, die Bytegrenze aus [`redact_core::MAX_AUX_FILE_BYTES`], die Zahl der
/// Zeichen aus dem Spiegeltext, und die Decke der benannten Stellen aus dem
/// Quelltext der Oberfläche.
#[test]
fn die_abgeleiteten_zahlen_stimmen() {
    use messwerte as m;

    // (1) Verhältnis: 6,27 / 5,20 = 1,21 — kein drittes Literal.
    let komma = |s: &str| s.replace(',', ".").parse::<f64>().expect("Zahl mit Komma");
    let verhaeltnis = format!(
        "{:.2}",
        komma(m::TAUSEND_BEGRIFFE_S) / komma(m::EIN_BEGRIFF_S)
    )
    .replace('.', ",");
    assert_eq!(
        verhaeltnis,
        m::VERHAELTNIS,
        "das Verhältnis in der Doku passt nicht zu den beiden Zeiten"
    );

    // (2) MB aus kB: 2 172 628 kB sind 2 122 MB, nicht 2 173.
    assert_eq!(kb_in_mb(m::BOMBE_PRUEFEN_KB), 2_122);
    assert_eq!(kb_in_mb(m::BOMBE_OBJSTM_KB), 3_145);
    assert_ne!(
        kb_in_mb(m::BOMBE_PRUEFEN_KB),
        m::BOMBE_PRUEFEN_KB / 1_000,
        "wenn beide Teilungen dasselbe ergäben, prüfte diese Bindung nichts"
    );

    // (3) Die Grenze der Vorprüfung ist 16 MB, und die Datei liegt darüber.
    assert_eq!(redact_core::MAX_AUX_FILE_BYTES, 16 * 1024 * 1024);
    const {
        assert!(m::ALTLAST_ROH_BYTES > redact_core::MAX_AUX_FILE_BYTES);
    }

    // (4) „10 Zeichen im Spiegel, 5 in den Glyphen“ steckt im Spiegeltext.
    assert_eq!(m::SPIEGELTEXT.chars().count(), 10);
    assert_eq!(m::GLYPHENTEXT.chars().count(), 5);
    assert_eq!(
        m::SPIEGELTEXT,
        format!("{}{}", m::GLYPHENTEXT, m::GLYPHENTEXT),
        "der Spiegeltext ist nicht mehr das zweifach gezeichnete Formular"
    );

    // (5) „bis zu drei davon beim Namen“ — die Zahl steht im Quelltext der
    // Oberfläche und darf nicht abgeschrieben werden.
    let state = lf(
        &std::fs::read_to_string(repo_root().join("crates/redact-gui/src/state.rs"))
            .expect("crates/redact-gui/src/state.rs lesbar"),
    );
    let zeile = state
        .lines()
        .find_map(|z| z.trim().strip_prefix("const MAX_NAMED_PLACES: usize = "))
        .expect("MAX_NAMED_PLACES steht nicht mehr in state.rs");
    let genannte: usize = zeile
        .trim_end_matches(';')
        .parse()
        .expect("MAX_NAMED_PLACES ist eine Zahl");
    let als_wort = match genannte {
        3 => "drei",
        n => panic!("MAX_NAMED_PLACES ist jetzt {n} — die Doku sagt „drei“"),
    };
    let readme = glatt(&lf(
        &std::fs::read_to_string(repo_root().join("README.md")).expect("README.md lesbar")
    ));
    assert!(
        readme.contains(&format!(
            "und nennt **bis zu {als_wort} davon** beim Namen samt Grund"
        )),
        "die README verspricht mehr, als `MAX_NAMED_PLACES` hält"
    );
    let security = glatt(&lf(&std::fs::read_to_string(
        repo_root().join("SECURITY.md"),
    )
    .expect("SECURITY.md lesbar")));
    assert!(
        security.contains(&format!(
            "höchstens {als_wort} beim Namen, der Rest gezählt („… und N weitere“); \
             `MAX_NAMED_PLACES = {genannte}`"
        )),
        "SECURITY.md nennt die Decke der benannten Stellen nicht"
    );
}

// ---------------------------------------------------------------------------
// E4 — die Kodierungen, in denen gesucht wird
// ---------------------------------------------------------------------------

/// Die **neun** Byte-Kodierungen, in denen ein Begriff aus reinem ASCII
/// gesucht wird — so, wie das Programm sie in der Fundstelle benennt.
///
/// `--help` und die README zählten bis zur Fix-Runde 6 nur „UTF-8,
/// Latin-1/PDFDoc, UTF-16BE und als Hex-String“ — vier Namen für neun Muster,
/// und UTF-16LE fehlte ganz, obwohl die Fix-Runde 5 es eingebaut hat. Diese
/// Liste ist deshalb **nicht** aus `audit_bytes.rs` abgeschrieben: der Test
/// unten legt jede der neun Bytefolgen in eine Datei und verlangt, dass der
/// Lauf genau diese neun Namen meldet — nicht mehr und nicht weniger.
const BYTE_KODIERUNGEN_ASCII_ERWARTET: [&str; 9] = [
    "UTF-8/ASCII",
    "Hex-String (Latin-1, gross)",
    "Hex-String (Latin-1, klein)",
    "UTF-16BE",
    "Hex-String (UTF-16BE, gross)",
    "Hex-String (UTF-16BE, klein)",
    "UTF-16LE",
    "Hex-String (UTF-16LE, gross)",
    "Hex-String (UTF-16LE, klein)",
];

fn hex(bytes: &[u8], gross: bool) -> Vec<u8> {
    let ziffern: &[u8; 16] = if gross {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut aus = Vec::with_capacity(bytes.len() * 2);
    for b in bytes {
        aus.push(ziffern[(b >> 4) as usize]);
        aus.push(ziffern[(b & 0x0f) as usize]);
    }
    aus
}

/// Jede Bytefassung von `text`, die in einer PDF-Datei stehen kann — dieselbe
/// Aufzählung, die `Needle::new` baut, hier von Hand nachgebaut.
fn alle_bytefassungen(text: &str) -> Vec<Vec<u8>> {
    let mut aus: Vec<Vec<u8>> = vec![text.as_bytes().to_vec()];
    if text.chars().all(|c| (c as u32) < 0x100) {
        let latin1: Vec<u8> = text.chars().map(|c| c as u8).collect();
        aus.push(latin1.clone());
        aus.push(hex(&latin1, true));
        aus.push(hex(&latin1, false));
    }
    let be: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    aus.push(be.clone());
    aus.push(hex(&be, true));
    aus.push(hex(&be, false));
    let le: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    aus.push(le.clone());
    aus.push(hex(&le, true));
    aus.push(hex(&le, false));
    aus.dedup();
    aus
}

/// Eine PDF-Datei aus fertigen Objektrümpfen, mit Querverweistabelle.
fn zusammensetzen(objekte: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut aus = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (id, rumpf) in objekte {
        offsets.push(aus.len());
        aus.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        aus.extend_from_slice(rumpf);
        aus.extend_from_slice(b"\nendobj\n");
    }
    let xref = aus.len();
    aus.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objekte.len() + 1).as_bytes(),
    );
    for offset in &offsets {
        aus.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    aus.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objekte.len() + 1
        )
        .as_bytes(),
    );
    aus
}

/// Eine Datei, in deren ungefiltertem Strom **jede** Bytefassung von `text`
/// steht.
fn pdf_mit_allen_fassungen(text: &str) -> Vec<u8> {
    let mut inhalt = b"BT ET\n".to_vec();
    for (i, fassung) in alle_bytefassungen(text).iter().enumerate() {
        inhalt.extend_from_slice(format!("% {i} ").as_bytes());
        inhalt.extend_from_slice(fassung);
        inhalt.push(b'\n');
    }
    let mut strom = format!("<< /Length {} >>\nstream\n", inhalt.len()).into_bytes();
    strom.extend_from_slice(&inhalt);
    strom.extend_from_slice(b"\nendstream");
    zusammensetzen(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, strom),
    ])
}

/// Die Kodierungsnamen, die ein Lauf in seinen `Rohdatei @0x…`-Fundstellen
/// nennt — dort steht der Name der Bytefassung allein in den eckigen Klammern.
fn gemeldete_kodierungen(ausgabe: &str) -> Vec<String> {
    let mut aus: Vec<String> = ausgabe
        .lines()
        .map(str::trim)
        .filter(|z| z.starts_with("Rohdatei @0x"))
        .filter_map(|z| {
            let von = z.find('[')? + 1;
            let bis = z[von..].find(']')? + von;
            Some(z[von..bis].to_string())
        })
        .collect();
    aus.sort();
    aus.dedup();
    aus
}

/// **E4.** In wie vielen Byte-Kodierungen gesucht wird, sagt der Lauf — und
/// `--help` und README sagen dasselbe.
///
/// Neun für einen Begriff aus reinem ASCII (Latin-1 fällt mit UTF-8
/// zusammen), zehn mit Umlaut, sieben jenseits von Latin-1 (dann gibt es
/// keine Einbytefassung). Die Zahl in der Doku ist damit an einen Lauf
/// gebunden und nicht abgeschrieben; die alte Angabe „12 000 Muster
/// (1 000 Begriffe × 12 Kodierungen)“ hat nie gestimmt.
///
/// Mutationsnachweis: in `crates/redact-pdf/src/audit_bytes.rs` die drei
/// UTF-16LE-Fassungen aus `Needle::new` entfernt → dieser Test ist rot
/// („6 statt 9“).
#[test]
fn in_neun_byte_kodierungen_wird_gesucht_und_so_steht_es_in_der_doku() {
    let dir = workdir("kodierungen");
    let mut gefunden: Vec<(&str, usize, Vec<String>)> = Vec::new();

    for (name, begriff, erwartet) in [
        ("ascii", "GEHEIMNIS", 9usize),
        ("umlaut", "GEHEIMNISSÄ", 10),
        ("jenseits", "GEHEIMNIS漢", 7),
        // Nur Ziffern und die Buchstaben D und E: der Hex-String ist in
        // Groß- und in Kleinschreibung **dieselbe** Bytefolge, ebenso für
        // UTF-16BE und -LE. Drei Fassungen fallen weg — deshalb steht in der
        // Doku „bis zu neun“ und nicht „neun“.
        ("iban", "DE89 3704 0044 0532 0130 00", 6),
    ] {
        let datei = format!("{name}.pdf");
        std::fs::write(dir.join(&datei), pdf_mit_allen_fassungen(begriff)).unwrap();
        let out = run_in(&dir, &[&datei, "--check-leaks", begriff]);
        let kodierungen = gemeldete_kodierungen(&stdout(&out));
        assert_eq!(
            kodierungen.len(),
            erwartet,
            "{name}: {} statt {erwartet} Kodierungen: {kodierungen:?}\n{}",
            kodierungen.len(),
            stdout(&out)
        );
        gefunden.push((name, erwartet, kodierungen));
    }

    // Und für ASCII sind es genau die neun, die die Doku aufzählt.
    let mut erwartet: Vec<String> = BYTE_KODIERUNGEN_ASCII_ERWARTET
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    erwartet.sort();
    assert_eq!(
        gefunden[0].2, erwartet,
        "die neun Kodierungen heißen anders als in `BYTE_KODIERUNGEN_ASCII_ERWARTET`"
    );

    // Der Hilfetext und die README nennen dieselbe Zahl und alle vier
    // Kodierungsfamilien — UTF-16LE eingeschlossen.
    let help = glatt(&stdout(&run_in(&repo_root(), &["--help"])));
    let readme = glatt(&lf(
        &std::fs::read_to_string(repo_root().join("README.md")).expect("README.md lesbar")
    ));
    for (wo, text) in [("--help", &help), ("README.md", &readme)] {
        assert!(
            text.contains(
                "**bis zu neun Byte-Kodierungen**: UTF-8/ASCII, Latin-1/PDFDoc, UTF-16BE und \
                 UTF-16LE, dazu jede der drei Bytefassungen (Latin-1, UTF-16BE, UTF-16LE) \
                 als Hex-String in Groß- und in Kleinschreibung."
            ),
            "{wo} zählt die Kodierungen nicht auf, in denen wirklich gesucht wird"
        );
        assert!(
            text.contains("zehn mit Umlaut und sieben mit einem Zeichen jenseits von Latin-1"),
            "{wo} nennt die beiden anderen Fälle nicht"
        );
        assert!(
            text.contains("Fassungen, die auf dieselben Bytes fallen, werden nur einmal gesucht"),
            "{wo} verschweigt, dass zusammenfallende Fassungen entdoppelt werden — \
             „neun“ wäre dann eine Zahl zu viel"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// E3 — die Gründe für „nicht geprüft“
// ---------------------------------------------------------------------------

/// Die **fünf** Gründe, aus denen eine Stelle als `NICHT GEPRÜFT` gemeldet
/// wird: Name (so nennt ihn `SECURITY.md`) und ein Stück des Wortlauts, den
/// `redact_pdf::leaks_many_within` wirklich schreibt.
///
/// `SECURITY.md` zählte bis zur Fix-Runde 6 **drei** auf — und ließ genau die
/// beiden weg, die die Fix-Runde 5 hinzugefügt hatte. Eine Aufzählung, die
/// weniger nennt, als es gibt, liest sich wie eine vollständige.
const GRUENDE: [(&str, &str); 5] = [
    ("Entpackgrenze", "nicht entpackt — "),
    (
        "Vorprüfung des Laders abgelehnt",
        "die Vorprüfung des Laders lehnt die Datei ab: ",
    ),
    (
        "Verschachtelungstiefe des Objektgraphen",
        "nicht durchsucht — Verschachtelungstiefe ",
    ),
    (
        "Filtername, den das Programm nicht kennt",
        "ist hier kein bekannter Filter",
    ),
    (
        "Schriftdekoder nicht gelaufen",
        "Sicht 7 (Schriftdekoder) nicht gelaufen: ",
    ),
];

/// **E3.** Jeder Grund, den das Orakel kennt, steht in `SECURITY.md` — und
/// jeder Wortlaut steht wirklich im Quelltext des Orakels.
///
/// Der Quelltext ist hier das Messgerät, weil die Kommandozeile nicht jeden
/// der fünf Gründe erreichen kann: „die Vorprüfung des Laders lehnt die Datei
/// ab“ meldet nur, wer das Orakel **ohne** den Ladeschritt von
/// `check::run` aufruft — also die Oberfläche. Vier der fünf werden
/// zusätzlich am gebauten Binary gefahren
/// (`ze_p4_check_leaks_grenzen::die_gruende_der_ausgabe_stehen_in_security_md`
/// und `zf_q5_unbekannter_filter::*`).
///
/// Mutationsnachweis: in `SECURITY.md` eine Tabellenzeile der fünf Gründe
/// gestrichen → dieser Test ist rot und nennt den fehlenden Grund.
#[test]
fn die_fuenf_gruende_fuer_nicht_geprueft_stehen_in_security_md() {
    let orakel = glatt(&lf(&std::fs::read_to_string(
        repo_root().join("crates/redact-pdf/src/audit_bytes.rs"),
    )
    .expect("audit_bytes.rs lesbar")));
    let security = glatt(&lf(&std::fs::read_to_string(
        repo_root().join("SECURITY.md"),
    )
    .expect("SECURITY.md lesbar")));

    assert!(
        security.contains("**Fünf Gründe gibt es, nicht drei**"),
        "SECURITY.md zählt die Gründe nicht mehr"
    );
    for (name, wortlaut) in GRUENDE {
        let wortlaut = glatt(wortlaut);
        assert!(
            orakel.contains(&wortlaut),
            "„{wortlaut}“ steht nicht mehr in audit_bytes.rs — der Grund heißt anders, \
             und SECURITY.md zitiert eine Meldung, die es nicht gibt"
        );
        assert!(
            security.contains(&format!("| {name} |")),
            "SECURITY.md nennt den Grund „{name}“ nicht"
        );
        assert!(
            security.contains(wortlaut.trim_end()),
            "SECURITY.md nennt zu „{name}“ nicht den Wortlaut „{wortlaut}“"
        );
    }
    assert!(
        security.contains("Fünf Gründe gibt es dafür")
            || glatt(&lf(
                &std::fs::read_to_string(repo_root().join("README.md")).expect("README lesbar")
            ))
            .contains("Fünf Gründe gibt es dafür"),
        "die README nennt die Zahl der Gründe nicht"
    );
}
