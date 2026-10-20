//! Experten-Review (Oktober 2026): Befunde an Rückgabewert und Ausgabenamen.
//!
//! * CLI-1: Ob eine Warnung eine Deckungslücke ist, entscheiden Textmarken.
//!   Ein Ressourcenname aus der Datei, der eine Marke trug, tarnte die Lücke
//!   als Mitteilung — Rückgabewert 0, die IBAN blieb stehen.
//! * CLI-2/CLI-3: zwei verschiedene Eingaben mit derselben Ausgabe; mit
//!   `--force` überschrieb das zweite Ergebnis still das erste.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lopdf::{dictionary, Document, Object, Stream};

const IBAN: &str = "DE89 3704 0044 0532 0130 00";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("experten-review-cli-{}-{name}", std::process::id()));
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

/// Dokument mit einem XObject ohne /Subtype (eine Deckungslücke), das unter
/// dem Ressourcennamen `xname` steht.
fn pdf(dir: &Path, name: &str, xname: &str, xname_in_content: &str) -> PathBuf {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1",
        "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
    });
    let fx_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 20.into()],
        },
        format!("BT /F1 10 Tf 0 0 Td (Fx: {IBAN}) Tj ET").into_bytes(),
    ));
    let mut xobjects = lopdf::Dictionary::new();
    xobjects.set(xname.as_bytes().to_vec(), Object::Reference(fx_id));
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
        "XObject" => xobjects,
    });
    let content = format!(
        "BT\n/F1 10 Tf\n72 700 Td\n(IBAN: {IBAN}) Tj\nET\nq 1 0 0 1 72 650 cm /{xname_in_content} Do Q\n"
    );
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "Contents" => content_id, "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let path = dir.join(name);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Gegenprobe: mit gewöhnlichem Namen endet derselbe Lauf mit 3.
#[test]
fn ein_gewoehnlicher_name_endet_mit_rueckgabewert_3() {
    let dir = workdir("kontrolle");
    let input = pdf(&dir, "in.pdf", "Fx", "Fx");
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("o.pdf").to_str().unwrap(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// CLI-1: heißt das XObject „enthalten Rasterbilder“ (eine Ausnahme-Marke),
/// bleibt die Lücke trotzdem eine Lücke — vorher Rückgabewert 0, „Warnung:“
/// statt „NICHT GEPRÜFT“, und die IBAN im XObject stand in der Ausgabe.
#[test]
fn ein_name_mit_ausnahme_marke_tarnt_keine_luecke() {
    let dir = workdir("tarnung");
    let input = pdf(
        &dir,
        "in.pdf",
        "enthalten Rasterbilder",
        "enthalten#20Rasterbilder",
    );
    let output = dir.join("o.pdf");
    let out = run(&[input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("hat kein bekanntes /Subtype"),
        "Lücke gar nicht gemeldet: {err}"
    );
    // Die IBAN steht im XObject-Strom unverändert in der Ausgabe.
    let check = run(&[output.to_str().unwrap(), "--check-leaks", "Fx: DE89"]);
    assert_eq!(
        check.status.code(),
        Some(3),
        "Leck-Kontrolle: {}",
        String::from_utf8_lossy(&check.stdout)
    );
    assert_eq!(
        out.status.code(),
        Some(3),
        "Deckungslücke endet mit {:?}; stderr:\n{err}",
        out.status.code()
    );
}

/// CLI-2: der Ausgabename wurde über `to_string_lossy` gebildet. Zwei
/// Eingaben, deren Namen sich nur in Nicht-UTF-8-Bytes unterscheiden, bekamen
/// denselben Ausgabepfad `\u{FFFD}_geschwaerzt.pdf`; mit `--force` überschrieb
/// das zweite Ergebnis still das erste, Rückgabewert 0.
#[cfg(unix)]
#[test]
fn nicht_utf8_namen_bekommen_je_eine_ausgabe() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let dir = workdir("nicht-utf8");
    let a = dir.join(OsStr::from_bytes(b"\xff.pdf"));
    let b = dir.join(OsStr::from_bytes(b"\xfe.pdf"));
    for p in [&a, &b] {
        let out = Command::new(bin())
            .arg("--write-demo")
            .arg(p.as_os_str())
            .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let out = Command::new(bin())
        .arg(dir.as_os_str())
        .arg("--force")
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .output()
        .unwrap();
    let outputs: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .filter(|n| n.to_string_lossy().contains("_geschwaerzt"))
        .collect();
    assert_eq!(
        outputs.len(),
        2,
        "zwei Eingaben, aber Ausgaben {outputs:?}; exit {:?}\nstdout:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
    );
}

/// CLI-3: `a` (ohne Endung) und `a.pdf` ergeben beide `a_geschwaerzt.pdf`.
/// Mit `--force` überschrieb das zweite Ergebnis das erste still,
/// Rückgabewert 0. Jetzt lehnt der Stapel das vor der ersten Datei ab.
#[test]
fn zwei_eingaben_mit_derselben_ausgabe_werden_abgelehnt() {
    let dir = workdir("ohne-endung");
    let a = dir.join("a");
    let b = dir.join("a.pdf");
    for p in [&a, &b] {
        let out = run(&["--write-demo", p.to_str().unwrap()]);
        assert!(out.status.success());
    }
    let out = run(&[a.to_str().unwrap(), b.to_str().unwrap(), "--force"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("dieselbe Ausgabe"), "{stderr}");
    assert!(
        !dir.join("a_geschwaerzt.pdf").exists(),
        "es wurde trotzdem geschrieben"
    );
}
