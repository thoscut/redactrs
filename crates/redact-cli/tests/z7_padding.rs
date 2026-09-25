//! `--padding nan` machte jede Schwärzung wirkungslos — und meldete Erfolg.
//!
//! # Der Befund
//!
//! `--padding` war ein unvalidiertes `f64`. clap nimmt `nan` und `inf` an, und
//! `1e400` wird beim Auswerten zu `inf`. Der Schlüssel `padding` der
//! Einstellungsdatei ist derselbe Wert aus einer anderen Quelle; YAML kennt
//! `.nan` und `.inf`.
//!
//! Gemessen mit dem gebauten Binary auf `redact_pdf::testing::demo_statement`
//! (2 Seiten, 15 Textzeilen, 7 Treffer), **vor** der Härtung:
//!
//! ```text
//! --padding=nan   → Rückgabewert 0, 0 entfernte Zeichen, 0 Deck-Rechtecke,
//!                   Ausgabedatei geschrieben
//! --padding=inf   → dasselbe
//! --padding=1e400 → dasselbe
//! padding: .nan   → dasselbe
//! ```
//!
//! [`redact_core::Rect::expanded`] macht aus jedem Trefferrechteck eines mit
//! NaN-Koordinaten, der Entartungsfilter wirft es weg, und heraus kommt eine
//! **ungeschwärzte Datei mit Rückgabewert 0**. Eine Warnung stand dabei —
//! „leeres Rechteck nach --padding … Ein negatives Padding verkleinert jeden
//! Bereich“ —, aber sie erklärt negative Zahlen, nicht `nan`, und sie steht
//! neben einer Ausgabe, die schon geschrieben ist.
//!
//! # Die Grenze
//!
//! [`redact_pipeline::check_padding`], an beiden Eingängen: als `value_parser`
//! an `--padding` (clap beendet damit selbst mit Rückgabewert 2, bevor eine
//! Datei gelesen wird) und in `Settings::validate` für die Einstellungsdatei.
//!
//! Zugelassen bleibt jede endliche Zahl bis zum Betrag
//! [`redact_pipeline::MAX_PADDING`] — die größte Seitenkante, mit der dieses
//! Programm rechnet. **Negative Werte gehören ausdrücklich dazu**: sie
//! verkleinern jeden Bereich, und das ist eine gültige Absicht.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-z7-pad-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn demo(dir: &Path) -> PathBuf {
    let path = dir.join("kontoauszug.pdf");
    std::fs::write(&path, redact_pdf::testing::demo_statement()).unwrap();
    path
}

/// Ein Lauf mit einem `--padding`-Wert. Die Einstellungsdatei wird ausdrücklich
/// auf eine nicht vorhandene Datei gelenkt, damit ein Profil auf dem
/// Testrechner das Ergebnis nicht verschiebt.
fn run_padding(dir: &Path, input: &Path, wert: &str) -> Output {
    let out = dir.join("out.pdf");
    Command::new(bin())
        .env("REDACT_RS_CONFIG", dir.join("gibt-es-nicht.yaml"))
        .args([
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            &format!("--padding={wert}"),
            "--force",
        ])
        .output()
        .expect("Binary startbar")
}

/// Ein Lauf, dessen `padding` aus der Einstellungsdatei kommt.
fn run_settings(dir: &Path, input: &Path, yaml: &str) -> Output {
    let cfg = dir.join("settings.yaml");
    std::fs::write(&cfg, yaml).unwrap();
    let out = dir.join("out.pdf");
    Command::new(bin())
        .env("REDACT_RS_CONFIG", &cfg)
        .args([
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--force",
        ])
        .output()
        .expect("Binary startbar")
}

fn entfernte_zeichen(out: &Output) -> usize {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("Entfernte Zeichen:"))
        .and_then(|z| z.trim().parse().ok())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Der Befund
// ---------------------------------------------------------------------------

/// **Der Befund.** Kein Wert, der keine Länge ist, kommt durch — und keiner
/// erzeugt eine Ausgabedatei.
///
/// Geprüft wird beides: der Rückgabewert **2** (Benutzungsfehler, nicht 0 und
/// nicht 1) und dass die Ausgabedatei gar nicht erst entsteht. Der zweite Teil
/// ist der eigentliche Punkt — vorher lag dort eine ungeschwärzte Datei, die
/// aussah wie das Ergebnis eines gelungenen Laufs.
#[test]
fn padding_ohne_laenge_wird_an_der_grenze_abgelehnt() {
    let dir = workdir("keine-laenge");
    let input = demo(&dir);
    for wert in ["nan", "NaN", "inf", "-inf", "1e400", "-1e400"] {
        let out = run_padding(&dir, &input, wert);
        assert_eq!(
            out.status.code(),
            Some(2),
            "--padding={wert}: Rückgabewert {:?}, stderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("padding"),
            "--padding={wert}: die Meldung nennt den Schalter nicht: {stderr}"
        );
        assert!(
            !dir.join("out.pdf").exists(),
            "--padding={wert}: es wurde trotzdem eine Ausgabedatei geschrieben"
        );
    }
}

/// Dieselbe Regel für die Einstellungsdatei. `.nan` und `.inf` sind gültiges
/// YAML, und `serde_yaml` macht daraus klaglos ein `f64`.
#[test]
fn padding_ohne_laenge_wird_auch_in_der_einstellungsdatei_abgelehnt() {
    let dir = workdir("yaml-keine-laenge");
    let input = demo(&dir);
    for yaml in ["padding: .nan\n", "padding: .inf\n", "padding: -.inf\n"] {
        let out = run_settings(&dir, &input, yaml);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{yaml:?}: Rückgabewert {:?}, stderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !dir.join("out.pdf").exists(),
            "{yaml:?}: es wurde trotzdem eine Ausgabedatei geschrieben"
        );
    }
}

/// Die Obergrenze: ein Rand jenseits der größten Seitenkante, mit der dieses
/// Programm rechnet, ist keine Angabe mehr, sondern ein Vertippen.
///
/// Beide Seiten der Grenze stehen hier — ohne den zweiten Teil wäre der Test
/// auch dann grün, wenn die Grenze bei 1 läge und jede brauchbare Polsterung
/// abwiese.
#[test]
fn padding_jenseits_der_groessten_seitenkante_wird_abgelehnt() {
    let dir = workdir("obergrenze");
    let input = demo(&dir);
    let grenze = redact_pipeline::MAX_PADDING;

    for wert in [format!("{}", grenze + 1.0), format!("{}", -grenze - 1.0)] {
        let out = run_padding(&dir, &input, &wert);
        assert_eq!(
            out.status.code(),
            Some(2),
            "--padding={wert}: kam durch, stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // Genau auf der Grenze ist noch erlaubt.
    let out = run_padding(&dir, &input, &format!("{grenze}"));
    assert_eq!(
        out.status.code(),
        Some(0),
        "--padding={grenze} wurde abgelehnt, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ---------------------------------------------------------------------------
// Gegenprobe — eine Grenze, die alles ablehnt, ist keine Härtung
// ---------------------------------------------------------------------------

/// **Gegenprobe.** Gewöhnliche Ränder laufen weiter durch, und sie wirken.
///
/// Gemessen am selben Dokument: mit Rand 1 und 2,5 werden 134 Zeichen
/// entfernt, ohne Rand und mit −2,5 sind es 127, mit 100 sind es 480. Die
/// Zahlen stehen hier nicht als Selbstzweck — ohne sie wäre der Test auch dann
/// grün, wenn jeder Lauf eine unveränderte Datei zurückgäbe.
#[test]
fn gewoehnliche_raender_laufen_weiter_durch() {
    let dir = workdir("gegenprobe");
    let input = demo(&dir);
    for (wert, erwartet) in [
        ("0", 127usize),
        ("1", 134),
        ("2.5", 134),
        ("-2.5", 127),
        ("1e2", 480),
    ] {
        let out = run_padding(&dir, &input, wert);
        assert_eq!(
            out.status.code(),
            Some(0),
            "--padding={wert} wurde abgelehnt, stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            entfernte_zeichen(&out),
            erwartet,
            "--padding={wert}: die Schwärzung wirkt anders als gemessen. stdout: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(
            dir.join("out.pdf").exists(),
            "--padding={wert}: keine Ausgabe"
        );
    }
}

/// Und die Einstellungsdatei mit einem gewöhnlichen Wert ebenso.
#[test]
fn ein_gewoehnlicher_wert_in_der_einstellungsdatei_laeuft_weiter_durch() {
    let dir = workdir("gegenprobe-yaml");
    let input = demo(&dir);
    let out = run_settings(&dir, &input, "padding: 2.5\n");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(entfernte_zeichen(&out), 134);
}

/// Ein negativer Rand, der jedes Rechteck leert, bleibt **erlaubt** — er ist
/// eine gültige Absicht, nur eine wirkungslose.
///
/// Das ist die Abgrenzung zu `nan`: dort war der Wert selbst keine Länge, hier
/// ist er eine, die nichts übrig lässt. Diesen Fall behandelt die vorhandene
/// Warnung samt Rückgabewert 0, und dabei bleibt es.
#[test]
fn ein_negativer_rand_der_alles_leert_bleibt_erlaubt() {
    let dir = workdir("negativ");
    let input = demo(&dir);
    let out = run_padding(&dir, &input, "-100");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(entfernte_zeichen(&out), 0);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("leeres Rechteck und konnten nichts entfernen"),
        "die Warnung zum Fall fehlt: {stderr}"
    );
}

/// Ein Wert, der gar keine Zahl ist, war schon vorher abgelehnt — die Meldung
/// darf sich durch den eigenen Auswerter nicht verschlechtern.
#[test]
fn ein_nichtzahlenwert_wird_weiterhin_verstaendlich_abgelehnt() {
    let dir = workdir("keine-zahl");
    let input = demo(&dir);
    let out = run_padding(&dir, &input, "zwei");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("keine Zahl"), "{stderr}");
}
