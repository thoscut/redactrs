//! Gegenprüfung der Nachbesserung zu Register #19, Linsen **Fehlalarm** und
//! **Scheintest** — Agent „zj_c" (Datei 2).
//!
//! Geprüft wird die eine Zusicherung, an der der ganze Umbau hängt, und die
//! im Bericht so steht:
//!
//! > die Fahne ist NICHT `result.is_ok()`, sondern `outcome.output.is_some()`
//! > […] Damit ist die Fahne in BEIDEN Richtungen exakt.
//!
//! Die GUI-Sammlung belegt das **nicht**: setzt man in
//! `ExportPlan::run_reporting` `let wrote = applied.is_ok();`, bleiben alle
//! 381 Tests von `redact-gui` grün (Mutation M8 dieser Gegenprüfung). Diese
//! Datei hängt die Zusicherung an einen Lauf, über den öffentlichen Weg
//! (`AppState::plan_export` → `ExportPlan::run_reporting`) und mit dem
//! Dateisystem als Zeugen:
//!
//! 1. gewöhnliche Arbeit: `(true, Ok)` — und die Datei steht da, geschwärzt.
//!    Das ist die Fehlalarm-Richtung: ein gewöhnlicher Export wird nicht
//!    verdächtigt;
//! 2. die PDF steht, das **Audit-Log** scheitert (der Logpfad ist ein
//!    Verzeichnis — `write_file` lehnt „keine gewöhnliche Datei" ab):
//!    `(true, Err)`. Genau dieser Fall trennt `output.is_some()` von
//!    `is_ok()`, und er ist erreichbar;
//! 3. ein Symlink als Ziel: `(false, Err)`, und die verlinkte Datei ist Byte
//!    für Byte unberührt. Das ist die Richtung, an der Einwand 1a hing.
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-gui --test zj_c_fahne_beide_richtungen`

use std::path::{Path, PathBuf};

use redact_core::{Rect, Region, Source};
use redact_gui::state::{AnnotatedRegion, AppState};
use redact_gui::Config;
use redact_pdf::testing::{build_pdf, TextItem};

const GEHEIM: &str = "GEHEIM-EINS";

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zj-c-fahne-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ein_geheimnis() -> Vec<u8> {
    build_pdf(&[vec![TextItem::new(
        72.0,
        700.0,
        10.0,
        "Zeile A GEHEIM-EINS",
    )]])
}

/// Ein Rechteck über der Zeile — es trifft, die Ausgabe ist sauber.
fn ueber() -> Rect {
    Rect::new(60.0, 690.0, 520.0, 714.0)
}

fn geladen(dir: &Path) -> AppState {
    let bytes = ein_geheimnis();
    let input = dir.join("eins.pdf");
    std::fs::write(&input, &bytes).unwrap();
    let mut state = AppState::with_config(Config {
        no_patterns: true,
        ..Config::default()
    });
    state.load_bytes(&bytes, Some(input)).unwrap();
    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        ueber(),
        Some(GEHEIM.to_string()),
        Source::Manual {
            reason: "zj-c".into(),
        },
    )));
    state
}

// ===========================================================================
// 1 — Gewöhnliche Arbeit: (true, Ok), und die Datei ist sauber
// ===========================================================================

#[test]
fn zj_c_gewoehnlicher_export_meldet_geschrieben_und_gelungen() {
    let dir = tmp("gewoehnlich");
    let state = geladen(&dir);
    let out = dir.join("out.pdf");
    let audit = dir.join("out.audit.json");

    let (geschrieben, ergebnis) = state
        .plan_export(&out, Some(&audit))
        .expect("Plan")
        .run_reporting();
    assert!(geschrieben, "die Datei steht da — die Fahne muss das sagen");
    let outcome = ergebnis.expect("gewöhnlicher Export");
    assert_eq!(outcome.drawn_rects, 1);
    assert!(
        out.exists() && audit.exists(),
        "Ausgabe und Log fehlen nicht"
    );

    // Das Orakel, nicht der Bericht: hier leckt nichts.
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        redact_pdf::leaks(&bytes, GEHEIM).is_empty(),
        "das Rechteck trifft — die Ausgabe muss sauber sein"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 2 — Die PDF steht, das Log scheitert: (true, Err)
// ===========================================================================

/// **Der Fall, der `output.is_some()` von `is_ok()` trennt.**
///
/// `redact_pipeline::apply` schreibt die PDF in Schritt 11 und setzt dabei
/// `outcome.output`; das Audit-Log kommt erst in Schritt 12 und geht über
/// `?`. Scheitert es, kommt ein `Err` **hinter** fertigen Bytes.
///
/// ## Warum der Auslöser ein langer Name ist
///
/// Die naheliegenden Auslöser fallen aus: `plan_outputs` prüft das Ziel des
/// Logs mit `check_target` **vor** jedem Schreiben (unten mitgeprüft), also
/// sind „Verzeichnis statt Datei", „Symlink", „Verzeichnis nicht da" schon
/// vorher erledigt — und die Fahne sagt dort richtig `false`.
///
/// Übrig bleibt, was erst beim Schreiben selbst auffällt: **volle Platte**
/// (`ENOSPC` — genau der Fall aus dem Einwand), `EIO`, ein Ziel, das sich
/// zwischen Prüfung und Schreiben ändert. Keiner davon lässt sich in einem
/// Test herstellen. Derselbe Ausgang, herstellbar: `write_file` legt neben dem
/// Ziel eine temporäre Datei `.<name>.redact-<pid>-<n>.tmp` an, und die ist
/// rund zwanzig Zeichen **länger** als der Name. Ein Logname knapp unter der
/// Grenze kommt also durch die Vorprüfung und scheitert beim Anlegen der
/// temporären Datei. Der Name ist ungewöhnlich, der **Ausgang** ist der
/// gewöhnliche: PDF steht, Log nicht, Lauf endet als `Err`.
///
/// Wäre die Fahne `is_ok()`, stünde hier `false`: `poll_exports` ließe dann
/// das Urteil über die **alten** Bytes stehen, obwohl auf der Platte neue
/// liegen — die Entwarnung über eine Ausgabe, die niemand geprüft hat.
#[test]
fn zj_c_die_pdf_steht_das_log_scheitert_die_fahne_sagt_geschrieben() {
    let dir = tmp("log-scheitert");

    // Gegenprobe zuerst: fällt der Logpfad schon der Vorprüfung auf, ist
    // **kein** Byte geschrieben — und die Fahne sagt das.
    {
        let state = geladen(&dir);
        let out = dir.join("vorher.pdf");
        let als_ordner = dir.join("ordner.json");
        std::fs::create_dir_all(&als_ordner).unwrap();
        let (geschrieben, ergebnis) = state
            .plan_export(&out, Some(&als_ordner))
            .expect("Plan")
            .run_reporting();
        let fehler = ergebnis.expect_err("ein Verzeichnis ist kein Logziel");
        eprintln!("Vorprüfung: {fehler}");
        assert!(
            !out.exists(),
            "plan_outputs prüft das Log VOR dem Schreiben — es darf keine PDF geben"
        );
        assert!(!geschrieben, "kein Byte, also `false`");
    }

    // Und nun der Fall hinter Schritt 11.
    let state = geladen(&dir);
    let out = dir.join("out.pdf");
    // 250 Zeichen: `check_target` kommt damit durch, die temporäre Datei
    // (~20 Zeichen mehr) nicht.
    let langer_name = format!("{}.json", "l".repeat(245));
    assert_eq!(langer_name.len(), 250);
    let audit = dir.join(&langer_name);

    let (geschrieben, ergebnis) = state
        .plan_export(&out, Some(&audit))
        .expect("Plan")
        .run_reporting();

    let fehler = ergebnis.expect_err("das Log kann nicht geschrieben werden");
    eprintln!("Fehler des Laufs: {fehler}");
    assert!(
        out.exists(),
        "die PDF ist in Schritt 11 geschrieben worden — vor dem Log"
    );
    assert!(!audit.exists(), "das Log dagegen fehlt");
    let bytes = std::fs::read(&out).unwrap();
    assert!(!bytes.is_empty(), "und sie ist nicht leer");
    assert!(
        redact_pdf::leaks(&bytes, GEHEIM).is_empty(),
        "die geschriebene Datei ist die geschwärzte"
    );
    assert!(
        geschrieben,
        "NEUE BYTES liegen auf der Platte, der Lauf endet trotzdem als Err — \
         genau hier wäre `is_ok()` falsch"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 3 — Symlink als Ziel: (false, Err), und kein Byte bewegt sich
// ===========================================================================

/// Die Gegenrichtung, an der Einwand 1a hing: `false` heißt wirklich
/// „kein Byte".
#[test]
#[cfg(unix)]
fn zj_c_ein_symlink_als_ziel_meldet_nicht_geschrieben() {
    let dir = tmp("symlink");
    let state = geladen(&dir);

    // Eine echte Ausgabe, die es schon gibt …
    let out = dir.join("out.pdf");
    let audit = dir.join("out.audit.json");
    let (geschrieben, ergebnis) = state
        .plan_export(&out, Some(&audit))
        .expect("Plan")
        .run_reporting();
    assert!(geschrieben && ergebnis.is_ok());
    let vorher = std::fs::read(&out).unwrap();

    // … und ein Link darauf als zweites Ziel.
    let link = dir.join("link.pdf");
    std::os::unix::fs::symlink(&out, &link).unwrap();
    let audit_link = dir.join("link.audit.json");
    let (geschrieben, ergebnis) = state
        .plan_export(&link, Some(&audit_link))
        .expect("plan_export lehnt den Link nicht ab")
        .run_reporting();
    let fehler = ergebnis.expect_err("durch Links wird nicht geschrieben");
    assert!(
        fehler.to_string().contains("symbolischer Link"),
        "unerwarteter Fehler: {fehler}"
    );
    assert!(
        !geschrieben,
        "kein Byte ist entstanden — die Fahne darf nichts anderes sagen"
    );
    assert_eq!(
        std::fs::read(&out).unwrap(),
        vorher,
        "die verlinkte Datei ist unberührt"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 4 — Restgrenze: `writing_key` VOR dem Anlegen des Verzeichnisses
// ===========================================================================

/// **Eine Grenze, die offen bleibt — und die der Doc-Kommentar von
/// `writing_key` zu klein macht.**
///
/// `writing_key` löst das Verzeichnis auf; der Doc-Kommentar begründet das mit
/// „(das gibt es, `check_target` legt es notfalls an)". Nur läuft
/// `writing_key` in `export_to` **vor** `check_target` — beim ersten Export in
/// einen noch nicht vorhandenen Ordner gibt es das Verzeichnis eben nicht,
/// und `canonicalize` fällt auf den getippten Pfad zurück. `check_target`
/// legt es dann an, und ab da liefert dieselbe Zeile für denselben Klick eine
/// **andere** Kennung.
///
/// Sichtbar wird der Unterschied, sobald über dem Ziel ein Symlink liegt —
/// ein symbolisch verlinktes Heimatverzeichnis genügt. Dann gilt für einen
/// einzigen Klick:
///
/// * beim Klick (Ordner fehlt): `…/link/neu/out.pdf`
/// * nach dem Schreiben (Ordner da): `…/echt/neu/out.pdf`
///
/// Folgen im Programm, in der Reihenfolge ihrer Schwere: die Sperre gegen
/// **zwei gleichzeitige Exporte derselben Datei** (`EXPORT_BUSY`) vergleicht
/// Kennungen und würde einen zweiten Klick durchlassen, obwohl er dasselbe
/// Ziel schreibt; und der Schnitt in `poll_exports` verglich eine Kennung von
/// **vor** dem Anlegen mit einer von **danach**.
///
/// Das ist **nicht** neu in dieser Nachbesserung (`writing_key` kam in der
/// Runde davor) und kein Fehlalarm — ein gewöhnlicher Export in einen
/// vorhandenen Ordner ist nicht betroffen. Es ist die Zusicherung im
/// Doc-Kommentar, die weiter reicht als der Code.
#[test]
#[cfg(unix)]
fn zj_c_vor_dem_anlegen_des_ordners_ist_die_kennung_eine_andere() {
    use redact_pdf::document::{check_target, WriteOptions};

    let basis = tmp("ordner-erst-spaeter");
    let echt = basis.join("echt");
    std::fs::create_dir_all(&echt).unwrap();
    let link = basis.join("link");
    std::os::unix::fs::symlink(&echt, &link).unwrap();

    // Das Ziel: ein Ordner, den es noch nicht gibt, unter dem Link.
    let ziel = link.join("neu").join("out.pdf");
    assert!(!ziel.parent().unwrap().exists(), "der Ordner fehlt noch");

    // Wörtlich `app::writing_key` zum Stand dieser Prüfung — mit Absicht: die
    // Zusicherung darunter („EIN Klick, ZWEI Kennungen“) hält den Defekt der
    // Runde 8 fest, den die Runde 9 mit `resolved_dir` geschlossen hat. Das
    // Original (seit Register #60 öffentlich, `redact_gui::app::writing_key`)
    // gäbe hier eine Kennung, und der Beleg des Defekts wäre keiner mehr.
    let wie_writing_key = |p: &Path| -> PathBuf {
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
    };

    let beim_klick = wie_writing_key(&ziel);

    // Das tut `check_target` — und zwar erst im Export-Thread.
    let target = check_target(&ziel, &WriteOptions::new().force(true)).expect("Ziel");
    assert!(
        ziel.parent().unwrap().exists(),
        "check_target hat ihn angelegt"
    );

    let nach_dem_schreiben = wie_writing_key(&ziel);

    eprintln!("beim Klick:          {}", beim_klick.display());
    eprintln!("nach dem Schreiben:  {}", nach_dem_schreiben.display());
    eprintln!("check_target:        {}", target.path().display());

    // Die Identität, mit der wirklich geschrieben wird, ist die aufgelöste.
    assert_eq!(
        target.path(),
        nach_dem_schreiben,
        "danach stimmen Kennung und Schreibziel überein"
    );
    assert_ne!(
        beim_klick, nach_dem_schreiben,
        "EIN Klick, ZWEI Kennungen — vor und nach dem Anlegen des Ordners"
    );

    std::fs::remove_dir_all(&basis).ok();
}
