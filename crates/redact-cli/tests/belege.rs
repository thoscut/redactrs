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
    ///
    /// [`DECKE_SPITZE_KB`] beschreibt den **ungedeckelten** Zustand und ist
    /// damit eine Zahl nach der Regel „Zahlen, die den Zustand VOR der
    /// Korrektur beschreiben“: an diesem Baum gibt sie kein Lauf mehr her, sie
    /// steht an [`UNGEDECKELT_STAND`]. `kb_in_mb` rechnet sie mit 1024² um —
    /// 6 288 MB. Die Doku **am Quelltext** (`content.rs`, `redact.rs`,
    /// `zg_r1_decke.rs`) nennt dieselbe Messung „6 439 MB“; das ist derselbe
    /// kB-Wert durch 1000 geteilt, also Dezimal-MB in einer Datei, die sonst
    /// überall mit 1024² rechnet. Diese drei Stellen gehören nicht hierher und
    /// stehen im Bericht als Vertrag.
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
    /// So oft war der Windows-Job der CI an dieser Fehlerklasse rot: der
    /// dumpable-Test, `/proc/self/status` im Orakel-Budget, der Pfeil nach
    /// cp1252 — und die Plattformregel selbst, die Pfade mit `/` erwartete.
    pub const WINDOWS_ROT: usize = 4;
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
    /// So viele Gründe kannte das Orakel nach der Fix-Runde 7 — die Zahl, die
    /// der CHANGELOG jener Runde nennt. Seit der Spur-A-Runde 2 sind es
    /// [`super::GRUENDE`] (Register #98).
    pub const GRUENDE_FIX_RUNDE_7: usize = 5;
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
    /// Lauf **im Testprozess**, Profil **Release** (Weg 2 der Liste oben):
    /// `strip_metadata` liegt im Baustein, das Material entsteht im Speicher
    /// und keine Kommandozeile zeigt es
    /// (`zg_r3_kosten::zweihunderttausend_annotationen_mit_verweis_contents_in_sekunden`).
    ///
    /// Das Profil ist **nachgewiesen, nicht geraten**: derselbe Test in Debug
    /// braucht hier 6,02 / 6,05 / 6,08 s (drei Läufe dieser Runde), also das
    /// Achtfache — aus einem Debug-Lauf kann [`ANNOT_NACHHER_MS`] nicht
    /// stammen. Der Absolutwert ist nicht wiederholbar: ein Release-Lauf
    /// desselben Tests kam hier auf 1,148 / 1,151 s, allerdings aus dem
    /// Release-Testbinär vom Stand des Gates der Runde 6 (`meta.rs` hat sich
    /// seither geändert) und auf einer Maschine, auf der gleichzeitig gebaut
    /// wurde. `SECURITY.md` sagt es für jede Zeit dieses Projekts: eine
    /// geteilte Maschine ist kein Messgerät. Gebunden ist deshalb die
    /// aufgezeichnete Spanne, und der Satz nennt jetzt ihren Weg.
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
    /// Wie viele Blöcke jedes Muster der alten Suche an einer Datei mit einem
    /// 64-MiB-Strom durchlief: Rohdatei, roher Stromblock gepackt und
    /// entpackt, derselbe Strom über den Objektgraphen dekodiert.
    pub const DURCHGANG_BLOECKE: usize = 4;
    /// **Abgeleitet, nicht abgeschrieben** — und deshalb in 1024²-MB, wie der
    /// Vorspann es für jedes MB dieses Projekts festlegt. Hier stand „268“:
    /// dieselbe Menge dezimal gerechnet, derselbe Fehler, den der Block seiner
    /// Nachbarzahl („205 MB vorher, 138 MB nachher, dort in Dezimal-MB
    /// gezählt“) ausdrücklich anschreibt. `audit_bytes.rs` nennt die Menge
    /// seit dieser Runde ebenso, und
    /// `zl_e_gegenwartszahl_ohne_weg::die_mb_je_durchgang_steht_in_beiden_dateien_gleich`
    /// hält die beiden Stellen zusammen.
    pub const DURCHGANG_MB: usize = DURCHGANG_BLOECKE * KLON_STROM_MIB;
    pub const MEMMEM_GB_S: &str = "9,9";
    pub const MEMMEM_JE_BEGRIFF_S: &str = "0,163";
    pub const HEUTE_1_S: &str = "5,01";
    pub const HEUTE_1000_S: &str = "5,99";

    // --- Die zwei Dienstverweigerungen der Fix-Runde 7, nachgemessen ------
    //
    // Die Agenten der Runde 7 hatten diese Zahlen nur in die Doku **am
    // Quelltext** geschrieben (`content.rs`, `redact.rs`); der CHANGELOG-Block
    // sagte deshalb, *was* sich geändert hat, und nicht, um wie viel. Hier
    // stehen sie am Baum dieser Runde nachgemessen, mit dem Lauf, aus dem sie
    // stammen.
    //
    // ## Der Ort der Messung: zwei erlaubte Wege, und nur zwei
    //
    // Die Regel dieses Projekts ist: eine Zahl in der Doku gilt, wenn sie aus
    // einem Lauf des **gebauten Binaries** stammt. Die Nachbesserung dieser
    // Runde hat sie eingelöst, wo die Kommandozeile die Größe zeigt — und wo
    // sie sie nicht zeigt, steht das jetzt **im Satz selbst**, samt Profil.
    // Erlaubt sind genau zwei Wege:
    //
    // 1. **Am gebauten Binary** (`target/release/redact-rs`, Release).
    //    Zeit und Spitze kommen von `/usr/bin/time -v`; die Spitze ist
    //    dessen `Maximum resident set size` (kB = 1024 Byte), also derselbe
    //    Höchststand, den `VmHWM` im Prozess meldet. Das Material schreibt
    //    `zg_r1_decke::schreibt_material` über `R1_OUT` — die Datei entsteht
    //    damit auf der Kommandozeile und nicht im Kopf.
    // 2. **Im Testprozess**, wenn die Größe eine Eigenschaft ist, die die
    //    Kommandozeile gar nicht herausgibt: die Dauer eines einzelnen
    //    `scan_page` liegt innerhalb des Extraktors, und die Statuszeile
    //    gehört der Oberfläche, die keine Kommandozeile hat. Dann nennt der
    //    Satz in der Doku das Wort „Testprozess“ **und** das Profil.
    //
    // Was keiner der beiden Wege ist: eine Debug-Zahl still als Binary-Zahl
    // ausgeben. Genau das war der Einwand der Gegenprüfung, und genau dafür
    // steht diese Liste.
    //
    // ## Zahlen, die den Zustand VOR der Korrektur beschreiben
    //
    // Eine Zahl, die den Schaden beziffert, den eine Korrektur beseitigt, ist
    // nach der Korrektur **nicht mehr messbar** — kein Lauf an diesem Baum
    // gibt sie her. Sie ist deshalb nicht falsch und nicht zu streichen: sie
    // ist ein Beleg von damals, keine Zusage über heute. Damit sie
    // nachvollziehbar bleibt, gilt die Regel:
    //
    // > Eine Zahl des alten Zustands sagt im Satz, **dass** sie den Zustand
    // > vor der Korrektur beschreibt, und nennt den **Stand**, an dem dieser
    // > Zustand steht ([`UNGEDECKELT_STAND`]). Dann ist sie prüfbar — durch
    // > Auschecken dieses Standes —, auch wenn sie nicht wiederholbar ist.
    //
    // Wiederholbar heißt: derselbe Lauf am selben Baum liefert sie wieder.
    // Nachvollziehbar heißt: es steht da, welcher Baum sie liefert. Das
    // Zweite ist zu haben, das Erste nicht, und eine Zahl ohne beides gehört
    // gestrichen.

    /// `BDC`-Klammern über Textoperationen: `B` verschachtelte Klammern über
    /// `S` Textoperationen ergeben `B × S` Zuordnungen, aus einer Datei, die
    /// dafür keinen Inhalt mitbringen muss. Die Zahl der Zuordnungen wird
    /// **abgeleitet** (Produkt der beiden Faktoren), nicht abgeschrieben.
    ///
    /// Lauf (**Testprozess**, Debug — `scan_page` liegt im Extraktor, die
    /// Kommandozeile zeigt es nicht; Weg 2 der Liste oben), vier Läufe:
    /// `R1_PAGES=1 R1_B=6000 R1_S=6000 cargo test -p redact-pdf --test
    /// zg_r1_decke -- --ignored --nocapture --exact
    /// mess_klammern_mal_textoperationen`
    /// → `Datei 263475 B`, `scan_page 301,6 / 310,6 ms` (Runde 7) und
    /// `298,0 / 299,2 ms` (nachgemessen in dieser Runde),
    /// `100000 Textzuordnungen`, `1 Warnung(en)`,
    /// `VmHWM 36352 / 36356 / 36504 kB`. Gebunden ist die Spanne über alle
    /// vier Läufe und der höchste gesehene Höchststand.
    pub const TJ_KLAMMERN: usize = 6_000;
    pub const TJ_OPERATIONEN: usize = 6_000;
    pub const TJ_DATEI_BYTES: u64 = 263_475;
    pub const TJ_SCAN_S: &str = "0,30–0,31";
    pub const TJ_SPITZE_KB: u64 = 36_504;

    /// Derselbe Fall **am gebauten Binary** (Weg 1) — der Lauf, der der
    /// Runde 7 fehlte.
    ///
    /// Das Material kommt von
    /// `R1_OUT=… R1_PAGES=1 R1_B=6000 R1_S=6000 cargo test -p redact-pdf
    /// --test zg_r1_decke --release -- --ignored --exact schreibt_material`
    /// als `text_1_6000x6000.pdf`. Es ist dieselbe Struktur wie oben mit
    /// **einem** `Do` mehr (`schreibt_material` setzt `mit_do`), daher
    /// 263 699 statt 263 475 Byte — deshalb steht die Größe hier eigens und
    /// wird nicht von [`TJ_DATEI_BYTES`] geborgt.
    ///
    /// Lauf, drei Mal:
    /// `/usr/bin/time -v target/release/redact-rs text_1_6000x6000.pdf -o … -f`
    /// → `0:00.11 / 0:00.12 / 0:00.13`,
    /// `Maximum resident set size 35340 / 35340 / 35416 kB`,
    /// `Exit status 3`, und auf der Konsole die Decke selbst
    /// („mehr als 100000 Zuordnungen zwischen einem Spiegel und einer
    /// Textoperation“) sowie `7 Stelle(n) … nicht durchsucht`. Das Binary:
    /// `redact-rs 0.6.0`, md5 `d894de2fa3e0e59af3931fc3f76c1e2e`, gebaut
    /// 2026-09-21 01:11 — also nach den Korrekturen der Runde 7 und vor den
    /// Änderungen, die in dieser Runde noch laufen.
    pub const TJ_BINARY_BYTES: u64 = 263_699;
    pub const TJ_BINARY_S: &str = "0,11–0,13";
    pub const TJ_BINARY_KB: u64 = 35_416;
    pub const TJ_BINARY_UNGEPRUEFT: usize = 7;

    /// Derselbe Fall, den [`DECKE_DATEI_BYTES`] und [`DECKE_SPITZE_KB`] als
    /// Befund festhalten — jetzt **nach** der dokumentweiten Decke und **am
    /// gebauten Binary** (Weg 1).
    ///
    /// Die Runde 7 band hier `45,5–45,6 s / 38 532 kB` aus dem Testprozess
    /// (Debug, `R1_SKIP_EXTRACT=1`, damit `VmHWM` den Redaktor allein trägt).
    /// Diese Zahl **hielt nicht**: zwei neue Läufe desselben Befehls gaben
    /// `45,001 s / 39 404 kB` und `45,289 s / 39 384 kB` — beide Zeiten unter
    /// der gebundenen Spanne, beide Spitzen darüber. Eine Spanne von einem
    /// Zehntel über eine 45-Sekunden-Messung auf einer geteilten Maschine ist
    /// keine Zusage, die dieses Projekt halten kann; sie ist deshalb
    /// gestrichen und nicht bloß verschoben.
    ///
    /// Gebunden ist stattdessen der Lauf, der der Runde 7 fehlte. Das
    /// Material schreibt
    /// `R1_OUT=… R1_PAGES=1000 R1_B=100 R1_D=999 cargo test -p redact-pdf
    /// --test zg_r1_decke --release -- --ignored --exact schreibt_material`
    /// als `seiten_1000_100x999.pdf`, **bytegleich** zu der Datei der
    /// Messung: 224 752 Byte, also genau [`DECKE_DATEI_BYTES`].
    ///
    /// Lauf, drei Mal:
    /// `/usr/bin/time -v target/release/redact-rs seiten_1000_100x999.pdf -o … -f`
    /// → `0:24.74 / 0:24.88 / 0:24.88`,
    /// `Maximum resident set size 88260 / 88312 / 88436 kB`,
    /// `Exit status 0`, `Treffer gesamt: 0`, keine Warnung.
    ///
    /// Diese Spitze trägt Extraktor **und** Redaktor, weil die Kommandozeile
    /// die beiden nicht trennt — sie ist damit größer als die 38 MB, die der
    /// Testprozess dem Redaktor allein zumaß, und bleibt drei
    /// Größenordnungen unter den [`DECKE_SPITZE_KB`] des ungedeckelten
    /// Zustands. Das ist die Aussage, um die es geht, und sie steht jetzt auf
    /// einem Lauf des Binaries.
    ///
    /// Die 1 000 Seiten × 100 Klammern sind genau `MAX_DEFERRED_MIRRORS`
    /// zurückgestellte Abschnitte, und dass die Decke dort still bleibt und
    /// einen Schritt weiter nicht, hält
    /// `zg_r1_decke::genau_an_der_dokumentweiten_decke_bleibt_es_still` fest.
    pub const DECKE_BINARY_S: &str = "24,7–24,9";
    pub const DECKE_BINARY_KB: u64 = 88_436;

    /// Der Stand, an dem die Zahlen des **ungedeckelten** Zustands stehen:
    /// der letzte Baum ohne `redact::MAX_DEFERRED_MIRRORS`, also vor der
    /// Arbeit der Fix-Runde 7 (die mit `5142160` beginnt).
    ///
    /// Prüfbar mit einem Griff:
    /// `git show 308ef38:crates/redact-pdf/src/redact.rs | grep -c
    /// MAX_DEFERRED_MIRRORS` → `0`; an `HEAD` ist die Decke da. Die Zahlen,
    /// die diesen Stand beschreiben — `36 000 000` Zuordnungen, Redaktor
    /// `105 s` / [`DECKE_SPITZE_KB`], Extraktor `31,1 s`, `3 320 MB`
    /// Eigenschaftslisten — gibt kein Lauf an diesem Baum mehr her. Sie
    /// nennen deshalb diesen Stand, nach der Regel oben.
    pub const UNGEDECKELT_STAND: &str = "308ef38";

    /// Die drei Zahlen des ungedeckelten Zustands, die die Doku **am
    /// Quelltext** trägt und die der CHANGELOG nur noch nennt, um zu sagen,
    /// dass sie nicht mehr messbar sind.
    ///
    /// Stellen: `content.rs` (Extraktor, zweimal), `redact.rs` (Redaktor und
    /// die Listen), `zg_r1_decke.rs` (beide). Alle drei gehören nach der Regel
    /// zu [`UNGEDECKELT_STAND`].
    pub const UNGEDECKELT_EXTRAKTOR_S: &str = "31,1";
    pub const UNGEDECKELT_REDAKTOR_S: &str = "105";
    pub const UNGEDECKELT_LISTEN_MB: &str = "3 320";

    /// Der Stand, an dem die Zahlen des Zustands **vor der Fix-Runde 6**
    /// stehen: der letzte Baum, in dem die Decke unter den Textspiegeln
    /// Formularplatzierungen zählte und nicht Zuordnungen. Dort lief die
    /// Spiegel-Bombe ([`SPIEGEL_BOMBE_KB`] → [`SPIEGEL_BOMBE_S`] /
    /// [`SPIEGEL_BOMBE_MB`]) ohne Schranke.
    ///
    /// Prüfbar mit einem Griff:
    /// `git show fedcabe:crates/redact-pdf/src/content.rs | grep -c
    /// "Zuordnungen zwischen einem Spiegel"` → `0`; an `HEAD` steht die Decke
    /// dort. Diese Zahlen gibt kein Lauf an diesem Baum mehr her; die Sätze
    /// nennen deshalb diesen Stand, nach der Regel oben.
    pub const VOR_RUNDE6_STAND: &str = "fedcabe";

    /// Die Zahl der neuen Messzahlen der Fix-Runde 7, die die Gegenprüfung
    /// als Testprozess-Messungen beanstandete — und die Zahl derer, die es
    /// bleiben, weil die Kommandozeile die Größe nicht herausgibt
    /// (`scan_page` im Extraktor, die Statuszeile in der Oberfläche).
    pub const NEUE_MESSZAHLEN_7: usize = 3;
    pub const TESTPROZESS_ZAHLEN_7: usize = 2;

    /// Die gestrichene Zahl und die zwei Läufe, die sie widerlegten.
    ///
    /// Sie steht hier, damit die Doku sie nennen darf: ein Satz, der sagt
    /// „diese Zahl hielt nicht“, trägt die Zahl notwendig mit sich, und auch
    /// eine widerlegte Zahl ist eine Zahl im Block.
    ///
    /// Lauf, zwei Mal: `R1_PAGES=1000 R1_B=100 R1_D=999 R1_SKIP_EXTRACT=1
    /// cargo test -p redact-pdf --test zg_r1_decke -- --ignored --nocapture
    /// --exact mess_seiten_mal_paare` → `45.001247419s` / `VmHWM 39404 kB`
    /// und `45.28853015s` / `VmHWM 39384 kB`.
    pub const WIDERLEGT_DECKE_S: &str = "45,5–45,6";
    pub const WIDERLEGT_DECKE_MB: u64 = 38;
    pub const NEU_DECKE_S1: &str = "45,001";
    pub const NEU_DECKE_KB1: u64 = 39_404;
    pub const NEU_DECKE_S2: &str = "45,289";
    pub const NEU_DECKE_KB2: u64 = 39_384;

    /// Die Statuszeile mit den echten Stellen eines Laufs, **vor** der
    /// Kürzung der Fix-Runde 7.
    ///
    /// Lauf **im Testprozess**, Profil **Debug** (Weg 2 der Liste oben): die
    /// Statuszeile gehört der Oberfläche, und die hat keine Kommandozeile —
    /// am gebauten `redact-rs` ist diese Zahl nicht zu holen, weder in
    /// Release noch in Debug. Der Satz in der Doku sagt das deshalb selbst.
    ///
    /// `cargo test -p redact-gui --lib
    /// zg_r4_3_die_statuszeile_ist_als_ganzes_gedeckelt -- --nocapture`
    /// → `der ganze Satz (1005 Zeichen)`, zwei Mal in dieser Runde
    /// nachgemessen, beide Male genau 1005.
    ///
    /// Anders als eine Zeit hängt eine **Zeichenzahl** nicht am Profil: der
    /// Satz ist derselbe String, ob optimiert gebaut oder nicht. Das macht
    /// den Testprozess hier nicht zur Ausnahme von der Regel, sondern zum
    /// einzigen Ort, an dem die Zahl überhaupt entsteht.
    ///
    /// Die 981, die die Doku am Quelltext (`state.rs`, `zg_r4_tests.rs`) dafür
    /// nannte, stammen aus einer früheren Fassung der Messung — sie nennen als
    /// Beleg einen Testnamen, den es nicht mehr gibt
    /// (`zg_r4_3_die_laengste_statuszeile_ist_wieder_ueber_804_zeichen`).
    /// Gebunden ist, was die Messung **heute** liefert.
    pub const STATUSZEILE_ZEICHEN: usize = 1_005;

    // Debug-Information der Testbinaries (nach der Runde 9, Register #59).
    //
    // Gemessen mit `ls -l` und `readelf -S -W` an
    // `target/debug/deps/zm_b_gleichzeitige_exporte-*` (Linux, ELF), MB =
    // 1024² Byte; `du -sh target` fuer das Ganze. „Vorher“ ist der Stand
    // `1282b16` ohne `[profile.dev]` — nach der Umstellung nicht mehr
    // messbar, deshalb Weg 3 im Satz. „Nachher“ nach `cargo clean` und dem
    // ganzen Gate — Weg 1, an den gebauten Binaries.
    pub const DEBUG_BINARY_VORHER_MB: &str = "240,7";
    pub const DEBUG_SEKTIONEN_VORHER_MB: &str = "219,4";
    pub const DEBUG_ANTEIL_VORHER_PROZENT: &str = "91";
    pub const DEBUG_TARGET_VORHER_GB: &str = "19";
    pub const DEBUG_BINARY_NACHHER_MB: &str = "78,0";
    pub const DEBUG_SEKTIONEN_NACHHER_MB: &str = "58,3";
    pub const DEBUG_TARGET_NACHHER_GB: &str = "8,0";

    // Die Verweiskette (Spur-A-Runde 1, Register #72).
    //
    // Gemessen mit `zo_b_traeger::c_mess_verweiskette` (`--ignored
    // --nocapture`): `strip_metadata` an einer Kette aus n Objekten, die
    // nichts als ein Verweis sind — im Testprozess, Profil Debug (Weg 2; die
    // Kommandozeile gibt die Dauer dieses einen Schritts nicht her).
    // „Vorher“ ist der Stand [`VERWEISKETTE_STAND`], an dem `Chains::of`
    // jedes Glied bis zum Ende laeuft — nach der Korrektur nicht mehr
    // messbar, deshalb Weg 3 im Satz. Die Dateigroesse ist die der Kette
    // mit `KETTENGLIEDER` Gliedern (`zo_b_traeger.rs`).
    pub const VERWEISKETTE_STAND: &str = "456d669";
    pub const VERWEISKETTE_GLIEDER_VORHER: u64 = 8_000;
    pub const VERWEISKETTE_VORHER_8000_S: &str = "33";
    pub const VERWEISKETTE_VORHER_20000_S: &str = "223";
    pub const VERWEISKETTE_NACHHER_S: &str = "0,08";
    pub const VERWEISKETTE_DATEI_KB: u64 = 936;

    // Die gehaltenen Formularspiegel (Spur-A-Runde 1, Register #68).
    //
    // Gemessen mit `zo_c_spiegel_umgebungen::mess_spiegel_in_formularen_je_seite`
    // (`ZO_C_PAGES=…`, eigener Prozess, VmHWM in MB = 1024 kB): der Redaktor
    // an je Seite einem eigenen Formular mit 100 × 999 Spiegel-Paaren —
    // Testprozess, Profil Debug (Weg 2). „Vorher“ ist der Stand
    // [`FORMULARSPIEGEL_STAND`], an dem `form_marked` den Datensatz des Scans
    // ungekuerzt haelt — nach der Korrektur nicht mehr messbar, deshalb Weg 3
    // im Satz. Die Zahl der Seiten und die Decke der Probe bindet
    // `konstante_aus` an der Belegdatei, die Decke des Kontos an `redact.rs`.
    pub const FORMULARSPIEGEL_STAND: &str = "e016a2f";
    pub const FORMULARSPIEGEL_SEITEN_KLEIN: u64 = 10;
    pub const FORMULARSPIEGEL_KLAMMERN: u64 = 100;
    pub const FORMULARSPIEGEL_PLATZIERUNGEN: u64 = 999;
    pub const FORMULARSPIEGEL_VORHER_KLEIN_MB: u64 = 78;
    pub const FORMULARSPIEGEL_VORHER_GROSS_MB: u64 = 637;
    pub const FORMULARSPIEGEL_VORHER_GROSS_S: &str = "9,8";
    pub const FORMULARSPIEGEL_DATEI_JE_SEITE_KB: u64 = 10;
    pub const FORMULARSPIEGEL_NACHHER_KLEIN_MB: u64 = 20;
    pub const FORMULARSPIEGEL_NACHHER_GROSS_MB: u64 = 31;

    // Die Kettenbombe auf dem Schreibpfad (Spur-A-Runde 1, Register #64).
    //
    // Gemessen am gebauten Binary (Weg 1, `/usr/bin/time -v`, Maximum
    // resident set size) am Stand [`KETTENBOMBE_STAND`], an dem die
    // Vorpruefung die Kette roh buchte — nach der Korrektur nicht mehr
    // messbar (Weg 3 im Satz): die Datei aus `zo_e_kettenbombe_schreibpfad`
    // (39 381 Byte), 20 000 000 RunLength-Laeufe zu 128 Byte = 2,4 GB
    // entpackt. Die Adressraumgrenze ist die des Belegs (`ulimit -v`).
    pub const KETTENBOMBE_STAND: &str = "14d03c7";
    pub const KETTENBOMBE_DATEI_KB: u64 = 39;
    pub const KETTENBOMBE_ENTPACKT_GB: &str = "2,4";
    pub const KETTENBOMBE_SPITZE_GB: &str = "3,8";
    pub const KETTENBOMBE_ADRESSRAUM_GIB: &str = "1,5";

    // Der Vorspann vor einem Bildfilter (Spur-A-Runde 1, Register #83).
    //
    // Gemessen am gebauten Binary (Weg 1, Debug, `/usr/bin/time -v`,
    // Maximum resident set size) am Stand [`VORSPANN_STAND`], an dem die
    // Vorpruefung `[/FlateDecode /DCTDecode]` roh buchte — nach der
    // Korrektur nicht mehr messbar (Weg 3 im Satz): 3 131 802 Byte Datei,
    // 3 GiB Nullen im Flate-Glied, 3 241 844 KB Spitze bei
    // `--max-decompressed-mb 64`. Probedateien und Erzeuger im Arbeitsbuch
    // der Runde, nicht im Baum; `zo_e_vorspann_vor_bildfilter` baut die
    // kleinere Form nach.
    pub const VORSPANN_STAND: &str = "72711d0";
    pub const VORSPANN_DATEI_MB: u64 = 3;
    pub const VORSPANN_NULLEN_GIB: u64 = 3;
    pub const VORSPANN_SPITZE_GB: &str = "3,1";
    pub const VORSPANN_BUDGET_MB: u64 = 64;

    // Die Bilddecke gegen die Maße im JPEG-Kopf (Spur-A-Runde 2, Register
    // #101). Gemessen am gebauten Binary (Weg 1, Debug, `/usr/bin/time -v`)
    // am Stand [`JPEGKOPF_STAND`], an dem der Dekoder die SOF-Maße ungeprüft
    // belegte (Weg 3 im Satz): 268 184 KB Spitze, Rückgabewert 0.
    pub const JPEGKOPF_STAND: &str = "2edf407";
    pub const JPEGKOPF_DICT: &str = "100 × 100";
    pub const JPEGKOPF_JPEG: &str = "6000 × 6000";
    pub const JPEGKOPF_SPITZE_MB: u64 = 262;
    pub const JPEGKOPF_DECKE_MB: u64 = 8;

    // Die Vorprüfung als Schranke (Spur-A-Runde 2, Register #89). Gemessen
    // am gebauten Binary (Weg 1, Debug, `/usr/bin/time -v`, Maximum resident
    // set size) am Stand [`SCHRANKE_STAND`], an dem die Vorprüfung eine Kette
    // mit Verweis roh buchte (Weg 3 im Satz): 1 044 167 Byte Datei, 1 GiB
    // Nullen hinter `/Filter 6 0 R`, 2 180 744 KB Spitze bei
    // `--max-decompressed-mb 64`, Rückgabewert 1; rohes Deflate 2 181 132 KB,
    // der Schlüssel mit `#xx` 2 180 856 KB. Nach der Korrektur: Budgetmeldung,
    // rund 81 MB Spitze. Probedateien und Erzeuger im Arbeitsbuch der Runde;
    // `zp_e_vorpruefung_als_schranke` baut die kleinere Form nach.
    pub const SCHRANKE_STAND: &str = "f6c5c68";
    pub const SCHRANKE_DATEI_MB: u64 = 1;
    pub const SCHRANKE_NULLEN_GIB: u64 = 1;
    pub const SCHRANKE_SPITZE_GB: &str = "2,1";
    pub const SCHRANKE_BUDGET_MB: u64 = 64;

    // Die Arbeit der Kette (Spur-A-Runde 2, Register #99). Gemessen am
    // gebauten Binary (Weg 1, Debug, `/usr/bin/time -v`) am Stand
    // [`ARBEIT_STAND`], an dem nur die Ausgabe des letzten Glieds zählte (Weg 3
    // im Satz): 73 610 Byte Datei, zweihundert Ströme
    // `[/FlateDecode /FlateDecode /ASCIIHexDecode]` über je 60 MiB Nullen;
    // Schwärzen 3:52,32 Wanduhr, Rückgabewert 0; `--check-leaks` von
    // `timeout 400` beendet. Nach der Korrektur beide 1,22 s, Budgetmeldung.
    pub const ARBEIT_STAND: &str = "9bda3b6";
    pub const ARBEIT_DATEI_KB: u64 = 72;
    pub const ARBEIT_STROEME: u64 = 200;
    pub const ARBEIT_NULLEN_MIB: u64 = 60;
    pub const ARBEIT_DAUER: &str = "3 min 52 s";
    pub const ARBEIT_BUDGET_MB: u64 = 64;
    pub const ARBEIT_ORAKEL_S: u64 = 400;
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
            zahlwort(m::GRUENDE_FIX_RUNDE_7)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Alle {} stehen jetzt mit ihrem Wortlaut in einer Tabelle; ein Test hält \
             jeden gegen den Quelltext des Orakels, {} davon zusätzlich gegen einen \
             Lauf des gebauten Binaries",
            zahlwort(m::GRUENDE_FIX_RUNDE_7),
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
             Rückgabewert {RC_LECK}); im Testprozess (Debug), der nur den Extraktor \
             fährt, {} s und {} MB; ohne `/ActualText` brauchte sie immer {} s",
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
            "gemessen im Testprozess (Release, {}-MiB-Strom, `/DCTDecode`, Budget \
             {} MiB) **{} MB vorher, {} MB nachher**",
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
             nachstellbare Rechnung ersetzt, gemessen im Testprozess (Release): {} \
             Muster je Begriff über {} MB je Durchgang — {} Blöcke à {} MiB, MB \
             wie überall 1024² Byte —, `memmem` {} GB/s → {} s je Begriff, rund \
             **{} s für {} Begriffe als untere Schranke**; heute {} s \
             (1 Begriff) gegen {} s ({})",
            mit_tausendertrenner(redact_core::MAX_CHECK_NEEDLES as u64),
            m::ALTE_KOSTENZAHL_S,
            m::MUSTER_JE_BEGRIFF_ALT,
            m::DURCHGANG_MB,
            zahlwort(m::DURCHGANG_BLOECKE),
            m::KLON_STROM_MIB,
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
             Byte belegten beim Schwärzen {} MB, ungedeckelt am Stand `{}`) —, {} \
             Befunde an der Oberfläche und die Erkenntnis, dass von {} einzeln \
             mutierten Zahlen der Doku weiter {} grün blieben",
            m::DECKE_SEITEN,
            mit_tausendertrenner(m::DECKE_DATEI_BYTES),
            mit_tausendertrenner(kb_in_mb(m::DECKE_SPITZE_KB)),
            m::UNGEDECKELT_STAND,
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
             MB“ für die entschärfte Spiegel-Bombe stammten aus dem **Testprozess** \
             (Debug)",
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
    // Seit der Spur-A-Runde 2 (Register #98) fünf Zähler; die Zeile über
    // Sicht 7 und die abgelehnten Seiten schließen einander aus.
    satz(
        "SECURITY.md",
        format!(
            "denn `LeakCheck::unchecked` darf bis zu **{}** Zeilen tragen",
            5 * (konstante_aus("crates/redact-pdf/src/audit_bytes.rs", "MAX_UNCHECKED") + 1)
        ),
    );
    satz(
        "SECURITY.md",
        format!(
            "die Decke `MAX_UNCHECKED = {}` einzeln genannter Stellen plus Summenzeile gilt je **Zähler**, und davon gibt es {} (zu große Ströme der Rohsicht, dieselben der Objektsicht, Stellen aus anderem Grund, verlesene oder in der Rohsicht nicht dekodierte Ströme, abgelehnte Seiten): {} × {}",
            konstante_aus("crates/redact-pdf/src/audit_bytes.rs", "MAX_UNCHECKED"),
            zahlwort(5),
            5,
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

    // --- Die Messzahlen der beiden Dienstverweigerungen ------------------
    //
    // Der erste Weg, vor der Decke: das Produkt aus Klammern und
    // Textoperationen wird **abgeleitet**, nicht abgeschrieben.
    satz(
        "CHANGELOG.md",
        format!(
            "{} Klammern über {} `Tj` aus einer Datei von {} Byte ergaben {} Zuordnungen",
            mit_tausendertrenner(m::TJ_KLAMMERN as u64),
            mit_tausendertrenner(m::TJ_OPERATIONEN as u64),
            mit_tausendertrenner(m::TJ_DATEI_BYTES),
            mit_tausendertrenner((m::TJ_KLAMMERN * m::TJ_OPERATIONEN) as u64)
        ),
    );
    // Derselbe Fall mit der Decke, im Testprozess — der Satz muss den Ort
    // **und** das Profil nennen, weil `scan_page` im Extraktor liegt und die
    // Kommandozeile es nicht zeigt. Die Zahl der Zuordnungen kommt aus dem
    // Quelltext (`zuordnungen`), nicht aus der Doku.
    satz(
        "CHANGELOG.md",
        format!(
            "bleibt dieselbe Datei bei {zuordnungen} Zuordnungen und einer Warnung; \
             `scan_page` braucht dafür {} s und die Spitze liegt bei {} MB — \
             gemessen im Testprozess (Debug), weil `scan_page` im Extraktor liegt \
             und die Kommandozeile es nicht herausgibt",
            m::TJ_SCAN_S,
            kb_in_mb(m::TJ_SPITZE_KB)
        ),
    );
    // Und derselbe Fall am gebauten Binary — der Lauf, der der Runde 7
    // fehlte. Der Rückgabewert kommt aus `RC_LECK`, nicht aus der Doku.
    satz(
        "CHANGELOG.md",
        format!(
            "Am gebauten Binary (`target/release/redact-rs`, Release) kostet dieselbe \
             Struktur als Datei von {} Byte {} s und {} MB Spitze, und der Lauf endet \
             mit Rückgabewert {RC_LECK} und {} ungeprüften Stellen",
            mit_tausendertrenner(m::TJ_BINARY_BYTES),
            m::TJ_BINARY_S,
            kb_in_mb(m::TJ_BINARY_KB),
            zahlwort(m::TJ_BINARY_UNGEPRUEFT)
        ),
    );
    // Der zweite Weg: dieselbe Eingabe wie im Befund der Einleitung, jetzt
    // hinter der dokumentweiten Decke — und am Binary gemessen, nicht im
    // Testprozess. Die Vergleichszahl beschreibt den Stand davor und nennt
    // ihn deshalb.
    satz(
        "CHANGELOG.md",
        format!(
            "kosten die {} Seiten aus {} Byte am gebauten Binary {} s und {} MB statt \
             {} MB, bei genau {} zurückgestellten Abschnitten",
            m::DECKE_SEITEN,
            mit_tausendertrenner(m::DECKE_DATEI_BYTES),
            m::DECKE_BINARY_S,
            mit_tausendertrenner(kb_in_mb(m::DECKE_BINARY_KB)),
            mit_tausendertrenner(kb_in_mb(m::DECKE_SPITZE_KB)),
            mit_tausendertrenner(dokumentdecke() as u64)
        ),
    );
    // Die Regel für Zahlen des alten Zustands: der Satz nennt den Stand.
    satz(
        "CHANGELOG.md",
        format!(
            "Die {} MB dagegen beschreiben den ungedeckelten Stand `{}` und sind an \
             diesem Baum nicht mehr zu messen",
            mit_tausendertrenner(kb_in_mb(m::DECKE_SPITZE_KB)),
            m::UNGEDECKELT_STAND
        ),
    );
    // Dieselbe Regel für die drei Zahlen, die nur noch am Quelltext stehen.
    satz(
        "CHANGELOG.md",
        format!(
            "der Extraktor brauchte {} s, der Redaktor {} s, und {} MB gingen allein \
             auf die Eigenschaftslisten. Sie beschreiben den ungedeckelten Stand `{}`",
            m::UNGEDECKELT_EXTRAKTOR_S,
            m::UNGEDECKELT_REDAKTOR_S,
            m::UNGEDECKELT_LISTEN_MB,
            m::UNGEDECKELT_STAND
        ),
    );

    // Dieselbe Regel, zweiter Fall: die Spiegel-Bombe der Runde 6. Drei Sätze
    // des Blocks nennen denselben Stand — die Einleitung der Runde 6, der
    // Punkt zur getaggten Seite und das „vorher“ des Stromklons. Ein
    // gebundener Wortlaut deckt alle drei; wer den Stand austauscht, ändert
    // eine Zusage, und dieser Satz merkt es.
    satz(
        "CHANGELOG.md",
        format!(
            "am Stand `{}`, dem letzten Baum vor den Korrekturen der Runde 6",
            m::VOR_RUNDE6_STAND
        ),
    );

    // --- Die Nachbesserung selbst: was gehalten hat und was nicht ---------
    satz(
        "CHANGELOG.md",
        format!(
            "dass alle {} neuen Messzahlen aus einem Testprozess (Debug) stammten",
            zahlwort(m::NEUE_MESSZAHLEN_7)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "{} Zahlen bleiben im Testprozess, und der Satz sagt es jetzt selbst samt \
             Profil",
            zahlwort_gross(m::TESTPROZESS_ZAHLEN_7)
        ),
    );
    // Die gestrichene Zahl und die beiden Läufe, die sie widerlegten.
    satz(
        "CHANGELOG.md",
        format!(
            "standen {} s und {} MB, zwei neue Läufe desselben Befehls im \
             Testprozess (Debug) gaben aber {} s und {} kB sowie {} s und {} kB",
            m::WIDERLEGT_DECKE_S,
            m::WIDERLEGT_DECKE_MB,
            m::NEU_DECKE_S1,
            mit_tausendertrenner(m::NEU_DECKE_KB1),
            m::NEU_DECKE_S2,
            mit_tausendertrenner(m::NEU_DECKE_KB2)
        ),
    );

    // --- Und die Statuszeile, gemessen und gedeckelt ----------------------
    //
    // Auch hier nennt der Satz den Ort und das Profil: die Statuszeile gehört
    // der Oberfläche, die keine Kommandozeile hat.
    satz(
        "CHANGELOG.md",
        format!(
            "war mit den echten Stellen eines Laufs {} Zeichen lang — gemessen im \
             Testprozess (Debug), weil die Statuszeile der Oberfläche gehört und am \
             gebauten `redact-rs` nicht entsteht",
            mit_tausendertrenner(m::STATUSZEILE_ZEICHEN as u64)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!("auf höchstens {} Zeichen", statuszeilendecke()),
    );

    // --- Nach der Runde 9: die Messung der Schreibweise (Register #60) -----
    //
    // Wie viele Einträge `gemessene_schreibweise` höchstens ansieht — eine
    // Decke im Code, nicht gemessen, aber eine Zusage, und deshalb gebunden.
    satz(
        "CHANGELOG.md",
        format!(
            "geprüft werden höchstens {} Einträge",
            zahlwort(redact_gui::app::PROBEN_HOECHSTENS)
        ),
    );

    // --- Nach der Runde 9: die Debug-Information (Register #59) ------------
    //
    // Der Vorher-Satz nennt „vorher“ (Weg 3: nach der Umstellung nicht mehr
    // messbar), der Nachher-Satz „gebauten Binaries“ (Weg 1). Dieselben Zahlen
    // stehen in CONTRIBUTING.md, an dieselben Konstanten gebunden.
    satz(
        "CHANGELOG.md",
        format!(
            "`target/` belegte vorher {} GB, und ein GUI-Testbinary \
             (`zm_b_gleichzeitige_exporte`) war vorher {} MB groß, davon {} MB in \
             `.debug_*`-Sektionen — {} %",
            m::DEBUG_TARGET_VORHER_GB,
            m::DEBUG_BINARY_VORHER_MB,
            m::DEBUG_SEKTIONEN_VORHER_MB,
            m::DEBUG_ANTEIL_VORHER_PROZENT
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Dasselbe Binary ist an den gebauten Binaries danach {} MB groß, davon \
             {} MB `.debug_*`, und `target/` belegt nach `cargo clean` und dem ganzen \
             Gate {} GB",
            m::DEBUG_BINARY_NACHHER_MB,
            m::DEBUG_SEKTIONEN_NACHHER_MB,
            m::DEBUG_TARGET_NACHHER_GB
        ),
    );
    satz(
        "CONTRIBUTING.md",
        format!(
            "an einem GUI-Testbinary vorher {} MB von {} MB",
            m::DEBUG_SEKTIONEN_VORHER_MB,
            m::DEBUG_BINARY_VORHER_MB
        ),
    );
    satz(
        "CONTRIBUTING.md",
        format!(
            "sind es an demselben gebauten Binary {} MB",
            m::DEBUG_BINARY_NACHHER_MB
        ),
    );

    // --- Die Verweiskette (Spur-A-Runde 1, Register #72) -------------------
    satz(
        "CHANGELOG.md",
        format!(
            "Am Stand `{}` brauchte `strip_metadata` an einer Kette aus {} Gliedern \
             {} s, an {} Gliedern {} s im Testprozess (Debug)",
            m::VERWEISKETTE_STAND,
            mit_tausendertrenner(m::VERWEISKETTE_GLIEDER_VORHER),
            m::VERWEISKETTE_VORHER_8000_S,
            mit_tausendertrenner(kettenglieder() as u64),
            m::VERWEISKETTE_VORHER_20000_S
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "die Datei dazu ist {} kB groß",
            mit_tausendertrenner(m::VERWEISKETTE_DATEI_KB)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Dieselben {} Glieder laufen im Testprozess (Debug) jetzt in {} s; die \
             Decke des Belegs liegt bei {} s",
            mit_tausendertrenner(kettenglieder() as u64),
            m::VERWEISKETTE_NACHHER_S,
            kettendecke_s()
        ),
    );

    // --- Die gehaltenen Formularspiegel (Spur-A-Runde 1, Register #68) ------
    satz(
        "CHANGELOG.md",
        format!(
            "Am Stand `{}` brauchte der Redaktor an je Seite einem eigenen Formular mit \
             {} × {} Paaren im Testprozess (Debug, eigener Prozess) für {} Seiten {} MB \
             und für {} Seiten {} MB bei {} s — aus rund {} kB Datei je Seite",
            m::FORMULARSPIEGEL_STAND,
            m::FORMULARSPIEGEL_KLAMMERN,
            m::FORMULARSPIEGEL_PLATZIERUNGEN,
            m::FORMULARSPIEGEL_SEITEN_KLEIN,
            m::FORMULARSPIEGEL_VORHER_KLEIN_MB,
            formularspiegel_seiten(),
            m::FORMULARSPIEGEL_VORHER_GROSS_MB,
            m::FORMULARSPIEGEL_VORHER_GROSS_S,
            m::FORMULARSPIEGEL_DATEI_JE_SEITE_KB
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "deckelt sie bei {} Einträgen",
            mit_tausendertrenner(gehaltene_formularspiegel() as u64)
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "Dieselben {} Seiten brauchen im Testprozess (Debug, eigener Prozess) jetzt \
             {} MB, {} Seiten {} MB",
            m::FORMULARSPIEGEL_SEITEN_KLEIN,
            m::FORMULARSPIEGEL_NACHHER_KLEIN_MB,
            formularspiegel_seiten(),
            m::FORMULARSPIEGEL_NACHHER_GROSS_MB
        ),
    );
    satz(
        "CHANGELOG.md",
        format!(
            "(im Testprozess, Debug, als Kindprozess: {} Seiten unter {} MB)",
            formularspiegel_seiten(),
            formularspiegel_decke_mb()
        ),
    );

    // --- Die Kettenbombe auf dem Schreibpfad (Spur-A-Runde 1, Register #64) --
    satz(
        "CHANGELOG.md",
        format!(
            "Am Stand `{}` stand `/Filter [/FlateDecode /RunLengthDecode]` über einem \
             RunLength-Strom, der sich auf {} GB aufbläst, als {} KB in der Datei; am \
             gebauten Binary brauchte der Lauf {} GB Spitze (`/usr/bin/time -v`), und \
             unter einer Adressraumgrenze von {} GiB starb er mit Signal",
            m::KETTENBOMBE_STAND,
            m::KETTENBOMBE_ENTPACKT_GB,
            m::KETTENBOMBE_DATEI_KB,
            m::KETTENBOMBE_SPITZE_GB,
            m::KETTENBOMBE_ADRESSRAUM_GIB
        ),
    );

    // --- Die Bilddecke gegen den JPEG-Kopf (Spur-A-Runde 2, Register #101) ---
    satz(
        "CHANGELOG.md",
        format!(
            "Am Stand `{}` belegte ein Bild mit {} im Dictionary über einem JPEG mit {} \
             am gebauten Binary {} MB Spitze (`/usr/bin/time -v`), bei `--max-image-mb {}`, \
             mit Rückgabewert 0",
            m::JPEGKOPF_STAND,
            m::JPEGKOPF_DICT,
            m::JPEGKOPF_JPEG,
            m::JPEGKOPF_SPITZE_MB,
            m::JPEGKOPF_DECKE_MB
        ),
    );

    // --- Die Arbeit der Kette (Spur-A-Runde 2, Register #99) ----------------
    satz(
        "CHANGELOG.md",
        format!(
            "Am Stand `{}` brauchte eine Datei von {} KB mit {} solcher Ströme zu je {} MiB \
             Nullen am gebauten Binary {} (`/usr/bin/time -v`), bei \
             `--max-decompressed-mb {}`, und endete mit Rückgabewert 0; `--check-leaks` \
             lief nach {} s noch",
            m::ARBEIT_STAND,
            m::ARBEIT_DATEI_KB,
            m::ARBEIT_STROEME,
            m::ARBEIT_NULLEN_MIB,
            m::ARBEIT_DAUER,
            m::ARBEIT_BUDGET_MB,
            m::ARBEIT_ORAKEL_S
        ),
    );

    // --- Die Vorprüfung als Schranke (Spur-A-Runde 2, Register #89) ---------
    satz(
        "CHANGELOG.md",
        format!(
            "Am Stand `{}` brauchte eine Datei von {} MB mit {} GiB Nullen hinter \
             `/Filter 6 0 R` am gebauten Binary {} GB Spitze (`/usr/bin/time -v`), bei \
             `--max-decompressed-mb {}`, und endete mit Rückgabewert 1",
            m::SCHRANKE_STAND,
            m::SCHRANKE_DATEI_MB,
            m::SCHRANKE_NULLEN_GIB,
            m::SCHRANKE_SPITZE_GB,
            m::SCHRANKE_BUDGET_MB
        ),
    );

    // --- Der Vorspann vor einem Bildfilter (Spur-A-Runde 1, Register #83) ----
    satz(
        "CHANGELOG.md",
        format!(
            "Am Stand `{}` brauchte eine Datei von {} MB mit {} GiB Nullen im Flate-Glied \
             am gebauten Binary {} GB Spitze (`/usr/bin/time -v`), bei \
             `--max-decompressed-mb {}`, und endete mit Rückgabewert 1",
            m::VORSPANN_STAND,
            m::VORSPANN_DATEI_MB,
            m::VORSPANN_NULLEN_GIB,
            m::VORSPANN_SPITZE_GB,
            m::VORSPANN_BUDGET_MB
        ),
    );

    aus
}

/// Seiten und Decke der Speicherprobe zu Register #68, aus
/// `zo_c_spiegel_umgebungen.rs`; die Decke des Kontos aus `redact.rs`.
fn formularspiegel_seiten() -> usize {
    konstante_aus(
        "crates/redact-pdf/tests/zo_c_spiegel_umgebungen.rs",
        "C4_SEITEN",
    )
}

fn formularspiegel_decke_mb() -> usize {
    konstante_aus(
        "crates/redact-pdf/tests/zo_c_spiegel_umgebungen.rs",
        "C4_DECKE_MB",
    )
}

fn gehaltene_formularspiegel() -> usize {
    konstante_aus("crates/redact-pdf/src/redact.rs", "MAX_HELD_FORM_MIRRORS")
}

/// Glieder und Decke des Verweisketten-Belegs, aus `zo_b_traeger.rs`.
fn kettenglieder() -> usize {
    konstante_aus("crates/redact-pdf/tests/zo_b_traeger.rs", "KETTENGLIEDER")
}

fn kettendecke_s() -> usize {
    konstante_aus("crates/redact-pdf/tests/zo_b_traeger.rs", "KETTEN_DECKE_S")
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

/// Die dokumentweite Decke der zurückgestellten Textspiegel — aus dem
/// Quelltext des Redaktors, nicht abgeschrieben.
fn dokumentdecke() -> usize {
    konstante_aus("crates/redact-pdf/src/redact.rs", "MAX_DEFERRED_MIRRORS")
}

/// Wie viele Zeichen die Statuszeile der Oberfläche höchstens trägt — aus dem
/// Quelltext der Oberfläche.
fn statuszeilendecke() -> usize {
    konstante_aus("crates/redact-gui/src/state.rs", "MAX_STATUS_CHARS")
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
    // --- Fix-Runde 7: Anzahlen von Befunden, Stellen und Vorfaellen -------
    // Keine davon misst das Programm; sie zaehlen, wovon der Abschnitt
    // handelt. Wer sie aendert, aendert keine Zusage ueber das Verhalten.
    // --- Der Vorspann der Release-Notizen ---------------------------------
    // Er liegt seit zi_e_lage_der_messzahlen IM geprueften Block. Was dort an
    // Zahlen steht, zaehlt oder benennt - gemessen wird nichts davon.
    (
        "Bereich: `git log v0.6.0..v0.7.0`.",
        "die Spanne der Release-Notizen; ein Git-Bereich, keine gemessene Groesse",
    ),
    (
        "## 0.7.0 — 2026-09-25",
        "Fassung und Datum des Release-Schnitts, keine Messung",
    ),
    (
        "Neun Fix-Runden seit 0.6.0",
        "zaehlt die Abschnitte dieser Datei und nennt den Vorgaenger-Tag, keine Messung",
    ),
    (
        "Zuletzt (Runde 9) fällt ein stilles Leck im Bild, eine Grenze, die eine \
         gewöhnliche Datei ablehnte, und eine Rückfrage, die beim falschen der beiden Fäden \
         stand",
        "zaehlt die Befunde der Runde und nennt ihre Nummer, keine gemessene Groesse",
    ),
    (
        "alle drei neuen Messzahlen der Runde 7",
        "zaehlt die Messzahlen und nennt die Runde - die Zahlen selbst stehen weiter unten",
    ),
    (
        "MB heißt 1024 Byte zum Quadrat",
        "die Definition der Einheit, in der gemessen wird; selbst keine Messung",
    ),
    // --- Fix-Runde 9 -----------------------------------------------------
    (
        "Fix-Runde 9: was die Gegenprüfung der Runde 8 noch fand",
        "Überschrift; beide Nummern benennen Abschnitte dieser Datei",
    ),
    (
        "Vier Gegenprüfer lasen die Korrekturen der Runde 8 mit eigenem Material gegen.",
        "eine Angabe über den Ablauf der Runde, nicht über das Programm",
    ),
    (
        "Was die Runde 8 getragen hat",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "Die Runde 8 hat den *Spiegel* daran ausgerichtet",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "auf einer Seite unter zwei Namen unter der Schwärzung",
        "zaehlt die Namen des Materials, an dem der Befund haengt, keine Messung",
    ),
    (
        "es waren drei Flächenfragen, nicht eine.",
        "zaehlt die Stellen im Quelltext, die dieselbe Frage stellten, keine Messung",
    ),
    (
        "Jetzt fragen alle drei dasselbe Viereck",
        "dieselben drei Stellen im Quelltext, keine gemessene Groesse",
    ),
    (
        "Derselbe Klick ergab dann zwei Kennungen",
        "zaehlt die Kennungen eines Exports — eine Aussage ueber den Code, keine Messung",
    ),
    (
        "standen aber die Zusagen der Runde 8",
        "Verweis auf den Abschnitt darunter, wo jede dieser Zusagen einzeln steht",
    ),
    (
        "**erste** Anweisung",
        "zitiert die Zusage des Abschnitts darunter, wo sie eigens begruendet ist",
    ),
    (
        "**einen** Namen",
        "zitiert die Zusage des Abschnitts darunter, wo sie eigens begruendet ist",
    ),
    (
        "**eine** Stelle",
        "zitiert die Zusage des Abschnitts darunter, wo sie eigens begruendet ist",
    ),
    (
        "**Zwei Helfer der Doku-Wächter waren zu grob.**",
        "zaehlt die Befunde dieses Punktes, keine gemessene Groesse am Programm",
    ),
    (
        "Rückgabewert 3. Dieselbe Datei",
        "zitiert einen Beispielsatz, an dem die Satztrennung vorgefuehrt wird",
    ),
    (
        "weil vor dem Punkt eine Ziffer stand; zwei Sätze verschmolzen",
        "zaehlt die Teile dieses Beispielsatzes, keine gemessene Groesse",
    ),
    (
        "**Eine Lockerung aus der Runde 8, zurückgenommen.**",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "hatte sechs neue Merkmale bekommen",
        "zaehlt Eintraege in einer Liste des Testbestands, keine Messung am Programm",
    ),
    (
        "sagte an zwei Stellen weiter, die Prüfung hänge",
        "zaehlt Stellen im Quelltext, keine gemessene Groesse",
    ),
    (
        "der Abschnitt der Fix-Runde 3 sagte im Präsens",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "Der Schritt ist in der Runde 8 getan.",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "**Drei Belegdateien der Prüfer hielten Kopien der Helfer, die sie prüfen.**",
        "zaehlt Dateien des Testbestands, keine gemessene Groesse am Programm",
    ),
    (
        "dieses Projekt seit der Runde 3 arbeitet",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "die Ablehnung eines **zweiten** Exports derselben Datei",
        "eine Aussage ueber das Verhalten der Oberflaeche, keine gemessene Groesse; geprueft von zh2_c_ein_klick_mitten_im_bild_sieht_den_fertigen_export",
    ),
    (
        "Die drei Punkte darunter sind die **Fehler** dieses Umbaus",
        "zaehlt die Listenpunkte darunter, keine gemessene Groesse",
    ),
    (
        "als **erste** Anweisung von `export_to`",
        "eine Aussage ueber die Reihenfolge zweier Anweisungen, keine gemessene Groesse; geprueft von zh2_c_ein_klick_mitten_im_bild_sieht_den_fertigen_export",
    ),
    (
        "Jetzt entscheidet **eine** Flächenfrage",
        "eine Aussage ueber den Bau des Bildlaufs, keine gemessene Groesse; geprueft von zm_d_bildwahrheit_gegengelesen und zk_b_bildpunkt_ist_die_wahrheit",
    ),
    (
        "`repoint_page` setzt genau **einen** Namen",
        "eine Aussage ueber das Verhalten einer Funktion, keine gemessene Groesse; geprueft von zm_a_zwei_namen_ein_bild",
    ),
    (
        "Jetzt beantwortet **eine** Stelle die Frage",
        "eine Aussage ueber den Bau des Bildlaufs, keine gemessene Groesse; geprueft von zl_b_pendel_beide_richtungen und zm_a_zwei_namen_ein_bild",
    ),
    // --- Fix-Runde 8 -----------------------------------------------------
    (
        "Fix-Runde 8: was die Gegenprüfung der Runde 7 noch fand",
        "Überschrift; beide Nummern benennen Abschnitte dieser Datei",
    ),
    (
        "Fünf Gegenprüfer lasen die Korrekturen der Runde 7 mit eigenem Material gegen.",
        "eine Angabe über den Ablauf der Runde, nicht über das Programm",
    ),
    (
        "bis er dreimal gedreht war, zwei Wächter über der Doku",
        "zaehlt die Anlaeufe einer Korrektur und die Waechter, keine gemessene Groesse",
    ),
    (
        "steht seit der Runde 4 in einem eigenen Faden",
        "Verweis auf einen Abschnitt dieser Datei, keine Messung",
    ),
    (
        "und zwei Schreibwege auf dieselbe Datei fallen zusammen",
        "zaehlt die Wege zu einer Datei — eine Aussage ueber die Kennung, keine Messung",
    ),
    (
        "Die Korrektur hat drei Anläufe gebraucht",
        "zaehlt die Anlaeufe dieser Korrektur, keine gemessene Groesse am Programm",
    ),
    (
        "**Sechs Teststellen behaupteten den Ausgang eines Programms",
        "zaehlt Stellen im Testbestand dieses Repositoriums, keine Messung am Programm",
    ),
    (
        "Jetzt übergeht jede der sechs Stellen den Fall",
        "dieselben sechs Stellen im Testbestand, keine gemessene Groesse",
    ),
    (
        "`cargo clippy --target x86_64-pc-windows-gnu` an derselben Zeile mit `E0433` \
         abbrach",
        "der Name eines Zielsystems und die Kennung eines Compilerfehlers, keine Messung",
    ),
    (
        "**Zwei Wächter über der Doku hielten ihre eigene Regel nicht.**",
        "zaehlt die beiden Tests dieses Befundes, keine gemessene Groesse",
    ),
    (
        "seine Wortliste endete bei „zwölf“ und hatte „siebzehn“ von Hand nachgetragen",
        "zitiert die Woerter einer Wortliste; Namen von Zahlwoertern, keine Messung",
    ),
    (
        "„dreizehn“, „zwanzig“, „hundert“, „tausend“ und „Dutzend“ waren für ihn keine \
         Zahlen",
        "zitiert Zahlwoerter als Woerter — was der Waechter sah, nicht was gemessen wurde",
    ),
    (
        "an einer Datei mit einem 64-MiB-Strom durchläuft",
        "die Stromgroesse ist im Satz ueber die Kostenrechnung gebunden (KLON_STROM_MIB)",
    ),
    // --- Nach der Runde 9: der Windows-Befund (Register #60) ---------------
    // Anzahlen im Szenario, Nummern von Runden und Commits, der Name einer
    // Windows-Fassung. Die eine Decke des Abschnitts (hoechstens acht
    // Eintraege) ist in `messsaetze` an `PROBEN_HOECHSTENS` gebunden.
    (
        "### Nach der Runde 9: ein Prüfer, der Windows heißt",
        "die Nummer einer Fix-Runde in einer Ueberschrift, keine Messung",
    ),
    (
        "Zwei Befunde außerhalb einer Gegenprüfung",
        "zaehlt die Befunde dieses Abschnitts, keine gemessene Groesse",
    ),
    (
        "oder ext4 mit `casefold` falten unter Linux",
        "der Name eines Dateisystems, keine Messung",
    ),
    (
        "gab der Kollisionsschutz der Oberfläche einer Datei zwei Kennungen",
        "zaehlt die Kennungen fuer eine Datei — der Befund selbst, keine Messung",
    ),
    (
        "aus der Runde 9: der Test schreibt",
        "die Nummer einer Fix-Runde, keine Messung",
    ),
    (
        "Zwei gleichzeitige Exporte auf diese Datei hätten sich nicht",
        "zaehlt die Exporte des Szenarios, keine Messung",
    ),
    (
        "der Stand `0f0b0f7` kanonisierte genauso nur das Verzeichnis",
        "eine Commit-Kennung, an der der alte Stand liegt — keine Messung",
    ),
    (
        "Der Satz der Fix-Runde 8 unten, zwei Schreibwege auf dieselbe Datei fielen \
         zusammen",
        "Nummer einer Runde und Zaehlung der Wege zu einer Datei, keine Messung",
    ),
    (
        "eine gemeinsame Kennung für zwei *verschiedene* Dateien",
        "zaehlt die Dateien des Szenarios, keine Messung",
    ),
    (
        "NTFS lässt seit Windows 10 je Verzeichnis",
        "der Name einer Windows-Fassung, keine Messung",
    ),
    (
        "bis zum Befund des Windows-Jobs nach der Runde 9 zwei Kennungen",
        "Nummer einer Runde und Zaehlung der Kennungen, keine Messung",
    ),
    // --- Nach der Runde 9: das Platzproblem (Register #59) -----------------
    (
        "brach darin zweimal mit „No space left on device“ ab",
        "zaehlt die Abbrueche eines Werkzeuglaufs, keine Messung am Programm",
    ),
    (
        "der Commit `1282b16` nannte den Knopf als Hebel",
        "eine Commit-Kennung, keine Messung",
    ),
    // --- Nach der Runde 9: die Arbeitsweise (drei Spuren) ------------------
    (
        "Die Fix-Runden hatten drei Aufgaben in einer Runde",
        "zaehlt die Aufgaben der Schleife, keine Messung am Programm",
    ),
    (
        "die Runde 9 hatte alle drei in einem Commit. Jetzt sind es drei Spuren",
        "Nummer einer Runde und Zaehlung von Aufgaben und Spuren, keine Messung",
    ),
    (
        "„Drei Spuren, eine Probenliste, ein Ende“",
        "der Titel eines CONTRIBUTING-Abschnitts, keine Messung",
    ),
    // --- Spur-A-Runde 1: Anzahlen der Runde, keine Messungen -------------
    (
        "### Spur-A-Runde 1: die Probenliste hält",
        "die Nummer einer Runde in einer Ueberschrift, keine Messung",
    ),
    (
        "Fünf Prüfer, nur Schwärzung, je ein Gebiet",
        "zaehlt die Pruefer der Runde, keine Messung",
    ),
    (
        "Alle acht Zeilen der Spur A in",
        "zaehlt die Zeilen der Probenliste, keine Messung",
    ),
    (
        "**Die zweite Frage: ja, und zwar 19 Mal** (Register #64 bis #82)",
        "zaehlt Registereintraege und nennt ihre Nummern, keine Messung",
    ),
    (
        "13 stille Lecks am Schwärzungscode, drei stille Lecks am Orakel selbst, drei \
         Dienstverweigerungen und eine abgelehnte gewöhnliche Datei",
        "zaehlt Befunde nach Klasse, keine Messung",
    ),
    (
        "ungeschwärzt in die Ausgabe** (Register #69, Prüfer B)",
        "eine Registernummer, keine Messung",
    ),
    (
        "Am gebauten Binary: Rückgabewert 0, `--check-leaks` an der Ausgabe \
         Rückgabewert 3 mit dem Rohstrom der alten Seite",
        "Rueckgabewerte des Binaries — Zusagen, in zo_b und cli-Tests gebunden, keine Messgroesse",
    ),
    (
        "Mutationsnachweis: `empty_orphan_pages` gibt null zurück",
        "der Rueckgabewert einer mutierten Funktion, keine Messung",
    ),
    (
        "mit den Bildpunkten von vorher** (Register #74, Prüfer A)",
        "eine Registernummer, keine Messung",
    ),
    (
        "ein Raster der Seite (Tabelle 30)",
        "die Nummer einer Tabelle der PDF-Norm, keine Messung",
    ),
    (
        "**⚠ Sicherheit: 13 Schlüssel an Katalog, Seiten und Objekten trugen",
        "zaehlt die Schluessel des Befundes, keine Messung",
    ),
    (
        "Klartext durch den Metadatenlauf** (Register #70, Prüfer B)",
        "eine Registernummer, keine Messung",
    ),
    (
        "Alle mit Rückgabewert 0 und leerem Bericht",
        "der Rueckgabewert des Binaries, in zo_b gebunden, keine Messgroesse",
    ),
    (
        "Belege: dreizehn `b_*`-Tests in",
        "zaehlt Tests, keine Messung",
    ),
    (
        "Katalogschleife entfernt → sechs davon rot",
        "zaehlt rote Tests unter Mutation, keine Messung",
    ),
    (
        "und die Bits sind die Form** (Register #77, Prüfer A)",
        "eine Registernummer, keine Messung",
    ),
    (
        "Mutationsnachweis: die Bedingung `filled == 0`",
        "eine Bedingung im Code, keine Messung",
    ),
    (
        "ein Formular unter zwei Umgebungen, beide mit Spiegel",
        "zaehlt die Umgebungen des Szenarios, keine Messung",
    ),
    (
        "nur der erste Fundort wurde geleert** (Register #65, Prüfer C)",
        "eine Registernummer, keine Messung",
    ),
    (
        "(oder zwei Seiten mit je eigenem `/Properties`)",
        "zaehlt Seiten des Szenarios, keine Messung",
    ),
    (
        "sind das zwei Listen an zwei Fundorten",
        "zaehlt Listen und Fundorte, keine Messung",
    ),
    (
        "und `…_auf_zwei_seiten` (sieben Ausprägungen",
        "zaehlt Testauspraegungen, keine Messung",
    ),
    (
        "löst `/MC0` unter jeder Umgebung neu auf",
        "der Name einer Eigenschaftsliste (Ressourcenname), keine Messung",
    ),
    (
        "hielt je Strom und Operation nur den **ersten** Datensatz",
        "beschreibt den alten Zustand; gebunden durch den Mutationsnachweis des Befundes, keine Messung",
    ),
    (
        "ohne Warnung, mit Rückgabewert 0; `--check-leaks` an der Ausgabe fand ihn",
        "der Rueckgabewert des Laufs, in zo_c gebunden, keine Messgroesse",
    ),
    (
        "**Eine reelle Breite umging die Bilddecke** (Register #79, Prüfer A)",
        "eine Registernummer, keine Messung",
    ),
    (
        "`/Width 100.0` ist nach PDF 32000-1 eine Breite",
        "ein Beispielwert und die Nummer der Norm, keine Messung",
    ),
    (
        "las eine reelle Zahl aber als null und ließ ein Bild",
        "der gelesene Wert im alten Code, keine Messung",
    ),
    (
        "Mutationsnachweis: reelle Zahl wieder als null gelesen",
        "der gelesene Wert unter Mutation, keine Messung",
    ),
    (
        "an** (Register #72, Prüfer B)",
        "eine Registernummer, keine Messung",
    ),
    (
        "sind (`4 0 obj 5 0 R`)",
        "ein Beispiel aus der Norm, keine Messung",
    ),
    (
        "bis zum Ende, Aufwand n²/2",
        "der Aufwand als Formel in der Objektzahl, keine Messung",
    ),
    (
        "den niemand zeichnet** (Register #76, Prüfer A)",
        "eine Registernummer, keine Messung",
    ),
    (
        "ungeschwärzt und ungemeldet** (Register #75, Prüfer A)",
        "eine Registernummer, keine Messung",
    ),
    (
        "Der Bericht zählte null geschwärzte Bilder",
        "der Zaehlerstand des Berichts im alten Zustand; gebunden durch den Mutationsnachweis, keine Messung",
    ),
    (
        "blieb stehen** (Register #66, Prüfer C; kein stilles Leck — `--check-leaks` fand ihn, Rückgabewert 3)",
        "Registernummer und der Rueckgabewert des Orakels (in zo_c gebunden), keine Messung",
    ),
    (
        "`zo_c_spiegel_umgebungen::spiegel_ueber_kachelmuster_mit_text` (drei Ausprägungen)",
        "zaehlt Testauspraegungen, keine Messung",
    ),
    (
        "BDC … /P0 scn … re f … EMC",
        "der Name eines Musters im Beispiel, keine Messung",
    ),
    (
        "eine Antwort am Leben hält** (Register #71, Prüfer B)",
        "eine Registernummer, keine Messung",
    ),
    (
        "**⚠ Sicherheit: zwei Erscheinungsströme, die niemand las",
        "zaehlt die Traeger des Befunds, keine Messung",
    ),
    (
        "(`/MK /I`, `/RI`, `/IX`, Tabelle 189)",
        "die Nummer einer Tabelle der Norm, keine Messung",
    ),
    (
        "`zo_b_traeger::b_mk_icon_mit_text_faellt` (drei Schlüssel)",
        "zaehlt die geprueften Schluessel, keine Messung",
    ),
    (
        "gehalten, ohne Decke** (Register #68, Prüfer C)",
        "eine Registernummer, keine Messung",
    ),
    (
        "`zo_c_spiegel_umgebungen::c4_spiegel_in_formularen_kosten_je_seite_wenig`",
        "ein Testname (Befund C-4), keine Messung",
    ),
    (
        "`…::c4_decke_der_gehaltenen_formularspiegel_wird_gesagt`",
        "ein Testname (Befund C-4), keine Messung",
    ),
    (
        "`zd_orakel_budget`, `ze_p1_budget_und_filter`, `zg_r2_decke`, `zf_q2_teildekoder` und `ze_p4_check_leaks_grenzen` sind darauf umgestellt",
        "Dateinamen (Gegenprüfungen P1, P4, Q2, R2), keine Messung",
    ),
    (
        "Ausgabedatei** (Register #78, Prüfer A; abgelehnte gewöhnliche Datei)",
        "eine Registernummer, keine Messung",
    ),
    (
        "`LZWDecode` ist ein Filter aus PDF 1.0",
        "die Fassung der Norm, keine Messung",
    ),
    (
        "den ältere Distiller, `tiff2pdf` und Ghostscript",
        "der Name eines Werkzeugs, keine Messung",
    ),
    (
        "LZW wieder nicht unterstützt → alle drei rot",
        "zaehlt rote Tests unter Mutation, keine Messung",
    ),
    (
        "nicht auspackte** (Register #64, Prüfer E;",
        "eine Registernummer, keine Messung",
    ),
    (
        "und die Bombe fällt dort mit Rückgabewert 1 und",
        "der Rueckgabewert des Binaries, in zo_e gebunden, keine Messgroesse",
    ),
    (
        "gilt weiter nur `LZWDecode` und `ASCII85Decode`.",
        "Filternamen der Norm, keine Messung",
    ),
    (
        "gefunden, ohne Meldung** (Register #81, Prüfer D). PDF 32000-1, Anhang D.2 weicht im Block 0x80–0xA0 und bei den Akzenten 0x18–0x1F von Latin-1 ab",
        "Registernummer, Norm, Anhang und Byteblöcke der Kodierung, keine Messung",
    ),
    (
        "`ﬁ`, `ﬂ` und `€` (0xA0 — nicht das geschützte Leerzeichen)",
        "ein Byte der Kodierung, keine Messung",
    ),
    (
        "Latin-1; `--check-leaks \"Betrag 5 €\"` an einer Datei, die `(Betrag 5 \\240)` trägt, sagte „nicht gefunden“ mit Rückgabewert 0",
        "der Suchbegriff des Belegs, seine oktale Schreibweise und der Rueckgabewert (in zo_d gebunden), keine Messung",
    ),
    (
        "das Orakel las PDFDocEncoding als Latin-1 —",
        "der Name einer Kodierung, keine Messung",
    ),
    (
        "die Abweichungen nach Anhang D.2; dieselbe Funktion",
        "ein Anhang der Norm, keine Messung",
    ),
    (
        "`zo_d_altgeneration_und_kodierung::zo_d4_pdfdoc_zeichen_stilles_leck`",
        "ein Testname (Befund D4), keine Messung",
    ),
    (
        "`zo_d_orakel_am_binary::zo_d12_pdfdoc_in_der_ausgabedatei_stilles_leck`",
        "ein Testname (Befund D4 am Binary), keine Messung",
    ),
    (
        "Mutationsnachweis: jedes Byte wieder Latin-1 → beide rot.",
        "der Name einer Kodierung unter Mutation, keine Messung",
    ),
    (
        "mit reinem Flate geschrieben war** (Register #80, Prüfer D)",
        "eine Registernummer, keine Messung",
    ),
    (
        "`/LZWDecode`, `/ASCII85Decode`, ASCIIHex mit Zeilenumbrüchen, Flate mit PNG-Prädiktor oder `[/ASCII85Decode /FlateDecode]` (Distiller)",
        "Filternamen der Norm, keine Messung",
    ),
    (
        "als UTF-16BE so maskiert wie pdfTeX es schreibt",
        "der Name einer Kodierung, keine Messung",
    ),
    (
        "`zo_d_altgeneration_und_kodierung::zo_d2_altgeneration_stroeme_stilles_leck`",
        "ein Testname (Befund D2), keine Messung",
    ),
    (
        "`…::zo_d3_altgeneration_zeichenketten_stilles_leck`",
        "ein Testname (Befund D3), keine Messung",
    ),
    (
        "`zo_d_orakel_am_binary::zo_d11_altgeneration_am_binary_stilles_leck`",
        "ein Testname (Befund D2/D3 am Binary), keine Messung",
    ),
    (
        "`…::zo_d0_probenliste_filterkette_bleibt_gruen`",
        "ein Testname (Probenliste), keine Messung",
    ),
    (
        "stehen bleibt wie die Objektsicht. Mutationsnachweis: Kette nicht gelesen → der erste und der dritte rot; Literale nicht gelesen → der zweite und der dritte rot.",
        "zaehlt rote Tests unter Mutation, keine Messung",
    ),
    (
        "die Rohsicht an `[null /ASCII85Decode]` stehen bleibt",
        "ein Filtername der Norm im Beispiel, keine Messung",
    ),
    (
        "an einem Bildfilter endet** (Register #83, Nachtrag zu #64; Dienstverweigerung)",
        "Registernummern, keine Messung",
    ),
    (
        "Die Vorprüfung packte seit #64 jede Kette aus",
        "eine Registernummer, keine Messung",
    ),
    (
        "`[/ASCII85Decode /DCTDecode]` lief vorher roh durch",
        "Filternamen der Norm, keine Messung",
    ),
    (
        "Die Zeile der Probenliste zu #64 trat damit noch auf",
        "eine Registernummer, keine Messung",
    ),
    (
        "`zf_q2_teildekoder` hält seither die Ablehnung fest",
        "ein Dateiname (Gegenprüfung Q2), keine Messung",
    ),
    (
        "ein großes ASCII85-Bild fiel an einer Grenze, die nichts mehr schützte** (Register #82, Prüfer D)",
        "ein Filtername und eine Registernummer, keine Messung",
    ),
    (
        "jede Kette mit `LZWDecode` oder `ASCII85Decode` über einer festen Rohgröße ab",
        "Filternamen der Norm, keine Messung",
    ),
    (
        "Seit #64 entpackt sie die Vorprüfung selbst",
        "eine Registernummer, keine Messung",
    ),
    (
        "etwa ein Bild unter `/ASCII85Decode`, wie es Distiller",
        "ein Filtername der Norm, keine Messung",
    ),
    (
        "Ein LZW- oder ASCII85-Strom, der mehr entpackt",
        "ein Filtername der Norm, keine Messung",
    ),
    (
        "der Test zum Befund D5 (bis dahin absichtlich rot und ignoriert)",
        "eine Befundkennung der Runde, keine Messung",
    ),
    (
        "ohne `/Properties` mitbringt** (Register #67, Prüfer C; mit Vorbehalt",
        "eine Registernummer, keine Messung",
    ),
    (
        "`/Span /MC0 BDC` in einem Form-XObject mit eigenem `/Resources` löst sich nach PDF 32000-1 nur dort auf",
        "ein Ressourcenname und die Norm, keine Messung",
    ),
    (
        "blieb mit dem Geheimnis stehen — ohne Warnung, mit Rückgabewert 0; `--check-leaks` an der Ausgabe fand sie",
        "ein Rückgabewert, keine Messung",
    ),
    (
        "Die zweite Runde unter demselben Mandat, auf dem Stand `2edf407`",
        "ein Commit-Stand, keine Messung",
    ),
    (
        "JPEG-Dekoder belegte die Maße aus dem JPEG** (Register #101, Prüfer A;",
        "eine Registernummer, keine Messung",
    ),
    (
        "### Spur-A-Runde 2: die Varianten der eigenen Korrekturen",
        "eine Rundennummer in der Überschrift, keine Messung",
    ),
    (
        "hielt die Suche nach dem Spiegel an** (Register #86, Prüfer C; Variante der Korrektur #67)",
        "Registernummern, keine Messung",
    ),
    (
        "`zo_c_spiegel_umgebungen::c3_null_eintrag_im_eigenen_verzeichnis_gilt_als_fehlend`",
        "ein Testname (Befund C-3 der Runde 1), keine Messung",
    ),
    (
        "ein Eintrag `null` im eigenen Verzeichnis hielt die Suche",
        "das PDF-Schlüsselwort null, keine Messung",
    ),
    (
        "den Namen mit dem Wert `null` — direkt oder als Verweis ins Leere",
        "das PDF-Schlüsselwort null, keine Messung",
    ),
    (
        "Mutationsnachweis: `null` wieder als vorhanden",
        "das PDF-Schlüsselwort null, keine Messung",
    ),
    (
        "lag keine Seite** (Register #85, Prüfer C; Variante der Korrektur #67)",
        "Registernummern, keine Messung",
    ),
    (
        "löst Poppler `/MC0` in den Ressourcen der Seite auf",
        "ein Ressourcenname, keine Messung",
    ),
    (
        "`zo_c_spiegel_umgebungen::c3_erscheinung_mit_eigenen_ressourcen_ohne_properties_name_aus_der_seite`",
        "ein Testname (Befund C-3 der Runde 1), keine Messung",
    ),    (
        "galt als nicht gezeichnet** (Register #88, Prüfer C; neue Klasse)",
        "eine Registernummer, keine Messung",
    ),
    (
        "`/Properties` folgt weiter der Regel aus #67",
        "eine Registernummer, keine Messung",
    ),
    (
        "(Register #89, Prüfer E; Dienstverweigerung, die Zeile #64/#83 der Probenliste)",
        "Registernummern, keine Messung",
    ),
    (
        "(Register #99, Prüfer D; Dienstverweigerung, die Zeile #64/#83 der Probenliste)",
        "Registernummern, keine Messung",
    ),
    (
        "(Register #103, beim Befund #98 gefunden; abgelehnte gewöhnliche Datei)",
        "Registernummern, keine Messung",
    ),
    (
        "Aufgefallen bei der Korrektur zu #98",
        "eine Registernummer, keine Messung",
    ),
    (
        "(Register #98, Prüfer D; stilles Leck des Orakels, neue Klassen)",
        "eine Registernummer, keine Messung",
    ),
    (
        "(Register #96, Prüfer D; stilles Leck des Orakels, neue Klasse)",
        "eine Registernummer, keine Messung",
    ),
    (
        "(Register #95, Prüfer D; stilles Leck des Orakels, die Zeile #80 der Probenliste trat weiter auf)",
        "Registernummern, keine Messung",
    ),
    (
        "`/Filter[/ASCII85Decode/FlateDecode]`",
        "ein Filtername als Beispiel der Schreibweise, keine Messung",
    ),
    (
        "`/DecodeParms<</Predictor 12/Columns 8>>`",
        "PDF-Syntax als Beispiel der Schreibweise, keine Messung",
    ),
    (
        "`/DecodeParms 9 0 R`",
        "ein Verweis als Beispiel der Schreibweise, keine Messung",
    ),
    (
        "der zu ASCII85 am genau passenden Budget hielt seit #64",
        "ein Filtername und eine Registernummer, keine Messung",
    ),
    (
        "`ze_p1_befunde`, `ze_p1_budget_und_filter`",
        "Dateinamen, keine Messung",
    ),
    (
        "(Register #97, Prüfer D; stilles Leck des Orakels, die Zeile #81 der Probenliste trat weiter auf)",
        "Registernummern, keine Messung",
    ),
    (
        "PDF 2.0 sie erlaubt",
        "eine Versionsnummer der Norm, keine Messung",
    ),
    (
        "UTF-8 mit BOM",
        "der Name einer Kodierung, keine Messung",
    ),
    (
        "UTF-8 am BOM",
        "der Name einer Kodierung, keine Messung",
    ),
    (
        "(Register #100, Prüfer D; Dienstverweigerung, neue Klasse)",
        "eine Registernummer, keine Messung",
    ),
    (
        "(Register #90, Prüfer B; stilles Leck, die Zeile #71 der Probenliste trat weiter auf)",
        "Registernummern, keine Messung",
    ),
    (
        "(Register #91, Prüfer B; stilles Leck, die Zeile #71 der Probenliste trat weiter auf)",
        "Registernummern, keine Messung",
    ),
    (
        "(Register #94, Prüfer B; Dienstverweigerung, neue Klasse)",
        "eine Registernummer, keine Messung",
    ),
    (
        "ein eigener Befund (Register #106)",
        "eine Registernummer, keine Messung",
    ),
    (
        "(Register #92, erster Teil, Prüfer B; stilles Leck, die Zeile der Metadaten-Träger trat weiter auf)",
        "eine Registernummer, keine Messung",
    ),
    (
        "(Register #92, zweiter Teil, Prüfer B; stilles Leck, die Zeile zum Beiwerk mit Klartext trat weiter auf)",
        "eine Registernummer, keine Messung",
    ),
    (
        "(Register #92, dritter Teil, Prüfer B; stilles Leck, die Zeile zum Beiwerk mit Klartext trat weiter auf)",
        "eine Registernummer, keine Messung",
    ),
    (
        "`/OutputIntents` an der Seite (PDF 2.0) und die Einheiten `/3DU` einer 3D-Annotation",
        "eine Versionsnummer der Norm und ein Schlüsselname, keine Messung",
    ),
    (
        "jede Stelle mit Rückgabewert 0 und ohne Warnung",
        "der Rückgabewert des Laufs vor der Änderung, keine Messgröße",
    ),
    (
        "verliert seine Texte wie am Katalog, `/3DU` fällt als Ganzes",
        "ein Schlüsselname, keine Messung",
    ),
    (
        "übergangen und `/3DU` aus der Liste genommen",
        "ein Schlüsselname, keine Messung",
    ),
    (
        "(Register #107, beim dritten Teil von #92 gefunden; stilles Leck, die Zeile zum Beiwerk mit Klartext trat weiter auf)",
        "Registernummern, keine Messung",
    ),
    (
        "Seit #92 behielten Katalog, Seitenbaum und Seite nur",
        "eine Registernummer, keine Messung",
    ),
    (
        "(Register #93, Prüfer B; stilles Leck, die Zeile der Metadaten-Träger trat weiter auf)",
        "eine Registernummer, keine Messung",
    ),
    (
        "(Register #87, Prüfer C; Leck, von `--check-leaks` gefunden, die Zeile #67 der Probenliste trat weiter auf)",
        "Registernummern, keine Messung",
    ),
    (
        "Trägt ein Formular `/Properties /MC0` selbst",
        "der Name einer Eigenschaftsliste (Ressourcenname), keine Messung",
    ),
    (
        "(Register #106, bei #94 gefunden; Dienstverweigerung, die Klasse aus #94 an Inhaltsströmen und Formularen)",
        "Registernummern, keine Messung",
    ),
    (
        "Jetzt führt das Buch aus #94 auch das Konto",
        "eine Registernummer, keine Messung",
    ),
    (
        "einen Schalter dafür gibt es nicht, wie bei der Decke aus #94",
        "eine Registernummer, keine Messung",
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
    // Der Schnitt beginnt beim jüngsten Abschnitt — der ersten Zeile, die mit
    // `## ` beginnt: bis zum Release-Schnitt `## Unveröffentlicht`, danach
    // `## <Version> — <Datum>`; am Namen hängt er nicht mehr, sonst bräche
    // jeder Release-Schnitt diese Prüfung. Nicht bei der ersten
    // `### Fix-Runde`: der VORSPANN der Release-Notizen lag sonst ausserhalb des
    // geprueften Bereichs, und genau dort stehen die Zahlen, die den Abschnitt
    // zusammenfassen („sieben Fix-Runden“, „vier stille Lecks“). Eine Zahl, die
    // eine Zusammenfassung traegt, ist so viel eine Zusage wie eine im Text —
    // und sie war ungebunden. Beleg: `zi_e_lage_der_messzahlen`.
    let anfang = text.find("\n## ").map_or(ueberschriften[0], |i| i + 1);
    assert!(
        anfang < ueberschriften[0],
        "der Vorspann liegt vor der ersten Fix-Runde — sonst schneidet der Block \
         ihn wieder weg"
    );
    glatt(&text[anfang..ueberschriften[2]])
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
///
/// Die Liste ging einmal bis „zwölf“ und hatte „siebzehn“ von Hand
/// nachgetragen — sie wuchs also genau dort, wo jemand hinsah. Eine Zahl wie
/// „achtzehn Stellen“ wäre daneben still liegen geblieben, und still liegen
/// bleiben ist in diesem Test das eine, was nicht passieren darf: er ist die
/// Gegenrichtung zu [`die_zahlen_der_doku_sind_gebunden`] und behauptet, im
/// Block sei **keine** Zahl unbedeckt. Deshalb steht die Reihe jetzt vollständig
/// da — bis „neunzehn“, die Zehner, „hundert“ und „tausend“ — und die
/// `…mal`-Formen entstehen aus derselben Reihe statt aus einer zweiten Liste.
fn traegt_zahl(wort: &str, ausgezeichnet: bool) -> bool {
    // Kardinalzahlen. **Nicht** die Ordnungszahlen („der vierte“): die stehen
    // hier für einen Platz in einer Liste und nicht für eine Messung. „eins“
    // fehlt mit Absicht — „ein“ und „eine“ sind der unbestimmte Artikel und
    // stünden in jedem zweiten Satz.
    const ZAHLWORTE: [&str; 26] = [
        // „null“ und „eins“ sind nie der unbestimmte Artikel — anders als
        // „ein“/„eine“, die deshalb weiter fehlen. „Rückgabewert null“ ist eine
        // Zusage wie „Rückgabewert 3“ zwei Absätze weiter, und sie stand
        // ungebunden da.
        "null",
        "eins",
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
        "dreizehn",
        "vierzehn",
        "fünfzehn",
        "sechzehn",
        "siebzehn",
        "achtzehn",
        "neunzehn",
        "zwanzig",
        "dreißig",
        "vierzig",
        "fünfzig",
        "sechzig",
        "siebzig",
    ];
    // Der Rest der Reihe. Zwei Listen, weil `achtzig`/`neunzig`/`hundert`/
    // `tausend` auch als Bestandteil zusammengesetzter Zahlwörter vorkommen
    // („zweihundert“) — dort greift die Endungsprüfung weiter unten.
    // `dutzend` ist keine Kardinalzahl, aber eine Menge, die ein Satz statt
    // einer Zahl schreiben kann („ein Dutzend Objekte“) — und genau darum geht
    // es hier.
    const WEITERE: [&str; 5] = ["achtzig", "neunzig", "hundert", "tausend", "dutzend"];
    let kern: String = wort
        .chars()
        .filter(|c| !"*`„“»«().,;:—–!?[]…\u{202f}".contains(*c))
        .collect();
    if kern.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    let klein = kern.to_lowercase();
    // `…mal` aus derselben Reihe: „siebenmal“ ist „sieben“ plus Suffix und
    // braucht keinen eigenen Eintrag.
    let ohne_mal = klein.strip_suffix("mal").unwrap_or(&klein);
    if ZAHLWORTE.contains(&ohne_mal) || WEITERE.contains(&ohne_mal) {
        return true;
    }
    // Zusammengesetzt — aber nur, wenn **davor ein Zahlwort steht**.
    // „zweihundert“ ja, „Jahrhundert“ nein: die Endung allein hielt ein
    // gewöhnliches Wort für eine Zahl. Das machte den Wächter strenger und nicht
    // schwächer, war aber trotzdem falsch — ein falscher Treffer verlangt
    // irgendwann eine Ausnahme, und die deckt dann etwas anderes mit.
    if WEITERE.iter().any(|w| {
        ohne_mal.len() > w.len() && ohne_mal.ends_with(w) && {
            let vorn = &ohne_mal[..ohne_mal.len() - w.len()];
            ZAHLWORTE.contains(&vorn) || WEITERE.contains(&vorn) || vorn == "ein"
        }
    }) {
        return true;
    }
    // **Und die Zusage in Fettschrift.** Ordnungszahlen und „ein/eine/einen“
    // stehen mit Absicht nicht in der Reihe: im Fließtext sind sie ein Platz in
    // einer Liste oder der unbestimmte Artikel. Steht so ein Wort aber in
    // `**…**`, ist es keine Floskel, sondern die Zusage des Satzes — „als
    // **erste** Anweisung“, „genau **einen** Namen“, „**eine** Stelle“. Genau
    // dort standen drei der vier Sicherheitspunkte der Fix-Runde 8, und jede
    // dieser Zusagen ließ sich umdrehen, ohne dass ein Test es merkte. Das
    // Sternchen unterscheidet den Artikel von der Behauptung, und es kostet
    // keine Ausnahme.
    const ORDNUNGSSTAEMME: [&str; 12] = [
        "erst", "zweit", "dritt", "viert", "fünft", "sechst", "siebt", "acht", "neunt", "zehnt",
        "elft", "zwölft",
    ];
    const ARTIKELZAHLEN: [&str; 6] = ["ein", "eine", "einen", "einem", "einer", "eines"];
    if ausgezeichnet {
        if ARTIKELZAHLEN.contains(&ohne_mal) {
            return true;
        }
        if ORDNUNGSSTAEMME.iter().any(|stamm| {
            ohne_mal
                .strip_prefix(stamm)
                .is_some_and(|rest| matches!(rest, "e" | "er" | "en" | "es" | "em"))
        }) {
            return true;
        }
    }
    false
}

/// Bis zu so vielen Wörtern gilt ein `**…**`-Lauf als **Betonung**; darüber als
/// Überschrift.
///
/// Drei, weil die Zusagen, um die es geht, so aussehen: „als **erste**
/// Anweisung", „genau **einen** Namen", „**eine** Stelle", „eines **zweiten**
/// Exports". Die Überschriften der Listenpunkte sind Sätze.
const FETT_WORTE: usize = 3;

/// Welche Bytes des Textes stehen in einem `**…**`-Lauf?
///
/// Gebraucht von [`traegt_zahl`]: ein Zahlwort in Fettschrift ist keine
/// Floskel, sondern die Zusage des Satzes. Ein **offener** Lauf (ein `**`, das
/// keinen Partner findet) zeichnet nichts aus — sonst gälte der Rest des
/// Blocks als fett.
///
/// Und ein **langer** Lauf zählt ebenfalls nicht. Die Listenpunkte dieser Datei
/// beginnen mit einer fetten Überschrift („**Eine Kennung je Export, und zwar
/// …**"), und darin ist „Eine" der Artikel und keine Behauptung über eine
/// Anzahl. Betonung ist kurz: bis zu [`FETT_WORTE`] Wörter. Wer eine Anzahl in
/// einer Überschrift nennt, nennt sie im Satz darunter noch einmal — und dort
/// trägt sie.
fn auszeichnung(text: &str) -> Vec<bool> {
    let bytes = text.as_bytes();
    let mut fett = vec![false; bytes.len()];
    let mut offen: Option<usize> = None;
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'*' {
            match offen.take() {
                Some(anfang) => {
                    let inhalt = &text[anfang + 2..i];
                    if inhalt.split_whitespace().count() <= FETT_WORTE {
                        for b in fett.iter_mut().take(i + 2).skip(anfang) {
                            *b = true;
                        }
                    }
                }
                None => offen = Some(i),
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    fett
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

    // Welche Stellen des Blocks stehen in `**…**`? Ein Auszeichnungslauf kann
    // mehrere Wörter umfassen („genau **einen** Namen" ebenso wie „**erste**"),
    // deshalb wird er über den Block gerechnet und nicht am einzelnen Wort
    // erkannt. Ein Lauf endet spätestens am Absatz.
    let fett = auszeichnung(&block);

    let mut zahlen = 0usize;
    let mut offen: Vec<String> = Vec::new();
    let mut pos = 0usize;
    for wort in block.split(' ') {
        let von = pos;
        pos += wort.len() + 1;
        let ausgezeichnet = fett[von..(von + wort.len()).min(fett.len())]
            .iter()
            .any(|b| *b);
        if !traegt_zahl(wort, ausgezeichnet) {
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

    // **Greift der Schnitt?** Hier stand `zahlen >= 100` — eine Zahl als
    // Ersatz für die Frage. Sie hielt, solange jede Runde zahlenreich war, und
    // schlug an, sobald eine Runde ihre Befunde ohne Messzahlen beschreiben
    // konnte: ein Fehlalarm, der den Wächter zwingt, Zahlen zu erfinden. Was
    // wirklich gemeint ist, lässt sich direkt sagen — der Block trägt den
    // Vorspann und **genau** die beiden jüngsten Fix-Runden.
    let ueberschriften: Vec<&str> = block
        .match_indices("### Fix-Runde ")
        .map(|(i, _)| block[i..].split(':').next().unwrap_or_default())
        .collect();
    assert!(
        block.trim_start().starts_with("## "),
        "der Vorspann liegt nicht im Block — der Schnitt greift nicht mehr"
    );
    assert_eq!(
        ueberschriften.len(),
        2,
        "der Block trägt {} Fix-Runden statt zwei: {ueberschriften:?}",
        ueberschriften.len()
    );
    assert!(
        zahlen >= 20,
        "nur {zahlen} Zahl(en) im Block — für zwei Fix-Runden und einen \
         Vorspann ist das zu wenig, da stimmt etwas mit dem Text nicht"
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

/// Die **sieben** Gründe, aus denen eine Stelle als `NICHT GEPRÜFT` gemeldet
/// wird: Name (so nennt ihn `SECURITY.md`) und ein Stück des Wortlauts, den
/// `redact_pdf::leaks_many_within` wirklich schreibt. Die letzten beiden
/// kamen mit der Spur-A-Runde 2 dazu (Register #98).
///
/// `SECURITY.md` zählte bis zur Fix-Runde 6 **drei** auf — und ließ genau die
/// beiden weg, die die Fix-Runde 5 hinzugefügt hatte. Eine Aufzählung, die
/// weniger nennt, als es gibt, liest sich wie eine vollständige.
const GRUENDE: [(&str, &str); 7] = [
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
    (
        "Strom anders geladen, als er in den Rohbytes steht",
        "der Lader übernahm nicht den Strom, der in den Rohbytes",
    ),
    (
        "Seite, die der Interpreter ablehnt",
        "Sicht 7 (Schriftdekoder): ",
    ),
];

/// **E3.** Jeder Grund, den das Orakel kennt, steht in `SECURITY.md` — und
/// jeder Wortlaut steht wirklich im Quelltext des Orakels.
///
/// Der Quelltext ist hier das Messgerät, weil die Kommandozeile nicht jeden
/// Grund erreichen kann: „die Vorprüfung des Laders lehnt die Datei ab“
/// meldet nur, wer das Orakel **ohne** den Ladeschritt von `check::run`
/// aufruft — also die Oberfläche. Die übrigen werden zusätzlich am gebauten
/// Binary oder an der Bibliothek gefahren
/// (`ze_p4_check_leaks_grenzen::die_gruende_der_ausgabe_stehen_in_security_md`,
/// `zf_q5_unbekannter_filter::*`, `zp_d_seite_und_lader`).
///
/// Mutationsnachweis: in `SECURITY.md` eine Tabellenzeile der Gründe
/// gestrichen → dieser Test ist rot und nennt den fehlenden Grund.
#[test]
fn die_gruende_fuer_nicht_geprueft_stehen_in_security_md() {
    let orakel = glatt(&lf(&std::fs::read_to_string(
        repo_root().join("crates/redact-pdf/src/audit_bytes.rs"),
    )
    .expect("audit_bytes.rs lesbar")));
    let security = glatt(&lf(&std::fs::read_to_string(
        repo_root().join("SECURITY.md"),
    )
    .expect("SECURITY.md lesbar")));

    assert!(
        security.contains("**Sieben Gründe gibt es, nicht drei**"),
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
        security.contains("Sieben Gründe gibt es dafür")
            || readme.contains("Sieben Gründe gibt es dafür"),
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
