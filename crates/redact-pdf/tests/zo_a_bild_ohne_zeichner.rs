//! Spur-A-Runde 1, Prüfer A — **Klassen, die nicht auf der Probenliste
//! stehen**: Bilder, die der Bildlauf (`image.rs::scan_page_images` über
//! `content::interpret`) nie zu Gesicht bekommt, obwohl ihre Bildpunkte in der
//! Ausgabedatei stehen.
//!
//! Drei Wege, auf denen ein Bild ohne `Do` im Seitenstrom in der Datei liegt:
//!
//! 1. **`/Thumb`** — das Vorschaubild der Seite (PDF 32000-1, Tabelle 30). Ein
//!    Raster der ganzen Seite, samt allem, was darauf steht. Niemand zeichnet
//!    es mit `Do`, `strip_metadata` kennt den Schlüssel nicht.
//! 2. **Kachelmuster** (`/Pattern`, `PatternType 1`) — `content.rs::
//!    scan_tiling_pattern` verließ ein Muster **ohne Textoperator** vor dem
//!    Durchlaufen („Schraffur- oder Logomuster: kein Befund“). Ein Bild darin
//!    wurde nie gemeldet und nie gefüllt (Register #75, behoben: der Ausstieg
//!    fragt jetzt auch nach `Do` und `BI`).
//! 3. **Nicht gezeichneter Name** — ein Bild, das nach dem Kopieren
//!    (`Fate::Copy`) unter einem Namen erreichbar bleibt, den niemand zeichnet:
//!    im geerbten `/Resources` des `/Pages`-Knotens oder als überzähliger
//!    Eintrag der Seite. `prune_unreachable` sieht es als erreichbar; die
//!    Klartext-Bildpunkte blieben in der Datei (Register #76, behoben durch
//!    `image.rs::retire_copied_originals`).
//!
//! Maßstab: `redact_pdf::leaks` an den **Ausgabebytes** — die Bildpunkte tragen
//! den Suchbegriff buchstäblich als Abtastwerte, die Ströme sind unkomprimiert.
//! Die Tests, die einen Befund festhalten, sind **absichtlich rot**; der
//! Doktext sagt jeweils, was sie festhalten.
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-pdf --test zo_a_bild_ohne_zeichner`

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, save_to_bytes, strip_metadata, PdfRedactor, RedactionReport};

/// Steht buchstäblich in den Bildpunkten: 27 Zeichen = 9 RGB-Bildpunkte.
const GEHEIM: &str = "DE89 3704 0044 0532 0130 00";

/// 9 x 4 RGB, unkomprimiert, Zeile 1 trägt `GEHEIM` als Abtastwerte.
fn bild_mit_klartext() -> Stream {
    let (breite, hoehe) = (9u32, 4u32);
    let mut data = Vec::with_capacity((breite * hoehe * 3) as usize);
    for y in 0..hoehe {
        if y == 1 {
            data.extend_from_slice(GEHEIM.as_bytes());
            continue;
        }
        for x in 0..breite {
            data.extend_from_slice(&[(x * 9 + 1) as u8, (y * 5 + 2) as u8, 0x30]);
        }
    }
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(breite),
            "Height" => i64::from(hoehe),
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
        },
        data,
    )
    .with_compression(false)
}

/// Seiten mit **eigenen** Ressourcen (je Seite ein Verweis).
fn seiten(mut doc: Document, seiten: Vec<(ObjectId, Vec<u8>)>) -> (Document, Vec<ObjectId>) {
    let pages_id = doc.new_object_id();
    let mut ids = Vec::new();
    for (resources_id, content) in seiten {
        let content_id = doc.add_object(Stream::new(dictionary! {}, content));
        ids.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 842.into(), 842.into()],
        }));
    }
    let count = ids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
            "Count" => count,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, ids)
}

/// Seiten, deren Ressourcen **vom `/Pages`-Knoten geerbt** werden — regulär
/// nach PDF 32000-1, Tabelle 30, und bei vielen Erzeugern die Regel: ein
/// Verzeichnis für das ganze Dokument.
fn seiten_mit_geerbten_ressourcen(
    mut doc: Document,
    resources_id: ObjectId,
    inhalte: Vec<Vec<u8>>,
) -> (Document, Vec<ObjectId>) {
    let pages_id = doc.new_object_id();
    let mut ids = Vec::new();
    for content in inhalte {
        let content_id = doc.add_object(Stream::new(dictionary! {}, content));
        ids.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 842.into(), 842.into()],
        }));
    }
    let count = ids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
            "Count" => count,
            "Resources" => resources_id,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, ids)
}

fn schwaerzung(page: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "Spur A, Runde 1, Prüfer A".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Wie die Verarbeitungskette: schwärzen, Metadaten strippen, speichern.
fn lauf(doc: &mut Document, list: &[Redaction]) -> (RedactionReport, Vec<u8>) {
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(doc, list)
        .expect("Schwärzung läuft");
    strip_metadata(doc);
    let bytes = save_to_bytes(doc).expect("Speichern");
    (report, bytes)
}

fn vorbedingung_orakel_findet(doc: &Document) {
    let vorher = save_to_bytes(doc).expect("Speichern");
    assert!(
        !leaks(&vorher, GEHEIM).is_empty(),
        "Vorbedingung: das Orakel findet die Klartext-Bildpunkte in der Eingabe"
    );
}

// ===========================================================================
// 1. /Thumb
// ===========================================================================

/// **Stilles Leck.** Die Seite trägt ein Vorschaubild (`/Thumb`) — ein Raster
/// der Seite, hier mit den Klartext-Bildpunkten. Der Seiteninhalt selbst wird
/// unter der Zone geschwärzt (ein Bild, das `Do` zeichnet). Erwartung: nach
/// Schwärzung **und** `strip_metadata` findet das Orakel den Klartext nicht
/// mehr — oder wenigstens eine Warnung sagt, dass das Vorschaubild steht.
///
/// Befund: `leaks` findet die Bildpunkte des `/Thumb`, der Bericht ist leer.
/// Vermutung: `/Thumb` kommt weder in `image.rs` (kein `Do`) noch in `meta.rs`
/// (kein Schlüssel der Liste) vor.
#[test]
fn vorschaubild_der_seite_ueberlebt_den_lauf_ohne_warnung() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let thumb_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, ids) = seiten(
        doc,
        vec![(res, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec())],
    );
    doc.get_dictionary_mut(ids[0])
        .expect("Seite")
        .set("Thumb", Object::Reference(thumb_id));
    vorbedingung_orakel_findet(&doc);

    let (report, out) = lauf(
        &mut doc,
        &[schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0))],
    );
    assert_eq!(
        report.redacted_images, 1,
        "das gezeichnete Bild fällt: {:?}",
        report.warnings
    );

    let funde = leaks(&out, GEHEIM);
    let gewarnt = report
        .warnings
        .iter()
        .any(|w| w.contains("Thumb") || w.contains("Vorschau"));
    assert!(
        funde.is_empty() || gewarnt,
        "das Vorschaubild /Thumb überlebt mit Klartext-Bildpunkten, ohne Warnung: \
         {funde:?} — Warnungen: {:?}",
        report.warnings
    );
}

// ===========================================================================
// 2. Bild im Kachelmuster
// ===========================================================================

fn kachelmuster_mit_bild(doc: &mut Document, bild_id: ObjectId, inhalt: &[u8]) -> ObjectId {
    let muster_res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    doc.add_object(Stream::new(
        dictionary! {
            "Type" => "Pattern",
            "PatternType" => 1_i64,
            "PaintType" => 1_i64,
            "TilingType" => 1_i64,
            "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "XStep" => 100_i64,
            "YStep" => 100_i64,
            "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 50.into(), 600.into()],
            "Resources" => muster_res,
        },
        inhalt.to_vec(),
    ))
}

/// **Stilles Leck.** Das Bild wird nicht mit `Do` im Seitenstrom gezeichnet,
/// sondern im Content-Stream eines Kachelmusters, mit dem die Seite ein
/// Rechteck füllt — genau die Fläche unter der Zone. Kein Textoperator im
/// Muster.
///
/// Erwartung: die Bildpunkte fallen, und die Musterwarnung sagt, was sie an
/// der ersten Kachel vermessen hat. Befund (Register #75, behoben):
/// `redacted_images == 0`, keine Warnung, das Orakel fand den Klartext in der
/// Ausgabe — `content.rs::scan_tiling_pattern` kehrte vor dem Durchlaufen
/// zurück, wenn das Muster keinen Textoperator hatte, und der Bildsammler
/// (`image.rs::Collector`) erhielt kein Ereignis.
#[test]
fn bild_im_kachelmuster_ohne_text_faellt() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let muster_id = kachelmuster_mit_bild(&mut doc, bild_id, b"q 100 0 0 100 0 0 cm /Im0 Do Q\n");
    let res = doc.add_object(dictionary! { "Pattern" => dictionary! { "P0" => muster_id } });
    let inhalt = b"/Pattern cs /P0 scn 50 600 100 100 re f\n".to_vec();
    let (mut doc, _ids) = seiten(doc, vec![(res, inhalt)]);
    vorbedingung_orakel_findet(&doc);

    let (report, out) = lauf(
        &mut doc,
        &[schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0))],
    );
    let funde = leaks(&out, GEHEIM);
    assert_eq!(
        report.redacted_images, 1,
        "Bild im Kachelmuster unter der Zone: Warnungen {:?}",
        report.warnings
    );
    assert!(
        funde.is_empty(),
        "Bild im Kachelmuster unter der Zone: Klartext in der Ausgabe {funde:?}, \
         Warnungen {:?}",
        report.warnings
    );
    // Und die Meldung sagt, dass das Muster nur an der ersten Kachel vermessen
    // wurde — mit dem, was darin steht.
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("Kachelmuster") && w.contains("platziert Bilder")),
        "die Musterwarnung fehlt oder nennt das Bild nicht: {:?}",
        report.warnings
    );
}

/// Dieselbe Wurzel eine Ebene tiefer: das Muster setzt keinen Text und zeichnet
/// kein Bild, es platziert ein **Formular** (`/Fm0 Do`), und erst das Formular
/// zeichnet das Bild. Der frühe Ausstieg sah nur die Operatoren des
/// Musterstroms selbst — ein `Do` darin galt als Schraffur.
#[test]
fn bild_im_formular_im_kachelmuster_faellt() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let form_res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let form_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => form_res,
        },
        b"q 100 0 0 100 0 0 cm /Im0 Do Q\n".to_vec(),
    ));
    let muster_res = doc.add_object(dictionary! { "XObject" => dictionary! { "Fm0" => form_id } });
    let muster_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "Pattern",
            "PatternType" => 1_i64,
            "PaintType" => 1_i64,
            "TilingType" => 1_i64,
            "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "XStep" => 100_i64,
            "YStep" => 100_i64,
            "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 50.into(), 600.into()],
            "Resources" => muster_res,
        },
        b"/Fm0 Do\n".to_vec(),
    ));
    let res = doc.add_object(dictionary! { "Pattern" => dictionary! { "P0" => muster_id } });
    let inhalt = b"/Pattern cs /P0 scn 50 600 100 100 re f\n".to_vec();
    let (mut doc, _ids) = seiten(doc, vec![(res, inhalt)]);
    vorbedingung_orakel_findet(&doc);

    let (report, out) = lauf(
        &mut doc,
        &[schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0))],
    );
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    let funde = leaks(&out, GEHEIM);
    assert!(
        funde.is_empty(),
        "Bild im Formular im Kachelmuster: Klartext in der Ausgabe {funde:?}, Warnungen {:?}",
        report.warnings
    );
}

/// Dieselbe Frage, aber das Muster **hat** einen Textoperator, wird also
/// durchlaufen (mit Warnung „enthält Text“). Dann sieht der Bildsammler das
/// `Do` mit `StreamKey::Form(muster)`. Hier wird festgehalten, was dann
/// geschieht: fällt das Bild? Bleibt Klartext?
#[test]
fn bild_im_kachelmuster_mit_text_wird_gesehen() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let muster_res = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
        "Font" => dictionary! { "F1" => font_id },
    });
    let muster_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "Pattern",
            "PatternType" => 1_i64,
            "PaintType" => 1_i64,
            "TilingType" => 1_i64,
            "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "XStep" => 100_i64,
            "YStep" => 100_i64,
            "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 50.into(), 600.into()],
            "Resources" => muster_res,
        },
        b"q 100 0 0 100 0 0 cm /Im0 Do Q BT /F1 8 Tf 2 2 Td (x) Tj ET\n".to_vec(),
    ));
    let res = doc.add_object(dictionary! { "Pattern" => dictionary! { "P0" => muster_id } });
    let inhalt = b"/Pattern cs /P0 scn 50 600 100 100 re f\n".to_vec();
    let (mut doc, _ids) = seiten(doc, vec![(res, inhalt)]);
    vorbedingung_orakel_findet(&doc);

    let (report, out) = lauf(
        &mut doc,
        &[schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0))],
    );
    let funde = leaks(&out, GEHEIM);
    assert!(
        funde.is_empty(),
        "Bild im Kachelmuster mit Text: {} geschwärzte Bilder, Klartext in der Ausgabe \
         {funde:?}, Warnungen {:?}",
        report.redacted_images,
        report.warnings
    );
}

// ===========================================================================
// 3. Nicht gezeichneter Name nach der Kopie
// ===========================================================================

/// **Stilles Leck.** Die Ressourcen stehen am `/Pages`-Knoten (geerbt) und
/// führen das Bild unter `/Im0`. Beide Seiten zeichnen es, beide unter einer
/// Zone. Das Bild hängt an zwei Seiten → `Fate::Copy` je Seite; `repoint_page`
/// verankert je Seite eigene Ressourcen mit `/Im0 → Kopie`. Das **geerbte**
/// Verzeichnis am `/Pages`-Knoten zeigt weiter auf das Original; es ist vom
/// Trailer aus erreichbar, `prune_unreachable` lässt es stehen.
///
/// Erwartung: kein Klartext in der Ausgabe (jede gezeichnete Platzierung ist
/// geschwärzt, kein Betrachter zeigt das Original mehr). Befund (Register #76,
/// behoben): das Orakel fand die Klartext-Bildpunkte im Original hinter
/// `/Pages /Resources`. Seit der Nachlese `retire_copied_originals` wird ein
/// Original, das nach dem Kopieren niemand mehr zeichnet, durch ein leeres
/// Bild ersetzt — der Bericht zählt es (`retired_originals`).
#[test]
fn geerbte_ressourcen_halten_das_original_nach_der_kopie() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _ids) = seiten_mit_geerbten_ressourcen(
        doc,
        res,
        vec![
            b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
            b"q 100 0 0 100 300 300 cm /Im0 Do Q\n".to_vec(),
        ],
    );
    vorbedingung_orakel_findet(&doc);

    let (report, out) = lauf(
        &mut doc,
        &[
            schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0)),
            schwaerzung(1, Rect::new(290.0, 290.0, 410.0, 410.0)),
        ],
    );
    assert_eq!(report.redacted_images, 2, "{:?}", report.warnings);
    assert_eq!(
        report.copied_images, 2,
        "beide Seiten kopieren: {:?}",
        report.warnings
    );
    assert_eq!(
        report.retired_originals, 1,
        "das Original zeichnet niemand mehr — es muss ersetzt sein: {:?}",
        report.warnings
    );
    let funde = leaks(&out, GEHEIM);
    assert!(
        funde.is_empty(),
        "das Original hinter dem geerbten /Resources trägt noch die Klartext-Bildpunkte: \
         {funde:?}"
    );
}

/// Dieselbe Wurzel ohne Vererbung: Seite 1 führt das Bild unter `/Im0`
/// (gezeichnet) **und** `/ImAlt` (nie gezeichnet — ein überzähliger Eintrag,
/// wie ihn Erzeuger häufig hinterlassen). Seite 2 zeichnet es unter `/Im0`.
/// Beide Platzierungen unter einer Zone → zwei Kopien; `/ImAlt` zeigt weiter
/// auf das Original. Erwartung: kein Klartext in der Ausgabe (Register #76,
/// behoben: das Original wird ersetzt, `/ImAlt` zeigt danach ein leeres Bild).
#[test]
fn ueberzaehliger_name_haelt_das_original_nach_der_kopie() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let res1 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id, "ImAlt" => bild_id },
    });
    let res2 = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _ids) = seiten(
        doc,
        vec![
            (res1, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec()),
            (res2, b"q 100 0 0 100 300 300 cm /Im0 Do Q\n".to_vec()),
        ],
    );
    vorbedingung_orakel_findet(&doc);

    let (report, out) = lauf(
        &mut doc,
        &[
            schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0)),
            schwaerzung(1, Rect::new(290.0, 290.0, 410.0, 410.0)),
        ],
    );
    assert_eq!(report.redacted_images, 2, "{:?}", report.warnings);
    assert_eq!(report.retired_originals, 1, "{:?}", report.warnings);
    let funde = leaks(&out, GEHEIM);
    assert!(
        funde.is_empty(),
        "das Original hinter dem nie gezeichneten Namen /ImAlt trägt noch die \
         Klartext-Bildpunkte: {funde:?}"
    );
}

/// Gegenprobe zur Vermutung: hängt das Bild nur an **einer** Seite, wird es
/// überschrieben (`Fate::Overwrite`), und dann zeigt auch der nie gezeichnete
/// Name die geschwärzten Bildpunkte. Grün — und damit die Abgrenzung: der
/// Befund hängt an der Kopie, nicht am überzähligen Namen allein. Ersetzt wird
/// hier nichts (`retired_originals` bleibt null).
#[test]
fn ueberzaehliger_name_ist_harmlos_wenn_ueberschrieben_wird() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let res1 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id, "ImAlt" => bild_id },
    });
    let (mut doc, _ids) = seiten(
        doc,
        vec![(res1, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec())],
    );
    vorbedingung_orakel_findet(&doc);
    let (report, out) = lauf(
        &mut doc,
        &[schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0))],
    );
    assert_eq!(report.copied_images, 0, "{:?}", report.warnings);
    assert_eq!(report.retired_originals, 0, "{:?}", report.warnings);
    assert!(leaks(&out, GEHEIM).is_empty());
}

// ===========================================================================
// Gegenproben: zwei weitere Wege ohne `Do` im Seitenstrom, die **halten**
// ===========================================================================

/// Bild in einem Erscheinungsstrom (`/AP /N`) einer Annotation, deren `/Rect`
/// die Zone schneidet. Der Bildlauf sieht es nicht (`interpret` betritt keine
/// Annotationen), aber `remove_annotations` nimmt die Annotation samt Strom,
/// und `prune_unreachable` nimmt das Bild. Grün.
#[test]
fn bild_in_annotationserscheinung_faellt_mit_der_annotation() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let ap_res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let ap_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => ap_res,
        },
        b"q 100 0 0 100 0 0 cm /Im0 Do Q\n".to_vec(),
    ));
    let annot_id = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Stamp",
        "Rect" => vec![50.into(), 600.into(), 150.into(), 700.into()],
        "AP" => dictionary! { "N" => ap_id },
    });
    let res = doc.add_object(dictionary! {});
    let (mut doc, ids) = seiten(doc, vec![(res, b"0 0 m 10 10 l S\n".to_vec())]);
    doc.get_dictionary_mut(ids[0])
        .expect("Seite")
        .set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
    vorbedingung_orakel_findet(&doc);
    let (report, out) = lauf(
        &mut doc,
        &[schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0))],
    );
    assert_eq!(report.removed_annotations, 1, "{:?}", report.warnings);
    let funde = leaks(&out, GEHEIM);
    assert!(funde.is_empty(), "{funde:?}");
}

/// Bild in der Gruppen-Form einer weichen Maske (`/ExtGState /SMask /G`), die
/// Gruppe wird über `gs` gesetzt, nie mit `Do` gezeichnet. `scan_soft_mask`
/// betritt sie, der Bildsammler erhält das Ereignis. Grün.
#[test]
fn bild_in_weicher_maske_faellt() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let gruppe_res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let gruppe_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 842.into(), 842.into()],
            "Group" => dictionary! { "S" => "Transparency", "CS" => "DeviceGray" },
            "Resources" => gruppe_res,
        },
        b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
    ));
    let res = doc.add_object(dictionary! {
        "ExtGState" => dictionary! {
            "GS0" => dictionary! {
                "SMask" => dictionary! { "S" => "Luminosity", "G" => gruppe_id },
            },
        },
    });
    let inhalt = b"q /GS0 gs 0 0 1 rg 50 600 100 100 re f Q\n".to_vec();
    let (mut doc, _ids) = seiten(doc, vec![(res, inhalt)]);
    vorbedingung_orakel_findet(&doc);
    let (report, out) = lauf(
        &mut doc,
        &[schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0))],
    );
    let funde = leaks(&out, GEHEIM);
    assert!(
        funde.is_empty(),
        "Bild in der weichen Maske: {} geschwärzt, {funde:?}, {:?}",
        report.redacted_images,
        report.warnings
    );
}
