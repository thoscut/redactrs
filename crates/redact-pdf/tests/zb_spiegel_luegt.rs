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

// ---------------------------------------------------------------------------
// Widerspruch: gelesen, gemeldet, geschwärzt
// ---------------------------------------------------------------------------

fn assert_lying_mirror_is_found_and_removed(bytes: Vec<u8>, key: &str) {
    let (runs, warnings) = analyse(&bytes);
    let found = mirror_warnings(&warnings);
    assert_eq!(found.len(), 1, "genau eine Spiegelwarnung: {warnings:?}");
    assert!(
        found[0].contains(&format!("(/{key})")) && found[0].contains("Seite 1"),
        "die Warnung nennt Schlüssel und Seite: {}",
        found[0]
    );
    assert!(
        !found[0].contains(SECRET),
        "die Warnung darf das Geheimnis nicht ins Audit-Log tragen"
    );
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

#[test]
fn ein_alt_der_etwas_anderes_sagt_verhaelt_sich_genauso() {
    let bytes = page_with(&format!(
        "/Span << /Alt (IBAN: {SECRET}) >> BDC (Kontodaten folgen unten) Tj EMC"
    ));
    assert_lying_mirror_is_found_and_removed(bytes, "Alt");
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
/// die Warnung. Der Lauf endet damit mit Rückgabewert 3 statt 0.
#[test]
fn ein_spiegel_ohne_glyphen_darunter_wird_gemeldet() {
    let bytes = page_with(&format!(
        "/Span << /ActualText (IBAN: {SECRET}) >> BDC EMC (Kontodaten folgen unten) Tj"
    ));
    let (runs, warnings) = analyse(&bytes);
    assert_eq!(mirror_warnings(&warnings).len(), 1, "{warnings:?}");
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
