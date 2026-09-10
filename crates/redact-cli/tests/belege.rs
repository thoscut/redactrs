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
