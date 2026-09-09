//! Ein Textspiegel, der etwas anderes sagt als die Glyphen darunter.
//!
//! Seit 0.4.0 wird ein `/ActualText` **geleert**, wenn die Schwärzung die
//! Glyphen darunter trifft. Gelesen wurde er nie. Gemessen (vor dieser
//! Änderung) mit `redact-rs a1.pdf -o out.pdf`:
//!
//! ```text
//! /Span << /ActualText (IBAN: DE89 …) >> BDC (Kontodaten folgen unten) Tj EMC
//!   → Treffer gesamt: 0, Rückgabewert 0, --check-leaks: gefunden
//! /Span << /Alt (IBAN: DE89 …) >> BDC (Kontodaten folgen unten) Tj EMC
//!   → dasselbe
//! ```
//!
//! Und ein `/ToUnicode`, das jeden Code auf „x“ legt: Betrachter zeigt die
//! IBAN, Analyse liest „xxxxxxxx“, 0 Treffer, Rückgabewert 0.
//!
//! Orakel ist [`redact_pdf::leaks`] — nie der eigene Extraktor allein; der
//! Extraktor wird nur dort befragt, wo es um seine **Warnungen** geht.

mod common;

use common::{page, text_ops, SECRET};
use lopdf::{dictionary, Object, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

fn analyse(bytes: &[u8]) -> (Vec<TextRun>, Vec<String>) {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion")
}

/// Die vollständige Verarbeitung, so wie das Werkzeug sie fährt.
fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> Vec<u8> {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    save_to_bytes(&doc).expect("Speichern")
}

/// Schwärzung für jedes Textstück, in dem `needle` steht — so, wie die
/// Mustererkennung sie anfordern würde.
fn redactions_for(runs: &[TextRun], needle: &str) -> Vec<Redaction> {
    runs.iter()
        .filter_map(|run| {
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
        })
        .collect()
}

fn mirror_warnings(warnings: &[String]) -> Vec<&String> {
    warnings
        .iter()
        .filter(|w| w.contains("Textspiegel"))
        .collect()
}

fn page_with(raw: &str) -> Vec<u8> {
    let mut d = page(&[]);
    d.set_content(format!("BT\n/F1 10 Tf\n72 700 Td\n{raw}\nET\n").as_bytes());
    d.finish()
}

/// Legt ein Form-XObject `name` mit dem Inhalt `body` an (die Schrift `F1`
/// der Seite ist darin bekannt) und trägt es in `resources` ein — in die
/// Ressourcen der Seite oder die eines anderen Formulars.
fn add_form(
    d: &mut common::Doc,
    resources: lopdf::ObjectId,
    name: &str,
    body: &str,
) -> lopdf::ObjectId {
    let font_id = d.font_id;
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            body.as_bytes().to_vec(),
        )
        .with_compression(false),
    ));
    let target = d.doc.get_dictionary_mut(resources).expect("Resources");
    let mut xobjects = target
        .get(b"XObject")
        .and_then(|o| o.as_dict())
        .cloned()
        .unwrap_or_default();
    xobjects.set(name, form_id);
    target.set("XObject", xobjects);
    form_id
}

/// Ein Spiegel im Seitenstrom über einem `Do`: die Glyphen darunter stehen
/// im Formular `Fm0`, der Spiegel in der Seite.
fn mirror_over_form(properties: &str, form_body: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let page_resources = d.resources_id;
    add_form(
        &mut d,
        page_resources,
        "Fm0",
        &format!("BT /F1 10 Tf 72 700 Td {form_body} ET"),
    );
    d.set_content(format!("/Span << {properties} >> BDC /Fm0 Do EMC\n").as_bytes());
    d.finish()
}

// ---------------------------------------------------------------------------
// Widerspruch: gelesen, gemeldet, geschwärzt
// ---------------------------------------------------------------------------

/// Der Spiegel trägt das Geheimnis, die Glyphen nicht: das Geheimnis muss als
/// eigener Textlauf gefunden und mit den Glyphen entfernt werden. Gemeldet
/// wird nur der Ersatz (`/ActualText`); `/Alt` und `/E` dürfen abweichen.
fn assert_lying_mirror_is_found_and_removed(bytes: Vec<u8>, key: &str) {
    let (runs, warnings) = analyse(&bytes);
    let found = mirror_warnings(&warnings);
    if key == "ActualText" {
        assert_eq!(found.len(), 1, "genau eine Spiegelwarnung: {warnings:?}");
        assert!(
            found[0].contains(&format!("(/{key})")) && found[0].contains("Seite 1"),
            "die Warnung nennt Schlüssel und Seite: {}",
            found[0]
        );
        assert!(
            found[0].contains("wurde zusätzlich als eigener Text durchsucht"),
            "mit Glyphen darunter gab es einen Kasten, also einen Suchlauf: {}",
            found[0]
        );
        assert!(
            !found[0].contains(SECRET),
            "die Warnung darf das Geheimnis nicht ins Audit-Log tragen"
        );
    } else {
        assert!(
            found.is_empty(),
            "/{key} darf von den Glyphen abweichen — kein Befund: {warnings:?}"
        );
    }
    let redactions = redactions_for(&runs, SECRET);
    assert_eq!(redactions.len(), 1, "der Spiegel ist ein eigener Textlauf");
    let out = pipeline(&bytes, &redactions);
    assert!(
        leaks(&out, SECRET).is_empty(),
        "{key}: nach der Schwärzung steht die IBAN noch in der Datei: {:?}",
        leaks(&out, SECRET)
    );
}

#[test]
fn ein_actualtext_der_etwas_anderes_sagt_wird_gelesen_gemeldet_und_geschwaerzt() {
    let bytes = page_with(&format!(
        "/Span << /ActualText (IBAN: {SECRET}) >> BDC (Kontodaten folgen unten) Tj EMC"
    ));
    assert_lying_mirror_is_found_and_removed(bytes, "ActualText");
}

/// `/Alt` **beschreibt** (PDF 32000-1, 14.9.3) und muss den Glyphen nicht
/// gleichen. Gelesen und geleert wird er wie der Ersatz; gemeldet wird er
/// nicht.
#[test]
fn ein_alt_der_etwas_anderes_sagt_wird_gelesen_und_geleert_aber_nicht_gemeldet() {
    let bytes = page_with(&format!(
        "/Span << /Alt (IBAN: {SECRET}) >> BDC (Kontodaten folgen unten) Tj EMC"
    ));
    assert_lying_mirror_is_found_and_removed(bytes, "Alt");
}

/// `/E` ist die **ausgeschriebene Form** einer Abkürzung (14.9.5): „z. B.“
/// mit `/E (zum Beispiel)` widerspricht den Glyphen — das ist sein Zweck,
/// kein Befund. Trägt er ein Geheimnis, wird es trotzdem gefunden und mit
/// den Glyphen entfernt.
#[test]
fn ein_e_ist_die_ausgeschriebene_abkuerzung_und_kein_widerspruch() {
    let harmless = page_with("/Span << /E (zum Beispiel) >> BDC (z. B.) Tj EMC");
    let (_, warnings) = analyse(&harmless);
    assert!(
        warnings.is_empty(),
        "eine ausgeschriebene Abkürzung ist die Normalform: {warnings:?}"
    );

    let bytes = page_with(&format!(
        "/Span << /E (IBAN: {SECRET}) >> BDC (Kto.) Tj EMC"
    ));
    assert_lying_mirror_is_found_and_removed(bytes, "E");
}

/// `/Figure <</Alt …>> BDC /Im0 Do EMC` — so beschriftet jeder
/// PDF/UA-Erzeuger ein Bild. Darunter gibt es keine Glyphen, und das ist kein
/// Befund: Rückgabewert 3 dafür wäre eine Grenze, die gewöhnliche Dateien
/// ablehnt. Der `/Alt` eines Bildes teilt den blinden Fleck der Pixel;
/// `--check-leaks` findet ihn über die Rohsichten.
#[test]
fn ein_alt_an_einem_bild_ohne_glyphen_bleibt_still() {
    let mut d = page(&[]);
    let image_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 1,
                "Height" => 1,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
            },
            vec![0x80],
        )
        .with_compression(false),
    ));
    d.doc
        .get_dictionary_mut(d.resources_id)
        .expect("Resources")
        .set("XObject", dictionary! { "Im0" => image_id });
    d.set_content(
        b"/Figure << /Alt (Diagramm der Kontobewegungen im Jahr 2024) >> BDC\n\
          q 200 0 0 100 72 500 cm /Im0 Do Q\nEMC\n",
    );
    let (runs, warnings) = analyse(&d.finish());
    assert!(
        warnings.is_empty(),
        "0 Warnungen für ein beschriftetes Bild: {warnings:?}"
    );
    assert!(
        runs.is_empty(),
        "ohne Glyphen kein Kasten, ohne Kasten kein Textlauf: {runs:?}"
    );
}

// ---------------------------------------------------------------------------
// Formulargrenze: der Spiegel in der Seite, die Glyphen im Formular
// ---------------------------------------------------------------------------

/// Der Spiegel steht im Seitenstrom, die Glyphen kommen über `/Fm0 Do`. Bis
/// zu dieser Korrektur kannte der Abschnitt nur die Textoperationen des
/// eigenen Stroms: „0 in den Glyphen“, Warnung — für eine Datei, in der beide
/// dasselbe sagen.
#[test]
fn ein_deckungsgleicher_spiegel_ueber_einem_formular_bleibt_still() {
    let bytes = mirror_over_form(
        &format!("/ActualText (IBAN: {SECRET})"),
        &format!("(IBAN: {SECRET}) Tj"),
    );
    let (runs, warnings) = analyse(&bytes);
    assert!(
        mirror_warnings(&warnings).is_empty(),
        "Spiegel und Formularglyphen sagen dasselbe: {warnings:?}"
    );
    let with_secret = runs.iter().filter(|r| r.text.contains(SECRET)).count();
    assert_eq!(
        with_secret, 1,
        "kein zweiter Textlauf für denselben Text: {runs:?}"
    );
}

/// Derselbe Aufbau, aber der Spiegel lügt: gelesen (als eigener Textlauf auf
/// dem Kasten der Formularglyphen), gemeldet — und **geleert**, obwohl der
/// Plan, der die Glyphen entfernt, zum Formular gehört und nicht zur Seite.
/// Nur Lesen ohne Leeren wäre ein neues Leck.
#[test]
fn ein_luegender_spiegel_ueber_einem_formular_wird_gelesen_gemeldet_und_geleert() {
    let bytes = mirror_over_form(
        &format!("/ActualText (IBAN: {SECRET})"),
        "(Kontodaten folgen unten) Tj",
    );
    assert_lying_mirror_is_found_and_removed(bytes, "ActualText");
}

/// `Fm0` zeichnet nur `Fm1`, und erst dort stehen die Glyphen. Der
/// Geltungsbereich des Spiegels schließt transitiv über die Formulare.
#[test]
fn formular_im_formular_zaehlt_zum_spiegel_darueber() {
    let mut d = page(&[]);
    let page_resources = d.resources_id;
    let outer = add_form(&mut d, page_resources, "Fm0", "q /Fm1 Do Q");
    let outer_resources = d
        .doc
        .get_object(outer)
        .and_then(|o| o.as_stream())
        .and_then(|s| s.dict.get(b"Resources"))
        .and_then(|r| r.as_dict())
        .cloned()
        .expect("Ressourcen des äußeren Formulars");
    let outer_resources_id = d.add(Object::Dictionary(outer_resources));
    if let Ok(Object::Stream(stream)) = d.doc.get_object_mut(outer) {
        stream.dict.set("Resources", outer_resources_id);
    }
    add_form(
        &mut d,
        outer_resources_id,
        "Fm1",
        &format!("BT /F1 10 Tf 72 700 Td (IBAN: {SECRET}) Tj ET"),
    );
    d.set_content(format!("/Span << /ActualText (IBAN: {SECRET}) >> BDC /Fm0 Do EMC\n").as_bytes());
    let (runs, warnings) = analyse(&d.finish());
    assert!(
        mirror_warnings(&warnings).is_empty(),
        "die Glyphen des inneren Formulars gehören zum Spiegel: {warnings:?}"
    );
    let with_secret = runs.iter().filter(|r| r.text.contains(SECRET)).count();
    assert_eq!(with_secret, 1, "{runs:?}");
}

#[test]
fn ein_spiegel_als_utf16_wird_genauso_gelesen() {
    let hex: String = common::utf16be_bom(&format!("IBAN: {SECRET}"))
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();
    let bytes = page_with(&format!(
        "/Span << /ActualText <{hex}> >> BDC (Kontodaten folgen unten) Tj EMC"
    ));
    assert_lying_mirror_is_found_and_removed(bytes, "ActualText");
}

/// Der umgekehrte Fall: die Glyphen tragen die IBAN, der Spiegel sagt
/// „harmlos“. Die Glyphen werden ohnehin gefunden — gemeldet wird der
/// Widerspruch trotzdem, denn welche Fassung ein Betrachter zeigt, weiß das
/// Werkzeug auch hier nicht.
#[test]
fn ein_harmloser_spiegel_ueber_einer_iban_wird_gemeldet_und_faellt_mit_den_glyphen() {
    let bytes = page_with(&format!(
        "/Span << /ActualText (harmlos) >> BDC (IBAN: {SECRET}) Tj EMC"
    ));
    let (runs, warnings) = analyse(&bytes);
    assert_eq!(mirror_warnings(&warnings).len(), 1, "{warnings:?}");
    let out = pipeline(&bytes, &redactions_for(&runs, SECRET));
    assert!(leaks(&out, SECRET).is_empty());
    assert!(
        leaks(&out, "harmlos").is_empty(),
        "der Spiegel eines geschwärzten Abschnitts fällt mit"
    );
}

/// Ohne Glyphen darunter gibt es keinen Kasten, also keinen Textlauf — nur
/// die Warnung. Der Lauf endet damit mit Rückgabewert 3 statt 0. Und die
/// Warnung darf keinen Suchlauf behaupten, der nicht stattfand.
#[test]
fn ein_spiegel_ohne_glyphen_darunter_wird_gemeldet() {
    let bytes = page_with(&format!(
        "/Span << /ActualText (IBAN: {SECRET}) >> BDC EMC (Kontodaten folgen unten) Tj"
    ));
    let (runs, warnings) = analyse(&bytes);
    let found = mirror_warnings(&warnings);
    assert_eq!(found.len(), 1, "{warnings:?}");
    assert!(
        found[0].contains("nicht durchsucht")
            && !found[0].contains("wurde zusätzlich als eigener Text durchsucht"),
        "ohne Kasten gab es keinen Suchlauf, und die Warnung sagt das: {}",
        found[0]
    );
    assert!(
        runs.iter().all(|r| !r.text.contains(SECRET)),
        "ohne Glyphen kein Kasten, ohne Kasten kein Textlauf"
    );
}

#[test]
fn ein_luegender_spiegel_in_einem_form_xobject_wird_gefunden() {
    let mut d = page(&["Kontoinhaber: Max Mustermann"]);
    let font_id = d.font_id;
    let form = format!(
        "BT /F1 10 Tf 0 0 Td /Span << /ActualText (IBAN: {SECRET}) >> BDC \
         (Kontodaten folgen unten) Tj EMC ET"
    );
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 400.into(), 20.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            form.into_bytes(),
        )
        .with_compression(false),
    ));
    d.doc
        .get_dictionary_mut(d.resources_id)
        .expect("Resources")
        .set("XObject", dictionary! { "Fm0" => form_id });
    let mut raw = text_ops(&["Kontoinhaber: Max Mustermann"]);
    raw.extend_from_slice(b"q 1 0 0 1 72 500 cm /Fm0 Do Q\n");
    d.set_content(&raw);
    assert_lying_mirror_is_found_and_removed(d.finish(), "ActualText");
}

// ---------------------------------------------------------------------------
// Gegenprobe: ehrliche Spiegel bleiben still
// ---------------------------------------------------------------------------

/// Was Word, InDesign und jeder PDF/UA-Erzeuger schreiben: derselbe Text,
/// als UTF-16, ein Tabulator über einem Leerraum, ein weicher Trennstrich
/// über einem Bindestrich, ein Wort über zwei `Tj`, ein einzelnes
/// Sonderzeichen. Keine dieser Dateien darf warnen — sonst warnt jede
/// getaggte Word-Datei.
#[test]
fn deckungsgleiche_spiegel_bleiben_ohne_warnung_und_ohne_eigenen_textlauf() {
    let bytes = page_with(&format!(
        "/Span << /ActualText (IBAN: {SECRET}) >> BDC (IBAN: {SECRET}) Tj EMC\n\
         0 -15 Td /Span << /ActualText <FEFF004B006F006E0074006F> >> BDC (Kon) Tj (to) Tj EMC\n\
         0 -15 Td /Span << /ActualText <FEFF0009> >> BDC ( ) Tj EMC\n\
         0 -15 Td /Span << /ActualText <FEFF00AD> >> BDC (-) Tj EMC\n\
         0 -15 Td /Span << /ActualText <FEFF2022> >> BDC (\\225) Tj EMC\n\
         0 -15 Td /Span << /ActualText (  Konto  ) >> BDC (Konto) Tj EMC"
    ));
    let (runs, warnings) = analyse(&bytes);
    assert!(
        mirror_warnings(&warnings).is_empty(),
        "ein ehrlicher Spiegel ist kein Befund: {warnings:?}"
    );
    let with_secret = runs.iter().filter(|r| r.text.contains(SECRET)).count();
    assert_eq!(with_secret, 1, "kein zweiter Textlauf für denselben Text");
}

/// Die Ligatur: ein Code, drei Zeichen — der Spiegel schreibt sie aus.
#[test]
fn eine_ligatur_mit_ausgeschriebenem_spiegel_bleibt_still() {
    let mut d = page(&[]);
    let cmap = b"/CIDInit /ProcSet findresource begin begincmap\n\
        1 begincodespacerange <00> <FF> endcodespacerange\n\
        2 beginbfchar <96> <FB03> <61> <0061> endbfchar\n\
        endcmap end end"
        .to_vec();
    let cmap_id = d.add(Object::Stream(Stream::new(dictionary! {}, cmap)));
    d.doc
        .get_dictionary_mut(d.font_id)
        .expect("Font")
        .set("ToUnicode", Object::Reference(cmap_id));
    d.set_content(
        b"BT /F1 10 Tf 72 700 Td /Span << /ActualText (ffi) >> BDC (\x96) Tj EMC (a) Tj ET",
    );
    let (_, warnings) = analyse(&d.finish());
    assert!(mirror_warnings(&warnings).is_empty(), "{warnings:?}");
}

// ---------------------------------------------------------------------------
// Ein /ToUnicode, das lügt
// ---------------------------------------------------------------------------

/// Ein Font, dessen `/ToUnicode` die Codes `20`..`7E` auf `text` legt — jeden.
fn font_with_uniform_to_unicode(text: &str, codes: std::ops::RangeInclusive<u8>) -> Vec<u8> {
    let mut d = page(&[]);
    let target: String = text.encode_utf16().map(|u| format!("{u:04X}")).collect();
    let mut chars = String::new();
    let mut count = 0usize;
    for code in codes {
        chars.push_str(&format!("<{code:02X}> <{target}>\n"));
        count += 1;
    }
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin begincmap\n\
         1 begincodespacerange <00> <FF> endcodespacerange\n\
         {count} beginbfchar\n{chars}endbfchar\nendcmap end end"
    );
    let cmap_id = d.add(Object::Stream(Stream::new(
        dictionary! {},
        cmap.into_bytes(),
    )));
    d.doc
        .get_dictionary_mut(d.font_id)
        .expect("Font")
        .set("ToUnicode", Object::Reference(cmap_id));
    d.set_content(&text_ops(&[&format!("IBAN: {SECRET}")]));
    d.finish()
}

fn to_unicode_warnings(warnings: &[String]) -> Vec<&String> {
    warnings
        .iter()
        .filter(|w| w.contains("/ToUnicode, das") && w.contains("denselben Text"))
        .collect()
}

#[test]
fn ein_tounicode_das_alle_codes_auf_dasselbe_zeichen_legt_wird_gemeldet() {
    let bytes = font_with_uniform_to_unicode("x", 0x20..=0x7e);
    let (runs, warnings) = analyse(&bytes);
    assert!(
        runs.iter().all(|r| !r.text.contains(SECRET)),
        "die Analyse liest, was das /ToUnicode behauptet — sonst wäre das kein Test"
    );
    let found = to_unicode_warnings(&warnings);
    assert_eq!(found.len(), 1, "{warnings:?}");
    assert!(
        found[0].contains("Helvetica") && found[0].contains("„x“"),
        "Font und Text stehen in der Warnung: {}",
        found[0]
    );
}

/// Gegenprobe: ein `/ToUnicode`, das nur wenige Codes zusammenlegt, ist
/// gewöhnlich (Glyphvarianten) und bleibt still — ebenso eines, das die
/// Codes richtig benennt.
#[test]
fn ein_gewoehnliches_tounicode_bleibt_still() {
    // Sieben verschiedene Codes auf „x“, alle anderen bleiben unabgebildet.
    let few = font_with_uniform_to_unicode("x", 0x30..=0x36);
    let (_, warnings) = analyse(&few);
    assert!(to_unicode_warnings(&warnings).is_empty(), "{warnings:?}");

    // Die Wahrheit: jeder Code auf sein eigenes Zeichen.
    let mut d = page(&[]);
    let chars: String = (0x20u8..=0x7e)
        .map(|c| format!("<{c:02X}> <{c:04X}>\n"))
        .collect();
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin begincmap\n\
         1 begincodespacerange <00> <FF> endcodespacerange\n\
         95 beginbfchar\n{chars}endbfchar\nendcmap end end"
    );
    let cmap_id = d.add(Object::Stream(Stream::new(
        dictionary! {},
        cmap.into_bytes(),
    )));
    d.doc
        .get_dictionary_mut(d.font_id)
        .expect("Font")
        .set("ToUnicode", Object::Reference(cmap_id));
    d.set_content(&text_ops(&[&format!("IBAN: {SECRET}")]));
    let (runs, warnings) = analyse(&d.finish());
    assert!(to_unicode_warnings(&warnings).is_empty(), "{warnings:?}");
    assert!(runs.iter().any(|r| r.text.contains(SECRET)));
}
