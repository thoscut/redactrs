//! Metadaten entfernen.
//!
//! Ein geschwärztes PDF nützt wenig, wenn im `/Info`-Dictionary noch
//! „Kontoauszug_Mustermann_DE89…“ als Titel steht. Entfernt werden:
//!
//! * das komplette `/Info`-Dictionary aus dem Trailer,
//! * der XMP-Metadatenstrom `/Metadata` aus Katalog **und** Seiten,
//! * `/PieceInfo` (anwendungsspezifische Zusatzdaten) aus Katalog und Seiten,
//! * die Dokumentstruktur — `/StructTreeRoot` samt `/MarkInfo` im Katalog und
//!   `/StructParents` je Seite; der `/K`-Baum darunter (mit `/ActualText` und
//!   `/Alt`, die den Seitentext spiegeln) verwaist damit und fällt beim
//!   Erreichbarkeitslauf in [`crate::document::save_to_bytes`] weg,
//! * der `/Names`-Baum des Katalogs. Er trägt benannte Ziele, JavaScript und
//!   eingebettete Dateien — allesamt Texttransporte. Preis: benannte Sprünge
//!   innerhalb des Dokuments funktionieren danach nicht mehr. Das ist die
//!   sichere Richtung.
//!
//! Was hier nur dereferenziert wird, verschwindet nicht automatisch aus der
//! Datei: `lopdf` schreibt beim Speichern alles, was in `doc.objects` steht.
//! Den Rest erledigt [`crate::document::prune_unreachable`].

use lopdf::{Document, Object, ObjectId};

/// Welche Metadaten entfernt wurden.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataReport {
    pub info_removed: bool,
    pub xmp_removed: bool,
    pub piece_info_removed: usize,
    pub struct_tree_removed: bool,
    /// `/Names`-Bäume (benannte Ziele, JavaScript, eingebettete Dateien).
    pub names_removed: usize,
}

impl MetadataReport {
    pub fn anything_removed(&self) -> bool {
        self.info_removed
            || self.xmp_removed
            || self.piece_info_removed > 0
            || self.struct_tree_removed
            || self.names_removed > 0
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
            // `/Names` — benannte Ziele, JavaScript, eingebettete Dateien.
            if let Ok(Object::Reference(id)) = catalog.get(b"Names") {
                to_delete.push(*id);
            }
            if catalog.remove(b"Names").is_some() {
                report.names_removed += 1;
            }
            // `/Dests` ist der alte, gleichwertige Weg zu benannten Zielen.
            if let Ok(Object::Reference(id)) = catalog.get(b"Dests") {
                to_delete.push(*id);
            }
            if catalog.remove(b"Dests").is_some() {
                report.names_removed += 1;
            }
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
            // Seiten-XMP: die Id muss mit auf die Löschliste, sonst bleibt der
            // Strom als verwaistes Objekt in der Datei stehen.
            if let Ok(Object::Reference(id)) = page.get(b"Metadata") {
                to_delete.push(*id);
            }
            if page.remove(b"Metadata").is_some() {
                report.xmp_removed = true;
            }
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

    const SECRET: &str = "DE89 3704 0044 0532 0130 00";

    fn catalog_of(doc: &Document) -> ObjectId {
        match doc.trailer.get(b"Root").unwrap() {
            Object::Reference(id) => *id,
            _ => unreachable!(),
        }
    }

    #[test]
    fn page_metadata_object_is_deleted_not_just_dereferenced() {
        let mut doc = doc_with_info();
        let xmp = doc.add_object(Object::Stream(lopdf::Stream::new(
            dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            format!("<x:xmpmeta><dc:title>Kontoauszug {SECRET}</dc:title></x:xmpmeta>")
                .into_bytes(),
        )));
        let page_id = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(page_id)
            .unwrap()
            .set("Metadata", Object::Reference(xmp));

        let report = strip_metadata(&mut doc);
        assert!(report.xmp_removed);
        assert!(
            !doc.objects.contains_key(&xmp),
            "das Seiten-XMP steht weiterhin als verwaistes Objekt in der Datei"
        );
    }

    #[test]
    fn names_tree_is_removed_as_the_module_documentation_promises() {
        let mut doc = doc_with_info();
        let names = doc.add_object(Object::Dictionary(dictionary! {
            "Dests" => dictionary! {
                "Names" => vec![
                    Object::string_literal("konto"),
                    Object::string_literal(SECRET),
                ],
            },
        }));
        let catalog_id = catalog_of(&doc);
        doc.get_dictionary_mut(catalog_id)
            .unwrap()
            .set("Names", Object::Reference(names));

        let report = strip_metadata(&mut doc);
        assert_eq!(report.names_removed, 1);
        assert!(!doc.objects.contains_key(&names));
        assert!(doc
            .get_dictionary(catalog_id)
            .unwrap()
            .get(b"Names")
            .is_err());

        let bytes = crate::document::save_to_bytes(&doc).unwrap();
        assert!(
            crate::leaks(&bytes, SECRET).is_empty(),
            "{:?}",
            crate::leaks(&bytes, SECRET)
        );
    }

    #[test]
    fn old_style_dests_dictionary_is_removed_too() {
        let mut doc = doc_with_info();
        let dests = doc.add_object(Object::Dictionary(dictionary! {
            "konto" => Object::string_literal(SECRET),
        }));
        let catalog_id = catalog_of(&doc);
        doc.get_dictionary_mut(catalog_id)
            .unwrap()
            .set("Dests", Object::Reference(dests));

        let report = strip_metadata(&mut doc);
        assert_eq!(report.names_removed, 1);
        assert!(!doc.objects.contains_key(&dests));
    }
}
