//! Eine Review-Datei mit Mehrbyte-Zeichen in der Prüfsumme darf das Programm
//! nicht zum Absturz bringen.
//!
//! # Der Befund (gemessen am gebauten Binary, vor der Änderung)
//!
//! ```text
//! $ printf '{"version":1,"input":{"path":"demo.pdf","sha256":"aaaaaaaaaaaä","pages":1},"items":[]}' > evil_review.json
//! $ redact-rs demo.pdf --apply-review evil_review.json -o out.pdf
//! thread 'main' panicked at crates/redact-pipeline/src/lib.rs:1544:9:
//! byte index 12 is not a char boundary; it is inside 'ä' (bytes 11..13) of `aaaaaaaaaaaä`
//! $ echo $?
//! 101
//! ```
//!
//! Dasselbe über `--manual-regions`, das denselben Weg nimmt. `short_sha`
//! kürzte die Prüfsumme für die Meldung „gehört zu einem anderen Dokument“ per
//! **Byte**-Index auf zwölf; die Prüfsumme kommt aber aus der Review-Datei und
//! damit von außen. Elf `a` und ein `ä` (13 Byte) legen den Schnitt mitten in
//! das `ä`.
//!
//! Erwartet ist dagegen genau die Ablehnung, die jede fremde Prüfsumme
//! bekommt — und die Meldung nennt die ersten zwölf **Zeichen**.

use std::path::PathBuf;

use redact_core::review::{ReviewFile, ReviewInput};
use redact_pipeline::{check_review_identity, load_manual_regions};

/// Eine gewöhnliche Prüfsumme des Dokuments — 64 Hexzeichen.
const DOKUMENT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Elf `a`, dann Mehrbyte-Zeichen: Byte 12 liegt im ersten `ä`.
const FREMD: &str = "aaaaaaaaaaaää";

fn review_mit(sha: &str) -> ReviewFile {
    ReviewFile::new(
        ReviewInput {
            path: "demo.pdf".into(),
            sha256: sha.to_string(),
            pages: 1,
        },
        Vec::new(),
        Vec::new(),
    )
}

#[test]
fn eine_mehrbyte_pruefsumme_wird_abgelehnt_statt_zu_panieren() {
    for erlaubt in [false, true] {
        let fehler = check_review_identity(&review_mit(FREMD), DOKUMENT, erlaubt)
            .expect_err("fremde Prüfsumme muss abgelehnt werden")
            .to_string();
        assert!(fehler.contains("anderen Dokument"), "{fehler}");
        // Zwölf Zeichen, nicht zwölf Byte — das `ä` bleibt ganz.
        assert!(fehler.contains("aaaaaaaaaaaä…"), "{fehler}");
        assert!(!fehler.contains("aaaaaaaaaaaää"), "{fehler}");
    }
}

/// Die Umkehrung: eine Prüfsumme, die **kürzer** als zwölf Zeichen ist, wird
/// unverändert gezeigt, eine mit genau zwölf Mehrbyte-Zeichen ebenso.
#[test]
fn kurze_und_genau_zwoelfstellige_pruefsummen_bleiben_ganz() {
    for (sha, erwartet) in [("ä", "ä…"), ("ääääääääääää", "ääääääääääää…")]
    {
        let fehler = check_review_identity(&review_mit(sha), DOKUMENT, false)
            .expect_err("fremde Prüfsumme muss abgelehnt werden")
            .to_string();
        assert!(fehler.contains(erwartet), "{sha:?}: {fehler}");
    }
}

/// `--manual-regions` nimmt denselben Weg über die Datei.
#[test]
fn dieselbe_datei_hinter_manual_regions_wird_ebenso_abgelehnt() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("redactrs_zb_mehrbyte_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("Arbeitsverzeichnis anlegbar");
    let pfad = dir.join("evil_review.json");
    std::fs::write(&pfad, review_mit(FREMD).to_json().unwrap()).unwrap();

    let fehler = load_manual_regions(&pfad, DOKUMENT, false)
        .expect_err("fremde Prüfsumme muss auch hier abgelehnt werden")
        .to_string();
    assert!(fehler.contains("anderen Dokument"), "{fehler}");
    assert!(fehler.contains("aaaaaaaaaaaä…"), "{fehler}");

    let _ = std::fs::remove_dir_all(&dir);
}
