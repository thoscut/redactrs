//! `--force` macht ein Ziel mit engeren Rechten nicht wieder lesbar.
//!
//! Vorher entstand die neue Datei immer mit 0644 und ersetzte per `rename`
//! die alte samt ihrem Modus: aus `chmod 600 ergebnis.pdf` wurde nach dem
//! nächsten Lauf mit `--force` stillschweigend 0644 (Lieferketten-Review,
//! Befund 5). Jetzt bekommt die neue Datei höchstens die Rechte der alten.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-zq-e-mode-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(dir: &Path, args: &[&str]) {
    let out = Command::new(bin())
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn engere_rechte_des_ziels_bleiben() {
    let dir = workdir("eng");
    run(&dir, &["--write-demo", "x.pdf"]);
    run(&dir, &["x.pdf", "-o", "o.pdf", "-q"]);
    std::fs::set_permissions(dir.join("o.pdf"), std::fs::Permissions::from_mode(0o600)).unwrap();

    run(&dir, &["x.pdf", "-o", "o.pdf", "-q", "--force"]);
    assert_eq!(mode(&dir.join("o.pdf")), 0o600);
    std::fs::remove_dir_all(&dir).ok();
}

/// Weitere Rechte der alten Datei erweitern nichts: höchstens die Vorgabe.
#[test]
fn weitere_rechte_des_ziels_erweitern_nichts() {
    let dir = workdir("weit");
    run(&dir, &["--write-demo", "x.pdf"]);
    run(&dir, &["x.pdf", "-o", "o.pdf", "-q"]);
    std::fs::set_permissions(dir.join("o.pdf"), std::fs::Permissions::from_mode(0o666)).unwrap();

    run(&dir, &["x.pdf", "-o", "o.pdf", "-q", "--force"]);
    assert_eq!(mode(&dir.join("o.pdf")) & !0o644, 0, "mehr als 0644");
    std::fs::remove_dir_all(&dir).ok();
}
