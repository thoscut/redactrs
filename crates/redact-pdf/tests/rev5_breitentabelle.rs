//! Was `/W` einer CID-Schrift **bauen** darf.
//!
//! Der Befund: `parse_cid_widths` deckelte den **einzelnen** `/W`-Bereich auf
//! 65 535 Codes, aber nicht die **Summe** über viele Bereiche. Ein Bereich ist
//! ein Zahlentripel von rund siebzehn Byte und ergibt bis zu 65 536 Einträge
//! einer `BTreeMap<u32, f64>`.
//!
//! Gemessen (Release, `VmHWM`, eine Schrift, ein `Tj`):
//!
//! | `/W`-Bereiche | Datei | vorher | nachher |
//! |---:|---:|---:|---:|
//! | 10 | 1,4 kB | 22 MB / 64 ms | 6 MB / 6 ms |
//! | 100 | 3,3 kB | 185 MB / 708 ms | 6 MB / 10 ms |
//! | 1 000 | 21 kB | ~1,9 GB (hochgerechnet) | 6 MB / 10 ms |
//!
//! `MAX_CACHED_FONT_ENTRIES` sah davon nichts: die Decke entscheidet, was
//! **liegen bleibt**, und sie wird gefragt, wenn die Tabelle schon gebaut ist.
//!
//! Die Grenze ist keine gegriffene Zahl, sondern die Norm: PDF 32000-1, 9.7.4.2
//! — „CIDs shall be in the range 0 to 65 535“. Eine Breite für einen CID
//! darüber kann kein Zeichen je abrufen.

use lopdf::{dictionary, Dictionary, Document, Object};
use redact_pdf::font::font_from_dict;

/// Eine Type0-Schrift, deren `/W` `ranges` Bereiche zu je 65 536 Codes
/// aufzählt — der erste bei 0, der nächste bei 65 536 und so fort.
fn cid_font_mit_bereichen(doc: &mut Document, ranges: usize) -> Dictionary {
    let mut w: Vec<Object> = Vec::new();
    for r in 0..ranges {
        let first = (r as i64) * 65_536;
        w.push(first.into());
        w.push((first + 65_535).into());
        w.push(500_i64.into());
    }
    let descendant = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "Test",
        "DW" => 1000_i64,
        "W" => Object::Array(w),
    });
    dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "Test",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(descendant)],
    }
}

/// **Der Befund:** hundert `/W`-Bereiche sind 3,3 kB Datei und waren 185 MB
/// Tabelle.
///
/// Nimmt man die CID-Grenze aus `parse_cid_widths` heraus, wiegt dieselbe
/// Schrift 6 553 600 Einträge statt 65 536, und dieser Test geht rot.
#[test]
fn viele_w_bereiche_ergeben_keine_groessere_tabelle_als_eine_ganze_schrift() {
    let mut doc = Document::with_version("1.7");
    let font = cid_font_mit_bereichen(&mut doc, 100);
    let info = font_from_dict(&doc, &font);
    assert!(
        info.weight() <= 65_536,
        "eine CID-Schrift kann höchstens 65 536 Codes benennen (PDF 32000-1, \
         9.7.4.2); gebaut wurden {} Einträge aus hundert `/W`-Bereichen.",
        info.weight()
    );
}

/// Und ein einzelner Bereich, der die Grenze überschreitet, wird an ihr
/// abgeschnitten — nicht erst bei `first + 65 535`.
#[test]
fn ein_bereich_endet_am_groessten_cid() {
    let mut doc = Document::with_version("1.7");
    let descendant = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "Test",
        "DW" => 1000_i64,
        "W" => Object::Array(vec![
            60_000_i64.into(),
            4_000_000_i64.into(),
            500_i64.into(),
        ]),
    });
    let font = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "Test",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(descendant)],
    };
    let info = font_from_dict(&doc, &font);
    assert_eq!(
        info.weight(),
        65_536 - 60_000,
        "von 60 000 bis 65 535, keinen Code weiter"
    );
}

/// **Die Gegenprobe:** eine gewöhnliche CID-Schrift verliert nichts.
///
/// Beide Schreibweisen von `/W` — die Liste und der Bereich — liefern
/// weiterhin genau die Breiten, die dastehen, und der Rest bleibt bei `/DW`.
#[test]
fn eine_gewoehnliche_schrift_behaelt_ihre_breiten() {
    let mut doc = Document::with_version("1.7");
    let descendant = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "Test",
        "DW" => 1000_i64,
        "W" => Object::Array(vec![
            // Liste: 1 → 500, 2 → 600
            1_i64.into(),
            Object::Array(vec![500_i64.into(), 600_i64.into()]),
            // Bereich: 10..=12 → 700
            10_i64.into(),
            12_i64.into(),
            700_i64.into(),
            // Und einmal quer über die obere Grenze hinweg.
            65_534_i64.into(),
            65_535_i64.into(),
            800_i64.into(),
        ]),
    });
    let font = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "Test",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(descendant)],
    };
    let info = font_from_dict(&doc, &font);
    assert_eq!(info.width(1, "a"), 0.5);
    assert_eq!(info.width(2, "b"), 0.6);
    assert_eq!(info.width(10, "c"), 0.7);
    assert_eq!(info.width(12, "d"), 0.7);
    assert_eq!(info.width(65_534, "e"), 0.8);
    assert_eq!(info.width(65_535, "f"), 0.8);
    // Nicht genannt: die Vorgabebreite aus `/DW`.
    assert_eq!(info.width(13, "g"), 1.0);
    assert_eq!(info.weight(), 2 + 3 + 2);
}
