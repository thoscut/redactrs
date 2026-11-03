//! Stapel: das Ergebnis einer Datei darf keine **andere** Eingabe desselben
//! Stapels überschreiben — auch nicht mit `--force`.
//!
//! Vorher: `redact-rs x.pdf x_geschwaerzt.pdf --force` schrieb das Ergebnis
//! von `x.pdf` auf das Original `x_geschwaerzt.pdf`, verarbeitete danach das
//! frische Ergebnis, meldete zwei Erfolge und endete mit 0. Das Original war
//! weg (Lieferketten-Review, Befund 3).

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-zq-e-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn demo(path: &Path) {
    let status = Command::new(bin())
        .arg("--write-demo")
        .arg(path)
        .env("HOME", path.parent().unwrap())
        .status()
        .unwrap();
    assert!(status.success());
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(dir)
        // Keine Einstellungsdatei des Prüfrechners.
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir)
        .env("APPDATA", dir)
        .output()
        .unwrap()
}

#[test]
fn ergebnis_auf_einer_anderen_eingabe_lehnt_den_stapel_ab() {
    let dir = workdir("force");
    demo(&dir.join("x.pdf"));
    // Ein eigenes Original, das zufällig wie ein Ergebnis heißt.
    let original = b"%PDF-1.5\n% eigenes Original, kein Ergebnis\n".to_vec();
    std::fs::write(dir.join("x_geschwaerzt.pdf"), &original).unwrap();

    let out = run(&dir, &["x.pdf", "x_geschwaerzt.pdf", "--force"]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("selbst eine Eingabe"), "{stderr}");
    assert_eq!(
        std::fs::read(dir.join("x_geschwaerzt.pdf")).unwrap(),
        original,
        "das Original wurde überschrieben"
    );
    assert!(
        !dir.join("x_geschwaerzt_geschwaerzt.pdf").exists(),
        "der Stapel ist trotzdem gelaufen"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Andere Schreibweise desselben Pfads: `./` und `dir/..` kommen nicht vorbei.
#[test]
fn andere_schreibweise_kommt_nicht_vorbei() {
    let dir = workdir("schreibweise");
    std::fs::create_dir(dir.join("c")).unwrap();
    demo(&dir.join("c/x.pdf"));
    demo(&dir.join("c/x_geschwaerzt.pdf"));
    let before = std::fs::read(dir.join("c/x_geschwaerzt.pdf")).unwrap();

    let out = run(&dir, &["./c/x_geschwaerzt.pdf", "c/../c/x.pdf", "--force"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read(dir.join("c/x_geschwaerzt.pdf")).unwrap(),
        before
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Die Prüfung trifft nur den Zusammenstoß: ein gewöhnlicher Stapel läuft,
/// und dieselbe Datei zweimal genannt ist weiter kein solcher Fall.
#[test]
fn gewoehnlicher_stapel_laeuft_weiter() {
    let dir = workdir("normal");
    demo(&dir.join("a.pdf"));
    demo(&dir.join("b.pdf"));
    let out = run(&dir, &["a.pdf", "b.pdf", "-q"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join("a_geschwaerzt.pdf").exists());
    assert!(dir.join("b_geschwaerzt.pdf").exists());

    let out = run(&dir, &["a.pdf", "./a.pdf", "-q", "--force"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("selbst eine Eingabe"), "{stderr}");
    std::fs::remove_dir_all(&dir).ok();
}
