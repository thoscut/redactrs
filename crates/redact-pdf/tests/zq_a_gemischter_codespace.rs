//! Prüfrunde „stille Lecks im Inhalt“, Befund 2: eine Type0-Schrift, deren
//! eingebettete `/Encoding`-CMap Codes **unterschiedlicher Länge** kennt
//! (`<00>–<7F>` einbytig, `<8000>–<FFFF>` zweibytig; PDF 32000-1, 9.7.6.2).
//!
//! Der Interpreter las jede Type0-Schrift ohne eindeutige Codebreite stur
//! zweibytig. Aus `(IBAN: DE89 …)` wurden Codes wie `0x4942`, die das
//! `/ToUnicode` nicht kennt; die Ersatzzeichen fielen nicht auf, weil die
//! Warnstatistik Schriften mit `/ToUnicode` nicht zählt. „Treffer 0“,
//! Rückgabewert 0 — Poppler zeigte und kopierte die IBAN.
//!
//! **Behoben:** die Länge jedes Codes ergibt sich aus seinen ersten Bytes.
//! Eine vordefinierte CMap, deren Codes nicht sicher zwei Byte lang sind
//! (`90ms-RKSJ-H` & Co., die Tabellen liefert das Werkzeug nicht mit), ist
//! eine Deckungslücke statt einer stillen Annahme.
//!
//! Orakel: [`redact_pdf::leaks`] an der Ausgabedatei.

mod common;

use common::{page, SECRET};
use lopdf::{dictionary, Object, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

const GEMISCHT: &str = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CMapName /Gemischt def
/CMapType 1 def
2 begincodespacerange
<00> <7F>
<8000> <FFFF>
endcodespacerange
2 begincidrange
<00> <7F> 0
<8000> <FFFF> 128
endcidrange
endcmap
CMapName currentdict /CMap defineresource pop
end
end";

/// `/ToUnicode` mit demselben Codespace; die einbytigen Codes 0x20–0x7E sind
/// ASCII, der zweibytige `<8141>` ist ein „X“.
const TO_UNICODE: &str = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CMapName /Gemischt-UCS def
/CMapType 2 def
2 begincodespacerange
<00> <7F>
<8000> <FFFF>
endcodespacerange
1 beginbfrange
<20> <7E> <0020>
endbfrange
1 beginbfchar
<8141> <0058>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";

/// Eine Seite mit einer Type0-Schrift F1; `encoding` ist entweder ein Name
/// oder `None` für die eingebettete gemischte CMap. `content` ist roh.
fn dokument(encoding: Option<&str>, content: &[u8]) -> Vec<u8> {
    let mut d = page(&[]);
    let cid = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "Helvetica",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
        "DW" => 600,
    }));
    let enc = match encoding {
        Some(name) => Object::Name(name.as_bytes().to_vec()),
        None => Object::Reference(d.add(Object::Stream(Stream::new(
            dictionary! { "Type" => "CMap", "CMapName" => "Gemischt" },
            GEMISCHT.as_bytes().to_vec(),
        )))),
    };
    let tou = d.add(Object::Stream(Stream::new(
        dictionary! {},
        TO_UNICODE.as_bytes().to_vec(),
    )));
    let font = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "Helvetica",
        "Encoding" => enc,
        "ToUnicode" => tou,
        "DescendantFonts" => vec![Object::Reference(cid)],
    }));
    let resources = d
        .doc
        .get_dictionary_mut(d.resources_id)
        .expect("Ressourcen");
    resources.set("Font", dictionary! { "F1" => font });
    d.set_content(content);
    d.finish()
}

fn analyse(bytes: &[u8]) -> (Vec<TextRun>, Vec<String>) {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion")
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

fn texte(runs: &[TextRun]) -> Vec<&str> {
    runs.iter().map(|r| r.text.as_str()).collect()
}

fn ist_luecke(warning: &str) -> bool {
    warning.contains("nicht vollständig geschwärzt")
}

#[test]
fn gemischte_codelaengen_werden_je_code_zerlegt() {
    let bytes = dokument(
        None,
        format!("BT /F1 12 Tf 72 700 Td (IBAN: {SECRET} ) Tj <8141> Tj ET").as_bytes(),
    );
    let (runs, warnings) = analyse(&bytes);
    assert!(
        runs.iter().any(|r| r.text.contains(SECRET)),
        "die Analyse liest die IBAN nicht: {:?}",
        texte(&runs)
    );
    // Der zweibytige Code steht hinter den einbytigen und wird als einer
    // gelesen — „X“, nicht zwei Ersatzzeichen.
    assert!(
        runs.iter().any(|r| r.text.ends_with('X')),
        "{:?}",
        texte(&runs)
    );
    assert!(!warnings.iter().any(|w| ist_luecke(w)), "{warnings:?}");

    let redactions: Vec<Redaction> = runs
        .iter()
        .filter_map(|run| redaction_for(run, SECRET))
        .collect();
    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    let rest = leaks(&out, SECRET);
    assert!(rest.is_empty(), "IBAN steht noch in der Ausgabe: {rest:?}");
    // Nur die IBAN ist fort, der Rest der Zeile steht.
    assert_eq!(report.removed_glyphs, SECRET.chars().count());
    assert!(!leaks(&out, "IBAN:").is_empty());
}

/// Eine vordefinierte CMap mit gemischten Codelängen, deren Tabelle das
/// Werkzeug nicht mitbringt: das ist eine Lücke, kein stilles „zweibytig“.
#[test]
fn eine_unbekannte_vordefinierte_cmap_ist_eine_luecke() {
    let bytes = dokument(
        Some("90ms-RKSJ-H"),
        format!("BT /F1 12 Tf 72 700 Td (IBAN: {SECRET}) Tj ET").as_bytes(),
    );
    let (_, warnings) = analyse(&bytes);
    assert!(
        warnings
            .iter()
            .any(|w| ist_luecke(w) && w.contains("90ms-RKSJ-H")),
        "{warnings:?}"
    );
}

/// Gegenprobe: `Identity-H` mit `/ToUnicode` ist der Normalfall und bleibt
/// ohne Meldung.
#[test]
fn identity_h_bleibt_ohne_meldung() {
    let mut content = b"BT /F1 12 Tf 72 700 Td <".to_vec();
    for c in SECRET.chars() {
        content.extend_from_slice(format!("00{:02X}", c as u32).as_bytes());
    }
    content.extend_from_slice(b"> Tj ET");
    let bytes = dokument(Some("Identity-H"), &content);
    let (_, warnings) = analyse(&bytes);
    assert!(!warnings.iter().any(|w| ist_luecke(w)), "{warnings:?}");
}
