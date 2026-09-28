//! Das ehrliche Orakel und die gewöhnliche Datei: eingebettete
//! Teilmengen-Schriften.
//!
//! # Der Befund
//!
//! `redact_pdf::leaks` verglich Bytes — UTF-8, Latin-1, UTF-16BE, Hex und die
//! Verkettung der Zeichenkettenliterale. Bei einer eingebetteten
//! Teilmengen-Schrift stehen im Strom aber Glyphnummern oder umgelenkte Codes,
//! keine Zeichen: LibreOffice Writer 24.2 schreibt `<01020304…>Tj` mit einer
//! TrueType-Teilmenge `BAAAAA+LiberationSerif`, PyMuPDF und reportlab
//! schreiben Type0/Identity-H mit zweibytigen CIDs. Beide liefern ein
//! `/ToUnicode`. An der **ungeschwärzten** Datei meldete `--check-leaks`
//! „keiner der 4 Suchbegriffe steht noch in der Datei“, Rückgabewert 0 —
//! an der Datei aus Word, LibreOffice, Chrome, also dem Regelfall.
//!
//! # Die Korrektur
//!
//! Sichtweise 7 in `audit_bytes`: jede Seite so lesen, wie der eigene
//! Schriftdekoder sie liest, **neben** den Bytesichten, nicht an ihrer
//! Stelle. Die Tests hier bauen dieselben Strukturen nach, die die beiden
//! Erzeuger schreiben — und prüfen beide Richtungen: der Dekoder findet, was
//! die Bytesichten nicht sehen, und er ersetzt sie nicht.

use lopdf::{dictionary, Document, Object, ObjectId, Stream, StringFormat};
use redact_pdf::leaks;

const IBAN: &str = "DE89 3704 0044 0532 0130 00";
const NAME: &str = "Max Mustermann";

/// Die Zeilen, die auf der Seite stehen — wie in den Exporten der
/// Gegenprüfer.
const LINES: [&str; 2] = [
    "Kontoinhaber Max Mustermann",
    "IBAN: DE89 3704 0044 0532 0130 00",
];

/// Ein `/ToUnicode`-CMap, der jedem Code sein Zeichen zuordnet — als
/// `bfchar`-Liste, wie LibreOffice und MuPDF sie schreiben. `code_bytes` ist
/// 1 (TrueType-Teilmenge, Codes `01`, `02`, …) oder 2 (Identity-H, CIDs).
fn to_unicode(map: &[(u32, char)], code_bytes: usize, target: impl Fn(char) -> String) -> Vec<u8> {
    let width = code_bytes * 2;
    let (lo, hi) = if code_bytes == 1 {
        ("<00>", "<FF>")
    } else {
        ("<0000>", "<FFFF>")
    };
    let mut cmap = format!(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n{lo} {hi}\nendcodespacerange\n\
         {} beginbfchar\n",
        map.len()
    );
    for (code, ch) in map {
        cmap.push_str(&format!("<{code:0width$X}> <{}>\n", target(*ch)));
    }
    cmap.push_str("endbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    cmap.into_bytes()
}

fn utf16_hex(ch: char) -> String {
    ch.encode_utf16(&mut [0u16; 2])
        .iter()
        .map(|u| format!("{u:04X}"))
        .collect()
}

/// Ordnet jedem Zeichen der Seite einen Code zu, in der Reihenfolge des
/// ersten Auftretens — genau so vergibt LibreOffice seine Codes `01`, `02`, …
fn subset_codes(first_code: u32) -> Vec<(u32, char)> {
    let mut map: Vec<(u32, char)> = Vec::new();
    for ch in LINES.iter().flat_map(|l| l.chars()) {
        if !map.iter().any(|(_, c)| *c == ch) {
            map.push((first_code + map.len() as u32, ch));
        }
    }
    map
}

fn code_of(map: &[(u32, char)], ch: char) -> u32 {
    map.iter()
        .find(|(_, c)| *c == ch)
        .map(|(code, _)| *code)
        .unwrap()
}

/// Seiteninhalt: je Zeile ein `Tj` mit Hex-String aus Glyphencodes, wie
/// `<01020304…>Tj` bei LibreOffice bzw. `[<002e0052…>]TJ` bei MuPDF.
fn content(map: &[(u32, char)], code_bytes: usize) -> Vec<u8> {
    let width = code_bytes * 2;
    let mut out = String::from("BT\n/F1 12 Tf\n");
    for (i, line) in LINES.iter().enumerate() {
        let hex: String = line
            .chars()
            .map(|c| format!("{:0width$X}", code_of(map, c)))
            .collect();
        out.push_str(&format!("1 0 0 1 50 {} Tm <{hex}> Tj\n", 770 - 20 * i));
    }
    out.push_str("ET\n");
    out.into_bytes()
}

/// Ein Dokument mit einer Seite; `font` legt die Schrift im Dokument an und
/// gibt ihre Objekt-Id zurück. Der Inhalt ist Flate-komprimiert, wie bei
/// jedem Export.
fn document(font: impl FnOnce(&mut Document) -> ObjectId, content: Vec<u8>) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let font_id = font(&mut doc);
    let mut stream = Stream::new(dictionary! {}, content);
    stream.compress().expect("komprimierbar");
    let content_id = doc.add_object(stream);
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("speicherbar");
    bytes
}

/// LibreOffice-Bauart: symbolische TrueType-Teilmenge, Codes ab `01`,
/// `/FirstChar 0`, `/ToUnicode` mit `bfchar`.
fn libreoffice_export(lie: bool) -> Vec<u8> {
    let map = subset_codes(1);
    let cmap = to_unicode(&map, 1, |c| utf16_hex(if lie { 'x' } else { c }));
    let widths: Vec<Object> = (0..=map.len()).map(|_| 500.into()).collect();
    document(
        |doc| {
            let cmap_id = doc.add_object(Stream::new(dictionary! {}, cmap));
            doc.add_object(dictionary! {
                "Type" => "Font",
                "Subtype" => "TrueType",
                "BaseFont" => "BAAAAA+LiberationSerif",
                "FirstChar" => 0,
                "LastChar" => map.len() as i64,
                "Widths" => widths,
                "FontDescriptor" => dictionary! {
                    "Type" => "FontDescriptor",
                    "FontName" => "BAAAAA+LiberationSerif",
                    "Flags" => 4,
                    "FontBBox" => vec![(-543).into(), (-303).into(), 1278.into(), 982.into()],
                    "ItalicAngle" => 0,
                    "Ascent" => 891,
                    "Descent" => (-216),
                    "CapHeight" => 981,
                    "StemV" => 80,
                },
                "ToUnicode" => cmap_id,
            })
        },
        content(&map, 1),
    )
}

/// MuPDF/reportlab-Bauart: Type0 mit Identity-H, CIDFontType2, zweibytige
/// CIDs, `/ToUnicode` mit `bfchar`.
fn identity_h_export() -> Vec<u8> {
    let map = subset_codes(0x2e);
    let cmap = to_unicode(&map, 2, utf16_hex);
    document(
        |doc| {
            let cmap_id = doc.add_object(Stream::new(dictionary! {}, cmap));
            doc.add_object(dictionary! {
                "Type" => "Font",
                "Subtype" => "Type0",
                "BaseFont" => "DACUOE+DejaVuSans",
                "Encoding" => "Identity-H",
                "DescendantFonts" => vec![Object::Dictionary(dictionary! {
                    "Type" => "Font",
                    "Subtype" => "CIDFontType2",
                    "BaseFont" => "DACUOE+DejaVuSans",
                    "CIDSystemInfo" => dictionary! {
                        "Registry" => Object::String(b"Adobe".to_vec(), StringFormat::Literal),
                        "Ordering" => Object::String(b"Identity".to_vec(), StringFormat::Literal),
                        "Supplement" => 0,
                    },
                    "DW" => 600,
                })],
                "ToUnicode" => cmap_id,
            })
        },
        content(&map, 2),
    )
}

/// Die Bytesichten dürfen das Geheimnis in diesen Dateien nicht sehen —
/// sonst prüfte der Test nicht die neue Sicht, sondern irgendeine.
fn only_the_decoder_finds(bytes: &[u8], needle: &str) -> Vec<String> {
    let hits = leaks(bytes, needle);
    assert!(
        !hits.is_empty(),
        "Orakel blind: „{needle}“ nicht gefunden, obwohl es auf der Seite steht"
    );
    let strangers: Vec<&String> = hits
        .iter()
        .filter(|h| !h.contains("Schriftdekoder"))
        .collect();
    assert!(
        strangers.is_empty(),
        "eine Bytesicht sieht Glyphencodes als Text — dann prüft dieser Test die falsche \
         Sicht:\n{strangers:?}"
    );
    hits
}

/// Der LibreOffice-Fall: `<01020304…>Tj`, TrueType-Teilmenge, `/ToUnicode`.
#[test]
fn libreoffice_teilmenge_wird_vom_schriftdekoder_gelesen() {
    let pdf = libreoffice_export(false);
    // Die kompakte Schreibweise „DE89370400440532013000“ steht bewusst nicht
    // hier: ein Begriff ohne Leerraum wird nur wörtlich gesucht — auch an
    // einer Helvetica-Datei, seit jeher (siehe `Needle::squeezed`).
    for needle in [IBAN, NAME] {
        let hits = only_the_decoder_finds(&pdf, needle);
        assert!(
            hits.iter().any(|h| h.contains("Seite 1")),
            "die Fundstelle nennt die Seite nicht: {hits:?}"
        );
    }
}

/// Der MuPDF-/reportlab-Fall: Identity-H, zweibytige CIDs.
#[test]
fn identity_h_teilmenge_wird_vom_schriftdekoder_gelesen() {
    let pdf = identity_h_export();
    for needle in [IBAN, NAME] {
        only_the_decoder_finds(&pdf, needle);
    }
}

/// Gegenprobe: was nicht auf der Seite steht, meldet auch die neue Sicht
/// nicht — auch nicht über die Fassung ohne Leerraum.
#[test]
fn schriftdekoder_meldet_keinen_fremden_begriff() {
    let pdf = libreoffice_export(false);
    for needle in ["DE99 3704 0044", "Erika Musterfrau", "Kontonummer"] {
        let hits = leaks(&pdf, needle);
        assert!(hits.is_empty(), "Fehlalarm für „{needle}“: {hits:?}");
    }
}

/// Sicht 7 kommt dazu, sie ersetzt nichts: an einer Datei mit
/// Standardschrift finden die Bytesichten **und** der Dekoder.
///
/// Das ist die Zusicherung gegen den Zirkelschluss. Ersetzte jemand die
/// Bytesichten durch den Dekoder, bliebe dieser Test rot.
#[test]
fn sicht_7_ersetzt_die_bytesichten_nicht() {
    let pdf = redact_pdf::testing::demo_statement();
    let hits = leaks(&pdf, IBAN);
    let (decoder, bytes): (Vec<&String>, Vec<&String>) =
        hits.iter().partition(|h| h.contains("Schriftdekoder"));
    assert!(
        !decoder.is_empty(),
        "der Dekoder sieht die Vorlage nicht: {hits:?}"
    );
    assert!(
        bytes.iter().any(|h| h.contains("Objekt")),
        "die Bytesichten sehen die Vorlage nicht mehr: {hits:?}"
    );
}

/// **Kanarienvogel** — ein benannter blinder Fleck, kein Ziel.
///
/// Ein `/ToUnicode`, das jeden Code auf „x“ abbildet, ist eine Behauptung
/// der Datei; der Dekoder glaubt ihr, und die Bytesichten sehen nur
/// Glyphnummern. Der Text auf dem Papier bleibt unsichtbar. Dieser Test
/// hält das fest: wird er rot, sieht das Orakel mehr als heute — dann gehört
/// der Modulkommentar von `audit_bytes` angepasst, nicht der Test.
#[test]
fn kanarienvogel_luegendes_tounicode_bleibt_blind() {
    let pdf = libreoffice_export(true);
    let hits = leaks(&pdf, IBAN);
    assert!(
        hits.is_empty(),
        "das Orakel sieht durch ein lügendes /ToUnicode — Modulkommentar anpassen: {hits:?}"
    );
}

/// Eine Seite, die der Interpreter ablehnt, nimmt die anderen nicht mit.
///
/// `extract` bricht beim ersten Fehler für das ganze Dokument ab; die Sicht
/// liest deshalb **nachsichtig** (`extract_lenient`): die abgelehnte Seite
/// wird übersprungen und als Warnung genannt, die übrigen kommen zurück.
/// Seite 1 ist hier kaputt (ein nie geschlossenes Zeichenkettenliteral, der
/// Interpreter lehnt den Strom ab), Seite 2 trägt das Geheimnis in der
/// Teilmengen-Schrift — nur der Dekoder kann es sehen, und er muss es trotz
/// Seite 1 sehen.
#[test]
fn eine_kaputte_seite_nimmt_die_anderen_nicht_mit() {
    let good = libreoffice_export(false);
    let mut doc = Document::load_mem(&good).expect("parsebar");
    let first_page = *doc.get_pages().values().next().expect("eine Seite");
    let pages_id = doc
        .get_dictionary(first_page)
        .and_then(|page| page.get(b"Parent"))
        .and_then(Object::as_reference)
        .expect("Seitenbaum");
    let broken_content = doc.add_object(Stream::new(dictionary! {}, b"BT (offen".to_vec()));
    let broken_page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => broken_content,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    let pages = doc.get_dictionary_mut(pages_id).expect("Pages");
    let mut kids = pages
        .get(b"Kids")
        .and_then(|k| k.as_array())
        .cloned()
        .expect("Kids");
    kids.insert(0, broken_page.into());
    pages.set("Kids", kids);
    pages.set("Count", 2);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("speicherbar");

    // Die Vorbedingung des Tests: der Extraktor lehnt das Dokument als
    // Ganzes ab. Sonst prüfte er nicht den Rückfall.
    let reloaded = Document::load_mem(&bytes).expect("parsebar");
    let extractor = redact_pdf::PdfExtractor::new();
    assert!(
        extractor.extract(&reloaded).is_err(),
        "Seite 1 sollte den Interpreter zum Abbruch bringen"
    );

    // Die nachsichtige Extraktion: Seite 2 kommt zurück, Seite 1 wird
    // genannt — und nur Seite 1.
    let (runs, warnings) = extractor.extract_lenient(&reloaded);
    assert!(
        runs.iter().any(|r| r.page == 1 && r.text.contains(IBAN)),
        "Seite 2 fehlt in der nachsichtigen Extraktion: {runs:?}"
    );
    assert!(
        runs.iter().all(|r| r.page != 0),
        "von der kaputten Seite darf nichts kommen: {runs:?}"
    );
    let skipped: Vec<&String> = warnings
        .iter()
        .filter(|w| w.contains("fehlt in dieser Sicht"))
        .collect();
    assert_eq!(
        skipped.len(),
        1,
        "genau eine übersprungene Seite: {warnings:?}"
    );
    assert!(
        skipped[0].starts_with("Seite 1 "),
        "die Warnung nennt die übersprungene Seite: {}",
        skipped[0]
    );

    let hits = leaks(&bytes, IBAN);
    assert!(
        hits.iter()
            .any(|h| h.contains("Seite 2") && h.contains("Schriftdekoder")),
        "Seite 2 ging mit Seite 1 verloren: {hits:?}"
    );
}
