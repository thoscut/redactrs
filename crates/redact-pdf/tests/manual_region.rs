//! Der GUI-Arbeitsablauf: ein von Hand gezogenes Rechteck in **echten**
//! Koordinaten.
//!
//! ## Der geprüfte Befund
//!
//! Gemeldet war: eine manuell gezogene Region entferne die falschen Zeichen.
//! `hidden_flags` wähle die Glyphen über *geschätzte* Rechtecke aus, während
//! die GUI-/JSON-Region in echten Koordinaten liege; Folge sei ein richtig
//! sitzendes schwarzes Rechteck, gelöschte unbeteiligte Zeichen und ein
//! Zieltext, der teilweise im Strom stehen bleibt.
//!
//! ## Wie hier gemessen wird
//!
//! Damit der Test nicht dieselbe Schätzung prüft, die er in Frage stellt, wird
//! **jedes Zeichen einzeln** auf ein festes Raster gesetzt (`grid_line`). Die
//! wahre Lage jedes Zeichens ergibt sich dann aus der Konstruktion — `x0 + i ·
//! Rasterweite` — und nicht aus einer Fontmetrik. Der Schwärzungsbereich wird
//! aus genau diesen Konstruktionsdaten gerechnet, so wie ein Nutzer ihn über
//! die sichtbaren Zeichen ziehen würde.
//!
//! Orakel ist [`redact_pdf::leaks`]: das Geheimnis muss aus der *Datei*
//! verschwinden, die Nachbarschaft darin bleiben.

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, save_to_bytes, PdfRedactor};

const SECRET: &str = "4711000";
const BEFORE: &str = "Konto ";
const AFTER: &str = " Ende";

/// Rasterweite und Schriftgröße. Die Weite liegt deutlich über dem natürlichen
/// Vorschub (Helvetica-Ziffer bei 10 pt: 5,56 pt), damit zwischen den Zellen
/// Luft bleibt und der Bereich eindeutig einer Zelle zuzuordnen ist.
const PITCH: f64 = 9.0;
const SIZE: f64 = 10.0;

/// Setzt jedes Zeichen einzeln auf das Raster; `matrix` ist der Drehanteil der
/// Textmatrix, `step` der Rastervorschub im User-Space.
fn grid_line(text: &str, x0: f64, y0: f64, matrix: &str, step: (f64, f64)) -> String {
    let mut out = format!("BT /F1 {SIZE} Tf\n");
    for (i, ch) in text.chars().enumerate() {
        let x = x0 + step.0 * i as f64;
        let y = y0 + step.1 * i as f64;
        out.push_str(&format!("{matrix} {x} {y} Tm ({ch}) Tj\n"));
    }
    out.push_str("ET\n");
    out
}

/// Waagerechte Rasterzeile bei (x0, y0).
fn line(text: &str, x0: f64, y0: f64) -> String {
    grid_line(text, x0, y0, "1 0 0 1", (PITCH, 0.0))
}

/// Der Bereich über den Zellen `range` einer waagerechten Rasterzeile.
///
/// Waagerecht von der linken Kante der ersten bis zur linken Kante der ersten
/// *nicht* mehr betroffenen Zelle, senkrecht von knapp unter der Grundlinie
/// bis knapp über die Versalhöhe — genau das, was ein Nutzer aufzieht.
fn cells(x0: f64, y0: f64, range: std::ops::Range<usize>) -> Rect {
    Rect::new(
        x0 + PITCH * range.start as f64,
        y0 - 1.0,
        x0 + PITCH * range.end as f64 - PITCH + SIZE * 0.556,
        y0 + SIZE * 0.72,
    )
}

fn doc_with(content: &str) -> (Document, ObjectId, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
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
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    (doc, page_id, resources_id)
}

fn manual(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
            rect,
            None,
            Source::Manual {
                reason: "von Hand gezogen".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Schwärzt, speichert und misst am Ergebnis.
#[track_caller]
fn assert_hits_only_the_target(what: &str, mut doc: Document, rect: Rect, keep: &[&str]) {
    let before = save_to_bytes(&doc).expect("Speichern");
    assert!(
        !leaks(&before, SECRET).is_empty(),
        "{what}: Vorbedingung — das Geheimnis steht in der Datei"
    );

    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[manual(rect)])
        .expect("Schwärzung");
    redact_pdf::strip_metadata(&mut doc);
    let after = save_to_bytes(&doc).expect("Speichern");

    let hits = leaks(&after, SECRET);
    assert!(
        hits.is_empty(),
        "{what}: das Geheimnis steht noch {} mal in der Datei ({} Zeichen entfernt):\n{}",
        hits.len(),
        report.removed_glyphs,
        hits.join("\n")
    );
    for needle in keep {
        assert!(
            !leaks(&after, needle).is_empty(),
            "{what}: „{needle}“ wurde mitentfernt, obwohl es außerhalb liegt"
        );
    }
}

/// Der Regelfall: eine Zeile, der Bereich über den Ziffern in der Mitte.
#[test]
fn a_hand_drawn_region_removes_exactly_the_characters_under_it() {
    let text = format!("{BEFORE}{SECRET}{AFTER}");
    let (doc, ..) = doc_with(&line(&text, 72.0, 700.0));
    let start = BEFORE.chars().count();
    let end = start + SECRET.chars().count();
    assert_hits_only_the_target(
        "eine Zeile",
        doc,
        cells(72.0, 700.0, start..end),
        &["Konto", "Ende"],
    );
}

/// Enger Zeilenabstand: der Bereich über Zeile 2 darf Zeile 1 und 3 nicht
/// mitnehmen. Bei 11 pt Zeilenabstand und 10 pt Schrift stoßen die
/// Glyphenkästen (Oberlänge 0,75 em, Unterlänge −0,22 em) beinahe aneinander —
/// hier entscheidet sich, ob die Auswahl zeilenscharf ist.
#[test]
fn tight_line_spacing_does_not_pull_in_the_neighbouring_lines() {
    let mut content = line("Zeile darueber ohne Geheimnis", 72.0, 711.0);
    content.push_str(&line(&format!("{BEFORE}{SECRET}{AFTER}"), 72.0, 700.0));
    content.push_str(&line("Zeile darunter ohne Geheimnis", 72.0, 689.0));
    let (doc, ..) = doc_with(&content);

    let start = BEFORE.chars().count();
    let end = start + SECRET.chars().count();
    assert_hits_only_the_target(
        "enger Zeilenabstand",
        doc,
        cells(72.0, 700.0, start..end),
        &["darueber", "darunter", "Konto", "Ende"],
    );
}

/// Die Kernbehauptung des Befunds: Muster-Treffer seien in Ordnung, weil Box
/// und Auswahl aus derselben Schätzung stammen — von Hand gezogene Bereiche
/// aber nicht, weil sie in echten Koordinaten liegen.
///
/// Hier stehen beide Wege nebeneinander an derselben Datei: einmal das
/// Rechteck, das die Extraktion aus ihren Glyphenkästen bildet, einmal eines
/// aus den Konstruktionsdaten des Rasters. Ergebnis und Zeichenzahl müssen
/// übereinstimmen.
#[test]
fn a_hand_drawn_region_and_a_pattern_hit_remove_the_same_characters() {
    let text = format!("{BEFORE}{SECRET}{AFTER}");
    let (doc, ..) = doc_with(&line(&text, 72.0, 700.0));

    // Weg 1: so, wie ein Muster-Treffer entsteht — aus den Glyphenkästen.
    let runs = redact_pdf::PdfExtractor::new()
        .extract(&doc)
        .expect("Extraktion");
    let run = runs
        .iter()
        .find(|r| r.text.contains(SECRET))
        .expect("die Zeile muss als eine Zeile erkannt werden");
    let pos = run.text.find(SECRET).expect("Fundstelle");
    let estimated = run
        .rect_for_byte_range(pos, pos + SECRET.len())
        .expect("Bounding-Box");

    // Weg 2: aus der Konstruktion des Rasters — echte Koordinaten.
    let start = BEFORE.chars().count();
    let end = start + SECRET.chars().count();
    let real = cells(72.0, 700.0, start..end);

    let count = |rect: Rect| {
        let mut copy = doc.clone();
        PdfRedactor::new()
            .apply_with_report(&mut copy, &[manual(rect)])
            .expect("Schwärzung")
            .removed_glyphs
    };
    assert_eq!(
        count(estimated),
        count(real),
        "geschätztes und echtes Rechteck treffen unterschiedlich viele Zeichen \
         (geschätzt {estimated:?}, echt {real:?})"
    );

    assert_hits_only_the_target(
        "aus der Extraktion",
        doc.clone(),
        estimated,
        &["Konto", "Ende"],
    );
    assert_hits_only_the_target("aus der Konstruktion", doc, real, &["Konto", "Ende"]);
}

/// Der Bereich liegt über einer 90° gedrehten Zeile.
#[test]
fn a_hand_drawn_region_works_on_rotated_text() {
    let text = format!("{BEFORE}{SECRET}{AFTER}");
    let content = grid_line(&text, 300.0, 400.0, "0 1 -1 0", (0.0, PITCH));
    let (doc, ..) = doc_with(&content);

    let start = BEFORE.chars().count() as f64;
    let end = (BEFORE.chars().count() + SECRET.chars().count()) as f64;
    // Schreibrichtung +y, Glyphenhöhe nach −x.
    let rect = Rect::new(
        300.0 - SIZE * 0.75 - 1.0,
        400.0 + PITCH * start,
        300.0 + SIZE * 0.25,
        400.0 + PITCH * end - PITCH + SIZE * 0.556,
    );
    assert_hits_only_the_target("90° gedreht", doc, rect, &["Konto", "Ende"]);
}

/// Der Text steht in einem Form-XObject, das mit `cm` verschoben und
/// vergrößert platziert ist. Die Region liegt trotzdem im User-Space der Seite.
#[test]
fn a_hand_drawn_region_reaches_into_a_scaled_form_xobject() {
    let text = format!("{BEFORE}{SECRET}{AFTER}");
    let (mut doc, _, resources_id) = doc_with("q 2 0 0 2 0 0 cm /Fm0 Do Q\n");
    let font_id = match doc.get_dictionary(resources_id).unwrap().get(b"Font") {
        Ok(Object::Dictionary(fonts)) => match fonts.get(b"F1") {
            Ok(Object::Reference(id)) => *id,
            other => panic!("kein Fontverweis: {other:?}"),
        },
        other => panic!("keine Fonts: {other:?}"),
    };
    let form_id = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 300.into(), 400.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            line(&text, 36.0, 350.0).into_bytes(),
        )
        .with_compression(false),
    ));
    doc.get_dictionary_mut(resources_id)
        .unwrap()
        .set("XObject", dictionary! { "Fm0" => form_id });

    // `cm 2 0 0 2 0 0` verdoppelt alles: Ursprung, Rasterweite und Schriftgröße.
    let start = BEFORE.chars().count() as f64;
    let end = (BEFORE.chars().count() + SECRET.chars().count()) as f64;
    let rect = Rect::new(
        72.0 + 2.0 * PITCH * start,
        700.0 - 2.0,
        72.0 + 2.0 * (PITCH * end - PITCH + SIZE * 0.556),
        700.0 + 2.0 * SIZE * 0.72,
    );
    assert_hits_only_the_target("skaliertes Form-XObject", doc, rect, &["Konto", "Ende"]);
}
