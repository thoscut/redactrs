//! Die Härtung am gebauten Binary — Befunde 3 bis 7 der Sicherheitsprüfung.
//!
//! Der Leitsatz aus `SECURITY.md` lautet: **die Software darf für das System,
//! auf dem sie läuft, keine Schwächung bedeuten**, und zugesichert ist ein
//! *kontrollierter Abbruch statt eines Speicherfehlers*. Drei Wege führten
//! trotzdem aus Dateien von wenigen hundert Kilobyte zum systemweiten
//! OOM-Killer oder zu SIGABRT; zwei weitere machten aus einer Einstellung bzw.
//! einem Dateinamen ein Werkzeug gegen den Bedienenden.
//!
//! Dazu der Abschnitt „Befund 4, Fortsetzung“: derselbe Fehler wie bei der
//! Eingabedatei — erst lesen, dann fragen — steckte in vier weiteren Schaltern
//! (`--booking-list`, `--patterns-config`, `--manual-regions`,
//! `--apply-review`). Über sie ließ sich der Prozess mit einer Datei, die auf
//! der Platte 4 kB belegt, auf mehrere Gigabyte treiben oder endlos anhalten.
//!
//! ## Warum hier keine Megabyte stehen
//!
//! Spitzenspeicher lässt sich in einem Testfall schlecht messen: der
//! interessante Fall ist gerade der, in dem der Prozess *nicht* zurückkehrt,
//! und ein Prozess, der 15 GB belegt, reißt den Testläufer mit. Gemessen wird
//! deshalb, was sich messen lässt — **abgelehnt oder angenommen**, der
//! Rückgabewert, die genannte Größe, und ob eine Ausgabedatei entstanden ist.
//! Die Speicherzahlen (mit `getrusage(RUSAGE_CHILDREN)` vorher/nachher
//! erhoben) stehen in `SECURITY.md`.
//!
//! ## Zwei Systeme, eine Zusage
//!
//! Drei Tests zu Befund 6 tragen ein `#[cfg(unix)]`, weil ihr Angriffsweg —
//! ein Steuerzeichen **im Dateinamen** — unter Windows gar nicht existiert:
//! `CreateFile` lehnt jedes Zeichen unter `U+0020` ab. Ohne das `cfg` war das
//! kein Vorteil, sondern ein Ausfall: `cargo test` scheiterte auf
//! `windows-2025` beim Anlegen der Datei, und damit gab es für das
//! Windows-Artefakt überhaupt keinen grünen Testlauf. Was auf Windows sehr wohl
//! geht, prüfen die Tests im Abschnitt „Befund 6, Fortsetzung“ — dort steht
//! auch, was dort prinzipiell nicht prüfbar ist.
//!
//! Aus demselben Grund trägt der Pipe-Test zu Befund 4 eines: es gibt unter
//! Windows weder `mkfifo` noch eine benannte Pipe, die sich einem Pfad
//! unterschieben ließe — sie leben dort unter `\\.\pipe\` und kommen als
//! Argument eines Dateischalters nicht vor. Die Größengrenze dagegen gilt auf
//! beiden Systemen und wird auf beiden geprüft; sie ist die Hälfte des
//! Schutzes, die überall greift.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use redact_pipeline::testing::{
    ENCRYPTED_BOMB_PDF, ENCRYPTED_NESTING_BOMB_PDF, ENCRYPTED_PDF, ENCRYPTED_PDF_PASSWORD,
};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-haerte-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    run_with_settings("/nicht/vorhanden.yaml", args)
}

fn run_with_settings(settings: &str, args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .env("REDACT_RS_CONFIG", settings)
        .env_remove("REDACT_RS_PASSWORD")
        .output()
        .expect("Binary startbar")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Ein Lauf, der weder abgestürzt ist noch durchgelaufen: genau ein
/// kontrollierter Abbruch.
///
/// Unter Unix hat ein durch ein Signal beendeter Prozess **keinen**
/// Rückgabewert (`status.code()` ist `None`) — genau daran ist ein
/// Speicherfehler zu erkennen, und genau das darf nicht mehr vorkommen.
#[track_caller]
fn assert_controlled_failure(out: &Output) {
    let code = out.status.code();
    assert!(
        code.is_some(),
        "der Prozess wurde durch ein Signal beendet (SIGKILL/SIGABRT) statt \
         mit einer Meldung abzubrechen: {:?}",
        out.status
    );
    assert_ne!(
        code,
        Some(0),
        "der Lauf ist durchgelaufen:\n{}",
        stdout(out)
    );
}

// ---------------------------------------------------------------------------
// Befund 3 — verschlüsselte Eingabe umging die Stream-Budgets vollständig
// ---------------------------------------------------------------------------

/// **Die Auflage:** die verschlüsselte Bombe endet mit einem klaren Fehler,
/// nicht mit SIGKILL oder SIGABRT.
///
/// Vorher: mit Passwort 3 876 MB Spitzenspeicher und SIGABRT unter `ulimit -v`,
/// ohne Grenze der systemweite OOM-Killer. Ohne Passwort war dieselbe Datei in
/// 0,0 s abgelehnt — das Passwort schaltete also nicht nur die Entschlüsselung
/// frei, sondern auch sämtliche Budgets ab.
#[test]
fn an_encrypted_decompression_bomb_ends_with_a_message_not_with_a_signal() {
    let dir = workdir("bombe");
    let input = write(&dir, "bombe.pdf", ENCRYPTED_BOMB_PDF);
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--password",
        ENCRYPTED_PDF_PASSWORD,
    ]);

    assert_controlled_failure(&out);
    let message = stderr(&out);
    assert!(message.contains("Budget"), "{message}");
    assert!(
        message.contains("entschlüsselt gilt weiter"),
        "die Meldung sagt nicht, dass entschlüsselt gemessen wurde: {message}"
    );
    assert!(!output.exists(), "trotz Ablehnung geschrieben");

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage:** die verschlüsselte Verschachtelungsbombe läuft nicht mehr
/// mit Rückgabewert 0 durch.
///
/// Der stillere und deshalb schlimmere der beiden Fälle. Vorher: Rückgabewert
/// 0, „0 Schwärzungen“, Ausgabe geschrieben — und in der Ausgabe stand die
/// IBAN unverändert. Wer das Ergebnis an der Zusammenfassung prüft, hält eine
/// Datei für sauber, in der nichts geschwärzt wurde.
#[test]
fn an_encrypted_nesting_bomb_does_not_quietly_produce_an_unredacted_file() {
    let dir = workdir("verschachtelt");
    let input = write(&dir, "nest.pdf", ENCRYPTED_NESTING_BOMB_PDF);
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--password",
        ENCRYPTED_PDF_PASSWORD,
        "--patterns",
        "iban_de",
    ]);

    assert_controlled_failure(&out);
    assert!(
        stderr(&out).contains("Verschachtelungstiefe"),
        "{}",
        stderr(&out)
    );
    assert!(
        !output.exists(),
        "es wurde eine Ausgabe geschrieben — und die enthielte die IBAN"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Die Gegenprobe, ohne die die beiden Tests oben nichts wert wären: ein
/// gewöhnliches verschlüsseltes Dokument wird weiterhin geöffnet **und**
/// geschwärzt.
#[test]
fn an_ordinary_encrypted_document_is_still_opened_and_redacted() {
    let dir = workdir("normal");
    let input = write(&dir, "auszug.pdf", ENCRYPTED_PDF);
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--password",
        ENCRYPTED_PDF_PASSWORD,
        "--patterns",
        "iban_de",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        redact_pdf::leaks(&std::fs::read(&output).unwrap(), "DE89").is_empty(),
        "die IBAN steht noch da"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Dieselbe Datei, dieselbe Antwort: die Grenze hängt nicht daran, ob ein
/// Passwort im Spiel war. Vorher hing genau das daran.
#[test]
fn the_answer_does_not_depend_on_whether_a_password_was_given() {
    let dir = workdir("gleich");
    let input = write(&dir, "bombe.pdf", ENCRYPTED_BOMB_PDF);

    let ohne = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("a.pdf").to_str().unwrap(),
    ]);
    let mit = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("b.pdf").to_str().unwrap(),
        "--password",
        ENCRYPTED_PDF_PASSWORD,
    ]);

    assert_controlled_failure(&ohne);
    assert_controlled_failure(&mit);
    assert!(!dir.join("a.pdf").exists());
    assert!(!dir.join("b.pdf").exists());

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Befund 4 — keine Obergrenze für die Eingabedatei
// ---------------------------------------------------------------------------

/// Eine dünn belegte Datei: 4 kB auf der Platte, `len` Byte Nennlänge.
///
/// Das ist der Fall aus dem Befund, und er ist mit einem Aufruf gebaut. Wer
/// eine solche Datei in ein Stapelverzeichnis legt, brauchte vorher weder
/// Rechte noch Platz, um den Rechner umzuwerfen.
fn sparse(dir: &Path, name: &str, len: u64) -> PathBuf {
    let path = dir.join(name);
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(len).unwrap();
    path
}

/// **Die Auflage:** eine Größengrenze **vor** dem Lesen.
///
/// Vorher: 6 149 MB Spitzenspeicher und 20 s, bevor überhaupt feststand, dass
/// die Datei kein PDF ist. Dass die Grenze jetzt *vor* dem Lesen greift, ist
/// im Test daran zu sehen, dass die Meldung die Größe nennt — die kann sie nur
/// aus der Dateiangabe haben — und dass der Lauf sofort zurückkommt.
#[test]
fn a_file_far_beyond_the_budget_is_refused_before_it_is_read() {
    let dir = workdir("sparse");
    let input = sparse(&dir, "riesig.pdf", 6 * 1024 * 1024 * 1024);

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert_controlled_failure(&out);
    let message = stderr(&out);
    assert!(message.contains("6144 MB"), "{message}");
    assert!(message.contains("512 MB"), "{message}");
    assert!(message.contains("riesig.pdf"), "{message}");
    assert!(message.contains("--max-input-mb"), "{message}");

    std::fs::remove_dir_all(&dir).ok();
}

/// Die Grenze ist einstellbar — nach oben wie nach unten.
#[test]
fn the_input_budget_can_be_set() {
    let dir = workdir("einstellbar");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());

    // Eine absurd enge Grenze lehnt auch ein gewöhnliches PDF ab …
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("a.pdf").to_str().unwrap(),
        "--max-input-mb",
        "0",
    ]);
    assert_controlled_failure(&out);
    assert!(stderr(&out).contains("--max-input-mb"), "{}", stderr(&out));

    // … und mit der Vorgabe läuft dieselbe Datei durch.
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("b.pdf").to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage:** im Stapel wird jede Datei benannt, **bevor** sie angefasst
/// wird — und eine riesige Datei reißt den Stapel nicht mit.
#[test]
fn a_batch_names_every_file_before_touching_it() {
    let dir = workdir("stapel");
    write(&dir, "a.pdf", &redact_pdf::testing::demo_statement());
    sparse(&dir, "riesig.pdf", 6 * 1024 * 1024 * 1024);
    write(&dir, "z.pdf", &redact_pdf::testing::demo_statement());

    let out = run(&[dir.to_str().unwrap(), "--patterns", "iban_de"]);
    let message = stderr(&out);

    // Jede der drei Dateien steht mit Zähler in der Meldung, und zwar in der
    // Reihenfolge, in der sie abgearbeitet werden.
    for (n, name) in [(1, "a.pdf"), (2, "riesig.pdf"), (3, "z.pdf")] {
        assert!(
            message.contains(&format!("[{n}/3] ")),
            "der Zähler {n}/3 fehlt:\n{message}"
        );
        assert!(message.contains(name), "{name} fehlt:\n{message}");
    }
    // Die riesige Datei steht **vor** ihrer Fehlermeldung — sonst wäre der
    // Name erst bekannt, wenn es zu spät ist.
    let angekuendigt = message.find("[2/3]").expect("Ankündigung");
    let gescheitert = message.find("FEHLGESCHLAGEN").expect("Meldung");
    assert!(angekuendigt < gescheitert, "{message}");

    // Und die beiden gesunden Dateien sind trotzdem geschwärzt.
    assert!(dir.join("a_geschwaerzt.pdf").exists());
    assert!(dir.join("z_geschwaerzt.pdf").exists());
    assert_eq!(out.status.code(), Some(1), "eine Datei ist gescheitert");

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Befund 5 — `output_suffix` mit Pfadanteilen
// ---------------------------------------------------------------------------

/// **Die Auflage:** ein Namenszusatz mit Pfadtrennern wird abgelehnt.
///
/// Vorher: drei Dateien, Meldung „3 verarbeitet, 0 fehlgeschlagen“,
/// Rückgabewert 0 — und in `ziel/alle.pdf` stand nur das Ergebnis der letzten.
/// Der Dateistamm war weg, die Zwischenverzeichnisse hatte `check_target`
/// selbst angelegt.
#[test]
fn a_suffix_with_a_path_component_is_refused_from_the_settings_file() {
    let dir = workdir("zusatz");
    let eingang = dir.join("ein");
    std::fs::create_dir_all(&eingang).unwrap();
    for name in ["eins.pdf", "zwei.pdf", "drei.pdf"] {
        write(&eingang, name, &redact_pdf::testing::demo_statement());
    }
    let settings = write(
        &dir,
        "settings.yaml",
        b"output_suffix: \"/../../ziel/alle\"\n",
    );

    let out = run_with_settings(
        settings.to_str().unwrap(),
        &[eingang.to_str().unwrap(), "--force"],
    );

    assert_eq!(out.status.code(), Some(2), "Konfigurationsfehler ⇒ 2");
    let message = stderr(&out);
    assert!(message.contains("Namenszusatz"), "{message}");
    assert!(message.contains("Pfadtrenner"), "{message}");
    // Nichts geschrieben, und vor allem: kein Verzeichnis angelegt.
    assert!(!dir.join("ziel").exists(), "ziel/ wurde angelegt");
    assert!(!eingang.join("ziel").exists());

    std::fs::remove_dir_all(&dir).ok();
}

/// Derselbe Zusatz über die Kommandozeile — derselbe Ausgang. Eine Prüfung,
/// die nur die Einstellungsdatei kennt, ließe den zweiten Weg offen.
#[test]
fn a_suffix_with_a_path_component_is_refused_on_the_command_line() {
    let dir = workdir("zusatz-cli");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());

    for suffix in ["/../../ziel/alle", "../alle", "unter/alle"] {
        let out = run(&[input.to_str().unwrap(), "--output-suffix", suffix]);
        assert_controlled_failure(&out);
        assert!(stderr(&out).contains("Namenszusatz"), "{}", stderr(&out));
    }
    assert!(!dir.join("ziel").exists());

    // Gegenprobe: ein gewöhnlicher Zusatz wirkt weiterhin.
    let out = run(&[
        input.to_str().unwrap(),
        "--output-suffix",
        "_anonym",
        "--patterns",
        "iban_de",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(dir.join("auszug_anonym.pdf").exists());

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Befund 6 — Steuerzeichen aus Dateinamen auf dem Terminal
// ---------------------------------------------------------------------------

/// Ein Dateiname mit ESC-Folgen darin.
///
/// `ESC [ 2 K` löscht die Zeile, `ESC [ A` fährt eine Zeile hoch: damit lässt
/// sich die Zusammenfassung eines Stapels überschreiben, bis „1 fehlgeschlagen“
/// als „0 fehlgeschlagen“ dasteht.
#[cfg(unix)]
const BOESER_NAME: &str = "a\u{1b}[31mrot\u{1b}[2K\u{1b}[A.pdf";

/// **Die Auflage:** kein Steuerzeichen erreicht stdout oder stderr.
///
/// Nur unter Unix, siehe den Abschnitt „Befund 6, Fortsetzung“ weiter unten:
/// `CreateFile` lehnt jedes Zeichen unter `U+0020` ab, [`BOESER_NAME`] lässt
/// sich dort also gar nicht erst anlegen. Vorher stand hier kein `cfg`, und
/// genau daran ist die Windows-Hälfte der CI gescheitert — mit
/// `Os { code: 123, kind: InvalidFilename }` schon beim Anlegen der Datei.
#[cfg(unix)]
#[test]
fn control_characters_from_a_file_name_never_reach_the_terminal() {
    let dir = workdir("steuer");
    write(&dir, BOESER_NAME, &redact_pdf::testing::demo_statement());
    write(&dir, "normal.pdf", &redact_pdf::testing::demo_statement());

    for args in [
        vec![dir.to_str().unwrap(), "--patterns", "iban_de", "--force"],
        vec![dir.to_str().unwrap(), "--review", "--force"],
    ] {
        let out = run(&args);
        for (kanal, text) in [
            ("stdout", out.stdout.clone()),
            ("stderr", out.stderr.clone()),
        ] {
            assert!(
                !text.contains(&0x1b),
                "{kanal} enthält ein ESC-Byte: {:?}",
                String::from_utf8_lossy(&text)
            );
        }
        // Und die Datei ist trotzdem erkennbar genannt — entschärft heißt
        // nicht verschwiegen.
        assert!(
            stdout(&out).contains("rot") || stderr(&out).contains("rot"),
            "der Name wird gar nicht mehr genannt"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// Auch die Fehlermeldung eines Stapels darf keine Zeile fälschen können.
#[cfg(unix)]
#[test]
fn a_failing_file_with_a_hostile_name_cannot_forge_the_summary() {
    let dir = workdir("steuer-fehler");
    // Kein PDF: diese Datei scheitert und wird gemeldet.
    write(&dir, BOESER_NAME, b"kein PDF");
    write(&dir, "gut.pdf", &redact_pdf::testing::demo_statement());

    let out = run(&[dir.to_str().unwrap(), "--patterns", "iban_de"]);
    assert!(!out.stdout.contains(&0x1b), "{}", stdout(&out));
    assert!(!out.stderr.contains(&0x1b), "{}", stderr(&out));
    // Die Zusammenfassung sagt die Wahrheit und steht als letzte Zeile da.
    assert!(
        stdout(&out).contains("1 fehlgeschlagen"),
        "{}",
        stdout(&out)
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// In `--json` bleibt der Name vollständig — dort liest ihn keine Anzeige,
/// sondern ein Programm, und `serde_json` schreibt Steuerzeichen als ``.
///
/// Das ist die eine Zusage dieses Abschnitts, für die es unter Windows **kein**
/// Gegenstück gibt: dort kann kein Steuerzeichen in einen Dateinamen, und einen
/// anderen Weg, eines in die JSON-Ausgabe zu bekommen, gibt es nicht. Siehe
/// den Abschnitt „Befund 6, Fortsetzung“.
#[cfg(unix)]
#[test]
fn json_keeps_the_name_but_escapes_it_itself() {
    let dir = workdir("steuer-json");
    write(&dir, BOESER_NAME, &redact_pdf::testing::demo_statement());
    write(&dir, "normal.pdf", &redact_pdf::testing::demo_statement());

    let out = run(&[
        dir.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--json",
        "--force",
    ]);
    assert!(!out.stdout.contains(&0x1b), "{}", stdout(&out));
    assert!(stdout(&out).contains("\\u001b"), "{}", stdout(&out));

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Befund 6, Fortsetzung — und unter Windows?
// ---------------------------------------------------------------------------
//
// Die drei Tests oben tragen die Zusage „fremde Zeichen erreichen das Terminal
// nicht“ — und liefen bis hierher **nur** unter Unix, ohne dass es dort stand.
// Für das Windows-Artefakt, die eigentliche Zielgruppe dieses Programms, gab es
// die Zusage damit gar nicht: `cargo test` brach auf `windows-2025` beim
// Anlegen der Datei ab (`Os { code: 123, kind: InvalidFilename }`).
//
// ## Was auf Windows prinzipiell nicht geht
//
// Ein Dateiname darf dort kein Zeichen unter `U+0020` enthalten — `CreateFile`
// weist das ab, und zwar unabhängig vom Dateisystem. Ein ESC in einem
// *Dateinamen* ist auf Windows also kein Angriffsweg, sondern eine
// Unmöglichkeit. Dasselbe gilt für den Zeilenumbruch im Namen und damit für
// die JSON-Zusage („`serde_json` maskiert Steuerzeichen selbst“): über einen
// Dateinamen kommt dort kein Steuerzeichen in die Ausgabe.
//
// ## Was sehr wohl geht
//
// Zwei Wege bleiben, und beide sind hier abgedeckt:
//
// 1. **Steuerzeichen aus einem Argument.** Die Kommandozeile ist kein
//    Dateiname; sie darf jedes Zeichen tragen, auf beiden Systemen. `redact-rs`
//    nimmt mit `--output-suffix` einen Namensbestandteil von aussen entgegen
//    und nennt ihn in der Fehlermeldung wieder — der Weg von der Eingabe auf
//    das Terminal ist derselbe wie beim Dateinamen.
// 2. **Richtungsumschalter im Dateinamen.** `U+202E` (RIGHT-TO-LEFT OVERRIDE)
//    liegt über `U+0020` und ist auf Windows in einem Dateinamen erlaubt — es
//    ist dort seit Jahren der übliche Trick, `…gpj.exe` als `…exe.jpg` aussehen
//    zu lassen. Es steuert kein Terminal, dreht aber die Leserichtung um und
//    kann damit dieselbe Zeile fälschen. `redact_core::display` behandelt es
//    aus genau diesem Grund wie ein Steuerzeichen.
//
// Die Tests dazu laufen bewusst auf **beiden** Systemen: unter Unix sind sie
// eine zusätzliche Prüfung, unter Windows sind sie die einzige.
//
// Nicht abgedeckt bleiben die Windows-eigenen Fallstricke `CON`/`NUL`, der
// abschließende Punkt und das abschließende Leerzeichen. Sie gehören nicht zu
// dieser Zusage: sie fälschen keine Ausgabe, sondern lassen einen Pfad auf
// etwas anderes zeigen, als er zu sagen scheint. Das ist ein eigener Befund und
// keine Fussnote zu diesem hier.

/// Ein Dateiname, der die Leserichtung umdreht — auf beiden Systemen anlegbar.
const RICHTUNGSNAME: &str = "a\u{202e}rot.pdf";

/// **Die Auflage, Windows-Hälfte:** ein Steuerzeichen aus einem *Argument*
/// erreicht das Terminal ebenso wenig wie eines aus einem Dateinamen.
///
/// `--output-suffix` ist der einzige Schalter, der einen Namensbestandteil von
/// aussen annimmt; abgelehnt wird er samt Begründung, und in der Begründung
/// steht er drin. Genau dort muss er entschärft sein.
#[test]
fn a_control_character_from_an_argument_never_reaches_the_terminal() {
    let dir = workdir("steuer-argument");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());

    let out = run(&[
        input.to_str().unwrap(),
        "--output-suffix",
        "a\u{1b}[2K\u{1b}[A",
    ]);

    assert_controlled_failure(&out);
    for (kanal, text) in [
        ("stdout", out.stdout.clone()),
        ("stderr", out.stderr.clone()),
    ] {
        assert!(
            !text.contains(&0x1b),
            "{kanal} enthält ein ESC-Byte: {:?}",
            String::from_utf8_lossy(&text)
        );
    }
    // Entschärft heißt auch hier nicht verschwiegen: die Meldung sagt weiterhin,
    // welcher Zusatz gemeint ist und warum er keiner ist.
    let message = stderr(&out);
    assert!(message.contains("\\u{1b}"), "{message}");
    assert!(message.contains("Steuerzeichen"), "{message}");

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage, Windows-Hälfte:** auch der Richtungsumschalter erreicht die
/// Anzeige nicht — und die Datei wird trotzdem genannt.
#[test]
fn a_direction_override_in_a_file_name_never_reaches_the_terminal() {
    let dir = workdir("richtung");
    write(&dir, RICHTUNGSNAME, &redact_pdf::testing::demo_statement());
    write(&dir, "normal.pdf", &redact_pdf::testing::demo_statement());

    for args in [
        vec![dir.to_str().unwrap(), "--patterns", "iban_de", "--force"],
        vec![dir.to_str().unwrap(), "--review", "--force"],
    ] {
        let out = run(&args);
        let gesamt = format!("{}{}", stdout(&out), stderr(&out));
        assert!(
            !gesamt.contains('\u{202e}'),
            "der Richtungsumschalter steht in der Ausgabe: {gesamt}"
        );
        assert!(
            gesamt.contains("\\u{202e}"),
            "er wurde weder ausgegeben noch sichtbar gemacht: {gesamt}"
        );
        assert!(
            gesamt.contains("rot"),
            "der Name wird gar nicht mehr genannt"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage, Windows-Hälfte:** eine scheiternde Datei mit einem solchen
/// Namen kann die Zusammenfassung nicht fälschen.
#[test]
fn a_failing_file_with_a_direction_override_cannot_hide_in_the_summary() {
    let dir = workdir("richtung-fehler");
    // Kein PDF: diese Datei scheitert und wird gemeldet.
    write(&dir, RICHTUNGSNAME, b"kein PDF");
    write(&dir, "gut.pdf", &redact_pdf::testing::demo_statement());

    let out = run(&[dir.to_str().unwrap(), "--patterns", "iban_de"]);
    let gesamt = format!("{}{}", stdout(&out), stderr(&out));
    assert!(!gesamt.contains('\u{202e}'), "{gesamt}");
    assert!(
        stdout(&out).contains("1 fehlgeschlagen"),
        "{}",
        stdout(&out)
    );
    assert_eq!(out.status.code(), Some(1), "eine Datei ist gescheitert");

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Gegenprobe, Windows-Hälfte:** in `--json` bleibt der Name so, wie er
/// auf der Platte steht — Zeichen für Zeichen.
///
/// Das ist bewusst die *andere* Erwartung als bei den Steuerzeichen: JSON liest
/// ein Programm, und ein Programm braucht den echten Namen, um die Datei
/// wiederzufinden. `serde_json` maskiert `U+202E` nicht, weil es aus JSON-Sicht
/// ein gewöhnliches Zeichen ist — hier wird deshalb geprüft, dass der Name den
/// Weg unbeschädigt übersteht.
#[test]
fn json_keeps_a_direction_override_verbatim() {
    let dir = workdir("richtung-json");
    write(&dir, RICHTUNGSNAME, &redact_pdf::testing::demo_statement());
    write(&dir, "normal.pdf", &redact_pdf::testing::demo_statement());

    let out = run(&[
        dir.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--json",
        "--force",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));

    let berichte: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("die JSON-Ausgabe ist gültiges JSON");
    let namen: Vec<&str> = berichte
        .as_array()
        .expect("eine Liste je Datei")
        .iter()
        .filter_map(|e| e.get("input").and_then(|v| v.as_str()))
        .collect();
    // Verglichen wird der Dateiname, nicht der ganze Pfad: der Pfadtrenner
    // unterscheidet sich zwischen den Systemen, der Name ist der Prüfling.
    assert!(
        namen.iter().any(|n| n.ends_with(RICHTUNGSNAME)),
        "der Name kommt aus der JSON-Ausgabe nicht unverändert zurück: {namen:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Befund 7 — die Fehlermeldung der Einstellungsdatei zitierte fremden Inhalt
// ---------------------------------------------------------------------------

/// **Die Auflage:** die Meldung gibt keine Zeile der Datei wieder.
///
/// `REDACT_RS_CONFIG` zeigt auf einen beliebigen Pfad, und an der Vorgabestelle
/// kann ein Symlink stehen. Keine Rechtegrenze wird dabei überschritten — das
/// Programm liest mit den Rechten des Nutzers —, aber es war ein Weg,
/// beliebige Zeilen einer fremden Datei in Protokolle und Bildschirmfotos zu
/// befördern.
#[test]
fn the_settings_error_does_not_quote_the_file() {
    let dir = workdir("fremd");
    const GEHEIM: &str = "$6$abcdefgh$SEHRGEHEIMESPASSWORT";
    let fremd = write(
        &dir,
        "schatten",
        format!("root:*:20501:0:99999:7::\nnutzer:{GEHEIM}:20501:0:99999:7:::\n").as_bytes(),
    );
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());

    let out = run_with_settings(fremd.to_str().unwrap(), &[input.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2), "Konfigurationsfehler ⇒ 2");

    let message = format!("{}{}", stdout(&out), stderr(&out));
    for teil in [GEHEIM, "20501", "99999", "root:*"] {
        assert!(
            !message.contains(teil),
            "„{teil}“ aus der fremden Datei steht in der Meldung:\n{message}"
        );
    }
    // Brauchbar bleibt sie trotzdem: sie nennt die Datei, die Stelle und die
    // erlaubten Schlüssel.
    assert!(message.contains("schatten"), "{message}");
    assert!(message.contains("Zeile"), "{message}");
    assert!(message.contains("output_suffix"), "{message}");

    std::fs::remove_dir_all(&dir).ok();
}

/// Ein Tippfehler in einer *echten* Einstellungsdatei wird weiterhin beim
/// Namen genannt — sonst hätte die Härtung die Meldung nur unbrauchbar
/// gemacht.
#[test]
fn a_typo_in_a_real_settings_file_is_still_named() {
    let dir = workdir("tippfehler");
    let settings = write(&dir, "settings.yaml", b"output_sufix: _x\n");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());

    let out = run_with_settings(settings.to_str().unwrap(), &[input.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("output_sufix"), "{}", stderr(&out));

    std::fs::remove_dir_all(&dir).ok();
}

/// Und auch die Einstellungsdatei selbst wird nicht mehr in beliebiger Größe
/// gelesen — derselbe Fehler wie bei der Eingabedatei, nur an anderer Stelle.
#[test]
fn an_oversized_settings_file_is_refused() {
    let dir = workdir("gross-settings");
    let settings = dir.join("settings.yaml");
    let file = std::fs::File::create(&settings).unwrap();
    file.set_len(2 * 1024 * 1024 * 1024).unwrap();
    drop(file);
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());

    let out = run_with_settings(settings.to_str().unwrap(), &[input.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("zu groß"), "{}", stderr(&out));

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Befund 4, Fortsetzung — dieselbe Lücke in den Hilfsdatei-Lesern
// ---------------------------------------------------------------------------
//
// Die Eingabe-PDF (`--max-input-mb`) und die Einstellungsdatei prüften erst und
// lasen dann; vier weitere Schalter taten es umgekehrt. `--booking-list`,
// `--patterns-config`, `--manual-regions` und `--apply-review` gingen mit
// `std::fs::read_to_string` bzw. `read_to_end` an einen Pfad, der von außen
// kommt — und legten damit einen Puffer in Dateigröße an, bevor überhaupt
// feststand, ob dort eine Liste, eine Konfiguration oder nur eine dünn belegte
// Datei liegt. Gemessen am gebauten Binary (dünn belegt: 4 kB auf der Platte,
// 6 GB Nennlänge):
//
// | Schalter | vorher | nachher |
// |---|---|---|
// | `--apply-review`, 6 GB | 6 157 MB, 20,2 s | 15 MB, 0,0 s |
// | `--manual-regions`, 6 GB | 6 160 MB, 12,1 s | 16 MB, 0,0 s |
// | `--apply-review`, Pipe | kein Ende (nach 45 s abgeschossen) | 15 MB, 0,0 s |
// | `--manual-regions`, Pipe | kein Ende (nach 45 s abgeschossen) | 17 MB, 0,0 s |
//
// Die Zahlen für `--booking-list` (5 259 MB bei 1 GB) und `--patterns-config`
// (2 114 MB bei 2 GB) stehen in `redact_core::read`; die Tests hier sind die
// Ebene, die am **gebauten Binary** nachweist, dass es für alle vier gilt.
//
// Gemessen wird in den Tests nicht der Speicher, sondern **woran** abgelehnt
// wird: dass die Meldung die Größe nennt, kann sie nur aus der Angabe des
// Dateisystems haben — also von vor dem ersten gelesenen Byte. Ein Test, der
// wirklich 6 GB belegte, risse den Testläufer mit.

/// Ein Lauf mit **Zeitschranke** — für die Fälle, in denen der Fehler das
/// Hängen selbst ist.
///
/// `run` wartet, bis der Prozess zurückkommt. An einer benannten Pipe ohne
/// Schreiber kommt er nie zurück, und ein hängender Test ist schlimmer als ein
/// roter: er nimmt keinen Rückgabewert an, sondern blockiert den ganzen
/// Testlauf, bis jemand von außen eingreift. Die Schranke gehört deshalb in den
/// Test und nicht in die Erwartung an den Testläufer.
///
/// Bewusst ohne das Programm `timeout`: das ist GNU-Coreutils und liegt weder
/// unter Windows noch auf macOS ohne Zutun bereit. `try_wait` gibt es überall,
/// wo es Rust gibt.
///
/// `None` heißt: die Schranke hat gegriffen, der Prozess wurde abgeschossen.
///
/// Das `#[cfg(unix)]` steht hier nicht, weil die Funktion unter Windows nicht
/// liefe — sie benutzt nur `std` —, sondern weil ihr einziger Aufrufer der
/// Pipe-Test ist. Ohne das `cfg` wäre sie dort ungenutzter Code, und
/// `-D warnings` machte daraus einen roten Lauf auf `windows-2025`.
#[cfg(unix)]
fn run_within(seconds: u64, args: &[&str]) -> Option<Output> {
    use std::process::Stdio;

    let mut child = Command::new(bin())
        .args(args)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Binary startbar");

    let frist = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    loop {
        match child.try_wait().expect("Kindprozess abfragbar") {
            // `wait_with_output` liest die Rohre leer; `wait` darin gibt den
            // schon abgeholten Status unverändert zurück.
            Some(_) => return Some(child.wait_with_output().expect("Ausgabe lesbar")),
            None if std::time::Instant::now() >= frist => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
}

/// Eine benannte Pipe: Länge 0, und beim Lesen kein Ende.
///
/// `#[cfg(unix)]`, weil es unter Windows weder `mkfifo` noch eine benannte
/// Pipe im Dateisystem gibt — dort leben sie unter `\\.\pipe\` und lassen sich
/// keinem Pfad unterschieben, den ein Schalter entgegennimmt. Der Angriffsweg
/// existiert dort also nicht; ein Test, der ihn nachstellen wollte, scheiterte
/// schon am Anlegen und nähme dem Windows-Artefakt seinen grünen Testlauf —
/// derselbe Grund, aus dem drei Tests zu Befund 6 ein `cfg` tragen.
#[cfg(unix)]
fn fifo(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let ok = Command::new("mkfifo")
        .arg(&path)
        .status()
        .expect("mkfifo startbar");
    assert!(ok.success(), "mkfifo ist fehlgeschlagen");
    path
}

/// Die gemeinsame Erwartung an eine dünn belegte Riesendatei hinter einem
/// Schalter: kontrolliert abgelehnt, mit Namen, Größe und Grenze — und ohne
/// Ausgabedatei.
///
/// Die **Größe** in der Meldung ist der eigentliche Nachweis: sie steht nirgends
/// im Dateiinhalt (der ist leer), sondern nur in der Angabe des Dateisystems.
/// Wer sie nennt, hat gefragt, bevor er las.
#[track_caller]
fn assert_refused_by_size(out: &Output, name: &str, grenze: &str, ausgabe: &Path) {
    assert_controlled_failure(out);
    let message = stderr(out);
    assert!(
        message.contains(name),
        "die Datei wird nicht genannt:\n{message}"
    );
    assert!(
        message.contains("6144 MB"),
        "die Größe fehlt — dann kann die Grenze nicht vor dem Lesen gegriffen haben:\n{message}"
    );
    assert!(
        message.contains(grenze),
        "die Grenze {grenze} fehlt:\n{message}"
    );
    assert!(
        message.contains("feste Grenze"),
        "der Hinweis fehlt, woher die Grenze kommt:\n{message}"
    );
    assert!(!ausgabe.exists(), "es ist trotzdem eine Ausgabe entstanden");
}

/// Die gemeinsame Erwartung an eine benannte Pipe hinter einem Schalter:
/// abgelehnt, **bevor** sie geöffnet wird.
///
/// Der Test kommt ohne Schreiber am anderen Ende aus, und das ist gerade der
/// Punkt: schon das *Öffnen* einer Pipe ohne Schreiber blockiert endlos. Kommt
/// der Lauf innerhalb der Schranke zurück, ist vor dem Öffnen entschieden
/// worden.
#[cfg(unix)]
#[track_caller]
fn assert_refused_as_not_a_file(out: Option<Output>, name: &str, ausgabe: &Path) {
    let out = out.unwrap_or_else(|| {
        panic!("der Lauf hängt an der Pipe, statt sie abzulehnen (Zeitschranke abgelaufen)")
    });
    assert_controlled_failure(&out);
    let message = stderr(&out);
    assert!(
        message.contains("gewöhnliche Datei"),
        "die Pipe wird nicht als solche benannt:\n{message}"
    );
    assert!(
        message.contains(name),
        "die Datei wird nicht genannt:\n{message}"
    );
    assert!(!ausgabe.exists(), "es ist trotzdem eine Ausgabe entstanden");
}

/// **Die Auflage:** eine Buchungsliste von 6 GB wird abgelehnt, bevor sie
/// gelesen wird.
///
/// Die Buchungsliste war der schlimmste der vier Wege, weil der CSV-Parser
/// obendrauf kam: aus 1 GB Datei wurden über 5 GB Arbeitsspeicher.
#[test]
fn an_oversized_booking_list_is_refused_before_it_is_read() {
    let dir = workdir("liste-gross");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());
    let liste = sparse(&dir, "riesig.csv", 6 * 1024 * 1024 * 1024);
    let ausgabe = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        ausgabe.to_str().unwrap(),
        "--booking-list",
        liste.to_str().unwrap(),
    ]);
    assert_refused_by_size(&out, "riesig.csv", "16 MB", &ausgabe);
    assert!(
        stderr(&out).contains("--booking-list"),
        "die Meldung sagt nicht, welcher Schalter gemeint ist:\n{}",
        stderr(&out)
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage:** eine Pattern-Konfiguration von 6 GB wird abgelehnt, bevor
/// sie gelesen wird — und mit der engeren Grenze von 1 MB.
#[test]
fn an_oversized_patterns_config_is_refused_before_it_is_read() {
    let dir = workdir("muster-gross");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());
    let muster = sparse(&dir, "riesig.yaml", 6 * 1024 * 1024 * 1024);
    let ausgabe = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        ausgabe.to_str().unwrap(),
        "--patterns-config",
        muster.to_str().unwrap(),
    ]);
    // 1 MB und nicht 16 MB: jedes kompilierte Muster kostet rund 12 kB, die
    // Verstärkung je Byte ist hier eine andere.
    assert_refused_by_size(&out, "riesig.yaml", "1 MB", &ausgabe);

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage:** eine Regionsliste von 6 GB wird abgelehnt, bevor sie
/// gelesen wird.
#[test]
fn oversized_manual_regions_are_refused_before_they_are_read() {
    let dir = workdir("regionen-gross");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());
    let regionen = sparse(&dir, "riesig.json", 6 * 1024 * 1024 * 1024);
    let ausgabe = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        ausgabe.to_str().unwrap(),
        "--manual-regions",
        regionen.to_str().unwrap(),
    ]);
    assert_refused_by_size(&out, "riesig.json", "16 MB", &ausgabe);

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage:** eine Review-Datei von 6 GB wird abgelehnt, bevor sie
/// gelesen wird.
///
/// Zusätzlich geprüft: die Meldung **nennt die Datei**. Vorher kam hier ein
/// nackter `Io`-Fehler heraus — `std::io::Error` führt den Pfad nicht mit, auf
/// dem Schirm stand „Parse-Fehler: expected value at line 1 column 1“, und wer
/// drei Dateien in der Kommandozeile stehen hat, weiß danach nicht, welche
/// gemeint ist.
#[test]
fn an_oversized_review_file_is_refused_before_it_is_read() {
    let dir = workdir("review-gross");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());
    let review = sparse(&dir, "riesig.json", 6 * 1024 * 1024 * 1024);
    let ausgabe = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        ausgabe.to_str().unwrap(),
        "--apply-review",
        review.to_str().unwrap(),
    ]);
    assert_refused_by_size(&out, "riesig.json", "16 MB", &ausgabe);
    assert!(
        stderr(&out).contains("--apply-review"),
        "die Meldung sagt nicht, welcher Schalter gemeint ist:\n{}",
        stderr(&out)
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage:** hinter keinem der vier Schalter lässt sich eine benannte
/// Pipe unterschieben, die den Lauf endlos hängen lässt.
///
/// Alle vier in einem Test, weil es eine einzige Aussage ist und jeder Fall bei
/// einem Rückfall dieselben 30 s kostet. Zum `#[cfg(unix)]` siehe [`fifo`].
#[cfg(unix)]
#[test]
fn a_named_pipe_behind_any_switch_does_not_hang_the_run() {
    let dir = workdir("pipe-schalter");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());

    for (schalter, name) in [
        ("--booking-list", "pipe_liste.csv"),
        ("--patterns-config", "pipe_muster.yaml"),
        ("--manual-regions", "pipe_regionen.json"),
        ("--apply-review", "pipe_review.json"),
    ] {
        let pipe = fifo(&dir, name);
        let ausgabe = dir.join(format!("{name}.pdf"));
        let out = run_within(
            30,
            &[
                input.to_str().unwrap(),
                "-o",
                ausgabe.to_str().unwrap(),
                schalter,
                pipe.to_str().unwrap(),
            ],
        );
        assert_refused_as_not_a_file(out, name, &ausgabe);
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Gegenprobe:** gewöhnliche Hilfsdateien gehen weiterhin durch — sonst
/// wäre der Weg nur zugemauert statt abgesichert.
///
/// Geprüft wird jeder der vier Schalter mit einer Datei, wie sie legitim
/// vorkommt: die Beispiele aus `examples/`, und für die Review-Datei die, die
/// `--review` im selben Lauf erzeugt hat.
#[test]
fn ordinary_auxiliary_files_still_go_through() {
    let dir = workdir("legitim");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());
    let beispiele = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");

    // Zuerst eine echte Review-Datei erzeugen — mit der Prüfsumme des
    // Dokuments, wie sie `--review` schreibt.
    let review = dir.join("review.json");
    let out = run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        review.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));

    for (n, (schalter, datei)) in [
        ("--booking-list", beispiele.join("booking_list.csv")),
        ("--patterns-config", beispiele.join("patterns.yaml")),
        ("--manual-regions", beispiele.join("manual_regions.json")),
        ("--apply-review", review.clone()),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(datei.exists(), "{} fehlt", datei.display());
        let ausgabe = dir.join(format!("out{n}.pdf"));
        let out = run(&[
            input.to_str().unwrap(),
            "-o",
            ausgabe.to_str().unwrap(),
            schalter,
            datei.to_str().unwrap(),
        ]);
        assert!(
            out.status.success(),
            "{schalter} lehnt eine legitime Datei ab:\n{}",
            stderr(&out)
        );
        assert!(
            ausgabe.exists(),
            "{schalter} hat keine Ausgabe erzeugt:\n{}",
            stdout(&out)
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Befund 16 — feindliche Feldwerte in einer formal gültigen Review-Datei
// ---------------------------------------------------------------------------
//
// Die Größengrenze oben prüft, ob die Datei gelesen werden darf. Was danach
// **in** ihr steht, prüfte niemand: `serde_json` nimmt für ein `usize` jede
// Zahl bis 18 446 744 073 709 551 615 an, und die Anzeige rechnet daraus
// „Seite n + 1“. Gemessen mit `"page": 18446744073709551615` in einer sonst
// gültigen Review-Datei — vorher / nachher:
//
// | Binary  | vorher                                       | nachher                |
// |---------|----------------------------------------------|------------------------|
// | Debug   | rc 101, Panic `audit.rs:453` (add overflow)  | rc 1, „keine Seitenzahl“ |
// | Release | rc 0, Ausgabe geschrieben, Warnung „Seite 0“ | rc 1, „keine Seitenzahl“ |
//
// Der Release-Fall war der schlimmere: eine Ausgabedatei mit rc 0 und eine
// Warnung, die auf die falsche Seite zeigt. Die Grenze steht in
// `redact_core::model::MAX_PAGE_INDEX` (an lopdf gebunden, das Seiten als
// `u32` zählt) und greift an der Deserialisierung, also vor dem ersten
// Zugriff auf das Dokument. Dahinter steht als zweite Verteidigung ein
// `saturating_add` an jeder Stelle, die „+ 1“ rechnet.

/// Ein Wert der Review-Datei wird verbogen; was der Lauf damit tun soll.
enum Expectation {
    /// rc 1, keine Ausgabedatei, und die Meldung enthält diesen Text.
    Refused(&'static str),
    /// rc 0 und eine Ausgabedatei: der Wert ist krumm, aber ungefährlich.
    Tolerated,
}

/// **Die Auflage:** kein Feldwert, der durch das Schema passt, bringt den
/// Lauf zum Absturz. Entweder die Datei taugt nicht (rc 1, wie krummes JSON)
/// oder der Wert ist harmlos (rc 0). Rückgabewert 101 — die Panic — kommt
/// nicht vor, und ein Signal auch nicht.
///
/// Rückgabewert 1 und nicht 2: `read_aux_text` legt „die Datei taugt nicht“
/// als `RedactError::Parse` fest; 2 ist für Fehler in der Bedienung
/// reserviert, und an der Bedienung war hier nichts falsch.
///
/// `id` und `pages` werden absichtlich **nicht** geprüft — kein Aufrufer
/// rechnet mit ihnen. Der Test füttert sie trotzdem, damit das so bleibt:
/// sollte jemand einmal `pages + 1` rechnen, fällt es hier auf.
#[test]
fn hostile_field_values_in_a_valid_review_file_end_with_a_message_not_a_panic() {
    let dir = workdir("feldwerte");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());

    // Eine echte Review-Datei mit der Prüfsumme des Dokuments — genau so,
    // wie sie ein Bedienender vor sich hat, bevor er darin herumschreibt.
    let review = dir.join("review.json");
    let out = run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        review.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let original: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&review).unwrap()).unwrap();
    assert!(
        original["items"].as_array().is_some_and(|i| !i.is_empty()),
        "die Review-Datei hat keinen Treffer, an dem sich etwas verbiegen ließe: {original}"
    );

    type Twist = fn(&mut serde_json::Value);
    let cases: [(&str, Twist, Expectation); 7] = [
        (
            "page_max",
            |v| v["items"][0]["region"]["page"] = serde_json::json!(u64::MAX),
            Expectation::Refused("keine Seitenzahl"),
        ),
        (
            "page_minus_one",
            |v| v["items"][0]["region"]["page"] = serde_json::json!(-1),
            Expectation::Refused("Parse-Fehler"),
        ),
        (
            "confidence_minus_one",
            |v| {
                v["items"][0]["region"]["source"]["pattern"]["confidence"] = serde_json::json!(-1.0)
            },
            Expectation::Tolerated,
        ),
        (
            "text_empty",
            |v| v["items"][0]["region"]["text"] = serde_json::json!(""),
            Expectation::Tolerated,
        ),
        (
            "id_max",
            |v| v["items"][0]["id"] = serde_json::json!(u64::MAX),
            Expectation::Tolerated,
        ),
        (
            "pages_zero",
            |v| v["input"]["pages"] = serde_json::json!(0),
            Expectation::Tolerated,
        ),
        (
            "pages_max",
            |v| v["input"]["pages"] = serde_json::json!(u64::MAX),
            Expectation::Tolerated,
        ),
    ];
    let apply = |name: &str, datei: &Path| -> (Output, PathBuf) {
        let ausgabe = dir.join(format!("out-{name}.pdf"));
        let out = run(&[
            input.to_str().unwrap(),
            "-o",
            ausgabe.to_str().unwrap(),
            "--apply-review",
            datei.to_str().unwrap(),
        ]);
        (out, ausgabe)
    };

    let assert_no_panic = |name: &str, out: &Output| {
        let code = out.status.code();
        assert!(
            code.is_some(),
            "{name}: der Prozess wurde durch ein Signal beendet: {:?}",
            out.status
        );
        assert_ne!(
            code,
            Some(101),
            "{name}: der Lauf ist abgestürzt (Panic) statt mit einer Meldung zu enden:\n{}",
            stderr(out)
        );
    };

    for (name, twist, expectation) in cases {
        let mut value = original.clone();
        twist(&mut value);
        let datei = write(&dir, &format!("{name}.json"), value.to_string().as_bytes());
        let (out, ausgabe) = apply(name, &datei);
        assert_no_panic(name, &out);
        match expectation {
            Expectation::Refused(needle) => {
                assert_eq!(out.status.code(), Some(1), "{name}:\n{}", stderr(&out));
                assert!(
                    stderr(&out).contains(needle),
                    "{name}: die Meldung nennt den Grund nicht:\n{}",
                    stderr(&out)
                );
                assert!(
                    !ausgabe.exists(),
                    "{name}: trotz Ablehnung wurde eine Ausgabedatei geschrieben"
                );
            }
            Expectation::Tolerated => {
                assert_eq!(
                    out.status.code(),
                    Some(0),
                    "{name}: ein harmloser Wert wurde abgelehnt:\n{}",
                    stderr(&out)
                );
                assert!(
                    ausgabe.exists(),
                    "{name}: keine Ausgabedatei:\n{}",
                    stdout(&out)
                );
            }
        }
    }

    // `NaN` ist kein JSON; das ist ein Fehler des Formats, nicht des Werts —
    // aber auch er darf nur mit rc 1 enden.
    let text = std::fs::read_to_string(&review).unwrap();
    let stelle = text
        .find("\"confidence\":")
        .expect("die Review-Datei hat eine confidence");
    let ende = stelle + text[stelle..].find([',', '\n', '}']).unwrap();
    let nan = format!("{}\"confidence\": NaN{}", &text[..stelle], &text[ende..]);
    let datei = write(&dir, "confidence_nan.json", nan.as_bytes());
    let (out, ausgabe) = apply("confidence_nan", &datei);
    assert_no_panic("confidence_nan", &out);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!ausgabe.exists());

    // Derselbe Wert hinter `--manual-regions`, als nacktes Array. Der Weg
    // dorthin ist ein anderer (`serde_json::from_str::<Vec<Region>>`), die
    // Grenze dieselbe — und die Meldung nennt den Grund, nicht „expected
    // struct ReviewFile“.
    let region = serde_json::json!([{
        "page": u64::MAX,
        "rect": original["items"][0]["region"]["rect"],
        "text": null,
        "source": { "manual": { "reason": "Feldwert" } }
    }]);
    let datei = write(
        &dir,
        "regionen_page_max.json",
        region.to_string().as_bytes(),
    );
    let ausgabe = dir.join("out-regionen.pdf");
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        ausgabe.to_str().unwrap(),
        "--no-patterns",
        "--manual-regions",
        datei.to_str().unwrap(),
    ]);
    assert_no_panic("manual_regions_page_max", &out);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("keine Seitenzahl"),
        "die Meldung nennt den Grund nicht:\n{}",
        stderr(&out)
    );
    assert!(!ausgabe.exists());

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Die Zusicherung selbst
// ---------------------------------------------------------------------------

/// `SECURITY.md` sagt die Grenzen zu — und muss sagen, dass sie **auch hinter
/// der Entschlüsselung** gelten.
///
/// Der Satz „Wer ein verschlüsseltes PDF öffnet, gibt ihm ausdrücklich mehr
/// Vertrauen“ stand dort und traf nicht zu: passwortgeschützte Kontoauszüge
/// werden samt Passwort verschickt, das Vertrauen gilt dem Absender und nicht
/// der Byte-Struktur der Datei.
#[test]
fn the_security_notes_no_longer_claim_encryption_earns_trust() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../SECURITY.md");
    let text = std::fs::read_to_string(&path).expect("SECURITY.md lesbar");

    // Der Satz darf noch **einmal** vorkommen — als Zitat in der
    // Richtigstellung. Eine Zusicherung, die stillschweigend verschwindet,
    // wäre die schlechtere Fassung: wer sie einmal gelesen hat, glaubt sie
    // weiter.
    const ZURUECKGENOMMEN: &str = "gibt ihm ausdrücklich mehr Vertrauen";
    assert_eq!(
        text.matches(ZURUECKGENOMMEN).count(),
        1,
        "der zurückgenommene Satz steht mehr als einmal (oder gar nicht) in SECURITY.md"
    );
    let stelle = text.find(ZURUECKGENOMMEN).unwrap();
    let richtigstellung = text[..stelle]
        .rfind("**Richtigstellung.**")
        .expect("der Satz steht ohne Richtigstellung da");
    assert!(
        stelle - richtigstellung < 400,
        "der Satz steht nicht in seiner Richtigstellung"
    );
    assert!(
        text.contains("--max-input-mb"),
        "SECURITY.md schweigt zur Grenze der Eingabedatei"
    );
    assert!(
        text.contains("hinter der Entschlüsselung")
            && text.contains("check_limits_after_decryption"),
        "SECURITY.md sagt nicht, dass die Budgets hinter der Entschlüsselung greifen"
    );
    // Und die Messung, auf die sich das stützt, steht dort mit Verfahren.
    assert!(
        text.contains("getrusage"),
        "SECURITY.md nennt das Messverfahren nicht"
    );
}
