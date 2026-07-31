//! Laden, Prüfen und Speichern von PDF-Dateien.
//!
//! Bewusst streng: verschlüsselte oder strukturell kaputte Dateien werden
//! abgelehnt statt repariert. Eine „reparierte“ Datei könnte Inhalte enthalten,
//! die der Analyse entgehen — und damit ungeschwärzt durchrutschen.

use std::path::Path;

use lopdf::{Document, Object};
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

/// Serialisiert das Dokument in den Speicher.
pub fn save_to_bytes(doc: &Document) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut copy = doc.clone();
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
}
