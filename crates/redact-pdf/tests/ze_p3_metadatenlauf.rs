//! Gegenprüfung Fix-Runde 4 (P3): der Metadatenlauf — `meta.rs`,
//! `document::prune_unreachable`, die Zahlen im Audit-Log.
//!
//! Zwei Sorten Tests:
//!
//! * gewöhnliche Tests (grün): sie halten fest, was der Lauf **nicht**
//!   kaputt macht — eine Seite, die nur über ein Formularfeld erreichbar
//!   ist, ein ausdrückliches Sprungziel, ein Erscheinungsstrom, und dass
//!   Zyklen und große Bäume in vertretbarer Zeit enden;
//! * die **Befunde** P3-1 bis P3-6: Material, an dem das Geheimnis nach
//!   `strip_metadata` + `save_to_bytes` noch in der Datei steht, oder an dem
//!   eine Zahl des Berichts etwas meldet, das nicht geschah. Sie sind
//!   `#[ignore]` und rot, bis die Lücke geschlossen ist
//!   (`cargo test -p redact-pdf --test ze_p3_metadatenlauf -- --ignored`).
//!
//! Orakel ist [`redact_pdf::leaks`] an den geschriebenen Bytes — dieselbe
//! Prüfung, die `redact-rs --check-leaks` fährt.

mod common;

use std::time::{Duration, Instant};

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport};

fn strip(bytes: &[u8]) -> (MetadataReport, Vec<u8>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = strip_metadata(&mut doc);
    (report, save_to_bytes(&doc).expect("Speichern"))
}

fn rect() -> Object {
    Object::Array(vec![100.into(), 600.into(), 300.into(), 640.into()])
}

fn secret_string() -> Object {
    Object::string_literal(format!("Notiz {SECRET}"))
}

#[track_caller]
fn assert_gone(bytes: &[u8], what: &str) {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "{what}: die Probe muss das Geheimnis vorher tragen, sonst misst der Test nichts"
    );
    let (_, out) = strip(bytes);
    let hits = leaks(&out, SECRET);
    assert!(
        hits.is_empty(),
        "{what}: nach der Verarbeitung steht die IBAN noch in der Datei:\n{}",
        hits.join("\n")
    );
}

fn text_of(bytes: &[u8]) -> String {
    let doc = load_from_bytes(bytes).expect("Ausgabe ladbar");
    redact_pdf::PdfExtractor::new()
        .extract(&doc)
        .expect("Text lesbar")
        .iter()
        .map(|run| run.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// A) Der Graphlauf darf gewöhnlichen Dateien nichts nehmen
// ---------------------------------------------------------------------------

/// Ein Formularfeld, dessen `/Kids` neben dem Widget auch die **Seite**
/// nennt (kaputt geschriebene Datei, wie sie vorkommt): der Lauf läuft über
/// `/Parent` und `/Kids` bis dorthin. Die Seite trägt `/Type /Page` und ist
/// damit kein Träger — ihr `/Contents` muss stehen bleiben, sonst ist die
/// Ausgabe leer.
///
/// Mutationsnachweis: in `is_carrier` den Namensvergleich aufgehoben
/// (`Ok(Object::Name(name)) => name == b"Annot"` → `Ok(Object::Name(_)) =>
/// true`) — dann verliert die Seite ihren Inhalt und der Test ist rot.
#[test]
fn seite_ueber_kids_eines_feldes_behaelt_ihren_inhalt() {
    let mut d = page(&["Rechnung Nr. 4711", "Betrag 100 EUR"]);
    let page_id = d.page_id;
    let field = d.doc.new_object_id();
    let widget = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "Rect" => rect(),
        "Parent" => Object::Reference(field), "FT" => "Tx",
    }));
    d.doc.objects.insert(
        field,
        Object::Dictionary(dictionary! {
            "FT" => "Tx",
            "T" => Object::string_literal("name"),
            // /Kids zeigt auf das Widget UND auf die Seite.
            "Kids" => Object::Array(vec![Object::Reference(widget), Object::Reference(page_id)]),
        }),
    );
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(widget)]));

    let bytes = d.finish();
    let (_, out) = strip(&bytes);
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert_eq!(doc.get_pages().len(), 1, "die Seite muss bleiben");
    assert!(
        doc.get_dictionary(page_id).expect("Seite").has(b"Contents"),
        "der Seiteninhalt wurde als Annotationstext behandelt und entfernt"
    );
    let text = text_of(&out);
    assert!(
        text.contains("Rechnung Nr. 4711") && text.contains("Betrag 100 EUR"),
        "der Seitentext fehlt nach dem Metadatenlauf: {text:?}"
    );
}

/// Dasselbe für den Seitenbaum und für ein Encoding-Dictionary **ohne**
/// `/Type` (dort ist `/Type` optional, PDF 32000-1, Tabelle 114): der Lauf
/// erreicht es über eine kaputte `/Parent`-Kette, findet aber keinen seiner
/// Schlüssel — `/BaseEncoding` und `/Differences` bleiben stehen.
#[test]
fn schrift_und_seitenbaum_ueberstehen_eine_kaputte_parent_kette() {
    let mut d = page(&["Rechnung Nr. 4711"]);
    let encoding = d.add(Object::Dictionary(dictionary! {
        "BaseEncoding" => "WinAnsiEncoding",
        "Differences" => Object::Array(vec![32.into(), Object::Name(b"space".to_vec())]),
    }));
    d.doc
        .get_dictionary_mut(d.font_id)
        .expect("Schrift")
        .set("Encoding", Object::Reference(encoding));
    let pages_id = d.pages_id;
    let annot = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
        "Contents" => Object::string_literal("harmlos"),
        "Parent" => Object::Reference(pages_id),
        "IRT" => Object::Reference(encoding),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot)]));

    let font_id = d.font_id;
    let (_, out) = strip(&d.finish());
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert_eq!(doc.get_pages().len(), 1);
    let enc = doc.get_dictionary(encoding).expect("Encoding-Dictionary");
    assert!(
        enc.has(b"BaseEncoding") && enc.has(b"Differences"),
        "dem Encoding-Dictionary wurden Schlüssel genommen"
    );
    assert!(
        doc.get_dictionary(font_id).is_ok(),
        "die Schrift wurde weggeräumt"
    );
    assert!(text_of(&out).contains("Rechnung Nr. 4711"));
}

/// Ein gewöhnliches Formular ohne Geheimnis: ein Widget mit
/// Erscheinungsstrom, ein Link mit **ausdrücklichem** Ziel
/// (`[Seite /XYZ …]`), Schrift im `/DR`. Nach dem Lauf müssen Seitenzahl,
/// Seitentext, der Erscheinungsstrom und das Sprungziel unverändert
/// dastehen.
///
/// Mutationsnachweis: die Ziel-Ausnahme entfernt
/// (`if !keep_dest && take(dict, b"Dest")` → `if take(dict, b"Dest")`) —
/// dann fällt das ausdrückliche Ziel und der Test ist rot.
#[test]
fn gewoehnliches_formular_verliert_weder_bild_noch_sprungziel() {
    let mut d = page(&["Antrag auf Kindergeld", "Name: Erika Mustermann"]);
    let page_id = d.page_id;
    let ap = d.add(Object::Stream(lopdf::Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => Object::Array(vec![0.into(), 0.into(), 200.into(), 40.into()]),
        },
        b"0 0 0 rg 0 0 200 40 re f".to_vec(),
    )));
    let field = d.doc.new_object_id();
    let widget = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "Rect" => rect(),
        "Parent" => Object::Reference(field), "F" => 4_i64,
        "AP" => dictionary! { "N" => Object::Reference(ap) },
        "DA" => Object::string_literal("/F1 10 Tf 0 g"),
    }));
    d.doc.objects.insert(
        field,
        Object::Dictionary(dictionary! {
            "FT" => "Tx", "T" => Object::string_literal("name"),
            "Kids" => Object::Array(vec![Object::Reference(widget)]),
        }),
    );
    let link = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(),
        "Dest" => Object::Array(vec![
            Object::Reference(page_id), Object::Name(b"XYZ".to_vec()),
            72.into(), 700.into(), Object::Null,
        ]),
    }));
    let acro = d.add(Object::Dictionary(dictionary! {
        "Fields" => Object::Array(vec![Object::Reference(field)]),
        "DR" => dictionary! { "Font" => dictionary! { "F1" => Object::Reference(d.font_id) } },
    }));
    d.catalog_set("AcroForm", Object::Reference(acro));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(widget), Object::Reference(link)]),
    );

    let (_, out) = strip(&d.finish());
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert_eq!(doc.get_pages().len(), 1, "Seitenzahl");
    let text = text_of(&out);
    assert!(
        text.contains("Antrag auf Kindergeld") && text.contains("Erika Mustermann"),
        "Seitentext verändert: {text:?}"
    );
    assert!(
        doc.get_object(ap).is_ok(),
        "der Erscheinungsstrom des Widgets wurde weggeräumt — das Formular ist nicht mehr anzeigbar"
    );
    assert!(
        doc.get_dictionary(widget).expect("Widget").has(b"AP"),
        "dem Widget wurde sein /AP genommen"
    );
    assert!(
        doc.get_dictionary(link).expect("Link").has(b"Dest"),
        "das ausdrückliche Sprungziel wurde entfernt"
    );
}

// ---------------------------------------------------------------------------
// B) Grenzen: Zyklen, lange Ketten, breite Bäume
// ---------------------------------------------------------------------------

/// Zyklus `/Popup` ↔ `/Parent` und `/Kids` auf sich selbst, dazu `/Annots`
/// als Verweis auf sich selbst: der Lauf endet.
///
/// Mutationsnachweis: die Besuchsmenge ausgehängt
/// (`if !visited.insert(id) { continue; }` → `visited.insert(id);`) — dann
/// endet der Test nicht mehr (Abbruch durch `timeout`).
#[test]
fn zyklen_enden() {
    let mut d = page(&["A"]);
    let a = d.doc.new_object_id();
    let b = d.doc.new_object_id();
    d.doc.objects.insert(
        a,
        Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
            "Popup" => Object::Reference(b),
            "Contents" => Object::string_literal("a"),
        }),
    );
    d.doc.objects.insert(
        b,
        Object::Dictionary(dictionary! {
            "Subtype" => "Popup", "Rect" => rect(),
            "Parent" => Object::Reference(a),
            "Kids" => Object::Array(vec![Object::Reference(a), Object::Reference(b)]),
            "Contents" => Object::string_literal("b"),
        }),
    );
    let selbst = d.doc.new_object_id();
    d.doc.objects.insert(selbst, Object::Reference(selbst));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![
            Object::Reference(a),
            Object::Reference(b),
            Object::Reference(selbst),
        ]),
    );
    let start = Instant::now();
    let report = strip_metadata(&mut d.doc);
    assert!(start.elapsed() < Duration::from_secs(5), "Zyklus hängt");
    assert_eq!(report.annotation_texts_cleared, 2, "je Träger einmal");
}

/// Eine `/Parent`-Kette von 200 000 Feldern und ein `/Kids`-Fächer
/// (5 Ebenen × 500 Knoten, jede Ebene vollständig auf die nächste) enden in
/// Sekunden. Daneben gemessen (Release, außerhalb dieses Tests): eine
/// `/Parent`-Kette von 1 000 000 Feldern in 1,92 s, ein Fächer aus
/// 6 Ebenen × 1 000 Knoten (5 000 000 Kanten) in 0,40 s, 100 000
/// Annotationen in 0,45 s.
#[test]
fn lange_ketten_und_breite_faecher_enden_in_sekunden() {
    // Kette
    let mut d = page(&["A"]);
    let n = 200_000usize;
    let ids: Vec<ObjectId> = (0..n).map(|_| d.doc.new_object_id()).collect();
    for (i, id) in ids.iter().enumerate() {
        let mut dict = dictionary! { "T" => Object::string_literal("f") };
        if i > 0 {
            dict.set("Parent", Object::Reference(ids[i - 1]));
        }
        d.doc.objects.insert(*id, Object::Dictionary(dict));
    }
    let last = *ids.last().expect("Kette");
    let tail = d.doc.get_dictionary_mut(last).expect("letztes Feld");
    tail.set("Type", Object::Name(b"Annot".to_vec()));
    tail.set("Rect", rect());
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(last)]));
    let start = Instant::now();
    let report = strip_metadata(&mut d.doc);
    let kette = start.elapsed();
    assert_eq!(report.annotation_texts_cleared, n, "jedes Feld einmal");
    assert!(
        kette < Duration::from_secs(60),
        "Kette zu langsam: {kette:?}"
    );

    // Fächer
    let levels = 5usize;
    let per = 500usize;
    let mut d = page(&["A"]);
    let ids: Vec<Vec<ObjectId>> = (0..levels)
        .map(|_| (0..per).map(|_| d.doc.new_object_id()).collect())
        .collect();
    for level in 0..levels {
        for (k, id) in ids[level].iter().enumerate() {
            let mut dict = dictionary! { "T" => Object::string_literal(format!("n{level}_{k}")) };
            if level + 1 < levels {
                dict.set(
                    "Kids",
                    Object::Array(
                        ids[level + 1]
                            .iter()
                            .map(|i| Object::Reference(*i))
                            .collect::<Vec<_>>(),
                    ),
                );
            }
            d.doc.objects.insert(*id, Object::Dictionary(dict));
        }
    }
    let root = ids[0][0];
    let head = d.doc.get_dictionary_mut(root).expect("Wurzel");
    head.set("Type", Object::Name(b"Annot".to_vec()));
    head.set("Rect", rect());
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(root)]));
    let start = Instant::now();
    let report = strip_metadata(&mut d.doc);
    let faecher = start.elapsed();
    assert_eq!(
        report.annotation_texts_cleared,
        1 + (levels - 1) * per,
        "jeder erreichbare Knoten einmal"
    );
    assert!(
        faecher < Duration::from_secs(60),
        "Fächer zu langsam: {faecher:?}"
    );
}

// ---------------------------------------------------------------------------
// Befunde
// ---------------------------------------------------------------------------

/// **P3-1**: ein `/Popup` als **eingebettetes** Dictionary an der Annotation.
/// `linked_ids` trägt nur Verweise (und Verweise in Feldern) in den Stapel —
/// ein direkt eingebettetes Dictionary wird nie bereinigt. Die
/// Modul-Dokumentation verspricht „ein direkt eingebettetes Dictionary wird
/// im Feld selbst bereinigt“; das gilt nur für `/Annots` selbst.
#[test]
#[ignore = "Befund P3-1: eingebettetes /Popup behält seinen Klartext"]
fn befund_p3_1_eingebettetes_popup() {
    let mut d = page(&["Rechnung Nr. 4711"]);
    let annot = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
        "Contents" => Object::string_literal("harmlos"),
        "Popup" => Object::Dictionary(dictionary! {
            "Subtype" => "Popup", "Rect" => rect(), "Contents" => secret_string(),
        }),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot)]));
    assert_gone(&d.finish(), "eingebettetes /Popup");
}

/// **P3-2**: `/Kids 9 0 R` — das Feld als eigenes Array-Objekt, darin ein
/// **eingebettetes** Widget-Dictionary. Im Array-Zweig von
/// `clean_annotations` werden ausschließlich Verweise weiterverfolgt; das
/// eingebettete Dictionary bleibt mit seinem `/TU` stehen und wird von
/// `prune_unreachable` gehalten, weil das Array erreichbar ist.
#[test]
#[ignore = "Befund P3-2: eingebettetes Dictionary in einem /Kids-Array bleibt unberührt"]
fn befund_p3_2_eingebettetes_dictionary_in_einem_kids_array() {
    let mut d = page(&["Rechnung Nr. 4711"]);
    let field = d.doc.new_object_id();
    let widget = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "Rect" => rect(),
        "Parent" => Object::Reference(field), "FT" => "Tx",
    }));
    let kids = d.add(Object::Array(vec![
        Object::Reference(widget),
        Object::Dictionary(dictionary! {
            "Subtype" => "Widget", "Rect" => rect(), "TU" => secret_string(),
        }),
    ]));
    d.doc.objects.insert(
        field,
        Object::Dictionary(dictionary! {
            "FT" => "Tx", "T" => Object::string_literal("feld"),
            "Kids" => Object::Reference(kids),
        }),
    );
    let acro = d.add(Object::Dictionary(
        dictionary! { "Fields" => Object::Array(vec![Object::Reference(field)]) },
    ));
    d.catalog_set("AcroForm", Object::Reference(acro));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(widget)]));
    assert_gone(&d.finish(), "eingebettetes Dictionary in /Kids");
}

/// **P3-3**: der Feldwert `/V` jenseits von 32 Ebenen. `clean_annotations`
/// läuft die Kette **ohne** Tiefengrenze ab (und leert dort 70 Feldnamen),
/// `collect_field_ids` (von `/AcroForm /Fields` nach unten) und
/// `collect_widget_fields` (vom Widget nach oben) hören beide nach
/// `MAX_TREE_DEPTH = 32` auf. Ein `/V` in der Mitte einer 70 Felder langen
/// Kette wird von keinem der beiden erreicht und bleibt stehen.
///
/// Die Kontrolle in derselben Prüfung: dieselbe Datei mit einer Kette der
/// Länge 10 verliert ihr `/V`.
#[test]
#[ignore = "Befund P3-3: /V jenseits der 32 Ebenen bleibt stehen"]
fn befund_p3_3_feldwert_jenseits_der_32_ebenen() {
    fn kette(len: usize, wert_bei: usize) -> Doc {
        let mut d = page(&["Rechnung Nr. 4711"]);
        let ids: Vec<ObjectId> = (0..len).map(|_| d.doc.new_object_id()).collect();
        for (i, id) in ids.iter().enumerate() {
            let mut dict =
                dictionary! { "FT" => "Tx", "T" => Object::string_literal(format!("f{i}")) };
            if i > 0 {
                dict.set("Parent", Object::Reference(ids[i - 1]));
            }
            if i + 1 < len {
                dict.set("Kids", Object::Array(vec![Object::Reference(ids[i + 1])]));
            }
            if i == wert_bei {
                dict.set("V", secret_string());
            }
            if i + 1 == len {
                dict.set("Type", Object::Name(b"Annot".to_vec()));
                dict.set("Subtype", Object::Name(b"Widget".to_vec()));
                dict.set("Rect", rect());
            }
            d.doc.objects.insert(*id, Object::Dictionary(dict));
        }
        let acro = d.add(Object::Dictionary(
            dictionary! { "Fields" => Object::Array(vec![Object::Reference(ids[0])]) },
        ));
        d.catalog_set("AcroForm", Object::Reference(acro));
        d.page_dict_set(
            "Annots",
            Object::Array(vec![Object::Reference(*ids.last().expect("Kette"))]),
        );
        d
    }

    // Kontrolle: kurze Kette, der Wert fällt.
    let kurz = kette(10, 5);
    assert_gone(&kurz.finish(), "Kette der Länge 10");

    // Befund: 70 Felder, der Wert sitzt in der Mitte.
    assert_gone(&kette(70, 35).finish(), "Kette der Länge 70");
}

/// **P3-4**: `outlines_removed` zählt **vor** dem Entfernen und meldet
/// deshalb eine Entfernung, die nicht stattfand. Ein Lesezeichen, das außer
/// im `/Outlines`-Baum noch an einer zweiten Stelle hängt, überlebt
/// `prune_unreachable` — samt `/Title`. Der Bericht sagt trotzdem „1
/// Lesezeichen entfernt“, und `MetadataReport` verspricht: „Jeder Eintrag
/// ist ein *gemessenes* Ergebnis, kein Vorsatz“.
#[test]
#[ignore = "Befund P3-4: gezähltes Lesezeichen bleibt samt /Title in der Datei"]
fn befund_p3_4_gezaehltes_lesezeichen_bleibt() {
    let mut d = page(&["Rechnung Nr. 4711"]);
    let item = d.doc.new_object_id();
    let root = d.add(Object::Dictionary(dictionary! {
        "Type" => "Outlines", "First" => Object::Reference(item),
        "Last" => Object::Reference(item), "Count" => 1_i64,
    }));
    d.doc.objects.insert(
        item,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal(format!("Kontoauszug {SECRET}")),
            "Parent" => Object::Reference(root),
        }),
    );
    d.catalog_set("Outlines", Object::Reference(root));
    d.catalog_set("Zusatz", Object::Reference(item));

    let bytes = d.finish();
    let (report, out) = strip(&bytes);
    let hits = leaks(&out, SECRET);
    assert_eq!(
        report.outlines_removed, 1,
        "der Bericht meldet eine Entfernung"
    );
    assert!(
        hits.is_empty(),
        "gemeldet „1 Lesezeichen entfernt“, tatsächlich steht es noch da:\n{}",
        hits.join("\n")
    );
}

/// **P3-5**: `reachable_objects` nimmt den **ganzen** Trailer als Wurzel.
/// Ein Objekt unter einem erfundenen Trailerschlüssel überlebt damit jeden
/// Lauf; der Trailer selbst wird nirgends auf die zulässigen Schlüssel
/// (`/Root`, `/Info`, `/Encrypt`, `/ID`, `/Size`) beschnitten.
#[test]
#[ignore = "Befund P3-5: Objekt unter einem fremden Trailerschlüssel überlebt"]
fn befund_p3_5_objekt_unter_fremdem_trailerschluessel() {
    let mut d = page(&["Rechnung Nr. 4711"]);
    let id = d.add(Object::Dictionary(
        dictionary! { "Foo" => Object::string_literal(SECRET) },
    ));
    d.doc.trailer.set("Zusatz", Object::Reference(id));
    assert_gone(&d.finish(), "fremder Trailerschlüssel");
}

/// **P3-6**: `collect_references` bricht bei `MAX_DIRECT_DEPTH = 64` ab und
/// zählt alles darunter als **unerreichbar**. `prune_unreachable` wirft das
/// referenzierte Objekt dann weg — eine Erreichbarkeitsprüfung, die abbricht,
/// muss im Zweifel „erreichbar“ sagen, nicht „weg damit“.
#[test]
#[ignore = "Befund P3-6: Verweis hinter 65 Ebenen direkter Verschachtelung wird weggeräumt"]
fn befund_p3_6_verweis_hinter_tiefer_verschachtelung() {
    let mut d = page(&["Rechnung Nr. 4711"]);
    let gebraucht = d.add(Object::Stream(lopdf::Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => Object::Array(vec![0.into(), 0.into(), 10.into(), 10.into()]),
        },
        b"q Q".to_vec(),
    )));
    let mut nested = Object::Reference(gebraucht);
    for _ in 0..70 {
        nested = Object::Array(vec![nested]);
    }
    d.catalog_set("Tief", nested);
    let mut doc = load_from_bytes(&d.finish()).expect("PDF ladbar");
    assert!(doc.objects.contains_key(&gebraucht), "vorher da");
    strip_metadata(&mut doc);
    assert!(
        doc.objects.contains_key(&gebraucht),
        "ein referenziertes Objekt wurde weggeräumt, weil die Erreichbarkeitsprüfung \
         bei Tiefe 64 abbricht"
    );
}
