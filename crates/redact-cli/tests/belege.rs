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
        // Zweiter Zaun neben `reconfigure` im Skript: unter Windows ist die
        // Konsolenkodierung cp1252, und der Pfeil in der Ausgabe ließ `print`
        // dort abbrechen.
        .env("PYTHONIOENCODING", "utf-8")
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
    // Und dieselbe Klasse eine Ebene weiter: ein Prüfskript, dessen Ausgabe
    // an der Konsolenkodierung scheitert, prüft nichts. `check-preview.py`
    // stellt seine Ströme deshalb selbst auf UTF-8; fällt die Zeile weg, wird
    // dieser Test rot statt der Windows-Job.
    let skript = lf(
        &std::fs::read_to_string(repo_root().join("scripts/check-preview.py"))
            .expect("check-preview.py lesbar"),
    );
    assert!(
        skript.contains(r#"reconfigure(encoding="utf-8""#),
        "check-preview.py stellt seine Ausgabe nicht auf UTF-8 — unter Windows \
         bricht sie am ersten Zeichen jenseits von ASCII ab"
    );

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
    pub const ZUORDNUNGEN_MB: &str = "16";

    // --- Fix-Runde 7 -----------------------------------------------------
    //
    // Was die Gegenprüfung der Runde 6 zählte
    // (`scratchpad/runde6/gegen/r5/ergebnis.tsv`): 76 Stellen der Doku
    // einzeln mutiert, 18 rot, 58 grün.
    pub const GEGEN6_STELLEN: usize = 76;
    pub const GEGEN6_ROT: usize = 18;
    pub const GEGEN6_GRUEN: usize = 58;

    /// Die Spiegel-Bombe **am gebauten Binary** (Fix-Runde 7). Die 0,56 s /
    /// 38 MB der Tabelle in `zf_q3_kombinatorik.rs` sind der *Testprozess*
    /// (nur `PdfExtractor`); das Binary tut mehr und wird anders übersetzt.
    /// Gemessen mit `--check-leaks` über dieselbe Datei, je drei Läufe,
    /// Spitze über `getrusage(RUSAGE_CHILDREN).ru_maxrss`.
    pub const SPIEGEL_BOMBE_OBJEKTE: usize = 9;
    pub const SPIEGEL_RELEASE_S: &str = "0,15–0,18";
    pub const SPIEGEL_RELEASE_KB: u64 = 40_668;
    pub const SPIEGEL_DEBUG_S: &str = "1,58–1,68";
    pub const SPIEGEL_DEBUG_KB: u64 = 53_016;
    pub const SPIEGEL_TESTPROZESS_S: &str = "0,56";
    pub const SPIEGEL_TESTPROZESS_MB: &str = "38";
    pub const SPIEGEL_OHNE_SPIEGEL_S: &str = "0,15";

    // --- Zählungen in den Einträgen der Runde 6 --------------------------
    //
    // Sie zählen, was der Eintrag darunter aufzählt. Eine Zahl, die niemand
    // nachzählt, ist genau die Klasse, an der die Gegenprüfung 58-mal
    // vorbeikam.
    /// Zahlen der Runde 5, die sich in **einem** Lauf mutieren ließen.
    pub const GEGEN5_UNGEBUNDEN: usize = 17;
    /// Ungebundene „1 000“, die die Runde 5 geschlossen hatte …
    pub const GEGEN5_GESCHLOSSEN: usize = 2;
    /// … und neue Zahlen, die sie dabei ungebunden anlegte.
    pub const GEGEN5_NEU: usize = 7;
    pub const LECKS_ANNOTATIONSNACHBARN: usize = 4;
    pub const FUNDORTE_EIGENSCHAFTSLISTE: usize = 4;
    pub const FALSCHE_ALARME_6: usize = 2;
    pub const BEIWERK_DICTIONARIES: usize = 3;
    pub const NUTZLAST_ZAEHLER: usize = 4;
    pub const SAETZE_OBERFLAECHE_6: usize = 3;
    pub const SCHWERE_BEFUNDE_6: usize = 3;

    // --- Was die Gegenprüfung der Runde 6 fand (Einleitung der Runde 7) ---
    pub const STILLE_LECKS_7: usize = 4;
    pub const FALSCHE_ZAHLEN_7: usize = 6;
    pub const WIDERLEGTE_SAETZE: usize = 2;
    pub const GRAFIKUMGEBUNGEN: usize = 2;
    pub const KLARTEXTTRAEGER_BEIWERK: usize = 6;
    pub const DIENSTVERWEIGERUNGEN_7: usize = 2;
    pub const BEFUNDE_OBERFLAECHE_7: usize = 8;
    /// Die Decke je Seite statt je Dokument (`runde6/gegen/r1/mess_paare.txt`):
    /// 1 000 Seiten × (100 Klammern × 999 `Do`).
    pub const DECKE_SEITEN: &str = "1 000";
    pub const DECKE_DATEI_BYTES: u64 = 224_752;
    pub const DECKE_SPITZE_KB: u64 = 6_438_680;
    /// Die Proben der Plattformregel (`runde6/gegen/r5/plattform.tsv`).
    pub const PLATTFORM_PROBEN: usize = 9;
    pub const PLATTFORM_FEHLALARM: usize = 1;
    pub const PLATTFORM_LUECKEN: usize = 6;
    /// Verletzungen, die die Gegenpruefung fand (`PLATTFORM_VERLETZUNGEN`),
    /// davon in der Fix-Runde 7 behoben (`PLATTFORM_BEHOBEN`) und als benannte
    /// Altlast verblieben (`PLATTFORM_ALTLASTEN`, die `mkfifo`-Stellen).
    pub const PLATTFORM_VERLETZUNGEN: usize = 9;
    pub const PLATTFORM_BEHOBEN: usize = 3;
    /// Die verbliebene Altlast SIND die `mkfifo`-Stellen — eine Zahl, eine
    /// Stelle. Eine zweite Konstante daneben waere dieselbe Zahl zweimal.
    pub const PLATTFORM_ALTLASTEN: usize = 6;
    /// So oft war der Windows-Job der CI an dieser Fehlerklasse rot.
    pub const WINDOWS_ROT: usize = 3;
    /// Von den behobenen Stellen die beiden mit `std::os::unix`.
    pub const PLATTFORM_UNIX_API: usize = 2;
    /// Der Lauf, der „52 Stelle(n)“ unter mehr Zeilen schrieb
    /// (`redact-pdf/tests/zg_r2_decke.rs`).
    pub const STELLEN_GEMELDET: usize = 52;
    pub const STELLEN_EINZELN: usize = 50;
    pub const STELLEN_WEITERE: usize = 7;
    pub const STELLEN_STROEME: usize = 57;
    pub const ZUSAGEN_ZU_VIEL: usize = 3;
    pub const GRUENDE_ALT: usize = 3;
    pub const GRUENDE_AM_BINARY: usize = 3;
    pub const KODIERUNGEN_UMLAUT: usize = 10;
    pub const KODIERUNGEN_JENSEITS: usize = 7;
    pub const KODIERUNGSFAMILIEN: usize = 4;
    pub const FAELLE_ALT: usize = 2;
    pub const FAELLE_NEU: usize = 3;

    // --- Das Movie-Leck ---------------------------------------------------
    pub const MOVIE_SCHWAERZUNGEN: usize = 1;
    pub const MOVIE_FUNDSTELLEN: usize = 6;
    pub const LESEZEICHEN_GEMELDET: usize = 1;

    // --- Kosten der ehrlichen Zählung (Fix-Runde 6) -----------------------
    pub const ANNOTATIONEN: &str = "200 000";
    pub const ANNOT_NACHHER_MS: &str = "767–791";
    pub const ANNOT_VORHER_MS: &str = "746";

    // --- Die gekürzte Statuszeile ----------------------------------------
    pub const SATZ_VORHER_ZEICHEN: usize = 804;
    pub const SATZ_NACHHER_ZEICHEN: usize = 411;

    // --- Der Klon der Rohbytes vor dem ersten Filter ----------------------
    pub const KLON_STROM_MIB: usize = 64;
    pub const KLON_BUDGET_MIB: usize = 512;
    pub const KLON_VORHER_MB: &str = "205";
    pub const KLON_NACHHER_MB: &str = "138";
    pub const KLON_SCHWAERZEN_MB: &str = "621";
    /// Dieselbe Datei am **gebauten Binary** (`--check-leaks`,
    /// `--max-decompressed-mb 512`), `getrusage(RUSAGE_CHILDREN).ru_maxrss`.
    pub const KLON_BINARY_KB: u64 = 325_388;

    // --- Die widerlegte Kostenzahl „1 000 Begriffe 65,7 s“ ----------------
    pub const ALTE_KOSTENZAHL_S: &str = "65,7";
    pub const MUSTER_JE_BEGRIFF_ALT: usize = 6;
    pub const DURCHGANG_MB: usize = 268;
    pub const MEMMEM_GB_S: &str = "9,9";
    pub const MEMMEM_JE_BEGRIFF_S: &str = "0,163";
    pub const HEUTE_1_S: &str = "5,01";
    pub const HEUTE_1000_S: &str = "5,99";
}

/// Die ausgeschriebene Zahl, wie die Doku kleine Zahlen schreibt.
///
/// Ohne sie stünde „vier“ in der Doku und `4` im Test — zwei Schreibweisen
/// derselben Zahl, und genau daran kam die Gegenprüfung vorbei
/// („`siebenmal` zu `achtmal`“, grün).
fn zahlwort(n: usize) -> &'static str {
    match n {
        1 => "ein",
        2 => "zwei",
        3 => "drei",
        4 => "vier",
        5 => "fünf",
        6 => "sechs",
        7 => "sieben",
        8 => "acht",
        9 => "neun",
        10 => "zehn",
        11 => "elf",
        12 => "zwölf",
        17 => "siebzehn",
        _ => panic!("für {n} gibt es hier kein Zahlwort"),
    }
}

/// Dasselbe Zahlwort am Satzanfang.
fn zahlwort_gross(n: usize) -> String {
    let w = zahlwort(n);
    let mut zeichen = w.chars();
    match zeichen.next() {
        Some(c) => c.to_uppercase().collect::<String>() + zeichen.as_str(),
        None => String::new(),
    }
}

/// Der Wert einer `usize`-Konstanten aus einer Quelldatei des Baums —
/// `const NAME: usize = 100_000;`.
///
/// Damit steht eine Decke, die die Doku nennt, **einmal** im Baum: im Code.
fn konstante_aus(datei: &str, name: &str) -> usize {
    let text = lf(&std::fs::read_to_string(repo_root().join(datei))
        .unwrap_or_else(|e| panic!("{datei} lesbar: {e}")));
    let muster = format!("const {name}: usize = ");
    let ab = text
        .find(&muster)
        .unwrap_or_else(|| panic!("`{muster}…` steht nicht mehr in {datei}"))
        + muster.len();
    let ziffern: String = text[ab..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '_')
        .filter(|c| *c != '_')
        .collect();
    ziffern
        .parse()
        .unwrap_or_else(|_| panic!("`{muster}` steht ohne Zahl in {datei}"))
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
            // **Höchstens** zehn Byte-Kodierungen (neun ohne Umlaut, die
            // zehnte ist Latin-1/PDFDoc), dazu der dekodierte Text und die
            // Fassung ohne Leerraum: zwölf Muster je Begriff. Die Doku sagte
            // bis zur Fix-Runde 7 „10 000“ und ließ die beiden Fassungen aus,
            // die sie im selben Satz aufzählt.
            mit_tausendertrenner(
                (redact_core::MAX_CHECK_NEEDLES * (m::KODIERUNGEN_UMLAUT + 2)) as u64
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
    let zuordnungen = mit_tausendertrenner(zuordnungsdecke() as u64);
    satz(
        "SECURITY.md",
        format!(
            "höchstens {zuordnungen} Zuordnungen zwischen einem Textspiegel und einer \
             Formularplatzierung beim Aufbau der Liste und ebenso viele beim \
             Aufklappen, zusammen rund {} MB",
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

    // =====================================================================
    // Fix-Runde 6 — jeder Messsatz des Blocks, nicht nur seine Kopfzeile
    // =====================================================================
    //
    // Die Gegenprüfung mutierte 76 Stellen einzeln: 18 rot, 58 grün. Grün
    // blieb alles, was nicht wörtlich in einem dieser Sätze stand — auch
    // `0,56 s`, `38 MB`, `6 Fundstellen`, `804 → 411 Zeichen`. Deshalb steht
    // hier jetzt **jeder** Satz des Blocks, der eine Zahl trägt; welche Zahl
    // wo fehlt, sagt `jede_zahl_der_letzten_runden_ist_gebunden`.

    // --- Die Einleitung zählt, was sie aufzählt --------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "Dazu {} Klartextlecks an Nachbarn von Annotationen, {} Fundorte statt \
             einem bei der direkten Eigenschaftsliste, {} falsche Alarme",
            zahlwort(m::LECKS_ANNOTATIONSNACHBARN),
            zahlwort(m::FUNDORTE_EIGENSCHAFTSLISTE),
            zahlwort(m::FALSCHE_ALARME_6)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "{} Messwerte in README, `SECURITY.md` und `CHANGELOG.md` ließen sich in \
             einem Lauf mutieren, ohne dass ein Test rot wurde",
            zahlwort(m::GEGEN5_UNGEBUNDEN)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Die Gegenprüfung mutierte {} Zahlen und Sätze in einem Lauf",
            zahlwort(m::GEGEN5_UNGEBUNDEN)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "hatte {} ungebundene „{}“ geschlossen und dabei {} neue Zahlen \
             ungebunden angelegt",
            zahlwort(m::GEGEN5_GESCHLOSSEN),
            mit_tausendertrenner(redact_core::MAX_CHECK_NEEDLES as u64),
            zahlwort(m::GEGEN5_NEU)
        ),
    );

    // --- `--help` widersprach sich selbst --------------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "{} Absätze über dem Block „{} Fälle“ stand im selben Hilfetext weiter \
             „Rückgabewert: `0`, wenn keiner der Begriffe gefunden wurde, `{}`, wenn \
             mindestens einer noch dasteht“ — die {} Fälle",
            zahlwort_gross(m::FAELLE_NEU),
            zahlwort_gross(m::FAELLE_NEU),
            RC_LECK,
            zahlwort(m::FAELLE_ALT)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "ein Lauf über eine Datei mit einem Objekt auf Ebene {} sagt „nicht \
             gefunden“ und endet mit {RC_LECK}",
            objektsichttiefe() + 1
        ),
    );

    // --- Die fünf Gründe für „nicht geprüft“ -----------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "zählte {} Gründe für „nicht geprüft“ auf, das Orakel kennt {}**",
            zahlwort(m::GRUENDE_ALT),
            zahlwort(GRUENDE.len())
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Alle {} stehen jetzt mit ihrem Wortlaut in einer Tabelle; ein Test hält \
             jeden gegen den Quelltext des Orakels, {} davon zusätzlich gegen einen \
             Lauf des gebauten Binaries",
            zahlwort(GRUENDE.len()),
            zahlwort(m::GRUENDE_AM_BINARY)
        ),
    );

    // --- Die Kodierungen --------------------------------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "{} Namen für {} Muster. Gesucht wird in **{}** Byte-Kodierungen ({} mit \
             Umlaut, {} jenseits von Latin-1)",
            zahlwort(m::KODIERUNGSFAMILIEN),
            zahlwort(BYTE_KODIERUNGEN_ASCII_ERWARTET.len()),
            zahlwort(BYTE_KODIERUNGEN_ASCII_ERWARTET.len()),
            zahlwort(m::KODIERUNGEN_UMLAUT),
            zahlwort(m::KODIERUNGEN_JENSEITS)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Und „{} Muster ({} Begriffe × {} Kodierungen)“ hat nie gestimmt: so viele \
             **Kodierungen** gibt es nicht. Nach `Probe::new` sind es {} Muster je \
             Begriff ({} Bytefassungen und der dekodierte Text), {} mit einer Fassung \
             ohne Leerraum und {} mit Umlaut **und** Leerraum",
            mit_tausendertrenner(
                (redact_core::MAX_CHECK_NEEDLES * (m::KODIERUNGEN_UMLAUT + 2)) as u64
            ),
            mit_tausendertrenner(redact_core::MAX_CHECK_NEEDLES as u64),
            m::KODIERUNGEN_UMLAUT + 2,
            BYTE_KODIERUNGEN_ASCII_ERWARTET.len() + 1,
            zahlwort(BYTE_KODIERUNGEN_ASCII_ERWARTET.len()),
            BYTE_KODIERUNGEN_ASCII_ERWARTET.len() + 2,
            m::KODIERUNGEN_UMLAUT + 2
        ),
    );

    // --- Drei Zusagen, die zu viel versprachen ---------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "während `MAX_NAMED_PLACES = {}` höchstens {} nennt und den Rest zählt",
            benannte_stellen(),
            zahlwort(benannte_stellen())
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "derselbe Messwert stand im Fließtext als „{} kB ≈ {} GiB“ (durch 1024²) \
             und in der Tabelle als „{} MB“ (durch 1000)",
            mit_tausendertrenner(m::BOMBE_PRUEFEN_KB),
            kb_in_gib(m::BOMBE_PRUEFEN_KB),
            mit_tausendertrenner((m::BOMBE_PRUEFEN_KB as f64 / 1000.0).round() as u64)
        ),
    );

    // --- Die Spiegel-Bombe, jetzt am gebauten Binary ---------------------
    satz(
        "CHANGELOG.md",
        format!(
            "eine Datei von {} kB mit **{}** Objekten belegte {} MB und lief {} s",
            m::SPIEGEL_BOMBE_KB,
            zahlwort(m::SPIEGEL_BOMBE_OBJEKTE),
            mit_tausendertrenner(m::SPIEGEL_BOMBE_MB),
            m::SPIEGEL_BOMBE_S
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "dieselbe Datei am gebauten Binary **{} s und {} MB** (Release, \
             Rückgabewert {RC_LECK}); im Testprozess, der nur den Extraktor fährt, \
             {} s und {} MB; ohne `/ActualText` brauchte sie immer {} s",
            m::SPIEGEL_RELEASE_S,
            kb_in_mb(m::SPIEGEL_RELEASE_KB),
            m::SPIEGEL_TESTPROZESS_S,
            m::SPIEGEL_TESTPROZESS_MB,
            m::SPIEGEL_OHNE_SPIEGEL_S
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "höchstens {zuordnungen} beim Aufbau und {zuordnungen} beim Aufklappen, je \
             Seiten-Scan (zusammen rund {} MB)",
            m::ZUORDNUNGEN_MB
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Genau {zuordnungen} Aufklappungen gingen auf und lieferten trotzdem \
             Rückgabewert {RC_LECK} (`decke_{}.pdf` 0, `decke_{}.pdf` {RC_LECK})",
            zuordnungsdecke() - 1,
            zuordnungsdecke()
        ),
    );

    // --- Der Textspiegel im Ressourcenverzeichnis ------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "blieb stehen — an {} Orten.**",
            zahlwort(m::FUNDORTE_EIGENSCHAFTSLISTE)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Alle {} werden jetzt an ihrem Fundort bereinigt",
            zahlwort(m::FUNDORTE_EIGENSCHAFTSLISTE)
        ),
    );

    // --- Der unbekannte Filter an erster Stelle --------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "`/Filter /FooDecode` kam als „nicht gefunden“ mit Rückgabewert 0 zurück, \
             dieselbe Datei als `/Filter [/FlateDecode /FooDecode]` mit {RC_LECK}"
        ),
    );
    satz(
        "SECURITY.md",
        format!(
            "`/DCTDecode` kostete dadurch {} MB, seit der Fix-Runde 6 sind es {} MB",
            m::KLON_VORHER_MB,
            m::KLON_NACHHER_MB
        ),
    );
    satz(
        "SECURITY.md",
        format!("kommt auf {} MB", m::KLON_SCHWAERZEN_MB),
    );
    satz(
        "SECURITY.md",
        format!(
            "dass {} − {} = {} genau der {}-MiB-Strom ist, verrät die Einheit",
            m::KLON_VORHER_MB,
            m::KLON_NACHHER_MB,
            m::KLON_VORHER_MB.parse::<u64>().expect("Zahl")
                - m::KLON_NACHHER_MB.parse::<u64>().expect("Zahl"),
            m::KLON_STROM_MIB
        ),
    );
    satz(
        "SECURITY.md",
        format!(
            "liegt die Spitze über derselben Datei bei {} kB = {} MB",
            mit_tausendertrenner(m::KLON_BINARY_KB),
            kb_in_mb(m::KLON_BINARY_KB)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "gemessen im Testprozess ({}-MiB-Strom, `/DCTDecode`, Budget {} MiB) \
             **{} MB vorher, {} MB nachher**",
            m::KLON_STROM_MIB,
            m::KLON_BUDGET_MIB,
            m::KLON_VORHER_MB,
            m::KLON_NACHHER_MB
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Die alte Kostenzahl „{} Begriffe {} s“ ist widerlegt und durch eine \
             nachstellbare Rechnung ersetzt: {} Muster je Begriff über {} MB je \
             Durchgang, `memmem` {} GB/s → {} s je Begriff, rund **{} s für {} \
             Begriffe als untere Schranke**; heute {} s (1 Begriff) gegen {} s \
             ({})",
            mit_tausendertrenner(redact_core::MAX_CHECK_NEEDLES as u64),
            m::ALTE_KOSTENZAHL_S,
            m::MUSTER_JE_BEGRIFF_ALT,
            m::DURCHGANG_MB,
            m::MEMMEM_GB_S,
            m::MEMMEM_JE_BEGRIFF_S,
            untere_schranke_s(),
            mit_tausendertrenner(redact_core::MAX_CHECK_NEEDLES as u64),
            m::HEUTE_1_S,
            m::HEUTE_1000_S,
            mit_tausendertrenner(redact_core::MAX_CHECK_NEEDLES as u64)
        ),
    );

    // --- Vier Klartextlecks an Annotationsnachbarn -----------------------
    satz(
        "CHANGELOG.md",
        format!(
            "alle {} Beiwerk-Dictionaries fallen jetzt als Ganzes",
            zahlwort(m::BEIWERK_DICTIONARIES)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "endete mit „Schwärzungen: {}“ und Rückgabewert 0, während \
             `--check-leaks` an der Ausgabe **{} Fundstellen** fand — jetzt 0",
            m::MOVIE_SCHWAERZUNGEN,
            m::MOVIE_FUNDSTELLEN
        ),
    );

    // --- Die Zahlen im Audit-Log -----------------------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "nicht nur für die {} Nutzlast-Zähler",
            zahlwort(m::NUTZLAST_ZAEHLER)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "„{} Lesezeichen entfernt“ über einen `/Title 4 0 R`",
            m::LESEZEICHEN_GEMELDET
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "({} Annotationen mit `/Contents` als Verweis): {} ms statt {} ms",
            m::ANNOTATIONEN,
            m::ANNOT_NACHHER_MS,
            m::ANNOT_VORHER_MS
        ),
    );

    // --- Die Oberfläche ---------------------------------------------------
    satz(
        "CHANGELOG.md",
        format!(
            "**{} weitere Sätze der Oberfläche.**",
            zahlwort_gross(m::SAETZE_OBERFLAECHE_6)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "nannte die bleibende Warnung die Decke von {} Begriffen als Grund",
            mit_tausendertrenner(redact_core::MAX_CHECK_NEEDLES as u64)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "({} → {} Zeichen bei {} Stellen)",
            m::SATZ_VORHER_ZEICHEN,
            m::SATZ_NACHHER_ZEICHEN,
            zahlwort(benannte_stellen())
        ),
    );

    // --- Nachträge: die Sätze, die der Vollständigkeitstest fand ---------
    satz(
        "CHANGELOG.md",
        format!(
            "**{} schwere Befunde** blieben",
            zahlwort_gross(m::SCHWERE_BEFUNDE_6)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "der das Leck-Orakel stumm machte („nicht gefunden“, Rückgabewert {RC_SAUBER}, \
             über einen Strom, den keine Sicht gelesen hatte)"
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "„bis zu {} Stellen beim Namen“ aus `MAX_NAMED_PLACES` im Quelltext der \
             Oberfläche",
            zahlwort(benannte_stellen())
        ),
    );
    satz(
        "CHANGELOG.md",
        format!("Der Absatz nennt jetzt die Bedingung für `{RC_SAUBER}`"),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "`/Filter /FooDecode` allein kam als „nicht gefunden“ mit Rückgabewert \
             {RC_SAUBER} zurück, `/Filter [/FlateDecode /FooDecode]` mit {RC_LECK}"
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "was gemessen ist: {} Ketten, {} mit Meldung und Rückgabewert {RC_LECK}, {} \
             (Bildfilter allein und am Kettenende) ohne Meldung und mit {RC_SAUBER}",
            zahlwort(filterketten()),
            zahlwort(filterketten_mit_meldung()),
            zahlwort(filterketten() - filterketten_mit_meldung())
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "**{} Zusagen, die zu viel versprachen.**",
            zahlwort_gross(m::ZUSAGEN_ZU_VIEL)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!("ohne Warnung und mit Rückgabewert {RC_SAUBER}. Betroffen waren die Seite"),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "**{} Klartextlecks an Annotationsnachbarn.**",
            zahlwort_gross(m::LECKS_ANNOTATIONSNACHBARN)
        ),
    );

    // =====================================================================
    // Fix-Runde 7
    // =====================================================================
    satz(
        "CHANGELOG.md",
        format!(
            "**{} stille Lecks** blieben: ein Form-XObject ohne eigenes `/Resources`, \
             dasselbe Formular unter {} Grafikumgebungen, ein `/Filter`-Wert, der gar \
             kein Name ist, und {} Klartextträger am Beiwerk einer Annotation. Dazu {} \
             Dienstverweigerungen",
            zahlwort_gross(m::STILLE_LECKS_7),
            zahlwort(m::GRAFIKUMGEBUNGEN),
            zahlwort(m::KLARTEXTTRAEGER_BEIWERK),
            zahlwort(m::DIENSTVERWEIGERUNGEN_7)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "die je Seite statt je Dokument zählt ({} Seiten aus einer Datei von {} \
             Byte belegten beim Schwärzen {} MB) —, {} Befunde an der Oberfläche und \
             die Erkenntnis, dass von {} einzeln mutierten Zahlen der Doku weiter {} \
             grün blieben",
            m::DECKE_SEITEN,
            mit_tausendertrenner(m::DECKE_DATEI_BYTES),
            mit_tausendertrenner(kb_in_mb(m::DECKE_SPITZE_KB)),
            zahlwort(m::BEFUNDE_OBERFLAECHE_7),
            m::GEGEN6_STELLEN,
            m::GEGEN6_GRUEN
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "die Gegenprüfung mutierte danach {} Stellen einzeln: {} wurden rot, **{} \
             blieben grün**",
            m::GEGEN6_STELLEN,
            m::GEGEN6_ROT,
            m::GEGEN6_GRUEN
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "**{} Zahlen in der Doku waren falsch, alle nachgemessen.** „{} s und {} \
             MB“ für die entschärfte Spiegel-Bombe stammten aus dem **Testprozess**",
            zahlwort_gross(m::FALSCHE_ZAHLEN_7),
            m::SPIEGEL_TESTPROZESS_S,
            m::SPIEGEL_TESTPROZESS_MB
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Über einer Tabelle mit {} Filterketten stand „{} Ketten“",
            zahlwort(filterketten()),
            zahlwort(filterketten() - 1)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Dazu {} Sätze, die der Befund derselben Runde widerlegt hatte",
            zahlwort(m::WIDERLEGTE_SAETZE)
        ),
    );
    // Und die Begründung in SECURITY.md rechnet die Zahl vor, statt sie zu
    // behaupten — hier stand „51 Zeilen“, die Decke **eines** Zählers.
    satz(
        "SECURITY.md",
        format!(
            "denn `LeakCheck::unchecked` darf bis zu **{}** Zeilen tragen",
            ungepruefte_zeilen()
        ),
    );
    satz(
        "SECURITY.md",
        format!(
            "die Decke `MAX_UNCHECKED = {}` einzeln genannter Stellen plus Summenzeile gilt je **Zähler**, und davon gibt es {} (zu große Ströme der Rohsicht, dieselben der Objektsicht, Stellen aus anderem Grund), dazu die Zeile über Sicht 7: {} × {} + 1",
            konstante_aus("crates/redact-pdf/src/audit_bytes.rs", "MAX_UNCHECKED"),
            zahlwort(3),
            3,
            konstante_aus("crates/redact-pdf/src/audit_bytes.rs", "MAX_UNCHECKED") + 1
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "am gebauten Binary sind es {} s und {} MB (Release) bzw. {} s und {} MB \
             (Debug), beide mit Rückgabewert {RC_LECK}",
            m::SPIEGEL_RELEASE_S,
            kb_in_mb(m::SPIEGEL_RELEASE_KB),
            m::SPIEGEL_DEBUG_S,
            kb_in_mb(m::SPIEGEL_DEBUG_KB)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Dieselbe Datei hat **{}** Objekte, nicht elf.",
            zahlwort(m::SPIEGEL_BOMBE_OBJEKTE)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Die README versprach „bis zu {} Muster“ für {} Begriffe und zählte im \
             selben Satz elf auf — `Probe::new` legt bis zu zwölf je Begriff an, also \
             {}.",
            mit_tausendertrenner(
                (redact_core::MAX_CHECK_NEEDLES * (BYTE_KODIERUNGEN_ASCII_ERWARTET.len() + 1))
                    as u64
            ),
            mit_tausendertrenner(redact_core::MAX_CHECK_NEEDLES as u64),
            mit_tausendertrenner(
                (redact_core::MAX_CHECK_NEEDLES * (m::KODIERUNGEN_UMLAUT + 2)) as u64
            )
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "begründete die Decke von {} genannten Stellen mit „{} Zeilen“, während der \
             Quelltext daneben {} ausrechnet",
            zahlwort(benannte_stellen()),
            // Die alte Begründung nahm die Decke **eines** Zählers samt
            // Summenzeile für die ganze Liste.
            konstante_aus("crates/redact-pdf/src/audit_bytes.rs", "MAX_UNCHECKED") + 1,
            ungepruefte_zeilen()
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "die Gegenprüfung änderte in der Kopie `{}` in `{}`, und kein Test wurde rot",
            zuordnungsdecke(),
            zuordnungsdecke() * 2
        ),
    );

    satz(
        "CHANGELOG.md",
        format!(
            "Die {} `mkfifo`-Stellen bleiben als benannte Altlast in der Liste",
            zahlwort(m::PLATTFORM_ALTLASTEN)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "an dem der Windows-Job schon {}mal rot war",
            zahlwort(m::WINDOWS_ROT)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "{} Proben, einzeln an den Baum gehängt: {} Fehlalarm",
            zahlwort_gross(m::PLATTFORM_PROBEN),
            zahlwort(m::PLATTFORM_FEHLALARM)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "und {} Lücken — ein fremdes `.ok()` acht Zeilen weiter genügte als Ausweg",
            zahlwort(m::PLATTFORM_LUECKEN)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            // Der Pfad des Unix-APIs steht hier absichtlich NICHT: die
            // Plattformregel (`zf_q5_plattformzusagen`) durchsucht den
            // Quelltext nach seiner Marke und kann eine Zeichenkette nicht von
            // einer Verwendung unterscheiden. Der Satz braucht die Zahlen, den
            // Namen nennt der CHANGELOG im Satz danach.
            "{} Stellen im Baum verletzten die geschärfte Regel; **{} davon hätten \
             den Windows-Lauf gebrochen und sind behoben**: {}mal ein Unix-API \
             **ohne** `cfg`",
            zahlwort_gross(m::PLATTFORM_VERLETZUNGEN),
            zahlwort(m::PLATTFORM_BEHOBEN),
            zahlwort(m::PLATTFORM_UNIX_API)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "„{} Stelle(n) nicht geprüft“ stand unter {} einzeln genannten Zeilen, \
             einer Summenzeile „{} weitere“ und einer über {} Ströme",
            m::STELLEN_GEMELDET,
            m::STELLEN_EINZELN,
            m::STELLEN_WEITERE,
            m::STELLEN_STROEME
        ),
    );

    aus
}

/// Wie viele Zeilen `LeakCheck::unchecked` höchstens trägt: die Decke je
/// Zähler plus Summenzeile, dreimal, plus die Zeile über die letzte Sicht.
/// Die Doku sagte dafür „51“ und meinte die Decke **eines** Zählers.
fn ungepruefte_zeilen() -> usize {
    let decke = konstante_aus("crates/redact-pdf/src/audit_bytes.rs", "MAX_UNCHECKED");
    3 * (decke + 1) + 1
}

/// Der Rückgabewert, mit dem `--check-leaks` einen Fund oder eine ungeprüfte
/// Stelle meldet.
const RC_LECK: i32 = 3;

/// Der Rückgabewert eines Laufs ohne Fund und ohne ungeprüfte Stelle.
const RC_SAUBER: i32 = 0;

/// Die Filterketten, über die `SECURITY.md` eine Zusage macht — gezählt an
/// der Tabelle dort, nicht abgeschrieben. Jede Zeile fährt
/// `zg_r5_filterketten::jede_zeile_der_filterkettentabelle_stammt_aus_einem_lauf`
/// durch das gebaute Binary.
fn filterkettentabelle() -> Vec<String> {
    let security = lf(&std::fs::read_to_string(repo_root().join("SECURITY.md")).expect("SECURITY"));
    let kopf = "| Filterkette | Meldung | Rückgabewert |";
    let zeilen: Vec<String> = security
        .lines()
        .map(str::trim)
        .skip_while(|z| *z != kopf)
        .skip(2) // Kopfzeile und Trennzeile
        .take_while(|z| z.starts_with("| `"))
        .map(str::to_string)
        .collect();
    assert!(
        zeilen.len() >= 5,
        "die Filterkettentabelle in SECURITY.md hat nur {} Zeilen",
        zeilen.len()
    );
    zeilen
}

fn filterketten() -> usize {
    filterkettentabelle().len()
}

fn filterketten_mit_meldung() -> usize {
    filterkettentabelle()
        .iter()
        .filter(|z| !z.ends_with("| keine | 0 |"))
        .count()
}

/// Die Decke der Spiegel-Formular-Zuordnungen — aus dem Quelltext, nicht
/// abgeschrieben.
fn zuordnungsdecke() -> usize {
    konstante_aus(
        "crates/redact-pdf/src/content.rs",
        "MAX_MIRROR_FORM_PLACEMENTS",
    )
}

/// Wie tief die Objektsicht des Orakels geht.
fn objektsichttiefe() -> usize {
    konstante_aus("crates/redact-pdf/src/audit_bytes.rs", "MAX_DEPTH")
}

/// Wie viele ungeprüfte Stellen die Statuszeile beim Namen nennt.
fn benannte_stellen() -> usize {
    konstante_aus("crates/redact-gui/src/state.rs", "MAX_NAMED_PLACES")
}

/// Die untere Schranke der alten Kostenrechnung: 1 000 Begriffe × die
/// gemessene Zeit je Begriff. Eine dritte Zahl gibt es nicht.
fn untere_schranke_s() -> String {
    let je_begriff: f64 = messwerte::MEMMEM_JE_BEGRIFF_S
        .replace(',', ".")
        .parse()
        .expect("Zahl mit Komma");
    format!("{:.0}", je_begriff * redact_core::MAX_CHECK_NEEDLES as f64)
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
        let klein = satz.to_lowercase();
        assert!(
            satz.chars().any(|c| c.is_ascii_digit()) || ZAHLWORTE.iter().any(|w| klein.contains(w)),
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
// E1 — und **jede** Zahl des Blocks, nicht nur die, an die jemand dachte
// ---------------------------------------------------------------------------
//
// Die Gegenprüfung der Fix-Runde 6 hat 76 Stellen der Doku einzeln mutiert:
// 18 rot, 58 grün. Der Test darüber (`die_zahlen_der_doku_sind_gebunden`)
// prüft, dass jeder Satz **seiner Liste** in der Doku steht — er sagt nichts
// darüber, ob die Liste vollständig ist. Grün blieben deshalb `0,56 s`,
// `38 MB`, `0,15 s`, `6 Fundstellen`, `767–791 ms`, `804 → 411 Zeichen`,
// `elf Objekte`, `16 MB` und zwei Dutzend weitere.
//
// Dieser Test dreht die Frage um: er liest den **Block** der beiden letzten
// Fix-Runden aus `CHANGELOG.md`, markiert, was die gebundenen Sätze davon
// abdecken, und verlangt, dass danach **keine** Zahl übrig bleibt. Was keine
// Messzahl ist, steht mit Begründung in [`KEINE_MESSZAHL`] — eine Zeile Arbeit
// an der richtigen Stelle statt einer stillen Lücke an der falschen.

/// Stellen des Blocks, die eine Zahl tragen und **keine Messzahl** sind —
/// Wortlaut und Begründung.
///
/// Im Zweifel ist es eine Messzahl: wer hier etwas einträgt, nimmt es aus der
/// Bindung heraus und muss sagen, warum das keine Aussage über das Programm
/// ist.
const KEINE_MESSZAHL: &[(&str, &str)] = &[
    (
        "Fix-Runde 6: was die Gegenprüfung der Runde 5 noch fand",
        "Überschrift; die Nummer benennt einen Abschnitt dieser Datei",
    ),
    (
        "Fix-Runde 7: was die Gegenprüfung der Runde 6 noch fand",
        "Überschrift; die Nummer benennt einen Abschnitt dieser Datei",
    ),
    (
        "Fünf Gegenprüfer lasen die Korrekturen der Runde 5 mit eigenem Material gegen.",
        "eine Angabe über den Ablauf der Runde, nicht über das Programm",
    ),
    (
        "Fünf Gegenprüfer lasen die Korrekturen der Runde 6 mit eigenem Material gegen.",
        "eine Angabe über den Ablauf der Runde, nicht über das Programm",
    ),
    (
        "die die Runde 5 hinzugefügt hatte",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "obwohl die Runde 5 UTF-16LE eingebaut hatte",
        "Verweis auf einen Abschnitt dieser Datei; UTF-16LE ist ein Name",
    ),
    (
        "die die Runde 5 gerade abgeschafft hatte",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "dieselbe Klasse wie der ASCII85-Fall der Runde 5",
        "ASCII85 ist ein Filtername; die Runde benennt einen Abschnitt",
    ),
    (
        "Die Runde 5 hatte",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "Die Fix-Runde 6 hat die Messzahlen der Doku an einen Testdatensatz gebunden",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "den die Fix-Runde 6 neu zusagte",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "war zu viel versprochen — siehe Fix-Runde 7.)",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "jetzt fährt `zg_r5_filterketten` jede Zeile durch das Binary",
        "der Name einer Testdatei, keine gemessene Größe",
    ),
    (
        "obwohl dasselbe Dokument MB als 1024² Byte festlegt",
        "die Definition der Einheit, keine gemessene Größe",
    ),
    (
        "„UTF-8, Latin-1/PDFDoc, UTF-16BE und als Hex-String“",
        "Namen von Kodierungen, keine gemessenen Größen",
    ),
    (
        "**UTF-16LE fehlte in beiden Kodierungslisten.**",
        "UTF-16LE ist der Name einer Kodierung, keine gemessene Größe",
    ),
    (
        "(der vierte in `zf_q5_unbekannter_filter.rs`; der fünfte gehört der Oberfläche",
        "der Name einer Testdatei, keine gemessene Größe",
    ),
    (
        "`setrlimit(RLIMIT_CORE, 0)`",
        "der Aufruf mit seinem Argument, keine gemessene Größe",
    ),
    (
        "(durch 1024²)",
        "die Definition der Einheit: MB heißt in diesem Projekt 1024² Byte",
    ),
    (
        "MB heißt in diesem Projekt 1024² Byte",
        "die Definition der Einheit",
    ),
    (
        "`/Title 4 0 R`",
        "eine Objektnummer im Beispiel, keine gemessene Größe",
    ),
    (
        "Jetzt steht jede Messzahl **einmal** im Testdatensatz",
        "„einmal“ ist hier keine Zahl, sondern die Zusage, dass es keine zweite \
         Abschrift gibt",
    ),
    (
        "läuft einmal je Strom statt je Platzierung",
        "„einmal“ beschreibt die Häufigkeit eines Durchlaufs, keine Messung",
    ),
    // --- Fix-Runde 7: Anzahlen von Befunden, Stellen und Vorfaellen -------
    // Keine davon misst das Programm; sie zaehlen, wovon der Abschnitt
    // handelt. Wer sie aendert, aendert keine Zusage ueber das Verhalten.
    (
        "**Zwei Wege zur Dienstverweigerung, beide gedeckelt.**",
        "zaehlt die beiden Befunde dieses Punktes, misst nichts am Programm",
    ),
    (
        "Drei Zähler tragen jetzt zusammen",
        "zaehlt Konstrukte im Quelltext (drei Konten unter einer Decke), keine Messung",
    ),
    (
        "Fix-Runde 6",
        "Verweis auf einen Abschnitt dieser Datei, keine Messzahl",
    ),
];

/// Der Block der beiden letzten Fix-Runden aus `CHANGELOG.md`, geglättet.
///
/// Von der Überschrift der jüngsten Fix-Runde bis zur Überschrift der
/// drittjüngsten — also genau das, was diese und die vorige Runde geschrieben
/// haben.
fn changelog_block() -> String {
    let text = lf(&std::fs::read_to_string(repo_root().join("CHANGELOG.md")).expect("CHANGELOG"));
    let ueberschriften: Vec<usize> = text
        .match_indices("\n### Fix-Runde ")
        .map(|(i, _)| i)
        .collect();
    assert!(
        ueberschriften.len() >= 3,
        "weniger als drei Fix-Runden im CHANGELOG — der Block lässt sich nicht \
         schneiden"
    );
    glatt(&text[ueberschriften[0]..ueberschriften[2]])
}

/// Markiert jedes Vorkommen von `muster` in `block` als gedeckt.
fn decke(block: &str, gedeckt: &mut [bool], muster: &str) -> bool {
    let mut gefunden = false;
    for (i, _) in block.match_indices(muster) {
        gefunden = true;
        for b in gedeckt.iter_mut().take(i + muster.len()).skip(i) {
            *b = true;
        }
    }
    gefunden
}

/// Der Kern eines Wortes — ohne führende und abschließende Satz- und
/// Auszeichnungszeichen, als Byte-Bereich im Wort.
fn kern(wort: &str) -> (usize, usize) {
    let von = wort
        .char_indices()
        .find(|(_, c)| c.is_alphanumeric())
        .map_or(0, |(i, _)| i);
    let bis = wort
        .char_indices()
        .rfind(|(_, c)| c.is_alphanumeric())
        .map_or(wort.len(), |(i, c)| i + c.len_utf8());
    (von, bis.max(von))
}

/// Trägt dieses Wort eine Zahl — als Ziffer oder ausgeschrieben?
fn traegt_zahl(wort: &str) -> bool {
    // Kardinalzahlen und ihre `…mal`-Formen. **Nicht** die Ordnungszahlen
    // („der vierte“): die stehen hier für einen Platz in einer Liste und nicht
    // für eine Messung.
    const ZAHLWORTE: [&str; 23] = [
        "zwei",
        "drei",
        "vier",
        "fünf",
        "sechs",
        "sieben",
        "acht",
        "neun",
        "zehn",
        "elf",
        "zwölf",
        "siebzehn",
        "zweimal",
        "dreimal",
        "viermal",
        "fünfmal",
        "sechsmal",
        "siebenmal",
        "achtmal",
        "neunmal",
        "zehnmal",
        "elfmal",
        "zwölfmal",
    ];
    let kern: String = wort
        .chars()
        .filter(|c| !"*`„“»«().,;:—–!?[]…\u{202f}".contains(*c))
        .collect();
    if kern.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    let klein = kern.to_lowercase();
    ZAHLWORTE.contains(&klein.as_str())
}

/// **Die Bindung, andersherum.** Keine Zahl im Block der beiden letzten
/// Fix-Runden steht ungebunden da.
///
/// Mutationsnachweis: jede Zahl des Blocks einzeln geändert (Lauf:
/// `scratchpad/runde7/e/zahlen.sh`) — jede macht diesen Test oder
/// `die_zahlen_der_doku_sind_gebunden` rot.
#[test]
fn jede_zahl_der_letzten_runden_ist_gebunden() {
    let block = changelog_block();
    let mut gedeckt = vec![false; block.len()];

    for (datei, satz) in messsaetze() {
        if datei == "CHANGELOG.md" {
            decke(&block, &mut gedeckt, &satz);
        }
    }
    let mut ungenutzt: Vec<&str> = Vec::new();
    for (wortlaut, grund) in KEINE_MESSZAHL {
        assert!(
            grund.len() > 15,
            "„{wortlaut}“ steht ohne belastbare Begründung in KEINE_MESSZAHL"
        );
        if !decke(&block, &mut gedeckt, &glatt(wortlaut)) {
            ungenutzt.push(wortlaut);
        }
    }

    let mut zahlen = 0usize;
    let mut offen: Vec<String> = Vec::new();
    let mut pos = 0usize;
    for wort in block.split(' ') {
        let von = pos;
        pos += wort.len() + 1;
        if !traegt_zahl(wort) {
            continue;
        }
        zahlen += 1;
        // Nicht das ganze Wort, sondern sein **Kern**: `Latin-1);` und `3.`
        // tragen Satzzeichen, die zu keinem gebundenen Satz gehören müssen.
        let (kern_von, kern_bis) = kern(wort);
        if gedeckt[von + kern_von..von + kern_bis].iter().all(|b| *b) {
            continue;
        }
        let links = block[..von]
            .char_indices()
            .rev()
            .nth(60)
            .map_or(0, |(i, _)| i);
        let rechts = block[von..]
            .char_indices()
            .nth(60)
            .map_or(block.len(), |(i, _)| von + i);
        offen.push(format!("„{wort}“ in: …{}…", &block[links..rechts]));
    }

    assert!(
        zahlen >= 100,
        "nur {zahlen} Zahl(en) im Block gefunden — der Schnitt greift nicht mehr"
    );
    assert!(
        offen.is_empty(),
        "{} Zahl(en) im Block der beiden letzten Fix-Runden sind an nichts \
         gebunden — wer sie ändert, ändert eine Zusage, und kein Test merkt es. \
         Entweder gehört ein Satz nach `messsaetze`, oder die Stelle ist keine \
         Messzahl und gehört mit Begründung nach `KEINE_MESSZAHL`:\n{}",
        offen.len(),
        offen.join("\n")
    );
    assert!(
        ungenutzt.is_empty(),
        "{} Eintrag/Einträge in KEINE_MESSZAHL kommen im Block nicht (mehr) vor — \
         eine Ausnahme ohne Fall deckt beim nächsten Mal etwas anderes:\n{}",
        ungenutzt.len(),
        ungenutzt.join("\n")
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
            text.contains(&format!(
                "{} sind es für einen Begriff aus reinem ASCII mit Buchstaben — dort \
                 fällt Latin-1 mit UTF-8 zusammen —, {} mit Umlaut und {} mit einem \
                 Zeichen jenseits von Latin-1",
                zahlwort_gross(gefunden[0].1),
                zahlwort(gefunden[1].1),
                zahlwort(gefunden[2].1)
            )),
            "{wo} nennt die drei Fälle nicht mit den Zahlen aus dem Lauf"
        );
        assert!(
            text.contains(&format!(
                "ist der Hex-String in Groß- und in Kleinschreibung dieselbe Bytefolge, \
                 dort sind es {}",
                zahlwort(gefunden[3].1)
            )),
            "{wo} nennt die Zahl für die IBAN nicht"
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
    let readme = glatt(&lf(
        &std::fs::read_to_string(repo_root().join("README.md")).expect("README lesbar")
    ));
    assert!(
        security.contains("Fünf Gründe gibt es dafür")
            || readme.contains("Fünf Gründe gibt es dafür"),
        "die README nennt die Zahl der Gründe nicht"
    );
    // Und die Rückgabewert-Tabelle nennt dieselbe Zahl: sie war bis zur
    // Fix-Runde 7 an nichts gebunden.
    assert!(
        readme.contains(&format!(
            "(`NICHT GEPRÜFT: …`, {} mögliche Gründe, siehe unten)",
            zahlwort(GRUENDE.len())
        )),
        "die Rückgabewert-Tabelle der README nennt nicht {} mögliche Gründe",
        GRUENDE.len()
    );
}

// ===========================================================================
// Die Spiegel-Bombe der Fix-Runde 6 — am gebauten Binary, nicht im Testprozess
// ===========================================================================
//
// Die Tabelle in `redact-pdf/tests/zf_q3_kombinatorik.rs` misst `analyse()`:
// laden und `PdfExtractor::extract_with_warnings`. Das ist **ein Teil** des
// Programms, und die Zahlen daraus (0,56 s / 38 MB) sind die des
// Testprozesses. Das gebaute Binary tut mehr — Muster, Schwärzung,
// Zusammenfassung —, und die Doku darf nur sagen, was es gemessen hat.
//
// Deshalb baut dieser Block dieselbe Datei noch einmal (Nachbau von
// `nested_brackets(6000, 6000, false)`) und fährt sie durch das Binary. Die
// Zeit und der Spitzenspeicher stehen als aufgezeichnete Messung in
// [`messwerte`]; **hier** gebunden sind die Eigenschaften, die nicht von der
// Maschine abhängen: die Größe der Datei, die Zahl ihrer Objekte und der
// Rückgabewert des Laufs.

/// Nachbau von `redact-pdf/tests/zf_q3_kombinatorik.rs::nested_brackets(b, d,
/// false)`: `b` verschachtelte `/Span <</ActualText (Ai)>> BDC`-Klammern über
/// `d` Platzierungen desselben Form-XObjects.
fn spiegel_bombe(klammern: usize, dos: usize) -> Vec<u8> {
    use lopdf::{dictionary, Document, Object, Stream};

    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let form_resources = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let form_id = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                "Resources" => form_resources,
            },
            b"BT /F1 10 Tf 72 600 Td (A) Tj ET\n".to_vec(),
        )
        .with_compression(false),
    ));
    doc.get_dictionary_mut(resources_id)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Fm1" => form_id });

    let mut raw = b"BT\n/F1 10 Tf\n72 700 Td\n(Kontoinhaber Max Mustermann) Tj\nET\n".to_vec();
    for i in 0..klammern {
        raw.extend_from_slice(format!("/Span <</ActualText (A{i})>> BDC\n").as_bytes());
    }
    raw.extend_from_slice("/Fm1 Do\n".repeat(dos).as_bytes());
    for _ in 0..klammern {
        raw.extend_from_slice(b"EMC\n");
    }
    doc.objects
        .insert(content_id, Object::Stream(Stream::new(dictionary! {}, raw)));

    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).expect("speicherbar");
    buffer
}

/// Die Datei ist so groß und hat so viele Objekte, wie die Doku sagt — und der
/// Lauf des **Binaries** endet, wie die Doku sagt.
///
/// Zeit und Spitzenspeicher stehen als aufgezeichnete Messung in [`messwerte`]
/// (eine geteilte Maschine ist kein Messgerät); hier steht, was nicht von ihr
/// abhängt. Die Doku sagte bis zur Fix-Runde 7 „elf Objekte“ und schrieb die
/// Zahlen des Testprozesses als die des Binaries.
///
/// Mutationsnachweis: `SPIEGEL_BOMBE_OBJEKTE` auf 11 → rot;
/// `SPIEGEL_BOMBE_KB` auf „277“ → rot.
#[test]
fn die_spiegel_bombe_am_binary() {
    let bytes = spiegel_bombe(6000, 6000);
    let objekte = redact_pdf::load_from_bytes(&bytes)
        .expect("ladbar")
        .objects
        .len();
    println!("Datei {} B, {objekte} Objekte", bytes.len());
    assert_eq!(
        objekte,
        messwerte::SPIEGEL_BOMBE_OBJEKTE,
        "die Datei hat {objekte} Objekte, die Doku sagt {}",
        messwerte::SPIEGEL_BOMBE_OBJEKTE
    );
    assert_eq!(
        ((bytes.len() + 500) / 1000).to_string(),
        messwerte::SPIEGEL_BOMBE_KB,
        "die Datei ist {} Byte groß, die Doku sagt {} kB",
        bytes.len(),
        messwerte::SPIEGEL_BOMBE_KB
    );

    let dir = workdir("spiegel");
    std::fs::write(dir.join("spiegel.pdf"), &bytes).expect("schreibbar");
    // Der Pfad bleibt stehen (und wird gemeldet): dieselbe Datei wird für die
    // aufgezeichnete Messung durch das **Release**-Binary gefahren, und ein
    // Beleg, den man nicht nachfahren kann, ist keiner.
    println!("Datei liegt unter {}", dir.join("spiegel.pdf").display());
    let start = std::time::Instant::now();
    let out = run_in(&dir, &["spiegel.pdf", "-o", "aus.pdf"]);
    let text = stdout(&out);
    println!(
        "rc={:?} nach {:?}\n{text}{}",
        out.status.code(),
        start.elapsed(),
        stderr(&out)
    );
    assert_eq!(
        out.status.code(),
        Some(RC_LECK),
        "die Doku nennt für diesen Lauf Rückgabewert {RC_LECK}:\n{text}"
    );
}
