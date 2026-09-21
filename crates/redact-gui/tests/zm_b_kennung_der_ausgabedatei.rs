//! Gegenprüfung der Fix-Runde 8, Gebiet **Oberfläche** — Gegenprüfer B,
//! Runde 9. Geprüft wird die **eine Kennung einer Ausgabedatei**,
//! `app::writing_key`, gegen das, was wirklich geschrieben wird.
//!
//! Die Zusage der Runde 8 lautet: „aufgelöstes Verzeichnis + unveränderter
//! Dateiname, folgt keinem Symlink". Daran hängen drei Entscheidungen:
//!
//! * darf ein zweiter Export laufen (`EXPORT_BUSY`)?
//! * wird das Urteil der Nachprüfung über die **alten** Bytes weggeworfen
//!   (`poll_exports` an `PendingExport::wrote`)?
//! * zu welcher Datei gehört eine Warnung (`WarnedFile`)?
//!
//! Jede Frage ist dieselbe: **sind zwei Pfade dieselbe Datei?** Der Maßstab
//! dafür ist hier nicht `writing_key` selbst, sondern
//!
//! * `redact_pdf::document::check_target` — die Stelle, die das echte
//!   Schreibziel festlegt (öffentlich, also prüfbar), und
//! * die geschriebenen **Bytes** samt Inode.
//!
//! `writing_key` ist privat und steht deshalb hier **wörtlich nachgebaut**
//! (`wie_writing_key`); jede Zusicherung stellt den Nachbau neben
//! `check_target`, damit ein Auseinanderlaufen auffällt und nicht bloß
//! behauptet wird.
//!
//! Zwei Richtungen zählen gleich:
//!
//! * **Lücke** — `check_target` schreibt beide Pfade auf dieselbe Datei,
//!   `writing_key` hält sie auseinander: zwei Threads schreiben dasselbe Ziel,
//!   und ein Urteil über die alten Bytes bleibt stehen.
//! * **Fehlalarm** — `writing_key` verschmilzt zwei Pfade, die verschiedene
//!   Dateien sind: ein gewöhnlicher Export wird mit `EXPORT_BUSY` abgelehnt,
//!   und die Handlung ist verloren.
//!
//! ```text
//! flock /tmp/redactrs-cargo.lock cargo test -p redact-gui --test zm_b_kennung_der_ausgabedatei -- --nocapture
//! ```

use std::path::{Path, PathBuf};

use redact_pdf::document::{check_target, WriteOptions};

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zm-b-kennung-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// **Wörtlich** `crate::app::writing_key` (privat, darum hier nachgebaut) —
/// samt der Korrektur der Gegenprüfung 9.
///
/// Eine Kopie ist eine Schuld: sie prüft sich selbst, sobald das Original sich
/// bewegt. Genau das ist hier passiert — dieser Nachbau blieb rot, nachdem das
/// Original schon stimmte, und hätte ebenso gut grün bleiben können, während
/// das Original falsch ist. Deshalb steht daneben
/// [`der_nachbau_haengt_am_quelltext`]: der Test liest `app.rs` und verlangt
/// die Stellen, die diesen Nachbau tragen.
fn wie_writing_key(p: &Path) -> PathBuf {
    let dir = p.parent().filter(|d| !d.as_os_str().is_empty());
    match (dir, p.file_name()) {
        (Some(dir), Some(name)) => wie_resolved_dir(dir).join(name),
        (None, Some(name)) => wie_resolved_dir(Path::new(".")).join(name),
        _ => p.to_path_buf(),
    }
}

/// **Wörtlich** `crate::app::resolved_dir`.
fn wie_resolved_dir(dir: &Path) -> PathBuf {
    use std::path::Component;
    let mut glatt = PathBuf::new();
    for teil in dir.components() {
        match teil {
            Component::CurDir => {}
            Component::ParentDir => {
                if !glatt.pop() {
                    glatt.push("..");
                }
            }
            sonst => glatt.push(sonst.as_os_str()),
        }
    }
    let mut kopf = glatt.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if !kopf.as_os_str().is_empty() {
            if let Ok(echt) = std::fs::canonicalize(&kopf) {
                let mut aus = echt;
                for teil in rest.iter().rev() {
                    aus.push(teil);
                }
                return aus;
            }
        }
        match kopf.file_name().map(std::ffi::OsStr::to_os_string) {
            Some(name) => {
                rest.push(name);
                kopf.pop();
            }
            None => {
                if glatt.is_absolute() {
                    return glatt;
                }
                let mut aus = std::fs::canonicalize(".").unwrap_or_else(|_| PathBuf::from("."));
                aus.push(&glatt);
                return aus;
            }
        }
    }
}

/// **Der Nachbau hängt am Quelltext.**
///
/// Ein nachgebauter Helfer prüft sein eigenes Abbild, sobald das Original sich
/// bewegt — dieselbe Klasse, an der in dieser Runde drei Belegdateien hingen.
/// Dagegen hilft keine Sorgfalt, sondern eine Bindung: dieser Test liest
/// `app.rs` und verlangt genau die Stellen, ohne die der Nachbau daneben läge.
#[test]
fn der_nachbau_haengt_am_quelltext() {
    let quelle = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app.rs"))
        .expect("app.rs");
    for stelle in [
        "fn writing_key(path: &std::path::Path) -> PathBuf {",
        "fn resolved_dir(dir: &std::path::Path) -> PathBuf {",
        "Component::CurDir => {}",
        "if !glatt.pop() {",
        "if let Ok(echt) = std::fs::canonicalize(&kopf) {",
        "(Some(dir), Some(name)) => resolved_dir(dir).join(name),",
    ] {
        assert!(
            quelle.contains(stelle),
            "`app.rs` trägt „{stelle}“ nicht mehr — dann prüft der Nachbau in \
             dieser Datei etwas anderes als das Original"
        );
    }
}

/// Das echte Schreibziel, wie `redact_pipeline::apply` es bestimmt:
/// `check_target` mit `force` (die Oberfläche setzt `force: true`, der Dialog
/// hat schon gefragt) und ohne geschützte Dateien.
fn schreibziel(p: &Path) -> Result<PathBuf, String> {
    check_target(p, &WriteOptions::new().force(true))
        .map(|t| t.path())
        .map_err(|e| e.to_string())
}

/// Beide Auskünfte nebeneinander, und der Vergleich als Text — damit der Lauf
/// selbst zeigt, worauf die Zusicherung sich stützt.
fn vergleich(a: &Path, b: &Path, was: &str) -> (bool, bool) {
    let ka = wie_writing_key(a);
    let kb = wie_writing_key(b);
    let za = schreibziel(a);
    let zb = schreibziel(b);
    let kennung_gleich = ka == kb;
    let ziel_gleich = matches!((&za, &zb), (Ok(x), Ok(y)) if x == y);
    println!("--- {was}");
    println!("  a = {}", a.display());
    println!("  b = {}", b.display());
    println!("  writing_key(a)  = {}", ka.display());
    println!("  writing_key(b)  = {}", kb.display());
    println!("  check_target(a) = {za:?}");
    println!("  check_target(b) = {zb:?}");
    println!("  Kennung gleich: {kennung_gleich}, Schreibziel gleich: {ziel_gleich}");
    (kennung_gleich, ziel_gleich)
}

// ===========================================================================
// 1 — Die Schreibweisen, die dieselbe Datei meinen
// ===========================================================================

/// **HÄLT.** `.`, `..`, der nackte Dateiname und ein **Verzeichnis**, das ein
/// Symlink ist: überall eine Kennung, und dieselbe, die `check_target`
/// beschreibt.
///
/// Das Verzeichnis ist der interessante Fall. Die Zusage „folgt keinem
/// Symlink" gilt dem **Dateinamen**; das Verzeichnis wird aufgelöst, und zwar
/// von `writing_key` und `check_target` gleichermaßen. Ein Klick auf
/// `link/out.pdf` und einer auf `echt/out.pdf` sind damit derselbe Export —
/// richtig, denn geschrieben wird dieselbe Datei.
#[test]
fn zm_b_eine_datei_eine_kennung() {
    let basis = tmp("gleich");
    let ordner = basis.join("ordner");
    std::fs::create_dir_all(&ordner).unwrap();

    let gerade = ordner.join("out.pdf");

    for (b, was) in [
        (ordner.join(".").join("out.pdf"), "ein Punkt"),
        (
            ordner.join("..").join("ordner").join("out.pdf"),
            "zwei Punkte",
        ),
    ] {
        let (kennung, ziel) = vergleich(&gerade, &b, was);
        assert!(ziel, "{was}: check_target schreibt dieselbe Datei");
        assert!(
            kennung,
            "{was}: dieselbe Datei, zwei Kennungen — zwei Threads würden sie gleichzeitig \
             schreiben, und ein Urteil über die alten Bytes bliebe stehen"
        );
    }

    // Der nackte Dateiname gegen `./name` — beides das laufende Verzeichnis.
    // Hier wird nichts geschrieben; verglichen werden nur die Auskünfte.
    let (kennung, ziel) = vergleich(
        Path::new("zm-b-nackt.pdf"),
        Path::new("./zm-b-nackt.pdf"),
        "ohne Verzeichnis",
    );
    assert!(ziel, "check_target setzt in beiden Fällen \".\" ein");
    assert!(kennung, "eine Datei, zwei Schreibweisen");

    std::fs::remove_dir_all(&basis).ok();
}

/// Das **Verzeichnis** als Symlink — getrennt, weil `symlink` nur unter Unix
/// ohne Sonderrechte geht.
#[test]
#[cfg(unix)]
fn zm_b_ein_verlinktes_verzeichnis_ist_dasselbe_verzeichnis() {
    let basis = tmp("linkdir");
    let echt = basis.join("echt");
    std::fs::create_dir_all(&echt).unwrap();
    let link = basis.join("link");
    // Schlägt das Anlegen fehl (kein Recht, kein Dateisystem dafür), sagt der
    // Lauf das und geht weiter — `cfg(unix)` heißt, dass es Links **gibt**,
    // nicht dass dieser Rechner sie anlegen lässt.
    if std::os::unix::fs::symlink(&echt, &link).is_err() {
        eprintln!("zm_b: kein Symlink anlegbar — diese Prüfung entfällt");
        return;
    }

    let (kennung, ziel) = vergleich(
        &echt.join("out.pdf"),
        &link.join("out.pdf"),
        "verlinktes Verzeichnis",
    );
    assert!(ziel, "check_target löst das Verzeichnis auf");
    assert!(
        kennung,
        "ein verlinktes Verzeichnis ist dasselbe Verzeichnis — sonst schreiben zwei Threads \
         dieselbe Datei"
    );

    std::fs::remove_dir_all(&basis).ok();
}

// ===========================================================================
// 2 — Die Schreibweisen, die verschiedene Dateien meinen
// ===========================================================================

/// **HÄLT, Gegenrichtung.** Gleicher Name in zwei Ordnern und Namen, die sich
/// nur in der Groß-/Kleinschreibung unterscheiden, sind hier **verschiedene**
/// Dateien — und bekommen verschiedene Kennungen. Ein gewöhnlicher zweiter
/// Export wird also nicht abgelehnt.
///
/// Bewiesen wird das nicht am Vergleich zweier Zeichenketten, sondern an der
/// Platte: beide Pfade werden beschrieben, und danach trägt jeder seinen
/// eigenen Inhalt.
///
/// **Offen bleibt der Fall, den dieser Rechner nicht herstellt**: auf einem
/// Dateisystem **ohne** Groß-/Kleinschreibung (NTFS, APFS in der Voreinstellung,
/// exFAT/VFAT auf einem Stick, eine SMB-Freigabe, ext4 mit `casefold`) sind
/// `Auszug.pdf` und `auszug.pdf` **eine** Datei. `writing_key` hängt den
/// Dateinamen unverändert an — dort also zwei Kennungen für eine Datei, und
/// damit genau die Lage, gegen die `EXPORT_BUSY` gebaut ist. Dass das kein
/// ausgedachter Fall ist, sagt `redact-pdf` selbst: `document::same_file`
/// vergleicht deshalb über Inode **und** über `eq_ignore_case`. Diese Datei
/// kann es nicht belegen — hier gibt es kein solches Dateisystem —, und ohne
/// Lauf ist es kein Befund, sondern eine offene Frage.
#[test]
fn zm_b_verschiedene_dateien_verschiedene_kennungen() {
    let basis = tmp("verschieden");
    let x = basis.join("x");
    let y = basis.join("y");
    std::fs::create_dir_all(&x).unwrap();
    std::fs::create_dir_all(&y).unwrap();

    let (kennung, ziel) = vergleich(
        &x.join("a.pdf"),
        &y.join("a.pdf"),
        "gleicher Name, zwei Ordner",
    );
    assert!(!ziel, "zwei Ordner sind zwei Schreibziele");
    assert!(
        !kennung,
        "gleicher Name in zwei Ordnern: eine gemeinsame Kennung lehnte den zweiten Export \
         mit EXPORT_BUSY ab, und die Handlung wäre verloren"
    );

    // Groß-/Kleinschreibung: hier zwei Dateien, und die Platte sagt es.
    let gross = basis.join("Auszug.pdf");
    let klein = basis.join("auszug.pdf");
    let (kennung, _) = vergleich(&gross, &klein, "Groß gegen klein");
    std::fs::write(&gross, b"GROSS").unwrap();
    std::fs::write(&klein, b"klein").unwrap();
    let inhalt_gross = std::fs::read(&gross).unwrap();
    let inhalt_klein = std::fs::read(&klein).unwrap();
    println!(
        "  Inhalt Auszug.pdf = {:?}, auszug.pdf = {:?}",
        String::from_utf8_lossy(&inhalt_gross),
        String::from_utf8_lossy(&inhalt_klein)
    );
    if inhalt_gross == inhalt_klein {
        // Dieses Dateisystem macht keine Groß-/Kleinschreibung — dann wäre
        // eine gemeinsame Kennung Pflicht, und `writing_key` hätte hier
        // verschiedene. Das ist die Lücke, die oben im Doktext steht.
        assert!(
            kennung,
            "dieses Dateisystem kennt keine Groß-/Kleinschreibung: Auszug.pdf und auszug.pdf \
             sind EINE Datei, writing_key gibt aber zwei Kennungen — zwei Threads schrieben \
             dieselbe Datei"
        );
    } else {
        assert!(
            !kennung,
            "hier sind es zwei Dateien, also müssen es zwei Kennungen sein"
        );
    }

    std::fs::remove_dir_all(&basis).ok();
}

/// **FEHLALARM-Verdacht widerlegt: der Hardlink.** Zwei Namen, ein Inode —
/// `writing_key` gibt zwei Kennungen, also laufen zwei Exporte gleichzeitig.
/// Geschadet hat das nicht, und der Lauf sagt warum: geschrieben wird
/// `Temp-Datei, dann rename`, und `rename` **ersetzt den Verzeichniseintrag**.
/// Danach zeigt jeder Name auf seinen eigenen, neuen Inode; es gibt keine
/// gemischten Bytes und kein Urteil über fremde Bytes.
#[test]
#[cfg(unix)]
fn zm_b_ein_hardlink_faellt_beim_schreiben_auseinander() {
    let basis = tmp("hardlink");
    let a = basis.join("a.pdf");
    let b = basis.join("b.pdf");
    std::fs::write(&a, b"ALT").unwrap();
    if std::fs::hard_link(&a, &b).is_err() {
        eprintln!("zm_b: kein Hardlink anlegbar — diese Prüfung entfällt");
        std::fs::remove_dir_all(&basis).ok();
        return;
    }
    let inode = |p: &Path| -> Option<u64> {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(p).ok().map(|m| m.ino())
    };
    assert_eq!(inode(&a), inode(&b), "ein Inode, zwei Namen");

    let (kennung, ziel) = vergleich(&a, &b, "Hardlink");
    assert!(!ziel, "check_target nennt zwei Pfade");
    assert!(!kennung, "und writing_key zwei Kennungen");

    // Und nun der Grund, warum das trägt: `redact_pdf::write_file` schreibt
    // atomar über eine Temp-Datei im selben Verzeichnis.
    redact_pdf::document::write_file(&a, b"NEU-A", &WriteOptions::new().force(true)).expect("a");
    println!(
        "  nach dem Schreiben von a: a = {:?}, b = {:?}, Inode gleich: {}",
        String::from_utf8_lossy(&std::fs::read(&a).unwrap()),
        String::from_utf8_lossy(&std::fs::read(&b).unwrap()),
        inode(&a) == inode(&b)
    );
    assert_eq!(std::fs::read(&a).unwrap(), b"NEU-A");
    assert_eq!(
        std::fs::read(&b).unwrap(),
        b"ALT",
        "rename ersetzt den Eintrag — der zweite Name behält die alten Bytes"
    );
    assert_ne!(inode(&a), inode(&b), "der Link ist gefallen");

    std::fs::remove_dir_all(&basis).ok();
}

/// **HÄLT.** Der **Dateiname** als Symlink: zwei Kennungen, und das ist
/// richtig, weil `check_target` den Link ablehnt — durch ihn entsteht kein
/// Byte. Ein Klick auf den Link darf das Urteil über das Linkziel nicht
/// anfassen.
#[test]
#[cfg(unix)]
fn zm_b_ein_verlinkter_dateiname_ist_kein_schreibziel() {
    let basis = tmp("linkdatei");
    let ziel = basis.join("ziel.pdf");
    std::fs::write(&ziel, b"ALT").unwrap();
    let link = basis.join("link.pdf");
    if std::os::unix::fs::symlink(&ziel, &link).is_err() {
        eprintln!("zm_b: kein Symlink anlegbar — diese Prüfung entfällt");
        std::fs::remove_dir_all(&basis).ok();
        return;
    }

    let (kennung, gleich) = vergleich(&ziel, &link, "Dateiname als Symlink");
    assert!(!gleich, "check_target lehnt den Link ab");
    assert!(
        !kennung,
        "der Link ist kein Schreibziel — sein Klick darf das Urteil über ziel.pdf nicht \
         wegwerfen"
    );
    assert!(
        schreibziel(&link).is_err(),
        "durch einen Link wird nicht geschrieben: {:?}",
        schreibziel(&link)
    );
    assert_eq!(std::fs::read(&ziel).unwrap(), b"ALT");

    std::fs::remove_dir_all(&basis).ok();
}

// ===========================================================================
// 3 — Die Stelle, an der die Zusage „das Verzeichnis gibt es" nicht gilt
// ===========================================================================

/// **Bestätigt (Befund der Runde 8 ist offen).** `writing_key` begründet sich
/// mit „aufgelöst wird das Verzeichnis (das gibt es, `check_target` legt es
/// notfalls an)". Das *notfalls* ist der Punkt: **vor** dem ersten Export gibt
/// es den Ordner nicht, `canonicalize` scheitert, und die Kennung ist der
/// ungelöste Pfad. Nach dem ersten Export ist sie der aufgelöste.
///
/// Dieser Lauf hält die Folge fest, die `zj_c_vor_dem_anlegen_des_ordners_ist_die_kennung_eine_andere`
/// (Runde 8) noch nicht nennt: **innerhalb eines Exports** benutzt die
/// Oberfläche beide Kennungen.
///
/// * `PendingExport::writing` — beim Klick gefasst, also die **ungelöste**;
///   daran hängt `poll_exports` den Schnitt und daran die Abbruchwarnung
///   `EXPORT_BROKEN` (`note_check_warning(&writing, …)`);
/// * `finish_export` rechnet `let key = writing_key(out);` **neu**, also die
///   **aufgelöste**; daran hängen `note_export_warnings` und die neue
///   Nachprüfung.
///
/// Trifft ein gescheiterter Export (Ordner bleibt aus) auf einen geglückten
/// (Ordner wird angelegt), finden sich die beiden Kennungen nicht — und „es
/// wurde keine Datei geschrieben" bleibt neben der Erfolgsmeldung stehen. Das
/// ist wörtlich der Fehler, den `zh2_c_die_abbruchwarnung_verschwindet_beim_naechsten_geglueckten_export`
/// für den Fall *mit* Ordner schließt.
///
/// Belegt ist hier die **Kennung**, nicht die Warnungsliste: `export_to`,
/// `exports` und `note_check_warning` sind privat. Der fehlende Lauf gehört
/// nach `zh2_c_leck_tests` (Kindmodul von `app`) und ist im Bericht benannt.
#[test]
fn zm_b_ein_ordner_der_erst_entsteht_hat_dieselbe_kennung() {
    let basis = tmp("nodir");
    // Eine Schreibweise, an der es etwas aufzulösen gibt, und ein Ordner, den
    // es noch nicht gibt. Ohne `..` fällt der ungelöste Pfad hier zufällig mit
    // dem aufgelösten zusammen (`/tmp` ist kein Link) — dann sagt der Lauf
    // nichts, und genau das wäre der Scheintest.
    let ziel = basis.join("neu").join("..").join("neu").join("out.pdf");
    assert!(
        !basis.join("neu").exists(),
        "der Ordner darf noch nicht da sein"
    );

    let beim_klick = wie_writing_key(&ziel);
    let ziel_vorher = schreibziel(&ziel);
    assert!(
        basis.join("neu").exists(),
        "check_target legt den Ordner an — genau das ist der Bruch"
    );
    let nach_dem_anlegen = wie_writing_key(&ziel);

    println!("Pfad:              {}", ziel.display());
    println!("beim Klick:        {}", beim_klick.display());
    println!("nach dem Anlegen:  {}", nach_dem_anlegen.display());
    println!("check_target:      {ziel_vorher:?}");

    assert_eq!(
        ziel_vorher.as_deref().map(Path::to_path_buf).ok(),
        Some(nach_dem_anlegen.clone()),
        "die Identität, mit der geschrieben wird, ist die aufgelöste"
    );
    assert_eq!(
        beim_klick, nach_dem_anlegen,
        "EINE Kennung je Export — und zwar vor und nach dem Anlegen des Ordners dieselbe. \
         Hier standen zwei: `PendingExport::writing` trug die beim Klick gefasste (daran \
         hängen der Schnitt in `poll_exports` und die Abbruchwarnung EXPORT_BROKEN), \
         `finish_export` rechnete eine zweite neu (daran hingen `note_export_warnings` und \
         die Nachprüfung). Jetzt glättet `resolved_dir` erst `.`/`..` und löst dann den \
         längsten vorhandenen Kopf auf, und `finish_export` bekommt die Kennung des Klicks \
         gereicht."
    );

    std::fs::remove_dir_all(&basis).ok();
}

/// Derselbe Bruch mit einem verlinkten Verzeichnis statt mit `..` — die Lage,
/// die auf einem gewöhnlichen Rechner wirklich vorkommt (ein verlinktes
/// `~/Dokumente`, `/tmp` als Link, ein eingehängtes Laufwerk).
///
/// Unter `cfg(unix)`, und ein `symlink`, das nicht geht, übergeht die Prüfung
/// mit einer Zeile auf der Fehlerausgabe.
#[test]
#[cfg(unix)]
fn zm_b_ein_ordner_unter_einem_link_hat_dieselbe_kennung() {
    let basis = tmp("nodir-link");
    let echt = basis.join("echt");
    std::fs::create_dir_all(&echt).unwrap();
    let link = basis.join("link");
    if std::os::unix::fs::symlink(&echt, &link).is_err() {
        eprintln!("zm_b: kein Symlink anlegbar — diese Prüfung entfällt");
        std::fs::remove_dir_all(&basis).ok();
        return;
    }

    let ziel = link.join("neu").join("out.pdf");
    assert!(!ziel.parent().unwrap().exists());
    let beim_klick = wie_writing_key(&ziel);
    let ziel_vorher = schreibziel(&ziel);
    let nach_dem_anlegen = wie_writing_key(&ziel);

    println!("Pfad:              {}", ziel.display());
    println!("beim Klick:        {}", beim_klick.display());
    println!("nach dem Anlegen:  {}", nach_dem_anlegen.display());
    println!("check_target:      {ziel_vorher:?}");

    assert_eq!(
        beim_klick, nach_dem_anlegen,
        "EINE Kennung je Export — siehe \
         zm_b_ein_ordner_der_erst_entsteht_hat_dieselbe_kennung"
    );

    std::fs::remove_dir_all(&basis).ok();
}

// ===========================================================================
// 4 — Die Gegenrichtung: darf die Eingabedatei je überschrieben werden?
// ===========================================================================

/// **HÄLT — aber erst eine Ebene tiefer.** `AppState::targets_the_input`
/// vergleicht `out == input` und danach die **kanonisierten Pfade**.
/// `canonicalize` löst Symlinks auf, **Hardlinks nicht**: ein zweiter Name
/// derselben Eingabedatei kommt an dieser Ablehnung vorbei.
///
/// Gefährlich ist das nicht, und der Lauf sagt warum: `check_target` fragt
/// nicht nach dem Pfad, sondern nach der **Datei** (`document::same_file`,
/// Gerät + Inode), und `redact_pipeline::apply` gibt ihm die Eingabedatei als
/// geschützt mit. Der Export scheitert also — mit dem richtigen Satz, nur eine
/// Ebene später und ohne die freundliche Formulierung der Oberfläche.
///
/// Gemessen wird am Ende das, worauf es ankommt: die **Eingabedatei ist Byte
/// für Byte unverändert**.
#[test]
#[cfg(unix)]
fn zm_b_ein_hardlink_der_eingabe_kommt_an_der_oberflaeche_vorbei_aber_nicht_durch() {
    // Hier eingeführt und nicht oben: den ganzen Abschnitt gibt es nur unter
    // Unix, und eine Einfuhr, die unter Windows niemand benutzt, ist dort ein
    // Baufehler (`-D warnings`).
    use redact_core::{Rect, Region, Source};
    use redact_gui::state::{AnnotatedRegion, AppState};
    use redact_gui::Config;
    use redact_pdf::testing::{build_pdf, TextItem};

    let dir = tmp("eingang-hardlink");
    let bytes = build_pdf(&[vec![TextItem::new(
        72.0,
        700.0,
        10.0,
        "Zeile A GEHEIM-EINS",
    )]]);
    let input = dir.join("eingang.pdf");
    std::fs::write(&input, &bytes).unwrap();
    let zweitname = dir.join("zweitname.pdf");
    if std::fs::hard_link(&input, &zweitname).is_err() {
        eprintln!("zm_b: kein Hardlink anlegbar — diese Prüfung entfällt");
        std::fs::remove_dir_all(&dir).ok();
        return;
    }

    let mut state = AppState::with_config(Config {
        no_patterns: true,
        ..Config::default()
    });
    state.load_bytes(&bytes, Some(input.clone())).unwrap();
    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        Rect::new(60.0, 690.0, 520.0, 714.0),
        Some("GEHEIM-EINS".to_string()),
        Source::Manual {
            reason: "zm-b".into(),
        },
    )));

    // Die Ablehnung der Oberfläche greift **nicht**.
    println!(
        "targets_the_input(zweitname) = {}",
        state.targets_the_input(&zweitname)
    );
    assert!(
        !state.targets_the_input(&zweitname),
        "hier greift sie doch — dann ist die Lage besser als gedacht"
    );

    // Der Schreibpfad lehnt trotzdem ab — und das Original bleibt stehen.
    let plan = state
        .plan_export(&zweitname, None)
        .expect("die Oberfläche lässt den Plan zu");
    let (geschrieben, ergebnis) = plan.run_reporting();
    println!("geschrieben = {geschrieben}, Ergebnis = {ergebnis:?}");
    assert!(!geschrieben, "kein Byte darf entstanden sein");
    let fehler = ergebnis.expect_err("der Lauf muss scheitern").to_string();
    println!("Fehler: {fehler}");
    assert!(
        fehler.contains("identisch"),
        "der Satz muss die Identität nennen: {fehler}"
    );
    assert_eq!(
        std::fs::read(&input).unwrap(),
        bytes,
        "die ungeschwärzte Eingabe muss Byte für Byte unverändert sein"
    );

    std::fs::remove_dir_all(&dir).ok();
}
