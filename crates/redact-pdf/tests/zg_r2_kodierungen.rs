//! Gegenprüfung R2 (nach Fix-Runde 6): die **Kodierungstabelle** des Orakels
//! — eigene Tabelle, eigene Kodierer, Zeichen für Zeichen verglichen.
//!
//! Der Modulkopf von `audit_bytes` sagt zu: „Beide PDF-String-Kodierungen
//! werden berücksichtigt: PDFDocEncoding/Latin-1 **und** UTF-16 (BE wie LE,
//! mit und ohne BOM). Ebenso beide Syntaxen: literal `(DE89…)` und
//! hexadezimal `<44453839…>`.“
//!
//! Hier steht dieselbe Tabelle noch einmal, unabhängig gebaut: bis zu zehn
//! Fassungen je Begriff, jede in einem eigenen Flate-Strom. Geprüft wird nicht nur
//! „gefunden“, sondern die **Beschriftung** der Fundstelle — sie nennt die
//! Kodierung, und ein Orakel, das die Fassung findet, sie aber falsch
//! benennt, hilft beim Suchen der Ursache nicht.
//!
//! Der zweite Begriff trägt Umlaute: dort fallen Latin-1 und UTF-8
//! auseinander, und die Latin-1-Fassung ist eine eigene Zeile der Tabelle
//! (bei reinem ASCII fällt sie mit UTF-8 zusammen und wird verworfen).

mod common;

use std::io::Write;

use common::{page, SECRET};
use lopdf::{dictionary, Object, Stream};
use redact_pdf::leaks_many_within;

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("komprimierbar");
    e.finish().expect("komprimierbar")
}

// ---------------------------------------------------------------------------
// Meine Kodierer
// ---------------------------------------------------------------------------

fn hex(data: &[u8], gross: bool) -> Vec<u8> {
    let ziffern: &[u8; 16] = if gross {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut out = Vec::with_capacity(data.len() * 2);
    for &b in data {
        out.push(ziffern[usize::from(b >> 4)]);
        out.push(ziffern[usize::from(b & 0x0f)]);
    }
    out
}

/// Latin-1: ein Byte je Zeichen — nur für Text, dessen Zeichen unter U+0100
/// liegen.
fn latin1(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(c as u32).expect("< 0x100"))
        .collect()
}

fn utf16(text: &str, gross_endian: bool) -> Vec<u8> {
    text.encode_utf16()
        .flat_map(|u| {
            let [a, b] = u.to_be_bytes();
            if gross_endian {
                [a, b]
            } else {
                [b, a]
            }
        })
        .collect()
}

/// Meine Tabelle: (Beschriftung, wie das Orakel sie nennen muss, Bytes).
fn tabelle(text: &str) -> Vec<(&'static str, Vec<u8>)> {
    let utf8 = text.as_bytes().to_vec();
    let l1 = latin1(text);
    let be = utf16(text, true);
    let le = utf16(text, false);
    let mut out = vec![("UTF-8/ASCII", utf8.clone())];
    if l1 != utf8 {
        out.push(("Latin-1/PDFDoc", l1.clone()));
    }
    out.extend([
        ("Hex-String (Latin-1, gross)", hex(&l1, true)),
        ("Hex-String (Latin-1, klein)", hex(&l1, false)),
        ("UTF-16BE", be.clone()),
        ("Hex-String (UTF-16BE, gross)", hex(&be, true)),
        ("Hex-String (UTF-16BE, klein)", hex(&be, false)),
        ("UTF-16LE", le.clone()),
        ("Hex-String (UTF-16LE, gross)", hex(&le, true)),
        ("Hex-String (UTF-16LE, klein)", hex(&le, false)),
    ]);
    // Wie das Orakel (`Needle::new`: `variants.dedup_by(|a, b| a.1 == b.1)`):
    // zwei benachbarte Fassungen mit denselben Bytes sind **eine**, und
    // gemeldet wird die erste. Bei einer Ziffernfolge wie der IBAN enthält
    // kein Hex-String einen Buchstaben — „gross“ und „klein“ fallen dort
    // zusammen, und die Beschriftung lautet immer „gross“.
    out.dedup_by(|a, b| a.1 == b.1);
    out
}

/// Ein Strom je Fassung, jeder unter `/FlateDecode` — so muss die Objektsicht
/// ihn entpacken, bevor sie ihn lesen kann.
fn datei(fassungen: &[(&'static str, Vec<u8>)]) -> (Vec<u8>, Vec<String>) {
    let mut d = page(&["harmlos"]);
    let mut orte = Vec::new();
    for (_, bytes) in fassungen {
        let mut inhalt = b"Anhang: ".to_vec();
        inhalt.extend_from_slice(bytes);
        inhalt.extend_from_slice(b" (Ende)");
        let id = d.add(Object::Stream(
            Stream::new(
                dictionary! { "Filter" => Object::Name(b"FlateDecode".to_vec()) },
                zlib(&inhalt),
            )
            .with_compression(false),
        ));
        d.catalog_set(&format!("R2K{}", id.0), Object::Reference(id));
        orte.push(format!(
            "Objekt {} {} <Stream, dekodiert: FlateDecode>",
            id.0, id.1
        ));
    }
    (d.finish(), orte)
}

// ---------------------------------------------------------------------------
// Der Vergleich
// ---------------------------------------------------------------------------

/// Jede Fassung der Tabelle wird gefunden — **und** mit ihrem eigenen Namen
/// beschriftet, an der Objektsicht des Stroms, der sie trägt.
///
/// Mutationsnachweis (gefahren, in `audit_bytes::Needle::new`): die drei
/// `variants.push(("UTF-16LE"…))`-Zeilen gestrichen → rot (drei Fassungen
/// ohne Fund). Ebenso rot, wenn eine Fassung zwar gefunden, aber falsch
/// beschriftet wird.
#[test]
fn r2_jede_fassung_der_tabelle_wird_gefunden_und_benannt() {
    for text in [SECRET, "Jürgen Müßig, Straße 3"] {
        let fassungen = tabelle(text);
        assert!(
            fassungen.len() >= 6,
            "{text}: {} Fassungen",
            fassungen.len()
        );
        let (pdf, orte) = datei(&fassungen);
        let check = leaks_many_within(&pdf, &[text], u64::MAX);
        assert!(check.unchecked.is_empty(), "{text}: {:#?}", check.unchecked);

        for ((wie, _), ort) in fassungen.iter().zip(&orte) {
            let erwartet = format!("{ort} [Inhalt, {wie}]: …");
            assert!(
                check.findings[0].iter().any(|m| m.starts_with(&erwartet)),
                "{text}/{wie}: keine Fundstelle mit dieser Beschriftung.\n\
                 erwartet Anfang: {erwartet}\nist: {:#?}",
                check.findings[0]
                    .iter()
                    .filter(|m| m.starts_with(ort.as_str()))
                    .collect::<Vec<_>>()
            );
        }
    }
}

/// Die Tabelle selbst, gegen bekannte Proben — sonst wäre der Vergleich oben
/// ein Vergleich mit sich selbst.
#[test]
fn r2_die_kodierungstabelle_ist_kalibriert() {
    assert_eq!(hex(b"\x00\xde\x89", true), b"00DE89".to_vec());
    assert_eq!(hex(b"\x00\xde\x89", false), b"00de89".to_vec());
    assert_eq!(latin1("Aü"), vec![b'A', 0xfc]);
    assert_eq!(utf16("Aü", true), vec![0, b'A', 0, 0xfc]);
    assert_eq!(utf16("Aü", false), vec![b'A', 0, 0xfc, 0]);
    assert_eq!("Aü".as_bytes(), b"A\xc3\xbc");
    // Bei reinem ASCII fällt Latin-1 mit UTF-8 zusammen und entfällt; und in
    // einer reinen Ziffernfolge hat kein Hex-String einen Buchstaben, sodass
    // „gross“ und „klein“ dieselben Bytes sind: 6 statt 9 Fassungen.
    assert_eq!(
        tabelle(SECRET).iter().map(|(w, _)| *w).collect::<Vec<_>>(),
        vec![
            "UTF-8/ASCII",
            "Hex-String (Latin-1, gross)",
            "UTF-16BE",
            "Hex-String (UTF-16BE, gross)",
            "UTF-16LE",
            "Hex-String (UTF-16LE, gross)",
        ]
    );
    assert_eq!(tabelle("Jürgen").len(), 10);
}
