//! Prüfrunde „stille Lecks im Inhalt“, Befund 3: die **eingebaute**
//! Kodierung eines eingebetteten Fontprogramms.
//!
//! Nennt eine einfache Schrift keine Basiskodierung (kein `/Encoding`, oder
//! `/Differences` ohne `/BaseEncoding`), gilt die des Fontprogramms (PDF
//! 32000-1, 9.6.6.1). Poppler liest sie aus dem Klartextteil von `/FontFile`
//! (`dup 97 /D put`) und extrahiert danach; der Interpreter nahm WinAnsi an.
//! Eine Schrift, deren eingebaute Kodierung `a` auf `D` legt, zeigte und
//! kopierte die IBAN, die Analyse las `abkl …` — Treffer 0, Rückgabewert 0,
//! und das Orakel, das dieselbe Sicht liest, sagte „nicht gefunden“.
//!
//! **Behoben:** die Tabelle aus `/FontFile` gilt. Ein CFF-Programm
//! (`/FontFile3 /Subtype /Type1C`) mit eigener Kodierung wird nicht
//! nachgebaut; ohne `/ToUnicode` ist es eine Deckungslücke. Standard-14 und
//! eingebettete Schriften mit Standardkodierung bleiben ohne Meldung.
//!
//! Orakel: [`redact_pdf::leaks`]. [`das_orakel_sieht_die_iban_in_der_eingabe`]
//! ist der Kalibrierfall: ohne die Korrektur ist er rot.

mod common;

use common::{page, SECRET};
use lopdf::{dictionary, Dictionary, Object, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

/// `SECRET` in den Codes der eingebauten Kodierung: a→D, b→E, c…l→0…9.
const ENCODED: &str = "abkl fjcg ccgg chfe cdfc cc";

const NAMEN: [&str; 12] = [
    "D", "E", "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
];

/// Klartextteil eines Type1-Programms; `encoding` steht hinter `/Encoding`.
fn type1_programm(encoding: &str) -> Vec<u8> {
    let mut clear = format!(
        "%!PS-AdobeFont-1.0: Fake 001.000\n12 dict begin\n/FontName /Fake def\n\
         /FontType 1 def\n/FontMatrix [0.001 0 0 0.001 0 0] readonly def\n\
         /Encoding {encoding} def\ncurrentdict end\ncurrentfile eexec\n"
    )
    .into_bytes();
    // 64 Byte Platzhalter für den verschlüsselten Teil (`/Length2`).
    clear.extend_from_slice(&[0u8; 64]);
    clear
}

fn eigene_tabelle() -> String {
    let mut out = String::from("256 array\n0 1 255 {1 index exch /.notdef put} for\n");
    for (i, name) in NAMEN.iter().enumerate() {
        out.push_str(&format!("dup {} /{name} put\n", 97 + i));
    }
    for (code, name) in [
        (32, "space"),
        (73, "I"),
        (66, "B"),
        (65, "A"),
        (78, "N"),
        (58, "colon"),
    ] {
        out.push_str(&format!("dup {code} /{name} put\n"));
    }
    out.push_str("readonly");
    out
}

/// Eine Seite mit Schrift F1. `font_file` ist (Schlüssel im Descriptor,
/// Strom); `encoding` ein optionaler `/Encoding`-Wert.
fn dokument(font_file: (&str, Stream), encoding: Option<Object>, content: &str) -> Vec<u8> {
    dokument_mit(font_file, encoding, None, content)
}

/// Wie [`dokument`], dazu ein optionales `/ToUnicode` (roh).
fn dokument_mit(
    font_file: (&str, Stream),
    encoding: Option<Object>,
    to_unicode: Option<&[u8]>,
    content: &str,
) -> Vec<u8> {
    let mut d = page(&[]);
    let (key, stream) = font_file;
    let ff = d.add(Object::Stream(stream));
    let mut descriptor = dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "Fake",
        "Flags" => 4,
        "FontBBox" => vec![0.into(), 0.into(), 1000.into(), 1000.into()],
        "ItalicAngle" => 0,
        "Ascent" => 800,
        "Descent" => -200,
        "CapHeight" => 700,
        "StemV" => 80,
    };
    descriptor.set(key, ff);
    let descriptor = d.add(Object::Dictionary(descriptor));
    let mut font: Dictionary = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Fake",
        "FirstChar" => 32,
        "LastChar" => 122,
        "Widths" => (32..=122).map(|_| Object::Integer(500)).collect::<Vec<_>>(),
        "FontDescriptor" => descriptor,
    };
    if let Some(encoding) = encoding {
        font.set("Encoding", encoding);
    }
    if let Some(cmap) = to_unicode {
        let tou = d.add(Object::Stream(Stream::new(dictionary! {}, cmap.to_vec())));
        font.set("ToUnicode", tou);
    }
    let font = d.add(Object::Dictionary(font));
    let resources = d
        .doc
        .get_dictionary_mut(d.resources_id)
        .expect("Ressourcen");
    resources.set("Font", dictionary! { "F1" => font });
    d.set_content(content.as_bytes());
    d.finish()
}

fn fontfile(encoding: &str) -> (&'static str, Stream) {
    let data = type1_programm(encoding);
    let length1 = data.len() - 64;
    (
        "FontFile",
        Stream::new(
            dictionary! { "Length1" => length1 as i64, "Length2" => 64, "Length3" => 0 },
            data,
        ),
    )
}

/// Ein minimales CFF: Kopf, Name-INDEX, Top-DICT-INDEX mit `top`.
fn cff(top: &[u8]) -> (&'static str, Stream) {
    let mut data = vec![1u8, 0, 4, 1];
    let name = b"Fake";
    data.extend_from_slice(&[0, 1, 1, 1, 1 + name.len() as u8]);
    data.extend_from_slice(name);
    data.extend_from_slice(&[0, 1, 1, 1, 1 + top.len() as u8]);
    data.extend_from_slice(top);
    (
        "FontFile3",
        Stream::new(dictionary! { "Subtype" => "Type1C" }, data),
    )
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

fn nach_der_pipeline(bytes: &[u8]) -> Vec<String> {
    let (runs, _) = analyse(bytes);
    let redactions: Vec<Redaction> = runs
        .iter()
        .filter_map(|run| redaction_for(run, SECRET))
        .collect();
    assert!(
        !redactions.is_empty(),
        "die Analyse findet die IBAN nicht: {:?}",
        runs.iter().map(|r| &r.text).collect::<Vec<_>>()
    );
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, &redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    leaks(&out, SECRET)
}

fn ist_luecke(warning: &str) -> bool {
    warning.contains("nicht vollständig geschwärzt")
}

fn verschluesselt() -> Vec<u8> {
    dokument(
        fontfile(&eigene_tabelle()),
        None,
        &format!("BT /F1 12 Tf 72 700 Td (IBAN: {ENCODED}) Tj ET"),
    )
}

#[test]
fn das_orakel_sieht_die_iban_in_der_eingabe() {
    let funde = leaks(&verschluesselt(), SECRET);
    assert!(
        !funde.is_empty(),
        "die eingebaute Kodierung des Type1-Programms wurde nicht gelesen"
    );
}

#[test]
fn die_eingebaute_kodierung_wird_geschwaerzt() {
    let rest = nach_der_pipeline(&verschluesselt());
    assert!(rest.is_empty(), "IBAN steht noch in der Ausgabe: {rest:?}");
    let (_, warnings) = analyse(&verschluesselt());
    assert!(!warnings.iter().any(|w| ist_luecke(w)), "{warnings:?}");
}

/// `/Differences` ohne `/BaseEncoding` liegt **über** der eingebauten
/// Kodierung: `m` wird zu einem weiteren `D`, der Rest bleibt eingebaut.
#[test]
fn differences_ohne_basis_liegen_ueber_der_eingebauten() {
    let encoding = Object::Dictionary(dictionary! {
        "Type" => "Encoding",
        "Differences" => vec![109.into(), Object::Name(b"D".to_vec())],
    });
    let bytes = dokument(
        fontfile(&eigene_tabelle()),
        Some(encoding),
        "BT /F1 12 Tf 72 700 Td (IBAN: mbkl fjcg ccgg chfe cdfc cc) Tj ET",
    );
    let rest = nach_der_pipeline(&bytes);
    assert!(rest.is_empty(), "{rest:?}");
}

/// Gegenprobe: eine genannte Basiskodierung geht vor — wie bei Poppler.
#[test]
fn eine_genannte_basis_geht_vor() {
    let bytes = dokument(
        fontfile(&eigene_tabelle()),
        Some(Object::Name(b"WinAnsiEncoding".to_vec())),
        &format!("BT /F1 12 Tf 72 700 Td (IBAN: {SECRET}) Tj ET"),
    );
    let (runs, warnings) = analyse(&bytes);
    assert!(runs.iter().any(|r| r.text.contains(SECRET)));
    assert!(!warnings.iter().any(|w| ist_luecke(w)), "{warnings:?}");
}

/// Gegenprobe: `StandardEncoding` im Programm ändert nichts und meldet nichts.
#[test]
fn standardkodierung_im_programm_bleibt_still() {
    let bytes = dokument(
        fontfile("StandardEncoding"),
        None,
        &format!("BT /F1 12 Tf 72 700 Td (IBAN: {SECRET}) Tj ET"),
    );
    let (runs, warnings) = analyse(&bytes);
    assert!(runs.iter().any(|r| r.text.contains(SECRET)));
    assert!(!warnings.iter().any(|w| ist_luecke(w)), "{warnings:?}");
}

/// CFF mit eigener Kodierung (Top DICT, Operator 16 mit Versatz 200) und
/// ohne `/ToUnicode`: eine Lücke. Mit `/ToUnicode`, oder mit
/// Standardkodierung (kein Operator 16), keine.
#[test]
fn cff_mit_eigener_kodierung_ist_ohne_to_unicode_eine_luecke() {
    let content = format!("BT /F1 12 Tf 72 700 Td (IBAN: {SECRET}) Tj ET");
    // 200 = (247 - 247) * 256 + 92 + 108
    let eigen = dokument(cff(&[247, 92, 16]), None, &content);
    let (_, warnings) = analyse(&eigen);
    assert!(
        warnings.iter().any(|w| ist_luecke(w) && w.contains("CFF")),
        "{warnings:?}"
    );

    // Standard: nur CharStrings (Operator 17, Wert 100).
    let standard = dokument(cff(&[239, 17]), None, &content);
    let (runs, warnings) = analyse(&standard);
    assert!(runs.iter().any(|r| r.text.contains(SECRET)));
    assert!(!warnings.iter().any(|w| ist_luecke(w)), "{warnings:?}");

    // Eine genannte Basis: das Programm spielt keine Rolle.
    let genannt = dokument(
        cff(&[247, 92, 16]),
        Some(Object::Name(b"WinAnsiEncoding".to_vec())),
        &content,
    );
    let (_, warnings) = analyse(&genannt);
    assert!(!warnings.iter().any(|w| ist_luecke(w)), "{warnings:?}");
}

/// Mit `/ToUnicode` sagt die Schrift selbst, was ihre Codes bedeuten.
#[test]
fn cff_mit_eigener_kodierung_und_to_unicode_bleibt_still() {
    let d_bytes = dokument_mit(
        cff(&[247, 92, 16]),
        None,
        Some(
            b"begincmap 1 begincodespacerange <00> <FF> endcodespacerange \
              1 beginbfrange <20> <7E> <0020> endbfrange endcmap",
        ),
        &format!("BT /F1 12 Tf 72 700 Td (IBAN: {SECRET}) Tj ET"),
    );
    let (runs, warnings) = analyse(&d_bytes);
    assert!(runs.iter().any(|r| r.text.contains(SECRET)));
    assert!(!warnings.iter().any(|w| ist_luecke(w)), "{warnings:?}");
}

/// Gegenprobe: die Standard-14-Schrift ohne Programm bleibt, wie sie war.
#[test]
fn standard_14_ohne_programm_bleibt_still() {
    let bytes = common::page(&[&format!("IBAN: {SECRET}")]).finish();
    let (runs, warnings) = analyse(&bytes);
    assert!(runs.iter().any(|r| r.text.contains(SECRET)));
    assert!(!warnings.iter().any(|w| ist_luecke(w)), "{warnings:?}");
}
