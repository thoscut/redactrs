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
//! Aus **jedem verbliebenen `/OCG`**:
//!
//! * `/Name` (auf einen leeren String gesetzt — das Feld ist Pflicht) und
//!   `/Usage`. `/OCProperties` zu löschen genügt nicht: eine Seite hält ein
//!   `/OCG` über `/Resources /Properties` am Leben, und der Ebenenname ist
//!   frei wählbarer Text.
//!
//! * `/Outlines` — die Lesezeichen. Jeder Eintrag trägt einen `/Title`, und
//!   der ist frei wählbarer Text: „Kontoauszug DE89 …“ ist ein Lesezeichen,
//!   wie es jeder Erzeuger schreibt. Gemessen (vor dieser Änderung): eine Datei mit der
//!   IBAN allein im Lesezeichen endete mit 0 Treffern, Rückgabewert 0, und
//!   `--check-leaks` fand sie. Preis: die Gliederung im Betrachter ist weg.
//!
//! Aus **jeder Seite**:
//!
//! * `/Metadata` (seitenweites XMP), `/PieceInfo`, `/StructParents`, `/AA`,
//! * Annotationen vom Typ `/FileAttachment` — ein Dateianhang hängt nicht nur
//!   im `/Names`-Baum, er kann auch direkt an einer Seite kleben.
//!
//! Aus **jeder verbliebenen Annotation** — und aus allem, was sie erreichbar
//! hält: ihrem `/Popup`, der `/Parent`-Kette nach oben (das Feld einer
//! Radiogruppe, das Feld hinter mehreren Widgets), `/Kids` nach unten und
//! `/IRT` (Antwortkette). Ein Träger, den nur `/AcroForm` erreichbar machte,
//! fällt mit dem Formular; einen, den ein Widget in `/Annots` über `/Parent`
//! hält, überlebt es — und der trägt `/T`, `/TU`, `/Opt` und `/AA` genauso
//! wie das Widget selbst. Gemessen (vor dieser Änderung): `/Contents` eines
//! nur über `/Popup` erreichbaren Popups, `/T` und `/TU` am Elternfeld,
//! `/Opt` am Feld und am Widget, `/AA /K /JS` am Elternfeld, `/MK /CA`,
//! `/OverlayText`, `/PA`, `/NM` und `/DS` an der Annotation — alle mit
//! Rückgabewert 0 überlebt. Entfernt wird an jedem erreichten Dictionary:
//!
//! * die Aktionen `/A`, `/AA` und `/PA` (die URI-Aktion eines Links) und ein
//!   benanntes `/Dest`. Ein Link fern des Textes trägt seinen Klartext in
//!   `/URI` (`mailto:…?subject=DE89 …`), in `/F` (`/GoToR`, `/Launch`:
//!   `Kontoauszug_DE89….pdf`) oder in `/JS`; ein benanntes Ziel ist eine
//!   Zeichenkette. Gemessen: alle drei Formen überlebten die Schwärzung mit
//!   Rückgabewert 0. Ein `/Dest` als Feld (`[Seite /XYZ x y z]`) bleibt — es
//!   trägt Zahlen und Verweise, keinen Text. Entschieden wird über das
//!   **Feld**, elementweise aufgelöst: ein `/Dest 12 0 R` wird aufgelöst, und
//!   ein Verweis *im* Feld muss auf ein Seitenobjekt (`/Type /Page`) führen —
//!   `[12 0 R /XYZ …]` mit einer Zeichenkette in Objekt 12 ist kein Ziel,
//!   sondern ein Versteck. Preis: Verweise ins Netz und in andere Dateien
//!   funktionieren danach nicht mehr,
//! * die Klartexte `/Contents`, `/RC`, `/T` (Verfasser; an einem Feld der
//!   Feldname) und `/Subj`, dazu `/TU` (der alternative Feldname — das ist
//!   der Tooltip, den der Betrachter beim Überfahren zeigt), `/TM` (der
//!   Exportname; PDF 32000-1, 12.7.3.1, Tabelle 220), `/Opt` (die
//!   Auswahltexte einer Liste, Tabelle 231), `/OverlayText` (Redact,
//!   Tabelle 195), `/NM` (der Annotationsname), `/DS` (die Stilangabe eines
//!   FreeText) und die Beschriftungen `/MK /CA`, `/MK /RC`, `/MK /AC`
//!   (Tabelle 189). Gemessen: eine Notiz mit Symbol-Erscheinungsstrom trug
//!   die IBAN in `/Contents`, ein `/T` und ein `/RC` neben einem
//!   Erscheinungsstrom ebenso — alle mit Rückgabewert 0. Was eine Annotation
//!   **zeichnet** (`/AP`), geht wie Seitentext durch die Schwärzung und
//!   bleibt; was sie daneben als Klartext mitführt, hat keine
//!   Glyphengeometrie und kann nicht anteilig geschwärzt werden. Das ist
//!   dieselbe Entscheidung wie bei den Feldwerten — und dieselbe, die
//!   Acrobats „Dokument bereinigen“ trifft.
//!
//! **Benannte Lücke:** `/DA` (Default Appearance) bleibt. Es ist bei FreeText
//! und Widgets Pflicht und eine Operatorfolge (`/Helv 12 Tf 0 g`), keine
//! Zeichenkette für Menschen — wer eine IBAN als Schriftnamen in `/DA`
//! schreibt, kommt durch. Ein Dictionary mit einem fremden `/Type` (Seite,
//! Seitenbaum) ist kein Träger und wird weder bereinigt noch abgelaufen: eine
//! kaputte `/Parent`-Kette, die auf die Seite führt, darf ihr nicht
//! `/Contents` nehmen.
//!
//! ## Was hier bewusst *nicht* passiert
//!
//! * Objekte werden nie einzeln gelöscht — nur Schlüssel. Was dadurch
//!   unerreichbar wird, räumt [`crate::document::prune_unreachable`] am Ende
//!   dieses Laufs weg (und noch einmal beim Speichern): `lopdf` schreibt
//!   alles, was in `doc.objects` steht — Erreichbarkeit interessiert den
//!   Writer nicht. Gemessen (vor dieser Änderung): das Löschen je Schlüssel
//!   traf auch geteilte Objekte — `/Contents 4 0 R` an einer Notiz auf den
//!   Seiteninhalt nahm der Seite ihren Inhalt, ein `/Outlines /First` auf die
//!   Seite löschte die Seite („enthält keine Seiten“).
//! * Der Inhalt einer Ebene wird nicht angerührt. Entfernt wird ihr *Name*,
//!   nicht der Text, den sie zeichnet — der geht denselben Weg wie jeder
//!   andere Seiteninhalt durch [`crate::redact`].
//! * Annotationen außerhalb eines Schwärzungsbereichs bleiben stehen (das
//!   entscheidet [`crate::redact`]), ihre Appearance-Streams also auch.

use std::collections::BTreeSet;

use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};

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
    /// Ebenennamen (`/OCG /Name`), die aus der Datei entfernt wurden.
    ///
    /// `/OCProperties` zu löschen genügt nicht: eine Seite kann dasselbe
    /// `/OCG` über `/Resources /Properties` weiter referenzieren, und dann
    /// bleibt sein `/Name` in der Datei stehen — frei wählbarer Text, der
    /// denselben Klartext tragen kann, der gerade aus dem Strom entfernt
    /// wurde.
    pub optional_content_names_cleared: usize,
    /// Lesezeichen (`/Outlines`-Einträge), mit dem Baum entfernt.
    pub outlines_removed: usize,
    /// `/A`, `/AA`, `/PA` und benannte `/Dest` an Annotationen und an
    /// allem, was sie erreichbar halten (`/Popup`, `/Parent`, `/Kids`,
    /// `/IRT`).
    pub annotation_actions_removed: usize,
    /// Klartexte an Annotationen und erreichbaren Feldern — je Schlüssel
    /// einer: `/Contents`, `/RC`, `/T`, `/Subj`, `/TU`, `/TM`, `/Opt`,
    /// `/OverlayText`, `/NM`, `/DS` sowie `/CA`, `/RC`, `/AC` in `/MK`.
    pub annotation_texts_cleared: usize,
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
        count(
            self.optional_content_names_cleared,
            "Ebenenname (/OCG /Name)",
            "Ebenennamen (/OCG /Name)",
        );
        count(
            self.outlines_removed,
            "Lesezeichen (/Outlines)",
            "Lesezeichen (/Outlines)",
        );
        count(
            self.annotation_actions_removed,
            "Aktion oder benanntes Ziel an einer Annotation (/A, /AA, /PA, /Dest)",
            "Aktionen oder benannte Ziele an Annotationen (/A, /AA, /PA, /Dest)",
        );
        count(
            self.annotation_texts_cleared,
            "Kommentartext an einer Annotation (/Contents, /RC, /T, /Subj, /TU, /TM, /Opt, /OverlayText, /NM, /DS, /MK)",
            "Kommentartexte an Annotationen (/Contents, /RC, /T, /Subj, /TU, /TM, /Opt, /OverlayText, /NM, /DS, /MK)",
        );
        out
    }
}

/// Entfernt alle Dokument-Metadaten und die im Modulkommentar genannten
/// Restdaten.
pub fn strip_metadata(doc: &mut Document) -> MetadataReport {
    let mut report = MetadataReport::default();

    // --- Trailer /Info ---
    if doc.trailer.remove(b"Info").is_some() {
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
    // Lesezeichen: zählen, solange der Baum steht. Ein Eintrag ist nur über
    // `/Outlines` erreichbar; er fällt mit `prune_unreachable`, aber die Zahl
    // gehört in den Bericht.
    if let Some(outlines) = catalog_id
        .and_then(|id| doc.get_dictionary(id).ok())
        .and_then(|catalog| catalog.get(b"Outlines").ok())
    {
        report.outlines_removed = collect_outline_items(doc, outlines);
    }

    // --- Katalog --------------------------------------------------------
    if let Some(catalog_id) = catalog_id {
        if let Ok(catalog) = doc.get_dictionary_mut(catalog_id) {
            if take(catalog, b"Metadata") {
                report.xmp_removed = true;
            }
            if take(catalog, b"PieceInfo") {
                report.piece_info_removed += 1;
            }
            if take(catalog, b"StructTreeRoot") {
                report.struct_tree_removed = true;
            }
            catalog.remove(b"MarkInfo");
            // `/Names` — benannte Ziele, JavaScript, eingebettete Dateien.
            if take(catalog, b"Names") {
                report.names_removed += 1;
            }
            // `/Dests` ist der alte, gleichwertige Weg zu benannten Zielen.
            if take(catalog, b"Dests") {
                report.names_removed += 1;
            }
            if take(catalog, b"AcroForm") {
                report.acroform_removed = true;
            } else {
                // Ohne `/AcroForm` gibt es auch kein `/XFA`.
                report.xfa_removed = false;
            }
            if take(catalog, b"OpenAction") {
                report.open_action_removed = true;
            }
            if take(catalog, b"AA") {
                report.additional_actions_removed += 1;
            }
            if take(catalog, b"OCProperties") {
                report.optional_content_removed = true;
            }
            take(catalog, b"Outlines");
        }
    }

    // --- Seiten ---------------------------------------------------------
    //
    // Die Besuchsmenge gilt für das ganze Dokument: ein Feld, dessen Widgets
    // auf zwei Seiten liegen, wird einmal bereinigt und einmal gezählt.
    let mut visited: BTreeSet<ObjectId> = BTreeSet::new();
    for page_id in &page_ids {
        report.file_attachments_removed += remove_file_attachments(doc, *page_id);
        if let Ok(page) = doc.get_dictionary_mut(*page_id) {
            if take(page, b"PieceInfo") {
                report.piece_info_removed += 1;
            }
            page.remove(b"StructParents");
            if take(page, b"Metadata") {
                report.xmp_removed = true;
            }
            if take(page, b"AA") {
                report.additional_actions_removed += 1;
            }
        }
        let (actions, texts) = clean_annotations(doc, *page_id, &mut visited);
        report.annotation_actions_removed += actions;
        report.annotation_texts_cleared += texts;
    }

    // --- Aufräumen ------------------------------------------------------
    //
    // Bis hierher sind nur Schlüssel gefallen. Alles, was dadurch niemand
    // mehr referenziert (das `/Info`-Objekt, der `/Names`-Baum, jeder
    // Lesezeichen-Eintrag, ein `/RC`-Strom), fällt jetzt — und ein geteiltes
    // Objekt (`/Contents 4 0 R` auf den Seiteninhalt) bleibt, weil die Seite
    // es weiterhin hält.
    crate::document::prune_unreachable(doc);

    // --- Ebenennamen ----------------------------------------------------
    //
    // Erst *nach* dem Aufräumen: was mit `/OCProperties` verschwunden ist,
    // wird hier weder angefasst noch gezählt. Was übrig bleibt, ist genau der
    // Fall, den dieses Modul bis Aufgabe #57 offen gelassen hat.
    report.optional_content_names_cleared = clear_optional_content_names(doc);

    report
}

/// Leert den Klartextnamen jedes verbliebenen `/OCG`-Dictionaries.
///
/// Warum überhaupt: `/OCProperties` aus dem Katalog zu entfernen macht die
/// Ebenenverwaltung unerreichbar, nicht aber die Ebenen selbst. Eine Seite,
/// die ein `/OCG` über `/Resources /Properties` benutzt (so wird `/BDC /OC`
/// aufgelöst), hält es weiterhin am Leben — samt `/Name`, und der ist frei
/// wählbarer Text: „Ebene Mustermann“ ist ein Ebenenname, wie ihn jedes
/// Layout-Programm schreibt.
///
/// Warum leeren statt löschen: `/Name` ist bei `/OCG` ein Pflichtfeld. Ein
/// leerer String hält die Datei regelkonform und trägt nichts mehr.
///
/// Gesucht wird in **allen** Objekten und rekursiv auch in direkt
/// eingebetteten Dictionaries — ein `/OCG` muss kein indirektes Objekt sein,
/// und über welchen Weg es erreichbar ist, spielt für den Klartext keine
/// Rolle. `/Usage` fällt mit: dort steht unter `/CreatorInfo` ebenfalls frei
/// wählbarer Text.
fn clear_optional_content_names(doc: &mut Document) -> usize {
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    let mut cleared = 0;
    for id in ids {
        if let Some(object) = doc.objects.get_mut(&id) {
            cleared += clear_ocg_names(object, 0);
        }
    }
    cleared
}

fn clear_ocg_names(object: &mut Object, depth: usize) -> usize {
    if depth > MAX_TREE_DEPTH {
        return 0;
    }
    match object {
        Object::Dictionary(dict) => clear_ocg_names_in_dict(dict, depth),
        Object::Stream(stream) => clear_ocg_names_in_dict(&mut stream.dict, depth),
        Object::Array(items) => items
            .iter_mut()
            .map(|item| clear_ocg_names(item, depth + 1))
            .sum(),
        _ => 0,
    }
}

fn clear_ocg_names_in_dict(dict: &mut Dictionary, depth: usize) -> usize {
    let mut cleared = 0;
    if dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"OCG") {
        let has_text = dict
            .get(b"Name")
            .is_ok_and(|name| !matches!(name, Object::String(bytes, _) if bytes.is_empty()));
        if has_text {
            dict.set("Name", Object::String(Vec::new(), StringFormat::Literal));
            cleared += 1;
        }
        dict.remove(b"Usage");
    }
    for (_, value) in dict.iter_mut() {
        cleared += clear_ocg_names(value, depth + 1);
    }
    cleared
}

// ---------------------------------------------------------------------------
// Hilfsfunktionen
// ---------------------------------------------------------------------------

/// Zählt die Einträge eines Lesezeichenbaums.
///
/// Gelaufen wird `/First` → `/Next` je Ebene und `/First` in die Tiefe, mit
/// Besuchsmenge: ein Baum, der auf sich selbst zeigt, wäre sonst endlos.
/// Gedeckelt ist der Lauf allein durch die Besuchsmenge — jede Id wird
/// höchstens einmal angefasst, und mehr Ids als Objekte gibt es nicht. Eine
/// Tiefengrenze gab es hier einmal; sie brach mit `break` ab und ließ die
/// Geschwister der oberen Ebenen ungezählt (33 statt 42). Der Stapel wächst
/// je Eintrag um höchstens zwei Einträge, Rekursion gibt es nicht — 100 000
/// Stufen tief enden in Millisekunden.
fn collect_outline_items(doc: &Document, root: &Object) -> usize {
    let mut seen: BTreeSet<ObjectId> = BTreeSet::new();
    if let Object::Reference(id) = root {
        seen.insert(*id);
    }
    let Some(first) = resolve(doc, root)
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"First").ok())
    else {
        return 0;
    };
    let mut count = 0usize;
    let mut stack: Vec<Object> = vec![first.clone()];
    while let Some(item) = stack.pop() {
        let Object::Reference(id) = item else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        let Ok(dict) = doc.get_dictionary(id) else {
            continue;
        };
        count += 1;
        for key in [b"Next".as_slice(), b"First".as_slice()] {
            if let Ok(link) = dict.get(key) {
                stack.push(link.clone());
            }
        }
    }
    count
}

/// Nimmt jeder Annotation der Seite — und allem, was sie erreichbar hält —
/// ihre Aktionen und Klartexte.
///
/// Liefert (entfernte Aktionen und benannte Ziele, entfernte Texte). Eine
/// Annotation steht gewöhnlich als eigenes Objekt in `/Annots`; ein direkt
/// eingebettetes Dictionary wird im Feld selbst bereinigt. Von jeder
/// Annotation aus werden [`ANNOTATION_LINK_KEYS`] verfolgt: das `/Popup`
/// hängt sonst nur an der Notiz, das Elternfeld eines Widgets nur an dessen
/// `/Parent`, und dessen `/Kids` halten die Geschwister-Widgets. `visited`
/// endet Zyklen (`/Parent` ↔ `/Kids`, `/Popup` ↔ `/Parent`) und sorgt dafür,
/// dass ein geteiltes Feld einmal gezählt wird.
fn clean_annotations(
    doc: &mut Document,
    page_id: ObjectId,
    visited: &mut BTreeSet<ObjectId>,
) -> (usize, usize) {
    let Some(annots) = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|page| page.get(b"Annots").ok())
        .and_then(|o| resolve(doc, o))
        .and_then(|o| o.as_array().ok())
        .cloned()
    else {
        return (0, 0);
    };
    let mut actions = 0usize;
    let mut texts = 0usize;
    let mut stack: Vec<ObjectId> = Vec::new();
    let mut inline: Vec<Object> = Vec::new();
    let mut inline_changed = false;
    for annot in annots {
        match annot {
            Object::Reference(id) => {
                stack.push(id);
                inline.push(Object::Reference(id));
            }
            Object::Dictionary(mut dict) => {
                let keep_dest = keeps_destination(doc, &dict);
                linked_ids(&dict, &mut stack);
                let (a, t) = clean_carrier(&mut dict, keep_dest);
                inline_changed |= a + t > 0;
                actions += a;
                texts += t;
                inline.push(Object::Dictionary(dict));
            }
            other => inline.push(other),
        }
    }
    if inline_changed {
        if let Ok(page) = doc.get_dictionary_mut(page_id) {
            page.set("Annots", Object::Array(inline));
        }
    }

    while let Some(id) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        // Was sich nur am unveränderten Dokument entscheiden lässt — das
        // Ziel, die Nachbarn, die Id hinter einer Verweiskette — kommt
        // zuerst; der schreibende Zugriff danach braucht nur noch die Id.
        let reference = Object::Reference(id);
        let Ok((resolved, object)) = doc.dereference(&reference) else {
            continue;
        };
        let id = resolved.unwrap_or(id);
        let (keep_dest, mk) = match object {
            // `/Kids 5 0 R` — das Feld selbst als eigenes Objekt.
            Object::Array(items) => {
                stack.extend(items.iter().filter_map(|item| item.as_reference().ok()));
                continue;
            }
            Object::Dictionary(dict) => {
                if !is_carrier(dict) {
                    continue;
                }
                linked_ids(dict, &mut stack);
                let mk = match dict.get(b"MK") {
                    Ok(Object::Reference(mk)) => Some(*mk),
                    _ => None,
                };
                (keeps_destination(doc, dict), mk)
            }
            _ => continue,
        };
        if let Ok(dict) = doc.get_dictionary_mut(id) {
            let (a, t) = clean_carrier(dict, keep_dest);
            actions += a;
            texts += t;
        }
        // Ein `/MK` als eigenes Objekt — von mehreren Widgets geteilt.
        if let Some(mk) = mk {
            if visited.insert(mk) {
                if let Ok(dict) = doc.get_dictionary_mut(mk) {
                    texts += clean_captions(dict);
                }
            }
        }
    }
    (actions, texts)
}

/// Die Schlüssel, unter denen eine Annotation Klartext neben ihrem
/// Erscheinungsbild führt.
///
/// `/Contents`, `/RC`, `/T` und `/Subj` sind die Kommentartexte (PDF 32000-1,
/// 12.5.2 und 12.5.6.2); an einem Feld ist `/T` der Feldname. `/TU` und
/// `/TM` stehen an Formularfeldern (12.7.3.1, Tabelle 220): `/TU` ist der
/// alternative Feldname, den der Betrachter als **Tooltip** zeigt, `/TM` der
/// Exportname beim Absenden. Beide sind frei wählbarer Text und werden von
/// Formulargeneratoren mit dem Beschriftungstext gefüllt — „Konto von Max
/// Mustermann“ ist ein Tooltip, wie ihn jeder Editor schreibt. `/Opt` sind
/// die Auswahltexte einer Liste oder die Exportwerte einer Radiogruppe
/// (Tabelle 231), `/OverlayText` der Text, den eine Redact-Annotation über
/// die Stelle schreibt (Tabelle 195), `/NM` der frei wählbare Name der
/// Annotation (Tabelle 164) und `/DS` die Stilangabe eines FreeText
/// (Tabelle 174) — alle vier Zeichenketten ohne Glyphengeometrie.
pub(crate) const ANNOTATION_TEXT_KEYS: [&[u8]; 10] = [
    b"Contents",
    b"RC",
    b"T",
    b"Subj",
    b"TU",
    b"TM",
    b"Opt",
    b"OverlayText",
    b"NM",
    b"DS",
];

/// Aktionen an einer Annotation oder einem Feld: `/A` und `/AA`
/// (Tabelle 164) und `/PA`, die URI-Aktion eines Links (Tabelle 173).
const ANNOTATION_ACTION_KEYS: [&[u8]; 3] = [b"A", b"AA", b"PA"];

/// Die Beschriftungen im `/MK`-Dictionary eines Widgets (Tabelle 189):
/// normal, beim Überfahren, beim Drücken.
const CAPTION_KEYS: [&[u8]; 3] = [b"CA", b"RC", b"AC"];

/// Die Schlüssel, über die eine Annotation weitere Träger erreichbar hält:
/// ihr `/Popup` (Tabelle 170), das Elternfeld (`/Parent`, Tabelle 220), die
/// Kindfelder und Geschwister-Widgets (`/Kids`) und die Annotation, auf die
/// sie antwortet (`/IRT`, Tabelle 170).
const ANNOTATION_LINK_KEYS: [&[u8]; 4] = [b"Popup", b"Parent", b"Kids", b"IRT"];

/// Trägt die Ids hinter [`ANNOTATION_LINK_KEYS`] ein — einzeln oder als
/// Feld (`/Kids`).
fn linked_ids(dict: &Dictionary, out: &mut Vec<ObjectId>) {
    for key in ANNOTATION_LINK_KEYS {
        match dict.get(key) {
            Ok(Object::Reference(id)) => out.push(*id),
            Ok(Object::Array(items)) => {
                out.extend(items.iter().filter_map(|item| item.as_reference().ok()));
            }
            _ => {}
        }
    }
}

/// Ist dieses Dictionary eine Annotation oder ein Formularfeld?
///
/// Beide tragen entweder kein `/Type` (Felder, und Annotationen dürfen es
/// weglassen) oder `/Type /Annot`. Alles andere — eine Seite, der
/// Seitenbaum, ein Katalog — ist kein Träger: eine kaputte `/Parent`-Kette,
/// die auf die Seite führt, darf ihr weder `/Contents` noch `/Kids` nehmen.
fn is_carrier(dict: &Dictionary) -> bool {
    match dict.get(b"Type") {
        Ok(Object::Name(name)) => name == b"Annot",
        Ok(_) => false,
        Err(_) => true,
    }
}

/// Darf das `/Dest` dieser Annotation stehen bleiben? Nur, wenn es eines
/// trägt **und** dieses — nach Auflösung — ein ausdrückliches Ziel ist.
fn keeps_destination(doc: &Document, dict: &Dictionary) -> bool {
    dict.get(b"Dest")
        .is_ok_and(|dest| is_explicit_destination(doc, dest))
}

/// Nimmt einer Annotation oder einem Feld Aktionen und Klartexte. `keep_dest`
/// ist vorher am unveränderten Dokument bestimmt (siehe
/// [`keeps_destination`]). Liefert (Aktionen, Texte).
fn clean_carrier(dict: &mut Dictionary, keep_dest: bool) -> (usize, usize) {
    let mut actions = 0usize;
    for key in ANNOTATION_ACTION_KEYS {
        if take(dict, key) {
            actions += 1;
        }
    }
    if !keep_dest && take(dict, b"Dest") {
        actions += 1;
    }
    let mut texts = 0usize;
    for key in ANNOTATION_TEXT_KEYS {
        if take(dict, key) {
            texts += 1;
        }
    }
    if let Ok(Object::Dictionary(mk)) = dict.get_mut(b"MK") {
        texts += clean_captions(mk);
    }
    (actions, texts)
}

/// Leert die Beschriftungen eines `/MK`-Dictionaries. Rückgabe: wie viele.
fn clean_captions(mk: &mut Dictionary) -> usize {
    CAPTION_KEYS.into_iter().filter(|key| take(mk, key)).count()
}

/// Ein `/Dest` ohne Text: ein Feld aus Seitenverweis, Zahlen und einem der
/// Anzeigenamen aus PDF 32000-1, Tabelle 151 (`[Seite /XYZ x y z]`).
///
/// Ein Verweis wird zuerst **aufgelöst** — das Feld selbst wie jedes seiner
/// Elemente: `/Dest 12 0 R` ist, was in Objekt 12 steht, und `[12 0 R /XYZ …]`
/// ist nur dann ein Ziel, wenn Objekt 12 eine Seite (`/Type /Page`) ist.
/// Entschieden wird über den Inhalt, nicht über die Schreibweise; ein
/// Erzeuger, der jedes Ziel als eigenes Objekt ablegt, verliert seine Sprünge
/// sonst grundlos — und einer, der eine Zeichenkette hinter den Verweis im
/// Feld legt, kommt nicht durch.
///
/// Alles andere — eine Zeichenkette (benanntes Ziel), ein Name als Ziel,
/// ein Feld mit einem fremden Namen darin — kann Text tragen und fällt; ein
/// Verweis ins Leere fällt mit, er führt ohnehin nirgendwohin.
fn is_explicit_destination(doc: &Document, dest: &Object) -> bool {
    const FIT: [&[u8]; 8] = [
        b"XYZ", b"Fit", b"FitH", b"FitV", b"FitR", b"FitB", b"FitBH", b"FitBV",
    ];
    let Some(Object::Array(items)) = resolve(doc, dest) else {
        return false;
    };
    items.iter().all(|item| match resolve(doc, item) {
        Some(Object::Integer(_) | Object::Real(_) | Object::Null) => true,
        Some(Object::Name(name)) => FIT.contains(&name.as_slice()),
        // Eine Seite ist immer ein eigenes Objekt — ein direkt eingebettetes
        // Dictionary im Zielfeld ist keine.
        Some(Object::Dictionary(dict)) => {
            matches!(item, Object::Reference(_))
                && dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Page")
        }
        _ => false,
    })
}

/// Entfernt `key`. Rückgabe: war der Schlüssel überhaupt vorhanden? Das
/// Objekt dahinter bleibt stehen — ob es noch jemand hält, entscheidet
/// `prune_unreachable`, nicht diese Stelle.
fn take(dict: &mut Dictionary, key: &[u8]) -> bool {
    dict.remove(key).is_some()
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
fn remove_file_attachments(doc: &mut Document, page_id: ObjectId) -> usize {
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

    /// Eine kaputte `/Parent`-Kette, die auf die Seite führt: die Seite ist
    /// kein Träger und behält `/Contents` — und über ihr `/Parent` (den
    /// Seitenbaum) läuft die Bereinigung nicht weiter.
    #[test]
    fn a_parent_chain_onto_the_page_tree_is_not_cleaned() {
        let mut doc = doc_with_info();
        let page_id = *doc.get_pages().values().next().unwrap();
        let content = doc.add_object(Object::Stream(Stream::new(
            Dictionary::new(),
            b"BT (Seitentext) Tj ET".to_vec(),
        )));
        let widget = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "T" => Object::string_literal("feld"),
            "Parent" => Object::Reference(page_id),
        }));
        let page = doc.get_dictionary_mut(page_id).unwrap();
        page.set("Contents", Object::Reference(content));
        page.set("Annots", Object::Array(vec![Object::Reference(widget)]));

        let report = strip_metadata(&mut doc);
        assert_eq!(report.annotation_texts_cleared, 1, "nur /T am Widget");
        let page = doc.get_dictionary(page_id).unwrap();
        assert_eq!(
            page.get(b"Contents").ok(),
            Some(&Object::Reference(content))
        );
        assert!(doc.objects.contains_key(&content));
        assert_eq!(doc.get_pages().len(), 1);
    }

    /// Zwei Widgets teilen sich ein Elternfeld: dessen Texte fallen einmal
    /// und werden einmal gezählt — der Bericht nennt, was fiel, nicht, wie
    /// oft es erreicht wurde. Und `/Parent` ↔ `/Kids` ist ein Zyklus, der
    /// enden muss.
    #[test]
    fn a_shared_parent_field_is_cleaned_and_counted_once() {
        let mut doc = doc_with_info();
        let field = doc.new_object_id();
        let widgets: Vec<ObjectId> = (0..2)
            .map(|_| {
                doc.add_object(Object::Dictionary(dictionary! {
                    "Type" => "Annot",
                    "Subtype" => "Widget",
                    "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                    "Parent" => Object::Reference(field),
                }))
            })
            .collect();
        doc.objects.insert(
            field,
            Object::Dictionary(dictionary! {
                "FT" => "Btn",
                "T" => Object::string_literal("gruppe"),
                "TU" => Object::string_literal(format!("Konto {SECRET}")),
                "Kids" => widgets.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
            }),
        );
        let page_id = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(page_id).unwrap().set(
            "Annots",
            Object::Array(widgets.iter().map(|id| Object::Reference(*id)).collect()),
        );

        let report = strip_metadata(&mut doc);
        assert_eq!(report.annotation_texts_cleared, 2, "/T und /TU, je einmal");
        assert!(!doc.get_dictionary(field).unwrap().has(b"TU"));
        assert_no_leak(&doc, "/TU am geteilten Elternfeld");
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
