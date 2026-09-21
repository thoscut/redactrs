//! Gegenprüfung D der Runde 9 zum CHANGELOG-Abschnitt **Fix-Runde 8**, Satz:
//!
//! > Die Kennung eines Exports ist dabei nicht mehr der Pfad, sondern das
//! > aufgelöste Verzeichnis samt unverändertem Dateinamen (`writing_key`) —
//! > sie folgt keinem Symlink, und zwei Schreibwege auf dieselbe Datei fallen
//! > zusammen.
//!
//! `writing_key` ist privat; hier steht sie **wörtlich nachgebaut** (wie in
//! `zj_c_fahne_beide_richtungen`) und wird an die Identität gebunden, mit der
//! wirklich geschrieben wird: [`redact_pdf::document::check_target`].
//!
//! Befund dieser Datei: der Satz gilt **für ein vorhandenes Verzeichnis**.
//! Fehlt das Verzeichnis beim Klick — `check_target` legt es erst im
//! Export-Faden an —, dann fallen zwei Schreibwege auf dieselbe Datei
//! **nicht** zusammen; die Sperre gegen zwei gleichzeitige Schreiber derselben
//! Datei greift dort nicht. Der Baum weiß das (`zj_c` hält es als Restgrenze
//! fest), der CHANGELOG-Satz sagt es nicht.
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-gui --test zm_d_kennung_des_exports`

use std::path::{Path, PathBuf};

use redact_pdf::document::{check_target, WriteOptions};

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zm-d-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Wörtlich `app::writing_key` (privat, darum hier nachgebaut) — Stand
/// `af55c66`.
fn wie_writing_key(p: &Path) -> PathBuf {
    let dir = p.parent().filter(|d| !d.as_os_str().is_empty());
    match (dir, p.file_name()) {
        (Some(dir), Some(name)) => std::fs::canonicalize(dir)
            .unwrap_or_else(|_| dir.to_path_buf())
            .join(name),
        (None, Some(name)) => std::fs::canonicalize(".")
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(name),
        _ => p.to_path_buf(),
    }
}

// ===========================================================================
// 1 — Der Satz, wo er hält: vorhandenes Verzeichnis
// ===========================================================================

/// **Hält.** Drei Schreibweisen derselben Datei in einem **vorhandenen**
/// Ordner ergeben dieselbe Kennung — und dieselbe, mit der geschrieben wird.
#[test]
fn drei_schreibweisen_fallen_zusammen() {
    let dir = tmp("schreibweisen");
    let schlicht = dir.join("out.pdf");
    let mit_punkt = dir.join(".").join("out.pdf");
    let ueber_den_vater = dir
        .join("..")
        .join(dir.file_name().unwrap())
        .join("out.pdf");

    let a = wie_writing_key(&schlicht);
    let b = wie_writing_key(&mit_punkt);
    let c = wie_writing_key(&ueber_den_vater);
    eprintln!("a={}\nb={}\nc={}", a.display(), b.display(), c.display());
    assert_eq!(
        a, b,
        "ein eingeschobenes Punktverzeichnis ändert die Datei nicht"
    );
    assert_eq!(a, c, "der Umweg über den Vaterordner ebenso wenig");

    let ziel = check_target(&schlicht, &WriteOptions::new().force(true)).expect("Ziel");
    assert_eq!(
        ziel.path(),
        a,
        "die Kennung ist die Identität, mit der geschrieben wird"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 2 — Der Symlink auf die **Datei**
// ===========================================================================

/// **Hält.** Ein Symlink auf die Ausgabedatei ist ein **eigenes** Ziel: die
/// Kennung folgt ihm nicht, und geschrieben wird auch nicht durch ihn hindurch
/// — `check_target` liefert den Linkpfad selbst, und der `rename` des
/// Schreibwegs ersetzt den Link, statt sein Ziel anzufassen.
///
/// Damit ist das Nicht-Verschmelzen hier **richtig**: `link.pdf` und
/// `echt.pdf` sind nach einem Schreiben zwei verschiedene Dateien.
#[test]
#[cfg(unix)]
fn ein_symlink_auf_die_datei_ist_ein_eigenes_ziel() {
    let dir = tmp("dateilink");
    let echt = dir.join("echt.pdf");
    std::fs::write(&echt, b"%PDF-1.5 alt\n").unwrap();
    let link = dir.join("link.pdf");
    std::os::unix::fs::symlink(&echt, &link).unwrap();

    let k_echt = wie_writing_key(&echt);
    let k_link = wie_writing_key(&link);
    eprintln!("echt: {}\nlink: {}", k_echt.display(), k_link.display());
    assert_ne!(k_echt, k_link, "die Kennung folgt dem Symlink nicht");

    // Und geschrieben wird durch den Link gar nicht: der Schreibweg lehnt ihn
    // ab. Die Kennung kann also nie das Urteil über das Linkziel treffen.
    let fehler = check_target(&link, &WriteOptions::new().force(true))
        .expect_err("ein Symlink als Ziel wird abgelehnt");
    eprintln!("check_target: {fehler}");
    assert!(
        format!("{fehler}").contains("schreibt nicht durch Links hindurch"),
        "{fehler}"
    );
    assert_eq!(
        std::fs::read(&echt).unwrap(),
        b"%PDF-1.5 alt\n",
        "das Ziel des Links ist unberührt"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 3 — Der Symlink auf das **Verzeichnis**: dem folgt sie sehr wohl
// ===========================================================================

/// **Halb.** „Sie folgt keinem Symlink" stimmt für den **Dateinamen**; das
/// **Verzeichnis** wird aufgelöst, und darin folgt sie jedem Symlink. Hier ist
/// das die richtige Antwort (beide Wege schreiben dieselbe Datei) — aber es
/// ist nicht das, was der Satz sagt.
#[test]
#[cfg(unix)]
fn dem_symlink_auf_das_verzeichnis_folgt_sie_sehr_wohl() {
    let basis = tmp("ordnerlink");
    let echt = basis.join("echt");
    std::fs::create_dir_all(&echt).unwrap();
    let link = basis.join("link");
    std::os::unix::fs::symlink(&echt, &link).unwrap();

    let ueber_link = wie_writing_key(&link.join("out.pdf"));
    let direkt = wie_writing_key(&echt.join("out.pdf"));
    eprintln!(
        "über den Link: {}\ndirekt:        {}",
        ueber_link.display(),
        direkt.display()
    );
    assert_eq!(
        ueber_link, direkt,
        "der Verzeichnis-Symlink wird aufgelöst — die Kennung folgt ihm"
    );
    std::fs::remove_dir_all(&basis).ok();
}

// ===========================================================================
// 4 — Wo der Satz bricht: das Verzeichnis gibt es beim Klick noch nicht
// ===========================================================================

/// **Bricht für den unbedingten Teil des Satzes.** Zwei Schreibwege auf
/// **dieselbe** Datei, beide in einen Ordner, den es beim Klick noch nicht
/// gibt: die Kennungen fallen nicht zusammen. Genau daran hängt die Sperre
/// gegen zwei gleichzeitige Exporte derselben Datei (`EXPORT_BUSY` vergleicht
/// `PendingExport::writing`).
///
/// Nachgestellt wird die Reihenfolge des Programms: `export_to` bildet die
/// Kennung **vor** `check_target`, und `check_target` legt den Ordner erst im
/// Export-Faden an.
#[test]
#[cfg(unix)]
fn ohne_vorhandenen_ordner_fallen_zwei_wege_nicht_zusammen() {
    let basis = tmp("ordner-fehlt");
    let echt = basis.join("echt");
    std::fs::create_dir_all(&echt).unwrap();
    let link = basis.join("link");
    std::os::unix::fs::symlink(&echt, &link).unwrap();

    // Zwei Schreibwege auf dieselbe Datei: über den Link und direkt.
    let weg_a = link.join("neu").join("out.pdf");
    let weg_b = echt.join("neu").join("out.pdf");
    assert!(!weg_a.parent().unwrap().exists(), "der Ordner fehlt noch");

    let klick_a = wie_writing_key(&weg_a);
    let klick_b = wie_writing_key(&weg_b);
    eprintln!("Klick A: {}", klick_a.display());
    eprintln!("Klick B: {}", klick_b.display());
    assert_ne!(
        klick_a, klick_b,
        "zwei Schreibwege auf dieselbe Datei, zwei Kennungen — \
         der zweite Export würde nicht abgelehnt"
    );

    // Und dass es wirklich dieselbe Datei ist, sagt der Schreibweg selbst.
    let ziel_a = check_target(&weg_a, &WriteOptions::new().force(true)).expect("Ziel A");
    let ziel_b = check_target(&weg_b, &WriteOptions::new().force(true)).expect("Ziel B");
    assert_eq!(
        ziel_a.path(),
        ziel_b.path(),
        "geschrieben wird beide Male dieselbe Datei"
    );

    // Derselbe Klick, nach dem Anlegen des Ordners: eine andere Kennung als
    // vorher — ein Klick, zwei Kennungen.
    assert_ne!(
        klick_a,
        wie_writing_key(&weg_a),
        "vor und nach dem Anlegen des Ordners"
    );
    std::fs::remove_dir_all(&basis).ok();
}
