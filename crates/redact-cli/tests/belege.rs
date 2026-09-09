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
//! Verglichen werden nur die Zeilen, die die Doku zitiert. Die Fundstellen
//! darunter tragen Byte-Offsets, und die hängen am Objektlayout, das
//! `make-preview.sh` ohnehin bei jedem Lauf neu erzeugt; ihr Vergleich stünde
//! im `git diff` nach dem Lauf, nicht hier.
//!
//! Die Suchbegriffe werden **aus dem Skript gelesen**, nicht hier
//! abgeschrieben — sonst wären es zwei Listen, und dieser Test prüfte die
//! falsche.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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
    let pruefung = std::fs::read_to_string(repo_root().join("docs/pruefung.txt"))
        .expect("docs/pruefung.txt lesbar");
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
    let pruefung = std::fs::read_to_string(repo_root().join("docs/pruefung.txt"))
        .expect("docs/pruefung.txt lesbar");
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
