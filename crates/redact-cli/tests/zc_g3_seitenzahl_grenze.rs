//! Gegenprüfung Fix-Runde 3 (g3): die Seitenzahl-Grenze `MAX_PAGE_INDEX` am
//! gebauten Binary — **beide Richtungen** und **beide Dateiwege**.
//!
//! `hardening.rs::hostile_field_values_…` prüft nur die Ablehnung mit
//! `u64::MAX` (über `--apply-review` und das nackte Array hinter
//! `--manual-regions`). Hier steht, was dort fehlt:
//!
//! * die Grenze liegt **genau** bei 4 294 967 295: der Wert selbst läuft
//!   durch (rc 0, Ausgabe, Warnung „Seite 4294967296“ — sättigend gerechnet,
//!   auf 64 Bit einfach richtig), eins darüber nicht (rc 1, Meldung nennt
//!   Wert **und** Grenze, keine Ausgabe);
//! * die Review-Datei hinter `--manual-regions` (der dritte Weg) nimmt
//!   dieselbe Grenze — mit der Meldung, die den Grund nennt;
//! * `BlockedRegion.page` in `blocked_by_negative_list` hat dieselbe Grenze,
//!   und der zulässige Höchstwert erscheint in `blocked_details` als
//!   „Seite 4294967296“.
//!
//! Mutationsnachweis: ohne `#[serde(deserialize_with = "page_index")]` an
//! `Region.page`/`BlockedRegion.page` werden die Ablehnungen grün-falsch
//! (rc 0); mit `>=` statt `>` in `page_index` lehnt das Binary den
//! erlaubten Höchstwert ab (falscher Alarm) — beides rot.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MAX: u64 = u32::MAX as u64;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-g3-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
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

/// Eine echte Review-Datei zum Demo-Auszug, mit Prüfsumme.
fn review_for(dir: &Path, input: &Path) -> serde_json::Value {
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
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&review).unwrap()).unwrap();
    assert!(
        value["items"].as_array().is_some_and(|i| !i.is_empty()),
        "kein Treffer in der Review-Datei: {value}"
    );
    value
}

fn blocked(page: u64) -> serde_json::Value {
    serde_json::json!({
        "page": page,
        "rect": { "ll": { "x": 1.0, "y": 1.0 }, "ur": { "x": 5.0, "y": 5.0 } },
        "pattern": "Max", "booking_id": "b003"
    })
}

#[track_caller]
fn assert_refused(name: &str, out: &Output, ausgabe: &Path, value: u64) {
    let err = stderr(out);
    assert_eq!(out.status.code(), Some(1), "{name}:\n{err}");
    assert!(
        err.contains(&format!(
            "\"page\": {value} ist keine Seitenzahl (höchstens {MAX})"
        )),
        "{name}: Meldung nennt Wert und Grenze nicht:\n{err}"
    );
    assert!(
        !err.contains("expected struct"),
        "{name}: serde-Rohtext statt Grund:\n{err}"
    );
    assert!(!ausgabe.exists(), "{name}: Ausgabedatei trotz Ablehnung");
}

#[track_caller]
fn assert_accepted(name: &str, out: &Output, ausgabe: &Path, seite_1basiert: &str) {
    let err = stderr(out);
    assert_eq!(out.status.code(), Some(0), "{name}: falscher Alarm:\n{err}");
    assert!(
        ausgabe.exists(),
        "{name}: keine Ausgabedatei:\n{}",
        stdout(out)
    );
    assert!(
        err.contains(&format!("(Seite {seite_1basiert};")),
        "{name}: die Warnung nennt die Seite nicht 1-basiert und ungekürzt:\n{err}"
    );
    assert!(
        !err.contains("Seite 0;"),
        "{name}: Überlauf auf Seite 0:\n{err}"
    );
}

/// `Region.page`: genau an der Grenze, über alle drei Dateiwege.
#[test]
fn the_page_limit_is_inclusive_on_every_file_path() {
    let dir = workdir("grenze");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());
    let original = review_for(&dir, &input);

    for (page, ok) in [(MAX, true), (MAX + 1, false), (u64::MAX, false)] {
        // Weg 1: --apply-review
        let mut v = original.clone();
        v["items"][0]["region"]["page"] = serde_json::json!(page);
        let datei = write(
            &dir,
            &format!("apply-{page}.json"),
            v.to_string().as_bytes(),
        );
        let ausgabe = dir.join(format!("apply-{page}.pdf"));
        let out = run(&[
            input.to_str().unwrap(),
            "-o",
            ausgabe.to_str().unwrap(),
            "--apply-review",
            datei.to_str().unwrap(),
        ]);
        // Weg 2: --manual-regions mit derselben Review-Datei
        let ausgabe2 = dir.join(format!("manual-review-{page}.pdf"));
        let out2 = run(&[
            input.to_str().unwrap(),
            "-o",
            ausgabe2.to_str().unwrap(),
            "--no-patterns",
            "--manual-regions",
            datei.to_str().unwrap(),
        ]);
        // Weg 3: --manual-regions mit nacktem Array
        let list = serde_json::json!([{
            "page": page,
            "rect": original["items"][0]["region"]["rect"],
            "source": { "manual": { "reason": "g3" } }
        }]);
        let datei3 = write(
            &dir,
            &format!("list-{page}.json"),
            list.to_string().as_bytes(),
        );
        let ausgabe3 = dir.join(format!("manual-list-{page}.pdf"));
        let out3 = run(&[
            input.to_str().unwrap(),
            "-o",
            ausgabe3.to_str().unwrap(),
            "--no-patterns",
            "--manual-regions",
            datei3.to_str().unwrap(),
        ]);
        for (name, out, ausgabe) in [
            ("apply-review", &out, &ausgabe),
            ("manual-regions/review", &out2, &ausgabe2),
            ("manual-regions/list", &out3, &ausgabe3),
        ] {
            assert_ne!(out.status.code(), Some(101), "{name} {page}: Panic");
            if ok {
                assert_accepted(name, out, ausgabe, "4294967296");
            } else {
                assert_refused(name, out, ausgabe, page);
            }
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// `BlockedRegion.page` in `blocked_by_negative_list`: dieselbe Grenze, und
/// der Höchstwert steht sättigend gerechnet in `blocked_details`.
#[test]
fn blocked_regions_share_the_page_limit() {
    let dir = workdir("blockiert");
    let input = write(&dir, "auszug.pdf", &redact_pdf::testing::demo_statement());
    let original = review_for(&dir, &input);

    let mut v = original.clone();
    v["blocked_by_negative_list"] = serde_json::json!([blocked(MAX + 1)]);
    let datei = write(&dir, "blocked-drueber.json", v.to_string().as_bytes());
    let ausgabe = dir.join("blocked-drueber.pdf");
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        ausgabe.to_str().unwrap(),
        "--apply-review",
        datei.to_str().unwrap(),
    ]);
    assert_refused("blocked", &out, &ausgabe, MAX + 1);

    let mut v = original.clone();
    v["blocked_by_negative_list"] = serde_json::json!([blocked(MAX)]);
    let datei = write(&dir, "blocked-max.json", v.to_string().as_bytes());
    let ausgabe = dir.join("blocked-max.pdf");
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        ausgabe.to_str().unwrap(),
        "--apply-review",
        datei.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(ausgabe.exists());
    let summary: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let details = summary["blocked_details"].to_string();
    assert!(details.contains("Seite 4294967296:"), "{details}");
    std::fs::remove_dir_all(&dir).ok();
}
