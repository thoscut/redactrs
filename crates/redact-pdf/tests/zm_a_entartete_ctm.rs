//! Gegenprüfung A, Runde 9 — die **Flächenfrage** der Runde 8 an entarteten und
//! an gewöhnlichen CTMs.
//!
//! Geprüft wird `image.rs::cell_meets_rect` / `Work::covers` / `placement_quad`
//! in **beiden** Richtungen: fällt jeder Bildpunkt unter der Zone (Leck), und
//! fällt keiner daneben (Fehlalarm)? Gemessen wird nicht am Bericht, sondern an
//! den Bildpunkten, die die **Ausgabedatei** zeigt: `page_ops` löst jede
//! Platzierung auf, dekodiert ihr Bild und nennt die CTM.
//!
//! Die Zahlen sind so gewählt, dass keine Zellkante mit einer Rechteckkante
//! zusammenfällt — sonst hinge der Befund an „Berührung zählt als Treffer“ statt
//! an der Geometrie. Bild 8 x 8 auf 100 x 100 Punkte: Zellkanten alle 12,5
//! Punkte, das Rechteck liegt mit 63 … 74 bzw. 613 … 637 dazwischen.

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Point, Rect, Redaction, Region, Source};
use redact_pdf::{load_from_bytes, page_ops, save_to_bytes, DrawOp, PdfRedactor, RedactionReport};

/// Ein 8 x 8 RGB-Bild, in dem jeder Bildpunkt an seinem Wert erkennbar ist —
/// und keiner schwarz ist.
fn bild(alt: &str) -> Stream {
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
            "Alt" => Object::string_literal(alt),
        },
        data,
    )
    .with_compression(false)
}

/// Der Wert, den Bildpunkt (x, y) in der Eingabe trägt.
fn eingabewert(x: usize, y: usize) -> [u8; 3] {
    [(x * 16 + 1) as u8, (y * 16 + 1) as u8, 0x40]
}

fn seite(mut doc: Document, resources_id: ObjectId, content: Vec<u8>) -> (Document, ObjectId) {
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, content));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 842.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, page_id)
}

/// Eine Seite mit genau einer Bildplatzierung unter der angegebenen CTM.
fn seite_mit_bild(ctm: &str, alt: &str) -> (Document, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild(alt)));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let inhalt = format!("q {ctm} cm /Im0 Do Q\n").into_bytes();
    let (doc, _) = seite(doc, res, inhalt);
    (doc, bild_id)
}

fn schwaerzung(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
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

/// Was die Ausgabeseite je Bildpunkt zeigt: `(Spalte, Zeile, Farbe, Mittelpunkt
/// im User-Space)`. Eine Zeile je Bildpunkt **je Platzierung**.
fn gezeigte_bildpunkte(bytes: &[u8], seite: usize) -> Vec<(usize, usize, [u8; 3], Point)> {
    let doc = load_from_bytes(bytes).expect("Ausgabe lädt");
    let ops = page_ops(&doc, seite).expect("page_ops läuft");
    let mut out = Vec::new();
    for op in &ops.ops {
        let DrawOp::Image { image, ctm, .. } = op else {
            continue;
        };
        let raster = &ops.images[*image];
        let (w, h) = (raster.width as f64, raster.height as f64);
        for y in 0..raster.height as usize {
            for x in 0..raster.width as usize {
                let p = ctm.apply((x as f64 + 0.5) / w, 1.0 - (y as f64 + 0.5) / h);
                let off = (y * raster.width as usize + x) * 4;
                out.push((
                    x,
                    y,
                    [raster.rgba[off], raster.rgba[off + 1], raster.rgba[off + 2]],
                    Point::new(p.x, p.y),
                ));
            }
        }
    }
    out
}

/// Der Kern beider Richtungen in einem Satz: welche Bildpunkte haben ihren
/// Eingabewert verloren, und wo liegen sie?
fn gefallene(bytes: &[u8]) -> Vec<(usize, usize)> {
    gezeigte_bildpunkte(bytes, 0)
        .into_iter()
        .filter(|(x, y, farbe, _)| *farbe != eingabewert(*x, *y))
        .map(|(x, y, _, _)| (x, y))
        .collect()
}

/// Das Rechteck, das keine Zellkante berührt: Spalten 1, Zeilen 5 und 6 bei
/// `100 0 0 100 50 600`.
fn zone_zwischen_den_zellkanten() -> Rect {
    Rect::new(63.0, 613.0, 74.0, 637.0)
}

// ===========================================================================
// Gewöhnliches Material: genau die Zellen unter der Zone fallen
// ===========================================================================

/// **Versucht zu widerlegen, nicht geschafft — beide Richtungen.** Das Rechteck
/// liegt ganz zwischen den Zellkanten: Spalte 1 (62,5 … 75) und die Zeilen 5
/// (625 … 637,5) und 6 (612,5 … 625). Genau diese zwei Bildpunkte dürfen fallen
/// — kein dritter (Fehlalarm) und keiner weniger (Leck).
#[test]
fn genau_die_zellen_unter_der_zone_fallen() {
    let (mut doc, _) = seite_mit_bild("100 0 0 100 50 600", "Scan");
    let (report, out) = schwaerze(&mut doc, &[schwaerzung(zone_zwischen_den_zellkanten())]);
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    let mut gefallen = gefallene(&out);
    gefallen.sort();
    assert_eq!(
        gefallen,
        vec![(1, 5), (1, 6)],
        "genau Spalte 1, Zeilen 5 und 6 — nicht mehr (Fehlalarm) und nicht \
         weniger (Leck)"
    );
}

/// **Versucht zu widerlegen, nicht geschafft.** Negative Skalierung: das Bild
/// wird gespiegelt gezeichnet (`-100 0 0 100 150 600`, Zielfläche wieder
/// 50 … 150). Dasselbe Rechteck muss jetzt die **gespiegelte** Spalte treffen —
/// Spalte 6 statt Spalte 1. Trifft es weiter Spalte 1, rechnet `pixel_bounds`
/// oder `covers` die Spiegelung nicht mit, und die Schwärzung läge an der
/// falschen Stelle: Leck und Fehlalarm in einem.
#[test]
fn negative_skalierung_trifft_die_gespiegelte_spalte() {
    let (mut doc, _) = seite_mit_bild("-100 0 0 100 150 600", "Scan");
    let (report, out) = schwaerze(&mut doc, &[schwaerzung(zone_zwischen_den_zellkanten())]);
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    let mut gefallen = gefallene(&out);
    gefallen.sort();
    assert_eq!(
        gefallen,
        vec![(6, 5), (6, 6)],
        "gespiegelt: Spalte 6 (u = 0,76 … 0,87), nicht Spalte 1"
    );
}

/// **Versucht zu widerlegen, nicht geschafft.** Scherung (`100 0 50 100 50 600`:
/// x = 100u + 50v + 50). Hier ist die genaue Zellenliste unübersichtlich;
/// gefragt sind deshalb die beiden Richtungen als Eigenschaft: **jeder**
/// Bildpunkt, dessen Mittelpunkt im Rechteck liegt, hat seinen Eingabewert
/// verloren, und es sind nicht alle 64 gefallen.
#[test]
fn scherung_trifft_die_zellen_unter_der_zone_und_nicht_das_ganze_bild() {
    let (mut doc, _) = seite_mit_bild("100 0 50 100 50 600", "Scan");
    let zone = Rect::new(103.0, 613.0, 114.0, 637.0);
    let (report, out) = schwaerze(&mut doc, &[schwaerzung(zone)]);
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    let gezeigt = gezeigte_bildpunkte(&out, 0);
    let innen: Vec<_> = gezeigt
        .iter()
        .filter(|(_, _, _, mitte)| zone.contains(*mitte))
        .collect();
    assert!(
        !innen.is_empty(),
        "Vorbedingung: die Zone liegt auf Bildpunkten"
    );
    let stehen_geblieben: Vec<_> = innen
        .iter()
        .filter(|(x, y, farbe, _)| *farbe == eingabewert(*x, *y))
        .collect();
    assert!(
        stehen_geblieben.is_empty(),
        "unter der Zone stehen {} Bildpunkte mit ihrem Eingabewert: {:?}",
        stehen_geblieben.len(),
        stehen_geblieben
    );
    assert!(
        gefallene(&out).len() < 64,
        "aber nicht das ganze Bild: {} von 64 gefallen",
        gefallene(&out).len()
    );
}

// ===========================================================================
// Entartete CTM
// ===========================================================================

/// **Versucht zu widerlegen, nicht geschafft.** Determinante 0
/// (`100 0 0 0 50 600`: das Bild wird auf eine Linie ohne Höhe abgebildet).
/// `Matrix::invert` gibt `None`, `Work::fill` kehrt sofort zurück, `filled`
/// bleibt 0 — kein Bildpunkt fällt, kein Vermerk, keine Warnung.
///
/// Das ist richtig und nicht still: eine Fläche der Höhe 0 zeigt **keinen**
/// Bildpunkt, also kann auch keiner unter der Schwärzung stehen bleiben. Was
/// hier zählt: der Lauf bricht nicht ab, behauptet keine Schwärzung, und der
/// Ersatztext eines Bildes, das nichts zeigt, bleibt stehen.
#[test]
fn determinante_null_faellt_nicht_und_behauptet_nichts() {
    let (mut doc, bild_id) = seite_mit_bild("100 0 0 0 50 600", "Scan");
    let vorher = match doc.get_object(bild_id).expect("Bild") {
        Object::Stream(s) => s.content.clone(),
        other => panic!("kein Strom: {other:?}"),
    };
    let (report, _out) = schwaerze(
        &mut doc,
        &[schwaerzung(Rect::new(40.0, 590.0, 160.0, 710.0))],
    );
    assert_eq!(
        report.redacted_images, 0,
        "nichts gefallen, nichts behauptet — {:?}",
        report.warnings
    );
    let nachher = match doc.get_object(bild_id).expect("Bild") {
        Object::Stream(s) => s.content.clone(),
        other => panic!("kein Strom: {other:?}"),
    };
    assert_eq!(nachher, vorher, "das Bildobjekt ist unverändert");
}

/// **Versucht zu widerlegen, nicht geschafft.** Eine CTM, deren Einträge beim
/// Multiplizieren nach ∞ laufen (`1e200` zweimal hintereinander). Die Zusage,
/// die hier zählt, ist die kleinste: der Lauf **bricht nicht ab** und **panikt
/// nicht** — weder in `placement_quad` (∞ in jeder Ecke) noch in
/// `cell_meets_rect` (Spannen aus ∞ und NaN) noch in `pixel_bounds`.
#[test]
fn unendliche_ctm_bricht_nicht_ab() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild("Scan")));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _) = seite(
        doc,
        res,
        b"q 1e200 0 0 1e200 0 0 cm 1e200 0 0 1e200 0 0 cm /Im0 Do Q\n".to_vec(),
    );
    let ergebnis = PdfRedactor::with_padding(0.0).apply_with_report(
        &mut doc,
        &[schwaerzung(Rect::new(40.0, 590.0, 160.0, 710.0))],
    );
    let report = ergebnis.expect("der Lauf darf an einer ∞-CTM nicht scheitern");
    let _ = save_to_bytes(&doc).expect("und die Ausgabedatei entsteht");
    // Welcher Ausgang es ist, sagt der Test nicht: eine Fläche, die kein
    // Betrachter zeichnet, darf gefüllt oder ungefüllt bleiben. Gefragt ist,
    // dass nichts panikt und nichts behauptet wird, was nicht geschah.
    assert!(
        report.redacted_images <= 1,
        "höchstens das eine Bild: {}",
        report.redacted_images
    );
}

/// **Versucht zu widerlegen, nicht geschafft.** Eine CTM mit `NaN`-Einträgen
/// (∞ mal 0). `cell_meets_rect` behandelt NaN ausdrücklich als Treffer; der
/// Lauf muss trotzdem durchkommen, ohne zu paniken.
#[test]
fn nan_in_der_ctm_bricht_nicht_ab() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild("Scan")));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    // 1e200 * 1e200 = ∞, und ∞ * 0 in der Summe der Matrixmultiplikation = NaN.
    let (mut doc, _) = seite(
        doc,
        res,
        b"q 1e200 1e200 1e200 1e200 0 0 cm 1e200 0 0 0 0 0 cm /Im0 Do Q\n".to_vec(),
    );
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(
            &mut doc,
            &[schwaerzung(Rect::new(40.0, 590.0, 160.0, 710.0))],
        )
        .expect("der Lauf darf an einer NaN-CTM nicht scheitern");
    let _ = save_to_bytes(&doc).expect("und die Ausgabedatei entsteht");
    assert!(report.redacted_images <= 1);
}

// ===========================================================================
// Die Gegenrichtung an gewöhnlichem Material
// ===========================================================================

/// **Versucht zu widerlegen, nicht geschafft.** Ein Bild **neben** der Zone
/// (nicht darunter): 100 Punkte Abstand. Nichts darf fallen, nichts darf
/// gewarnt werden, der Ersatztext muss stehen bleiben.
#[test]
fn bild_neben_der_zone_verliert_nichts() {
    let (mut doc, bild_id) = seite_mit_bild("100 0 0 100 400 600", "Organigramm");
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(Rect::new(40.0, 590.0, 160.0, 710.0))],
    );
    assert_eq!(report.redacted_images, 0, "{:?}", report.warnings);
    assert_eq!(report.image_alt_texts_cleared, 0);
    assert!(gefallene(&out).is_empty(), "kein Bildpunkt fällt");
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    let alt = match aus.get_object(bild_id).expect("Bild") {
        Object::Stream(s) => s
            .dict
            .get(b"Alt")
            .ok()
            .and_then(|o| o.as_str().ok())
            .map(|b| String::from_utf8_lossy(b).into_owned()),
        _ => None,
    };
    assert_eq!(
        alt.as_deref(),
        Some("Organigramm"),
        "und der Ersatztext bleibt stehen"
    );
}

/// **Versucht zu widerlegen, nicht geschafft.** Ein 300-dpi-Scan (300 x 300
/// Bildpunkte auf 72 x 72 Punkte) mit einer kleinen Schwärzung darin: es darf
/// nur der Bereich fallen, nicht das Bild. Gemessen: der Anteil der gefallenen
/// Bildpunkte liegt zwischen dem, was die Fläche verlangt, und dem, was eine
/// Zelle Rand zulässt.
#[test]
fn dreihundert_dpi_scan_verliert_nur_den_bereich() {
    let mut doc = Document::with_version("1.5");
    let (breite, hoehe) = (300u32, 300u32);
    let mut data = Vec::with_capacity((breite * hoehe * 3) as usize);
    for y in 0..hoehe {
        for x in 0..breite {
            data.extend_from_slice(&[(x % 251) as u8, (y % 241) as u8, 0x40]);
        }
    }
    let mut stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(breite),
            "Height" => i64::from(hoehe),
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
        },
        data,
    );
    let _ = stream.compress();
    let bild_id = doc.add_object(Object::Stream(stream));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    // 72 x 72 Punkte bei (100, 600); die Zone deckt 12 x 6 Punkte, also ein
    // Sechstel der Breite und ein Zwölftel der Höhe = 1/72 der Fläche.
    let (mut doc, _) = seite(doc, res, b"q 72 0 0 72 100 600 cm /Im0 Do Q\n".to_vec());
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(Rect::new(112.0, 613.0, 124.0, 619.0))],
    );
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    let gezeigt = gezeigte_bildpunkte(&out, 0);
    assert_eq!(gezeigt.len(), 90_000, "Vorbedingung: 300 x 300 Bildpunkte");
    let gefallen = gezeigt
        .iter()
        .filter(|(x, y, farbe, _)| *farbe != [(*x % 251) as u8, (*y % 241) as u8, 0x40])
        .count();
    // 12 x 6 Punkte bei 300/72 Bildpunkten je Punkt = 50 x 25 = 1250 Zellen,
    // plus höchstens ein Ring von einer Zelle: (52 x 27) = 1404.
    assert!(
        (1250..=1404).contains(&gefallen),
        "gefallen: {gefallen} — erwartet 1250 (die Fläche) bis 1404 (mit einem \
         Ring Rand)"
    );
}
