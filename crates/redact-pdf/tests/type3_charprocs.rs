//! #73 — eine Type3-Glyphprozedur, die selbst Text setzt.
//!
//! Ein Type3-Font hat kein Fontprogramm; jede Glyphe **ist** ein
//! Content-Stream unter `/CharProcs`. Üblicherweise malt der nur Striche und
//! Flächen — dann ist alles in Ordnung. Er darf aber auch `BT … Tj`
//! enthalten, also mit einem anderen Font Klartext setzen.
//!
//! Dann geht die Schwärzung ins Leere, ohne es zu merken: der Zeichencode
//! verschwindet sauber aus dem Seitenstrom (die Glyphe ist danach unsichtbar),
//! der Klartext steht aber weiter im Prozedurstrom. Der Interpreter betritt
//! `/CharProcs` nicht — für ihn ist die Seite sauber, der Befund lautet
//! „angewendet“, und niemand erfährt etwas.
//!
//! Gemessen wird mit [`leaks`], nicht mit dem eigenen Extraktor.

mod common;

use lopdf::{dictionary, Dictionary, Object, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfRedactor, RedactionReport};

use common::SECRET;

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

/// Baut eine Seite mit einem Type3-Font, dessen Glyphen `procs` malen.
///
/// `codes` sind die Zeichen, die die Seite dann setzt (ein Byte je Glyphe, ab
/// `'a'`). Die Glyphe ist 10 pt breit und wird bei (72,700) gesetzt.
fn page_with_type3(procs: &[&[u8]], codes: &str) -> Vec<u8> {
    let mut d = common::page(&[]);
    let helvetica = d.font_id;

    let mut char_procs = Dictionary::new();
    let mut differences: Vec<Object> = vec![Object::Integer(b'a' as i64)];
    for (i, proc_content) in procs.iter().enumerate() {
        let name = format!("g{i}");
        let id = d.add(Object::Stream(
            Stream::new(dictionary! {}, proc_content.to_vec()).with_compression(false),
        ));
        char_procs.set(name.as_bytes().to_vec(), Object::Reference(id));
        differences.push(Object::Name(name.into_bytes()));
    }

    let widths: Vec<Object> = procs.iter().map(|_| Object::Integer(1000)).collect();
    let font_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type3",
        "FontBBox" => vec![0.into(), 0.into(), 1000.into(), 1000.into()],
        "FontMatrix" => vec![
            Object::Real(0.001), Object::Real(0.0), Object::Real(0.0),
            Object::Real(0.001), Object::Real(0.0), Object::Real(0.0),
        ],
        "CharProcs" => char_procs,
        "Encoding" => dictionary! {
            "Type" => "Encoding",
            "Differences" => Object::Array(differences),
        },
        "FirstChar" => b'a' as i64,
        "LastChar" => (b'a' as usize + procs.len() - 1) as i64,
        "Widths" => Object::Array(widths),
        // Der Prozedurstrom braucht selbst Ressourcen, wenn er Text setzt.
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => helvetica } },
    }));

    let fonts = dictionary! { "F1" => helvetica, "T3" => font_id };
    d.doc
        .get_dictionary_mut(d.resources_id)
        .expect("Resources")
        .set("Font", fonts);

    d.set_content(format!("BT\n/T3 10 Tf\n72 700 Td\n({codes}) Tj\nET\n").as_bytes());
    d.finish()
}

/// Eine Glyphprozedur, die brav nur malt.
fn drawing_proc() -> Vec<u8> {
    b"1000 0 0 0 1000 1000 d1\n0 0 800 800 re f\n".to_vec()
}

/// Eine Glyphprozedur, die selbst Text setzt — mit einem anderen Font.
fn text_setting_proc() -> Vec<u8> {
    format!("1000 0 0 0 1000 1000 d1\nBT\n/F1 100 Tf\n0 0 Td\n(IBAN: {SECRET}) Tj\nET\n")
        .into_bytes()
}

/// Deckt die gesetzten Glyphen großzügig ab.
fn over_the_glyphs() -> Redaction {
    Redaction::new(
        Region::new(
            0,
            Rect::new(60.0, 690.0, 200.0, 715.0),
            None,
            Source::Manual {
                reason: "#73".into(),
            },
        ),
        Action::Blackout,
    )
}

fn pipeline(bytes: &[u8]) -> (RedactionReport, Vec<u8>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[over_the_glyphs()])
        .expect("Schwärzung");
    (report, save_to_bytes(&doc).expect("Speichern"))
}

/// Warnt der Bericht über Glyphprozeduren, die selbst Text setzen?
fn warns_about_charprocs(report: &RedactionReport) -> bool {
    report.warnings.iter().any(|w| w.contains("CharProcs"))
}

// ---------------------------------------------------------------------------
// Der offene Fall
// ---------------------------------------------------------------------------

/// Sichtbar ist der Text weg, in der Datei steht er weiter — und das muss
/// gesagt werden. Ein Befund „angewendet“ ohne jede Warnung wäre eine Lüge.
#[test]
fn a_glyph_procedure_that_sets_text_is_not_passed_over_in_silence() {
    let pdf = page_with_type3(&[&text_setting_proc()], "a");
    assert!(
        !leaks(&pdf, SECRET).is_empty(),
        "Vorbedingung: das Geheimnis steht in der Glyphprozedur"
    );

    let (report, out) = pipeline(&pdf);

    // Die Glyphe selbst verschwindet aus dem Seitenstrom — das funktioniert.
    assert_eq!(
        report.removed_glyphs, 1,
        "Vorbedingung: der Zeichencode wird aus dem Seitenstrom entfernt"
    );
    assert!(
        warns_about_charprocs(&report),
        "keine Warnung — der Klartext steht {} mal weiter in der Datei, und der \
         Befund heißt trotzdem „angewendet“. Warnungen: {:?}",
        leaks(&out, SECRET).len(),
        report.warnings
    );
}

// ---------------------------------------------------------------------------
// Gegenprobe: der gewöhnliche Fall bleibt still
// ---------------------------------------------------------------------------

/// Sechs Glyphen, die nur malen: sechs entfernte Zeichen, kein Leck — und
/// **keine** Warnung. Eine Warnung, die bei jedem Type3-Font käme, wäre
/// wertlos.
#[test]
fn ordinary_type3_glyphs_stay_quiet() {
    let procs: Vec<Vec<u8>> = (0..6).map(|_| drawing_proc()).collect();
    let refs: Vec<&[u8]> = procs.iter().map(|p| p.as_slice()).collect();
    let pdf = page_with_type3(&refs, "abcdef");

    let (report, out) = pipeline(&pdf);

    assert_eq!(report.removed_glyphs, 6, "{:?}", report.warnings);
    assert!(
        leaks(&out, SECRET).is_empty(),
        "hier steht das Geheimnis gar nicht drin"
    );
    assert!(
        !warns_about_charprocs(&report),
        "Fehlalarm bei einem gewöhnlichen Type3-Font: {:?}",
        report.warnings
    );
}

/// Und ein Type3-Font, dessen Glyphen gar nicht gesetzt werden, ist kein
/// Befund: was nie gezeichnet wird, kann auch keine Schwärzung verfehlen.
#[test]
fn an_unused_type3_font_is_not_reported() {
    let mut pdf_source = page_with_type3(&[&text_setting_proc()], "a");
    // Dieselbe Datei, aber die Seite setzt nichts.
    {
        let mut doc = load_from_bytes(&pdf_source).expect("PDF ladbar");
        let page_id = *doc.get_pages().values().next().expect("Seite");
        let content_id = match doc.get_dictionary(page_id).and_then(|d| d.get(b"Contents")) {
            Ok(Object::Reference(id)) => *id,
            _ => unreachable!("Seiteninhalt ist ein Verweis"),
        };
        doc.objects.insert(
            content_id,
            Object::Stream(Stream::new(dictionary! {}, b"q Q\n".to_vec())),
        );
        pdf_source = save_to_bytes(&doc).expect("Speichern");
    }

    let mut doc = load_from_bytes(&pdf_source).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[over_the_glyphs()])
        .expect("Schwärzung");
    assert!(
        !warns_about_charprocs(&report),
        "Warnung über einen Font, den die Seite gar nicht benutzt: {:?}",
        report.warnings
    );
}
