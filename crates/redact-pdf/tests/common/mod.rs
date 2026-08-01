//! Handgebaute PDFs, die ein bekanntes Geheimnis an jeweils *einer* Stelle
//! verstecken.
//!
//! Zwei Aufgaben:
//!
//! * **Kalibrierung** — ein Messgerät, das man nicht gegen bekannte Proben
//!   geprüft hat, ist wertlos. `leak_detector.rs` prüft damit, dass
//!   [`redact_pdf::leaks`] jede dieser Verstecke findet.
//! * **Regression** — `known_leaks.rs` schickt dieselben Dokumente durch die
//!   echte Schwärzung und misst, was danach noch in der Datei steht.

// Nicht jede Testbinary benutzt jeden Baustein.
#![allow(dead_code)]

use lopdf::{dictionary, Document, Object, ObjectId, Stream, StringFormat};

/// Das Geheimnis, das in allen Szenarien versteckt wird.
pub const SECRET: &str = "DE89 3704 0044 0532 0130 00";

/// Ein Grundgerüst mit einer Seite; die Bausteine hängen sich daran.
pub struct Doc {
    pub doc: Document,
    pub catalog_id: ObjectId,
    pub pages_id: ObjectId,
    pub page_id: ObjectId,
    pub resources_id: ObjectId,
    pub content_id: ObjectId,
    pub font_id: ObjectId,
}

impl Doc {
    pub fn finish(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        self.doc.clone().save_to(&mut buffer).expect("speicherbar");
        buffer
    }

    /// Ersetzt den Seiteninhalt vollständig (Rohbytes eines Content-Streams).
    pub fn set_content(&mut self, raw: &[u8]) {
        self.doc.objects.insert(
            self.content_id,
            Object::Stream(Stream::new(dictionary! {}, raw.to_vec())),
        );
    }

    pub fn page_dict_set(&mut self, key: &str, value: Object) {
        self.doc
            .get_dictionary_mut(self.page_id)
            .expect("Seite")
            .set(key, value);
    }

    pub fn catalog_set(&mut self, key: &str, value: Object) {
        self.doc
            .get_dictionary_mut(self.catalog_id)
            .expect("Katalog")
            .set(key, value);
    }

    pub fn add(&mut self, object: Object) -> ObjectId {
        self.doc.add_object(object)
    }
}

/// Erzeugt eine Seite mit sichtbarem Text bei x=72, y=700, Zeilenabstand 15.
pub fn page(lines: &[&str]) -> Doc {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content_id = doc.add_object(Stream::new(dictionary! {}, text_ops(lines)));
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

    Doc {
        doc,
        catalog_id,
        pages_id,
        page_id,
        resources_id,
        content_id,
        font_id,
    }
}

/// Baut einen Content-Stream aus Textzeilen (ASCII, WinAnsi = ASCII).
pub fn text_ops(lines: &[&str]) -> Vec<u8> {
    let mut out = String::from("BT\n/F1 10 Tf\n72 700 Td\n");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push_str("0 -15 Td\n");
        }
        out.push_str(&format!("({}) Tj\n", escape(line)));
    }
    out.push_str("ET\n");
    out.into_bytes()
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn stream_with_filter(filter: &str, content: Vec<u8>) -> Stream {
    Stream::new(
        dictionary! { "Filter" => Object::Name(filter.as_bytes().to_vec()) },
        content,
    )
    // Absicherung: der Stream ist bereits kodiert und darf nicht noch einmal
    // angefasst werden, damit die Kalibrierung wirklich diesen Filter misst.
    .with_compression(false)
}

// ---------------------------------------------------------------------------
// Szenarien aus dem Audit
// ---------------------------------------------------------------------------

/// Inline-Bild (`BI … ID … EI`) **vor** weiterem Text im selben Stream.
///
/// `lopdf::content::Content::decode` kennt keine Inline-Bilder und verliert
/// alles dahinter.
pub fn inline_image_before_text(secret: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let mut raw = Vec::new();
    raw.extend_from_slice(b"q 20 0 0 20 300 780 cm\n");
    raw.extend_from_slice(b"BI /W 2 /H 2 /CS /G /BPC 8 ID ");
    raw.extend_from_slice(&[0x00, 0xff, 0x7f, 0x30]);
    raw.extend_from_slice(b" EI Q\n");
    raw.extend_from_slice(&text_ops(&[
        "Kontoinhaber: Max Mustermann",
        &format!("IBAN: {secret}"),
    ]));
    d.set_content(&raw);
    d.finish()
}

/// Ein Form-XObject mit dem Geheimnis; die Seite zeichnet es per `Do`.
/// Optional steht vor dem `Do` ein Inline-Bild im Seiten-Stream.
pub fn form_xobject(secret: &str, inline_image_first: bool) -> Vec<u8> {
    let mut d = page(&[]);
    let form_content = {
        let mut ops = String::from("BT\n/F1 10 Tf\n72 640 Td\n");
        ops.push_str(&format!("(IBAN: {}) Tj\n", escape(secret)));
        ops.push_str("ET\n");
        ops.into_bytes()
    };
    let font_id = d.font_id;
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            form_content,
        )
        .with_compression(false),
    ));
    d.doc
        .get_dictionary_mut(d.resources_id)
        .expect("Resources")
        .set("XObject", dictionary! { "Fm0" => form_id });

    let mut raw = Vec::new();
    raw.extend_from_slice(&text_ops(&["Kontoinhaber: Max Mustermann"]));
    if inline_image_first {
        raw.extend_from_slice(b"q 20 0 0 20 300 780 cm\n");
        raw.extend_from_slice(b"BI /W 2 /H 2 /CS /G /BPC 8 ID ");
        raw.extend_from_slice(&[0x00, 0xff, 0x7f, 0x30]);
        raw.extend_from_slice(b" EI Q\n");
    }
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    d.finish()
}

/// Annotation, deren Appearance-Stream (`/AP /N`) das Geheimnis trägt.
///
/// `intersecting = true`: `/Rect` überlappt die Zeile, die geschwärzt wird.
pub fn annotation_appearance(secret: &str, intersecting: bool) -> Vec<u8> {
    let mut d = page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {secret}")]);
    let font_id = d.font_id;
    let ap_content = format!("BT\n/F1 8 Tf\n0 4 Td\n(Notiz: {}) Tj\nET\n", escape(secret));
    let ap_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 240.into(), 20.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            ap_content.into_bytes(),
        )
        .with_compression(false),
    ));
    let rect = if intersecting {
        vec![72.into(), 680.into(), 312.into(), 700.into()]
    } else {
        vec![400.into(), 100.into(), 540.into(), 120.into()]
    };
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FreeText",
        "Rect" => rect,
        "F" => 4_i64,
        "AP" => dictionary! { "N" => ap_id },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
    d.finish()
}

// ---------------------------------------------------------------------------
// Marked Content mit Textspiegel (`/ActualText`, `/Alt`)
// ---------------------------------------------------------------------------

/// Wo und wie der Textspiegel im Strom steht.
///
/// `/ActualText` und `/Alt` sind der kanonische Textspiegel eines
/// Marked-Content-Abschnitts: sie sollen dasselbe sagen wie die Glyphen
/// darunter. Word („Als PDF speichern“), InDesign und jeder PDF/UA-Erzeuger
/// schreiben sie routinemäßig — für Ligaturen, Tabellen, Sonderzeichen.
#[derive(Debug, Clone, Copy)]
pub struct Mirror {
    /// Marked-Content-Tag, z.B. `Span` oder `Figure`.
    pub tag: &'static str,
    /// Schlüssel in der Eigenschaftsliste: `ActualText` oder `Alt`.
    pub key: &'static str,
    /// `BDC` (Klammer mit `EMC`) oder `DP` (Punkt ohne Klammer).
    pub operator: &'static str,
    /// Den Wert als Hex-String `<44 45 …>` statt als Literal `(…)` schreiben.
    pub hex: bool,
    /// Eigenschaftsliste als eigenes Objekt unter `/Resources /Properties`
    /// statt inline im Strom.
    pub via_properties: bool,
    /// Den Abschnitt in ein Form-XObject legen statt in den Seitenstrom.
    pub in_form: bool,
    /// Den Wert der Eigenschaftsliste als **indirekten Verweis** führen
    /// (`/ActualText 12 0 R`). Nur zusammen mit [`Mirror::via_properties`]
    /// sinnvoll: inline im Strom sind Verweise nicht zulässig.
    pub indirect_value: bool,
}

impl Default for Mirror {
    fn default() -> Self {
        Self {
            tag: "Span",
            key: "ActualText",
            operator: "BDC",
            hex: false,
            via_properties: false,
            in_form: false,
            indirect_value: false,
        }
    }
}

impl Mirror {
    /// Der Wert des Textschlüssels, so wie er im Strom steht.
    fn value(&self, secret: &str) -> String {
        if self.hex {
            let hex: String = secret.bytes().map(|b| format!("{b:02X}")).collect();
            format!("<{hex}>")
        } else {
            format!("({})", escape(secret))
        }
    }

    /// Die Eigenschaftsliste als Operand des `BDC`/`DP`.
    fn property_operand(&self, secret: &str) -> String {
        if self.via_properties {
            "/MC0".to_string()
        } else {
            format!("<< /{} {} >>", self.key, self.value(secret))
        }
    }
}

/// Eine Seite, deren IBAN-Zeile zusätzlich in einem Marked-Content-Textspiegel
/// steht.
///
/// Die Glyphen stehen ganz normal im `Tj`; der Spiegel wiederholt sie in der
/// Eigenschaftsliste. Wer nur die Glyphen entfernt, lässt den Klartext stehen —
/// `pdftotext` gibt in der Voreinstellung den Spiegel aus.
pub fn marked_content_mirror(secret: &str, mirror: Mirror) -> Vec<u8> {
    let mut d = page(&[]);

    let open = format!(
        "/{} {} {}\n",
        mirror.tag,
        mirror.property_operand(secret),
        mirror.operator
    );
    let close = if mirror.operator == "BDC" {
        "EMC\n"
    } else {
        ""
    };

    let secret_line = format!("(IBAN: {}) Tj\n", escape(secret));

    if mirror.in_form {
        let font_id = d.font_id;
        let form_content = format!("BT\n/F1 10 Tf\n72 640 Td\n{open}{secret_line}{close}ET\n");
        let mut form_dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        };
        let mut form_resources = dictionary! { "Font" => dictionary! { "F1" => font_id } };
        if mirror.via_properties {
            let value = mirror_value_object(&mut d, secret, mirror);
            let props_id = d.add(Object::Dictionary(dictionary! { mirror.key => value }));
            form_resources.set("Properties", dictionary! { "MC0" => props_id });
        }
        form_dict.set("Resources", Object::Dictionary(form_resources));
        let form_id = d.add(Object::Stream(
            Stream::new(form_dict, form_content.into_bytes()).with_compression(false),
        ));
        d.doc
            .get_dictionary_mut(d.resources_id)
            .expect("Resources")
            .set("XObject", dictionary! { "Fm0" => form_id });

        let mut raw = text_ops(&["Kontoinhaber: Max Mustermann"]);
        raw.extend_from_slice(b"q /Fm0 Do Q\n");
        d.set_content(&raw);
        return d.finish();
    }

    if mirror.via_properties {
        let value = mirror_value_object(&mut d, secret, mirror);
        let props_id = d.add(Object::Dictionary(dictionary! { mirror.key => value }));
        d.doc
            .get_dictionary_mut(d.resources_id)
            .expect("Resources")
            .set("Properties", dictionary! { "MC0" => props_id });
    }

    let raw = format!(
        "BT\n/F1 10 Tf\n72 700 Td\n(Kontoinhaber: Max Mustermann) Tj\n0 -15 Td\n\
         {open}{secret_line}{close}ET\n"
    );
    d.set_content(raw.as_bytes());
    d.finish()
}

/// Der Wert des Textschlüssels als PDF-Objekt (für die Fassung im
/// `/Properties`-Objekt).
fn mirror_value_object(d: &mut Doc, secret: &str, mirror: Mirror) -> Object {
    let value = if mirror.hex {
        Object::String(secret.as_bytes().to_vec(), StringFormat::Hexadecimal)
    } else {
        Object::string_literal(secret)
    };
    if mirror.indirect_value {
        Object::Reference(d.add(value))
    } else {
        value
    }
}

/// Zwei Marked-Content-Abschnitte mit je eigenem Textspiegel: der erste trägt
/// das Geheimnis, der zweite (`harmless`) steht in einer eigenen Zeile weit
/// darunter und wird von keiner Schwärzung berührt.
///
/// Gegenprobe zu [`marked_content_mirror`]: der unbeteiligte Spiegel muss
/// erhalten bleiben.
pub fn marked_content_two_sections(secret: &str, harmless: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let raw = format!(
        "BT\n/F1 10 Tf\n72 700 Td\n\
         /Span << /ActualText ({secret_escaped}) >> BDC\n\
         (IBAN: {secret_escaped}) Tj\n\
         EMC\n\
         ET\n\
         BT\n/F1 10 Tf\n72 200 Td\n\
         /Span << /ActualText ({harmless_escaped}) >> BDC\n\
         (Bank: {harmless_escaped}) Tj\n\
         EMC\n\
         ET\n\
         BT\n/F1 10 Tf\n72 180 Td\n\
         /Span << /ActualText ({harmless_escaped}) >> DP\n\
         (Filiale: {harmless_escaped}) Tj\n\
         ET\n",
        secret_escaped = escape(secret),
        harmless_escaped = escape(harmless),
    );
    d.set_content(raw.as_bytes());
    d.finish()
}

/// `/StructElem` mit `/ActualText`, das den geschwärzten Text spiegelt.
pub fn struct_elem_actual_text(secret: &str) -> Vec<u8> {
    let mut d = page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {secret}")]);
    let elem_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "Span",
        "ActualText" => Object::string_literal(secret),
        "Alt" => Object::string_literal(format!("Kontonummer {secret}")),
    }));
    let root_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructTreeRoot",
        "K" => vec![Object::Reference(elem_id)],
    }));
    d.catalog_set("StructTreeRoot", Object::Reference(root_id));
    d.catalog_set(
        "MarkInfo",
        Object::Dictionary(dictionary! { "Marked" => true }),
    );
    d.finish()
}

/// XMP-Metadaten **auf Seitenebene** (`/Metadata` im Seiten-Dictionary),
/// Flate-komprimiert — eine reine Rohbyte-Suche sieht das nicht.
pub fn page_metadata_xmp(secret: &str) -> Vec<u8> {
    let mut d = page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {secret}")]);
    let xmp = format!(
        "<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
         xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
         <rdf:Description><dc:title>Kontoauszug {secret}</dc:title>\
         </rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    );
    let mut stream = Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        xmp.into_bytes(),
    );
    stream.compress().expect("komprimierbar");
    let meta_id = d.add(Object::Stream(stream));
    d.page_dict_set("Metadata", Object::Reference(meta_id));
    d.finish()
}

/// AcroForm-Feld, dessen `/V` das Geheimnis als UTF-16BE mit BOM trägt.
pub fn form_field_value(secret: &str) -> Vec<u8> {
    let mut d = page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {secret}")]);
    let field_id = d.add(Object::Dictionary(dictionary! {
        "FT" => "Tx",
        "T" => Object::string_literal("iban"),
        "TU" => Object::string_literal("Bitte IBAN eintragen"),
        "V" => Object::String(utf16be_bom(secret), StringFormat::Literal),
    }));
    d.catalog_set(
        "AcroForm",
        Object::Dictionary(dictionary! {
            "Fields" => vec![Object::Reference(field_id)],
        }),
    );
    d.finish()
}

/// Ein verwaistes Objekt: von nirgends referenziert, aber weiterhin in
/// `doc.objects` — `save_to` schreibt es wortwörtlich mit.
pub fn orphan_object(secret: &str) -> Vec<u8> {
    let mut d = page(&["Kontoinhaber: Max Mustermann"]);
    d.add(Object::Dictionary(dictionary! {
        "Type" => "Vergessen",
        "ActualText" => Object::string_literal(secret),
    }));
    d.finish()
}

/// Objekt-Stream (`/ObjStm`): ein Flate-komprimierter Container voller
/// Objekte. Eine Rohbyte-Suche findet darin nichts.
///
/// Muss von Hand angehängt werden: `lopdf::Document::save_to` überspringt
/// Objekte vom Typ `/ObjStm` beim Schreiben. Ein Eingabedokument darf so etwas
/// aber selbstverständlich enthalten.
pub fn object_stream(secret: &str) -> Vec<u8> {
    let d = page(&["Kontoinhaber: Max Mustermann"]);
    let catalog_id = d.catalog_id;
    // +1 vergibt `save_to` selbst für den XRef-Stream.
    let objstm_id = d.doc.max_id + 2;
    let inner_id = objstm_id + 1;
    let base = d.finish();

    let header = format!("{inner_id} 0 ");
    let mut plain = header.clone().into_bytes();
    // Füllmaterial, damit Deflate wirklich komprimiert statt einen
    // „stored block“ zu schreiben — sonst stünde das Geheimnis im Klartext da
    // und der Test würde die falsche Ebene messen.
    let filler = "A".repeat(512);
    plain.extend_from_slice(
        format!(
            "<</Type/Vergessen/Alt({filler})/ActualText({})>>",
            escape(secret)
        )
        .as_bytes(),
    );
    let packed = deflate(&plain);

    let mut body = format!(
        "<</Type/ObjStm/N 1/First {}/Filter/FlateDecode/Length {}>>\nstream\n",
        header.len(),
        packed.len()
    )
    .into_bytes();
    body.extend_from_slice(&packed);
    body.extend_from_slice(b"\nendstream");

    // Zusätzlich ein Platzhalter mit hoher Id: sonst hält `lopdf` die Id des
    // ausgepackten Objekts für frei und vergibt sie beim Schreiben neu — das
    // Objekt verschwände dann aus Versehen statt aus Absicht.
    let filler_id = inner_id + 10;
    append_revision(
        base,
        catalog_id,
        filler_id + 1,
        &[
            (objstm_id, body),
            (filler_id, b"<</Type/Platzhalter>>".to_vec()),
        ],
    )
}

fn deflate(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use std::io::Write;
    let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(data).expect("komprimierbar");
    encoder.finish().expect("komprimierbar")
}

/// Ein Stream mit dem angegebenen Filter, der das Geheimnis enthält.
pub fn filtered_stream(secret: &str, filter: &str) -> Vec<u8> {
    let mut d = page(&["Kontoinhaber: Max Mustermann"]);
    let plain = format!("Notiz zur IBAN {secret}").into_bytes();
    let stream = match filter {
        "FlateDecode" => {
            let mut s = Stream::new(dictionary! {}, plain);
            s.compress().expect("komprimierbar");
            s.with_compression(false)
        }
        "ASCIIHexDecode" => stream_with_filter(filter, ascii_hex_encode(&plain)),
        "ASCII85Decode" => stream_with_filter(filter, ascii85_encode(&plain)),
        "RunLengthDecode" => stream_with_filter(filter, run_length_encode(&plain)),
        "LZWDecode" => stream_with_filter(filter, lzw_encode(&plain)),
        other => panic!("unbekannter Filter {other}"),
    };
    d.add(Object::Stream(stream));
    d.finish()
}

/// Ein PDF mit `/Prev`-Kette: die Basisrevision enthält das Geheimnis, die
/// angehängte Revision ersetzt den Seiteninhalt durch `replacement`.
pub fn incremental_history(secret: &str, replacement: &str) -> Vec<u8> {
    let d = page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {secret}")]);
    let content_id = d.content_id;
    let catalog_id = d.catalog_id;
    let size = d.doc.max_id + 2;
    let base = d.finish();

    let new_content = text_ops(&[
        "Kontoinhaber: Max Mustermann",
        &format!("IBAN: {replacement}"),
    ]);
    let mut body = format!("<</Length {}>>\nstream\n", new_content.len()).into_bytes();
    body.extend_from_slice(&new_content);
    body.extend_from_slice(b"\nendstream");

    append_revision(base, catalog_id, size, &[(content_id.0, body)])
}

/// Hängt eine weitere Revision an: Objekte, klassische xref-Sektion, `/Prev`.
fn append_revision(
    mut out: Vec<u8>,
    catalog_id: ObjectId,
    size: u32,
    objects: &[(u32, Vec<u8>)],
) -> Vec<u8> {
    let prev = last_startxref(&out).expect("startxref in der Basisrevision");
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }

    let mut offsets = Vec::new();
    for (id, body) in objects {
        offsets.push((*id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }

    let xref_offset = out.len();
    let mut xref = String::from("xref\n0 1\n0000000000 65535 f \n");
    for (id, offset) in &offsets {
        xref.push_str(&format!("{id} 1\n{offset:010} 00000 n \n"));
    }
    xref.push_str(&format!(
        "trailer\n<</Size {size} /Root {} {} R /Prev {prev}>>\nstartxref\n{xref_offset}\n%%EOF\n",
        catalog_id.0, catalog_id.1
    ));
    out.extend_from_slice(xref.as_bytes());
    out
}

fn last_startxref(bytes: &[u8]) -> Option<usize> {
    let key = b"startxref";
    let pos = bytes
        .windows(key.len())
        .enumerate()
        .rfind(|(_, w)| *w == key)
        .map(|(i, _)| i)?;
    let tail = &bytes[pos + key.len()..];
    let digits: String = tail
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|&b| b as char)
        .collect();
    digits.parse().ok()
}

// ---------------------------------------------------------------------------
// Kodierer für die Filter-Kalibrierung
// ---------------------------------------------------------------------------

pub fn utf16be_bom(text: &str) -> Vec<u8> {
    let mut out = vec![0xfe, 0xff];
    out.extend(text.encode_utf16().flat_map(|u| u.to_be_bytes()));
    out
}

pub fn ascii_hex_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2 + 1);
    for b in data {
        out.extend_from_slice(format!("{b:02X}").as_bytes());
    }
    out.push(b'>');
    out
}

pub fn ascii85_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        let value = u32::from_be_bytes(word);
        if value == 0 && chunk.len() == 4 {
            out.push(b'z');
            continue;
        }
        let mut digits = [0u8; 5];
        let mut rest = value;
        for slot in digits.iter_mut().rev() {
            *slot = (rest % 85) as u8 + b'!';
            rest /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

pub fn run_length_encode(data: &[u8]) -> Vec<u8> {
    // Bewusst simpel: nur Literalblöcke, kein Zusammenfassen von Wiederholungen.
    let mut out = Vec::new();
    for chunk in data.chunks(128) {
        out.push(chunk.len() as u8 - 1);
        out.extend_from_slice(chunk);
    }
    out.push(128);
    out
}

/// LZW ohne Wörterbuchnutzung: Clear-Code, jedes Byte als eigener Code, EOD.
/// Für kurze Texte bleibt die Codebreite bei 9 Bit.
pub fn lzw_encode(data: &[u8]) -> Vec<u8> {
    assert!(data.len() < 200, "sonst wächst die Codebreite");
    let mut bits = BitWriter::default();
    bits.push(256, 9);
    for &b in data {
        bits.push(u16::from(b), 9);
    }
    bits.push(257, 9);
    bits.finish()
}

#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    used: u32,
}

impl BitWriter {
    fn push(&mut self, code: u16, width: u32) {
        self.acc = (self.acc << width) | u32::from(code);
        self.used += width;
        while self.used >= 8 {
            self.used -= 8;
            self.out.push((self.acc >> self.used) as u8);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.out.push((self.acc << (8 - self.used)) as u8);
        }
        self.out
    }
}
