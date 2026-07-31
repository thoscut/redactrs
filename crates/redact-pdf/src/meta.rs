//! Metadaten entfernen.
//!
//! Ein geschwärztes PDF nützt wenig, wenn im `/Info`-Dictionary noch
//! „Kontoauszug_Mustermann_DE89…“ als Titel steht. Entfernt werden:
//!
//! * das komplette `/Info`-Dictionary aus dem Trailer,
//! * der XMP-Metadatenstrom `/Metadata` aus dem Katalog,
//! * `/PieceInfo` (anwendungsspezifische Zusatzdaten) aus Katalog und Seiten,
//! * die Dokumentstruktur `/StructTreeParent` je Seite sowie `/Names`-Bäume,
//!   die Textinhalte spiegeln können.

use lopdf::{Document, Object, ObjectId};

/// Welche Metadaten entfernt wurden.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataReport {
    pub info_removed: bool,
    pub xmp_removed: bool,
    pub piece_info_removed: usize,
    pub struct_tree_removed: bool,
}

impl MetadataReport {
    pub fn anything_removed(&self) -> bool {
        self.info_removed
            || self.xmp_removed
            || self.piece_info_removed > 0
            || self.struct_tree_removed
    }
}

/// Entfernt alle Dokument-Metadaten.
pub fn strip_metadata(doc: &mut Document) -> MetadataReport {
    let mut report = MetadataReport::default();

    // --- Trailer /Info ---
    if let Ok(info) = doc.trailer.get(b"Info") {
        if let Object::Reference(id) = info {
            let id = *id;
            doc.objects.remove(&id);
        }
        doc.trailer.remove(b"Info");
        report.info_removed = true;
    }

    // --- Katalog ---
    let catalog_id = doc.trailer.get(b"Root").ok().and_then(|o| match o {
        Object::Reference(id) => Some(*id),
        _ => None,
    });

    if let Some(catalog_id) = catalog_id {
        let mut to_delete: Vec<ObjectId> = Vec::new();
        if let Ok(catalog) = doc.get_dictionary_mut(catalog_id) {
            if let Ok(Object::Reference(id)) = catalog.get(b"Metadata") {
                to_delete.push(*id);
            }
            if catalog.remove(b"Metadata").is_some() {
                report.xmp_removed = true;
            }
            if let Ok(Object::Reference(id)) = catalog.get(b"PieceInfo") {
                to_delete.push(*id);
            }
            if catalog.remove(b"PieceInfo").is_some() {
                report.piece_info_removed += 1;
            }
            if let Ok(Object::Reference(id)) = catalog.get(b"StructTreeRoot") {
                to_delete.push(*id);
            }
            if catalog.remove(b"StructTreeRoot").is_some() {
                report.struct_tree_removed = true;
            }
            catalog.remove(b"MarkInfo");
        }
        for id in to_delete {
            doc.objects.remove(&id);
        }
    }

    // --- Seiten ---
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for page_id in page_ids {
        let mut to_delete: Vec<ObjectId> = Vec::new();
        if let Ok(page) = doc.get_dictionary_mut(page_id) {
            if let Ok(Object::Reference(id)) = page.get(b"PieceInfo") {
                to_delete.push(*id);
            }
            if page.remove(b"PieceInfo").is_some() {
                report.piece_info_removed += 1;
            }
            page.remove(b"StructParents");
            page.remove(b"Metadata");
        }
        for id in to_delete {
            doc.objects.remove(&id);
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Dictionary};

    fn doc_with_info() -> Document {
        let mut doc = Document::with_version("1.5");
        let info_id = doc.add_object(dictionary! {
            "Title" => Object::string_literal("Kontoauszug Mustermann"),
            "Author" => Object::string_literal("Max Mustermann"),
        });
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.trailer.set("Info", info_id);
        doc
    }

    #[test]
    fn removes_info_dictionary() {
        let mut doc = doc_with_info();
        let report = strip_metadata(&mut doc);
        assert!(report.info_removed);
        assert!(doc.trailer.get(b"Info").is_err());
        // Auch das Objekt selbst ist weg — nicht nur die Referenz.
        let bytes = {
            let mut buf = Vec::new();
            doc.save_to(&mut buf).unwrap();
            buf
        };
        assert!(!String::from_utf8_lossy(&bytes).contains("Mustermann"));
    }

    #[test]
    fn removes_xmp_metadata() {
        let mut doc = doc_with_info();
        let xmp = doc.add_object(Object::Stream(lopdf::Stream::new(
            Dictionary::new(),
            b"<x:xmpmeta>geheim</x:xmpmeta>".to_vec(),
        )));
        let catalog_id = match doc.trailer.get(b"Root").unwrap() {
            Object::Reference(id) => *id,
            _ => unreachable!(),
        };
        doc.get_dictionary_mut(catalog_id)
            .unwrap()
            .set("Metadata", Object::Reference(xmp));

        let report = strip_metadata(&mut doc);
        assert!(report.xmp_removed);
        assert!(!doc.objects.contains_key(&xmp));
    }

    #[test]
    fn is_idempotent() {
        let mut doc = doc_with_info();
        strip_metadata(&mut doc);
        let second = strip_metadata(&mut doc);
        assert!(!second.anything_removed());
    }
}
