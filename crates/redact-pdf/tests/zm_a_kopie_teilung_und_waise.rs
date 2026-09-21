//! Gegenprüfung A, Runde 9 — die drei anderen Wege aus `image.rs::fate`:
//! `Fate::Overwrite`, `Fate::OverwriteShared` und der Rückfall von
//! `repoint_form` auf `repoint_page`.
//!
//! Hier wird **versucht zu widerlegen** und größtenteils nicht geschafft; die
//! Tests sind grün und halten fest, **was** sie messen. Maßstab ist das Orakel
//! `redact_pdf::leaks`: die Bildpunkte tragen den Suchbegriff buchstäblich als
//! Abtastwerte, und der Bildstrom ist unkomprimiert — was von den
//! Eingabebildpunkten in der Ausgabedatei übrig bleibt, findet das Orakel
//! deshalb in den Rohbytes.

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfRedactor, RedactionReport};

/// Steht buchstäblich in den Bildpunkten: 27 Zeichen = 9 RGB-Bildpunkte.
const GEHEIM: &str = "DE89 3704 0044 0532 0130 00";

fn bild_mit_klartext_bildpunkten() -> Stream {
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

fn schwaerzung(page: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "Gegenprüfung A, Runde 9".into(),
            },
        ),
        Action::Blackout,
    )
}

fn schwaerze(doc: &mut Document, list: &[Redaction]) -> (RedactionReport, Vec<u8>) {
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(doc, list)
        .expect("Schwärzung läuft");
    let bytes = save_to_bytes(doc).expect("Speichern");
    (report, bytes)
}

/// Vorbedingung aller Tests dieser Datei: das Orakel findet die
/// Klartext-Bildpunkte in der **Eingabe**. Ohne diese Zeile wäre jedes „nichts
/// gefunden“ unten wertlos.
#[test]
fn das_orakel_findet_die_klartext_bildpunkte_in_der_eingabe() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext_bildpunkten()));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _) = seiten(
        doc,
        vec![(res, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec())],
    );
    let bytes = save_to_bytes(&mut doc).expect("Speichern");
    assert!(
        !leaks(&bytes, GEHEIM).is_empty(),
        "Vorbedingung: die Bildpunkte sind für das Orakel sichtbar"
    );
}

/// **Versucht zu widerlegen, nicht geschafft.** Dasselbe Bildobjekt hängt an
/// zwei Seiten und wird auf **beiden** geschwärzt. `fate` antwortet beide Male
/// `Fate::Copy` (`image_pages` entsteht in Phase 1 und wird nie fortgeschrieben),
/// also entstehen **zwei** Kopien und niemand verweist mehr auf das Original —
/// dessen Bildpunkte sind unversehrt.
///
/// Der Verdacht: das unversehrte Original bleibt als Waise in der Datei stehen,
/// und damit die Bildpunkte, die beide Seiten geschwärzt haben wollten.
/// Gemessen: `save_to_bytes` ruft `document::prune_unreachable`, bevor es
/// serialisiert — die Waise fällt heraus. Kein Leck.
#[test]
fn das_geteilte_bild_auf_beiden_seiten_geschwaerzt_laesst_keine_waise() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext_bildpunkten()));
    let res1 = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let res2 = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let inhalt = b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec();
    let (mut doc, _) = seiten(doc, vec![(res1, inhalt.clone()), (res2, inhalt)]);
    let zone = Rect::new(40.0, 590.0, 160.0, 710.0);
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, zone), schwaerzung(1, zone)],
    );
    assert_eq!(
        report.copied_images, 2,
        "Vorbedingung: zwei Kopien, das Original bleibt unberührt — {:?}",
        report.warnings
    );
    // Das Original ist noch im Speicher (nichts hat es gelöscht) …
    assert!(
        matches!(doc.get_object(bild_id), Ok(Object::Stream(_))),
        "Vorbedingung: das unversehrte Original steht noch im Dokument"
    );
    // … aber es steht nicht mehr in der Datei.
    assert_eq!(
        leaks(&out, GEHEIM),
        Vec::<String>::new(),
        "die verwaiste unversehrte Vorlage darf nicht in der Ausgabedatei stehen"
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert!(
        aus.get_object(bild_id).is_err(),
        "prune_unreachable hat die Waise entfernt"
    );
}

/// **Versucht zu widerlegen, nicht geschafft.** `Fate::OverwriteShared`: das
/// Bild steckt in einem Form-XObject, das **zwei** Seiten zeichnen, und die
/// eigenen Ressourcen des Formulars führen es. Damit lässt sich der Verweis
/// nicht isolieren; überschrieben wird, und es wird gewarnt.
///
/// Gemessen: die Bildpunkte fallen vollständig (das Orakel findet nichts mehr),
/// und die Warnung nennt den Namen und den Grund. Die Über-Schwärzung auf der
/// zweiten Seite ist genannt — grob entscheiden ist erlaubt, wenn es gesagt wird.
#[test]
fn geteiltes_formular_ueberschreibt_und_warnt_fuer_beide_seiten() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext_bildpunkten()));
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
    let res1 = doc.add_object(dictionary! { "XObject" => dictionary! { "Fm0" => form_id } });
    let res2 = doc.add_object(dictionary! { "XObject" => dictionary! { "Fm0" => form_id } });
    let inhalt = b"q 1 0 0 1 50 600 cm /Fm0 Do Q\n".to_vec();
    let (mut doc, _) = seiten(doc, vec![(res1, inhalt.clone()), (res2, inhalt)]);
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0))],
    );
    assert_eq!(report.copied_images, 0, "nicht kopiert, sondern überschrieben");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("mehrere Seiten benutzen")),
        "die Über-Schwärzung auf der zweiten Seite wird gesagt: {:?}",
        report.warnings
    );
    assert_eq!(
        leaks(&out, GEHEIM),
        Vec::<String>::new(),
        "die Bildpunkte fallen vollständig"
    );
}

/// **Versucht zu widerlegen, nicht geschafft.** Der Rückfall in `write_work`:
/// ein Form-XObject **ohne eigenes** `/Resources` löst `/Im0` über die
/// Seitenressourcen auf. `repoint_form` findet dort nichts umzubiegen und gibt
/// `false` zurück; umgebogen wird dann die **Seite**.
///
/// Das Bild hängt zusätzlich an Seite 2, wird also kopiert. Gemessen: Seite 1
/// zeigt die Kopie (über die geerbten Seitenressourcen), Seite 2 das
/// unversehrte Original — und die Klartextbildpunkte, die Seite 1 geschwärzt
/// hat, stehen nur noch dort, wo sie stehen dürfen.
#[test]
fn formular_ohne_eigene_ressourcen_biegt_die_seite_um() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext_bildpunkten()));
    let form_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
        },
        b"q 100 0 0 100 0 0 cm /Im0 Do Q\n".to_vec(),
    ));
    let res1 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id, "Fm0" => form_id },
    });
    let res2 = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, ids) = seiten(
        doc,
        vec![
            (res1, b"q 1 0 0 1 50 600 cm /Fm0 Do Q\n".to_vec()),
            (res2, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec()),
        ],
    );
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0))],
    );
    assert_eq!(
        report.copied_images, 1,
        "Vorbedingung: kopiert — {:?}",
        report.warnings
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    let ziel = |seite: ObjectId| -> ObjectId {
        let res = aus
            .get_dictionary(seite)
            .and_then(|d| d.get(b"Resources"))
            .and_then(|o| aus.dereference(o).map(|(_, r)| r))
            .and_then(|o| o.as_dict())
            .expect("Ressourcen")
            .clone();
        let xo = res
            .get(b"XObject")
            .and_then(|o| aus.dereference(o).map(|(_, r)| r))
            .and_then(|o| o.as_dict())
            .expect("XObject")
            .clone();
        match xo.get(b"Im0").expect("Im0") {
            Object::Reference(id) => *id,
            other => panic!("kein Verweis: {other:?}"),
        }
    };
    assert_ne!(
        ziel(ids[0]),
        bild_id,
        "Seite 1 erbt die umgebogene Kopie an das Formular weiter"
    );
    assert_eq!(
        ziel(ids[1]),
        bild_id,
        "Seite 2 zeigt weiter das unversehrte Original"
    );
    // Und die Klartextbildpunkte stehen nur noch in dem Strom, den Seite 2
    // zeichnet — die Kopie, die Seite 1 (über das Formular) zeigt, hat sie
    // nicht mehr. Das Orakel meldet denselben Fund je Sichtweise mehrfach;
    // gefragt ist deshalb der Strom, nicht die Zahl der Fundstellen.
    let inhalt = |id: ObjectId| -> Vec<u8> {
        match aus.get_object(id).expect("Bildstrom") {
            Object::Stream(stream) => stream
                .decompressed_content()
                .unwrap_or_else(|_| stream.content.clone()),
            other => panic!("kein Strom: {other:?}"),
        }
    };
    let traegt = |id: ObjectId| -> bool {
        inhalt(id)
            .windows(GEHEIM.len())
            .any(|w| w == GEHEIM.as_bytes())
    };
    assert!(
        !traegt(ziel(ids[0])),
        "die Kopie, die das Formular auf Seite 1 zeichnet, ist geschwärzt"
    );
    assert!(
        traegt(bild_id),
        "und das Original — für die ungeschwärzte Seite 2 — ist unberührt"
    );
    assert!(
        !leaks(&out, GEHEIM).is_empty(),
        "genau deshalb findet das Orakel die Bildpunkte noch: sie gehören zu \
         Seite 2, auf der niemand geschwärzt hat"
    );
}
