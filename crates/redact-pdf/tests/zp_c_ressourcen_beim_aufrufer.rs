//! Spur-A-Runde 2, Prüfer C-4 (Register #88): Ressourcennamen, die sich nur
//! beim **Aufrufer** auflösen.
//!
//! Ein Formular mit eigenem `/Resources` benutzt `/F1 Tf`, `/Fm1 Do`,
//! `/GS0 gs` oder `/P0 scn`, und der Name steht nicht im eigenen Verzeichnis,
//! sondern in dem der Seite. Nach PDF 32000-1, 7.8.3 ist das nicht
//! vorgesehen; Poppler (`GfxResources`, Kette der Aufrufer) löst den Namen
//! trotzdem auf und zeichnet. Der Scan sah „nichts gezeichnet“: kein Text,
//! keine Schwärzung, und auch das Orakel fand nichts — ein stilles Leck in
//! der Ausgabe und in `--check-leaks`.
//!
//! **Behoben:** kennt das eigene Verzeichnis einen benutzten Namen nicht,
//! liest der Scan den Strom unter einer zusammengesetzten Sicht — die
//! Kategorien der Aufrufer, das eigene Verzeichnis darüber
//! (`content::merged_resources`). Ein eigener Eintrag geht vor
//! ([`eigener_eintrag_geht_vor_dem_des_aufrufers`]).
//!
//! Orakel überall: [`redact_pdf::leaks`] an der **Ausgabedatei**.

mod common;

use common::{page, text_ops, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

// ---------------------------------------------------------------------------
// Bausteine (wie zo_c_spiegel_umgebungen.rs)
// ---------------------------------------------------------------------------

fn analyse(bytes: &[u8]) -> Vec<TextRun> {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion")
        .0
}

fn redaction_for(run: &TextRun, needle: &str) -> Option<Redaction> {
    let pos = run.text.find(needle)?;
    let rect = run.rect_for_byte_range(pos, pos + needle.len())?;
    Some(Redaction::new(
        Region::new(
            run.page,
            rect,
            Some(needle.to_string()),
            Source::Pattern {
                pattern_id: "iban_de".into(),
                confidence: 1.0,
            },
        ),
        Action::Blackout,
    ))
}

/// Schwärzt jede Fundstelle von `SECRET` und liefert (Lecks in der Ausgabe,
/// Warnungen des Redaktors).
fn nach_der_pipeline(bytes: &[u8]) -> (Vec<String>, Vec<String>) {
    let runs = analyse(bytes);
    let redactions: Vec<Redaction> = runs
        .iter()
        .filter_map(|run| redaction_for(run, SECRET))
        .collect();
    assert!(!redactions.is_empty(), "nichts zu schwärzen: {runs:?}");
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    (leaks(&out, SECRET), report.warnings)
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn text_at(y: i32, text: &str) -> String {
    format!("BT /F1 10 Tf 72 {y} Td ({}) Tj ET\n", escape(text))
}

/// Trägt `name` in die Kategorie `category` des Ressourcenobjekts `holder`.
fn eintragen(d: &mut Doc, holder: ObjectId, category: &str, name: &str, target: Object) {
    let holder = d.doc.get_dictionary_mut(holder).expect("Ressourcen");
    let mut entries = holder
        .get(category.as_bytes())
        .and_then(|o| o.as_dict())
        .cloned()
        .unwrap_or_default();
    entries.set(name, target);
    holder.set(category, entries);
}

/// Eigenes Ressourcenobjekt eines Formulars: mit `/F1` oder ganz ohne
/// Schriften (dann nur `/ProcSet`).
fn ressourcen(d: &mut Doc, font: Option<ObjectId>) -> ObjectId {
    let dict = match font {
        Some(font) => dictionary! { "Font" => dictionary! { "F1" => font } },
        None => dictionary! {
            "ProcSet" => vec![Object::Name(b"PDF".to_vec()), Object::Name(b"Text".to_vec())],
        },
    };
    d.add(Object::Dictionary(dict))
}

/// Form-XObject; ohne `resources` ohne eigenes `/Resources`.
fn formular(d: &mut Doc, resources: Option<ObjectId>, body: &str) -> ObjectId {
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
    };
    if let Some(resources) = resources {
        dict.set("Resources", resources);
    }
    d.add(Object::Stream(
        Stream::new(dict, body.as_bytes().to_vec()).with_compression(false),
    ))
}

/// Die Zeichen des Geheimnisses, je einmal.
fn zeichenvorrat() -> Vec<char> {
    let mut seen = Vec::new();
    for c in SECRET.chars() {
        if !seen.contains(&c) {
            seen.push(c);
        }
    }
    seen
}

fn glyph_name(c: char) -> &'static str {
    match c {
        '0' => "zero",
        '1' => "one",
        '2' => "two",
        '3' => "three",
        '4' => "four",
        '5' => "five",
        '6' => "six",
        '7' => "seven",
        '8' => "eight",
        '9' => "nine",
        ' ' => "space",
        'D' => "D",
        'E' => "E",
        _ => panic!("kein Glyphname für {c:?}"),
    }
}

/// Schrift mit eigener Kodierung: das Geheimnis steht in Codes ab 0x80,
/// kein Byte davon ist im Klartext.
fn eigene_kodierung(d: &mut Doc) -> ObjectId {
    let mut differences: Vec<Object> = vec![Object::Integer(0x80)];
    for c in zeichenvorrat() {
        differences.push(Object::Name(glyph_name(c).as_bytes().to_vec()));
    }
    d.add(Object::Dictionary(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => dictionary! {
            "Type" => "Encoding",
            "BaseEncoding" => "WinAnsiEncoding",
            "Differences" => Object::Array(differences),
        },
    }))
}

/// Das Geheimnis in den Codes von [`eigene_kodierung`], gesetzt in `/F1`.
fn geheim_kodiert(y: i32) -> String {
    let vorrat = zeichenvorrat();
    let codes: String = SECRET
        .chars()
        .map(|c| {
            format!(
                "{:02X}",
                0x80 + vorrat.iter().position(|v| *v == c).unwrap()
            )
        })
        .collect();
    format!("BT /F1 10 Tf 72 {y} Td <{codes}> Tj ET\n")
}

/// Seite mit `Fm0` (eigenes `/Resources`, `body`) und dem üblichen Kopftext.
fn seite_mit_fm0(d: &mut Doc, fm0_resources: ObjectId, body: &str) {
    let res = d.resources_id;
    let fm0 = formular(d, Some(fm0_resources), body);
    eintragen(d, res, "XObject", "Fm0", Object::Reference(fm0));
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
}

// ===========================================================================
// Die Klasse, je Kategorie
// ===========================================================================

/// `/F1` nur in der Seite; `Fm0` hat eigenes `/Resources` ohne `/Font`.
fn schrift_nur_in_der_seite() -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    let font = eigene_kodierung(&mut d);
    eintragen(&mut d, res, "Font", "F1", Object::Reference(font));
    let fm0_res = ressourcen(&mut d, None);
    seite_mit_fm0(&mut d, fm0_res, &geheim_kodiert(600));
    d.finish()
}

#[test]
fn schrift_nur_in_der_seite_wird_geschwaerzt() {
    let (found, warnings) = nach_der_pipeline(&schrift_nur_in_der_seite());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// Das Orakel liest dieselbe Sicht: ohne sie findet `--check-leaks` im
/// **Eingang** nichts, obwohl Poppler das Geheimnis zeigt.
#[test]
fn orakel_findet_den_text_in_der_schrift_der_seite() {
    let found = leaks(&schrift_nur_in_der_seite(), SECRET);
    assert!(!found.is_empty(), "das Orakel sieht das Geheimnis nicht");
}

/// `/Fm1 Do` aus `Fm0`, `Fm1` nur im `/XObject` der Seite.
#[test]
fn formular_nur_in_der_seite_wird_geschwaerzt() {
    let mut d = page(&[]);
    let res = d.resources_id;
    let font = d.font_id;
    let fm1_res = ressourcen(&mut d, Some(font));
    let fm1 = formular(&mut d, Some(fm1_res), &text_at(600, SECRET));
    eintragen(&mut d, res, "XObject", "Fm1", Object::Reference(fm1));
    let fm0_res = ressourcen(&mut d, Some(font));
    seite_mit_fm0(&mut d, fm0_res, "q /Fm1 Do Q\n");
    let (found, warnings) = nach_der_pipeline(&d.finish());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// `Fm1` ohne `/Resources`, direkt auf der Seite (Seitenschrift: harmlos) und
/// über den Rückfall aus `Fm0` gezeichnet — dort gilt die Schrift von `Fm0`,
/// und erst dort steht das Geheimnis.
#[test]
fn formular_ohne_ressourcen_unter_dem_rueckfall_liest_die_schrift_des_aufrufers() {
    let mut d = page(&[]);
    let res = d.resources_id;
    let fm1 = formular(&mut d, None, &geheim_kodiert(600));
    eintragen(&mut d, res, "XObject", "Fm1", Object::Reference(fm1));
    let font = eigene_kodierung(&mut d);
    let fm0_res = ressourcen(&mut d, Some(font));
    let fm0 = formular(&mut d, Some(fm0_res), "q /Fm1 Do Q\n");
    eintragen(&mut d, res, "XObject", "Fm0", Object::Reference(fm0));
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm1 Do Q\nq 1 0 0 1 0 -300 cm /Fm0 Do Q\n");
    d.set_content(&raw);
    let (found, warnings) = nach_der_pipeline(&d.finish());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// `/GS0 gs` aus `Fm0`, der Grafikzustand (mit `/SMask`-Gruppe, die Text
/// setzt) nur in der Seite.
#[test]
fn grafikzustand_mit_maske_nur_in_der_seite_wird_geschwaerzt() {
    let mut d = page(&[]);
    let res = d.resources_id;
    let font = d.font_id;
    let group_res = ressourcen(&mut d, Some(font));
    let group = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                "Group" => dictionary! { "S" => "Transparency", "CS" => "DeviceGray" },
                "Resources" => group_res,
            },
            format!("1 g {}", text_at(605, SECRET)).into_bytes(),
        )
        .with_compression(false),
    ));
    let gs = Object::Dictionary(dictionary! {
        "Type" => "ExtGState",
        "SMask" => dictionary! { "Type" => "Mask", "S" => "Luminosity", "G" => group },
    });
    eintragen(&mut d, res, "ExtGState", "GS0", gs);
    let fm0_res = ressourcen(&mut d, Some(font));
    seite_mit_fm0(&mut d, fm0_res, "q /GS0 gs 0 0 1 rg 72 600 300 20 re f Q\n");
    let (found, warnings) = nach_der_pipeline(&d.finish());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// `/P0 scn` aus `Fm0`, das Kachelmuster (setzt Text) nur in der Seite.
#[test]
fn kachelmuster_nur_in_der_seite_wird_geschwaerzt() {
    let mut d = page(&[]);
    let res = d.resources_id;
    let font = d.font_id;
    let pattern_res = ressourcen(&mut d, Some(font));
    let pattern = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "Pattern",
                "PatternType" => 1,
                "PaintType" => 1,
                "TilingType" => 1,
                "BBox" => vec![0.into(), 0.into(), 300.into(), 20.into()],
                "XStep" => 600,
                "YStep" => 600,
                "Resources" => pattern_res,
            },
            format!("BT /F1 10 Tf 0 5 Td ({}) Tj ET\n", escape(SECRET)).into_bytes(),
        )
        .with_compression(false),
    ));
    eintragen(&mut d, res, "Pattern", "P0", Object::Reference(pattern));
    let fm0_res = ressourcen(&mut d, Some(font));
    seite_mit_fm0(&mut d, fm0_res, "/Pattern cs /P0 scn 72 600 300 20 re f\n");
    let (found, warnings) = nach_der_pipeline(&d.finish());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

// ===========================================================================
// Die Grenze: der eigene Eintrag geht vor
// ===========================================================================

/// `Fm0` kennt `/F1` selbst (WinAnsi), die Seite kennt ein anderes `/F1`
/// (eigene Kodierung) und `Fm1`. `Fm1` hat kein `/Resources` und wird nur
/// über den Rückfall aus `Fm0` gezeichnet: es gilt das `/F1` von `Fm0`, wie
/// in Poppler — die Codes zeigen dort nicht das Geheimnis.
#[test]
fn eigener_eintrag_geht_vor_dem_des_aufrufers() {
    let mut d = page(&[]);
    let res = d.resources_id;
    let winansi = d.font_id;
    let font = eigene_kodierung(&mut d);
    eintragen(&mut d, res, "Font", "F1", Object::Reference(font));
    let body = format!("{}{}", text_at(620, "Fm1 gelesen"), geheim_kodiert(600));
    let fm1 = formular(&mut d, None, &body);
    eintragen(&mut d, res, "XObject", "Fm1", Object::Reference(fm1));
    let fm0_res = ressourcen(&mut d, Some(winansi));
    seite_mit_fm0(&mut d, fm0_res, "q /Fm1 Do Q\n");
    let runs = analyse(&d.finish());
    assert!(
        runs.iter().any(|r| r.text.contains("Fm1 gelesen")),
        "Fm1 wurde nicht gelesen: {runs:?}"
    );
    assert!(
        !runs.iter().any(|r| r.text.contains(SECRET)),
        "das /F1 der Seite hat das eigene überdeckt: {runs:?}"
    );
}
