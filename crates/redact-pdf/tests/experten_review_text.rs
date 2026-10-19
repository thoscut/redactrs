//! Expertenprüfung: Text, den ein Betrachter anders liest als der Extraktor.
//!
//! Drei Befunde dieser Runde:
//!
//! * **Schreibrichtung aus `Tm × CTM` allein.** `-10 Tf` mit `-1 0 0 -1 x y Tm`
//!   oder `-100 Tz` mit `-1 0 0 1 x y Tm` ergibt `Trm = diag(10, 10)`: jeder
//!   Betrachter zeigt aufrechten Text von links nach rechts. Der Extraktor nahm
//!   die Richtung ohne das Vorzeichen von `Tfs · Th`, las die Zeile rückwärts,
//!   fand die IBAN nicht — Rückgabewert 0, IBAN in der Ausgabe.
//! * **Spiegeltreffer auf erfundenen Kästen.** Ein `/ActualText` über
//!   unlesbaren Glyphen wurde gleichmäßig über den Abschnitt verteilt. Steht
//!   vor der IBAN schmaler Text, lagen ihre echten Glyphen außerhalb des
//!   erfundenen Kastens und blieben stehen.
//! * **`Indexed`-Farbraum mit riesigem Index.** `1e20 sc` ließ `index * n`
//!   überlaufen: Panik im Debug-Build, Slice-Panik im Release.
//!
//! Dazu die Proben, die schon vorher hielten und das auch weiter tun sollen:
//! Nachbarn behalten ihre Lage, Fettdruck durch Überdrucken wird gefunden,
//! ein Font namens `/BI` verwirrt den Inline-Bild-Leser nicht.

mod common;

use common::SECRET;
use lopdf::{dictionary, Object};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

fn extract(bytes: &[u8]) -> Result<Vec<TextRun>, String> {
    let doc = load_from_bytes(bytes).map_err(|e| e.to_string())?;
    PdfExtractor::new().extract(&doc).map_err(|e| e.to_string())
}

fn texts(runs: &[TextRun]) -> Vec<String> {
    runs.iter().map(|r| r.text.clone()).collect()
}

/// Die Schwärzung des ersten Vorkommens von `needle` — so, wie die Kette sie
/// aus einem Mustertreffer baut.
fn redaction_for(runs: &[TextRun], needle: &str) -> Option<Redaction> {
    runs.iter().find_map(|run| {
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
}

/// Schwärzen, Metadaten entfernen, speichern — wie die Kette.
fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> Result<Vec<u8>, String> {
    let mut doc = load_from_bytes(bytes).map_err(|e| e.to_string())?;
    PdfRedactor::new()
        .apply_with_report(&mut doc, redactions)
        .map_err(|e| e.to_string())?;
    strip_metadata(&mut doc);
    save_to_bytes(&doc).map_err(|e| e.to_string())
}

/// Eine Seite mit dem Standardfont unter `font_name` und dem Inhalt `raw`.
fn with_content(raw: &[u8], font_name: &str) -> Vec<u8> {
    let mut d = common::page(&[]);
    let font_id = d.font_id;
    d.doc
        .get_dictionary_mut(d.resources_id)
        .unwrap()
        .set("Font", dictionary! { font_name => font_id });
    d.set_content(raw);
    d.finish()
}

/// Die Glyphen von `needle` als `(Zeichen, x, y)` der linken unteren Ecke.
fn glyph_positions(runs: &[TextRun], needle: &str) -> Option<Vec<(char, f64, f64)>> {
    runs.iter().find_map(|run| {
        let pos = run.text.find(needle)?;
        let start = run.text[..pos].chars().count();
        let n = needle.chars().count();
        Some(
            run.glyphs[start..start + n]
                .iter()
                .map(|g| (g.ch, g.rect.ll.x, g.rect.ll.y))
                .collect(),
        )
    })
}

/// Findet, schwärzt und prüft: die IBAN ist weg, `BICXYZ` dahinter steht noch.
fn assert_found_and_removed(raw: &str) {
    let pdf = with_content(raw.as_bytes(), "F1");
    let runs = extract(&pdf).unwrap();
    let red = redaction_for(&runs, SECRET)
        .unwrap_or_else(|| panic!("IBAN wird nicht gefunden: {:?}", texts(&runs)));
    let out = pipeline(&pdf, &[red]).unwrap();
    assert!(
        leaks(&out, SECRET).is_empty(),
        "IBAN steht nach der Schwärzung noch in der Datei"
    );
    let after = extract(&out).unwrap();
    assert!(
        after.iter().any(|r| r.text.contains("BICXYZ")),
        "der Nachbar wurde mitgeschwärzt: {:?}",
        texts(&after)
    );
}

// ---------------------------------------------------------------------------
// Schreibrichtung mit negativem `Tfs · Th`
// ---------------------------------------------------------------------------

/// Negative Schriftgröße mit gespiegelter Textmatrix: `Trm = diag(10, 10)`,
/// der Betrachter zeigt gewöhnlichen, aufrechten Text von links nach rechts.
/// Gelesen, geschwärzt und der Nachbar bleibt.
#[test]
fn negative_schriftgroesse_mit_gespiegelter_textmatrix_liest_wie_ein_betrachter() {
    assert_found_and_removed(&format!(
        "BT /F1 -10 Tf -1 0 0 -1 72 700 Tm (IBAN: {SECRET} BICXYZ) Tj ET"
    ));
}

/// Dasselbe über `-100 Tz` und eine waagerecht gespiegelte Textmatrix.
#[test]
fn negative_horizontalskalierung_mit_gespiegelter_textmatrix_liest_wie_ein_betrachter() {
    assert_found_and_removed(&format!(
        "BT /F1 10 Tf -100 Tz -1 0 0 1 72 700 Tm (IBAN: {SECRET} BICXYZ) Tj ET"
    ));
}

/// Die Glyphen der gespiegelten Zeile stehen von links nach rechts, auf einer
/// Grundlinie — wie im Betrachter.
#[test]
fn gespiegelte_zeile_steht_von_links_nach_rechts() {
    let raw = format!("BT /F1 -10 Tf -1 0 0 -1 72 700 Tm (IBAN: {SECRET}) Tj ET");
    let runs = extract(&with_content(raw.as_bytes(), "F1")).unwrap();
    let glyphs = glyph_positions(&runs, SECRET).expect("IBAN gefunden");
    for pair in glyphs.windows(2) {
        assert!(
            pair[1].1 > pair[0].1,
            "nicht von links nach rechts: {glyphs:?}"
        );
        assert!(
            (pair[1].2 - pair[0].2).abs() < 0.01,
            "nicht auf einer Grundlinie: {glyphs:?}"
        );
    }
}

/// Ein negatives `Tc` bei positiver Schrift schiebt wirklich zurück — die
/// Korrektur der Richtung darf daran nichts ändern: gewöhnlicher Text mit
/// leichtem Unterschneiden wird weiter als eine Zeile gelesen.
#[test]
fn negativer_zeichenabstand_bei_positiver_schrift_bleibt_eine_zeile() {
    assert_found_and_removed(&format!(
        "BT /F1 10 Tf -0.3 Tc 72 700 Td (IBAN: {SECRET} BICXYZ) Tj ET"
    ));
}

// ---------------------------------------------------------------------------
// Treffer nur im Textspiegel
// ---------------------------------------------------------------------------

/// Eine Seite mit Identity-H-Font **ohne** `/ToUnicode`: jeder Code ist der
/// Unicode-Wert des Zeichens, lesbar ist davon nichts. Darüber ein
/// `/ActualText` mit `mirror`, die Glyphen tragen `line`.
fn mirror_over_unreadable_glyphs(mirror: &str, line: &str) -> Vec<u8> {
    let mut d = common::page(&[]);
    let cid = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "Foo",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
        "DW" => 556,
        // Schmale Glyphen vor der IBAN: Leerzeichen, Doppelpunkt, `i`.
        "W" => vec![
            Object::Integer(0x20), Object::Array(vec![Object::Integer(278)]),
            Object::Integer(0x3A), Object::Array(vec![Object::Integer(278)]),
            Object::Integer(0x69), Object::Array(vec![Object::Integer(222)]),
        ],
    }));
    let f = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Foo",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(cid)],
    }));
    d.doc
        .get_dictionary_mut(d.resources_id)
        .unwrap()
        .set("Font", dictionary! { "F1" => f });
    let hex: String = line.chars().map(|c| format!("{:04X}", c as u32)).collect();
    let raw =
        format!("/Span << /ActualText ({mirror}) >> BDC BT /F1 10 Tf 72 700 Td <{hex}> Tj ET EMC");
    d.set_content(raw.as_bytes());
    d.finish()
}

/// Die Zeichen, deren Codes nach der Schwärzung noch im Seiteninhalt stehen.
fn remaining_codes(pdf: &[u8]) -> String {
    let doc = load_from_bytes(pdf).unwrap();
    let scan = redact_pdf::scan_page(&doc, doc.get_pages()[&1]).unwrap();
    scan.shows
        .iter()
        .flat_map(|s| s.glyphs())
        .filter_map(|g| char::from_u32(((g.bytes[0] as u32) << 8) | g.bytes[1] as u32))
        .collect()
}

fn assert_mirror_match_removes_secret(mirror: &str, line: &str) {
    let pdf = mirror_over_unreadable_glyphs(mirror, line);
    let runs = extract(&pdf).unwrap();
    let red = redaction_for(&runs, SECRET).expect("im Spiegel gefunden");
    let out = pipeline(&pdf, &[red]).unwrap();
    let left = remaining_codes(&out);
    assert!(
        !left.chars().any(|c| "DE0123456789".contains(c)),
        "Glyphen der IBAN stehen noch im Strom: {left:?}"
    );
}

/// Spiegel und Glyphen haben gleich viele Zeichen: jedes Spiegelzeichen
/// bekommt den Kasten seiner Glyphe, die IBAN verschwindet ganz — auch wenn
/// schmaler Text davor steht.
#[test]
fn spiegeltreffer_entfernt_alle_glyphen_des_geheimnisses() {
    let line = format!("iiiiiiiiii: {SECRET}");
    assert_mirror_match_removes_secret(&line, &line);
}

/// Spiegel und Glyphen haben verschieden viele Zeichen: die Zuordnung ist
/// unbekannt, geschwärzt wird der ganze Abschnitt.
#[test]
fn spiegeltreffer_ohne_eins_zu_eins_zuordnung_schwaerzt_den_abschnitt() {
    let line = format!("iiiiiiiiii: {SECRET}");
    assert_mirror_match_removes_secret(&format!("Konto {line}"), &line);
}

// ---------------------------------------------------------------------------
// Indexed-Farbraum
// ---------------------------------------------------------------------------

/// `sc` mit riesigem Index in einen `Indexed`-Farbraum: `index * n` lief über.
#[test]
fn riesiger_indexed_farbindex_fuehrt_nicht_zur_panik() {
    let mut d = common::page(&[]);
    let raw = format!(
        "/CS0 cs 100000000000000000000.0 sc 0 0 10 10 re f\nBT /F1 10 Tf 72 700 Td (IBAN: {SECRET}) Tj ET\n"
    );
    d.set_content(raw.as_bytes());
    d.doc.get_dictionary_mut(d.resources_id).unwrap().set(
        "ColorSpace",
        dictionary! {
            "CS0" => vec![
                Object::Name(b"Indexed".to_vec()),
                Object::Name(b"DeviceRGB".to_vec()),
                Object::Integer(1),
                Object::String(vec![0, 0, 0, 255, 255, 255], lopdf::StringFormat::Hexadecimal),
            ]
        },
    );
    let pdf = d.finish();
    let runs = extract(&pdf).unwrap();
    let red = redaction_for(&runs, SECRET).expect("IBAN gefunden");
    let out = std::panic::catch_unwind(|| pipeline(&pdf, &[red]))
        .expect("Schwärzung panikt bei `1e20 sc` in einem Indexed-Farbraum")
        .unwrap();
    assert!(leaks(&out, SECRET).is_empty());
}

// ---------------------------------------------------------------------------
// Proben, die schon hielten
// ---------------------------------------------------------------------------

/// Ein Font, der `/BI` heißt (Bold-Italic), und ein Inline-Bild weiter hinten:
/// der Name darf nicht als Beginn eines Inline-Bildes gelesen werden.
#[test]
fn font_namens_bi_vor_einem_inline_bild() {
    let mut raw = Vec::new();
    raw.extend_from_slice(format!("BT /BI 10 Tf 72 700 Td (IBAN: {SECRET}) Tj ET\n").as_bytes());
    raw.extend_from_slice(b"q 20 0 0 20 300 780 cm\nBI /W 2 /H 2 /CS /G /BPC 8 ID ");
    raw.extend_from_slice(&[0x00, 0xff, 0x7f, 0x30]);
    raw.extend_from_slice(b" EI Q\n");
    let pdf = with_content(&raw, "BI");
    let runs = extract(&pdf).expect("eine gewöhnliche Datei muss lesbar sein");
    let red = redaction_for(&runs, SECRET).expect("IBAN gefunden");
    let out = pipeline(&pdf, &[red]).unwrap();
    assert!(leaks(&out, SECRET).is_empty());
}

/// Schwärzt die IBAN und prüft, dass `BICXYZ` dahinter an seinem Platz bleibt.
fn check_layout(name: &str, pdf: &[u8]) -> Result<(), String> {
    let runs = extract(pdf)?;
    let before = glyph_positions(&runs, "BICXYZ").ok_or(format!("{name}: BIC fehlt vorher"))?;
    let red = redaction_for(&runs, SECRET).ok_or(format!("{name}: IBAN nicht gefunden"))?;
    let out = pipeline(pdf, &[red])?;
    if !leaks(&out, SECRET).is_empty() {
        return Err(format!("{name}: IBAN steht noch in der Datei"));
    }
    let after =
        glyph_positions(&extract(&out)?, "BICXYZ").ok_or(format!("{name}: BIC fehlt nachher"))?;
    for (b, a) in before.iter().zip(&after) {
        if (b.1 - a.1).abs() > 0.05 || (b.2 - a.2).abs() > 0.05 {
            return Err(format!(
                "{name}: {} wanderte von ({:.2}, {:.2}) nach ({:.2}, {:.2})",
                b.0, b.1, b.2, a.1, a.2
            ));
        }
    }
    Ok(())
}

/// Nach der Schwärzung stehen die Nachbarn an derselben Stelle — bei allen
/// Textzustandsparametern, die den Vorschub beeinflussen.
#[test]
fn nachbarn_behalten_ihre_lage() {
    let s = SECRET;
    let cases: Vec<(&str, String)> = vec![
        (
            "schlicht",
            format!("BT /F1 10 Tf 72 700 Td (IBAN: {s} BICXYZ) Tj ET"),
        ),
        (
            "tc_tw",
            format!("BT /F1 10 Tf 1.5 Tc 4 Tw 72 700 Td (IBAN: {s} BICXYZ) Tj ET"),
        ),
        (
            "tz",
            format!("BT /F1 10 Tf 70 Tz 3 Tw 72 700 Td (IBAN: {s} BICXYZ) Tj ET"),
        ),
        (
            "tj_kerning",
            format!("BT /F1 10 Tf 72 700 Td [(IBAN: ) -120 ({s}) 50 ( BICXYZ)] TJ ET"),
        ),
        (
            "apostroph",
            format!("BT /F1 10 Tf 12 TL 72 712 Td (IBAN: {s} BICXYZ) ' ET"),
        ),
        (
            "anfuehrung",
            format!("BT /F1 10 Tf 12 TL 72 712 Td 5 1 (IBAN: {s} BICXYZ) \" ET"),
        ),
        (
            "anfuehrung_dann_tj",
            format!("BT /F1 10 Tf 12 TL 72 712 Td 5 1 (IBAN: {s}) \" (  BICXYZ) Tj ET"),
        ),
        (
            "hochstellung",
            format!("BT /F1 10 Tf 3 Ts 72 700 Td (IBAN: {s} BICXYZ) Tj ET"),
        ),
        (
            "skalierte_tm",
            format!("BT /F1 1 Tf 10 0 0 10 72 700 Tm 0.2 Tw (IBAN: {s} BICXYZ) Tj ET"),
        ),
        (
            "gedreht",
            format!("BT /F1 10 Tf 0 1 -1 0 300 300 Tm 2 Tw (IBAN: {s} BICXYZ) Tj ET"),
        ),
        (
            "geteilt",
            "BT /F1 10 Tf 72 700 Td (IBAN: DE89 3704 ) Tj (0044 0532 0130 00 BICXYZ) Tj ET"
                .to_string(),
        ),
        (
            "negative_tfs",
            format!("BT /F1 -10 Tf -1 0 0 -1 72 700 Tm 1 Tc (IBAN: {s} BICXYZ) Tj ET"),
        ),
        (
            "negative_tz",
            format!("BT /F1 10 Tf -100 Tz -1 0 0 1 72 700 Tm 3 Tw (IBAN: {s} BICXYZ) Tj ET"),
        ),
    ];
    let errors: Vec<String> = cases
        .iter()
        .filter_map(|(name, raw)| check_layout(name, &with_content(raw.as_bytes(), "F1")).err())
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

/// Dasselbe mit einem Type0-Font (Identity-H, `/ToUnicode`), zwei Byte je Code.
#[test]
fn type0_nachbarn_behalten_ihre_lage() {
    type Content = Box<dyn Fn(&Hex) -> String>;
    let cases: Vec<(&str, Content)> = vec![
        (
            "tj_tc",
            Box::new(|h| {
                format!(
                    "BT /F1 10 Tf 1 Tc 72 700 Td {} Tj ET",
                    h(&format!("IBAN: {SECRET} BICXYZ"))
                )
            }),
        ),
        (
            "tj_tw",
            Box::new(|h| {
                format!(
                    "BT /F1 10 Tf 5 Tw 72 700 Td {} Tj ET",
                    h(&format!("IBAN: {SECRET} BICXYZ"))
                )
            }),
        ),
        (
            "tj_array",
            Box::new(|h| {
                format!(
                    "BT /F1 10 Tf 72 700 Td [{} -300 {} 20 {}] TJ ET",
                    h("IBAN: "),
                    h(SECRET),
                    h(" BICXYZ")
                )
            }),
        ),
    ];
    let errors: Vec<String> = cases
        .iter()
        .filter_map(|(name, content)| check_layout(name, &type0_pdf(content)).err())
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

/// Schreibt Text als Hex-String mit zwei Byte je Zeichen.
type Hex = dyn Fn(&str) -> String;

/// Eine Seite mit Type0-Font; `content` bekommt den [`Hex`]-Schreiber.
fn type0_pdf(content: &dyn Fn(&Hex) -> String) -> Vec<u8> {
    let mut d = common::page(&[]);
    let tu = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
1 beginbfrange <0020> <007E> <0020> endbfrange\n\
endcmap CMapName currentdict /CMap defineresource pop end end\n"
        .to_vec();
    let tu_id = d.add(Object::Stream(lopdf::Stream::new(dictionary! {}, tu)));
    let cid = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "Foo",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
        "DW" => 600,
    }));
    let f = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Foo",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(cid)],
        "ToUnicode" => tu_id,
    }));
    d.doc
        .get_dictionary_mut(d.resources_id)
        .unwrap()
        .set("Font", dictionary! { "F1" => f });
    let hex = |s: &str| -> String {
        format!(
            "<{}>",
            s.chars()
                .map(|c| format!("{:04X}", c as u32))
                .collect::<String>()
        )
    };
    d.set_content(content(&hex).as_bytes());
    d.finish()
}

/// Fettdruck durch Überdrucken, Zeichen für Zeichen gesetzte Zeilen und
/// Render-Modus 2: jede Fassung wird gefunden, und nach dem Schwärzen aller
/// Vorkommen steht die IBAN nirgends mehr.
#[test]
fn fettdruck_durch_ueberdrucken_wird_gefunden() {
    let s = SECRET;
    let per_char = |reverse: bool| {
        let chars: Vec<char> = format!("IBAN: {s}").chars().collect();
        let mut order: Vec<usize> = (0..chars.len()).collect();
        if reverse {
            order.reverse();
        }
        let mut out = String::from("BT /F1 10 Tf ");
        for i in order {
            out.push_str(&format!(
                "1 0 0 1 {:.2} 700 Tm ({}) Tj ",
                72.0 + 6.0 * i as f64,
                chars[i]
            ));
        }
        out.push_str("ET");
        out
    };
    let cases: Vec<(&str, String)> = vec![
        ("dx0.3", format!("BT /F1 10 Tf 72 700 Td (IBAN: {s}) Tj ET BT /F1 10 Tf 72.3 700 Td (IBAN: {s}) Tj ET")),
        ("dx0.5_dy0.3", format!("BT /F1 10 Tf 72 700 Td (IBAN: {s}) Tj ET BT /F1 10 Tf 72.5 700.3 Td (IBAN: {s}) Tj ET")),
        ("verschraenkt", format!("BT /F1 10 Tf 72 700 Td (IBAN: ) Tj ET BT /F1 10 Tf 72.3 700 Td (IBAN: ) Tj ET BT /F1 10 Tf 100 700 Td ({s}) Tj ET BT /F1 10 Tf 100.3 700 Td ({s}) Tj ET")),
        ("dreifach", format!("BT /F1 10 Tf 72 700 Td (IBAN: {s}) Tj ET BT /F1 10 Tf 72.2 700 Td (IBAN: {s}) Tj ET BT /F1 10 Tf 72.4 700 Td (IBAN: {s}) Tj ET")),
        ("tr2", format!("BT 2 Tr /F1 10 Tf 72 700 Td (IBAN: {s}) Tj ET")),
        ("je_zeichen_tm", per_char(false)),
        ("je_zeichen_tm_rueckwaerts", per_char(true)),
    ];
    let mut errors = Vec::new();
    for (name, raw) in &cases {
        let pdf = with_content(raw.as_bytes(), "F1");
        let runs = extract(&pdf).unwrap();
        let reds: Vec<Redaction> = runs
            .iter()
            .filter_map(|r| redaction_for(std::slice::from_ref(r), SECRET))
            .collect();
        if reds.is_empty() {
            errors.push(format!("{name}: nicht gefunden {:?}", texts(&runs)));
            continue;
        }
        let out = pipeline(&pdf, &reds).unwrap();
        if !leaks(&out, SECRET).is_empty() {
            errors.push(format!(
                "{name}: IBAN steht nach der Schwärzung noch in der Datei"
            ));
        }
    }
    assert!(errors.is_empty(), "{errors:#?}");
}
