//! Prüfrunde Oberfläche (0.8.1): was beim Öffnen eines **zweiten** Dokuments
//! vom ersten übrig blieb.
//!
//! Vorher verwarf `AppState::load_bytes` beim Wechsel nur `-o`. Gemessen:
//!
//! * `--audit-log`: der Export des zweiten Dokuments **überschrieb das
//!   Audit-Log des ersten** (`force: true`) — der Nachweis über die
//!   Schwärzung von A war weg;
//! * `--review-out`: der Review-Dialog schlug den Dateinamen von A vor;
//! * `--manual-regions` als Regionsliste: Rechtecke für A landeten in B;
//! * `--manual-regions` als Review-Datei von A: **jede** Analyse von B
//!   scheiterte, B war nicht mehr automatisch zu durchsuchen;
//! * das Passwort von A öffnete ein verschlüsseltes B ohne Rückfrage.
//!
//! `cargo test -p redact-gui --test zq_gui_dokumentwechsel`

use std::path::{Path, PathBuf};

use redact_core::{Rect, Region, Source};
use redact_gui::state::{options_dropped_notice, AnnotatedRegion, AppState};
use redact_gui::Config;
use redact_pdf::testing::{build_pdf, TextItem};
use redact_pipeline::testing::{ENCRYPTED_PDF, ENCRYPTED_PDF_PASSWORD};

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zq-gui-wechsel-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pdf(text: &str) -> Vec<u8> {
    build_pdf(&[vec![TextItem::new(72.0, 700.0, 10.0, text)]])
}

/// Zwei Dokumente `a.pdf` und `b.pdf` mit verschiedenem Inhalt.
fn zwei(dir: &Path) -> (PathBuf, PathBuf) {
    let a = dir.join("a.pdf");
    let b = dir.join("b.pdf");
    std::fs::write(&a, pdf("Zeile A EINS")).unwrap();
    std::fs::write(&b, pdf("Zeile B ZWEI")).unwrap();
    (a, b)
}

fn rechteck() -> AnnotatedRegion {
    AnnotatedRegion::new(Region::new(
        0,
        Rect::new(60.0, 690.0, 300.0, 715.0),
        None,
        Source::Manual {
            reason: "zq".into(),
        },
    ))
}

#[test]
fn zq_das_audit_log_des_ersten_dokuments_wird_nicht_ueberschrieben() {
    let dir = tmp("audit");
    let (a, b) = zwei(&dir);
    let audit_a = dir.join("a_audit.json");
    let review_a = dir.join("a_review.json");
    let mut state = AppState::with_config(Config {
        input: a.clone(),
        audit_log: Some(audit_a.clone()),
        review_out: Some(review_a.clone()),
        ..Config::default()
    });
    state.load_document(&a).unwrap();
    // Für das Startdokument gelten die Schalter.
    let out_a = state.suggested_output_path().unwrap();
    assert_eq!(state.audit_target(&out_a), audit_a);
    assert_eq!(state.suggested_review_path().unwrap(), review_a);
    state.regions.push(rechteck());
    state
        .export(&out_a, Some(&state.audit_target(&out_a)))
        .expect("Export A");
    let sha_a = state.input_sha256.clone();
    let log_a = std::fs::read_to_string(&audit_a).unwrap();
    assert!(log_a.contains(&sha_a));

    state.load_document(&b).unwrap();
    let out_b = state.suggested_output_path().unwrap();
    assert_ne!(state.audit_target(&out_b), audit_a, "Audit-Log von A für B");
    assert_eq!(state.audit_target(&out_b), AppState::audit_path_for(&out_b));
    assert_eq!(
        state.suggested_review_path().unwrap(),
        dir.join("b_review.json"),
        "Review-Vorschlag von A für B"
    );
    // Und es wird gesagt.
    let notice = options_dropped_notice(&["--audit-log", "--review-out"]);
    assert!(state.warnings.contains(&notice), "{:?}", state.warnings);
    assert!(
        !state.extract_warnings.contains(&notice),
        "keine Abdeckungslücke"
    );

    state.regions.push(rechteck());
    state
        .export(&out_b, Some(&state.audit_target(&out_b)))
        .expect("Export B");
    assert_eq!(
        std::fs::read_to_string(&audit_a).unwrap(),
        log_a,
        "das Audit-Log von A wurde überschrieben"
    );
}

#[test]
fn zq_die_regionsliste_des_ersten_dokuments_gilt_nicht_im_zweiten() {
    let dir = tmp("liste");
    let (a, b) = zwei(&dir);
    let list = dir.join("a_regionen.json");
    let region = Region::new(
        0,
        Rect::new(10.0, 10.0, 50.0, 30.0),
        None,
        Source::Manual {
            reason: "für A".into(),
        },
    );
    std::fs::write(&list, serde_json::to_string(&vec![region]).unwrap()).unwrap();
    let mut state = AppState::with_config(Config {
        input: a.clone(),
        manual_regions: Some(list),
        no_patterns: true,
        ..Config::default()
    });
    state.load_document(&a).unwrap();
    state.analyze().unwrap();
    assert_eq!(state.regions.len(), 1, "für A gilt die Liste");

    state.load_document(&b).unwrap();
    state.analyze().unwrap();
    assert!(state.regions.is_empty(), "Rechteck von A in B");
    assert!(state
        .warnings
        .contains(&options_dropped_notice(&["--manual-regions"])));
}

#[test]
fn zq_eine_review_des_ersten_dokuments_sperrt_die_analyse_des_zweiten_nicht() {
    let dir = tmp("review");
    let (a, b) = zwei(&dir);
    let mut first = AppState::with_config(Config::default());
    first.load_document(&a).unwrap();
    first.regions.push(rechteck());
    let review = dir.join("a_review.json");
    first.save_review_file(&review).unwrap();

    let mut state = AppState::with_config(Config {
        input: a.clone(),
        manual_regions: Some(review.clone()),
        apply_review: Some(review),
        ..Config::default()
    });
    state.load_document(&a).unwrap();
    state.analyze().expect("A passt zu seiner Review");
    state.load_document(&b).unwrap();
    state
        .analyze()
        .expect("B muss analysierbar sein, die Review gehört zu A");
    assert!(state.config.apply_review.is_none());
}

#[test]
fn zq_dasselbe_dokument_erneut_behaelt_die_schalter() {
    let dir = tmp("dasselbe");
    let (a, _) = zwei(&dir);
    let audit = dir.join("log.json");
    let mut state = AppState::with_config(Config {
        input: a.clone(),
        audit_log: Some(audit.clone()),
        ..Config::default()
    });
    state.load_document(&a).unwrap();
    state.load_document(&a).unwrap();
    assert_eq!(state.config.audit_log, Some(audit));
    assert!(state.warnings.is_empty(), "{:?}", state.warnings);
}

#[test]
fn zq_das_passwort_des_ersten_dokuments_oeffnet_kein_zweites() {
    let dir = tmp("passwort");
    let first = dir.join("auszug.pdf");
    let second = dir.join("auszug-kopie.pdf");
    std::fs::write(&first, ENCRYPTED_PDF).unwrap();
    std::fs::write(&second, ENCRYPTED_PDF).unwrap();

    let mut state = AppState::with_config(Config::default());
    let _ = state.load_document(&first);
    assert!(state.needs_password());
    state
        .unlock(ENCRYPTED_PDF_PASSWORD)
        .expect("richtiges Passwort");
    assert!(state.is_loaded());

    // Dasselbe Dokument erneut: das Passwort gilt ihm weiter.
    state.load_document(&first).expect("dasselbe Dokument");

    // Ein anderes Dokument fragt selbst.
    let error = state
        .load_document(&second)
        .expect_err("ohne Rückfrage mit dem Passwort des vorigen geöffnet");
    assert!(error.to_string().contains("verschlüsselt"), "{error}");
    assert!(state.needs_password());
    assert!(state.config.password.is_none());
    // Das erste bleibt geladen, bis das zweite wirklich offen ist.
    assert_eq!(state.pdf_path.as_deref(), Some(first.as_path()));
    state
        .unlock(ENCRYPTED_PDF_PASSWORD)
        .expect("mit eigenem Passwort");
    assert_eq!(state.pdf_path.as_deref(), Some(second.as_path()));
}
