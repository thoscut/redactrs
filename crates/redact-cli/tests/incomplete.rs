//! Befund #79: eine weiche Warnung darf nicht mit Rückgabewert 0 enden.
//!
//! # Was hier gemessen wird
//!
//! Eine Seite, deren Content-Stream sich in keine Operation zerlegen lässt,
//! bricht seit kurzem hart ab. Vier weitere Fälle sagen aber wörtlich dasselbe
//! — „wurde nicht durchsucht und kann deshalb nicht geschwärzt worden sein"
//! und endeten trotzdem mit Rückgabewert 0: ein Font ohne `/ToUnicode`, ein
//! Form-XObject unterhalb der Verschachtelungsgrenze, ein Kachelmuster mit
//! Text, ein XObject ohne bekanntes `/Subtype`. Im Stapelbetrieb zählten
//! solche Dateien als „verarbeitet".
//!
//! Stellvertreter ist seit Fix-Runde 4 das **XObject ohne `/Subtype`**. Bis
//! dahin war es die Annotation ohne Erscheinungsstrom — die ist keine
//! Deckungslücke mehr: ihr Text wird von `strip_metadata` als Ganzes
//! entfernt, und der Lauf sagt das jetzt so (Befund G2-7). Dass sie mit 0
//! endet, prüft `an_annotation_without_appearance_stream_is_no_longer_a_gap`.
//!
//! Gemessen wird am **gebauten Binary**, weil der Rückgabewert genau das ist,
//! was ein Skript sieht — eine Zusicherung über `Outcome` allein hätte den
//! Befund nicht berührt.
//!
//! # Die drei Zusicherungen
//!
//! 1. Eine Datei mit einer Deckungslücke endet mit Rückgabewert 3 und sagt
//!    warum.
//! 2. Eine **harmlose** Datei endet weiterhin mit 0 — sonst wäre der neue Wert
//!    nach zwei Läufen ein Wert, den man wegdrückt.
//! 3. Im Stapel zählt eine solche Datei **nicht** als „verarbeitet", und der
//!    Rückgabewert des Stapels sagt es.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lopdf::{dictionary, Document, Object, Stream};

/// Siehe `crates/redact-cli/src/main.rs`.
const EXIT_OK: i32 = 0;
const EXIT_ERROR: i32 = 1;
const EXIT_INCOMPLETE: i32 = 3;

const IBAN: &str = "DE89 3704 0044 0532 0130 00";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "redact-unvollstaendig-{}-{name}",
        std::process::id()
    ));
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

/// Was die Datei außer dem Seitentext tragen soll.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Zusatz {
    /// Nichts — die harmlose Gegenprobe.
    Nichts,
    /// Ein XObject ohne `/Subtype`, per `Do` gezeichnet: die Analyse weiß
    /// nicht, was darin steht, und sagt es — die Deckungslücke, um die es geht.
    XObjectOhneSubtype,
    /// Eine Annotation mit Text in `/Contents`, aber ohne Erscheinungsstrom:
    /// seit Fix-Runde 4 **keine** Lücke mehr, der Text fällt mit den Metadaten.
    AnnotationOhneAp,
}

/// Ein gewöhnliches Dokument mit einer IBAN im Seiteninhalt, plus [`Zusatz`].
///
/// Das übrige Dokument bleibt völlig normal, damit die Gegenprobe (dieselbe
/// Datei ohne den Zusatz) sich in *nichts* anderem unterscheidet.
fn pdf(dir: &Path, name: &str, zusatz: Zusatz) -> PathBuf {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1",
        "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
    });
    let mut resources = dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    };
    let mut content = format!("BT\n/F1 10 Tf\n72 700 Td\n(IBAN: {IBAN}) Tj\nET\n");
    if zusatz == Zusatz::XObjectOhneSubtype {
        // Ein Strom, der aussieht wie ein Formular, aber nicht sagt, was er
        // ist: kein /Subtype. Die Analyse liest ihn nicht und meldet das.
        let fx_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "BBox" => vec![0.into(), 0.into(), 100.into(), 20.into()],
            },
            format!("BT /F1 10 Tf 0 0 Td (Fx: {IBAN}) Tj ET").into_bytes(),
        ));
        resources.set("XObject", dictionary! { "Fx" => fx_id });
        content.push_str("q 1 0 0 1 72 650 cm /Fx Do Q\n");
    }
    let resources_id = doc.add_object(resources);
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));

    let pages_id = doc.new_object_id();
    let mut page = dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "Contents" => content_id, "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    };
    if zusatz == Zusatz::AnnotationOhneAp {
        let annot_id = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text",
            "Rect" => vec![72.into(), 600.into(), 300.into(), 620.into()],
            // Text ja, Erscheinungsstrom nein.
            "Contents" => Object::string_literal(format!("Notiz: {IBAN}")),
        });
        page.set("Annots", vec![Object::Reference(annot_id)]);
    }
    let page_id = doc.add_object(page);
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let path = dir.join(name);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("speicherbar");
    std::fs::write(&path, bytes).unwrap();
    path
}

// ---------------------------------------------------------------------------
// 1. Die Deckungslücke bekommt einen eigenen Rückgabewert
// ---------------------------------------------------------------------------

#[test]
fn a_file_that_could_not_be_fully_searched_does_not_report_success() {
    let dir = workdir("einzeln");
    let input = pdf(&dir, "mit-luecke.pdf", Zusatz::XObjectOhneSubtype);
    let output = dir.join("aus.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);

    // Der Lauf ist *durchgelaufen*: die Ausgabe steht da, und der sichtbare
    // Seitentext ist geschwärzt. Es ist kein Fehler.
    assert!(
        output.exists(),
        "keine Ausgabe geschrieben: {}",
        stderr(&out)
    );
    assert_ne!(
        out.status.code(),
        Some(EXIT_ERROR),
        "eine Deckungslücke ist kein Fehlschlag: {}",
        stderr(&out)
    );

    // Aber er ist auch kein „alles gut".
    assert_eq!(
        out.status.code(),
        Some(EXIT_INCOMPLETE),
        "Rückgabewert {:?} statt {EXIT_INCOMPLETE}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        stdout(&out),
        stderr(&out)
    );

    // Und er sagt, woran es lag — mit dem Wortlaut, an dem es zu erkennen ist.
    let fehler = stderr(&out);
    assert!(
        fehler.contains("nicht durchsucht"),
        "die Meldung nennt den Grund nicht:\n{fehler}"
    );
    assert!(
        fehler.contains("NICHT GEPRÜFT"),
        "die Deckungslücke steht zwischen den übrigen Warnungen:\n{fehler}"
    );
    assert!(
        fehler.contains(&EXIT_INCOMPLETE.to_string()),
        "der Rückgabewert wird nicht erklärt:\n{fehler}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die wichtigere Hälfte:** ein gewöhnliches Dokument endet weiterhin mit 0.
///
/// Ohne diese Gegenprobe wäre der Test oben auch dann grün, wenn jeder Lauf
/// mit 3 endete — und ein Rückgabewert, der immer anspringt, ist keiner.
/// Verglichen wird mit *derselben* Datei ohne die Annotation.
#[test]
fn an_ordinary_file_still_reports_plain_success() {
    let dir = workdir("harmlos");
    let input = pdf(&dir, "ohne-luecke.pdf", Zusatz::Nichts);
    let output = dir.join("aus.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    assert_eq!(
        out.status.code(),
        Some(EXIT_OK),
        "eine harmlose Datei endet nicht mehr mit 0\nstdout:\n{}\nstderr:\n{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        !stderr(&out).contains("NICHT GEPRÜFT"),
        "hier gibt es nichts zu melden:\n{}",
        stderr(&out)
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// **Gegenrichtung zu Fix-Runde 4 (Befund G2-7):** eine Annotation mit Text
/// in `/Contents`, aber ohne Erscheinungsstrom, war bis dahin der
/// Stellvertreter für Rückgabewert 3 — obwohl derselbe Lauf den Text mit den
/// Metadaten entfernt und `--check-leaks` danach 0 meldete. Jetzt endet sie
/// mit 0, die Warnung sagt, was geschieht, und die Ausgabe trägt den Text
/// nicht mehr. Fällt die Marke aus `NOT_A_COVERAGE_GAP`
/// (`redact-pipeline/src/coverage.rs`), wird dieser Test rot.
#[test]
fn an_annotation_without_appearance_stream_is_no_longer_a_gap() {
    let dir = workdir("annotation-ohne-ap");
    let input = pdf(&dir, "notiz.pdf", Zusatz::AnnotationOhneAp);
    let output = dir.join("aus.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    assert_eq!(
        out.status.code(),
        Some(EXIT_OK),
        "die Annotation ohne /AP gilt wieder als Deckungslücke\nstdout:\n{}\nstderr:\n{}",
        stdout(&out),
        stderr(&out)
    );
    let fehler = stderr(&out);
    assert!(
        !fehler.contains("NICHT GEPRÜFT"),
        "der Text wird als Ganzes entfernt, das ist keine Lücke:\n{fehler}"
    );
    assert!(
        fehler.contains("als Ganzes entfernt"),
        "der Lauf sagt nicht, was mit dem Text geschieht:\n{fehler}"
    );

    // Und die Tat zur Warnung: der Text ist wirklich weg.
    let bytes = std::fs::read(&output).unwrap();
    let funde = redact_pdf::leaks(&bytes, IBAN);
    assert!(
        funde.is_empty(),
        "der Annotationstext steht noch in der Ausgabe: {funde:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Ein Bedienfehler bleibt ein Bedienfehler: die 2 wird nicht überladen.
#[test]
fn a_usage_error_keeps_its_own_exit_code() {
    let dir = workdir("bedienung");
    // Zwei Eingabedateien und ein festes Ausgabeziel: jedes Ergebnis
    // überschriebe das vorige, das lehnt der Stapel ab. Beide Dateien haben
    // eine Deckungslücke — die darf den Bedienfehler nicht verdrängen.
    let a = pdf(&dir, "a.pdf", Zusatz::XObjectOhneSubtype);
    let b = pdf(&dir, "b.pdf", Zusatz::XObjectOhneSubtype);

    let out = run(&[
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "-o",
        dir.join("aus.pdf").to_str().unwrap(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "der Bedienfehler hat seinen eigenen Rückgabewert verloren: {}",
        stderr(&out)
    );
    assert!(stderr(&out).contains("--output"), "{}", stderr(&out));

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// 2. Der Stapel weist solche Dateien getrennt aus
// ---------------------------------------------------------------------------

/// **Der Kern des Befunds im Stapelbetrieb.**
///
/// Vorher: „3 Datei(en): 3 verarbeitet, 0 fehlgeschlagen." und Rückgabewert 0
/// — obwohl in zweien davon ein Teil des Dokuments nie gelesen wurde.
#[test]
fn the_batch_summary_counts_unsearched_files_separately() {
    let dir = workdir("stapel");
    pdf(&dir, "a-harmlos.pdf", Zusatz::Nichts);
    pdf(&dir, "b-lueckenhaft.pdf", Zusatz::XObjectOhneSubtype);
    pdf(&dir, "c-lueckenhaft.pdf", Zusatz::XObjectOhneSubtype);

    let out = run(&[dir.to_str().unwrap(), "--patterns", "iban_de"]);
    let zusammenfassung = stdout(&out);

    // Eine Datei vollständig geprüft, zwei nicht — und keine davon zählt in
    // beiden Zahlen mit.
    assert!(
        zusammenfassung.contains("3 Datei(en): 1 vollständig geprüft, 2 verarbeitet"),
        "die Zusammenfassung verbucht die Lücken unter „verarbeitet“:\n{zusammenfassung}"
    );
    assert!(
        zusammenfassung.contains("0 fehlgeschlagen"),
        "gescheitert ist nichts:\n{zusammenfassung}"
    );
    // Und es steht **an jeder betroffenen Zeile**, nicht nur einmal am Ende:
    // wer zwanzig Dateien laufen lässt, muss sehen, welche gemeint ist.
    let markiert = |name: &str| {
        zusammenfassung
            .lines()
            .any(|zeile| zeile.contains(name) && zeile.contains("nicht vollständig geprüft"))
    };
    for name in ["b-lueckenhaft.pdf", "c-lueckenhaft.pdf"] {
        assert!(
            markiert(name),
            "{name} wird in seiner eigenen Zeile nicht ausgewiesen:\n{zusammenfassung}"
        );
    }
    // Die Gegenprobe: die unauffällige Datei bekommt die Markierung nicht.
    assert!(
        !markiert("a-harmlos.pdf"),
        "die harmlose Datei wird mitmarkiert:\n{zusammenfassung}"
    );
    assert_eq!(
        out.status.code(),
        Some(EXIT_INCOMPLETE),
        "der Stapel meldet „alles gut“:\n{zusammenfassung}\n{}",
        stderr(&out)
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Ein echter Fehlschlag schlägt die Deckungslücke: beides in einer Zahl geht
/// nicht, und „eine Datei ließ sich gar nicht verarbeiten“ ist dringender.
#[test]
fn a_real_failure_outranks_an_incomplete_inspection() {
    let dir = workdir("stapel-fehler");
    pdf(&dir, "a-lueckenhaft.pdf", Zusatz::XObjectOhneSubtype);
    std::fs::write(dir.join("b-kaputt.pdf"), b"das ist kein PDF").unwrap();

    let out = run(&[dir.to_str().unwrap(), "--patterns", "iban_de"]);
    assert_eq!(
        out.status.code(),
        Some(EXIT_ERROR),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&out),
        stderr(&out)
    );
    // Die Lücke geht dabei nicht verloren, sie steht in der Zusammenfassung.
    assert!(
        stdout(&out).contains("1 verarbeitet (aber nicht vollständig geprüft), 1 fehlgeschlagen"),
        "{}",
        stdout(&out)
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// `--json` nennt die betroffene Datei beim Namen — der Rückgabewert ist eine
/// Zahl für den ganzen Stapel, ein Skript braucht die Liste.
#[test]
fn the_json_summary_marks_the_incomplete_files() {
    let dir = workdir("json");
    pdf(&dir, "a-harmlos.pdf", Zusatz::Nichts);
    pdf(&dir, "b-lueckenhaft.pdf", Zusatz::XObjectOhneSubtype);

    let out = run(&[dir.to_str().unwrap(), "--patterns", "iban_de", "--json"]);
    let entries: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("JSON parsebar");
    let entries = entries.as_array().expect("Liste");
    assert_eq!(entries.len(), 2);

    let unvollstaendig: Vec<&str> = entries
        .iter()
        .filter(|e| e["incomplete"] == serde_json::json!(true))
        .map(|e| e["input"].as_str().unwrap())
        .collect();
    assert_eq!(unvollstaendig.len(), 1, "{entries:#?}");
    assert!(
        unvollstaendig[0].ends_with("b-lueckenhaft.pdf"),
        "{unvollstaendig:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// 3. Der Hilfetext nennt alle Rückgabewerte
// ---------------------------------------------------------------------------

#[test]
fn the_help_documents_every_exit_code() {
    let help = stdout(&run(&["--help"]));
    assert!(help.contains("Rückgabewerte:"), "{help}");
    for (code, stichwort) in [
        (EXIT_OK, "vollständig durchsucht"),
        (EXIT_ERROR, "Fehlgeschlagen"),
        (2, "Bedienfehler"),
        (EXIT_INCOMPLETE, "nicht vollständig geprüft"),
    ] {
        let zeile = help
            .lines()
            .skip_while(|l| !l.starts_with("Rückgabewerte:"))
            .find(|l| l.trim_start().starts_with(&format!("{code}  ")))
            .unwrap_or_else(|| panic!("Rückgabewert {code} fehlt im Hilfetext:\n{help}"));
        assert!(
            help.contains(stichwort),
            "Rückgabewert {code} ({zeile}) wird nicht erklärt:\n{help}"
        );
    }
}
