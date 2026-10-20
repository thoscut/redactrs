//! Expertenreview: Lücken im Leck-Detektor (`--check-leaks`).
//!
//! Jeder Test hier war ein Fall, in dem der Detektor an einer
//! **ungeschwärzten** Datei „nicht gefunden“ meldete, ohne eine Stelle als
//! „nicht geprüft“ zu nennen — die eine Antwort, die er nie geben darf.

mod common;

use std::io::Write;

use common::{page, text_ops, SECRET};
use lopdf::{dictionary, Object, Stream};
use redact_pdf::leaks_many_within;

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// TIFF-Prädiktor 2 (PDF 32000-1, Tabelle 8), 8 Bit, eine Farbe: jedes Byte
/// minus seinem linken Nachbarn in derselben Zeile.
fn tiff2(data: &[u8], columns: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for row in data.chunks(columns) {
        let mut prev = 0u8;
        for &b in row {
            out.push(b.wrapping_sub(prev));
            prev = b;
        }
    }
    out
}

/// Ein Fund oder eine nicht geprüfte Stelle — alles andere wäre eine
/// falsche Entwarnung.
fn weder_fund_noch_luecke(bytes: &[u8]) -> bool {
    let check = leaks_many_within(bytes, &[SECRET], u64::MAX);
    check.findings[0].is_empty() && check.unchecked.is_empty()
}

/// Der Seiteninhalt hinter `/FlateDecode` mit `/Predictor 2`.
fn praediktor2_seite(secret: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let raw = text_ops(&[secret]);
    let columns = raw.len();
    let packed = deflate(&tiff2(&raw, columns));
    let stream = Stream::new(
        dictionary! {
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 2, "Columns" => columns as i64 },
        },
        packed,
    )
    .with_compression(false);
    d.doc.objects.insert(d.content_id, Object::Stream(stream));
    d.finish()
}

/// Ein Form-XObject hinter `/FlateDecode` mit `/Predictor 2` — gezeichnet
/// oder nur in den Ressourcen eingetragen.
fn praediktor2_formular(secret: &str, gezeichnet: bool) -> Vec<u8> {
    let mut d = page(&["Kontoauszug"]);
    let raw = text_ops(&[secret]);
    let columns = raw.len();
    let packed = deflate(&tiff2(&raw, columns));
    let form = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => Object::Reference(d.resources_id),
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 2, "Columns" => columns as i64 },
        },
        packed,
    )
    .with_compression(false);
    let form_id = d.add(Object::Stream(form));
    let resources = d.doc.get_dictionary_mut(d.resources_id).unwrap();
    resources.set("XObject", dictionary! { "X1" => form_id });
    if gezeichnet {
        let mut content = text_ops(&["Kontoauszug"]);
        content.extend_from_slice(b"q /X1 Do Q\n");
        d.set_content(&content);
    }
    d.finish()
}

/// `/Predictor 2` im Seiteninhalt: der Text steht erst nach dem
/// Zurückrechnen da. Der Detektor muss ihn finden oder die Stelle nennen.
#[test]
fn tiff_praediktor_2_im_seiteninhalt_wird_gefunden() {
    assert!(
        !weder_fund_noch_luecke(&praediktor2_seite(SECRET)),
        "weder Fund noch ungeprüfte Stelle"
    );
}

/// Dasselbe in einem Form-XObject; `pdftotext` liest den Text, sobald es
/// gezeichnet wird.
#[test]
fn tiff_praediktor_2_im_form_xobject_wird_gefunden() {
    for gezeichnet in [false, true] {
        assert!(
            !weder_fund_noch_luecke(&praediktor2_formular(SECRET, gezeichnet)),
            "gezeichnet={gezeichnet}: weder Fund noch ungeprüfte Stelle"
        );
    }
}

/// Ein gezeichnetes XObject, das der Schriftdekoder nicht lesen kann (hier:
/// ohne `/Subtype`), steht als Lücke der Sicht 7 in `unchecked`. Früher
/// verwarf diese Sicht alle Warnungen des Interpreters außer der ganz
/// abgelehnten Seite — und Glyphencodes darin fände keine Bytesicht.
#[test]
fn ein_unlesbares_xobject_ist_eine_luecke_des_schriftdekoders() {
    let mut d = page(&["Kontoauszug"]);
    let form = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        },
        text_ops(&["Glyphen"]),
    );
    let form_id = d.add(Object::Stream(form));
    let resources = d.doc.get_dictionary_mut(d.resources_id).unwrap();
    resources.set("XObject", dictionary! { "X1" => form_id });
    let mut content = text_ops(&["Kontoauszug"]);
    content.extend_from_slice(b"q /X1 Do Q\n");
    d.set_content(&content);
    let check = leaks_many_within(&d.finish(), &[SECRET], u64::MAX);
    assert!(
        check
            .unchecked
            .iter()
            .any(|line| line.starts_with("Sicht 7 (Schriftdekoder)") && line.contains("X1")),
        "{:?}",
        check.unchecked
    );
}

/// Gegenprobe: ein Form-XObject, das nirgends gezeichnet wird, ist **keine**
/// Lücke — es steht nicht auf dem Papier, und seinen Strom lesen die
/// Bytesichten. Eine Zeile dafür machte aus jeder Datei mit geteilten
/// Ressourcen einen Befund.
#[test]
fn ein_ungezeichnetes_form_xobject_ist_keine_luecke() {
    let check = leaks_many_within(&ungezeichnetes_formular(false), &[SECRET], u64::MAX);
    assert!(check.unchecked.is_empty(), "{:?}", check.unchecked);
}

/// Ein Form-XObject in `/Resources`, das kein `Do` zeichnet. Vor dem Text ein
/// Inline-Bild, dessen Bilddaten ein `(` enthalten können; der Text selbst
/// per `TJ` in Bruchstücke zerlegt, sodass nur die Verkettung (Sicht 6) ihn
/// zusammensetzt.
fn ungezeichnetes_formular(klammer_im_bild: bool) -> Vec<u8> {
    let mut d = page(&["Kontoauszug"]);
    let mut raw = Vec::new();
    raw.extend_from_slice(b"q BI /W 4 /H 1 /BPC 8 /CS /G ID ");
    raw.extend_from_slice(if klammer_im_bild {
        b"\x01(\x02\x03"
    } else {
        b"\x01\x05\x02\x03"
    });
    raw.extend_from_slice(
        b" EI Q\nBT /F1 10 Tf 72 600 Td [(DE89 3704) -10 (0044 0532) -10 ( 0130 00)] TJ ET\n",
    );
    let form = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => Object::Reference(d.resources_id),
        },
        raw,
    );
    let form_id = d.add(Object::Stream(form));
    let resources = d.doc.get_dictionary_mut(d.resources_id).unwrap();
    resources.set("XObject", dictionary! { "X2" => form_id });
    d.finish()
}

/// Ein `(` in den Bilddaten eines Inline-Bilds öffnete in Sicht 6 eine
/// Zeichenkette, die erst an der nächsten `)` endete — mitten im folgenden
/// `TJ`. Die IBAN dahinter fehlte dann in der Verkettung.
#[test]
fn eine_klammer_in_inline_bilddaten_bricht_die_verkettung_nicht() {
    for klammer in [false, true] {
        let bytes = ungezeichnetes_formular(klammer);
        let check = leaks_many_within(&bytes, &[SECRET], u64::MAX);
        assert!(
            check.findings[0]
                .iter()
                .any(|f| f.contains("Objekt 7 0") && f.contains("Zeichenketten-Verkettung")),
            "klammer={klammer}: {:#?}",
            check.findings
        );
    }
}

/// Dasselbe für einen Kommentar: `% (` ist keine Zeichenkette.
#[test]
fn eine_klammer_im_kommentar_bricht_die_verkettung_nicht() {
    let mut d = page(&[]);
    d.set_content(
        b"% Notiz (offen\nBT /F1 10 Tf 72 600 Td [(DE89 3704) -10 (0044 0532) -10 ( 0130 00)] TJ ET\n",
    );
    let check = leaks_many_within(&d.finish(), &[SECRET], u64::MAX);
    assert!(
        check.findings[0]
            .iter()
            .any(|f| f.contains("Objekt 3 0") && f.contains("Zeichenketten-Verkettung")),
        "{:#?}",
        check.findings
    );
}

/// Ein Name als Wert wird wie eine Zeichenkette verglichen — auch ohne
/// Leerraum. Vorher verglich ihn nur die Bytesicht, und die kennt keine
/// Fassung ohne Leerraum.
#[test]
fn ein_name_ohne_leerraum_wird_gefunden() {
    let mut d = page(&["Kontoauszug"]);
    d.zweiter_halter(Object::Name(b"DE89370400440532013000".to_vec()));
    let check = leaks_many_within(&d.finish(), &[SECRET], u64::MAX);
    assert!(!check.findings[0].is_empty(), "Name-Wert nicht gefunden");

    // Gegenprobe: derselbe Text als Zeichenkette.
    let mut d = page(&["Kontoauszug"]);
    d.zweiter_halter(Object::string_literal("DE89370400440532013000"));
    let check = leaks_many_within(&d.finish(), &[SECRET], u64::MAX);
    assert!(!check.findings[0].is_empty());
}

/// Ein Dictionary-Schlüssel ist Inhalt wie jeder Wert. In der Datei steht
/// er mit `#20` für jedes Leerzeichen — die Rohbytes treffen den Begriff
/// nicht, erst der gelesene Name.
#[test]
fn ein_dictionary_schluessel_wird_durchsucht() {
    let mut d = page(&["Kontoauszug"]);
    let mut inner = lopdf::Dictionary::new();
    inner.set(SECRET.as_bytes().to_vec(), Object::Boolean(true));
    d.zweiter_halter(Object::Dictionary(inner));
    let check = leaks_many_within(&d.finish(), &[SECRET], u64::MAX);
    assert!(
        check.findings[0].iter().any(|f| f.contains("Schlüssel")),
        "{:#?}",
        check.findings
    );
}
