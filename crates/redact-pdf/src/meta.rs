//! Metadaten und Restdaten entfernen.
//!
//! Ein geschwärztes PDF nützt wenig, wenn im `/Info`-Dictionary noch
//! „Kontoauszug_Mustermann_DE89…“ als Titel steht — oder wenn derselbe Text im
//! Wert eines Formularfeldes, in einem Dateianhang oder in einem
//! JavaScript-Schnipsel weiterlebt. Der Content-Stream ist nur *eine* von
//! mehreren Stellen, an denen ein Geheimnis in einer PDF-Datei steht.
//!
//! ## Was tatsächlich entfernt wird
//!
//! Aus dem **Trailer**:
//!
//! * das komplette `/Info`-Dictionary.
//!
//! Aus dem **Katalog**:
//!
//! * der XMP-Metadatenstrom `/Metadata`,
//! * `/PieceInfo` (anwendungsspezifische Zusatzdaten),
//! * die Dokumentstruktur — `/StructTreeRoot` samt `/MarkInfo`; der `/K`-Baum
//!   darunter (mit `/ActualText` und `/Alt`, die den Seitentext spiegeln)
//!   verwaist damit,
//! * der `/Names`-Baum. Er trägt benannte Ziele, **JavaScript**
//!   (`/Names /JavaScript`) und **eingebettete Dateien**
//!   (`/Names /EmbeddedFiles`) — allesamt Texttransporte. Ebenso `/Dests`, der
//!   alte, gleichwertige Weg zu benannten Zielen. Preis: benannte Sprünge
//!   innerhalb des Dokuments funktionieren danach nicht mehr. Das ist die
//!   sichere Richtung,
//! * `/AcroForm`. Zusätzlich werden in **jedem** Feld des Formularbaums die
//!   Werte `/V`, `/DV` und `/RV` gelöscht — ein Widget kann über `/Annots`
//!   erreichbar bleiben, und `/V` trägt dort genau den Text, der aus dem
//!   Content-Stream entfernt wurde. `/XFA` (ein vollständiger zweiter
//!   Formulardatensatz als XML) fällt mit dem `/AcroForm`-Dictionary,
//! * `/OpenAction` und `/AA` — Aktionen, die beim Öffnen bzw. bei Ereignissen
//!   laufen. Beide dürfen `/S /JavaScript` mit beliebigem Quelltext sein,
//! * `/OCProperties` — die Verwaltung optionaler Inhalte („Ebenen“).
//!
//! Aus **jeder Seite**:
//!
//! * `/Metadata` (seitenweites XMP), `/PieceInfo`, `/StructParents`, `/AA`,
//! * Annotationen vom Typ `/FileAttachment` — ein Dateianhang hängt nicht nur
//!   im `/Names`-Baum, er kann auch direkt an einer Seite kleben.
//!
//! ## Was hier bewusst *nicht* passiert
//!
//! * Objekte werden nur dann direkt gelöscht, wenn die entfernte Referenz die
//!   einzige war, die sie erreichbar gemacht hat. Alles Übrige erledigt
//!   [`crate::document::prune_unreachable`]: `lopdf` schreibt beim Speichern
//!   alles, was in `doc.objects` steht — Erreichbarkeit interessiert den Writer
//!   nicht.
//! * Ein `/OCG`-Dictionary, das eine Seite über `/Resources /Properties`
//!   weiterhin referenziert, überlebt das Entfernen von `/OCProperties` samt
//!   seinem `/Name`. Der Ebenenname ist damit die eine bekannte Restdatenstelle,
//!   die dieses Modul offen lässt.
//! * Annotationen außerhalb eines Schwärzungsbereichs bleiben stehen (das
//!   entscheidet [`crate::redact`]), ihre Appearance-Streams also auch.

use std::collections::BTreeSet;

use lopdf::{Dictionary, Document, Object, ObjectId};

/// Maximale Verschachtelungstiefe beim Ablaufen von Feld- und Namensbäumen.
const MAX_TREE_DEPTH: usize = 32;

/// Welche Metadaten und Restdaten entfernt wurden.
///
/// Jeder Eintrag ist ein *gemessenes* Ergebnis, kein Vorsatz — das Audit-Log
/// übernimmt die Zahlen unverändert.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataReport {
    pub info_removed: bool,
    pub xmp_removed: bool,
    pub piece_info_removed: usize,
    pub struct_tree_removed: bool,
    /// `/Names`-Bäume (benannte Ziele, JavaScript, eingebettete Dateien).
    pub names_removed: usize,
    /// Einträge unterhalb von `/Names /EmbeddedFiles`.
    pub embedded_files_removed: usize,
    /// Einträge unterhalb von `/Names /JavaScript`.
    pub javascript_removed: usize,
    /// `/AcroForm` aus dem Katalog entfernt.
    pub acroform_removed: bool,
    /// `/XFA` war vorhanden (und fällt mit dem `/AcroForm`-Dictionary).
    pub xfa_removed: bool,
    /// Gelöschte Feldwerte (`/V`, `/DV`, `/RV`) über alle Felder und Widgets.
    pub field_values_cleared: usize,
    /// Annotationen vom Typ `/FileAttachment`.
    pub file_attachments_removed: usize,
    pub open_action_removed: bool,
    /// `/AA`-Dictionaries aus Katalog und Seiten.
    pub additional_actions_removed: usize,
    pub optional_content_removed: bool,
}

impl MetadataReport {
    pub fn anything_removed(&self) -> bool {
        !self.summary().is_empty()
    }

    /// Klartextzeilen für Konsole und Audit-Log — eine je tatsächlich
    /// durchgeführter Entfernung.
    pub fn summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut flag = |cond: bool, text: &str| {
            if cond {
                out.push(text.to_string());
            }
        };
        flag(self.info_removed, "/Info-Dictionary");
        flag(self.xmp_removed, "XMP-Metadaten (/Metadata)");
        flag(
            self.struct_tree_removed,
            "Dokumentstruktur (/StructTreeRoot)",
        );
        flag(self.acroform_removed, "Formulardefinition (/AcroForm)");
        flag(self.xfa_removed, "XFA-Formulardaten (/XFA)");
        flag(self.open_action_removed, "Öffnen-Aktion (/OpenAction)");
        flag(self.optional_content_removed, "Ebenen (/OCProperties)");

        let mut count = |n: usize, singular: &str, plural: &str| {
            if n == 1 {
                out.push(format!("1 {singular}"));
            } else if n > 1 {
                out.push(format!("{n} {plural}"));
            }
        };
        count(self.piece_info_removed, "/PieceInfo", "/PieceInfo-Blöcke");
        count(self.names_removed, "/Names-Baum", "/Names-Bäume");
        count(
            self.embedded_files_removed,
            "eingebettete Datei",
            "eingebettete Dateien",
        );
        count(
            self.javascript_removed,
            "JavaScript-Eintrag",
            "JavaScript-Einträge",
        );
        count(self.field_values_cleared, "Feldwert", "Feldwerte");
        count(
            self.file_attachments_removed,
            "Dateianhang-Annotation",
            "Dateianhang-Annotationen",
        );
        count(
            self.additional_actions_removed,
            "Ereignisaktion (/AA)",
            "Ereignisaktionen (/AA)",
        );
        out
    }
}

/// Entfernt alle Dokument-Metadaten und die im Modulkommentar genannten
/// Restdaten.
pub fn strip_metadata(doc: &mut Document) -> MetadataReport {
    let mut report = MetadataReport::default();
    let mut to_delete: BTreeSet<ObjectId> = BTreeSet::new();

    // --- Trailer /Info ---
    if let Ok(info) = doc.trailer.get(b"Info") {
        if let Object::Reference(id) = info {
            to_delete.insert(*id);
        }
        doc.trailer.remove(b"Info");
        report.info_removed = true;
    }

    let catalog_id = doc.trailer.get(b"Root").ok().and_then(|o| match o {
        Object::Reference(id) => Some(*id),
        _ => None,
    });
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();

    // --- Erst vermessen, dann verändern ---------------------------------
    //
    // Was in `/Names` steckt und welche Feld-Objekte es gibt, lässt sich nur
    // solange feststellen, wie die Referenzen noch stehen.
    let mut field_ids: BTreeSet<ObjectId> = BTreeSet::new();
    if let Some(catalog_id) = catalog_id {
        if let Ok(catalog) = doc.get_dictionary(catalog_id) {
            if let Ok(names) = catalog.get(b"Names") {
                let (files, js) = count_names_payload(doc, names);
                report.embedded_files_removed += files;
                report.javascript_removed += js;
            }
            if let Ok(acroform) = catalog.get(b"AcroForm") {
                if let Some(dict) = resolve(doc, acroform).and_then(|o| o.as_dict().ok()) {
                    report.xfa_removed = dict.has(b"XFA");
                    if let Ok(fields) = dict.get(b"Fields") {
                        collect_field_ids(doc, fields, &mut field_ids, 0);
                    }
                }
            }
        }
    }
    // Widgets hängen über `/Annots` an der Seite und überleben das Entfernen
    // von `/AcroForm` — samt `/V` und samt ihrer Elternfelder.
    for page_id in &page_ids {
        collect_widget_fields(doc, *page_id, &mut field_ids);
    }
    report.field_values_cleared = clear_field_values(doc, &field_ids);

    // --- Katalog --------------------------------------------------------
    if let Some(catalog_id) = catalog_id {
        if let Ok(catalog) = doc.get_dictionary_mut(catalog_id) {
            if take(catalog, b"Metadata", &mut to_delete) {
                report.xmp_removed = true;
            }
            if take(catalog, b"PieceInfo", &mut to_delete) {
                report.piece_info_removed += 1;
            }
            if take(catalog, b"StructTreeRoot", &mut to_delete) {
                report.struct_tree_removed = true;
            }
            catalog.remove(b"MarkInfo");
            // `/Names` — benannte Ziele, JavaScript, eingebettete Dateien.
            if take(catalog, b"Names", &mut to_delete) {
                report.names_removed += 1;
            }
            // `/Dests` ist der alte, gleichwertige Weg zu benannten Zielen.
            if take(catalog, b"Dests", &mut to_delete) {
                report.names_removed += 1;
            }
            if take(catalog, b"AcroForm", &mut to_delete) {
                report.acroform_removed = true;
            } else {
                // Ohne `/AcroForm` gibt es auch kein `/XFA`.
                report.xfa_removed = false;
            }
            if take(catalog, b"OpenAction", &mut to_delete) {
                report.open_action_removed = true;
            }
            if take(catalog, b"AA", &mut to_delete) {
                report.additional_actions_removed += 1;
            }
            if take(catalog, b"OCProperties", &mut to_delete) {
                report.optional_content_removed = true;
            }
        }
    }

    // --- Seiten ---------------------------------------------------------
    for page_id in &page_ids {
        report.file_attachments_removed += remove_file_attachments(doc, *page_id, &mut to_delete);
        if let Ok(page) = doc.get_dictionary_mut(*page_id) {
            if take(page, b"PieceInfo", &mut to_delete) {
                report.piece_info_removed += 1;
            }
            page.remove(b"StructParents");
            // Seiten-XMP: die Id muss mit auf die Löschliste, sonst bleibt der
            // Strom als verwaistes Objekt in der Datei stehen.
            if take(page, b"Metadata", &mut to_delete) {
                report.xmp_removed = true;
            }
            if take(page, b"AA", &mut to_delete) {
                report.additional_actions_removed += 1;
            }
        }
    }

    for id in to_delete {
        doc.objects.remove(&id);
    }

    report
}

// ---------------------------------------------------------------------------
// Hilfsfunktionen
// ---------------------------------------------------------------------------

/// Entfernt `key` und merkt sich das Objekt, das dadurch seine Referenz
/// verliert. Rückgabe: war der Schlüssel überhaupt vorhanden?
fn take(dict: &mut Dictionary, key: &[u8], to_delete: &mut BTreeSet<ObjectId>) -> bool {
    let referenced = match dict.get(key) {
        Ok(Object::Reference(id)) => Some(*id),
        _ => None,
    };
    let existed = dict.remove(key).is_some();
    if let (true, Some(id)) = (existed, referenced) {
        to_delete.insert(id);
    }
    existed
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Object> {
    doc.dereference(object).ok().map(|(_, o)| o)
}

/// Zählt die Blätter eines Namensbaums (`/Names`-Array bzw. `/Kids`).
fn count_name_tree(doc: &Document, node: &Object, depth: usize) -> usize {
    if depth > MAX_TREE_DEPTH {
        return 0;
    }
    let Some(dict) = resolve(doc, node).and_then(|o| o.as_dict().ok()) else {
        return 0;
    };
    let mut count = 0usize;
    if let Some(items) = dict
        .get(b"Names")
        .ok()
        .and_then(|o| resolve(doc, o))
        .and_then(|o| o.as_array().ok())
    {
        // Ein Namensbaum speichert Paare: Name, Wert, Name, Wert …
        count += items.len() / 2;
    }
    if let Some(kids) = dict
        .get(b"Kids")
        .ok()
        .and_then(|o| resolve(doc, o))
        .and_then(|o| o.as_array().ok())
    {
        for kid in kids {
            count += count_name_tree(doc, kid, depth + 1);
        }
    }
    count
}

/// Wie viele eingebettete Dateien und JavaScript-Einträge hängen am
/// `/Names`-Baum? Ein vorhandener, aber leerer Teilbaum zählt als eins — auch
/// er verschwindet und gehört ins Audit-Log.
fn count_names_payload(doc: &Document, names: &Object) -> (usize, usize) {
    let Some(dict) = resolve(doc, names).and_then(|o| o.as_dict().ok()) else {
        return (0, 0);
    };
    let count_of = |key: &[u8]| match dict.get(key) {
        Ok(node) => count_name_tree(doc, node, 0).max(1),
        Err(_) => 0,
    };
    (count_of(b"EmbeddedFiles"), count_of(b"JavaScript"))
}

/// Sammelt alle als indirektes Objekt vorliegenden Felder eines
/// `/AcroForm /Fields`-Baums (inklusive `/Kids`).
fn collect_field_ids(doc: &Document, fields: &Object, out: &mut BTreeSet<ObjectId>, depth: usize) {
    if depth > MAX_TREE_DEPTH {
        return;
    }
    let Some(items) = resolve(doc, fields).and_then(|o| o.as_array().ok()) else {
        return;
    };
    for item in items {
        if let Object::Reference(id) = item {
            if !out.insert(*id) {
                // Schon gesehen — schützt vor Zyklen in kaputten Dateien.
                continue;
            }
        }
        if let Some(dict) = resolve(doc, item).and_then(|o| o.as_dict().ok()) {
            if let Ok(kids) = dict.get(b"Kids") {
                collect_field_ids(doc, kids, out, depth + 1);
            }
        }
    }
}

/// Trägt die Formularfelder ein, die über die Annotationen einer Seite
/// erreichbar sind — das Widget selbst und die `/Parent`-Kette darüber.
fn collect_widget_fields(doc: &Document, page_id: ObjectId, out: &mut BTreeSet<ObjectId>) {
    let Some(items) = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|page| page.get(b"Annots").ok())
        .and_then(|annots| resolve(doc, annots))
        .and_then(|o| o.as_array().ok())
    else {
        return;
    };
    for item in items {
        let Object::Reference(id) = item else {
            continue;
        };
        let mut current = *id;
        let mut visited: BTreeSet<ObjectId> = BTreeSet::new();
        while visited.insert(current) && visited.len() <= MAX_TREE_DEPTH {
            let Ok(dict) = doc.get_dictionary(current) else {
                break;
            };
            if is_form_field(dict) {
                out.insert(current);
            }
            match dict.get(b"Parent") {
                Ok(Object::Reference(parent)) => current = *parent,
                _ => break,
            }
        }
    }
}

fn is_form_field(dict: &Dictionary) -> bool {
    dict.has(b"FT")
        || dict.has(b"V")
        || dict.has(b"DV")
        || dict.has(b"RV")
        || matches!(dict.get(b"Subtype"), Ok(Object::Name(name)) if name == b"Widget")
}

/// Löscht `/V`, `/DV` und `/RV` — die drei Stellen, an denen ein Formularfeld
/// seinen Wert trägt. Rückgabe: Anzahl gelöschter Werte.
fn clear_field_values(doc: &mut Document, ids: &BTreeSet<ObjectId>) -> usize {
    let mut cleared = 0usize;
    for id in ids {
        let Ok(dict) = doc.get_dictionary_mut(*id) else {
            continue;
        };
        for key in [&b"V"[..], b"DV", b"RV"] {
            if dict.remove(key).is_some() {
                cleared += 1;
            }
        }
    }
    cleared
}

/// Entfernt Annotationen vom Typ `/FileAttachment` aus einer Seite.
fn remove_file_attachments(
    doc: &mut Document,
    page_id: ObjectId,
    to_delete: &mut BTreeSet<ObjectId>,
) -> usize {
    let Some(items) = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|page| page.get(b"Annots").ok())
        .and_then(|annots| resolve(doc, annots))
        .and_then(|o| o.as_array().ok())
        .cloned()
    else {
        return 0;
    };

    let mut kept = Vec::with_capacity(items.len());
    let mut removed = 0usize;
    for item in items {
        let is_attachment = resolve(doc, &item)
            .and_then(|o| o.as_dict().ok())
            .map(|d| matches!(d.get(b"Subtype"), Ok(Object::Name(n)) if n == b"FileAttachment"))
            .unwrap_or(false);
        if is_attachment {
            if let Object::Reference(id) = item {
                to_delete.insert(id);
            }
            removed += 1;
        } else {
            kept.push(item);
        }
    }

    if removed > 0 {
        if let Ok(page) = doc.get_dictionary_mut(page_id) {
            page.set("Annots", Object::Array(kept));
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Dictionary, Stream, StringFormat};

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
        let catalog_id = catalog_of(&doc);
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
        assert!(!second.anything_removed(), "{:?}", second.summary());
    }

    const SECRET: &str = "DE89 3704 0044 0532 0130 00";

    fn catalog_of(doc: &Document) -> ObjectId {
        match doc.trailer.get(b"Root").unwrap() {
            Object::Reference(id) => *id,
            _ => unreachable!(),
        }
    }

    fn set_catalog(doc: &mut Document, key: &str, value: Object) {
        let catalog_id = catalog_of(doc);
        doc.get_dictionary_mut(catalog_id).unwrap().set(key, value);
    }

    #[track_caller]
    fn assert_no_leak(doc: &Document, what: &str) {
        let bytes = crate::document::save_to_bytes(doc).unwrap();
        let hits = crate::leaks(&bytes, SECRET);
        assert!(hits.is_empty(), "{what}: {hits:?}");
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
        set_catalog(&mut doc, "Names", Object::Reference(names));

        let report = strip_metadata(&mut doc);
        assert_eq!(report.names_removed, 1);
        assert!(!doc.objects.contains_key(&names));
        let catalog_id = catalog_of(&doc);
        assert!(doc
            .get_dictionary(catalog_id)
            .unwrap()
            .get(b"Names")
            .is_err());

        assert_no_leak(&doc, "/Names-Baum");
    }

    #[test]
    fn old_style_dests_dictionary_is_removed_too() {
        let mut doc = doc_with_info();
        let dests = doc.add_object(Object::Dictionary(dictionary! {
            "konto" => Object::string_literal(SECRET),
        }));
        set_catalog(&mut doc, "Dests", Object::Reference(dests));

        let report = strip_metadata(&mut doc);
        assert_eq!(report.names_removed, 1);
        assert!(!doc.objects.contains_key(&dests));
    }

    // -----------------------------------------------------------------------
    // Restdaten: Formulare, Anhänge, JavaScript, Aktionen, Ebenen
    // -----------------------------------------------------------------------

    #[test]
    fn acroform_field_values_are_cleared_and_counted() {
        let mut doc = doc_with_info();
        let field = doc.add_object(Object::Dictionary(dictionary! {
            "FT" => "Tx",
            "T" => Object::string_literal("iban"),
            "V" => Object::String(utf16be(SECRET), StringFormat::Literal),
            "DV" => Object::string_literal(SECRET),
        }));
        set_catalog(
            &mut doc,
            "AcroForm",
            Object::Dictionary(dictionary! {
                "Fields" => vec![Object::Reference(field)],
                "XFA" => Object::string_literal(format!("<xfa>{SECRET}</xfa>")),
            }),
        );

        let report = strip_metadata(&mut doc);
        assert!(report.acroform_removed);
        assert!(report.xfa_removed);
        assert_eq!(report.field_values_cleared, 2);
        assert_no_leak(&doc, "AcroForm /V, /DV und /XFA");
    }

    #[test]
    fn a_widget_reachable_through_annots_loses_its_value_too() {
        // Der harte Fall: das Feld hängt zusätzlich an der Seite. `/AcroForm`
        // zu entfernen genügt hier gerade *nicht* — das Widget bleibt über
        // `/Annots` erreichbar.
        let mut doc = doc_with_info();
        let field = doc.add_object(Object::Dictionary(dictionary! {
            "FT" => "Tx",
            "V" => Object::string_literal(SECRET),
        }));
        let widget = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "Parent" => Object::Reference(field),
            "Rect" => vec![0.into(), 0.into(), 100.into(), 20.into()],
        }));
        let page_id = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(page_id)
            .unwrap()
            .set("Annots", Object::Array(vec![Object::Reference(widget)]));
        set_catalog(
            &mut doc,
            "AcroForm",
            Object::Dictionary(dictionary! {
                "Fields" => vec![Object::Reference(field)],
            }),
        );

        let report = strip_metadata(&mut doc);
        assert_eq!(report.field_values_cleared, 1);
        assert_no_leak(&doc, "Widget über /Annots");
    }

    #[test]
    fn embedded_files_and_javascript_are_counted_before_the_names_tree_goes() {
        let mut doc = doc_with_info();
        let attachment = doc.add_object(Object::Stream(Stream::new(
            Dictionary::new(),
            format!("Kontoauszug\nIBAN {SECRET}\n").into_bytes(),
        )));
        let filespec = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Filespec",
            "F" => Object::string_literal("kontoauszug.txt"),
            "EF" => dictionary! { "F" => Object::Reference(attachment) },
        }));
        let names = doc.add_object(Object::Dictionary(dictionary! {
            "EmbeddedFiles" => dictionary! {
                "Names" => vec![
                    Object::string_literal("anhang"),
                    Object::Reference(filespec),
                ],
            },
            "JavaScript" => dictionary! {
                "Names" => vec![
                    Object::string_literal("start"),
                    Object::Dictionary(dictionary! {
                        "S" => "JavaScript",
                        "JS" => Object::string_literal(format!("var iban = '{SECRET}';")),
                    }),
                ],
            },
        }));
        set_catalog(&mut doc, "Names", Object::Reference(names));

        let report = strip_metadata(&mut doc);
        assert_eq!(report.embedded_files_removed, 1);
        assert_eq!(report.javascript_removed, 1);
        assert_no_leak(&doc, "/EmbeddedFiles und /JavaScript");
    }

    #[test]
    fn open_action_and_additional_actions_are_removed() {
        let mut doc = doc_with_info();
        let action = doc.add_object(Object::Dictionary(dictionary! {
            "S" => "JavaScript",
            "JS" => Object::string_literal(format!("app.alert('{SECRET}');")),
        }));
        set_catalog(&mut doc, "OpenAction", Object::Reference(action));
        set_catalog(
            &mut doc,
            "AA",
            Object::Dictionary(dictionary! {
                "WC" => dictionary! {
                    "S" => "JavaScript",
                    "JS" => Object::string_literal(format!("// {SECRET}")),
                },
            }),
        );
        let page_id = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(page_id).unwrap().set(
            "AA",
            Object::Dictionary(dictionary! {
                "O" => dictionary! {
                    "S" => "JavaScript",
                    "JS" => Object::string_literal(format!("// Seite {SECRET}")),
                },
            }),
        );

        let report = strip_metadata(&mut doc);
        assert!(report.open_action_removed);
        assert_eq!(report.additional_actions_removed, 2);
        assert_no_leak(&doc, "/OpenAction und /AA");
    }

    #[test]
    fn optional_content_properties_are_removed() {
        let mut doc = doc_with_info();
        let ocg = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "OCG",
            "Name" => Object::string_literal(format!("Ebene {SECRET}")),
        }));
        set_catalog(
            &mut doc,
            "OCProperties",
            Object::Dictionary(dictionary! {
                "OCGs" => vec![Object::Reference(ocg)],
                "D" => dictionary! { "ON" => vec![Object::Reference(ocg)] },
            }),
        );

        let report = strip_metadata(&mut doc);
        assert!(report.optional_content_removed);
        assert_no_leak(&doc, "/OCProperties");
    }

    #[test]
    fn file_attachment_annotations_are_removed_from_pages() {
        let mut doc = doc_with_info();
        let attachment = doc.add_object(Object::Stream(Stream::new(
            Dictionary::new(),
            format!("IBAN {SECRET}").into_bytes(),
        )));
        let annot = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "FileAttachment",
            "Rect" => vec![0.into(), 0.into(), 20.into(), 20.into()],
            "FS" => dictionary! {
                "Type" => "Filespec",
                "F" => Object::string_literal("anhang.txt"),
                "EF" => dictionary! { "F" => Object::Reference(attachment) },
            },
        }));
        let other = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Square",
            "Rect" => vec![0.into(), 0.into(), 20.into(), 20.into()],
        }));
        let page_id = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(page_id).unwrap().set(
            "Annots",
            Object::Array(vec![Object::Reference(annot), Object::Reference(other)]),
        );

        let report = strip_metadata(&mut doc);
        assert_eq!(report.file_attachments_removed, 1);
        // Die unbeteiligte Annotation bleibt stehen.
        assert!(doc.objects.contains_key(&other));
        assert_no_leak(&doc, "/FileAttachment-Annotation");
    }

    #[test]
    fn summary_lists_exactly_what_was_removed() {
        let mut doc = doc_with_info();
        let report = strip_metadata(&mut doc);
        assert_eq!(report.summary(), vec!["/Info-Dictionary".to_string()]);
    }

    fn utf16be(text: &str) -> Vec<u8> {
        let mut out = vec![0xfe, 0xff];
        out.extend(text.encode_utf16().flat_map(|u| u.to_be_bytes()));
        out
    }
}
