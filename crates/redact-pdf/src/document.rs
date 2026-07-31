//! Laden, Prüfen und Speichern von PDF-Dateien.
//!
//! Bewusst streng: verschlüsselte oder strukturell kaputte Dateien werden
//! abgelehnt statt repariert. Eine „reparierte“ Datei könnte Inhalte enthalten,
//! die der Analyse entgehen — und damit ungeschwärzt durchrutschen.

use std::collections::BTreeSet;
use std::path::Path;

use lopdf::{Document, Object, ObjectId};
use redact_core::{Rect, RedactError, Renderer, Result};

/// Lädt ein PDF von der Platte und prüft es.
pub fn load(path: &Path) -> Result<Document> {
    let bytes = std::fs::read(path)?;
    load_from_bytes(&bytes).map_err(|e| match e {
        RedactError::Pdf(msg) => RedactError::Pdf(format!("{}: {msg}", path.display())),
        other => other,
    })
}

/// Lädt ein PDF aus dem Speicher (es werden keine temporären Dateien angelegt).
pub fn load_from_bytes(bytes: &[u8]) -> Result<Document> {
    if !bytes.starts_with(b"%PDF-") {
        // Manche Dateien haben ein paar Bytes Vorspann — das ist zulässig,
        // aber der Header muss in den ersten 1024 Bytes auftauchen.
        let head = &bytes[..bytes.len().min(1024)];
        if !head.windows(5).any(|w| w == b"%PDF-") {
            return Err(RedactError::Pdf(
                "keine PDF-Datei (Header %PDF- fehlt)".into(),
            ));
        }
    }

    let doc = Document::load_mem(bytes)
        .map_err(|e| RedactError::Pdf(format!("Datei nicht lesbar: {e}")))?;

    validate(&doc)?;
    Ok(doc)
}

/// Strukturelle Mindestanforderungen.
pub fn validate(doc: &Document) -> Result<()> {
    if doc.is_encrypted() {
        return Err(RedactError::Pdf(
            "Dokument ist verschlüsselt. Verschlüsselte PDFs werden nicht verarbeitet — \
             bitte vorher entschlüsseln."
                .into(),
        ));
    }
    if doc.catalog().is_err() {
        return Err(RedactError::Pdf(
            "Katalog (/Root) fehlt oder ist defekt".into(),
        ));
    }
    if doc.get_pages().is_empty() {
        return Err(RedactError::Pdf("Dokument enthält keine Seiten".into()));
    }
    Ok(())
}

/// Anzahl der Seiten.
pub fn page_count(doc: &Document) -> usize {
    doc.get_pages().len()
}

/// MediaBox jeder Seite (0-basiert), inklusive Vererbung vom Seitenbaum.
pub fn page_boxes(doc: &Document) -> Vec<Rect> {
    doc.get_pages()
        .values()
        .map(|id| page_box(doc, *id))
        .collect()
}

/// MediaBox einer Seite; Standard ist A4, falls nichts angegeben ist.
pub fn page_box(doc: &Document, page_id: lopdf::ObjectId) -> Rect {
    const A4: Rect = Rect {
        ll: redact_core::Point { x: 0.0, y: 0.0 },
        ur: redact_core::Point {
            x: 595.276,
            y: 841.89,
        },
    };

    // /MediaBox kann von /Pages geerbt werden.
    let mut current = Some(page_id);
    let mut depth = 0;
    while let Some(id) = current {
        if depth > 32 {
            break;
        }
        depth += 1;
        let Ok(dict) = doc.get_dictionary(id) else {
            break;
        };
        if let Some(rect) = dict
            .get(b"MediaBox")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| rect_from(o))
        {
            return rect;
        }
        current = match dict.get(b"Parent") {
            Ok(Object::Reference(parent)) => Some(*parent),
            _ => None,
        };
    }
    A4
}

fn rect_from(obj: &Object) -> Option<Rect> {
    let array = obj.as_array().ok()?;
    let v: Vec<f64> = array
        .iter()
        .take(4)
        .filter_map(|o| match o {
            Object::Integer(i) => Some(*i as f64),
            Object::Real(r) => Some(*r as f64),
            _ => None,
        })
        .collect();
    if v.len() < 4 {
        return None;
    }
    Some(Rect::new(v[0], v[1], v[2], v[3]))
}

// ---------------------------------------------------------------------------
// Erreichbarkeit
// ---------------------------------------------------------------------------

/// Maximale Verschachtelungstiefe direkter Objekte (Arrays in Arrays in …).
/// Referenzen zählen nicht mit, die laufen über die Arbeitsliste.
const MAX_DIRECT_DEPTH: usize = 64;

/// Meldet, ob das Dokument aus mehreren inkrementellen Revisionen besteht.
///
/// `lopdf` behält den Trailer der jüngsten Revision; ein `/Prev` darin ist der
/// Zeiger auf die vorige XRef-Sektion — also der Beleg für eine Vorgeschichte.
pub fn has_incremental_history(doc: &Document) -> bool {
    doc.trailer.get(b"Prev").is_ok() || doc.trailer.get(b"XRefStm").is_ok()
}

/// Sammelt alle vom Trailer aus erreichbaren Objekte.
fn reachable_objects(doc: &Document) -> BTreeSet<ObjectId> {
    let mut seen: BTreeSet<ObjectId> = BTreeSet::new();
    let mut queue: Vec<ObjectId> = Vec::new();

    // Wurzeln: alles, was der Trailer referenziert — /Root, /Info, /Encrypt.
    // Der Trailer wird generisch abgelaufen, damit kein Schlüssel vergessen
    // wird, den eine künftige PDF-Version einführt.
    collect_references(
        &Object::Dictionary(doc.trailer.clone()),
        0,
        &mut seen,
        &mut queue,
    );

    // Breitensuche durch den Objektgraphen. `seen` verhindert Zyklen.
    while let Some(id) = queue.pop() {
        let Some(object) = doc.objects.get(&id) else {
            continue;
        };
        collect_references(object, 0, &mut seen, &mut queue);
    }
    seen
}

/// Trägt alle Referenzen eines Objekts in die Arbeitsliste ein.
///
/// Rekursiv durch Dictionaries, Arrays und Stream-Dictionaries — dort steckt
/// unter anderem ein `/Length`, das als indirektes Objekt vorliegen darf.
fn collect_references(
    object: &Object,
    depth: usize,
    seen: &mut BTreeSet<ObjectId>,
    queue: &mut Vec<ObjectId>,
) {
    if depth > MAX_DIRECT_DEPTH {
        return;
    }
    match object {
        Object::Reference(id) => {
            if seen.insert(*id) {
                queue.push(*id);
            }
        }
        Object::Array(items) => {
            for item in items {
                collect_references(item, depth + 1, seen, queue);
            }
        }
        Object::Dictionary(dict) => {
            for (_, value) in dict.iter() {
                collect_references(value, depth + 1, seen, queue);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter() {
                collect_references(value, depth + 1, seen, queue);
            }
        }
        _ => {}
    }
}

/// Entfernt alle Objekte, die vom Trailer aus nicht mehr erreichbar sind.
///
/// `lopdf::Document::save_to` schreibt **alles**, was in `doc.objects` steht —
/// Erreichbarkeit interessiert den Writer nicht. Wer nur eine Referenz löscht
/// (Annotation ohne ihren `/AP`-Stream, `/StructTreeRoot` ohne den `/K`-Baum
/// darunter, `/Metadata` einer Seite), lässt das Objekt selbst stehen, und es
/// landet unkomprimiert und gut lesbar in der Ausgabe. Dasselbe gilt für
/// Objekte, die eine ältere Revision einer `/Prev`-Kette beigesteuert hat und
/// die niemand mehr referenziert.
///
/// Ein einziger Erreichbarkeitslauf vor dem Speichern erschlägt alle diese
/// Fälle. Rückgabe: Anzahl entfernter Objekte.
pub fn prune_unreachable(doc: &mut Document) -> usize {
    let reachable = reachable_objects(doc);
    let before = doc.objects.len();
    doc.objects.retain(|id, _| reachable.contains(id));
    before - doc.objects.len()
}

/// Serialisiert das Dokument in den Speicher.
///
/// Vor dem Schreiben wird aufgeräumt: unerreichbare Objekte fliegen raus und
/// der Trailer verliert die Zeiger auf ältere Revisionen (`/Prev`,
/// `/XRefStm`). Die Ausgabe ist genau eine Revision — ohne Vorgeschichte und
/// ohne Karteileichen. Das Dokument des Aufrufers bleibt unverändert.
pub fn save_to_bytes(doc: &Document) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut copy = doc.clone();
    prune_unreachable(&mut copy);
    // Die Ausgabe ist eine vollständige, in sich geschlossene Datei. Ein
    // geerbtes `/Prev` zeigt in ihr auf einen völlig anderen Offset — im
    // Zweifel mitten in einen Content-Stream.
    copy.trailer.remove(b"Prev");
    copy.trailer.remove(b"XRefStm");
    copy.save_to(&mut buffer)
        .map_err(|e| RedactError::Pdf(format!("Speichern fehlgeschlagen: {e}")))?;
    Ok(buffer)
}

/// Schreibt das Dokument als neue Datei.
#[derive(Debug, Clone, Copy, Default)]
pub struct PdfRenderer;

impl PdfRenderer {
    pub fn new() -> Self {
        Self
    }
}

impl Renderer for PdfRenderer {
    fn render(&self, doc: &Document, path: &Path) -> Result<()> {
        let bytes = save_to_bytes(doc)?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn rejects_non_pdf() {
        let err = load_from_bytes(b"hello world").unwrap_err();
        assert!(matches!(err, RedactError::Pdf(_)));
        assert!(err.to_string().contains("%PDF-"));
    }

    #[test]
    fn rejects_truncated_pdf() {
        assert!(load_from_bytes(b"%PDF-1.7\nnur muell").is_err());
    }

    #[test]
    fn loads_minimal_document() {
        let bytes = crate::testing::minimal_pdf("Hallo Welt");
        let doc = load_from_bytes(&bytes).unwrap();
        assert_eq!(page_count(&doc), 1);
        assert_eq!(page_boxes(&doc)[0], Rect::new(0.0, 0.0, 595.0, 842.0));
    }

    #[test]
    fn saving_is_deterministic() {
        let bytes = crate::testing::minimal_pdf("Hallo Welt");
        let doc = load_from_bytes(&bytes).unwrap();
        assert_eq!(save_to_bytes(&doc).unwrap(), save_to_bytes(&doc).unwrap());
    }

    // -----------------------------------------------------------------------
    // Erreichbarkeitslauf
    // -----------------------------------------------------------------------

    const SECRET: &str = "DE89 3704 0044 0532 0130 00";

    /// Objekte, die nur die *Dateistruktur* der Eingabe beschreiben: XRef-Strom,
    /// Objekt-Stream-Container, Linearisierungs-Dictionary. Sie hängen an der
    /// XRef-Tabelle, nicht am Objektgraphen, und `lopdf` schreibt sie beim
    /// Speichern ohnehin nicht mit. Dass der Erreichbarkeitslauf sie entfernt,
    /// kostet also nichts.
    fn is_file_structure(object: &Object) -> bool {
        matches!(
            object.type_name().ok(),
            Some("XRef") | Some("ObjStm") | Some("Linearized")
        )
    }

    /// Was der Erreichbarkeitslauf entfernt hat — ohne die reinen
    /// Dateistruktur-Objekte.
    fn pruned_payload(doc: &Document) -> Vec<ObjectId> {
        let mut copy = doc.clone();
        prune_unreachable(&mut copy);
        doc.objects
            .iter()
            .filter(|(id, object)| !copy.objects.contains_key(id) && !is_file_structure(object))
            .map(|(id, _)| *id)
            .collect()
    }

    fn extracted_text(bytes: &[u8]) -> Vec<String> {
        use redact_core::Extractor;
        let doc = load_from_bytes(bytes).expect("ladbar");
        crate::PdfExtractor::new()
            .extract(&doc)
            .expect("Extraktion")
            .into_iter()
            .map(|run| run.text)
            .collect()
    }

    #[test]
    fn a_normal_document_loses_nothing() {
        let bytes = crate::testing::demo_statement();
        let doc = load_from_bytes(&bytes).unwrap();
        let before_pages = page_count(&doc);
        let before_boxes = page_boxes(&doc);
        let before_text = extracted_text(&bytes);

        assert_eq!(
            pruned_payload(&doc),
            Vec::<ObjectId>::new(),
            "der Erreichbarkeitslauf hat an einem sauberen Dokument Inhalt entfernt"
        );

        let after = save_to_bytes(&doc).unwrap();
        let reloaded = load_from_bytes(&after).expect("Ausgabe wieder ladbar");
        assert_eq!(page_count(&reloaded), before_pages);
        assert_eq!(page_boxes(&reloaded), before_boxes);
        assert_eq!(extracted_text(&after), before_text);
    }

    #[test]
    fn an_orphaned_object_does_not_reach_the_output() {
        let bytes = crate::testing::minimal_pdf("Kontoinhaber: Max Mustermann");
        let mut doc = load_from_bytes(&bytes).unwrap();
        doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Vergessen",
            "ActualText" => Object::string_literal(SECRET),
        }));

        // Ohne Aufräumen schreibt `lopdf` das Objekt wortwörtlich mit.
        let mut naive = Vec::new();
        doc.clone().save_to(&mut naive).unwrap();
        assert!(!crate::leaks(&naive, SECRET).is_empty());

        let cleaned = save_to_bytes(&doc).unwrap();
        assert!(
            crate::leaks(&cleaned, SECRET).is_empty(),
            "verwaistes Objekt überlebt: {:?}",
            crate::leaks(&cleaned, SECRET)
        );
        assert_eq!(page_count(&load_from_bytes(&cleaned).unwrap()), 1);
    }

    #[test]
    fn a_cycle_does_not_hang_the_traversal() {
        let bytes = crate::testing::minimal_pdf("Hallo Welt");
        let mut doc = load_from_bytes(&bytes).unwrap();
        let a = doc.new_object_id();
        let b = doc.new_object_id();
        doc.objects.insert(
            a,
            Object::Dictionary(dictionary! { "Next" => Object::Reference(b) }),
        );
        doc.objects.insert(
            b,
            Object::Dictionary(dictionary! { "Next" => Object::Reference(a) }),
        );
        // Vom Katalog aus erreichbar machen, damit der Zyklus wirklich betreten wird.
        let catalog_id = match doc.trailer.get(b"Root").unwrap() {
            Object::Reference(id) => *id,
            _ => unreachable!(),
        };
        doc.get_dictionary_mut(catalog_id)
            .unwrap()
            .set("Ring", Object::Reference(a));

        let mut copy = doc.clone();
        prune_unreachable(&mut copy);
        assert!(copy.objects.contains_key(&a) && copy.objects.contains_key(&b));
    }

    #[test]
    fn an_indirect_stream_length_keeps_its_object() {
        let bytes = crate::testing::minimal_pdf("Hallo Welt");
        let mut doc = load_from_bytes(&bytes).unwrap();
        let page_id = *doc.get_pages().values().next().unwrap();
        let content_id = doc.get_page_contents(page_id)[0];
        let length = doc
            .get_object(content_id)
            .unwrap()
            .as_stream()
            .unwrap()
            .content
            .len() as i64;
        let length_id = doc.add_object(Object::Integer(length));
        if let Ok(Object::Stream(stream)) = doc.get_object_mut(content_id) {
            stream.dict.set("Length", Object::Reference(length_id));
        }

        let mut copy = doc.clone();
        prune_unreachable(&mut copy);
        assert!(
            copy.objects.contains_key(&length_id),
            "/Length als indirektes Objekt wurde weggeräumt"
        );
    }

    // -----------------------------------------------------------------------
    // Inkrementelle Vorversionen
    // -----------------------------------------------------------------------

    /// Ein PDF mit `/Prev`-Kette: die Basisrevision trägt `secret` in einem
    /// Content-Stream, die angehängte Revision hängt die Seite auf einen
    /// **neuen** Stream um. Genau das tun Werkzeuge, die „geschwärzt“ per
    /// inkrementellem Update speichern — der alte Strom bleibt in der Datei.
    fn incremental_with_orphaned_content(secret: &str, replacement: &str) -> Vec<u8> {
        use lopdf::Stream;

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
        let content = format!("BT\n/F1 10 Tf\n72 700 Td\n(IBAN: {secret}) Tj\nET\n");
        let content_id = doc
            .add_object(Stream::new(dictionary! {}, content.into_bytes()).with_compression(false));
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

        let mut base = Vec::new();
        doc.clone().save_to(&mut base).unwrap();

        // +1 vergibt `save_to` bereits für den XRef-Strom.
        let new_content_id = doc.max_id + 2;
        let new_content = format!("BT\n/F1 10 Tf\n72 700 Td\n(IBAN: {replacement}) Tj\nET\n");
        let mut stream_body = format!("<</Length {}>>\nstream\n", new_content.len()).into_bytes();
        stream_body.extend_from_slice(new_content.as_bytes());
        stream_body.extend_from_slice(b"\nendstream");

        let page_body = format!(
            "<</Type/Page/Parent {} 0 R/Contents {new_content_id} 0 R/Resources {} 0 R\
             /MediaBox[0 0 595 842]>>",
            pages_id.0, resources_id.0
        )
        .into_bytes();

        append_revision(
            base,
            catalog_id,
            new_content_id + 1,
            &[(new_content_id, stream_body), (page_id.0, page_body)],
        )
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
        let digits: String = bytes[pos + key.len()..]
            .iter()
            .skip_while(|b| b.is_ascii_whitespace())
            .take_while(|b| b.is_ascii_digit())
            .map(|&b| b as char)
            .collect();
        digits.parse().ok()
    }

    #[test]
    fn the_base_revision_of_a_prev_chain_does_not_survive() {
        let bytes = incremental_with_orphaned_content(SECRET, "XXXX XXXX XXXX");
        assert!(
            !crate::leaks(&bytes, SECRET).is_empty(),
            "Testdaten taugen nicht: die Historie enthält das Geheimnis gar nicht"
        );
        let doc = load_from_bytes(&bytes).expect("ladbar");
        assert!(has_incremental_history(&doc));

        // Ohne Aufräumen wandert der verwaiste Basis-Stream in die Ausgabe.
        let mut naive = Vec::new();
        doc.clone().save_to(&mut naive).unwrap();
        assert!(
            !crate::leaks(&naive, SECRET).is_empty(),
            "Vorbedingung: ohne Erreichbarkeitslauf leckt die Datei"
        );

        let cleaned = save_to_bytes(&doc).unwrap();
        assert!(
            crate::leaks(&cleaned, SECRET).is_empty(),
            "Basisrevision überlebt: {:?}",
            crate::leaks(&cleaned, SECRET)
        );
        assert_eq!(extracted_text(&cleaned), vec!["IBAN: XXXX XXXX XXXX"]);
    }

    #[test]
    fn the_output_trailer_has_no_stale_prev() {
        let bytes = incremental_with_orphaned_content(SECRET, "XXXX XXXX XXXX");
        let doc = load_from_bytes(&bytes).expect("ladbar");
        let cleaned = save_to_bytes(&doc).unwrap();

        let reloaded = load_from_bytes(&cleaned).expect("Ausgabe ladbar");
        assert!(!has_incremental_history(&reloaded));
        assert!(
            !String::from_utf8_lossy(&cleaned).contains("/Prev"),
            "die Ausgabe trägt weiterhin ein /Prev"
        );
    }

    #[test]
    fn pruning_stays_deterministic() {
        let bytes = incremental_with_orphaned_content(SECRET, "XXXX XXXX XXXX");
        let doc = load_from_bytes(&bytes).unwrap();
        assert_eq!(save_to_bytes(&doc).unwrap(), save_to_bytes(&doc).unwrap());
    }
}
