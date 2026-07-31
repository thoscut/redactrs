//! Erzeugt ein möglichst breites Korpus von PDF-Dateien im Speicher.
//!
//! Hintergrund: die GUI-Vorschau darf **niemals** eine leere Seite zeigen.
//! `redact_pdf::testing::build_pdf` deckt nur Helvetica-Text ab — hier
//! entstehen deshalb per `lopdf` (und für ein paar Fälle per Hand geschriebene
//! Rohbytes) Dateien aus allen Ecken der Spezifikation: CID-Fonts, Vektorpfade,
//! Bilder in sechs Varianten, Strukturvarianten des Seitenbaums und eine Reihe
//! bewusst kaputter Dateien.
//!
//! Jede Datei ist ein [`Sample`]. `expect_content == true` heißt: auf der Seite
//! ist wirklich etwas zu sehen, eine weiße Ausgabe wäre also ein Fehler.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

// ---------------------------------------------------------------------------
// Sample
// ---------------------------------------------------------------------------

/// Regelschwelle für [`Sample::min_non_white`]: 0,1 % der Pixel.
pub const DEFAULT_MIN_NON_WHITE: f64 = 0.001;

/// Eine erzeugte PDF-Datei samt Erwartungshaltung.
pub struct Sample {
    /// Eindeutiger, sprechender Name (dient auch als Fehlermeldung).
    pub name: &'static str,
    /// Grobe Einordnung für die Übersichtstabelle.
    pub category: Category,
    /// Die fertige Datei.
    pub bytes: Vec<u8>,
    /// Ist auf der Seite garantiert etwas sichtbar?
    pub expect_content: bool,
    /// Erwartete Seitenzahl.
    pub pages: usize,
    /// Untergrenze für den Anteil nicht-weißer Pixel, wenn `expect_content`.
    ///
    /// Regelwert ist [`DEFAULT_MIN_NON_WHITE`] (0,1 %); alle selbst gebauten
    /// Beispiele sind bewusst so reich bedruckt, dass sie weit darüber liegen.
    /// Abweichen darf nur, wer es begründet: ein echtes Dokument, das
    /// nachweislich aus einer einzigen kurzen Textzeile besteht, kann diese
    /// Schwelle nicht erreichen, ohne dass etwas fehlt.
    pub min_non_white: f64,
}

impl Sample {
    /// Bewusst kaputte Datei? Für die wird kein sauberes Laden verlangt.
    pub fn is_hostile(&self) -> bool {
        matches!(self.category, Category::Hostile)
    }
}

/// Kategorien wie in der Aufgabenstellung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Text,
    Vector,
    Image,
    Structure,
    Hostile,
    /// Keine Konstruktion, sondern eine echte Datei von dieser Maschine.
    Real,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Text => "text",
            Category::Vector => "vector",
            Category::Image => "image",
            Category::Structure => "structure",
            Category::Hostile => "hostile",
            Category::Real => "real",
        }
    }
}

/// Das gesamte Korpus.
pub fn corpus() -> Vec<Sample> {
    let mut out = Vec::new();
    out.extend(text_samples());
    out.extend(vector_samples());
    out.extend(image_samples());
    out.extend(structure_samples());
    out.extend(hostile_samples());
    out.extend(real_samples());
    out
}

// ---------------------------------------------------------------------------
// Baukasten
// ---------------------------------------------------------------------------

/// Standardseite: A4 in Punkten.
pub const PAGE_W: f64 = 595.0;
pub const PAGE_H: f64 = 842.0;

/// Sammelt Objekte und schnürt am Ende Katalog + Seitenbaum darum.
struct Builder {
    doc: Document,
    pages_id: ObjectId,
    page_ids: Vec<ObjectId>,
}

impl Builder {
    fn new() -> Self {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        Self {
            doc,
            pages_id,
            page_ids: Vec::new(),
        }
    }

    fn add(&mut self, obj: impl Into<Object>) -> ObjectId {
        self.doc.add_object(obj)
    }

    /// Legt einen unkomprimierten Content-Stream an.
    fn content(&mut self, ops: &str) -> ObjectId {
        self.add(Stream::new(dictionary! {}, ops.as_bytes().to_vec()))
    }

    /// Hängt eine Seite an; `Type` und `Parent` werden ergänzt.
    fn page(&mut self, mut dict: Dictionary) -> ObjectId {
        dict.set("Type", Object::Name(b"Page".to_vec()));
        dict.set("Parent", Object::Reference(self.pages_id));
        let id = self.doc.add_object(dict);
        self.page_ids.push(id);
        id
    }

    /// Bequemer Sonderfall: eine Seite mit Content, Ressourcen und MediaBox.
    fn simple_page(&mut self, content: ObjectId, resources: ObjectId, media: [f64; 4]) {
        self.page(dictionary! {
            "Contents" => content,
            "Resources" => resources,
            "MediaBox" => rect_obj(media),
        });
    }

    /// Leeres Ressourcen-Dictionary (viele Beispiele brauchen keine).
    fn no_resources(&mut self) -> ObjectId {
        self.add(Object::Dictionary(dictionary! {}))
    }

    /// Ressourcen mit genau einem Standardfont unter `/F1`.
    fn helvetica(&mut self) -> ObjectId {
        let font = self.add(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        self.add(Object::Dictionary(dictionary! {
            "Font" => dictionary! { "F1" => font },
        }))
    }

    fn finish(self) -> Vec<u8> {
        self.finish_with(dictionary! {})
    }

    /// `extra` wird in den `/Pages`-Knoten gemischt (für vererbte Attribute).
    fn finish_with(mut self, extra: Dictionary) -> Vec<u8> {
        let count = self.page_ids.len() as i64;
        let kids: Vec<Object> = self
            .page_ids
            .iter()
            .map(|id| Object::Reference(*id))
            .collect();
        let mut pages = dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => count,
        };
        for (key, value) in extra.iter() {
            pages.set(key.clone(), value.clone());
        }
        self.doc
            .objects
            .insert(self.pages_id, Object::Dictionary(pages));

        let catalog = self.doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => self.pages_id,
        });
        self.doc.trailer.set("Root", catalog);

        let mut buffer = Vec::new();
        self.doc.save_to(&mut buffer).expect("PDF speicherbar");
        buffer
    }
}

fn rect_obj(r: [f64; 4]) -> Vec<Object> {
    r.iter().map(|v| Object::Real(*v as f32)).collect()
}

const A4: [f64; 4] = [0.0, 0.0, PAGE_W, PAGE_H];

/// Ein paar Zeilen Blindtext — bewusst viel, damit der Schwärzungsanteil
/// deutlich über der 0,1-%-Schwelle liegt und der Test nicht am Rauschen hängt.
const LINES: [&str; 18] = [
    "Musterbank AG - Kontoauszug Nummer 042",
    "Kontoinhaber: Maximilian Mustermann",
    "IBAN DE89 3704 0044 0532 0130 00",
    "BIC COBADEFFXXX - Filiale Berlin Mitte",
    "Buchungstag Wertstellung Verwendungszweck",
    "05.01.2026 Ueberweisung Musterfirma GmbH",
    "12.01.2026 Gehalt Arbeitgeber XY GmbH",
    "18.01.2026 Lastschrift Stadtwerke Berlin",
    "23.01.2026 Dauerauftrag Miete Hausverwaltung",
    "27.01.2026 Kartenzahlung Supermarkt Nord",
    "29.01.2026 Bargeldauszahlung Geldautomat",
    "31.01.2026 Zinsabschluss Kontokorrent",
    "Alter Saldo 12.345,67 EUR",
    "Neuer Saldo 9.876,54 EUR",
    "Summe Belastungen 4.321,00 EUR",
    "Summe Gutschriften 1.851,87 EUR",
    "Bitte pruefen Sie den Auszug sofort.",
    "Reklamationen innerhalb von sechs Wochen.",
];

/// Baut `BT … ET` mit vielen Zeilen linksbündigem Text.
fn text_block(font: &str, size: f64, leading: f64, x: f64, y: f64, lines: &[&str]) -> String {
    let mut out = format!("BT\n/{font} {size} Tf\n{leading} TL\n1 0 0 1 {x} {y} Tm\n");
    for line in lines {
        out.push_str(&format!("({}) Tj T*\n", escape_pdf_string(line)));
    }
    out.push_str("ET\n");
    out
}

/// Klammern und Backslashes müssen in Literal-Strings maskiert werden.
fn escape_pdf_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

fn text_samples() -> Vec<Sample> {
    vec![
        text_helvetica(),
        text_type0_identity_h(),
        text_encoding_differences(),
        text_without_widths(),
        text_tj_kerning(),
        text_tz_ts(),
        text_rotated_tm(),
        text_render_mode_3(),
    ]
}

fn one_page(
    name: &'static str,
    category: Category,
    expect_content: bool,
    bytes: Vec<u8>,
) -> Sample {
    Sample {
        name,
        category,
        bytes,
        expect_content,
        pages: 1,
        min_non_white: DEFAULT_MIN_NON_WHITE,
    }
}

/// Basisfall: die 14 Standardfonts, hier Helvetica/WinAnsi.
fn text_helvetica() -> Sample {
    let mut b = Builder::new();
    let res = b.helvetica();
    let ops = text_block("F1", 22.0, 34.0, 40.0, 790.0, &LINES);
    let content = b.content(&ops);
    b.simple_page(content, res, A4);
    one_page("text_helvetica_base14", Category::Text, true, b.finish())
}

/// Type0/Identity-H mit `/W`-Breitentabelle und `/ToUnicode`-CMap.
///
/// Ohne eingebettetes Fontprogramm — genau die Konstellation, in der der
/// Renderer über `/ToUnicode` auf einen Ersatzfont ausweichen muss.
fn text_type0_identity_h() -> Sample {
    let mut b = Builder::new();

    // CID 1..=26 -> 'A'..='Z', 27..=52 -> 'a'..='z', 53 -> Leerzeichen,
    // 54..=63 -> '0'..='9', 64 -> Punkt, 65 -> Komma, 66 -> Bindestrich.
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n\
         12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> def\n\
         /CMapName /Identity-H def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let mut entries: Vec<(u32, char)> = Vec::new();
    for (i, ch) in ('A'..='Z').enumerate() {
        entries.push((1 + i as u32, ch));
    }
    for (i, ch) in ('a'..='z').enumerate() {
        entries.push((27 + i as u32, ch));
    }
    entries.push((53, ' '));
    for (i, ch) in ('0'..='9').enumerate() {
        entries.push((54 + i as u32, ch));
    }
    entries.push((64, '.'));
    entries.push((65, ','));
    entries.push((66, '-'));

    cmap.push_str(&format!("{} beginbfchar\n", entries.len()));
    for (cid, ch) in &entries {
        cmap.push_str(&format!("<{cid:04X}> <{:04X}>\n", *ch as u32));
    }
    cmap.push_str("endbfchar\nendcmap\nend\nend\n");
    let to_unicode = b.add(Stream::new(dictionary! {}, cmap.into_bytes()));

    // /W: CID 1..=66 sind alle 560 Einheiten breit, ausser der Leerraum.
    let widths: Vec<Object> = vec![
        Object::Integer(1),
        Object::Array((1..=52).map(|_| Object::Integer(560)).collect()),
        Object::Integer(53),
        Object::Integer(53),
        Object::Integer(280),
        Object::Integer(54),
        Object::Array((54..=66).map(|_| Object::Integer(560)).collect()),
    ];

    let descriptor = b.add(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "RedactCID",
        "Flags" => 32,
        "FontBBox" => vec![(-200).into(), (-250).into(), 1000.into(), 900.into()],
        "ItalicAngle" => 0,
        "Ascent" => 750,
        "Descent" => (-250),
        "CapHeight" => 700,
        "StemV" => 80,
    });
    let descendant = b.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "RedactCID",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
        "FontDescriptor" => descriptor,
        "DW" => 1000,
        "W" => widths,
        "CIDToGIDMap" => "Identity",
    });
    let font = b.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "RedactCID",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(descendant)],
        "ToUnicode" => to_unicode,
    });
    let res = b.add(Object::Dictionary(dictionary! {
        "Font" => dictionary! { "F1" => font },
    }));

    // Text als Hex-String aus 2-Byte-CIDs.
    let cid_of =
        |ch: char| -> Option<u32> { entries.iter().find(|(_, c)| *c == ch).map(|(cid, _)| *cid) };
    let mut ops = String::from("BT\n/F1 24 Tf\n34 TL\n1 0 0 1 40 790 Tm\n");
    for line in LINES.iter() {
        let mut hex = String::new();
        for ch in line.chars() {
            if let Some(cid) = cid_of(ch) {
                hex.push_str(&format!("{cid:04X}"));
            }
        }
        ops.push_str(&format!("<{hex}> Tj T*\n"));
    }
    ops.push_str("ET\n");

    let content = b.content(&ops);
    b.simple_page(content, res, A4);
    one_page("text_type0_identity_h", Category::Text, true, b.finish())
}

/// Einfacher Font mit `/Encoding << /Differences … >>`.
fn text_encoding_differences() -> Sample {
    let mut b = Builder::new();
    // Codes 97.. ('a'..) werden auf Grossbuchstaben umgebogen.
    let mut diffs: Vec<Object> = vec![Object::Integer(97)];
    for ch in 'A'..='Z' {
        diffs.push(Object::Name(ch.to_string().into_bytes()));
    }
    let encoding = b.add(Object::Dictionary(dictionary! {
        "Type" => "Encoding",
        "BaseEncoding" => "WinAnsiEncoding",
        "Differences" => diffs,
    }));
    let font = b.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => encoding,
    });
    let res = b.add(Object::Dictionary(dictionary! {
        "Font" => dictionary! { "F1" => font },
    }));

    let lines: Vec<String> = LINES.iter().map(|l| l.to_ascii_lowercase()).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let ops = text_block("F1", 22.0, 34.0, 40.0, 790.0, &refs);
    let content = b.content(&ops);
    b.simple_page(content, res, A4);
    one_page(
        "text_encoding_differences",
        Category::Text,
        true,
        b.finish(),
    )
}

/// Font ganz ohne `/Widths` — die Breiten müssen aus den Standardmetriken
/// oder aus dem Ersatzfont kommen, sonst stapeln sich alle Glyphen.
fn text_without_widths() -> Sample {
    let mut b = Builder::new();
    let font = b.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "TrueType",
        "BaseFont" => "ArialUnbekannt",
    });
    let res = b.add(Object::Dictionary(dictionary! {
        "Font" => dictionary! { "F1" => font },
    }));
    let ops = text_block("F1", 22.0, 34.0, 40.0, 790.0, &LINES);
    let content = b.content(&ops);
    b.simple_page(content, res, A4);
    one_page("text_font_without_widths", Category::Text, true, b.finish())
}

/// `TJ` mit Kerning-Zahlen zwischen den Teilstrings.
fn text_tj_kerning() -> Sample {
    let mut b = Builder::new();
    let res = b.helvetica();
    let mut ops = String::from("BT\n/F1 22 Tf\n34 TL\n1 0 0 1 40 790 Tm\n");
    for line in LINES.iter() {
        let mut items = String::new();
        // Jedes Wort einzeln, dazwischen wechselnde Kerning-Werte.
        for (i, word) in line.split(' ').enumerate() {
            let kern = if i % 2 == 0 { -120 } else { 60 };
            items.push_str(&format!("({}) {kern} ", escape_pdf_string(word)));
        }
        ops.push_str(&format!("[{items}] TJ T*\n"));
    }
    ops.push_str("ET\n");
    let content = b.content(&ops);
    b.simple_page(content, res, A4);
    one_page("text_tj_kerning", Category::Text, true, b.finish())
}

/// Horizontale Skalierung (`Tz`) und Grundlinienversatz (`Ts`).
fn text_tz_ts() -> Sample {
    let mut b = Builder::new();
    let res = b.helvetica();
    let mut ops = String::from("BT\n/F1 22 Tf\n34 TL\n1 0 0 1 30 790 Tm\n");
    for (i, line) in LINES.iter().enumerate() {
        let tz = 60 + (i % 4) * 30;
        let ts = if i % 2 == 0 { 6 } else { -6 };
        ops.push_str(&format!(
            "{tz} Tz {ts} Ts ({}) Tj T*\n",
            escape_pdf_string(line)
        ));
    }
    ops.push_str("ET\n");
    let content = b.content(&ops);
    b.simple_page(content, res, A4);
    one_page("text_tz_ts", Category::Text, true, b.finish())
}

/// Gedrehte Textmatrix innerhalb von `BT`/`ET`.
fn text_rotated_tm() -> Sample {
    let mut b = Builder::new();
    let res = b.helvetica();
    let mut ops = String::from("BT\n/F1 20 Tf\n");
    // Acht Zeilen, jede um 30 Grad weiter gedreht, alle um die Seitenmitte.
    for (i, line) in LINES.iter().take(12).enumerate() {
        let angle = std::f64::consts::PI * (i as f64) / 6.0;
        let (s, c) = angle.sin_cos();
        ops.push_str(&format!(
            "{c:.6} {s:.6} {:.6} {c:.6} 297 421 Tm ({}) Tj\n",
            -s,
            escape_pdf_string(line)
        ));
    }
    ops.push_str("ET\n");
    let content = b.content(&ops);
    b.simple_page(content, res, A4);
    one_page("text_rotated_tm", Category::Text, true, b.finish())
}

/// Textrendermodus 3 = unsichtbar (typische OCR-Ebene über einem Scan).
/// Auf der Seite ist sonst nichts — die Ausgabe **darf** also weiss sein.
fn text_render_mode_3() -> Sample {
    let mut b = Builder::new();
    let res = b.helvetica();
    let mut ops = String::from("BT\n3 Tr\n/F1 22 Tf\n34 TL\n1 0 0 1 40 790 Tm\n");
    for line in LINES.iter() {
        ops.push_str(&format!("({}) Tj T*\n", escape_pdf_string(line)));
    }
    ops.push_str("ET\n");
    let content = b.content(&ops);
    b.simple_page(content, res, A4);
    one_page("text_render_mode_3", Category::Text, false, b.finish())
}

// ---------------------------------------------------------------------------
// Vektor
// ---------------------------------------------------------------------------

fn vector_samples() -> Vec<Sample> {
    vec![
        vector_filled_rect(),
        vector_dashed_stroke(),
        vector_bezier(),
        vector_even_odd(),
        vector_nested_q(),
        vector_clip(),
        vector_hairline(),
        vector_colorspaces(),
    ]
}

fn vector_page(name: &'static str, ops: &str) -> Sample {
    let mut b = Builder::new();
    let res = b.no_resources();
    let content = b.content(ops);
    b.simple_page(content, res, A4);
    one_page(name, Category::Vector, true, b.finish())
}

fn vector_filled_rect() -> Sample {
    vector_page(
        "vector_filled_rect",
        "0.85 0.15 0.15 rg\n60 120 470 600 re f\n",
    )
}

fn vector_dashed_stroke() -> Sample {
    let mut ops = String::from("0 0 0.8 RG\n5 w\n[12 6] 0 d\n");
    for i in 0..24 {
        let y = 60.0 + i as f64 * 30.0;
        ops.push_str(&format!("40 {y} m 555 {y} l S\n"));
    }
    ops.push_str("[3 3 9 3] 2 d\n1 0 0 RG\n8 w\n");
    for i in 0..12 {
        let x = 60.0 + i as f64 * 42.0;
        ops.push_str(&format!("{x} 60 m {x} 780 l S\n"));
    }
    vector_page("vector_dashed_stroke", &ops)
}

fn vector_bezier() -> Sample {
    let mut ops = String::from("0.1 0.5 0.2 RG\n6 w\n");
    for i in 0..10 {
        let y = 80.0 + i as f64 * 70.0;
        ops.push_str(&format!(
            "40 {y} m 180 {} 400 {} 555 {y} c S\n",
            y + 120.0,
            y - 120.0
        ));
    }
    // Zusätzlich eine gefüllte, aus Bézierkurven zusammengesetzte Fläche.
    ops.push_str(
        "0.9 0.6 0.1 rg\n\
         297 300 m 460 300 460 520 297 520 c 134 520 134 300 297 300 c f\n",
    );
    vector_page("vector_bezier", &ops)
}

fn vector_even_odd() -> Sample {
    // Fünfzackiger Stern: selbstüberschneidend, mit `f*` bleibt die Mitte leer.
    let mut ops = String::from("0.2 0.2 0.7 rg\n");
    let cx = 297.5;
    let cy = 421.0;
    let r = 300.0;
    for k in 0..5 {
        let angle = std::f64::consts::PI * 2.0 * (2 * k) as f64 / 5.0 - std::f64::consts::FRAC_PI_2;
        let (s, c) = angle.sin_cos();
        let verb = if k == 0 { "m" } else { "l" };
        ops.push_str(&format!("{:.3} {:.3} {verb}\n", cx + r * c, cy + r * s));
    }
    ops.push_str("h f*\n");
    vector_page("vector_even_odd", &ops)
}

fn vector_nested_q() -> Sample {
    let ops = "q 1 0 0 rg 40 500 240 280 re f\n\
               q 0 0.6 0 rg 80 540 160 200 re f\n\
               q 0 0 1 rg 120 580 80 120 re f Q\n\
               0.5 g 130 440 60 60 re f Q\n\
               0.9 0.4 0 rg 320 500 235 280 re f Q\n\
               0.2 0.2 0.2 rg 40 80 515 340 re f\n";
    vector_page("vector_nested_q", ops)
}

fn vector_clip() -> Sample {
    // Ganzseitige Füllung, per `W n` auf ein Rechteck begrenzt.
    let ops = "q\n120 200 360 480 re W n\n\
               0.1 0.3 0.8 rg 0 0 595 842 re f\n\
               Q\n\
               q\n60 60 100 100 re W n\n0 0 0 rg 0 0 595 842 re f\nQ\n";
    vector_page("vector_clip", ops)
}

fn vector_hairline() -> Sample {
    // `0 w` = dünnstmögliche Linie des Geräts, nicht Breite null.
    let mut ops = String::from("0 w 0 0 0 RG\n");
    for i in 0..70 {
        let y = 40.0 + i as f64 * 11.0;
        ops.push_str(&format!("20 {y} m 575 {y} l S\n"));
    }
    for i in 0..50 {
        let x = 20.0 + i as f64 * 11.0;
        ops.push_str(&format!("{x} 40 m {x} 800 l S\n"));
    }
    vector_page("vector_hairline", &ops)
}

fn vector_colorspaces() -> Sample {
    let mut b = Builder::new();
    // ICCBased braucht einen Stream mit /N; das Profil selbst wird von keinem
    // vernünftigen Renderer gebraucht, /N entscheidet über die Komponentenzahl.
    let icc = b.add(Stream::new(
        dictionary! { "N" => 3, "Alternate" => "DeviceRGB" },
        vec![0u8; 32],
    ));
    let cs = b.add(Object::Dictionary(dictionary! {
        "CS0" => vec![Object::Name(b"ICCBased".to_vec()), Object::Reference(icc)],
    }));
    let res = b.add(Object::Dictionary(dictionary! {
        "ColorSpace" => cs,
    }));
    let ops = "0.25 g 40 620 515 180 re f\n\
               0.9 0.2 0.1 rg 40 420 515 180 re f\n\
               0.1 0.8 0.2 0.05 k 40 220 515 180 re f\n\
               /CS0 cs 0.2 0.4 0.9 sc 40 20 515 180 re f\n\
               /DeviceGray CS 0 G 6 w 20 10 555 800 re S\n";
    let content = b.content(ops);
    b.simple_page(content, res, A4);
    one_page("vector_colorspaces", Category::Vector, true, b.finish())
}

// ---------------------------------------------------------------------------
// Bilder
// ---------------------------------------------------------------------------

fn image_samples() -> Vec<Sample> {
    vec![
        image_rgb8_flate(),
        image_gray8(),
        image_bilevel_1bit(),
        image_stencil_mask(),
        image_smask(),
        image_dct_jpeg(),
        image_decode_inverted(),
    ]
}

/// Legt eine Seite an, die genau ein Bild-XObject formatfüllend zeichnet.
fn image_page(
    name: &'static str,
    stream: Stream,
    extra_ops: &str,
    build: impl FnOnce(&mut Builder) -> Option<(ObjectId, &'static str)>,
) -> Sample {
    let mut b = Builder::new();
    let extra = build(&mut b);
    let img = b.add(Object::Stream(stream));
    let mut xobjects = dictionary! { "Im0" => img };
    if let Some((id, key)) = extra {
        xobjects.set(key, Object::Reference(id));
    }
    let res = b.add(Object::Dictionary(dictionary! {
        "XObject" => xobjects,
    }));
    let ops = format!("q\n{extra_ops}515 0 0 720 40 61 cm\n/Im0 Do\nQ\n");
    let content = b.content(&ops);
    b.simple_page(content, res, A4);
    one_page(name, Category::Image, true, b.finish())
}

/// 8-Bit-RGB, FlateDecode.
fn image_rgb8_flate() -> Sample {
    const W: usize = 24;
    const H: usize = 32;
    let mut raw = Vec::with_capacity(W * H * 3);
    for y in 0..H {
        for x in 0..W {
            raw.push((x * 255 / (W - 1)) as u8);
            raw.push((y * 255 / (H - 1)) as u8);
            raw.push(if (x / 4 + y / 4) % 2 == 0 { 40 } else { 200 });
        }
    }
    let stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => W as i64,
            "Height" => H as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "FlateDecode",
        },
        zlib_stored(&raw),
    );
    image_page("image_rgb8_flate", stream, "", |_| None)
}

/// 8-Bit-Graustufen, unkomprimiert.
fn image_gray8() -> Sample {
    const W: usize = 32;
    const H: usize = 32;
    let mut raw = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            raw.push(((x * 8 + y * 4) % 256) as u8);
        }
    }
    let stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => W as i64,
            "Height" => H as i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
        },
        raw,
    );
    image_page("image_gray8", stream, "", |_| None)
}

/// 1 Bit je Punkt — genau das, was ein Schwarzweiss-Scanner liefert.
fn image_bilevel_1bit() -> Sample {
    const W: usize = 64;
    const H: usize = 64;
    let raw = pack_bilevel(W, H, |x, y| {
        // Grobes Streifen-/Kästchenmuster, damit reichlich schwarze Fläche
        // entsteht — ein Scan ist nie zur Hälfte weiss.
        (x / 8 + y / 8) % 2 == 0 || y % 16 < 3
    });
    let stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => W as i64,
            "Height" => H as i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 1,
        },
        raw,
    );
    image_page("image_bilevel_1bit", stream, "", |_| None)
}

/// `/ImageMask`: eine Schablone, gemalt wird in der aktuellen Füllfarbe.
fn image_stencil_mask() -> Sample {
    const W: usize = 48;
    const H: usize = 48;
    // Bei `/Decode [0 1]` malt die 0 — also Bits löschen, wo Farbe hin soll.
    let raw = pack_bilevel(W, H, |x, y| !((x / 6 + y / 6) % 2 == 0 || x % 12 < 2));
    let stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => W as i64,
            "Height" => H as i64,
            "ImageMask" => true,
            "BitsPerComponent" => 1,
            "Decode" => vec![0.into(), 1.into()],
        },
        raw,
    );
    image_page("image_stencil_mask", stream, "0.8 0.1 0.4 rg\n", |_| None)
}

/// RGB-Bild mit weichem Alphakanal (`/SMask`).
fn image_smask() -> Sample {
    const W: usize = 32;
    const H: usize = 32;
    let mut raw = Vec::with_capacity(W * H * 3);
    for y in 0..H {
        for x in 0..W {
            raw.push(20 + (x * 6) as u8);
            raw.push(30);
            raw.push(200 - (y * 5) as u8);
        }
    }
    let mut alpha = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            // Ein Verlauf, aber nirgends ganz durchsichtig.
            alpha.push((90 + (x + y) * 2).min(255) as u8);
        }
    }
    let stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => W as i64,
            "Height" => H as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
        },
        raw,
    );
    let mut b = Builder::new();
    let smask = b.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => W as i64,
            "Height" => H as i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
        },
        alpha,
    )));
    let mut dict = stream.dict.clone();
    dict.set("SMask", Object::Reference(smask));
    let img = b.add(Object::Stream(Stream::new(dict, stream.content.clone())));
    let res = b.add(Object::Dictionary(dictionary! {
        "XObject" => dictionary! { "Im0" => img },
    }));
    let content = b.content("q\n515 0 0 720 40 61 cm\n/Im0 Do\nQ\n");
    b.simple_page(content, res, A4);
    one_page("image_smask", Category::Image, true, b.finish())
}

/// DCTDecode. Das JPEG entsteht in [`jpeg::baseline_gray`] — handgeschriebener
/// Baseline-Encoder nach ITU-T T.81, keine fremden Daten.
fn image_dct_jpeg() -> Sample {
    let (data, w, h) = jpeg::baseline_gray();
    let stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => w as i64,
            "Height" => h as i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
        },
        data,
    );
    image_page("image_dct_jpeg", stream, "", |_| None)
}

/// `/Decode [1 0]` dreht die Helligkeit um — ein heller Puffer wird dunkel.
fn image_decode_inverted() -> Sample {
    const W: usize = 32;
    const H: usize = 32;
    let mut raw = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            // Fast weiss; erst /Decode [1 0] macht daraus fast schwarz.
            raw.push(if (x / 4 + y / 4) % 2 == 0 { 250 } else { 230 });
        }
    }
    let stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => W as i64,
            "Height" => H as i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
            "Decode" => vec![1.into(), 0.into()],
        },
        raw,
    );
    image_page("image_decode_inverted", stream, "", |_| None)
}

/// Packt ein 1-Bit-Bild zeilenweise; jede Zeile beginnt an einer Bytegrenze.
/// `set(x, y) == true` setzt das Bit (Wert 1 = weiss bei `/DeviceGray`).
fn pack_bilevel(w: usize, h: usize, set: impl Fn(usize, usize) -> bool) -> Vec<u8> {
    let row_bytes = w.div_ceil(8);
    let mut out = vec![0u8; row_bytes * h];
    for y in 0..h {
        for x in 0..w {
            if set(x, y) {
                out[y * row_bytes + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    out
}

/// Zlib-Strom aus unkomprimierten („stored“) Deflate-Blöcken.
///
/// `flate2` ist keine Abhängigkeit dieses Crates und darf hier auch keine
/// werden — ein Stored-Block ist aber trivial und ergibt einen völlig
/// regulären `FlateDecode`-Strom.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01]; // CM=8, CINFO=7, FCHECK passend
    let mut chunks = data.chunks(0xFFFF).peekable();
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    }
    while let Some(chunk) = chunks.next() {
        let last = u8::from(chunks.peek().is_none());
        out.push(last);
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

// ---------------------------------------------------------------------------
// Minimaler Baseline-JPEG-Encoder
// ---------------------------------------------------------------------------

/// Erzeugt ein winziges Baseline-JPEG ohne fremde Bibliothek.
///
/// Bewusst simpel: ein Graustufenkanal, Quantisierung 16 überall, pro 8×8-Block
/// nur der DC-Koeffizient (die AC-Koeffizienten sind alle null, es folgt sofort
/// EOB). Das Ergebnis ist ein Schachbrett aus flächigen Blöcken — mehr braucht
/// der Test nicht, und es ist ein streng nach ITU-T T.81 gebautes, gültiges
/// JFIF-freies Baseline-JPEG.
mod jpeg {
    /// Kantenlänge in Blöcken.
    const BLOCKS: usize = 8;
    /// Quantisierungsfaktor für alle 64 Koeffizienten.
    const QUANT: i32 = 16;

    /// Liefert `(daten, breite, hoehe)`.
    pub fn baseline_gray() -> (Vec<u8>, usize, usize) {
        let size = BLOCKS * 8;
        let mut out = Vec::new();
        out.extend_from_slice(&[0xFF, 0xD8]); // SOI

        // DQT: eine 8-Bit-Tabelle, ID 0.
        out.extend_from_slice(&[0xFF, 0xDB]);
        out.extend_from_slice(&(67u16).to_be_bytes());
        out.push(0x00);
        out.extend(std::iter::repeat(QUANT as u8).take(64));

        // SOF0: 8 Bit Präzision, ein Kanal, kein Subsampling.
        out.extend_from_slice(&[0xFF, 0xC0]);
        out.extend_from_slice(&(11u16).to_be_bytes());
        out.push(8);
        out.extend_from_slice(&(size as u16).to_be_bytes());
        out.extend_from_slice(&(size as u16).to_be_bytes());
        out.push(1);
        out.push(1); // Komponenten-ID
        out.push(0x11); // Sampling 1x1
        out.push(0); // Quantisierungstabelle 0

        // DHT DC (Klasse 0, ID 0): zwölf Codes der Länge 4, Symbole 0..=11.
        let mut dc_bits = [0u8; 16];
        dc_bits[3] = 12;
        let dc_vals: Vec<u8> = (0..12).collect();
        write_dht(&mut out, 0x00, &dc_bits, &dc_vals);

        // DHT AC (Klasse 1, ID 0): zwei Codes der Länge 2. Benutzt wird nur
        // EOB (0x00); 0x01 existiert bloss, damit kein Ein-Bit-Code entsteht.
        let mut ac_bits = [0u8; 16];
        ac_bits[1] = 2;
        write_dht(&mut out, 0x10, &ac_bits, &[0x00, 0x01]);

        // SOS
        out.extend_from_slice(&[0xFF, 0xDA]);
        out.extend_from_slice(&(8u16).to_be_bytes());
        out.push(1);
        out.push(1);
        out.push(0x00); // DC-Tabelle 0, AC-Tabelle 0
        out.extend_from_slice(&[0x00, 0x3F, 0x00]);

        // Entropiedaten: je Block DC-Differenz, danach EOB.
        let dc_codes = canonical(&dc_bits, &dc_vals);
        let ac_codes = canonical(&ac_bits, &[0x00, 0x01]);
        let mut writer = BitWriter::default();
        let mut prev_dc = 0i32;
        for by in 0..BLOCKS {
            for bx in 0..BLOCKS {
                // Zielhelligkeit des Blocks: kräftiges Schachbrett.
                let level: i32 = if (bx + by) % 2 == 0 { 30 } else { 190 };
                // DC-Koeffizient der DCT einer konstanten Fläche:
                // 8 * (level - 128), quantisiert.
                let dc = (8 * (level - 128)) / QUANT;
                let diff = dc - prev_dc;
                prev_dc = dc;

                let (size_cat, bits) = magnitude(diff);
                let (code, len) = dc_codes[size_cat as usize];
                writer.put(code, len);
                if size_cat > 0 {
                    writer.put(bits, size_cat as u32);
                }
                // EOB
                let (code, len) = ac_codes[0];
                writer.put(code, len);
            }
        }
        out.extend_from_slice(&writer.finish());
        out.extend_from_slice(&[0xFF, 0xD9]); // EOI
        (out, size, size)
    }

    fn write_dht(out: &mut Vec<u8>, id: u8, bits: &[u8; 16], vals: &[u8]) {
        out.extend_from_slice(&[0xFF, 0xC4]);
        let len = 2 + 1 + 16 + vals.len();
        out.extend_from_slice(&(len as u16).to_be_bytes());
        out.push(id);
        out.extend_from_slice(bits);
        out.extend_from_slice(vals);
    }

    /// Kanonische Huffman-Codes: `index == symbolwert`.
    fn canonical(bits: &[u8; 16], vals: &[u8]) -> Vec<(u32, u32)> {
        let mut table = vec![(0u32, 0u32); 256];
        let mut code = 0u32;
        let mut k = 0usize;
        for (i, count) in bits.iter().enumerate() {
            let len = i as u32 + 1;
            for _ in 0..*count {
                table[vals[k] as usize] = (code, len);
                code += 1;
                k += 1;
            }
            code <<= 1;
        }
        table
    }

    /// JPEG-Grössenkategorie plus die zugehörigen Zusatzbits.
    fn magnitude(diff: i32) -> (u8, u32) {
        if diff == 0 {
            return (0, 0);
        }
        let abs = diff.unsigned_abs();
        let cat = 32 - abs.leading_zeros();
        let bits = if diff > 0 {
            abs
        } else {
            // Negative Werte: Einerkomplement innerhalb der Kategorie.
            abs ^ ((1u32 << cat) - 1)
        };
        (cat as u8, bits)
    }

    #[derive(Default)]
    struct BitWriter {
        out: Vec<u8>,
        acc: u32,
        used: u32,
    }

    impl BitWriter {
        fn put(&mut self, code: u32, len: u32) {
            for i in (0..len).rev() {
                let bit = (code >> i) & 1;
                self.acc = (self.acc << 1) | bit;
                self.used += 1;
                if self.used == 8 {
                    let byte = self.acc as u8;
                    self.out.push(byte);
                    // Byte-Stuffing: 0xFF muss ein 0x00 folgen.
                    if byte == 0xFF {
                        self.out.push(0x00);
                    }
                    self.acc = 0;
                    self.used = 0;
                }
            }
        }

        fn finish(mut self) -> Vec<u8> {
            if self.used > 0 {
                // Mit Einsen auffüllen, wie es der Standard verlangt.
                let pad = 8 - self.used;
                self.put((1u32 << pad) - 1, pad);
            }
            self.out
        }
    }
}

// ---------------------------------------------------------------------------
// Struktur
// ---------------------------------------------------------------------------

fn structure_samples() -> Vec<Sample> {
    let mut out = vec![
        struct_inherited_attributes(),
        struct_contents_array(),
        struct_form_matrix(),
        struct_form_nested(),
        struct_form_twice(),
        struct_mediabox_offset(),
        struct_cropbox_smaller(),
        struct_multipage(),
        struct_object_stream(),
    ];
    out.extend([90, 180, 270].map(struct_rotate));
    out
}

/// `/Resources` und `/MediaBox` stehen nur am `/Pages`-Knoten.
fn struct_inherited_attributes() -> Sample {
    let mut b = Builder::new();
    let res = b.helvetica();
    let ops = format!(
        "{}\n0.1 0.4 0.9 rg 40 60 515 200 re f\n",
        text_block("F1", 22.0, 34.0, 40.0, 790.0, &LINES[..12])
    );
    let content = b.content(&ops);
    b.page(dictionary! { "Contents" => content });
    let bytes = b.finish_with(dictionary! {
        "Resources" => Object::Reference(res),
        "MediaBox" => rect_obj(A4),
    });
    one_page(
        "struct_inherited_attributes",
        Category::Structure,
        true,
        bytes,
    )
}

/// `/Contents` als Array — der zweite Teil beginnt mitten in einer Operation.
///
/// Das ist erlaubt, solange die Trennung auf einer Token-Grenze liegt, und
/// bringt Parser zu Fall, die jeden Teilstrom für sich auswerten.
fn struct_contents_array() -> Sample {
    let mut b = Builder::new();
    let res = b.helvetica();
    let s1 = b.content("0.85 0.2 0.1 rg\n40 480 515 300");
    // Operanden oben, Operator hier: klassische Falle.
    let s2 = b.content(" re f\n0 0 0 rg\nBT /F1 24 Tf 1 0 0 1 50 400 Tm");
    let s3 = b.content(
        " (Fortsetzung im dritten Teilstrom) Tj ET\n\
         0.2 0.6 0.3 rg 40 60 515 280 re f\n",
    );
    let content = Object::Array(vec![
        Object::Reference(s1),
        Object::Reference(s2),
        Object::Reference(s3),
    ]);
    b.page(dictionary! {
        "Contents" => content,
        "Resources" => res,
        "MediaBox" => rect_obj(A4),
    });
    one_page(
        "struct_contents_array_split",
        Category::Structure,
        true,
        b.finish(),
    )
}

/// Form-XObject mit eigener `/Matrix`.
fn struct_form_matrix() -> Sample {
    let mut b = Builder::new();
    let res = b.helvetica();
    let form_content = "0.1 0.2 0.8 rg 0 0 100 100 re f\n\
                        1 0.6 0 rg 20 20 60 60 re f\n"
        .as_bytes()
        .to_vec();
    let form = b.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            // Skaliert und verschiebt den Inhalt des Formulars.
            "Matrix" => vec![3.into(), 0.into(), 0.into(), 5.into(), 60.into(), 120.into()],
            "Resources" => Object::Dictionary(dictionary! {}),
        },
        form_content,
    )));
    let full_res = b.add(Object::Dictionary(dictionary! {
        "XObject" => dictionary! { "Fm0" => form },
    }));
    let _ = res;
    let content = b.content("/Fm0 Do\n");
    b.simple_page(content, full_res, A4);
    one_page(
        "struct_form_xobject_matrix",
        Category::Structure,
        true,
        b.finish(),
    )
}

/// Form-XObject, das ein weiteres Form-XObject zeichnet.
fn struct_form_nested() -> Sample {
    let mut b = Builder::new();
    let inner = b.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 50.into(), 50.into()],
            "Resources" => Object::Dictionary(dictionary! {}),
        },
        b"0.9 0.1 0.3 rg 0 0 50 50 re f\n".to_vec(),
    )));
    let inner_res = b.add(Object::Dictionary(dictionary! {
        "XObject" => dictionary! { "Fm1" => inner },
    }));
    let outer = b.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
            "Resources" => Object::Reference(inner_res),
        },
        b"0.2 0.5 0.9 rg 0 0 200 200 re f\n\
          q 3 0 0 3 20 20 cm /Fm1 Do Q\n\
          q 2 0 0 2 120 120 cm /Fm1 Do Q\n"
            .to_vec(),
    )));
    let res = b.add(Object::Dictionary(dictionary! {
        "XObject" => dictionary! { "Fm0" => outer },
    }));
    let content = b.content("q 2.5 0 0 3.8 30 40 cm /Fm0 Do Q\n");
    b.simple_page(content, res, A4);
    one_page(
        "struct_form_xobject_nested",
        Category::Structure,
        true,
        b.finish(),
    )
}

/// Dasselbe Form-XObject an zwei Stellen — der Renderer muss beide Instanzen
/// zeichnen und darf den Zustand nicht zwischen ihnen verschleppen.
fn struct_form_twice() -> Sample {
    let mut b = Builder::new();
    let form = b.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => Object::Dictionary(dictionary! {}),
        },
        b"0.15 0.55 0.25 rg 0 0 100 100 re f\n\
          0 0 0 RG 4 w 10 10 80 80 re S\n"
            .to_vec(),
    )));
    let res = b.add(Object::Dictionary(dictionary! {
        "XObject" => dictionary! { "Fm0" => form },
    }));
    let content = b.content(
        "q 4 0 0 3 40 470 cm /Fm0 Do Q\n\
         q 4 0 0 3 40 80 cm /Fm0 Do Q\n",
    );
    b.simple_page(content, res, A4);
    one_page(
        "struct_form_xobject_twice",
        Category::Structure,
        true,
        b.finish(),
    )
}

/// `/Rotate` 90/180/270.
fn struct_rotate(deg: i64) -> Sample {
    let name: &'static str = match deg {
        90 => "struct_rotate_90",
        180 => "struct_rotate_180",
        _ => "struct_rotate_270",
    };
    let mut b = Builder::new();
    let res = b.helvetica();
    let ops = format!(
        "0.9 0.5 0.1 rg 40 600 515 200 re f\n{}",
        text_block("F1", 22.0, 34.0, 40.0, 560.0, &LINES[..12])
    );
    let content = b.content(&ops);
    b.page(dictionary! {
        "Contents" => content,
        "Resources" => res,
        "MediaBox" => rect_obj(A4),
        "Rotate" => deg,
    });
    one_page(name, Category::Structure, true, b.finish())
}

/// MediaBox mit einer Ecke links-unten ungleich (0,0).
fn struct_mediabox_offset() -> Sample {
    let mut b = Builder::new();
    let res = b.helvetica();
    let ops = format!(
        "0.2 0.3 0.85 rg 140 260 400 200 re f\n{}",
        text_block("F1", 22.0, 34.0, 140.0, 900.0, &LINES[..12])
    );
    let content = b.content(&ops);
    b.simple_page(content, res, [100.0, 200.0, 695.0, 1042.0]);
    one_page(
        "struct_mediabox_offset",
        Category::Structure,
        true,
        b.finish(),
    )
}

/// `/CropBox` kleiner als die MediaBox; der Inhalt liegt in der CropBox.
fn struct_cropbox_smaller() -> Sample {
    let mut b = Builder::new();
    let res = b.helvetica();
    let ops = format!(
        "0.85 0.35 0.1 rg 110 110 280 130 re f\n{}",
        text_block("F1", 14.0, 20.0, 115.0, 480.0, &LINES[..12])
    );
    let content = b.content(&ops);
    b.page(dictionary! {
        "Contents" => content,
        "Resources" => res,
        "MediaBox" => rect_obj(A4),
        "CropBox" => rect_obj([100.0, 100.0, 400.0, 500.0]),
    });
    one_page(
        "struct_cropbox_smaller",
        Category::Structure,
        true,
        b.finish(),
    )
}

/// Vier Seiten mit unterschiedlichem Inhalt.
fn struct_multipage() -> Sample {
    let mut b = Builder::new();
    let res = b.helvetica();
    for page in 0..4 {
        let shade = 0.15 + 0.2 * page as f64;
        // Jede Seite bekommt einen anderen Ausschnitt des Blindtextes.
        let lines: Vec<&str> = LINES.iter().cycle().skip(page).take(12).copied().collect();
        let ops = format!(
            "{shade:.2} 0.4 0.7 rg 40 60 515 220 re f\n{}",
            text_block("F1", 22.0, 34.0, 40.0, 790.0, &lines)
        );
        let content = b.content(&ops);
        b.simple_page(content, res, A4);
    }
    Sample {
        name: "struct_multipage",
        category: Category::Structure,
        bytes: b.finish(),
        expect_content: true,
        pages: 4,
        min_non_white: DEFAULT_MIN_NON_WHITE,
    }
}

/// Objekte in einem `/ObjStm`, adressiert über einen Cross-Reference-Stream.
///
/// `lopdf` *liest* beides, kann es aber nicht schreiben (der Writer überspringt
/// `/ObjStm`- und `/XRef`-Objekte). Deshalb entsteht diese Datei als Rohbytes.
fn struct_object_stream() -> Sample {
    let content = format!(
        "0.1 0.45 0.8 rg 40 440 515 340 re f\n\
         0.95 0.6 0.05 rg 40 60 515 340 re f\n\
         0 0 0 RG 8 w 20 20 555 800 re S\n{}",
        text_block("F1", 20.0, 30.0, 60.0, 700.0, &LINES[..8])
    );

    // Objekt 1 = Katalog, 2 = Seitenbaum, 3 = Seite, 4 = Font
    // (alle vier komprimiert in Objekt 6), 5 = Content, 6 = ObjStm, 7 = XRef.
    let compressed: [(u32, String); 4] = [
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 595 842] >>".into(),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 5 0 R \
             /Resources << /Font << /F1 4 0 R >> >> >>"
                .into(),
        ),
        (
            4,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                .into(),
        ),
    ];

    let mut body = String::new();
    let mut pairs = String::new();
    for (id, text) in &compressed {
        pairs.push_str(&format!("{id} {} ", body.len()));
        body.push_str(text);
        body.push('\n');
    }
    let objstm_payload = format!("{pairs}\n{body}");
    let first = pairs.len() + 1;

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = [0usize; 8];

    offsets[5] = out.len();
    out.extend_from_slice(format!("5 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes());
    out.extend_from_slice(content.as_bytes());
    out.extend_from_slice(b"\nendstream\nendobj\n");

    offsets[6] = out.len();
    out.extend_from_slice(
        format!(
            "6 0 obj\n<< /Type /ObjStm /N {} /First {first} /Length {} >>\nstream\n",
            compressed.len(),
            objstm_payload.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(objstm_payload.as_bytes());
    out.extend_from_slice(b"\nendstream\nendobj\n");

    // Cross-Reference-Stream, /W [1 4 2], unkomprimiert.
    let xref_offset = out.len();
    offsets[7] = xref_offset;
    let mut table: Vec<u8> = Vec::new();
    let push_entry = |kind: u8, f2: u32, f3: u16, table: &mut Vec<u8>| {
        table.push(kind);
        table.extend_from_slice(&f2.to_be_bytes());
        table.extend_from_slice(&f3.to_be_bytes());
    };
    push_entry(0, 0, 0xFFFF, &mut table); // Objekt 0: frei
    for (index, (id, _)) in compressed.iter().enumerate() {
        debug_assert_eq!(*id as usize, index + 1);
        push_entry(2, 6, index as u16, &mut table);
    }
    push_entry(1, offsets[5] as u32, 0, &mut table);
    push_entry(1, offsets[6] as u32, 0, &mut table);
    push_entry(1, xref_offset as u32, 0, &mut table);

    out.extend_from_slice(
        format!(
            "7 0 obj\n<< /Type /XRef /Size 8 /W [1 4 2] /Root 1 0 R /Length {} >>\nstream\n",
            table.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&table);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    out.extend_from_slice(format!("startxref\n{xref_offset}\n%%EOF\n").as_bytes());

    one_page("struct_objstm_xref_stream", Category::Structure, true, out)
}

// ---------------------------------------------------------------------------
// Bösartig / entartet
// ---------------------------------------------------------------------------

/// Ein grosses, eindeutig sichtbares Rechteck. Steht in den kaputten Beispielen
/// vor und hinter dem Unsinn: der Test prüft damit, dass eine defekte Stelle
/// nicht den Rest der Seite mitreisst.
const ANCHOR_BEFORE: &str = "0.1 0.35 0.75 rg 40 440 515 340 re f\n";
const ANCHOR_AFTER: &str = "\n0.9 0.5 0.05 rg 40 60 515 340 re f\n";

fn hostile_samples() -> Vec<Sample> {
    vec![
        hostile_empty_content(),
        hostile_no_contents(),
        hostile_garbage_content(),
        hostile_unbalanced_q(),
        hostile_lonely_restore(),
        hostile_singular_matrix(),
        hostile_absurd_coordinates(),
        hostile_zero_mediabox(),
        hostile_negative_mediabox(),
        hostile_truncated_image(),
        hostile_broken_fontfile(),
        hostile_recursive_form(),
        hostile_huge_content(),
    ]
}

fn hostile_page(name: &'static str, expect_content: bool, ops: &str) -> Sample {
    let mut b = Builder::new();
    let res = b.no_resources();
    let content = b.content(ops);
    b.simple_page(content, res, A4);
    one_page(name, Category::Hostile, expect_content, b.finish())
}

fn hostile_empty_content() -> Sample {
    hostile_page("hostile_empty_content", false, "")
}

/// Seite ganz ohne `/Contents`-Eintrag.
fn hostile_no_contents() -> Sample {
    let mut b = Builder::new();
    let res = b.no_resources();
    b.page(dictionary! {
        "Resources" => res,
        "MediaBox" => rect_obj(A4),
    });
    one_page("hostile_no_contents", Category::Hostile, false, b.finish())
}

/// Reiner Byte-Müll im Content-Stream.
fn hostile_garbage_content() -> Sample {
    let mut b = Builder::new();
    let res = b.no_resources();
    // Deterministischer Pseudozufall, damit das Korpus reproduzierbar bleibt.
    let mut state: u32 = 0x1234_5678;
    let mut junk = Vec::with_capacity(4096);
    for _ in 0..4096 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        junk.push((state >> 16) as u8);
    }
    let content = b.add(Object::Stream(Stream::new(dictionary! {}, junk)));
    b.simple_page(content, res, A4);
    one_page(
        "hostile_garbage_content",
        Category::Hostile,
        false,
        b.finish(),
    )
}

/// Drei `q` ohne passendes `Q`.
fn hostile_unbalanced_q() -> Sample {
    hostile_page(
        "hostile_unbalanced_q",
        true,
        &format!("q q q\n{ANCHOR_BEFORE}q 0.5 g 100 100 200 200 re f{ANCHOR_AFTER}"),
    )
}

/// `Q` ohne vorheriges `q` — der Grafikzustandsstapel läuft leer.
fn hostile_lonely_restore() -> Sample {
    hostile_page(
        "hostile_lonely_restore",
        true,
        &format!("Q Q Q Q\n{ANCHOR_BEFORE}Q Q{ANCHOR_AFTER}Q\n"),
    )
}

/// Nicht invertierbare Transformationsmatrix.
fn hostile_singular_matrix() -> Sample {
    hostile_page(
        "hostile_singular_matrix",
        true,
        &format!(
            "{ANCHOR_BEFORE}\
             q 0 0 0 0 0 0 cm 0 0 1 rg 0 0 500 500 re f Q\n\
             q 1 2 2 4 10 10 cm 1 0 0 rg 0 0 300 300 re f Q\
             {ANCHOR_AFTER}"
        ),
    )
}

/// Absurd grosse Koordinaten und unbrauchbare Zahlenliterale.
fn hostile_absurd_coordinates() -> Sample {
    hostile_page(
        "hostile_absurd_coordinates",
        true,
        &format!(
            "{ANCHOR_BEFORE}\
             q 1e20 1e20 m 1e20 -1e20 l -1e20 1e20 l h f Q\n\
             q 1e30 0 0 1e30 -1e30 -1e30 cm 0 0 1 rg 0 0 1 1 re f Q\n\
             q nan nan m inf inf l S Q\n\
             q 0.0000000001 w -99999999 -99999999 99999999 99999999 re S Q\
             {ANCHOR_AFTER}"
        ),
    )
}

/// MediaBox ohne Fläche.
fn hostile_zero_mediabox() -> Sample {
    let mut b = Builder::new();
    let res = b.no_resources();
    let content = b.content("0 0 1 rg 0 0 100 100 re f\n");
    b.simple_page(content, res, [0.0, 0.0, 0.0, 0.0]);
    one_page(
        "hostile_zero_mediabox",
        Category::Hostile,
        false,
        b.finish(),
    )
}

/// MediaBox mit negativer Ausdehnung (Ecken vertauscht).
fn hostile_negative_mediabox() -> Sample {
    let mut b = Builder::new();
    let res = b.no_resources();
    let content = b.content("0.2 0.7 0.3 rg -500 -700 400 500 re f\n");
    b.simple_page(content, res, [0.0, 0.0, -595.0, -842.0]);
    one_page(
        "hostile_negative_mediabox",
        Category::Hostile,
        false,
        b.finish(),
    )
}

/// Bildstrom, der viel zu wenige Bytes für die angegebene Grösse enthält.
fn hostile_truncated_image() -> Sample {
    let mut b = Builder::new();
    let img = b.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 512,
            "Height" => 512,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "FlateDecode",
        },
        // Als FlateDecode deklariert, aber weder Zlib-Kopf noch genug Daten.
        b"\x78\x9c kaputt".to_vec(),
    )));
    let res = b.add(Object::Dictionary(dictionary! {
        "XObject" => dictionary! { "Im0" => img },
    }));
    let content = b.content(&format!(
        "{ANCHOR_BEFORE}q 515 0 0 300 40 100 cm /Im0 Do Q{ANCHOR_AFTER}"
    ));
    b.simple_page(content, res, A4);
    one_page(
        "hostile_truncated_image",
        Category::Hostile,
        true,
        b.finish(),
    )
}

/// `/FontFile2` enthält Müll — der Renderer muss auf einen Ersatzfont wechseln.
fn hostile_broken_fontfile() -> Sample {
    let mut b = Builder::new();
    let file = b.add(Object::Stream(Stream::new(
        dictionary! { "Length1" => 64 },
        b"das ist ganz sicher kein TrueType-Font, sondern nur Text.".to_vec(),
    )));
    let descriptor = b.add(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "KaputtTT",
        "Flags" => 32,
        "FontBBox" => vec![0.into(), 0.into(), 1000.into(), 1000.into()],
        "ItalicAngle" => 0,
        "Ascent" => 750,
        "Descent" => (-250),
        "CapHeight" => 700,
        "StemV" => 80,
        "FontFile2" => file,
    });
    let font = b.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "TrueType",
        "BaseFont" => "KaputtTT",
        "FirstChar" => 32,
        "LastChar" => 126,
        "Widths" => (32..=126).map(|_| Object::Integer(556)).collect::<Vec<_>>(),
        "FontDescriptor" => descriptor,
        "Encoding" => "WinAnsiEncoding",
    });
    let res = b.add(Object::Dictionary(dictionary! {
        "Font" => dictionary! { "F1" => font },
    }));
    let ops = format!(
        "{ANCHOR_BEFORE}{}",
        text_block("F1", 22.0, 34.0, 40.0, 400.0, &LINES[..10])
    );
    let content = b.content(&ops);
    b.simple_page(content, res, A4);
    one_page(
        "hostile_broken_fontfile2",
        Category::Hostile,
        true,
        b.finish(),
    )
}

/// Form-XObject, das sich selbst zeichnet — ohne Rekursionsbremse endlos.
fn hostile_recursive_form() -> Sample {
    let mut b = Builder::new();
    let form_id = b.doc.new_object_id();
    let res_id = b.doc.new_object_id();
    b.doc.objects.insert(
        res_id,
        Object::Dictionary(dictionary! {
            "XObject" => dictionary! { "Fm0" => Object::Reference(form_id) },
        }),
    );
    b.doc.objects.insert(
        form_id,
        Object::Stream(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 400.into(), 400.into()],
                "Resources" => Object::Reference(res_id),
            },
            b"0.2 0.6 0.9 rg 0 0 400 400 re f\n\
              q 0.8 0 0 0.8 20 20 cm /Fm0 Do Q\n"
                .to_vec(),
        )),
    );
    let content = b.content(&format!("{ANCHOR_BEFORE}q 1 0 0 1 60 60 cm /Fm0 Do Q\n"));
    b.simple_page(content, res_id, A4);
    one_page(
        "hostile_recursive_form",
        Category::Hostile,
        true,
        b.finish(),
    )
}

/// Rund 2 MB Content-Stream — die Leistungsbremse des Korpus.
fn hostile_huge_content() -> Sample {
    let mut ops = String::with_capacity(2_200_000);
    let mut state: u32 = 0x2468_ACE0;
    let mut next = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 16) as f64 / 65_536.0
    };
    while ops.len() < 2_000_000 {
        let x = 10.0 + next() * 570.0;
        let y = 10.0 + next() * 820.0;
        let r = next();
        let g = next();
        ops.push_str(&format!("{r:.3} {g:.3} 0.4 rg {x:.2} {y:.2} 12 12 re f\n"));
    }
    hostile_page("hostile_huge_content", true, &ops)
}

// ---------------------------------------------------------------------------
// Echte Dateien
// ---------------------------------------------------------------------------

/// Selbstgebaute Dateien treffen immer nur das, woran der Autor gedacht hat.
/// Deshalb kommen — sofern vorhanden — ein paar echte PDFs von dieser Maschine
/// dazu: erzeugt von fremden Werkzeugen, mit allem, was die so einbauen.
///
/// Alle Pfade sind mit [`Path::exists`] abgesichert; fehlt eine Datei, fällt
/// das Beispiel einfach weg und der Rest des Korpus läuft weiter.
fn real_samples() -> Vec<Sample> {
    let mut out = Vec::new();
    let mut candidates: Vec<(&'static str, PathBuf)> = vec![
        (
            "real_theme_showcase",
            PathBuf::from("/mnt/skills/examples/theme-factory/theme-showcase.pdf"),
        ),
        (
            "real_libreoffice_xpdfimport_err",
            PathBuf::from("/usr/lib/libreoffice/share/xpdfimport/xpdfimport_err.pdf"),
        ),
    ];
    for (name, file) in [
        ("real_lopdf_annotation_demo", "AnnotationDemo.pdf"),
        ("real_lopdf_incremental", "Incremental.pdf"),
        ("real_lopdf_example", "example.pdf"),
        ("real_lopdf_unicode", "unicode.pdf"),
    ] {
        if let Some(path) = cargo_registry_asset("lopdf-0.34.0", file) {
            candidates.push((name, path));
        }
    }

    for (name, path) in candidates {
        if !path.exists() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        // Seitenzahl wird gemessen, nicht geraten. Was sich nicht laden lässt,
        // gehört nicht in dieses (nicht-bösartige) Segment des Korpus.
        let Ok(doc) = redact_pdf::load_from_bytes(&bytes) else {
            continue;
        };
        let pages = redact_pdf::page_count(&doc);
        if pages == 0 {
            continue;
        }
        out.push(Sample {
            name,
            category: Category::Real,
            bytes,
            // Echte Seiten aus echten Werkzeugen zeigen immer etwas; genau
            // dieser Anspruch wird hier geprüft.
            expect_content: true,
            pages,
            // `unicode.pdf` (Google Docs / Skia) besteht aus genau einer
            // kurzen Textzeile auf einer Letter-Seite — gemessen 0,04 %
            // Farbe. Die Regelschwelle von 0,1 % wäre hier keine Aussage über
            // die Vorschau, sondern über den Inhalt der Datei. Die Zeile muss
            // trotzdem erscheinen, deshalb bleibt eine Schwelle stehen, nur
            // eine niedrigere.
            min_non_white: match name {
                "real_lopdf_unicode" => 0.0002,
                _ => DEFAULT_MIN_NON_WHITE,
            },
        });
    }
    out
}

/// Sucht `~/.cargo/registry/src/<irgendein-index>/<krate>/assets/<datei>`.
///
/// Der Verzeichnisname des Index ist ein Hash und darf nicht fest verdrahtet
/// werden, deshalb wird die Ebene durchsucht statt geraten.
fn cargo_registry_asset(crate_dir: &str, file: &str) -> Option<PathBuf> {
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cargo")))?;
    let entries = std::fs::read_dir(home.join("registry/src")).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join(crate_dir).join("assets").join(file);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}
