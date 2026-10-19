//! Runde 2, Verschlüsselung: was nach dem Entschlüsseln noch geprüft werden
//! muss — auf **jedem** Weg dorthin.
//!
//! Zwei Wege führen zu einem entschlüsselten Dokument:
//!
//! * `--password` über `Document::load_mem_with_options` (der Passwortzweig
//!   von `redact_pipeline::load_document`), und
//! * das **leere Benutzerpasswort**, das `lopdf` in `Document::load_mem` von
//!   sich aus probiert. So eine Datei — nur ein Eigentümerpasswort, der
//!   übliche „Rechteschutz“ — öffnet jeder Betrachter ohne Nachfrage, und
//!   `lopdf` entfernt danach `/Encrypt` aus dem Trailer.
//!
//! Bis Runde 2 liefen die Budgets nach der Entschlüsselung
//! (`check_limits_after_decryption`) nur auf dem ersten Weg. Auf dem zweiten
//! hatte die Vorprüfung nur RC4-Rauschen gesehen, und eine 131-kB-Datei trieb
//! den Lauf auf 5,4 GB und in den OOM-Killer. Seitdem liegt die Nacharbeit in
//! `redact_pdf::document::finish_decrypted`, und beide Wege rufen sie auf.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use lopdf::{Document, EncryptionState, EncryptionVersion, LoadOptions, Permissions};
use redact_pipeline::testing::{
    ENCRYPTED_BOMB_PDF, ENCRYPTED_NESTING_BOMB_PDF, ENCRYPTED_PDF, ENCRYPTED_PDF_IBAN,
    ENCRYPTED_PDF_PASSWORD,
};
use redact_pipeline::{load_document, Config, Secret};

/// Entschlüsselt eine der Testdateien und verschlüsselt sie neu — mit
/// **leerem Benutzerpasswort** und einem Eigentümerpasswort.
fn reencrypt_with_empty_user_password(bytes: &[u8]) -> Vec<u8> {
    let mut doc =
        Document::load_mem_with_options(bytes, LoadOptions::with_password(ENCRYPTED_PDF_PASSWORD))
            .expect("Testdatei lässt sich mit Passwort öffnen");
    let version = EncryptionVersion::V2 {
        document: &doc,
        owner_password: "eigentuemer",
        user_password: "",
        key_length: 128,
        permissions: Permissions::all(),
    };
    let state = EncryptionState::try_from(version).expect("Schlüssel ableitbar");
    doc.encrypt(&state).expect("verschlüsselbar");
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    assert!(
        out.windows(8).any(|w| w == b"/Encrypt"),
        "die Probe ist nicht verschlüsselt"
    );
    out
}

/// Hängt eine leere inkrementelle Revision an: eine Querverweistabelle ohne
/// Einträge, deren Trailer mit `/Prev` auf die alte Sektion zeigt.
fn with_incremental_revision(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let sx = text.rfind("startxref").expect("startxref");
    let prev: usize = text[sx + 9..]
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .expect("Offset hinter startxref");
    let trailer = text.rfind("trailer").expect("klassischer Trailer");
    let dict_start = trailer + text[trailer..].find("<<").expect("<<");
    let dict_end = text[..sx].rfind(">>").expect(">>");
    let dict = &text[dict_start + 2..dict_end];
    let mut out = bytes.to_vec();
    out.push(b'\n');
    let offset = out.len();
    out.extend_from_slice(
        format!("xref\n0 0\ntrailer\n<<{dict} /Prev {prev} >>\nstartxref\n{offset}\n%%EOF\n")
            .as_bytes(),
    );
    out
}

fn workdir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("r2-verschluesselung-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("Arbeitsverzeichnis");
    dir
}

/// Gegenprobe: eine gewöhnliche Datei mit leerem Benutzerpasswort öffnet
/// ohne `--password` — die Prüfung darf sie nicht ablehnen.
#[test]
fn leeres_benutzerpasswort_oeffnet_ohne_passwort() {
    let bytes = reencrypt_with_empty_user_password(ENCRYPTED_PDF);
    load_document(&bytes, &Config::default()).expect("lädt ohne Passwort");
}

/// Die Dekompressionsbombe mit leerem Benutzerpasswort wird abgelehnt —
/// dasselbe Budget wie mit `--password`.
#[test]
fn leeres_benutzerpasswort_dekompressionsbombe_wird_abgelehnt() {
    let bytes = reencrypt_with_empty_user_password(ENCRYPTED_BOMB_PDF);
    let err = load_document(&bytes, &Config::default())
        .expect_err("Bombe mit leerem Benutzerpasswort ohne Budgetprüfung geladen");
    assert!(
        err.to_string().contains("entschlüsselt gilt weiter"),
        "die Meldung sagt nicht, welcher Durchgang angeschlagen hat: {err}"
    );
}

/// Ebenso die Verschachtelungsbombe.
#[test]
fn leeres_benutzerpasswort_verschachtelungsbombe_wird_abgelehnt() {
    let bytes = reencrypt_with_empty_user_password(ENCRYPTED_NESTING_BOMB_PDF);
    assert!(
        load_document(&bytes, &Config::default()).is_err(),
        "Verschachtelungsbombe mit leerem Benutzerpasswort geladen"
    );
}

/// `--check-leaks` lädt über `load_from_bytes_with_limits` direkt, nicht über
/// die Pipeline — und bekommt dieselbe Prüfung.
#[test]
fn leeres_benutzerpasswort_bombe_auch_fuer_die_lecksuche_abgelehnt() {
    let bytes = reencrypt_with_empty_user_password(ENCRYPTED_BOMB_PDF);
    assert!(
        redact_pdf::document::load_from_bytes(&bytes).is_err(),
        "der Lader selbst prüft die entschlüsselte Bombe nicht"
    );
}

/// Ende zu Ende am Binary: die Verschachtelungsbombe mit leerem
/// Benutzerpasswort endet nicht mit Rückgabewert 0, und es entsteht keine
/// Ausgabe mit der IBAN.
#[test]
fn leeres_benutzerpasswort_verschachtelungsbombe_am_binary() {
    let dir = workdir("verschachtelung");
    let input = dir.join("ein.pdf");
    let output = dir.join("aus.pdf");
    std::fs::write(
        &input,
        reencrypt_with_empty_user_password(ENCRYPTED_NESTING_BOMB_PDF),
    )
    .expect("Eingabe schreibbar");
    let out = Command::new(env!("CARGO_BIN_EXE_redact-rs"))
        .args([
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--patterns",
            "iban_de",
        ])
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .expect("Binary startet");
    let leaked = std::fs::read(&output)
        .map(|pdf| String::from_utf8_lossy(&pdf).contains(ENCRYPTED_PDF_IBAN))
        .unwrap_or(false);
    let _ = std::fs::remove_dir_all(&dir);
    assert_ne!(
        out.status.code(),
        Some(0),
        "Rückgabewert 0 für die Bombe\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!leaked, "IBAN in der Ausgabe");
}

/// Die Warnung „mehrere Revisionen“ überlebt auch den Passwortzweig: dort
/// fehlte bis Runde 2 der Aufruf von `restore_revision_markers`.
#[test]
fn vorgeschichte_ueberlebt_den_passwortzweig() {
    // Gegenprobe: mit leerem Benutzerpasswort kommt die Marke an.
    let empty = with_incremental_revision(&reencrypt_with_empty_user_password(ENCRYPTED_PDF));
    let doc = load_document(&empty, &Config::default()).expect("lädt ohne Passwort");
    assert!(
        redact_pdf::document::has_incremental_history(&doc),
        "Gegenprobe: leeres Benutzerpasswort ohne Vorgeschichte"
    );

    let bytes = with_incremental_revision(ENCRYPTED_PDF);
    let config = Config {
        password: Some(Secret::new(ENCRYPTED_PDF_PASSWORD)),
        ..Config::default()
    };
    let doc = load_document(&bytes, &config).expect("lädt mit Passwort");
    assert!(
        redact_pdf::document::has_incremental_history(&doc),
        "Vorgeschichte im Passwortzweig verloren"
    );
}
