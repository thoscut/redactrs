//! Gegenprüfung J-B, Linse **FALSCHER ALARM**: nimmt die Nachbesserung von
//! Register #20 einem gewöhnlichen Dokument etwas weg, das es behalten muss?
//!
//! Die Nachbesserung tut zwei Dinge, die in diese Richtung wirken können:
//!
//! 1. Sie führt `ImagePlacement::covers` ein und entscheidet die Bildfrage an
//!    der **Fläche** statt an der Hülle. Das ist die Richtung *weniger* Alarm —
//!    geprüft wird hier eine **Scherung** (ein schräg gesetzter Stempel,
//!    `100 0 60 100 cm`) und eine **entartete** Platzierung (`0 0 0 0 cm`, wie
//!    sie kaputte Erzeuger schreiben), also zwei Formen, die der 45°-Test von
//!    Agent B nicht abdeckt.
//! 2. Sie fragt über `touches_form_image` / `DeferredMirror::touched_by_image`
//!    **je Formular** „verliert es Bildpunkte?“. Diese Frage kennt den
//!    Operationsindex des Formulars, nicht die Platzierung — sie kann also zu
//!    weit greifen. Geprüft wird deshalb, dass ein **zweiter** Abschnitt neben
//!    dem getroffenen seinen Spiegel behält: einmal innerhalb desselben
//!    Formulars, einmal über einem zweiten Formular.
//!
//! Gemessen wird am Leck-Orakel [`redact_pdf::leaks`] und an der geladenen
//! Ausgabe, nicht am Bericht.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfRedactor, RedactionReport};

/// Spiegel über dem Bild, dessen Pixel fallen.
const SPIEGEL_A: &str = "Kontoauszug, IBAN DE89 3704 0044 0532 0130 00";
/// Spiegel über dem Bild, das nichts verliert.
const SPIEGEL_B: &str = "Organigramm der Fachabteilung";
/// Ersatztext am Dictionary des getroffenen Bildes.
const BILD_A_ALT: &str = "Scan des Kontoauszugs";
/// Ersatztext am Dictionary des unberührten Bildes.
const BILD_B_ALT: &str = "Firmenlogo der Musterbank";

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

fn bild(width: u32, height: u32, alt: &str) -> Stream {
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            data.extend_from_slice(&[(x * 11 + 3) as u8, (y * 7 + 5) as u8, 0x40]);
        }
    }
    let mut stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(width),
            "Height" => i64::from(height),
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Alt" => Object::string_literal(alt),
        },
        data,
    );
    let _ = stream.compress();
    stream
}

fn eine_seite(mut doc: Document, resources_id: ObjectId, content: Vec<u8>) -> (Document, ObjectId) {
    let content_id = doc.add_object(Stream::new(dictionary! {}, content));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, page_id)
}

fn schwaerzung(page: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "Gegenprüfung J-B".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Ohne `padding`: das Rechteck ist genau das, was der Test sagt.
fn schwaerze_mit(
    doc: &mut Document,
    list: &[Redaction],
    allow_undecodable: bool,
) -> (RedactionReport, Vec<u8>) {
    let report = PdfRedactor::with_padding(0.0)
        .allowing_undecodable_images(allow_undecodable)
        .apply_with_report(doc, list)
        .expect("Schwärzung läuft");
    let bytes = save_to_bytes(doc).expect("Speichern");
    (report, bytes)
}

fn alt_am_objekt(doc: &Document, id: ObjectId) -> Option<String> {
    let dict: &Dictionary = match doc.get_object(id).ok()? {
        Object::Stream(stream) => &stream.dict,
        Object::Dictionary(dict) => dict,
        _ => return None,
    };
    dict.get(b"Alt")
        .ok()
        .and_then(|value| value.as_str().ok())
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

/// Der OCR-Hinweis steht in jedem Lauf mit einem Bild. Alles andere wäre eine
/// neue Warnung.
fn nur_ocr_hinweis(report: &RedactionReport) -> bool {
    report
        .warnings
        .iter()
        .all(|w| w.contains("enthalten Rasterbilder"))
}

/// Ein Formular mit **zwei** getaggten Bildern: `Im0` bei (50,600)-(150,700),
/// `Im1` bei (300,600)-(400,700), jedes in seinem eigenen `/Figure`-Abschnitt.
fn formular_mit_zwei_bildern(doc: &mut Document) -> (ObjectId, ObjectId, ObjectId) {
    let im0 = doc.add_object(Object::Stream(bild(20, 20, BILD_A_ALT)));
    let im1 = doc.add_object(Object::Stream(bild(20, 20, BILD_B_ALT)));
    let content = format!(
        "/Figure <</Alt ({SPIEGEL_A})>> BDC\nq 100 0 0 100 50 600 cm /Im0 Do Q\nEMC\n\
         /Figure <</Alt ({SPIEGEL_B})>> BDC\nq 100 0 0 100 300 600 cm /Im1 Do Q\nEMC\n"
    );
    let form = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! {
                    "XObject" => dictionary! { "Im0" => im0, "Im1" => im1 },
                },
            },
            content.into_bytes(),
        )
        .with_compression(false),
    ));
    (form, im0, im1)
}

/// Ein Formular mit genau einem getaggten Bild an `(x,600)-(x+100,700)`.
fn formular_mit_einem_bild(doc: &mut Document, x: f64, alt: &str) -> (ObjectId, ObjectId) {
    let im = doc.add_object(Object::Stream(bild(20, 20, alt)));
    let form = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im" => im } },
            },
            format!("q 100 0 0 100 {x} 600 cm /Im Do Q\n").into_bytes(),
        )
        .with_compression(false),
    ));
    (form, im)
}

/// Mitten in `Im0` bei (50,600)-(150,700).
fn ueber_bild_a() -> Rect {
    Rect::new(80.0, 640.0, 110.0, 670.0)
}

// ---------------------------------------------------------------------------
// 1. Der Nachbar im selben Formular
// ---------------------------------------------------------------------------

/// Zwei `/Figure`-Abschnitte in **einem** Formular, jeder über seinem eigenen
/// Bild; geschwärzt wird nur das erste. Der zweite Abschnitt beschreibt ein
/// Bild, das keinen Bildpunkt verliert — sein Spiegel und der Ersatztext an
/// seinem Dictionary müssen bleiben.
///
/// Das ist die Gegenprobe zu `touches_form_image`: die Frage „verliert dieses
/// Formular Bildpunkte?“ kennt nur die Objekt-Id, nicht den Abschnitt. Würde
/// sie auch für die **eigenen** Abschnitte des Formulars gestellt (statt der
/// operationsgenauen Spanne `image_hits`), fiele hier beides.
///
/// Gefahren in beiden Betriebsarten, weil `clear_image_alternates` nur mit
/// `--allow-undecodable-images` überhaupt gefüttert wird.
#[test]
fn nachbarabschnitt_im_selben_formular_behaelt_seinen_spiegel() {
    for allow_undecodable in [false, true] {
        let mut doc = Document::with_version("1.5");
        let (form, im0, im1) = formular_mit_zwei_bildern(&mut doc);
        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { "Fm0" => form },
        });
        let (mut doc, _) = eine_seite(doc, resources_id, b"q /Fm0 Do Q\n".to_vec());
        let (report, out) = schwaerze_mit(
            &mut doc,
            &[schwaerzung(0, ueber_bild_a())],
            allow_undecodable,
        );

        assert_eq!(
            report.redacted_images, 1,
            "Vorbedingung: genau ein Bild verliert Pixel — {:?}",
            report.warnings
        );
        assert!(
            nur_ocr_hinweis(&report),
            "keine neue Warnung: {:?}",
            report.warnings
        );
        assert_eq!(
            report.image_alt_texts_cleared, 1,
            "genau der getroffene Abschnitt zählt (allow_undecodable = {allow_undecodable})"
        );
        // Was fallen muss.
        assert!(
            leaks(&out, SPIEGEL_A).is_empty(),
            "der Spiegel über dem geschwärzten Bild: {:?}",
            leaks(&out, SPIEGEL_A)
        );
        assert!(
            leaks(&out, BILD_A_ALT).is_empty(),
            "der Ersatztext am geschwärzten Bild: {:?}",
            leaks(&out, BILD_A_ALT)
        );
        // Was bleiben muss.
        assert!(
            !leaks(&out, SPIEGEL_B).is_empty(),
            "der Spiegel über dem unversehrten Nachbarbild ist weg \
             (allow_undecodable = {allow_undecodable})"
        );
        let aus = load_from_bytes(&out).expect("die Ausgabe lädt");
        assert_eq!(
            alt_am_objekt(&aus, im1).as_deref(),
            Some(BILD_B_ALT),
            "das unversehrte Nachbarbild behält sein /Alt \
             (allow_undecodable = {allow_undecodable})"
        );
        // Und das Formular ist noch ein brauchbares Formular.
        assert!(
            matches!(aus.get_object(form), Ok(Object::Stream(_))),
            "das neu geschriebene Formular ist noch ein Strom"
        );
        let _ = im0;
    }
}

// ---------------------------------------------------------------------------
// 2. Das Nachbarformular
// ---------------------------------------------------------------------------

/// Zwei Abschnitte im **Seitenstrom**, jeder über einem eigenen Formular mit
/// eigenem Bild; geschwärzt wird nur das erste Formular. Der zweite Spiegel
/// muss bleiben — `touches_form_image` fragt die Formulare im Geltungsbereich
/// *dieses* Abschnitts, nicht die des Dokuments.
#[test]
fn nachbarabschnitt_ueber_zweitem_formular_behaelt_seinen_spiegel() {
    let mut doc = Document::with_version("1.5");
    let (fm0, _im0) = formular_mit_einem_bild(&mut doc, 50.0, BILD_A_ALT);
    let (fm1, im1) = formular_mit_einem_bild(&mut doc, 300.0, BILD_B_ALT);
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => fm0, "Fm1" => fm1 },
    });
    let content = format!(
        "/Figure <</Alt ({SPIEGEL_A})>> BDC\nq /Fm0 Do Q\nEMC\n\
         /Figure <</Alt ({SPIEGEL_B})>> BDC\nq /Fm1 Do Q\nEMC\n"
    );
    let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
    let (report, out) = schwaerze_mit(&mut doc, &[schwaerzung(0, ueber_bild_a())], true);

    assert_eq!(
        report.redacted_images, 1,
        "Vorbedingung: genau ein Bild verliert Pixel — {:?}",
        report.warnings
    );
    assert!(
        nur_ocr_hinweis(&report),
        "keine neue Warnung: {:?}",
        report.warnings
    );
    assert_eq!(report.image_alt_texts_cleared, 1, "genau ein Abschnitt");
    assert!(
        leaks(&out, SPIEGEL_A).is_empty(),
        "der Spiegel über dem geschwärzten Formular: {:?}",
        leaks(&out, SPIEGEL_A)
    );
    assert!(
        !leaks(&out, SPIEGEL_B).is_empty(),
        "der Spiegel über dem unbeteiligten Formular ist weg"
    );
    let aus = load_from_bytes(&out).expect("die Ausgabe lädt");
    assert_eq!(
        alt_am_objekt(&aus, im1).as_deref(),
        Some(BILD_B_ALT),
        "das Bild im unbeteiligten Formular behält sein /Alt"
    );
}

// ---------------------------------------------------------------------------
// 3. Die Scherung — eine schräge Abbildung, die keine Drehung ist
// ---------------------------------------------------------------------------

/// Ein **geschertes** Bild: `100 0 60 100 200 500 cm`. Das Viereck ist das
/// Parallelogramm
///
/// ```text
/// (200,500) (300,500) (360,600) (260,600)
/// ```
///
/// mit der Hülle (200,500)-(360,600). Die rechte untere Ecke der Hülle ist
/// leer: bei `y = 509` reicht das Bild nur bis `x = 305.4`. Das Rechteck
/// (320,501)-(350,509) liegt also **in der Hülle und außerhalb der Fläche** —
/// dort steht in einem gewöhnlichen Dokument Text, und `crate::image` fasst das
/// Bild richtigerweise nicht an.
///
/// Mutation, die diesen Test rot macht: in `redact.rs` `placement.covers(rect)`
/// durch `placement.bounds.intersects(rect)` ersetzen.
#[test]
fn geschertes_bild_behaelt_in_der_leeren_huellenecke_alles() {
    for allow_undecodable in [false, true] {
        let mut doc = Document::with_version("1.5");
        let im0 = doc.add_object(Object::Stream(bild(32, 32, BILD_B_ALT)));
        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { "Im0" => im0 },
        });
        let content = format!(
            "/Figure <</Alt ({SPIEGEL_B})>> BDC\nq 100 0 60 100 200 500 cm /Im0 Do Q\nEMC\n"
        );
        let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
        let (report, out) = schwaerze_mit(
            &mut doc,
            &[schwaerzung(0, Rect::new(320.0, 501.0, 350.0, 509.0))],
            allow_undecodable,
        );
        assert_eq!(
            report.redacted_images, 0,
            "Vorbedingung: kein Bildpunkt fällt (allow_undecodable = {allow_undecodable}) — {:?}",
            report.warnings
        );
        assert!(
            nur_ocr_hinweis(&report),
            "keine Warnung: {:?}",
            report.warnings
        );
        assert_eq!(
            report.image_alt_texts_cleared, 0,
            "nichts fällt, also zählt nichts"
        );
        assert!(
            !leaks(&out, SPIEGEL_B).is_empty(),
            "der Spiegel über dem unversehrten gescherten Bild ist weg \
             (allow_undecodable = {allow_undecodable})"
        );
        let aus = load_from_bytes(&out).expect("die Ausgabe lädt");
        assert_eq!(
            alt_am_objekt(&aus, im0).as_deref(),
            Some(BILD_B_ALT),
            "das unversehrte gescherte Bild behält sein /Alt \
             (allow_undecodable = {allow_undecodable})"
        );
    }
}

/// Die Gegenrichtung, damit der Test darüber nicht aus dem falschen Grund grün
/// wird: dasselbe gescherte Bild, das Rechteck jetzt **mitten** im
/// Parallelogramm.
#[test]
fn geschertes_bild_mitten_getroffen_verliert_alles() {
    let mut doc = Document::with_version("1.5");
    let im0 = doc.add_object(Object::Stream(bild(32, 32, BILD_A_ALT)));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    let content =
        format!("/Figure <</Alt ({SPIEGEL_A})>> BDC\nq 100 0 60 100 200 500 cm /Im0 Do Q\nEMC\n");
    let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
    let (report, out) = schwaerze_mit(
        &mut doc,
        &[schwaerzung(0, Rect::new(270.0, 540.0, 300.0, 570.0))],
        true,
    );
    assert_eq!(
        report.redacted_images, 1,
        "Vorbedingung: Bildpunkte fallen — {:?}",
        report.warnings
    );
    assert_eq!(report.image_alt_texts_cleared, 1);
    assert!(
        leaks(&out, SPIEGEL_A).is_empty(),
        "{:?}",
        leaks(&out, SPIEGEL_A)
    );
    assert!(
        leaks(&out, BILD_A_ALT).is_empty(),
        "{:?}",
        leaks(&out, BILD_A_ALT)
    );
}

// ---------------------------------------------------------------------------
// 4. Die entartete Platzierung
// ---------------------------------------------------------------------------

/// `q 0 0 0 0 100 650 cm /Im0 Do Q` — eine Platzierung mit nicht umkehrbarer
/// CTM. Das schreiben kaputte Erzeuger; gezeichnet wird dabei nichts, also darf
/// auch nichts weggenommen werden. Die Schwärzung liegt genau auf dem Punkt, zu
/// dem das Einheitsquadrat zusammenfällt.
///
/// Geprüft wird zugleich, dass der Lauf das gewöhnliche Material **annimmt**:
/// keine Ablehnung, keine Warnung, eine ladbare Ausgabe.
#[test]
fn entartete_platzierung_nimmt_niemandem_etwas() {
    for allow_undecodable in [false, true] {
        let mut doc = Document::with_version("1.5");
        let im0 = doc.add_object(Object::Stream(bild(8, 8, BILD_B_ALT)));
        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { "Im0" => im0 },
        });
        let content =
            format!("/Figure <</Alt ({SPIEGEL_B})>> BDC\nq 0 0 0 0 100 650 cm /Im0 Do Q\nEMC\n");
        let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
        let (report, out) = schwaerze_mit(
            &mut doc,
            &[schwaerzung(0, Rect::new(90.0, 640.0, 110.0, 660.0))],
            allow_undecodable,
        );
        assert_eq!(
            report.redacted_images, 0,
            "eine entartete Platzierung zeichnet nichts — {:?}",
            report.warnings
        );
        assert!(
            nur_ocr_hinweis(&report),
            "keine Warnung über gewöhnliches Material: {:?}",
            report.warnings
        );
        assert_eq!(report.image_alt_texts_cleared, 0);
        assert!(
            !leaks(&out, SPIEGEL_B).is_empty(),
            "der Spiegel über der entarteten Platzierung ist weg \
             (allow_undecodable = {allow_undecodable})"
        );
        let aus = load_from_bytes(&out).expect("die Ausgabe lädt");
        assert_eq!(
            alt_am_objekt(&aus, im0).as_deref(),
            Some(BILD_B_ALT),
            "das nie gezeichnete Bild behält sein /Alt"
        );
    }
}

// ---------------------------------------------------------------------------
// 5. Die Zahl im Bericht: geteilte Eigenschaftsliste
// ---------------------------------------------------------------------------

/// **Nebenbefund, kein Alarm an der Datei.** `image_alt_texts_cleared` verspricht
/// „**entfernte Schlüssel**, nicht Absichten“ (Feldkommentar an
/// `RedactionReport::image_alt_texts_cleared`). Gezählt wird aber je Abschnitt.
/// Zwei Abschnitte, die sich **eine** Eigenschaftsliste als eigenes Objekt
/// teilen und beide über einer geschwärzten Bildfläche liegen, ergeben deshalb
/// die Zahl 2 — entfernt wird der eine Schlüssel des einen Objekts genau
/// **einmal**.
///
/// Der Eingriff an der Datei ist richtig (die geteilte Liste ist ein geteilter
/// Spiegel, und sie fällt ganz); nur die Zahl ist zu groß. Dieser Test hält
/// fest, was gilt, damit die Zahl nicht unbemerkt etwas anderes behauptet.
#[test]
fn geteilte_eigenschaftsliste_wird_zweimal_gezaehlt_aber_einmal_geleert() {
    let mut doc = Document::with_version("1.5");
    let im0 = doc.add_object(Object::Stream(bild(20, 20, BILD_A_ALT)));
    let im1 = doc.add_object(Object::Stream(bild(20, 20, BILD_B_ALT)));
    let liste = doc.add_object(dictionary! { "Alt" => Object::string_literal(SPIEGEL_A) });
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0, "Im1" => im1 },
        "Properties" => dictionary! { "MC0" => liste },
    });
    let content = "/Figure /MC0 BDC\nq 100 0 0 100 50 600 cm /Im0 Do Q\nEMC\n\
                   /Figure /MC0 BDC\nq 100 0 0 100 300 600 cm /Im1 Do Q\nEMC\n";
    let (mut doc, _) = eine_seite(doc, resources_id, content.as_bytes().to_vec());
    let (report, out) = schwaerze_mit(
        &mut doc,
        &[
            schwaerzung(0, ueber_bild_a()),
            schwaerzung(0, Rect::new(330.0, 640.0, 360.0, 670.0)),
        ],
        true,
    );
    assert_eq!(
        report.redacted_images, 2,
        "Vorbedingung: beide Bilder verlieren Pixel — {:?}",
        report.warnings
    );
    // Der Eingriff: die geteilte Liste ist leer, der Klartext ist aus der Datei.
    assert!(
        leaks(&out, SPIEGEL_A).is_empty(),
        "die geteilte Liste muss fallen: {:?}",
        leaks(&out, SPIEGEL_A)
    );
    let aus = load_from_bytes(&out).expect("die Ausgabe lädt");
    assert_eq!(
        alt_am_objekt(&aus, liste),
        None,
        "genau ein Schlüssel wurde entfernt — das Objekt hat nur einen"
    );
    // Die Zahl: zwei Abschnitte, ein entfernter Schlüssel.
    assert_eq!(
        report.image_alt_texts_cleared, 2,
        "zwei Abschnitte werden gezählt, obwohl nur ein Schlüssel fiel — \
         wird das je richtig gezählt, ist DIESE Erwartung anzupassen"
    );
}
