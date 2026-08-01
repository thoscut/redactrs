//! Nachweise zur Font-Behandlung: Breiten-Rangfolge und Dekodierbarkeit.

use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};
use redact_pdf::font::font_from_dict;

#[test]
fn type0_without_dw_uses_the_spec_default_not_the_name_guess() {
    let mut doc = Document::new();
    let descendant = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "ABCDEF+Helvetica",
    });
    let font = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "ABCDEF+Helvetica",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(descendant)],
    };
    let info = font_from_dict(&doc, &font);
    // PDF 32000-1, 9.7.4.3: ohne /DW gilt 1000.
    assert_eq!(info.width(b'4' as u32, "4"), 1.0);
}

#[test]
fn simple_font_with_widths_does_not_fall_back_to_the_name_guess() {
    let doc = Document::new();
    let font = dictionary! {
        "Type" => "Font",
        "Subtype" => "TrueType",
        "BaseFont" => "ABCDEF+SomeSans",
        "FirstChar" => 48_i64,
        "Widths" => vec![Object::Integer(250)],
    };
    let info = font_from_dict(&doc, &font);
    assert_eq!(info.width(b'0' as u32, "0"), 0.25);
    // Code außerhalb von /Widths: /MissingWidth (Vorgabe 0), nicht Helvetica.
    assert_eq!(info.width(b'M' as u32, "M"), 0.0);
}

/// Baut ein PDF mit einem simplen Font, dessen `/Widths` bewusst von der
/// Standard-14-Tabelle abweichen, und zwei getrennt positionierten Runs.
fn deviating_widths_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    // Alle Codes 32..126 sind 250/1000 breit — Helvetica hätte für Buchstaben
    // 500..750. Wer die Namensschätzung benutzt, schiebt den Stift pro Zeichen
    // um bis zu 0,5 em zu weit nach rechts.
    let widths: Vec<Object> = (32u32..127).map(|_| Object::Integer(250)).collect();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "TrueType",
        "BaseFont" => "ABCDEF+SchmalSans",
        "Encoding" => "WinAnsiEncoding",
        "FirstChar" => 32_i64,
        "LastChar" => 126_i64,
        "Widths" => widths,
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });

    // „Kontonummer:“ ab x=72, 10 pt, 0,25 em je Zeichen = 2,5 pt je Zeichen.
    let label = "Kontonummer:";
    let number = "12345678901";
    let start = 72.0f64;
    let size = 10.0f64;
    let next_x = start + label.len() as f64 * 0.25 * size + 5.0;

    let mut operations = vec![Operation::new("BT", vec![])];
    for (x, text) in [(start, label), (next_x, number)] {
        operations.push(Operation::new(
            "Tf",
            vec![Object::Name(b"F1".to_vec()), Object::Real(size as f32)],
        ));
        operations.push(Operation::new(
            "Tm",
            vec![
                Object::Real(1.0),
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(1.0),
                Object::Real(x as f32),
                Object::Real(700.0),
            ],
        ));
        operations.push(Operation::new(
            "Tj",
            vec![Object::String(
                text.as_bytes().to_vec(),
                StringFormat::Literal,
            )],
        ));
    }
    operations.push(Operation::new("ET", vec![]));

    let content = Content { operations }.encode().expect("kodierbar");
    let content_id = doc.add_object(Stream::new(dictionary! {}, content));
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

    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).expect("speicherbar");
    buffer
}

#[test]
fn deviating_widths_keep_the_glyph_order() {
    let bytes = deviating_widths_pdf();
    let doc = redact_pdf::load_from_bytes(&bytes).expect("ladbar");
    let runs = redact_pdf::PdfExtractor::new().extract(&doc).expect("Text");
    let text: String = runs.iter().map(|r| r.text.clone()).collect();
    assert!(
        text.contains("Kontonummer:") && text.contains("12345678901"),
        "verwürfelt: {text:?}"
    );
}

#[test]
fn cid_font_with_dw_drives_the_pen() {
    let mut doc = Document::new();
    let descendant = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "ABCDEF+Helvetica",
        "DW" => 750,
    });
    let font = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "ABCDEF+Helvetica",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(descendant)],
    };
    let info = font_from_dict(&doc, &font);
    for code in [0x20u32, b'M' as u32, b'0' as u32, 0x1234] {
        assert_eq!(info.width(code, "M"), 0.75, "Code {code}");
    }
}

// ---------------------------------------------------------------------------
// Identity-H ohne /ToUnicode: dekodieren, wo es geht — sonst warnen
// ---------------------------------------------------------------------------

/// Minimaler sfnt-Font, der nur eine `cmap` (Format 4, Plattform 3/1) trägt.
fn sfnt_with_cmap(pairs: &[(char, u16)]) -> Vec<u8> {
    let mut segs: Vec<(u16, u16)> = pairs.iter().map(|(c, g)| (*c as u16, *g)).collect();
    segs.sort_unstable();
    segs.push((0xFFFF, 1));
    let seg_count = segs.len();

    let mut sub: Vec<u8> = Vec::new();
    let push16 = |v: &mut Vec<u8>, n: u16| v.extend_from_slice(&n.to_be_bytes());
    push16(&mut sub, 4); // format
    push16(&mut sub, 0); // length (gleich korrigiert)
    push16(&mut sub, 0); // language
    push16(&mut sub, seg_count as u16 * 2);
    push16(&mut sub, 2); // searchRange (für uns unerheblich)
    push16(&mut sub, 0); // entrySelector
    push16(&mut sub, 0); // rangeShift
    for (code, _) in &segs {
        push16(&mut sub, *code); // endCode
    }
    push16(&mut sub, 0); // reservedPad
    for (code, _) in &segs {
        push16(&mut sub, *code); // startCode
    }
    for (code, gid) in &segs {
        push16(&mut sub, gid.wrapping_sub(*code)); // idDelta
    }
    for _ in &segs {
        push16(&mut sub, 0); // idRangeOffset
    }
    let len = sub.len() as u16;
    sub[2..4].copy_from_slice(&len.to_be_bytes());

    let mut cmap: Vec<u8> = Vec::new();
    push16(&mut cmap, 0); // version
    push16(&mut cmap, 1); // numTables
    push16(&mut cmap, 3); // platformID
    push16(&mut cmap, 1); // encodingID
    cmap.extend_from_slice(&12u32.to_be_bytes()); // offset
    cmap.extend_from_slice(&sub);

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // sfntVersion
    push16(&mut out, 1); // numTables
    push16(&mut out, 16);
    push16(&mut out, 0);
    push16(&mut out, 0);
    out.extend_from_slice(b"cmap");
    out.extend_from_slice(&0u32.to_be_bytes()); // checkSum
    out.extend_from_slice(&28u32.to_be_bytes()); // offset
    out.extend_from_slice(&(cmap.len() as u32).to_be_bytes());
    out.extend_from_slice(&cmap);
    out
}

/// Text → CIDs ab `first`, in der Reihenfolge des ersten Auftretens (wie ein
/// echter Subset-Font sie vergibt).
fn subset_cids_from(text: &str, first: u16) -> (Vec<(char, u16)>, Vec<u16>) {
    let mut pairs: Vec<(char, u16)> = Vec::new();
    let mut cids = Vec::new();
    for ch in text.chars() {
        let gid = match pairs.iter().find(|(c, _)| *c == ch) {
            Some((_, g)) => *g,
            None => {
                let g = pairs.len() as u16 + first;
                pairs.push((ch, g));
                g
            }
        };
        cids.push(gid);
    }
    (pairs, cids)
}

fn subset_cids(text: &str) -> (Vec<(char, u16)>, Vec<u16>) {
    subset_cids_from(text, 1)
}

/// Ein einseitiges PDF mit einem Type0/Identity-H-Font ohne `/ToUnicode`.
fn identity_subset_pdf(text: &str, embed: bool) -> Vec<u8> {
    identity_subset_pdf_from(text, embed, 1)
}

fn identity_subset_pdf_from(text: &str, embed: bool, first_cid: u16) -> Vec<u8> {
    let (pairs, cids) = subset_cids_from(text, first_cid);
    let mut doc = Document::with_version("1.5");

    let mut descriptor = dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "ABCDEF+SubsetSans",
        "Flags" => 4_i64,
    };
    if embed {
        let program = sfnt_with_cmap(&pairs);
        let file = doc.add_object(Stream::new(dictionary! {}, program).with_compression(false));
        descriptor.set("FontFile2", Object::Reference(file));
    }
    let descriptor_id = doc.add_object(descriptor);
    let descendant = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "ABCDEF+SubsetSans",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0_i64,
        },
        "DW" => 500,
        "CIDToGIDMap" => "Identity",
        "FontDescriptor" => Object::Reference(descriptor_id),
    });
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "ABCDEF+SubsetSans",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(descendant)],
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });

    let bytes: Vec<u8> = cids.iter().flat_map(|c| c.to_be_bytes()).collect();
    let operations = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), Object::Real(10.0)]),
        Operation::new(
            "Tm",
            vec![
                Object::Real(1.0),
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(1.0),
                Object::Real(72.0),
                Object::Real(700.0),
            ],
        ),
        Operation::new("Tj", vec![Object::String(bytes, StringFormat::Literal)]),
        Operation::new("ET", vec![]),
    ];
    let content = Content { operations }.encode().expect("kodierbar");
    let content_id = doc.add_object(Stream::new(dictionary! {}, content));
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

    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).expect("speicherbar");
    buffer
}

const SUBSET_LINE: &str = "Kontonummer 532013000";

#[test]
fn identity_subset_is_decoded_from_the_embedded_cmap() {
    let bytes = identity_subset_pdf(SUBSET_LINE, true);
    let doc = redact_pdf::load_from_bytes(&bytes).expect("ladbar");
    let (runs, warnings) = redact_pdf::PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Text");
    let text: String = runs.iter().map(|r| r.text.clone()).collect();
    assert_eq!(text, SUBSET_LINE);
    assert!(warnings.is_empty(), "unerwartete Warnung: {warnings:?}");
    // Gegenprobe: der Extractor-Einstieg ohne Warnungen liefert dasselbe.
    let plain: String = redact_pdf::PdfExtractor::new()
        .extract(&doc)
        .expect("Text")
        .iter()
        .map(|r| r.text.clone())
        .collect();
    assert_eq!(plain, SUBSET_LINE);
}

#[test]
fn identity_subset_without_any_clue_warns() {
    let bytes = identity_subset_pdf(SUBSET_LINE, false);
    let doc = redact_pdf::load_from_bytes(&bytes).expect("ladbar");
    let (runs, warnings) = redact_pdf::PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Text");
    let text: String = runs.iter().map(|r| r.text.clone()).collect();
    assert!(
        !text.contains("Kontonummer"),
        "Testdaten taugen nicht, der Text ist lesbar: {text:?}"
    );
    assert!(
        warnings.iter().any(|w| w.contains("ToUnicode")),
        "keine Warnung trotz undekodierbarem Font: {warnings:?}"
    );
}

#[test]
fn identity_subset_with_printable_cids_does_not_pass_as_readable() {
    // Der gefährliche Fall: ein Subset, dessen CIDs zufällig im druckbaren
    // ASCII-Bereich liegen. Der Identity-Rückfall macht daraus lesbar
    // *aussehenden* Unsinn — kein Ersatzzeichen, also auch keine Warnung, und
    // die Schwärzung meldet Erfolg an einer Datei, in der alles steht.
    let bytes = identity_subset_pdf_from(SUBSET_LINE, false, 40);
    let doc = redact_pdf::load_from_bytes(&bytes).expect("ladbar");
    let (runs, warnings) = redact_pdf::PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Text");
    let text: String = runs.iter().map(|r| r.text.clone()).collect();
    assert!(
        !text.contains("Kontonummer"),
        "Testdaten taugen nicht: {text:?}"
    );
    assert!(
        warnings.iter().any(|w| w.contains("ToUnicode")),
        "kein Warnhinweis, obwohl der Text erfunden ist ({text:?}): {warnings:?}"
    );
}

#[test]
fn cid_to_gid_map_stream_is_followed() {
    // CIDs sind hier gegenüber den Glyph-IDs um 100 verschoben — nur wer
    // `/CIDToGIDMap` liest, kommt auf den richtigen Text.
    let (pairs, gids) = subset_cids("Konto 4711");
    let mut doc = Document::new();
    let max_cid = 100 + gids.len();
    let mut table = vec![0u8; (max_cid + 1) * 2];
    for (i, gid) in pairs.iter().map(|(_, g)| *g).enumerate() {
        let cid = 101 + i;
        table[cid * 2..cid * 2 + 2].copy_from_slice(&gid.to_be_bytes());
    }
    let map_id = doc.add_object(Stream::new(dictionary! {}, table).with_compression(false));
    let file_id =
        doc.add_object(Stream::new(dictionary! {}, sfnt_with_cmap(&pairs)).with_compression(false));
    let descriptor_id = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "ABCDEF+SubsetSans",
        "FontFile2" => Object::Reference(file_id),
    });
    let descendant = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "ABCDEF+SubsetSans",
        "CIDToGIDMap" => Object::Reference(map_id),
        "FontDescriptor" => Object::Reference(descriptor_id),
    });
    let font = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "ABCDEF+SubsetSans",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(descendant)],
    };
    let info = font_from_dict(&doc, &font);
    let decoded: String = (0..pairs.len())
        .map(|i| info.charmap.text_for(101 + i as u32))
        .collect();
    let expected: String = pairs.iter().map(|(c, _)| *c).collect();
    assert_eq!(decoded, expected);
    // Ohne die Verschiebung wäre bei CID 1 nichts zu holen.
    assert_eq!(info.charmap.text_for(1), "\u{FFFD}");
}

#[test]
fn ucs2_cmap_names_make_the_code_the_codepoint() {
    let mut doc = Document::new();
    let descendant = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType0",
        "BaseFont" => "MSGothic",
    });
    let font = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "MSGothic",
        "Encoding" => "UniJIS-UCS2-H",
        "DescendantFonts" => vec![Object::Reference(descendant)],
    };
    let info = font_from_dict(&doc, &font);
    assert_eq!(info.charmap.text_for(0x004B), "K");
    assert_eq!(info.charmap.text_for(0x20AC), "€");
    // Steuerzeichen bleiben unlesbar statt still durchzurutschen.
    assert_eq!(info.charmap.text_for(0x0001), "\u{FFFD}");
}
