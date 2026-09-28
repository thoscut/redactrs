//! CI-Befund nach der Fix-Runde 9, Register #60: **die Kennung eines Exports
//! auf einem Dateisystem ohne Groß-/Kleinschreibung.**
//!
//! Der Windows-Job der CI (Lauf 35621733310, „Build (windows-2025)“) hat mit
//! `zm_b_verschiedene_dateien_verschiedene_kennungen` einen Programmfehler
//! gefunden: `Auszug.pdf` und `auszug.pdf` sind auf NTFS **eine** Datei, und
//! [`writing_key`] gab zwei Kennungen — zwei gleichzeitige Exporte hätten
//! dieselbe Datei geschrieben, und keiner hätte `EXPORT_BUSY` gemeldet. Keine
//! Regression der Runde 9: der Stand `0f0b0f7` kanonisierte genauso nur das
//! Verzeichnis. Freigelegt hat es der neue Test, und auf Linux (zwei Dateien,
//! zwei Kennungen) blieb er grün — das lokale Gate kann Windows nicht
//! ausführen (`zf_q5_plattformzusagen`).
//!
//! Die Korrektur misst das **Verzeichnis**, statt die Plattform zu raten
//! ([`gemessene_schreibweise`]); die Faltung selbst ([`gefalteter_name`]) ist
//! rein und nimmt die Antwort als Parameter. Dadurch ist die Windows-Hälfte
//! der Faltung auf Linux prüfbar, und die Messung wird gegen dasselbe Orakel
//! geprüft wie im CI-Test: zwei Schreibweisen schreiben, beide zurücklesen —
//! was die Platte sagt, muss die Messung sagen. Auf dem Windows-Läufer der CI
//! prüft [`die_messung_sagt_was_die_platte_sagt`] damit den **positiven**
//! Zweig (`Faltet`), auf Linux den negativen; die Fälle, in denen die Messung
//! nicht antworten kann, stehen einzeln.
//!
//! Mutationsnachweise: `gefalteter_name` den Namen unverändert zurückgeben
//! lassen → [`faltet_zwei_schreibweisen_auf_eine_kennung`] rot.
//! `gemessene_schreibweise` immer `None` → auf Windows
//! [`die_messung_sagt_was_die_platte_sagt`] rot; auf Linux deckt die Vorgabe
//! den Fall — deshalb steht der Windows-Läufer der CI als der einzige, der
//! diese Mutation sieht. Die Vorfahren-Suche entfernen →
//! [`ein_fehlender_zielordner_antwortet_wie_sein_vorfahr`] rot.

use super::{
    gefalteter_name, gekippte_schreibweise, gemessene_schreibweise, writing_key, Schreibweise,
};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zn-b-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("Verzeichnis");
    dir
}

fn eintraege(dir: &Path) -> Vec<OsString> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .expect("lesbar")
        .map(|e| e.expect("Eintrag").file_name())
        .collect();
    v.sort();
    v
}

/// Das Orakel des CI-Tests: zwei Schreibweisen schreiben, beide zurücklesen.
/// Sagt die Platte „eine Datei“, ist es `Faltet`.
fn was_die_platte_sagt(dir: &Path) -> Schreibweise {
    let gross = dir.join("Orakel.txt");
    let klein = dir.join("orakel.txt");
    std::fs::write(&gross, b"GROSS").unwrap();
    std::fs::write(&klein, b"klein").unwrap();
    if std::fs::read(&gross).unwrap() == std::fs::read(&klein).unwrap() {
        Schreibweise::Faltet
    } else {
        Schreibweise::Unterscheidet
    }
}

#[test]
fn faltet_zwei_schreibweisen_auf_eine_kennung() {
    let basis = Path::new("irgendwo");
    let (a, b) = (OsStr::new("Auszug.pdf"), OsStr::new("auszug.pdf"));
    assert_eq!(
        basis.join(gefalteter_name(a, Schreibweise::Faltet)),
        basis.join(gefalteter_name(b, Schreibweise::Faltet)),
        "faltend: eine Kennung"
    );
    assert_ne!(
        basis.join(gefalteter_name(a, Schreibweise::Unterscheidet)),
        basis.join(gefalteter_name(b, Schreibweise::Unterscheidet)),
        "unterscheidend: zwei Kennungen"
    );
    assert_eq!(
        gefalteter_name(a, Schreibweise::Unterscheidet),
        a,
        "unterscheidend bleibt der Name, wie er ist"
    );
}

#[test]
fn faltet_nicht_ueber_ordner_hinweg() {
    // Gleicher Name, zwei Ordner: zwei Kennungen, auch faltend — sonst lehnte
    // der zweite Export mit EXPORT_BUSY ab, und die Handlung wäre verloren.
    let name = gefalteter_name(OsStr::new("a.pdf"), Schreibweise::Faltet);
    assert_ne!(Path::new("x").join(&name), Path::new("y").join(&name));
}

#[test]
fn die_probe_kippt_den_ersten_ascii_buchstaben() {
    let kipp = |n: &str| gekippte_schreibweise(OsStr::new(n));
    assert_eq!(kipp("Auszug.pdf").unwrap(), "auszug.pdf");
    assert_eq!(kipp("auszug.pdf").unwrap(), "Auszug.pdf");
    assert_eq!(kipp("2026-x").unwrap(), "2026-X");
    assert_eq!(
        kipp("ßtraße").unwrap(),
        "ßTraße",
        "ß bleibt stehen — großgeschrieben wäre es SS, und die Probe hätte eine andere Länge"
    );
    assert_eq!(kipp("2026-09-22"), None);
    assert_eq!(kipp("ÄÖÜ"), None);
}

#[test]
fn die_messung_sagt_was_die_platte_sagt() {
    let dir = tmp("platte");
    std::fs::write(dir.join("Probe.txt"), b"x").unwrap();
    let vorher = eintraege(&dir);
    let gemessen = gemessene_schreibweise(&dir);
    assert_eq!(eintraege(&dir), vorher, "die Messung legt nichts an");
    assert_eq!(gemessen, Some(was_die_platte_sagt(&dir)));
}

#[test]
fn writing_key_faltet_dort_wo_die_platte_faltet() {
    let dir = tmp("kennung");
    std::fs::write(dir.join("Probe.txt"), b"x").unwrap();
    let gleich = writing_key(&dir.join("Auszug.pdf")) == writing_key(&dir.join("auszug.pdf"));
    assert_eq!(
        gleich,
        was_die_platte_sagt(&dir) == Schreibweise::Faltet,
        "eine Kennung genau dann, wenn die Platte eine Datei daraus macht"
    );
}

#[test]
fn eine_zweite_datei_unter_gekippter_schreibweise_heisst_unterscheidet() {
    // Auf einer Platte mit Groß/Klein liegen hier zwei Dateien: die Probe
    // findet unter dem gekippten Namen eine *andere* Datei → Unterscheidet.
    // Auf einer Platte ohne ist es eine → Faltet. Beides sagt das Orakel.
    let dir = tmp("zwei");
    std::fs::write(dir.join("Probe.txt"), b"a").unwrap();
    std::fs::write(dir.join("probe.txt"), b"b").unwrap();
    assert_eq!(
        gemessene_schreibweise(&dir),
        Some(was_die_platte_sagt(&dir))
    );
}

#[test]
fn ein_leeres_verzeichnis_gibt_keine_antwort() {
    let dir = tmp("leer");
    assert_eq!(gemessene_schreibweise(&dir), None);
}

#[test]
fn ohne_ascii_buchstaben_keine_antwort() {
    let dir = tmp("ziffern");
    for name in ["2026-09-22", "12345", "ÄÖÜ"] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    assert_eq!(gemessene_schreibweise(&dir), None);
}

#[test]
fn ein_fehlender_zielordner_antwortet_wie_sein_vorfahr() {
    let dir = tmp("vorfahr");
    std::fs::write(dir.join("Probe.txt"), b"x").unwrap();
    let fehlt = dir.join("neu").join("tiefer");
    assert!(!fehlt.exists());
    let vom_vorfahren = gemessene_schreibweise(&dir);
    assert!(
        vom_vorfahren.is_some(),
        "der Vorfahr hat eine Antwort — sonst prüfte dieser Test None gegen None"
    );
    assert_eq!(gemessene_schreibweise(&fehlt), vom_vorfahren);
    assert!(!fehlt.exists(), "und die Messung legt ihn nicht an");
}

#[cfg(unix)]
#[test]
fn ein_symlink_wird_uebergangen() {
    let dir = tmp("link");
    std::os::unix::fs::symlink("nirgendwo", dir.join("Link")).unwrap();
    assert_eq!(gemessene_schreibweise(&dir), None);
}

#[cfg(unix)]
#[test]
fn ohne_leserecht_keine_antwort() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp("rechte");
    std::fs::write(dir.join("Probe.txt"), b"x").unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o100)).unwrap();
    let lesbar = std::fs::read_dir(&dir).is_ok();
    let antwort = gemessene_schreibweise(&dir);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    if lesbar {
        eprintln!("übergangen: dieser Prozess liest das Verzeichnis trotz --x (root)");
        return;
    }
    assert_eq!(antwort, None);
}
