//! Die Konfigurationsdatei bestimmt nicht mehr, wie viel Arbeitsspeicher sie
//! kostet.
//!
//! Der Befund: `--patterns-config` prüfte nicht, was da liegt, sondern las erst
//! (`std::fs::read_to_string` legt einen Puffer in Dateigröße an) und fragte
//! dann. Gemessen am gebauten Binary — eine dünn belegte Datei belegt 4 kB auf
//! der Platte:
//!
//! | Datei | vorher | nachher |
//! |---|---|---|
//! | 2 GB dünn belegt | 2 114 MB, 14,8 s | 17 MB, 0,00 s |
//! | 6 GB dünn belegt | 6 149 MB (Messung der Prüfung) | 17 MB, 0,00 s |
//! | 4 MB, 102 710 Muster | 1 155 MB, 5:59 min | 17 MB, 0,06 s |
//! | benannte Pipe | 5 502 MB nach 15 s, kein Ende | 17 MB, 0,01 s |
//!
//! Gemessen wird hier nicht der Speicher — ein Test, der Gigabyte belegt, reißt
//! den Testläufer mit —, sondern **ob abgelehnt wird und woran**: dass die
//! Meldung die Größe nennt, kann sie nur aus der Angabe des Dateisystems haben,
//! also von vor dem ersten gelesenen Byte.

use std::path::PathBuf;

use redact_core::RedactError;
use redact_patterns::PatternMatcher;

/// Ein eigenes Verzeichnis je Test; Tests laufen nebenläufig.
fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "redact-patterns-limit-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// **Die Auflage:** die Grenze greift *vor* dem Lesen.
///
/// Zu sehen daran, dass die Meldung die Größe nennt — und dass die Datei dünn
/// belegt ist: gelesen hätte sie 6 GB gekostet, abgelehnt kostet sie nichts.
#[test]
fn a_file_beyond_the_limit_is_refused_before_it_is_read() {
    let dir = tempdir("gross");
    let path = dir.join("riesig.yaml");
    let file = std::fs::File::create(&path).unwrap();
    // Dünn belegt: 6 GB Nennlänge, 0 Byte geschrieben.
    file.set_len(6 * 1024 * 1024 * 1024).unwrap();
    drop(file);

    match PatternMatcher::from_config_file(&path) {
        Err(RedactError::Pattern(msg)) => {
            assert!(msg.contains("riesig.yaml"), "{msg}");
            // Die Größe kann nur aus der Dateiangabe stammen …
            assert!(msg.contains("6144 MB"), "{msg}");
            // … die Meldung sagt, welche Grenze das war …
            assert!(msg.contains("1 MB"), "{msg}");
            // … und wo sie herkommt.
            assert!(msg.contains("feste Grenze"), "{msg}");
            assert!(msg.contains("--patterns-config"), "{msg}");
        }
        other => panic!("Pattern-Fehler erwartet, war: {other:?}"),
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage:** eine benannte Pipe wird abgelehnt, statt endlos zu liefern.
///
/// Ohne Schreiber am anderen Ende, und das ist der Punkt: schon das *Öffnen*
/// einer Pipe ohne Schreiber blockiert endlos. Dass dieser Test zurückkehrt,
/// ist die Aussage.
///
/// `cfg(unix)` sagt, dass es benannte Pipes gibt — nicht, dass `mkfifo` im
/// `PATH` steht. Auf einem schlanken Unix-Bild ohne die Werkzeuge (BusyBox
/// ohne `mkfifo`, ein Container mit `scratch`-Basis) ließ
/// `expect("mkfifo startbar")` den Testlauf platzen, obwohl am Programm nichts
/// falsch war — derselbe Fehler, den `belege.rs` bei `python3` schon
/// vermeidet: fehlt das Werkzeug, ist hier nichts zu prüfen, und der Test
/// **sagt das** und endet grün.
///
/// Dasselbe gilt für **jeden** Ausgang außer null — und das ist die Korrektur
/// der Korrektur. Der Anlassfall selbst war damit nicht gedeckt: auf einem
/// BusyBox-Bild steht der Name als Symlink im `PATH` und das Applet fehlt;
/// `status()` liefert `Ok(exit status: 127)` und nicht `Err`. Ein Ziel ohne
/// FIFOs (vfat, ein 9p-Bindmount), eine Verweigerung von `mknod` durch seccomp
/// oder ein BusyBox-Wrapper tun es ebenso. Scheitert `mkfifo`, gibt es **keine**
/// Pipe — es ist genauso nichts zu prüfen wie bei fehlendem Werkzeug, und ein
/// harter `assert` auf das Startergebnis wäre derselbe Fehler eine Ebene höher.
#[cfg(unix)]
#[test]
fn a_named_pipe_is_refused_without_opening_it() {
    let dir = tempdir("pipe");
    let path = dir.join("muster.yaml");
    let fehlt = match std::process::Command::new("mkfifo").arg(&path).status() {
        Ok(status) if status.success() => None,
        Ok(status) => Some(format!("mkfifo endete mit {status}")),
        Err(e) => Some(format!("kein mkfifo im Pfad: {e}")),
    };
    if let Some(grund) = fehlt {
        eprintln!(
            "keine benannte Pipe angelegt ({grund}) — dass `PatternMatcher::from_config_file` \
             eine benannte Pipe ablehnt, bleibt hier ungeprüft"
        );
        std::fs::remove_dir_all(&dir).ok();
        return;
    }

    match PatternMatcher::from_config_file(&path) {
        Err(RedactError::Pattern(msg)) => {
            assert!(msg.contains("gewöhnliche Datei"), "{msg}");
            assert!(msg.contains("muster.yaml"), "{msg}");
        }
        other => panic!("Pattern-Fehler erwartet, war: {other:?}"),
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// Die Grenze ist eng, weil jedes kompilierte Muster rund 12 kB kostet: eine
/// Konfiguration knapp über 1 MB (rund 26 000 Muster) belegte gemessen 306 MB,
/// eine mit 4 MB schon 1 155 MB und sechs Minuten.
///
/// Der Test baut die Datei aus echten, gültigen Mustern — sie scheitert also
/// an ihrer Größe und nicht daran, dass sie unsinnig wäre.
#[test]
fn a_configuration_with_absurdly_many_patterns_is_refused() {
    let dir = tempdir("viele");
    let path = dir.join("muster.yaml");
    let mut yaml = String::from("extend_builtins: false\npatterns:\n");
    let mut n = 0;
    while yaml.len() <= 1024 * 1024 {
        yaml.push_str(&format!("  - id: p{n}\n    regex: 'A{n}[0-9]+'\n"));
        n += 1;
    }
    std::fs::write(&path, &yaml).unwrap();

    match PatternMatcher::from_config_file(&path) {
        Err(RedactError::Pattern(msg)) => {
            assert!(msg.contains("muster.yaml"), "{msg}");
            assert!(msg.contains("erlaubt sind 1 MB"), "{msg}");
            assert!(msg.contains("12 kB"), "der Grund fehlt: {msg}");
        }
        other => panic!("Pattern-Fehler erwartet, war: {other:?}"),
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// Die Gegenprobe: eine gewöhnliche Konfiguration geht weiterhin durch — die
/// mitgelieferte aus `examples/` und eine mit tausend Mustern.
#[test]
fn an_ordinary_configuration_still_loads() {
    let example =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patterns.yaml");
    let matcher = PatternMatcher::from_config_file(&example)
        .expect("die mitgelieferte Konfiguration muss ladbar sein");
    assert!(matcher.active_count() > 0);

    let dir = tempdir("normal");
    let path = dir.join("muster.yaml");
    let mut yaml = String::from("extend_builtins: false\npatterns:\n");
    for n in 0..1_000 {
        yaml.push_str(&format!("  - id: p{n}\n    regex: 'A{n}[0-9]+'\n"));
    }
    std::fs::write(&path, &yaml).unwrap();
    let matcher = PatternMatcher::from_config_file(&path).expect("1 000 Muster sind erlaubt");
    assert_eq!(matcher.active_count(), 1_000);

    std::fs::remove_dir_all(&dir).ok();
}

/// Eine Datei, die kein Text ist, wird beim Namen genannt — und ihr Inhalt
/// bleibt aus der Meldung heraus. Der Pfad kommt von außen und kann auf eine
/// fremde Datei zeigen.
#[test]
fn a_file_that_is_not_utf8_is_named_but_not_quoted() {
    let dir = tempdir("binaer");
    let path = dir.join("muster.yaml");
    std::fs::write(&path, [0xffu8, 0xfe, 0x00, 0x41]).unwrap();

    match PatternMatcher::from_config_file(&path) {
        Err(RedactError::Pattern(msg)) => {
            assert!(msg.contains("muster.yaml"), "{msg}");
            assert!(msg.contains("UTF-8"), "{msg}");
        }
        other => panic!("Pattern-Fehler erwartet, war: {other:?}"),
    }

    std::fs::remove_dir_all(&dir).ok();
}
