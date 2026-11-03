//! Ein Schreibfehler auf stdout ist kein Absturz.
//!
//! Vorher schrieb die Zusammenfassung mit `println!`: `redact-rs
//! --list-patterns | head -1` und `redact-rs a.pdf > /dev/full` endeten mit
//! einer Panik und Rückgabewert 101 — ein Wert aus keiner Tabelle, und im
//! zweiten Fall, nachdem die Ausgabedatei schon geschrieben war
//! (Lieferketten-Review, Befund 4). Jetzt: eine geschlossene Pipe beendet die
//! Ausgabe still, jeder andere Schreibfehler ist ein IO-Fehler mit 1.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-zq-e-out-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn command(dir: &Path) -> Command {
    let mut cmd = Command::new(bin());
    cmd.current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir)
        .env("APPDATA", dir);
    cmd
}

/// Der Leser ist schon gegangen, bevor die erste Zeile kommt — wie `head
/// -1` an einer Ausgabe, die nicht in den Puffer der Pipe passt. Das Lese-Ende
/// wird gleich nach dem Start geschlossen; das Kind braucht zum Hochfahren
/// länger, sein erster Schreibversuch trifft also sicher `EPIPE` (Rust
/// ignoriert `SIGPIPE`, der Fehler kommt als `BrokenPipe` an).
fn mit_geschlossener_pipe(mut cmd: Command) -> std::process::Output {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    child.wait_with_output().unwrap()
}

#[test]
fn geschlossene_pipe_endet_still() {
    let dir = workdir("pipe");
    let mut cmd = command(&dir);
    cmd.arg("--list-patterns");
    let out = mit_geschlossener_pipe(cmd);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(target_os = "linux")]
#[test]
fn volle_platte_ist_ein_io_fehler() {
    let dir = workdir("full");
    let input = dir.join("x.pdf");
    let status = command(&dir)
        .arg("--write-demo")
        .arg(&input)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());

    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let out = command(&dir)
        .args(["x.pdf", "-o", "o.pdf"])
        .stdout(full)
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("IO-Fehler"), "{stderr}");
    std::fs::remove_dir_all(&dir).ok();
}
