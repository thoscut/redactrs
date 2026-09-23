//! Gegenprüfung Fix-Runde 8, Linse **Scheintest** — Agent „zi_c".
//!
//! Festgehalten wird **eine** Tatsache über `std::path`, an der ein Teil der
//! neuen Prüfung von Register #19 hängt.
//!
//! `RedactApp::export_to` lehnt einen zweiten Export **derselben** Datei ab,
//! solange der erste schreibt. Welche Datei „dieselbe" ist, entscheidet
//! `app::writing_key`: es löst das **Verzeichnis** auf (`canonicalize`) und
//! hängt den Dateinamen unverändert daran — denn die Datei selbst gibt es beim
//! ersten Export noch nicht, und `file_key` könnte sie darum nicht auflösen.
//!
//! Der Test dazu (`zh_c_ein_zweiter_export_derselben_datei_wird_abgelehnt`)
//! prüft die Auflösung an der Schreibweise `dir/./out.pdf`. Das kann nicht
//! fehlschlagen: `Path` vergleicht **Komponenten**, und `Component::CurDir`
//! fällt dabei weg — `dir/./out.pdf` und `dir/out.pdf` sind für `==` schon
//! ohne jedes `canonicalize` gleich. Belegt ist das unten in
//! [`ein_punkt_ist_fuer_path_schon_gleich`]; der Lauf, der zeigt, dass der
//! Test deshalb ohne seine Korrektur grün bleibt, steht im Bericht (Mutation
//! B12: `writing_key` → `path.to_path_buf()`, ganze GUI-Sammlung 375 grün).
//!
//! `..` ist der Fall, der die Auflösung wirklich verlangt: `Component::ParentDir`
//! bleibt in der Komponentenfolge stehen, also sind `dir/unten/../out.pdf` und
//! `dir/out.pdf` für `==` **verschieden**, solange niemand auflöst. Genau diese
//! Schreibweise prüft das Projekt bei `file_key` schon
//! (`zg_r4_2_ein_punkt_punkt_im_pfad_beendet_die_alte_pruefung`) — bei
//! `writing_key` fehlt sie.
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-gui --test zi_c_scheintest_schreibweise`

use std::path::{Component, Path, PathBuf};

/// Ein `.` mitten im Pfad ist für `Path::==` nicht zu sehen.
///
/// Damit ist jede Zusicherung „diese Schreibweise zeigt auf dieselbe Datei,
/// und die Auflösung erkennt das" an einem `.` keine Aussage über die
/// Auflösung.
#[test]
fn ein_punkt_ist_fuer_path_schon_gleich() {
    let dir = PathBuf::from("/tmp/zi-c-beispiel");
    let gerade = dir.join("out.pdf");
    let mit_punkt = dir.join(".").join("out.pdf");

    // Die Zeichen sind verschieden …
    assert_ne!(
        gerade.as_os_str(),
        mit_punkt.as_os_str(),
        "die Schreibweisen sollen sich unterscheiden"
    );
    // … der Vergleich ist es nicht.
    assert_eq!(
        gerade, mit_punkt,
        "Path vergleicht Komponenten, und CurDir fällt weg"
    );
    assert!(
        !mit_punkt.components().any(|c| c == Component::CurDir),
        "{:?}",
        mit_punkt.components().collect::<Vec<_>>()
    );
}

/// `..` bleibt dagegen stehen — das ist die Schreibweise, die eine Auflösung
/// verlangt und sie deshalb auch prüfen kann.
#[test]
fn zwei_punkte_bleiben_fuer_path_verschieden() {
    let dir = PathBuf::from("/tmp/zi-c-beispiel");
    let gerade = dir.join("out.pdf");
    let mit_umweg = dir.join("unten").join("..").join("out.pdf");

    assert_ne!(
        gerade, mit_umweg,
        "ParentDir bleibt in der Komponentenfolge stehen"
    );
    assert!(mit_umweg.components().any(|c| c == Component::ParentDir));
}

/// Und die Gegenprobe am Dateisystem: aufgelöst zeigen beide Schreibweisen auf
/// dieselbe Datei. Ohne diesen Teil wäre die Aussage oben nur eine über
/// Zeichenketten.
#[test]
fn aufgeloest_zeigen_beide_auf_dieselbe_datei() {
    let dir = std::env::temp_dir().join(format!("zi-c-schreibweise-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("unten")).expect("Unterverzeichnis");
    std::fs::write(dir.join("out.pdf"), b"x").expect("Datei");

    let gerade = std::fs::canonicalize(dir.join("out.pdf")).expect("gerade");
    let umweg = std::fs::canonicalize(dir.join("unten").join("..").join("out.pdf")).expect("Umweg");
    assert_eq!(gerade, umweg);

    // So, wie `writing_key` es zum Stand dieser Prüfung tat: nur das
    // Verzeichnis auflösen, den Namen unverändert daran. Das reicht für `..`
    // — und wird gebraucht. Seit Register #60 faltet das Original den Namen,
    // wo das Verzeichnis Groß/Klein nicht unterscheidet; für gleich
    // geschriebene Namen wie hier ohne Unterschied. Das Original ist öffentlich
    // (`redact_gui::app::writing_key`), wer den Satz neu belegt, ruft es.
    let wie_writing_key = |p: &Path| {
        let ordner = p.parent().expect("Ordner");
        std::fs::canonicalize(ordner)
            .unwrap_or_else(|_| ordner.to_path_buf())
            .join(p.file_name().expect("Name"))
    };
    let nicht_vorhanden = dir.join("gibt-es-nicht.pdf");
    let umweg_dazu = dir.join("unten").join("..").join("gibt-es-nicht.pdf");
    assert!(
        std::fs::canonicalize(&nicht_vorhanden).is_err(),
        "die Datei darf es nicht geben — darum kann file_key sie nicht auflösen"
    );
    assert_eq!(
        wie_writing_key(&nicht_vorhanden),
        wie_writing_key(&umweg_dazu),
        "die Auflösung des Verzeichnisses trägt auch vor dem ersten Byte"
    );

    std::fs::remove_dir_all(&dir).ok();
}
