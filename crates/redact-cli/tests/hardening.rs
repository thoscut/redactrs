//! Die Härtung am gebauten Binary — Befunde 3 bis 7 der Sicherheitsprüfung.
//!
//! Der Leitsatz aus `SECURITY.md` lautet: **die Software darf für das System,
//! auf dem sie läuft, keine Schwächung bedeuten**, und zugesichert ist ein
//! *kontrollierter Abbruch statt eines Speicherfehlers*. Drei Wege führten
//! trotzdem aus Dateien von wenigen hundert Kilobyte zum systemweiten
//! OOM-Killer oder zu SIGABRT; zwei weitere machten aus einer Einstellung bzw.
//! einem Dateinamen ein Werkzeug gegen den Bedienenden.
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
const BOESER_NAME: &str = "a\u{1b}[31mrot\u{1b}[2K\u{1b}[A.pdf";

/// **Die Auflage:** kein Steuerzeichen erreicht stdout oder stderr.
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
