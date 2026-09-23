//! Gegenrichtung zu Q1-1 und Q1-2: ein Korpus **gewöhnlicher** Dateien.
//!
//! Die Korrekturen der Fix-Runde 6 greifen an Stellen, an denen jede
//! alltägliche PDF-Datei etwas stehen hat: `/Alt` ist die Standardform der
//! Barrierefreiheit, `/Movie`, `/Measure` und `/RichMediaContent` sind
//! Beiwerk an Annotationen, und die Zähler laufen über den ganzen
//! Objektgraphen. „Eine Grenze, die gewöhnliche Dateien ablehnt, ist genauso
//! ein Fehler wie eine Lücke“ — also acht Dateien, wie sie ein Erzeuger
//! schreibt:
//!
//! mehrseitig mit Lesezeichen und Links, Bild, gedreht mit Annotationen,
//! Ebenen (OCG), Anhang, Objektströme, ein echtes AcroForm-Formular und eine
//! getaggte Datei mit `/Alt` und `/ActualText`.
//!
//! Geprüft wird: der Lauf gelingt, die Ausgabe ist ladbar, sie trägt
//! **denselben Text** und dieselbe Seitenzahl, und `--check-leaks` findet
//! nichts (rc 0). Der Vergleich der *Bilder* (bytegleich gerendert) läuft
//! über `korpus_schreiben` — siehe dort.

mod common;

use common::{page, text_ops, Doc};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_pdf::{leaks_many, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport};

// ---------------------------------------------------------------------------
// Der Korpus
// ---------------------------------------------------------------------------

/// Acht gewöhnliche Dateien, je (Name, Bytes).
pub fn korpus() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("mehrseitig_lesezeichen_links", mehrseitig()),
        ("bild", mit_bild()),
        ("gedreht_annotationen", gedreht_mit_annotationen()),
        ("ebenen_ocg", mit_ebenen()),
        ("anhang", mit_anhang()),
        ("objektstroeme", mit_objektstrom()),
        ("acroform_formular", echtes_formular()),
        ("getaggt", getaggt()),
    ]
}

/// Zwei Seiten, ein Lesezeichenbaum mit ausdrücklichen Zielen und ein Link
/// von Seite 1 auf Seite 2.
fn mehrseitig() -> Vec<u8> {
    let mut d = page(&["Rechnung Nr. 4711", "Seite 1 von 2"]);
    let zweite_inhalt = d.add(Object::Stream(Stream::new(
        dictionary! {},
        text_ops(&["Anlage zur Rechnung", "Seite 2 von 2"]),
    )));
    let zweite = d.add(Object::Dictionary(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(d.pages_id),
        "Contents" => Object::Reference(zweite_inhalt),
        "Resources" => Object::Reference(d.resources_id),
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    }));
    let (erste, pages_id) = (d.page_id, d.pages_id);
    d.doc.get_dictionary_mut(pages_id).expect("Seitenbaum").set(
        "Kids",
        Object::Array(vec![Object::Reference(erste), Object::Reference(zweite)]),
    );
    d.doc
        .get_dictionary_mut(pages_id)
        .expect("Seitenbaum")
        .set("Count", 2_i64);

    // Lesezeichen mit ausdrücklichem Ziel.
    let wurzel = d.doc.new_object_id();
    let kapitel1 = d.doc.new_object_id();
    let kapitel2 = d.doc.new_object_id();
    d.doc.objects.insert(
        kapitel1,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("Rechnung"),
            "Parent" => Object::Reference(wurzel),
            "Next" => Object::Reference(kapitel2),
            "Dest" => Object::Array(vec![Object::Reference(erste), "Fit".into()]),
        }),
    );
    d.doc.objects.insert(
        kapitel2,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("Anlage"),
            "Parent" => Object::Reference(wurzel),
            "Prev" => Object::Reference(kapitel1),
            "Dest" => Object::Array(vec![Object::Reference(zweite), "Fit".into()]),
        }),
    );
    d.doc.objects.insert(
        wurzel,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => Object::Reference(kapitel1),
            "Last" => Object::Reference(kapitel2),
            "Count" => 2_i64,
        }),
    );
    d.catalog_set("Outlines", Object::Reference(wurzel));

    // Ein Link von Seite 1 auf Seite 2.
    let link = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![72.into(), 690.into(), 300.into(), 715.into()],
        "Border" => vec![0.into(), 0.into(), 0.into()],
        "Dest" => Object::Array(vec![Object::Reference(zweite), "Fit".into()]),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(link)]));
    d.finish()
}

/// Ein Bild-XObject, das die Seite auch wirklich zeichnet.
fn mit_bild() -> Vec<u8> {
    let mut d = page(&["Ausweiskopie", "Anlage 1"]);
    // 2×2 RGB, unkomprimiert.
    let pixel: Vec<u8> = vec![
        0xf0, 0x20, 0x20, 0x20, 0xf0, 0x20, 0x20, 0x20, 0xf0, 0xf0, 0xf0, 0x20,
    ];
    let bild = d.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 2_i64,
            "Height" => 2_i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
        },
        pixel,
    )));
    let resources_id = d.resources_id;
    d.doc
        .get_dictionary_mut(resources_id)
        .expect("Ressourcen")
        .set(
            "XObject",
            Object::Dictionary(dictionary! { "Im0" => Object::Reference(bild) }),
        );
    let mut inhalt = text_ops(&["Ausweiskopie", "Anlage 1"]);
    inhalt.extend_from_slice(b"q\n200 0 0 120 72 520 cm\n/Im0 Do\nQ\n");
    d.set_content(&inhalt);
    d.finish()
}

/// Gedrehte Seite (`/Rotate 90`) mit einer Notiz samt Erscheinungsstrom.
fn gedreht_mit_annotationen() -> Vec<u8> {
    let mut d = page(&["Querformat", "Anlage 2"]);
    d.page_dict_set("Rotate", Object::Integer(90));
    let ap = d.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 20.into(), 20.into()],
        },
        b"0 0 1 rg\n0 0 20 20 re\nf\n".to_vec(),
    )));
    let notiz = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![400.into(), 700.into(), 420.into(), 720.into()],
        "Contents" => Object::string_literal("Bitte gegenzeichnen."),
        "T" => Object::string_literal("Sachbearbeitung"),
        "AP" => dictionary! { "N" => Object::Reference(ap) },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(notiz)]));
    d.finish()
}

/// Eine sichtbare Ebene (`/OCG`) samt `/OCProperties` und `/BDC /OC`.
fn mit_ebenen() -> Vec<u8> {
    let mut d = page(&["Grundriss", "Bemaßung"]);
    let ocg = d.add(Object::Dictionary(dictionary! {
        "Type" => "OCG",
        "Name" => Object::string_literal("Bemaßung"),
    }));
    let resources_id = d.resources_id;
    d.doc
        .get_dictionary_mut(resources_id)
        .expect("Ressourcen")
        .set(
            "Properties",
            Object::Dictionary(dictionary! { "MC0" => Object::Reference(ocg) }),
        );
    d.catalog_set(
        "OCProperties",
        Object::Dictionary(dictionary! {
            "OCGs" => vec![Object::Reference(ocg)],
            "D" => dictionary! {
                "Order" => vec![Object::Reference(ocg)],
                "ON" => vec![Object::Reference(ocg)],
            },
        }),
    );
    let mut inhalt = b"/OC /MC0 BDC\n".to_vec();
    inhalt.extend_from_slice(&text_ops(&["Grundriss", "Bemaßung"]));
    inhalt.extend_from_slice(b"EMC\n");
    d.set_content(&inhalt);
    d.finish()
}

/// Ein Dateianhang im `/Names`-Baum.
fn mit_anhang() -> Vec<u8> {
    let mut d = page(&["Antrag", "mit Anlage"]);
    let datei = d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "EmbeddedFile", "Subtype" => "text/plain" },
        b"Anlage: Nachweis ueber die Zahlung.\n".to_vec(),
    )));
    let spec = d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("nachweis.txt"),
        "EF" => dictionary! { "F" => Object::Reference(datei) },
    }));
    d.catalog_set(
        "Names",
        Object::Dictionary(dictionary! {
            "EmbeddedFiles" => dictionary! {
                "Names" => vec![
                    Object::string_literal("nachweis.txt"),
                    Object::Reference(spec),
                ],
            },
        }),
    );
    d.finish()
}

/// Katalog und Seitenbaum liegen in einem Objekt-Strom (`/ObjStm`), wie ihn
/// jeder Erzeuger seit PDF 1.5 schreibt.
fn mit_objektstrom() -> Vec<u8> {
    let d = page(&["Kontoauszug", "Januar"]);
    let catalog_id = d.catalog_id;
    let pages_id = d.pages_id;
    let page_id = d.page_id;
    // +1 vergibt `save_to` selbst für den XRef-Strom.
    let objstm_id = d.doc.max_id + 2;
    let base = d.finish();

    let o1 = format!("<</Type/Catalog/Pages {} 0 R>>", pages_id.0);
    let o2 = format!("<</Type/Pages/Kids[{} 0 R]/Count 1>>", page_id.0);
    let header = format!("{} 0 {} {} ", catalog_id.0, pages_id.0, o1.len() + 1);
    let mut plain = header.clone().into_bytes();
    plain.extend_from_slice(o1.as_bytes());
    plain.push(b'\n');
    plain.extend_from_slice(o2.as_bytes());

    let mut body = format!(
        "<</Type/ObjStm/N 2/First {}/Length {}>>\nstream\n",
        header.len(),
        plain.len()
    )
    .into_bytes();
    body.extend_from_slice(&plain);
    body.extend_from_slice(b"\nendstream");

    let fueller = objstm_id + 10;
    append_revision(
        base,
        catalog_id,
        fueller + 1,
        &[
            (objstm_id, body),
            (fueller, b"<</Type/Platzhalter>>".to_vec()),
        ],
    )
}

/// Ein echtes Formular: `/AcroForm` mit `/DA`, `/DR` und einem Widget, das
/// auch in `/Annots` der Seite steht.
fn echtes_formular() -> Vec<u8> {
    let mut d = page(&["Antragsformular", "Bitte ausfüllen"]);
    let font_id = d.font_id;
    let ap = d.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 200.into(), 20.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => Object::Reference(font_id) },
            },
        },
        b"/Tx BMC\nq\nBT\n/F1 10 Tf\n2 5 Td\n(Max Mustermann) Tj\nET\nQ\nEMC\n".to_vec(),
    )));
    let widget = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Tx",
        "Rect" => vec![72.into(), 600.into(), 272.into(), 620.into()],
        "T" => Object::string_literal("name"),
        "TU" => Object::string_literal("Name des Antragstellers"),
        "V" => Object::string_literal("Max Mustermann"),
        "DA" => Object::string_literal("/F1 10 Tf 0 g"),
        "AP" => dictionary! { "N" => Object::Reference(ap) },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(widget)]));
    d.catalog_set(
        "AcroForm",
        Object::Dictionary(dictionary! {
            "Fields" => vec![Object::Reference(widget)],
            "DA" => Object::string_literal("/F1 10 Tf 0 g"),
            "DR" => dictionary! {
                "Font" => dictionary! { "F1" => Object::Reference(font_id) },
            },
            "NeedAppearances" => false,
        }),
    );
    d.finish()
}

/// Eine gewöhnliche getaggte Datei: markierte Abschnitte im Strom, ein
/// `/StructTreeRoot` mit `/Alt` und `/ActualText`, ein Link mit `/StructParent`.
fn getaggt() -> Vec<u8> {
    let mut d = page(&["Jahresbericht 2024", "Abschnitt 1"]);
    let mut inhalt = b"/P <</MCID 0>> BDC\n".to_vec();
    inhalt.extend_from_slice(&text_ops(&["Jahresbericht 2024", "Abschnitt 1"]));
    inhalt.extend_from_slice(b"EMC\n");
    d.set_content(&inhalt);
    d.page_dict_set("StructParents", Object::Integer(0));

    let wurzel = d.doc.new_object_id();
    let absatz = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "P",
        "P" => Object::Reference(wurzel),
        "Pg" => Object::Reference(d.page_id),
        "Alt" => Object::string_literal("Überschrift des Jahresberichts"),
        "ActualText" => Object::string_literal("Jahresbericht 2024"),
        "K" => Object::Integer(0),
    }));
    d.doc.objects.insert(
        wurzel,
        Object::Dictionary(dictionary! {
            "Type" => "StructTreeRoot",
            "K" => vec![Object::Reference(absatz)],
        }),
    );
    d.catalog_set("StructTreeRoot", Object::Reference(wurzel));
    d.catalog_set(
        "MarkInfo",
        Object::Dictionary(dictionary! { "Marked" => true }),
    );
    d.finish()
}

/// Hängt eine weitere Revision an: Objekte, klassische xref-Sektion, `/Prev`.
/// (Wortgleich mit dem Helfer in `common` — der ist dort privat.)
fn append_revision(
    mut out: Vec<u8>,
    catalog_id: ObjectId,
    size: u32,
    objects: &[(u32, Vec<u8>)],
) -> Vec<u8> {
    let key = b"startxref";
    let pos = out
        .windows(key.len())
        .enumerate()
        .rfind(|(_, w)| *w == key)
        .map(|(i, _)| i)
        .expect("startxref in der Basisrevision");
    let prev: usize = out[pos + key.len()..]
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|&b| b as char)
        .collect::<String>()
        .parse()
        .expect("Zahl hinter startxref");
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

// ---------------------------------------------------------------------------
// Die Prüfungen
// ---------------------------------------------------------------------------

fn strip(bytes: &[u8]) -> (MetadataReport, Vec<u8>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = strip_metadata(&mut doc);
    (report, save_to_bytes(&doc).expect("Speichern"))
}

fn text_of(bytes: &[u8]) -> String {
    let doc = load_from_bytes(bytes).expect("ladbar");
    redact_pdf::PdfExtractor::new()
        .extract(&doc)
        .expect("Text lesbar")
        .iter()
        .map(|run| run.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn seitenzahl(bytes: &[u8]) -> usize {
    load_from_bytes(bytes).expect("ladbar").get_pages().len()
}

/// Jede Datei des Korpus übersteht den Metadatenlauf: ladbar, gleiche
/// Seitenzahl, **gleicher Text**.
#[test]
fn gewoehnliche_dateien_verlieren_ihren_text_nicht() {
    for (name, bytes) in korpus() {
        let vorher = text_of(&bytes);
        assert!(
            !vorher.trim().is_empty(),
            "{name}: die Probe trägt keinen Text, dann misst der Test nichts"
        );
        let (_, out) = strip(&bytes);
        assert_eq!(
            seitenzahl(&out),
            seitenzahl(&bytes),
            "{name}: Seitenzahl verändert"
        );
        assert_eq!(text_of(&out), vorher, "{name}: der Text hat sich verändert");
    }
}

/// Der sichtbare Seitentext steht nach dem Lauf noch in den Bytes — mit
/// demselben Messgerät, mit dem die Befunde gemessen werden.
#[test]
fn der_sichtbare_text_bleibt_im_korpus_stehen() {
    for (name, bytes) in korpus() {
        let (_, out) = strip(&bytes);
        let erste_zeile = text_of(&bytes)
            .lines()
            .next()
            .expect("Text")
            .trim()
            .to_string();
        let check = leaks_many(&out, &[erste_zeile.as_str()]);
        assert!(
            !check[0].is_empty(),
            "{name}: „{erste_zeile}“ steht nach dem Metadatenlauf nicht mehr in der Ausgabe"
        );
    }
}

/// Ein `/StructElem`, das **keine Annotation** erreicht, wird vom Trägerlauf
/// nicht angefasst — auch dann nicht, wenn es ein zweiter Halter über das
/// Aufräumen rettet.
///
/// Das ist die Gegenrichtung zu Q1-1d: dort läuft der Lauf über `/IRT`
/// wirklich dorthin. Hier tut er es nicht, und dann bewegt sich auch kein
/// Zähler. Wer `/Alt` stattdessen über alle Objekte hinweg leeren wollte,
/// macht diesen Test rot.
#[test]
fn ein_strukturelement_ohne_annotation_bleibt_unberuehrt() {
    let mut d: Doc = page(&["Jahresbericht 2024"]);
    let elem = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "P",
        "Alt" => Object::string_literal("Überschrift des Jahresberichts"),
    }));
    let wurzel = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructTreeRoot",
        "K" => Object::Reference(elem),
    }));
    d.catalog_set("StructTreeRoot", Object::Reference(wurzel));
    // Ein zweiter Halter, der keine Annotation ist: das Element überlebt das
    // Aufräumen.
    d.zweiter_halter(Object::Reference(elem));
    // Eine gewöhnliche Annotation daneben — der Trägerlauf läuft, er kommt
    // nur nicht am Struktur-Element vorbei.
    let notiz = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "Contents" => Object::string_literal("Bitte prüfen."),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(notiz)]));

    let (report, out) = strip(&d.finish());
    assert_eq!(
        report.annotation_texts_cleared, 1,
        "gezählt gehört genau der /Contents der Annotation"
    );
    let hits = leaks_many(&out, &["Überschrift des Jahresberichts"]);
    assert!(
        !hits[0].is_empty(),
        "das /Alt eines Struktur-Elements ohne Annotation darf der Metadatenlauf \
         nicht anfassen — er erreicht es gar nicht"
    );
}

/// Schreibt den Korpus als Dateipaare (`<name>_vorher.pdf` /
/// `<name>_nachher.pdf`) in das Verzeichnis aus `KORPUS_DIR`.
///
/// Kein Prüftest — das Werkzeug für den Bildvergleich:
///
/// ```console
/// $ KORPUS_DIR=/tmp/korpus cargo test -p redact-pdf --test zf_q1_korpus \
///       -- --ignored korpus_schreiben --nocapture
/// $ cargo run -p redact-render --example page_to_png -- \
///       a_vorher.pdf a_vorher.png a_nachher.pdf a_nachher.png
/// ```
#[test]
#[ignore = "Werkzeug, keine Prüfung"]
fn korpus_schreiben() {
    let dir = std::env::var("KORPUS_DIR").expect("KORPUS_DIR setzen");
    std::fs::create_dir_all(&dir).expect("Verzeichnis");
    for (name, bytes) in korpus() {
        let (report, out) = strip(&bytes);
        std::fs::write(format!("{dir}/{name}_vorher.pdf"), &bytes).expect("schreibbar");
        std::fs::write(format!("{dir}/{name}_nachher.pdf"), &out).expect("schreibbar");
        println!("{name}: {:?}", report.summary());
    }
}
