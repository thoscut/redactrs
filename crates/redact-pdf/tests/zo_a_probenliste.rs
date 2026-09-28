//! Spur-A-Runde 1, Prüfer A — **eigener Lauf** für die zwei Zeilen der
//! Probenliste (`PRUEFLISTE.md`), die das Bildgebiet betreffen:
//!
//! * „Bild unter zwei Namen; Kopie, Teilung, Waise“
//! * „Flächenfrage am Rand: berührt ist nicht geschnitten; entartete Matrix“
//!
//! Nicht die Belegdateien gelesen, sondern eigenes Material mit **anderen**
//! Konstellationen als dort: drei Namen statt zwei (zwei im Seitenstrom, einer
//! in einem Formular mit eigenen Ressourcen), ein geteiltes Formular mit Zone
//! auf nur einer Seite, ein Inline-Bild, das eine Zone nur berührt, eine
//! Nullhöhen-CTM, ein um 45 Grad gedrehtes Bild mit Zone in der leeren
//! Hüllenecke, eine gespiegelte Platzierung.
//!
//! Maßstab: die Bildpunkte, die die **Ausgabedatei** zeigt (`page_ops`, der
//! Dekoder des Renderers), und `redact_pdf::leaks` an den Ausgabebytes — nie
//! der Bericht. Alle Tests hier sind **grün**: die Zeilen treten nicht mehr
//! auf.
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-pdf --test zo_a_probenliste`

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Point, Rect, Redaction, Region, Source};
use redact_pdf::{
    leaks, load_from_bytes, page_ops, save_to_bytes, DrawOp, PdfRedactor, RedactionReport,
};

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

/// 8 x 8 RGB, jeder Bildpunkt an seinem Wert erkennbar, keiner schwarz.
fn bild_8x8() -> Stream {
    let mut data = Vec::with_capacity(8 * 8 * 3);
    for y in 0..8u32 {
        for x in 0..8u32 {
            data.extend_from_slice(&[(x * 16 + 1) as u8, (y * 16 + 1) as u8, 0x40]);
        }
    }
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 8_i64,
            "Height" => 8_i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
        },
        data,
    )
    .with_compression(false)
}

fn wert_8x8(x: usize, y: usize) -> [u8; 3] {
    [(x * 16 + 1) as u8, (y * 16 + 1) as u8, 0x40]
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
                reason: "Spur A, Runde 1, Prüfer A".into(),
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

/// Alle Bildpunkte, die Seite `seite` der Ausgabedatei zeigt, mit ihrem
/// Bildindex, ihrer Zelle und ihrem Mittelpunkt im User-Space.
fn gezeigte_bildpunkte(bytes: &[u8], seite: usize) -> Vec<(usize, usize, [u8; 4], Point)> {
    let doc = load_from_bytes(bytes).expect("Ausgabe lädt");
    let ops = page_ops(&doc, seite).expect("page_ops läuft");
    let mut out = Vec::new();
    for op in &ops.ops {
        let DrawOp::Image { image, ctm, .. } = op else {
            continue;
        };
        let raster = &ops.images[*image];
        assert!(!raster.placeholder, "Ausgabebild ist dekodierbar");
        let (w, h) = (raster.width as f64, raster.height as f64);
        for y in 0..raster.height as usize {
            for x in 0..raster.width as usize {
                let p = ctm.apply((x as f64 + 0.5) / w, 1.0 - (y as f64 + 0.5) / h);
                let off = (y * raster.width as usize + x) * 4;
                out.push((
                    x,
                    y,
                    [
                        raster.rgba[off],
                        raster.rgba[off + 1],
                        raster.rgba[off + 2],
                        raster.rgba[off + 3],
                    ],
                    p,
                ));
            }
        }
    }
    out
}

fn klartext_bildpunkte() -> Vec<[u8; 4]> {
    GEHEIM
        .as_bytes()
        .chunks(3)
        .map(|c| [c[0], c[1], c[2], 255])
        .collect()
}

// ===========================================================================
// Zeile: Bild unter zwei Namen; Kopie, Teilung, Waise
// ===========================================================================

/// **Drei** Namen auf einer Seite — `/Im0` und `/Im1` im Seitenstrom, `/ImA` in
/// einem Formular mit eigenen Ressourcen —, alle drei unter der Zone, und das
/// Bild hängt außerdem an Seite 2 (also Kopie, nicht Überschreiben).
///
/// Erwartung: kein Klartext-Bildpunkt innerhalb der Zone auf Seite 1; Seite 2
/// zeigt ihr Bild **unverändert** (keine Über-Schwärzung); eine Kopie.
#[test]
fn drei_namen_ein_bild_kopie_alle_umgebogen_seite_zwei_unversehrt() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let form_res = doc.add_object(dictionary! {
        "XObject" => dictionary! { "ImA" => bild_id },
    });
    let form_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => form_res,
        },
        b"q 100 0 0 100 0 0 cm /ImA Do Q\n".to_vec(),
    ));
    let res1 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id, "Im1" => bild_id, "Fm0" => form_id },
    });
    let res2 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    let inhalt1 = b"q 100 0 0 100 50 600 cm /Im0 Do Q\n\
                    q 100 0 0 100 250 600 cm /Im1 Do Q\n\
                    q 1 0 0 1 450 600 cm /Fm0 Do Q\n"
        .to_vec();
    let inhalt2 = b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec();
    let (mut doc, _ids) = seiten(doc, vec![(res1, inhalt1), (res2, inhalt2)]);

    let zone = Rect::new(40.0, 590.0, 560.0, 710.0);
    let (report, out) = schwaerze(&mut doc, &[schwaerzung(0, zone)]);
    assert_eq!(
        report.copied_images, 1,
        "eine Kopie — {:?}",
        report.warnings
    );

    let klartext = klartext_bildpunkte();
    let seite1 = gezeigte_bildpunkte(&out, 0);
    assert_eq!(seite1.len(), 3 * 36, "drei Platzierungen à 36 Bildpunkte");
    let stehen: Vec<_> = seite1
        .iter()
        .filter(|(_, _, farbe, p)| zone.contains(*p) && klartext.contains(farbe))
        .collect();
    assert!(
        stehen.is_empty(),
        "Seite 1 zeigt in der Zone noch {} Klartext-Bildpunkte: {:?}",
        stehen.len(),
        stehen
    );
    // Und **jeder** Bildpunkt in der Zone ist schwarz (die Zone deckt alle drei
    // Platzierungen vollständig).
    assert!(
        seite1
            .iter()
            .filter(|(_, _, _, p)| zone.contains(*p))
            .all(|(_, _, f, _)| *f == [0, 0, 0, 255]),
        "alle Bildpunkte unter der Zone sind gefüllt"
    );

    // Seite 2: unversehrt, die neun Klartext-Bildpunkte stehen noch — das ist
    // richtig, dort lag keine Schwärzung.
    let seite2 = gezeigte_bildpunkte(&out, 1);
    let noch_da = seite2
        .iter()
        .filter(|(_, _, f, _)| klartext.contains(f))
        .count();
    assert_eq!(noch_da, 9, "Seite 2 zeigt ihr Bild unverändert");
}

/// **Waise**: dasselbe Bild auf beiden Seiten, auf beiden unter einer Zone —
/// zweimal kopiert; das Original hängt danach an nichts mehr und darf die
/// Ausgabedatei nicht überleben. Maßstab `leaks` an den Ausgabebytes.
#[test]
fn zwei_kopien_lassen_keine_waise_mit_klartext_zurueck() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let res1 = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let res2 = doc.add_object(dictionary! { "XObject" => dictionary! { "Bild" => bild_id } });
    let inhalt1 = b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec();
    let inhalt2 = b"q 100 0 0 100 300 300 cm /Bild Do Q\n".to_vec();
    let (mut doc, _ids) = seiten(doc, vec![(res1, inhalt1), (res2, inhalt2)]);
    let vorher = save_to_bytes(&doc).expect("Speichern");
    assert!(
        !leaks(&vorher, GEHEIM).is_empty(),
        "Vorbedingung: das Orakel findet die Klartext-Bildpunkte in der Eingabe"
    );

    let (report, out) = schwaerze(
        &mut doc,
        &[
            schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0)),
            schwaerzung(1, Rect::new(290.0, 290.0, 410.0, 410.0)),
        ],
    );
    assert_eq!(report.redacted_images, 2, "{:?}", report.warnings);
    let funde = leaks(&out, GEHEIM);
    assert!(
        funde.is_empty(),
        "Waise mit Klartext in der Ausgabe: {funde:?}"
    );
}

/// **Teilung**: das Bild steckt in einem Formular, das beide Seiten zeichnen;
/// die Zone liegt nur auf Seite 1. Erwartung: überschrieben **mit** Warnung,
/// kein Klartext mehr in der Datei (auch Seite 2 zeigt das geschwärzte Bild —
/// die bewusste, gesagte Über-Schwärzung).
#[test]
fn geteiltes_formular_ueberschreibt_mit_warnung_und_ohne_klartext() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext()));
    let form_res = doc.add_object(dictionary! { "XObject" => dictionary! { "ImA" => bild_id } });
    let form_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => form_res,
        },
        b"q 100 0 0 100 0 0 cm /ImA Do Q\n".to_vec(),
    ));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Fm0" => form_id } });
    let inhalt = b"q 1 0 0 1 50 600 cm /Fm0 Do Q\n".to_vec();
    let (mut doc, _ids) = seiten(doc, vec![(res, inhalt.clone()), (res, inhalt)]);

    let zone = Rect::new(40.0, 590.0, 160.0, 710.0);
    let (report, out) = schwaerze(&mut doc, &[schwaerzung(0, zone)]);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("Form-XObject, das mehrere Seiten benutzen")),
        "die Über-Schwärzung wird gesagt: {:?}",
        report.warnings
    );
    assert_eq!(report.copied_images, 0);
    let funde = leaks(&out, GEHEIM);
    assert!(funde.is_empty(), "Klartext in der Ausgabe: {funde:?}");
    let seite2 = gezeigte_bildpunkte(&out, 1);
    assert!(
        seite2
            .iter()
            .filter(|(_, y, _, _)| *y == 1)
            .all(|(_, _, f, _)| *f == [0, 0, 0, 255]),
        "Seite 2 zeigt dieselben geschwärzten Bildpunkte"
    );
}

// ===========================================================================
// Zeile: Flächenfrage am Rand; entartete Matrix
// ===========================================================================

/// Die Zone **berührt** die Bildkante nur (gemeinsame Kante bei x = 150) und
/// ist der einzige Anlass auf der Seite. Früher übersprang der grobe Vorfilter
/// die Seite (Hülle, strenge Vergleiche). Erwartung heute: der Lauf endet mit
/// Ausgabedatei, und die berührte Randspalte fällt (Berührung zählt).
#[test]
fn beruehrung_an_der_kante_als_einziger_anlass_laeuft_durch_und_fuellt_die_randspalte() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_8x8()));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _ids) = seiten(
        doc,
        vec![(res, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec())],
    );

    // Bild: x 50..150, y 600..700. Zone: x 150..200 — gemeinsame Kante.
    let zone = Rect::new(150.0, 620.0, 200.0, 680.0);
    let ergebnis =
        PdfRedactor::with_padding(0.0).apply_with_report(&mut doc, &[schwaerzung(0, zone)]);
    let report = ergebnis.expect("eine Berührung darf den Lauf nicht abbrechen");
    let out = save_to_bytes(&doc).expect("Speichern");
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    let gefallen: Vec<(usize, usize)> = gezeigte_bildpunkte(&out, 0)
        .into_iter()
        .filter(|(x, y, f, _)| f[..3] != wert_8x8(*x, *y))
        .map(|(x, y, _, _)| (x, y))
        .collect();
    // Zellen 12,5 Punkte breit: die Spalte 7 (x 137,5..150) berührt; y 620..680
    // deckt Zeilen mit v in (0.2, 0.8) → y-Zeilen 1..=6 (Berührung an 625/675
    // eingeschlossen: Zeilen 1 und 6 berühren nur, zählen aber).
    assert!(!gefallen.is_empty(), "die berührte Randspalte fällt");
    assert!(
        gefallen.iter().all(|(x, _)| *x == 7),
        "nur die Randspalte, nicht das Bildinnere: {gefallen:?}"
    );
}

/// Dasselbe mit einem **Inline-Bild**: hier gingen die zwei Flächenfragen
/// auseinander, die Rohdaten wurden nicht mitgeführt und der Lauf endete ohne
/// Ausgabedatei. Erwartung: Ausgabedatei entsteht, Randspalte fällt.
#[test]
fn inline_bild_nur_beruehrt_laeuft_durch() {
    let mut doc = Document::with_version("1.5");
    let mut rohdaten = Vec::new();
    for y in 0..8u32 {
        for x in 0..8u32 {
            rohdaten.extend_from_slice(&[(x * 16 + 1) as u8, (y * 16 + 1) as u8, 0x40]);
        }
    }
    let mut inhalt = b"q 100 0 0 100 50 600 cm BI /W 8 /H 8 /CS /RGB /BPC 8 ID ".to_vec();
    inhalt.extend_from_slice(&rohdaten);
    inhalt.extend_from_slice(b" EI Q\n");
    let res = doc.add_object(dictionary! {});
    let (mut doc, _ids) = seiten(doc, vec![(res, inhalt)]);

    let zone = Rect::new(150.0, 620.0, 200.0, 680.0);
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &[schwaerzung(0, zone)])
        .expect("Berührung eines Inline-Bildes darf den Lauf nicht abbrechen");
    let out = save_to_bytes(&doc).expect("Speichern");
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    let gefallen: Vec<(usize, usize)> = gezeigte_bildpunkte(&out, 0)
        .into_iter()
        .filter(|(x, y, f, _)| f[..3] != wert_8x8(*x, *y))
        .map(|(x, y, _, _)| (x, y))
        .collect();
    assert!(
        !gefallen.is_empty() && gefallen.iter().all(|(x, _)| *x == 7),
        "{gefallen:?}"
    );
}

/// Entartete Matrix mit **Nullhöhe** (`100 0 0 0 cm`): das Bild ist eine Linie,
/// die Zone liegt darauf. Erwartung: kein Abbruch, keine Behauptung — 0
/// geschwärzte Bilder, Ausgabedatei entsteht.
#[test]
fn nullhoehe_ctm_bricht_nicht_ab_und_behauptet_nichts() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_8x8()));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _ids) = seiten(
        doc,
        vec![(res, b"q 100 0 0 0 50 600 cm /Im0 Do Q\n".to_vec())],
    );
    let zone = Rect::new(40.0, 590.0, 160.0, 610.0);
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &[schwaerzung(0, zone)])
        .expect("eine entartete Matrix darf den Lauf nicht abbrechen");
    save_to_bytes(&doc).expect("Speichern");
    assert_eq!(report.redacted_images, 0, "{:?}", report.warnings);
}

/// Um 45 Grad gedreht, die Zone in der **leeren Hüllenecke**: kein Bildpunkt
/// darf fallen, nichts darf behauptet werden. Und dieselbe Zone in der Mitte:
/// es fällt etwas.
#[test]
fn gedrehtes_bild_huellenecke_faellt_nicht_mitte_faellt() {
    let s = std::f64::consts::FRAC_1_SQRT_2 * 100.0;
    let ctm = format!("{s} {s} {} {s} 300 500 cm", -s);
    let bau = || {
        let mut doc = Document::with_version("1.5");
        let bild_id = doc.add_object(Object::Stream(bild_8x8()));
        let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
        seiten(
            doc,
            vec![(res, format!("q {ctm} /Im0 Do Q\n").into_bytes())],
        )
    };

    // Viereck: (300,500) (370.7,570.7) (300,641.4) (229.3,570.7). Die Hülle
    // reicht bis x = 229.3 bei y = 500 — dort liegt kein Bildpunkt.
    let (mut doc, _) = bau();
    let ecke = Rect::new(230.0, 500.0, 245.0, 510.0);
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &[schwaerzung(0, ecke)])
        .expect("läuft");
    let out = save_to_bytes(&doc).expect("Speichern");
    assert_eq!(
        report.redacted_images, 0,
        "Hüllenecke: {:?}",
        report.warnings
    );
    let unversehrt = gezeigte_bildpunkte(&out, 0)
        .iter()
        .all(|(x, y, f, _)| f[..3] == wert_8x8(*x, *y));
    assert!(
        unversehrt,
        "in der Hüllenecke liegt kein Bildpunkt — keiner darf fallen"
    );

    let (mut doc, _) = bau();
    let mitte = Rect::new(295.0, 565.0, 305.0, 575.0);
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &[schwaerzung(0, mitte)])
        .expect("läuft");
    let out = save_to_bytes(&doc).expect("Speichern");
    assert_eq!(report.redacted_images, 1, "Mitte: {:?}", report.warnings);
    let gefallen = gezeigte_bildpunkte(&out, 0)
        .iter()
        .filter(|(x, y, f, _)| f[..3] != wert_8x8(*x, *y))
        .count();
    assert!(
        gefallen > 0 && gefallen < 64,
        "nur die Mitte fällt: {gefallen}"
    );
}

/// Gespiegelt (`-100 0 0 100 150 600 cm`): Bildspalte 0 liegt rechts. Eine
/// Zone am rechten Rand (x 140..160) muss die **Spalte 0** treffen, nicht
/// Spalte 7. Gemessen an den Bildpunkten, die die Ausgabe zeigt: kein Bildpunkt
/// mit Mittelpunkt in der Zone trägt noch seinen Eingabewert.
#[test]
fn gespiegelte_platzierung_faellt_dort_wo_sie_gezeigt_wird() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_8x8()));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _ids) = seiten(
        doc,
        vec![(res, b"q -100 0 0 100 150 600 cm /Im0 Do Q\n".to_vec())],
    );
    let zone = Rect::new(140.0, 590.0, 160.0, 710.0);
    let (report, out) = schwaerze(&mut doc, &[schwaerzung(0, zone)]);
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    let punkte = gezeigte_bildpunkte(&out, 0);
    let in_zone: Vec<_> = punkte
        .iter()
        .filter(|(_, _, _, p)| zone.contains(*p))
        .collect();
    assert_eq!(
        in_zone.len(),
        8,
        "eine Spalte à 8 Bildpunkte liegt in der Zone"
    );
    assert!(
        in_zone
            .iter()
            .all(|(x, _, f, _)| *x == 0 && *f == [0, 0, 0, 255]),
        "die gezeigte Spalte 0 ist gefüllt: {in_zone:?}"
    );
    let unversehrt = punkte
        .iter()
        .filter(|(x, _, _, _)| *x != 0)
        .all(|(x, y, f, _)| f[..3] == wert_8x8(*x, *y));
    assert!(unversehrt, "die übrigen Spalten bleiben, wie sie waren");
}
