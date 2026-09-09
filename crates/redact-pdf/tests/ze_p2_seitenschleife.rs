//! Gegenprüfung P2 (Fix-Runde 4): der **aufgeschobene** Schreibvorgang.
//!
//! Seit `f982c12` werden Seiten nicht mehr in der Seitenschleife geschrieben,
//! sondern als `redact::PendingPage` gesammelt und erst nach der
//! Formularschleife geschrieben. Diese Datei fragt die naheliegende
//! Gegenfrage: wird **jede** Seite geschrieben, die geschrieben werden muss —
//! und keine doppelt?
//!
//! Orakel ist immer dreifach: `leaks` an der gespeicherten Ausgabe ist leer,
//! die Ausgabe lädt wieder, die Seitenzahl stimmt, und der sichtbare Text der
//! **nicht** geschwärzten Teile ist unverändert.

mod common;

use std::collections::BTreeSet;

use common::{page, text_ops, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, page_count, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
    RedactionReport,
};

// ---------------------------------------------------------------------------
// Bausteine
// ---------------------------------------------------------------------------

/// Textoperationen ab einer wählbaren Höhe — `common::text_ops` setzt immer
/// bei y=700 an, und zwei Texte an derselben Stelle machen jedes Rechteck
/// mehrdeutig.
fn text_ops_at(y: i32, lines: &[&str]) -> Vec<u8> {
    let mut out = format!("BT\n/F1 10 Tf\n72 {y} Td\n");
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

/// Ein Form-XObject mit `body`, unter `name` in den Seitenressourcen.
fn add_form(d: &mut Doc, name: &str, body: &str) -> ObjectId {
    let font_id = d.font_id;
    let form_resources = d.add(Object::Dictionary(
        dictionary! { "Font" => dictionary! { "F1" => font_id } },
    ));
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                "Resources" => form_resources,
            },
            body.as_bytes().to_vec(),
        )
        .with_compression(false),
    ));
    let resources = d.resources_id;
    let target = d.doc.get_dictionary_mut(resources).expect("Resources");
    let mut xobjects = target
        .get(b"XObject")
        .and_then(|o| o.as_dict())
        .cloned()
        .unwrap_or_default();
    xobjects.set(name, form_id);
    target.set("XObject", xobjects);
    form_id
}

/// Hängt eine weitere Seite an; `contents` ist der fertige `/Contents`-Wert.
fn add_page_with_contents(d: &mut Doc, contents: Object) -> ObjectId {
    let resources = d.resources_id;
    let pages_id = d.pages_id;
    let page_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => contents,
        "Resources" => resources,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    }));
    let pages = d.doc.get_dictionary_mut(pages_id).expect("Pages");
    let mut kids = pages
        .get(b"Kids")
        .and_then(|k| k.as_array())
        .cloned()
        .expect("Kids");
    kids.push(page_id.into());
    let count = kids.len() as i64;
    pages.set("Kids", kids);
    pages.set("Count", count);
    page_id
}

/// Weitere Seite mit eigenem Inhaltsstrom.
fn add_page(d: &mut Doc, content: &[u8]) -> ObjectId {
    let content_id = d.add(Object::Stream(
        Stream::new(dictionary! {}, content.to_vec()).with_compression(false),
    ));
    add_page_with_contents(d, Object::Reference(content_id))
}

// ---------------------------------------------------------------------------
// Orakel
// ---------------------------------------------------------------------------

fn analyse(bytes: &[u8]) -> (Vec<TextRun>, Vec<String>) {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion")
}

fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> (Vec<u8>, RedactionReport) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    (save_to_bytes(&doc).expect("Speichern"), report)
}

fn redactions_for(runs: &[TextRun], needle: &str) -> Vec<Redaction> {
    runs.iter()
        .filter_map(|run| {
            let pos = run.text.find(needle)?;
            let rect = run.rect_for_byte_range(pos, pos + needle.len())?;
            Some(Redaction::new(
                Region::new(
                    run.page,
                    rect,
                    Some(needle.to_string()),
                    Source::Pattern {
                        pattern_id: "iban_de".into(),
                        confidence: 1.0,
                    },
                ),
                Action::Blackout,
            ))
        })
        .collect()
}

/// Der sichtbare Text je Seite, ohne das Geheimnis und ohne Leerraum: das
/// Geheimnis fällt weg, alles andere muss stehen bleiben. Verglichen wird
/// je Seite als ein Text, denn ein Lauf darf durch die Schwärzung zerfallen.
fn visible_without_secret(runs: &[TextRun]) -> BTreeSet<(usize, String)> {
    let mut per_page: std::collections::BTreeMap<usize, String> = Default::default();
    for run in runs {
        let entry = per_page.entry(run.page).or_default();
        entry.push(' ');
        entry.push_str(&run.text);
    }
    per_page
        .into_iter()
        .map(|(page, text)| {
            let text = text.replace(SECRET, " ");
            (page, text.split_whitespace().collect::<Vec<_>>().join(" "))
        })
        .collect()
}

/// Das volle Orakel: kein Leck, gültige Datei, gleiche Seitenzahl, der
/// übrige sichtbare Text unverändert.
fn assert_clean(bytes: &[u8], label: &str) -> RedactionReport {
    let (runs_before, _) = analyse(bytes);
    let redactions = redactions_for(&runs_before, SECRET);
    assert!(!redactions.is_empty(), "{label}: nichts zu schwärzen");
    let (out, report) = pipeline(bytes, &redactions);

    let found = leaks(&out, SECRET);
    assert!(found.is_empty(), "{label}: Leck {found:?}");

    let before = load_from_bytes(bytes).expect("Eingabe ladbar");
    let after = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert_eq!(
        page_count(&after),
        page_count(&before),
        "{label}: Seitenzahl"
    );

    let (runs_after, _) = analyse(&out);
    assert_eq!(
        visible_without_secret(&runs_after),
        visible_without_secret(&runs_before),
        "{label}: sichtbarer Rest"
    );
    report
}

// ---------------------------------------------------------------------------
// 1, 2, 50 Seiten
// ---------------------------------------------------------------------------

fn many_pages(count: usize) -> Vec<u8> {
    let mut d = page(&["Kontoinhaber Max Mustermann", &format!("IBAN {SECRET}")]);
    for i in 1..count {
        let body = text_ops(&[
            &format!("Seite {} Kontoinhaber Max Mustermann", i + 1),
            &format!("IBAN {SECRET}"),
        ]);
        add_page(&mut d, &body);
    }
    d.finish()
}

#[test]
fn eine_zwei_fuenfzig_seiten_werden_alle_geschrieben() {
    for count in [1usize, 2, 50] {
        let bytes = many_pages(count);
        let report = assert_clean(&bytes, &format!("{count} Seiten"));
        assert_eq!(report.drawn_rects, count, "{count} Seiten: Deckrechtecke");
    }
}

// ---------------------------------------------------------------------------
// Geteilter und mehrteiliger Inhaltsstrom
// ---------------------------------------------------------------------------

/// Zwei Seiten, **ein** Inhaltsstrom (`/Contents 4 0 R` an beiden).
#[test]
fn zwei_seiten_ein_inhaltsstrom() {
    let mut d = page(&["Kontoinhaber Max Mustermann", &format!("IBAN {SECRET}")]);
    let shared = d.content_id;
    add_page_with_contents(&mut d, Object::Reference(shared));
    let bytes = d.finish();

    let (runs, _) = analyse(&bytes);
    assert_eq!(runs.iter().filter(|r| r.page == 1).count(), 2, "{runs:?}");
    assert_clean(&bytes, "geteilter Strom");
}

/// Eine Seite mit **mehreren** Inhaltsströmen (`/Contents [4 0 R 5 0 R]`).
/// Die Operationsindizes gelten für die Verkettung — genau daran hängt der
/// aufgeschobene Schreibvorgang.
#[test]
fn eine_seite_mehrere_inhaltsstroeme() {
    let mut d = page(&["Kontoinhaber Max Mustermann"]);
    let first = d.content_id;
    let second = d.add(Object::Stream(
        Stream::new(
            dictionary! {},
            text_ops_at(600, &[&format!("IBAN {SECRET}")]),
        )
        .with_compression(false),
    ));
    d.page_dict_set(
        "Contents",
        Object::Array(vec![Object::Reference(first), Object::Reference(second)]),
    );
    let bytes = d.finish();
    assert_clean(&bytes, "zwei Ströme auf einer Seite");
}

// ---------------------------------------------------------------------------
// Formulare quer über die Seiten
// ---------------------------------------------------------------------------

/// Ein Formular steht auf Seite 1 und 3, auf Seite 2 gar nicht. Auf Seite 1
/// liegt ein Spiegel darüber; geschwärzt wird über den Lauf von Seite 3.
#[test]
fn formular_auf_seite_1_und_3_nicht_auf_2() {
    let mut d = page(&[]);
    let form = add_form(
        &mut d,
        "Fm0",
        &format!("BT /F1 10 Tf 72 600 Td ({}) Tj ET", escape(SECRET)),
    );
    assert_ne!(form, d.page_id);
    // Seite 1: Spiegel über dem Formular.
    let mut raw = text_ops(&["Seite 1 Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!(
            "/Span <</ActualText ({})>> BDC\nq /Fm0 Do Q\nEMC\n",
            escape(SECRET)
        )
        .as_bytes(),
    );
    d.set_content(&raw);
    // Seite 2: kein Formular.
    add_page(&mut d, &text_ops(&["Seite 2 ohne Formular"]));
    // Seite 3: dasselbe Formular, ohne Spiegel.
    let mut raw3 = text_ops(&["Seite 3 Kontoinhaber Max Mustermann"]);
    raw3.extend_from_slice(b"q /Fm0 Do Q\n");
    add_page(&mut d, &raw3);
    let bytes = d.finish();

    assert_eq!(page_count(&load_from_bytes(&bytes).unwrap()), 3);
    assert_clean(&bytes, "Formular auf 1 und 3");
}

/// Ein Formular, das sich selbst zeichnet. Der Interpreter darf daran nicht
/// hängen bleiben, und die Seite muss trotzdem geschrieben werden.
#[test]
fn formular_das_sich_selbst_enthaelt() {
    let mut d = page(&[]);
    let form = add_form(
        &mut d,
        "Fm0",
        &format!(
            "BT /F1 10 Tf 72 600 Td ({}) Tj ET\nq /Fm0 Do Q\n",
            escape(SECRET)
        ),
    );
    // Das Formular hat sich selbst in seinen Ressourcen.
    let form_resources = d
        .doc
        .get_object(form)
        .and_then(|o| o.as_stream())
        .expect("Formular")
        .dict
        .get(b"Resources")
        .and_then(|o| o.as_reference())
        .expect("eigene Ressourcen");
    d.doc
        .get_dictionary_mut(form_resources)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Fm0" => form });

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    let bytes = d.finish();
    assert_clean(&bytes, "Zyklus");
}

/// Dasselbe Formular dient als Seiteninhalt (`Do`) **und** als
/// Erscheinungsstrom einer Annotation (`/AP /N`). Es wird einmal neu
/// geschrieben; danach darf an keiner der beiden Stellen etwas stehen.
#[test]
fn formular_ist_auch_annotationsstrom() {
    let mut d = page(&[]);
    let form = add_form(
        &mut d,
        "Fm0",
        &format!("BT /F1 10 Tf 72 600 Td ({}) Tj ET", escape(SECRET)),
    );
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);

    let annot = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "Rect" => vec![72.into(), 690.into(), 400.into(), 715.into()],
        "AP" => dictionary! { "N" => form },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot)]));
    let bytes = d.finish();
    assert_clean(&bytes, "Formular als /AP");
}

/// Eine Seite **ohne** eigenen Treffer, aber mit einem Spiegel über einem
/// Formular, das erst von einer **anderen** Seite getroffen wird. Das ist der
/// Fall, für den `PendingPage` gebaut wurde (Befund G1-A2): geschwärzt wird
/// allein über den Lauf von Seite 2 — so, wie ein Seitenbereich oder eine
/// Handauswahl es täte.
///
/// `properties_object` schaltet um, ob der Spiegel inline im Strom steht
/// (dann muss die Seite neu geschrieben werden) oder als eigenes Objekt in
/// `/Resources /Properties` (dann hängt alles an `clear_mirror_object`).
fn mirror_only_on_first_page(properties_object: bool) -> Vec<u8> {
    let mut d = page(&[]);
    add_form(
        &mut d,
        "Fm0",
        &format!("BT /F1 10 Tf 72 600 Td ({}) Tj ET", escape(SECRET)),
    );
    let bdc = if properties_object {
        let secret_obj = d.add(Object::string_literal(SECRET.to_string()));
        let props = d.add(Object::Dictionary(dictionary! {
            "ActualText" => Object::Reference(secret_obj),
        }));
        let resources = d.resources_id;
        d.doc
            .get_dictionary_mut(resources)
            .expect("Ressourcen")
            .set("Properties", dictionary! { "MC0" => props });
        "/Span /MC0 BDC\n".to_string()
    } else {
        format!("/Span <</ActualText ({})>> BDC\n", escape(SECRET))
    };
    let mut raw = text_ops(&["Seite 1"]);
    raw.extend_from_slice(bdc.as_bytes());
    raw.extend_from_slice(b"q /Fm0 Do Q\nEMC\n");
    d.set_content(&raw);
    let mut raw2 = text_ops(&["Seite 2"]);
    raw2.extend_from_slice(b"q /Fm0 Do Q\n");
    add_page(&mut d, &raw2);
    d.finish()
}

/// Nur die Läufe von Seite `page` (0-basiert) werden geschwärzt.
fn redactions_on_page(runs: &[TextRun], needle: &str, only: usize) -> Vec<Redaction> {
    redactions_for(
        &runs
            .iter()
            .filter(|run| run.page == only)
            .cloned()
            .collect::<Vec<_>>(),
        needle,
    )
}

fn assert_mirror_on_other_page_is_cleared(properties_object: bool, label: &str) {
    let bytes = mirror_only_on_first_page(properties_object);
    let (runs, _) = analyse(&bytes);
    let redactions = redactions_on_page(&runs, SECRET, 1);
    assert_eq!(redactions.len(), 1, "{label}: nur Seite 2 wird angefordert");
    let (out, _) = pipeline(&bytes, &redactions);
    let found = leaks(&out, SECRET);
    assert!(found.is_empty(), "{label}: {found:?}");
    let after = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert_eq!(page_count(&after), 2, "{label}: Seitenzahl");
}

#[test]
fn seite_ohne_treffer_mit_spiegel_ueber_getroffenem_formular() {
    assert_mirror_on_other_page_is_cleared(false, "Spiegel inline");
}

#[test]
fn seite_ohne_treffer_mit_spiegel_als_eigenem_objekt() {
    assert_mirror_on_other_page_is_cleared(true, "Spiegel als eigenes Objekt");
}

/// Eine Seite mit Treffer und **ohne** Spiegel neben leeren Seiten: der
/// Regelfall darf durch die Umstellung nicht anders ausfallen.
#[test]
fn treffer_ohne_spiegel_zwischen_leeren_seiten() {
    let mut d = page(&[]);
    d.set_content(b"");
    add_page(&mut d, b"");
    add_page(
        &mut d,
        &text_ops(&["Kontoinhaber Max Mustermann", &format!("IBAN {SECRET}")]),
    );
    add_page(&mut d, b"");
    let bytes = d.finish();
    let report = assert_clean(&bytes, "leere Nachbarn");
    assert_eq!(report.drawn_rects, 1);
}

/// Keine Seite wird **doppelt** geschrieben: je Seite genau ein
/// Deckrechteck, und der Inhaltsstrom trägt es genau einmal.
#[test]
fn keine_seite_wird_doppelt_geschrieben() {
    let bytes = many_pages(3);
    let (runs, _) = analyse(&bytes);
    let redactions = redactions_for(&runs, SECRET);
    assert_eq!(redactions.len(), 3);
    let (out, report) = pipeline(&bytes, &redactions);
    assert_eq!(report.drawn_rects, 3, "je Seite ein Rechteck");

    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    for (index, page_id) in doc.get_pages().values().enumerate() {
        let content = String::from_utf8_lossy(&redact_pdf::filters::page_content(&doc, *page_id))
            .into_owned();
        assert_eq!(
            content.matches(" re\n").count(),
            1,
            "Seite {}: {content}",
            index + 1
        );
    }
}

// ---------------------------------------------------------------------------
// Der Seiteninhalt bleibt streng — auch neben dem nachsichtigen Orakel
// ---------------------------------------------------------------------------

/// Eine Kette mit unbekanntem Glied: `[/ASCIIHexDecode /FlateDecode
/// /DCTDecode]`. Nach zwei Filtern steht der **fertige Seiteninhalt** da,
/// samt Geheimnis und samt gültiger PDF-Syntax; das dritte Glied ist ein
/// Bildfilter, den dieses Modul bewusst nicht dekodiert.
///
/// Genau hier laufen die beiden Leser auseinander (Fix-Runde 5, Befund P1-1):
///
/// * Das **Orakel** (`leaks`, `filters::decoded_prefix_within`) ist
///   nachsichtig und findet das Geheimnis im entzifferbaren Anfang.
/// * Der **Interpreter** (`filters::page_content` für `content::scan_page`,
///   `ops::page_ops` und `image`) bleibt streng: er nimmt diesen Anfang
///   **nicht** als Seiteninhalt. Was hinter dem unbekannten Glied steht, ist
///   keine PDF-Syntax; eine daraus gelesene Seite wäre erfunden, und eine
///   Schwärzung darauf hätte einen anderen Strom vor sich als der Betrachter.
///
/// Die Gegenprobe zur Strenge ist der Abbruch: die Datei wird abgelehnt, statt
/// als „nichts gefunden“ durchzugehen.
#[test]
fn halb_dekodierter_strom_wird_nie_seiteninhalt() {
    use std::io::Write;

    // Ein Inline-Bild gehört mit hinein: `image` liest denselben Seiteninhalt
    // und darf ebenfalls nichts daraus machen.
    let plain = format!(
        "BT\n/F1 10 Tf\n72 700 Td\n(IBAN {SECRET}) Tj\nET\n\
         q 10 0 0 10 0 0 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \u{0} EI Q\n"
    );
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(plain.as_bytes()).expect("deflate");
    let packed = common::ascii_hex_encode(&encoder.finish().expect("deflate"));

    let mut d = page(&[]);
    d.doc.objects.insert(
        d.content_id,
        Object::Stream(
            Stream::new(
                dictionary! { "Filter" => Object::Array(vec![
                    "ASCIIHexDecode".into(),
                    "FlateDecode".into(),
                    "DCTDecode".into(),
                ]) },
                packed,
            )
            .with_compression(false),
        ),
    );
    let bytes = d.finish();

    // Das Orakel sieht das Geheimnis — sonst prüft der Test die falsche Datei.
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "das Orakel findet den entzifferbaren Anfang nicht"
    );

    let doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let page_id = *doc.get_pages().values().next().expect("eine Seite");

    // Der Interpreter liest den Anfang nicht: `page_content` gibt die
    // Rohbytes zurück, nicht die halb dekodierte Sicht.
    let content = redact_pdf::filters::page_content(&doc, page_id);
    let as_text = String::from_utf8_lossy(&content);
    assert!(
        !as_text.contains(SECRET),
        "halb dekodiert gelesen: {as_text}"
    );
    assert!(
        !as_text.contains(" Tj"),
        "halb dekodiert gelesen: {as_text}"
    );
    // Und damit sieht auch `image` (dieselbe Quelle) kein `Do`.
    assert!(
        !redact_pdf::image::page_has_images(&doc, page_id),
        "ein Bild aus einem halb dekodierten Strom"
    );

    // Statt still weiterzulaufen: Abbruch. Weder Extraktion noch Schwärzung
    // geben eine Seite aus, die niemand gelesen hat.
    let extracted = PdfExtractor::new().extract_with_warnings(&doc);
    assert!(extracted.is_err(), "die Seite ging als lesbar durch");
    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    assert!(
        PdfRedactor::new().apply_with_report(&mut doc, &[]).is_err(),
        "die Schwärzung lief über eine ungelesene Seite"
    );
}
