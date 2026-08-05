//! Eine `/ToUnicode`-CMap als Verstärker — und die Grenze, die ihn deckelt.
//!
//! [`redact_pdf::encoding::parse_to_unicode`] baute aus einer CMap eine
//! `BTreeMap<u32, String>` ohne jede Grenze. `apply_bfrange` deckelt zwar
//! *einen* Bereich auf 65 536 Codes, aber weder die **Anzahl** der
//! `bfrange`-Anweisungen noch die **Länge** ihrer Zielstrings war beschränkt.
//! Beides vervielfacht sich, und beides steht in einer Datei, die jede
//! dokumentierte Grenze einhält: das 16-MB-Parse-Budget sieht nur die
//! entpackte CMap, und die ist winzig, weil sich wiederholte Zeilen etwa
//! 1000:1 flate-komprimieren.
//!
//! Gemessen am Release-Binary (Laden + Scan + Extraktion), Spitzenspeicher
//! über `/usr/bin/time -v`, virtueller Adressraum mit `ulimit -v` gedeckelt:
//!
//! | Datei | Größe | vorher | nachher |
//! |---|---|---|---|
//! | 20 `bfrange`-Anweisungen | 1 202 B | 342 MB, 1,5 s | 14 MB, 0,08 s |
//! | 100 Anweisungen | 1 558 B | 1 696 MB, 8,8 s | 14 MB, 0,09 s |
//! | 200 Anweisungen | 2 018 B | 3 388 MB, 20,4 s | 14 MB, 0,10 s |
//! | 400 Anweisungen | 3 032 B | 6 775 MB, 33,1 s | 15 MB, 0,09 s |
//! | 1 000 Anweisungen | 6 050 B | **SIGABRT** (Rückgabewert 134) | 15 MB, 0,09 s |
//! | 1 `bfrange`, Ziel 2 048 Zeichen | 1 131 B | 530 MB, 2,2 s | 61 MB, 0,36 s |
//! | 1 `bfrange`, Ziel 32 768 Zeichen | 1 253 B | **SIGABRT** | 39 MB, 0,36 s |
//!
//! Gegenprobe, unverändert angenommen: eine ehrliche ToUnicode über den
//! vollen Zweibyte-Coderaum (65 295 Einträge, Datei 2 565 B) — vorher wie
//! nachher 20 MB, und der Text wird gelesen. Genau das prüft
//! [`eine_volle_zweibyte_zuordnung_wird_weiter_benutzt`] hier nach.
//!
//! Warum die Tests keine Bytes messen: Spitzenspeicher lässt sich in einem
//! Testlauf nicht verlässlich beobachten — der Testläufer teilt sich den
//! Prozess mit allen anderen Tests. Gemessen wird deshalb das, was sich messen
//! lässt: wie viele Einträge und wie viele Textbytes eine CMap überhaupt
//! erzeugen darf, und was der Interpreter danach über den Font sagt.

use lopdf::{dictionary, Document, Object, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::encoding::{parse_to_unicode, CodeWidth, MAX_TO_UNICODE_BYTES};
use redact_pdf::{scan_page, PdfExtractor, PdfRedactor};

/// Das Geheimnis, das die Gegenprobe finden muss.
const SECRET: &str = "532013000";

/// Die Textmarke, an der `redact_pipeline::coverage` eine Deckungslücke
/// erkennt (Rückgabewert 3).
const OHNE_TO_UNICODE: &str = "hat kein /ToUnicode";

// ---------------------------------------------------------------------------
// CMap-Bau
// ---------------------------------------------------------------------------

/// `n` `bfrange`-Anweisungen über je 65 536 **verschiedene** Codes.
///
/// Die Deckelung in `apply_bfrange` greift je Bereich; verschiedene Bereiche
/// addieren sich. Jede Anweisung ist rund 50 Byte lang und erzeugt bis zu
/// 65 536 Einträge — die Verstärkung, um die es geht.
fn bfrange_bomb(n: u32) -> Vec<u8> {
    let mut out = String::from("1 begincodespacerange <0000> <FFFF> endcodespacerange\n");
    for k in 0..n {
        let lo = k * 0x1_0000;
        out.push_str(&format!(
            "1 beginbfrange <{lo:08X}> <{:08X}> <0041> endbfrange\n",
            lo + 0xFFFF
        ));
    }
    out.push_str("endcmap\n");
    out.into_bytes()
}

/// **Eine** `bfrange` über 65 536 Codes mit einem `chars` Zeichen langen Ziel.
///
/// Das Ziel wird für jeden Code kopiert und dabei hochgezählt. Eine Grenze,
/// die nur Einträge zählte, sähe hier 65 536 — und ließe damit beliebig viele
/// Megabyte durch.
fn long_target_bomb(chars: usize) -> Vec<u8> {
    let mut out = String::from("1 begincodespacerange <0000> <FFFF> endcodespacerange\n");
    out.push_str("1 beginbfrange <0000> <FFFF> <");
    for _ in 0..chars {
        out.push_str("0041");
    }
    out.push_str("> endbfrange\nendcmap\n");
    out.into_bytes()
}

/// Codes für `Kto. ` und die Ziffern — die Belegung, die [`full_two_byte_cmap`]
/// vergibt.
fn code_of(ch: char) -> u32 {
    match ch {
        '0'..='9' => 1 + (ch as u32 - '0' as u32),
        'K' => 0x11,
        't' => 0x12,
        'o' => 0x13,
        '.' => 0x14,
        ' ' => 0x15,
        other => panic!("kein Code für {other:?}"),
    }
}

/// Eine **ehrliche** ToUnicode über den vollen Zweibyte-Coderaum.
///
/// Mehr geht nicht: Identity-H benutzt Zweibyte-Codes, und sfnt wie CFF können
/// ohnehin nicht mehr als 65 535 Glyphen führen. Ein CJK-Font mit vollem
/// Umfang sieht so aus — er ist keine Bombe und muss weiterhin durchkommen.
fn full_two_byte_cmap() -> Vec<u8> {
    let mut out = String::from("1 begincodespacerange <0000> <FFFF> endcodespacerange\n");
    let named: Vec<char> = "0123456789Kto. ".chars().collect();
    out.push_str(&format!("{} beginbfchar\n", named.len()));
    for ch in &named {
        out.push_str(&format!("<{:04X}> <{:04X}>\n", code_of(*ch), *ch as u32));
    }
    out.push_str("endbfchar\n");

    // Der Rest des Coderaums, in Blöcken zu 256 Codes auf den CJK-Bereich.
    let mut ranges = String::new();
    let mut count = 0;
    let mut code = 0x0100u32;
    let mut target = 0x4E00u32;
    while code + 0xFF <= 0xFFFF {
        ranges.push_str(&format!(
            "<{code:04X}> <{:04X}> <{target:04X}>\n",
            code + 0xFF
        ));
        count += 1;
        code += 0x100;
        target += 0x100;
        if target > 0x9F00 {
            target = 0x4E00;
        }
    }
    out.push_str(&format!(
        "{count} beginbfrange\n{ranges}endbfrange\nendcmap\n"
    ));
    out.into_bytes()
}

// ---------------------------------------------------------------------------
// Dateibau
// ---------------------------------------------------------------------------

/// Der Zeichenkette `text` als Zweibyte-Codes, so wie sie im Strom steht.
fn hex_codes(text: &str) -> String {
    text.chars()
        .map(|c| format!("{:04X}", code_of(c)))
        .collect()
}

/// Eine Seite mit **einem** Type0-Font, dessen `/ToUnicode` `cmap` ist.
fn type0_page(cmap: &[u8], shown: &str) -> (Document, lopdf::ObjectId) {
    let mut doc = Document::with_version("1.5");
    let tounicode_id = doc.add_object(Object::Stream(
        Stream::new(dictionary! {}, cmap.to_vec()).with_compression(false),
    ));
    let descendant_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "ABCDEF+Bombe",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0_i64,
        },
        "DW" => 600_i64,
    });
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "ABCDEF+Bombe",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(descendant_id)],
        "ToUnicode" => Object::Reference(tounicode_id),
    });
    let content = format!("BT /F0 12 Tf 72 700 Td <{}> Tj ET\n", hex_codes(shown));
    finish(doc, font_id, content.into_bytes())
}

/// Eine Seite mit einem **einfachen** WinAnsi-Font, dem dieselbe Bombe als
/// `/ToUnicode` angehängt ist.
fn simple_page(cmap: &[u8], shown: &str) -> (Document, lopdf::ObjectId) {
    let mut doc = Document::with_version("1.5");
    let tounicode_id = doc.add_object(Object::Stream(
        Stream::new(dictionary! {}, cmap.to_vec()).with_compression(false),
    ));
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
        "ToUnicode" => Object::Reference(tounicode_id),
    });
    let content = format!("BT /F0 12 Tf 72 700 Td ({shown}) Tj ET\n");
    finish(doc, font_id, content.into_bytes())
}

fn finish(
    mut doc: Document,
    font_id: lopdf::ObjectId,
    content: Vec<u8>,
) -> (Document, lopdf::ObjectId) {
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F0" => font_id },
    });
    let content_id = doc.add_object(Stream::new(dictionary! {}, content).with_compression(false));
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
    (doc, page_id)
}

fn warnings_of(doc: &Document, page_id: lopdf::ObjectId) -> Vec<String> {
    scan_page(doc, page_id).expect("Scan").warnings
}

fn lines_of(doc: &Document) -> Vec<String> {
    PdfExtractor::new()
        .extract(doc)
        .expect("Extraktion")
        .into_iter()
        .map(|run| run.text)
        .collect()
}

// ---------------------------------------------------------------------------
// Die Grenze greift — beide Bauarten
// ---------------------------------------------------------------------------

/// Viele `bfrange`-Anweisungen: die Zuordnung bricht ab, statt zu wachsen.
///
/// Acht Anweisungen könnten 524 288 Einträge tragen. Ohne die Grenze tut die
/// CMap das auch — dieser Test wird dann an beiden Zusicherungen rot.
#[test]
fn viele_bfrange_anweisungen_sprengen_die_zuordnung_nicht() {
    let r = parse_to_unicode(&bfrange_bomb(8));
    assert!(
        r.over_limit,
        "acht Anweisungen zu je 65 536 Codes müssen die Decke reißen"
    );
    assert!(
        r.map.len() <= 200_000,
        "die Zuordnung wuchs auf {} Einträge",
        r.map.len()
    );
    // Die Code-Breite steht vor der Bombe und bleibt brauchbar.
    assert_eq!(r.code_width, Some(CodeWidth::Two));
}

/// Ein langes Ziel: dieselbe Decke, nur zählt hier nicht die Anzahl.
///
/// Eine einzige `bfrange` über 65 536 Codes mit einem 1 024 Zeichen langen
/// Ziel ergibt 67 MB Text — bei nur 65 536 Einträgen. Eine Grenze, die
/// Einträge zählte, ließe das durch; deshalb wird der Platzbedarf gezählt.
#[test]
fn ein_langes_bfrange_ziel_sprengt_die_zuordnung_nicht() {
    let r = parse_to_unicode(&long_target_bomb(1024));
    assert!(r.over_limit, "das lange Ziel muss die Decke reißen");
    let text_bytes: usize = r.map.values().map(String::len).sum();
    assert!(
        text_bytes <= MAX_TO_UNICODE_BYTES,
        "die Zielstrings belegen {text_bytes} Byte"
    );
}

/// Ein `bfrange` am oberen Ende des Coderaums darf nicht paniken.
///
/// Beim Aufsammeln der Decke gefunden: `lo + offset` lief für ein Array hinter
/// `<FFFFFFFF>` über und paniket in jedem Debug-Build — also auch im Testlauf.
/// Bestanden, wenn der Aufruf zurückkehrt.
#[test]
fn ein_bfrange_am_ende_des_coderaums_laeuft_nicht_ueber() {
    parse_to_unicode(b"1 beginbfrange <FFFFFFFF> <FFFFFFFF> [<0041> <0042> <0043>] endbfrange");
    parse_to_unicode(b"1 beginbfrange <FFFFFFFF> <FFFFFFFF> <0041> endbfrange");
}

/// Auch `bfchar` zählt mit — sonst wäre die Decke nur halb da.
///
/// `bfchar` verstärkt kaum: 20 Byte Quelle je Eintrag gegen rund 260 Byte
/// Speicher. Das reicht trotzdem, um aus 4 MB CMap über 200 000 Einträge zu
/// machen, und die Quelle bleibt unter dem 16-MB-Parse-Budget.
#[test]
fn auch_bfchar_zaehlt_gegen_die_decke() {
    const ENTRIES: u32 = 200_000;
    let mut cmap = String::from("1 begincodespacerange <0000> <FFFF> endcodespacerange\n");
    cmap.push_str(&format!("{ENTRIES} beginbfchar\n"));
    for code in 1..=ENTRIES {
        cmap.push_str(&format!("<{code:08X}> <0041>\n"));
    }
    cmap.push_str("endbfchar\nendcmap\n");
    let r = parse_to_unicode(cmap.as_bytes());
    assert!(r.over_limit, "{ENTRIES} bfchar-Einträge reißen die Decke");
    assert!(
        r.map.len() < ENTRIES as usize,
        "es wurden alle {} Einträge aufgenommen",
        r.map.len()
    );
}

// ---------------------------------------------------------------------------
// Was danach geschieht: „Font ohne /ToUnicode“, nicht „Font mit halber Tabelle“
// ---------------------------------------------------------------------------

/// Der Type0-Font mit der Bombe gilt als Font **ohne** `/ToUnicode`.
///
/// Damit greift der vorhandene Weg: `CharMap::text_for` liefert Ersatzzeichen,
/// der Interpreter meldet die Deckungslücke, und die Kette macht daraus
/// Rückgabewert 3. Das ist der Gegenentwurf zum stillen Weg — ein Bruchstück
/// der Tabelle hieße `has_to_unicode() == true`, und die Warnstatistik ließe
/// den Font dann ganz aus.
#[test]
fn ein_type0_font_mit_bombe_meldet_die_deckungsluecke() {
    let (doc, page_id) = type0_page(&bfrange_bomb(8), "Kto. 532013000");
    let warnungen = warnings_of(&doc, page_id);
    assert!(
        warnungen.iter().any(|w| w.contains(OHNE_TO_UNICODE)),
        "die Deckungslücke blieb unerwähnt: {warnungen:?}"
    );
    let lines = lines_of(&doc);
    assert!(
        !lines.iter().any(|l| l.contains(SECRET)),
        "aus einer abgelehnten Zuordnung darf kein Text entstehen: {lines:?}"
    );
}

/// Bei einem einfachen Font gilt dieselbe Ablehnung — dort deckt aber die
/// Basistabelle die höchstens 256 Codes vollständig ab.
///
/// Deshalb ist hier **keine** Warnung fällig: es geht nichts verloren, was
/// eine Deckungslücke wäre. WinAnsi ist für Einbyte-Codes die vollständige
/// und richtige Auskunft; die Erkennung arbeitet unverändert weiter.
#[test]
fn ein_einfacher_font_mit_bombe_liest_weiter_ueber_winansi() {
    let (doc, page_id) = simple_page(&bfrange_bomb(8), "Kto. 532013000");
    let warnungen = warnings_of(&doc, page_id);
    assert!(
        !warnungen.iter().any(|w| w.contains(OHNE_TO_UNICODE)),
        "WinAnsi deckt Einbyte-Codes ab, das ist keine Deckungslücke: {warnungen:?}"
    );
    let lines = lines_of(&doc);
    assert!(
        lines.iter().any(|l| l.contains(SECRET)),
        "die Basistabelle muss den Text weiterhin liefern: {lines:?}"
    );
}

// ---------------------------------------------------------------------------
// Gegenprobe: eine ehrliche große ToUnicode muss weiterhin arbeiten
// ---------------------------------------------------------------------------

/// Eine Zuordnung über den **vollen** Zweibyte-Coderaum kommt unverändert
/// durch — parsen, erkennen, schwärzen.
///
/// Ohne diesen Nachweis wäre die Grenze wertlos: sie tauschte einen Fehler
/// gegen einen schlimmeren, wenn sie echte Dokumente aussperrte. 65 295
/// Einträge sind das Maximum, das ein Font überhaupt erreichen kann.
#[test]
fn eine_volle_zweibyte_zuordnung_wird_weiter_benutzt() {
    let cmap = full_two_byte_cmap();
    let r = parse_to_unicode(&cmap);
    assert!(
        !r.over_limit,
        "die größte ehrliche ToUnicode wurde abgelehnt ({} Einträge)",
        r.map.len()
    );
    assert!(
        r.map.len() >= 65_000,
        "nur {} Einträge — der Coderaum ist nicht ausgeschöpft",
        r.map.len()
    );

    let (mut doc, page_id) = type0_page(&cmap, "Kto. 532013000");
    let warnungen = warnings_of(&doc, page_id);
    assert!(
        !warnungen.iter().any(|w| w.contains(OHNE_TO_UNICODE)),
        "der Font sagt sehr wohl, was seine Codes bedeuten: {warnungen:?}"
    );

    // Erkennung: die Zeile muss lesbar herauskommen.
    let runs = PdfExtractor::new().extract(&doc).expect("Extraktion");
    let treffer = runs
        .iter()
        .find(|run| run.text.contains(SECRET))
        .unwrap_or_else(|| {
            panic!(
                "das Geheimnis wurde nicht gelesen: {:?}",
                runs.iter().map(|r| &r.text).collect::<Vec<_>>()
            )
        });

    // Und die Schwärzung darüber muss den Text auch wirklich entfernen.
    let redaction = Redaction::new(
        Region::new(
            0,
            Rect::new(
                treffer.rect.ll.x - 2.0,
                treffer.rect.ll.y - 2.0,
                treffer.rect.ur.x + 2.0,
                treffer.rect.ur.y + 2.0,
            ),
            None,
            Source::Manual {
                reason: "Gegenprobe".into(),
            },
        ),
        Action::Blackout,
    );
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[redaction])
        .expect("Schwärzung");
    assert!(
        report.per_redaction[0] > 0,
        "die Schwärzung hat nichts angefasst"
    );
    let danach = lines_of(&doc);
    assert!(
        !danach.iter().any(|l| l.contains(SECRET)),
        "nach der Schwärzung stand das Geheimnis noch da: {danach:?}"
    );
}

/// Dieselbe Decke schützt auch die `/Encoding`-CMap eines Type0-Fonts.
///
/// `load_type0` liest sie mit demselben Parser — nur, um die Code-Breite zu
/// erfahren. Ohne Grenze baute es dafür die ganze Bombe auf und warf sie
/// gleich wieder weg.
#[test]
fn auch_die_encoding_cmap_laeuft_gegen_dieselbe_decke() {
    let mut doc = Document::with_version("1.5");
    let encoding_id = doc.add_object(Object::Stream(
        Stream::new(dictionary! {}, bfrange_bomb(8)).with_compression(false),
    ));
    let descendant_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "ABCDEF+Bombe",
        "DW" => 600_i64,
    });
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "ABCDEF+Bombe",
        "Encoding" => Object::Reference(encoding_id),
        "DescendantFonts" => vec![Object::Reference(descendant_id)],
    });
    let (doc, page_id) = finish(
        doc,
        font_id,
        b"BT /F0 12 Tf 72 700 Td <0001> Tj ET\n".to_vec(),
    );
    // Bestanden, wenn der Scan zurückkehrt: die CMap darf den Speicher nicht
    // sprengen, und ihre Codebreite bleibt die übliche.
    let warnungen = warnings_of(&doc, page_id);
    assert!(
        warnungen.iter().any(|w| w.contains(OHNE_TO_UNICODE)),
        "ohne /ToUnicode ist der Font unlesbar und muss das sagen: {warnungen:?}"
    );
}
