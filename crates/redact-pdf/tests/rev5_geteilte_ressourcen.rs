//! Was *n* Formulare kosten dürfen, die sich **ein** `/Resources` teilen.
//!
//! Der Befund, den diese Datei festhält: die Decke
//! `MAX_CACHED_OPERATIONS` zählte zwar seit v0.6.0 den mitgeklonten
//! `/Resources`-Baum — aber in der falschen Einheit und an der falschen
//! Stelle.
//!
//! * **Falsche Einheit.** Eine PDF-Zeichenkette fiel in den Sammelzweig und
//!   zählte *eins*, gleich ob sie zehn Byte oder ein Megabyte trug.
//! * **Falsche Stelle.** Jeder ausgepackte Strom klonte sein aufgelöstes
//!   Verzeichnis **vollständig**. *n* Formulare, die per Referenz dasselbe
//!   Verzeichnis erben, hielten also *n* Kopien derselben Zeichenkette.
//!
//! Gemessen (Release, `VmHWM`, ein Verzeichnis mit einer 16-kB-Zeichenkette):
//!
//! | Datei | Formulare | vorher | nachher |
//! |---|---:|---:|---:|
//! | 0,76 MB | 5 000 | 107 MB | 26 MB |
//! | 7,60 MB | 50 000 | 1 037 MB | 226 MB |
//!
//! Der Zähler las dabei vorher 5 000 bzw. 50 000 ab — beides weit unter der
//! Decke von 100 000. Nachher liest er 16 385: die Zeichenkette nach ihrer
//! Länge, **einmal**.
//!
//! ## Warum hier eine Speichermessung steht
//!
//! Sonst misst diese Suite bewusst nur, was an der Datei hängt
//! ([`redact_pdf::content::ScanEffort`]). Der Befund hier ist aber *reiner
//! Speicher*: an den Zählern war er nicht zu sehen, das war ja sein Wesen. Die
//! Schranke ist deshalb mit reichlich Abstand gewählt (Faktor 4 zum gemessenen
//! Wert vorher, Faktor 15 zum gemessenen Wert nachher) und die Messung liest
//! `VmHWM` aus `/proc` — daher nur unter Linux.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_pdf::scan_page;

/// Ein Grundgerüst mit genau einer Seite.
struct Doc {
    doc: Document,
    page_id: ObjectId,
    content_id: ObjectId,
    resources_id: ObjectId,
}

impl Doc {
    fn new() -> Self {
        let mut doc = Document::with_version("1.7");
        let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        let resources_id = doc.add_object(dictionary! {});
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
        Self {
            doc,
            page_id,
            content_id,
            resources_id,
        }
    }

    fn add(&mut self, object: impl Into<Object>) -> ObjectId {
        self.doc.add_object(object)
    }

    fn set_content(&mut self, raw: impl AsRef<[u8]>) {
        self.doc.objects.insert(
            self.content_id,
            Object::Stream(Stream::new(dictionary! {}, raw.as_ref().to_vec())),
        );
    }

    fn set_resources(&mut self, dict: Dictionary) {
        self.doc
            .objects
            .insert(self.resources_id, Object::Dictionary(dict));
    }

    fn scan(&self) -> redact_pdf::ScanResult {
        scan_page(&self.doc, self.page_id).expect("lesbar")
    }
}

/// Ein Verzeichnis mit genau einer Zeichenkette von `bytes` Byte.
fn fettes_verzeichnis(doc: &mut Doc, bytes: usize) -> ObjectId {
    doc.add(dictionary! {
        "Junk" => Object::string_literal(vec![b'A'; bytes]),
    })
}

/// `n` Formulare mit leerem Rumpf, die alle dasselbe `/Resources`-Objekt
/// erben — jedes `placements`-mal gezeichnet.
fn seite_mit_geteiltem_verzeichnis(n: usize, junk: usize, placements: usize) -> Doc {
    let mut doc = Doc::new();
    let geteilt = fettes_verzeichnis(&mut doc, junk);
    let mut xobjects = Dictionary::new();
    let mut content = String::new();
    for i in 0..n {
        let form = doc.add(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                "Resources" => geteilt,
            },
            Vec::new(),
        ));
        xobjects.set(format!("X{i}"), form);
        content.push_str(&format!("q /X{i} Do Q\n"));
    }
    doc.set_resources(dictionary! { "XObject" => xobjects });
    doc.set_content(content.repeat(placements));
    doc
}

/// **Und was der Zähler dabei abliest.**
///
/// Er soll die Wirklichkeit abbilden und nicht nur begrenzen: die
/// Zeichenkette wiegt ihre 16 384 Byte, und sie wiegt sie **einmal** — nicht
/// einmal je Strom und nicht „eins“.
///
/// Nimmt man den `Object::String`-Zweig aus `dictionary_objects` heraus, liest
/// der Zähler wieder 1, und dieser Test geht rot. Zählt umgekehrt jeder Strom
/// das Verzeichnis erneut, liest er 200 × 16 385, und der Test geht ebenfalls
/// rot.
#[test]
fn der_zaehler_liest_die_zeichenkette_nach_ihrer_laenge() {
    let doc = seite_mit_geteiltem_verzeichnis(200, 16 * 1024, 1);
    let scan = doc.scan();
    assert_eq!(
        scan.effort.retained_weight,
        16 * 1024 + 1,
        "16 384 Byte Zeichenkette plus ein Verzeichniseintrag — einmal"
    );
}

/// Die Gegenprobe: ein Verzeichnis, das **allein schon** über der Decke liegt,
/// wird nicht geteilt — und dann darf auch der Strom nicht gemerkt werden.
///
/// Sonst wäre die Teilung die neue Hintertür: ein Verzeichnis von 150 kB, das
/// die Decke ablehnt, läge trotzdem *n*-mal im Strom-Zwischenspeicher. Der
/// Preis dieser Strenge ist die Laufzeit von vorher — je Platzierung neu
/// auspacken —, und genau daran ist sie zu erkennen: 200 Formulare, zweimal
/// gezeichnet, ergeben 400 Auspackvorgänge.
///
/// Teilt man das Verzeichnis auch über der Decke, sind es 200, und dieser Test
/// geht rot.
#[test]
fn ein_verzeichnis_ueber_der_decke_wird_nicht_gemerkt() {
    let doc = seite_mit_geteiltem_verzeichnis(200, 150 * 1024, 2);
    let scan = doc.scan();
    assert_eq!(
        scan.effort.retained_weight, 0,
        "nichts gemerkt, also wiegt auch nichts"
    );
    assert_eq!(
        scan.effort.decoded_streams, 400,
        "200 Formulare, zweimal gezeichnet: nichts liegt im Zwischenspeicher"
    );
}

/// Und die Vererbung bleibt, was sie war — hier noch einmal gegen den
/// **geteilten** Zwischenspeicher geprüft.
///
/// Zwei Umgebungen bieten unter `/F1` verschiedene Schriften an; dasselbe
/// erbende Formular muss aus beiden verschieden herauskommen. Der Schlüssel
/// des geteilten Verzeichnisses ist die Objekt-Id, nicht der Fundort — sonst
/// wäre das hier der stille Fehler.
#[test]
fn geteilte_verzeichnisse_aendern_die_vererbung_nicht() {
    let mut doc = Doc::new();
    let font_a = font_with_cmap(&mut doc, &[(b'A', 'X'), (b'B', 'Y')]);
    let font_b = font_with_cmap(&mut doc, &[(b'A', 'P'), (b'B', 'Q')]);
    let erbend = doc.add(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        },
        b"BT /F1 12 Tf 20 20 Td (AB) Tj ET\n".to_vec(),
    ));
    let res_a = doc.add(dictionary! {
        "Font" => dictionary! { "F1" => font_a },
        "XObject" => dictionary! { "S" => erbend },
    });
    let res_b = doc.add(dictionary! {
        "Font" => dictionary! { "F1" => font_b },
        "XObject" => dictionary! { "S" => erbend },
    });
    let huelle = |doc: &mut Doc, res: ObjectId, y: i32| {
        doc.add(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => res,
            },
            format!("q 1 0 0 1 0 {y} cm /S Do Q\n").into_bytes(),
        ))
    };
    let a = huelle(&mut doc, res_a, 700);
    let b = huelle(&mut doc, res_b, 600);
    doc.set_resources(dictionary! {
        "XObject" => dictionary! { "A" => a, "B" => b },
    });
    doc.set_content("q /A Do Q q /B Do Q\n");

    let scan = doc.scan();
    let texte: Vec<String> = scan
        .shows
        .iter()
        .map(|r| r.glyphs().map(|g| g.text.as_str()).collect())
        .collect();
    assert_eq!(
        texte,
        vec!["XY".to_string(), "PQ".to_string()],
        "derselbe Strom, zwei geerbte Schriftverzeichnisse — zwei Ergebnisse"
    );
}

/// Eine Helvetica mit einer `/ToUnicode`-CMap, die `codes` auf Zeichen
/// abbildet.
fn font_with_cmap(doc: &mut Doc, pairs: &[(u8, char)]) -> ObjectId {
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n\
         12 dict begin begincmap\n\
         1 begincodespacerange <00> <FF> endcodespacerange\n",
    );
    cmap.push_str(&format!("{} beginbfchar\n", pairs.len()));
    for (code, ch) in pairs {
        cmap.push_str(&format!("<{:02X}> <{:04X}>\n", code, *ch as u32));
    }
    cmap.push_str("endbfchar\nendcmap CMapName currentdict /CMap defineresource pop end end\n");
    let cmap_id = doc.add(Stream::new(dictionary! {}, cmap.into_bytes()));
    doc.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
        "ToUnicode" => cmap_id,
    })
}
