//! Gegenprüfung L-B, Linse **WIDERLEGEN** zu Register #20, Runde III.
//!
//! Das Pendel dieses Gebiets schlug zweimal: Runde 1 entschied über den
//! Ersatztext eines Bildes an der **Hülle** der Platzierung (Fehlalarm — ein
//! unversehrtes gedrehtes Bild verlor seinen `/Alt`), Runde 2 am **Viereck**
//! der Platzierung (Leck — am Rand fiel der Bildpunkt und der Spiegel blieb
//! stehen). Runde 3 liest die Antwort dort ab, wo die Pixel überschrieben
//! werden (`ImageOutcome::page_image_hits` / `::form_image_hits` /
//! `::undecodable_images`).
//!
//! Hier werden **beide** Richtungen an eigenem Material nachgemessen, jede mit
//! ihrer Gegenprobe. Maßstab ist das Leck-Orakel `redact_pdf::leaks` und die
//! geladene Ausgabedatei, nicht der Bericht.
//!
//! Alle Zahlen sind in f64 exakt gewählt (ganze Zahlen und Zweierpotenzen),
//! damit kein Befund von einer Rundung abhängt.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfRedactor, RedactionReport};

/// Der Spiegel am `/Figure`-Abschnitt über der getroffenen Platzierung.
const SPIEGEL: &str = "Kontoauszug, IBAN DE89 3704 0044 0532 0130 00";
/// Der Ersatztext am Bilddictionary selbst.
const BILD_ALT: &str = "Scan des Kontoauszugs";
/// Der Spiegel über einer Platzierung, die nichts verliert.
const SPIEGEL_B: &str = "Organigramm der Fachabteilung";

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

fn bild(width: u32, height: u32, alt: &str) -> Stream {
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            data.extend_from_slice(&[(x * 11 + 3) as u8, (y * 7 + 5) as u8, 0x40]);
        }
    }
    let mut stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(width),
            "Height" => i64::from(height),
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Alt" => Object::string_literal(alt),
        },
        data,
    );
    let _ = stream.compress();
    stream
}

/// Ein Bild, dessen Pixel sich **nicht** dekodieren lassen — ein
/// `/JPXDecode`-Scan, wie ihn ein Gerät liefert.
fn unlesbares_bild(alt: &str) -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 20_i64,
            "Height" => 20_i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Filter" => "JPXDecode",
            "Alt" => Object::string_literal(alt),
        },
        b"kein JPEG-2000".to_vec(),
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
                reason: "Gegenprüfung L-B".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Ohne `padding`: das Rechteck ist genau das, was der Test hinschreibt.
fn schwaerze(
    doc: &mut Document,
    list: &[Redaction],
    zugestaendnis: bool,
) -> (RedactionReport, Vec<u8>) {
    let report = PdfRedactor::with_padding(0.0)
        .allowing_undecodable_images(zugestaendnis)
        .apply_with_report(doc, list)
        .expect("Schwärzung läuft");
    let bytes = save_to_bytes(doc).expect("Speichern");
    (report, bytes)
}

fn alt_am_objekt(doc: &Document, id: ObjectId) -> Option<String> {
    let dict: &Dictionary = match doc.get_object(id).ok()? {
        Object::Stream(stream) => &stream.dict,
        Object::Dictionary(dict) => dict,
        _ => return None,
    };
    dict.get(b"Alt")
        .ok()
        .and_then(|value| value.as_str().ok())
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

fn pixel_bytes(doc: &Document, id: ObjectId) -> Vec<u8> {
    match doc.get_object(id).expect("Bild") {
        Object::Stream(stream) => stream.content.clone(),
        other => panic!("kein Strom: {other:?}"),
    }
}

/// Eine Seite, ein Spiegel, eine Bildplatzierung.
fn seite_mit_bild(bild: Stream, ctm: &str, spiegel: &str) -> (Document, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    let content = format!("/Figure <</Alt ({spiegel})>> BDC\nq {ctm} cm /Im0 Do Q\nEMC\n");
    let (doc, _) = seiten(doc, vec![(resources_id, content.into_bytes())]);
    (doc, bild_id)
}

/// Die 45°-Drehung aus `zk_b`: Kantenlänge 100, Ecken (300,400) (370.7,470.7)
/// (300,541.4) (229.3,470.7). Die Hülle ist (229.3,400)-(370.7,541.4); ihre
/// linke untere Ecke ist **leer** — dort liegt kein Bildpunkt.
const GEDREHT: &str = "70.7107 70.7107 -70.7107 70.7107 300 400";
/// Ein Rechteck in genau dieser leeren Hüllenecke.
fn leere_huellenecke() -> Rect {
    Rect::new(232.0, 402.0, 252.0, 422.0)
}

// ===========================================================================
// Richtung FEHLALARM: was nicht gefallen ist, darf nicht weggenommen werden
// ===========================================================================

/// **Der Befund.** Dasselbe Material wie
/// `zk_b::leere_huellenecke_nimmt_niemandem_etwas`, nur ist das Bild ein
/// `/JPXDecode`-Scan: die Schwärzung liegt in der **leeren Ecke der Hülle** des
/// gedrehten Bildes, also nachweislich **nicht** auf dem Bild.
///
/// Dass sich die Pixel nicht auspacken lassen, ändert daran nichts: das
/// Viereck der Platzierung ist bekannt, ohne ein einziges Byte zu dekodieren
/// (`content::ImagePlacement::covers` beantwortet genau das). Kein Bildpunkt
/// dieses Bildes liegt unter der Schwärzung — nicht „unbekannt“, sondern
/// **keiner**.
///
/// Runde 2 entschied hier am Viereck und ließ `/Alt` und Spiegel stehen. Runde
/// 3 entscheidet über `ImageOutcome::undecodable_images`, und diese Liste
/// entsteht hinter dem Kandidatenfilter von `image.rs`, der die **Hülle**
/// fragt (`fill_page`: `ctm_bounds(&placement.ctm).intersects(&z.rect)`).
/// Damit ist der Fehlalarm aus Runde 1 wieder da.
#[test]
fn unlesbares_bild_in_der_leeren_huellenecke_behaelt_seinen_ersatztext() {
    let (mut doc, bild_id) = seite_mit_bild(unlesbares_bild(BILD_ALT), GEDREHT, SPIEGEL);
    let vorher = pixel_bytes(&doc, bild_id);

    // Vorbedingung, und der Kern des Befunds: **ohne ein Byte zu dekodieren**
    // ist entschieden, dass hier kein Bildpunkt liegt. Die Hülle wird
    // getroffen (der Vorfilter von `image.rs` lässt die Platzierung durch),
    // das Viereck nicht.
    let seite = *doc.get_pages().values().next().expect("eine Seite");
    let scan = redact_pdf::scan_page(&doc, seite).expect("Scan läuft");
    let platzierung = &scan.images[0];
    assert!(
        platzierung.bounds.intersects(&leere_huellenecke()),
        "Vorbedingung: die Hülle wird getroffen"
    );
    assert!(
        !platzierung.covers(&leere_huellenecke()),
        "Vorbedingung: das Viereck der Platzierung wird NICHT getroffen — \
         kein Bildpunkt kann unter der Schwärzung liegen, und das steht ohne \
         Dekodieren fest"
    );

    let (report, out) = schwaerze(&mut doc, &[schwaerzung(0, leere_huellenecke())], true);

    // Vorbedingung: es fällt kein Bildpunkt (es kann keiner fallen).
    assert_eq!(
        report.redacted_images, 0,
        "Vorbedingung: nichts überschrieben — {:?}",
        report.warnings
    );
    assert_eq!(
        pixel_bytes(&doc, bild_id),
        vorher,
        "Vorbedingung: das Bildobjekt ist unverändert"
    );

    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(
        alt_am_objekt(&aus, bild_id).as_deref(),
        Some(BILD_ALT),
        "die Schwärzung liegt in der leeren Hüllenecke: kein Bildpunkt kann \
         dort liegen, also darf der Ersatztext nicht fallen. Warnungen: {:?}",
        report.warnings
    );
    assert!(
        !leaks(&out, SPIEGEL).is_empty(),
        "und der Spiegel über dem unversehrten Bild muss stehen bleiben"
    );
    assert_eq!(
        report.image_alt_texts_cleared, 0,
        "nichts fällt, also zählt nichts"
    );
}

/// Die Gegenprobe zum Befund: dieselbe Datei, dieselbe Betriebsart, das
/// Rechteck jetzt **mitten** im gedrehten unlesbaren Bild. Dort weiß niemand,
/// was unter der Schwärzung lag — hier **darf** grob entschieden werden, und
/// hier steht die Warnung dafür. Das muss grün bleiben, damit der Befund
/// darüber nicht bloß „entscheidet nie etwas“ verlangt.
#[test]
fn unlesbares_bild_mitten_getroffen_verliert_seinen_ersatztext() {
    let (mut doc, bild_id) = seite_mit_bild(unlesbares_bild(BILD_ALT), GEDREHT, SPIEGEL);
    // (300,470) liegt im Inneren der Raute.
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(290.0, 460.0, 310.0, 480.0))],
        true,
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("nicht dekodieren")),
        "grob entschieden, und es wird gesagt: {:?}",
        report.warnings
    );
    assert!(leaks(&out, BILD_ALT).is_empty(), "der Ersatztext fällt");
    assert!(leaks(&out, SPIEGEL).is_empty(), "und der Spiegel mit ihm");
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(alt_am_objekt(&aus, bild_id), None);
}

/// Zweiter Fehlalarm, gefunden an derselben Naht: `note_lost_pixels` vermerkt
/// **jede** Platzierung desselben Bildobjekts auf der Seite und begründet das
/// damit, dass der Ressourcenverweis der Seite auf die Kopie zeigt „und damit
/// alle Platzierungen **dieses Namens**“.
///
/// Vermerkt wird aber nach **Objekt-Id**, umgebogen wird nach **Name**
/// (`repoint_page` setzt genau einen Schlüssel). Trägt dieselbe Seite das Bild
/// unter zwei Namen — `/Resources /XObject << /Im0 5 0 R /Im1 5 0 R >>`, eine
/// gewöhnliche Folge von Seitenzusammenführung —, dann zeigt `/Im1` nach dem
/// Lauf weiter auf das **unversehrte** Original: seine Bildpunkte sind alle
/// noch da. Sein Spiegel fällt trotzdem.
///
/// (Die zweite Hälfte desselben Fadens gehört nicht hierher, steht aber im
/// Bericht: unter `/Im1` ist auf **derselben Seite** weiter zu sehen, was unter
/// `/Im0` geschwärzt wurde.)
#[test]
fn zweiter_name_desselben_bildes_behaelt_seinen_spiegel() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild(4, 4, BILD_ALT)));
    let res1 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id, "Im1" => bild_id },
    });
    let res2 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    let inhalt1 = format!(
        "/Figure <</Alt ({SPIEGEL})>> BDC\nq 100 0 0 100 50 600 cm /Im0 Do Q\nEMC\n\
         /Figure <</Alt ({SPIEGEL_B})>> BDC\nq 100 0 0 100 400 600 cm /Im1 Do Q\nEMC\n"
    );
    let inhalt2 = b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec();
    let (mut doc, ids) = seiten(doc, vec![(res1, inhalt1.into_bytes()), (res2, inhalt2)]);
    // Vorbedingung: das Viereck der zweiten Platzierung liegt nirgends in der
    // Nähe der Schwärzung.
    let scan = redact_pdf::scan_page(&doc, ids[0]).expect("Scan läuft");
    assert_eq!(scan.images.len(), 2, "Vorbedingung: zwei Platzierungen");
    assert!(
        !scan.images[1].covers(&Rect::new(60.0, 610.0, 80.0, 630.0)),
        "Vorbedingung: /Im1 wird von der Schwärzung nicht berührt"
    );

    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(60.0, 610.0, 80.0, 630.0))],
        false,
    );

    // Vorbedingung: das Bild wird kopiert (es hängt an zwei Seiten), das
    // Original bleibt unversehrt.
    assert_eq!(report.redacted_images, 1, "Vorbedingung: etwas fiel");
    assert_eq!(
        report.copied_images, 1,
        "Vorbedingung: das geteilte Bild wurde kopiert, nicht überschrieben"
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(
        alt_am_objekt(&aus, bild_id).as_deref(),
        Some(BILD_ALT),
        "Vorbedingung: das Original ist unangetastet (Seite 2 zeigt es)"
    );
    // Und /Im1 zeigt auf Seite 1 weiter auf dieses Original: `repoint_page`
    // setzt genau **einen** Namen.
    let xobjects = aus
        .get_dictionary(ids[0])
        .and_then(|d| d.get(b"Resources"))
        .and_then(|o| aus.dereference(o).map(|(_, r)| r))
        .and_then(|o| o.as_dict())
        .and_then(|d| d.get(b"XObject"))
        .and_then(|o| aus.dereference(o).map(|(_, r)| r))
        .and_then(|o| o.as_dict())
        .expect("XObject-Verzeichnis der Seite 1")
        .clone();
    assert_eq!(
        xobjects.get(b"Im1").ok().and_then(|o| match o {
            Object::Reference(id) => Some(*id),
            _ => None,
        }),
        Some(bild_id),
        "Vorbedingung: /Im1 zeigt weiter auf das unversehrte Original"
    );
    assert_ne!(
        xobjects.get(b"Im0").ok().and_then(|o| match o {
            Object::Reference(id) => Some(*id),
            _ => None,
        }),
        Some(bild_id),
        "Vorbedingung: /Im0 zeigt auf die geschwärzte Kopie"
    );
    // Der Befund, und zwar so formuliert, dass ihn keine der beiden möglichen
    // Auflösungen umgeht: die Datei widerspricht sich. Entweder gehören die
    // Bildpunkte unter `/Im1` auch geschwärzt (dann darf der Spiegel fallen),
    // oder sie sind unversehrt (dann muss der Spiegel stehen bleiben).
    // Beides zugleich — Spiegel weg **und** ungeschwärzte Bildpunkte auf
    // derselben Seite sichtbar — ist Fehlalarm und Leck in einem.
    let spiegel_weg = leaks(&out, SPIEGEL_B).is_empty();
    let pixel_unversehrt = pixel_bytes(&aus, bild_id) == pixel_bytes(&doc, bild_id);
    assert!(
        !(spiegel_weg && pixel_unversehrt),
        "Spiegel über /Im1 gefallen: {spiegel_weg}; die Bildpunkte, die /Im1 auf \
         Seite 1 zeichnet, unversehrt: {pixel_unversehrt}. Vermerkt wird nach \
         Objekt-Id (image.rs::note_lost_pixels), umgebogen nach Name \
         (image.rs::repoint_page) — Warnungen: {:?}",
        report.warnings
    );
}

// ===========================================================================
// Richtung LECK: was gefallen ist, muss vollständig fallen
// ===========================================================================

/// Der neue Flächentest je Pixelzelle muss auch den Fall treffen, in dem
/// Rechteck und Zelle sich **kreuzen**, ohne dass eine Ecke der einen in der
/// anderen liegt: ein 2 x 2 Bild auf 100 x 100 Punkte (Zellen 50 x 50, Ecken
/// bei x ∈ {50,100,150}, y ∈ {600,650,700}), und ein Streifen, der die obere
/// rechte Zelle waagerecht ganz durchquert, aber senkrecht in ihr bleibt.
///
/// Weder liegt eine Zellecke im Rechteck noch eine Rechteckecke in der Zelle.
/// Die vier-Ecken-Prüfung aus Runde 2 verneinte hier; die Bildpunkte blieben
/// still in der Datei.
#[test]
fn streifen_quer_durch_eine_pixelzelle_schwaerzt_sie() {
    let (mut doc, bild_id) = seite_mit_bild(bild(2, 2, BILD_ALT), "100 0 0 100 50 600", SPIEGEL);
    let vorher = pixel_bytes(&doc, bild_id);
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(90.0, 660.0, 160.0, 680.0))],
        false,
    );
    assert_eq!(
        report.redacted_images, 1,
        "der Streifen liegt auf Bildpunkten — {:?}",
        report.warnings
    );
    assert_ne!(pixel_bytes(&doc, bild_id), vorher);
    assert!(
        leaks(&out, SPIEGEL).is_empty(),
        "der Spiegel fällt mit: {:?}",
        leaks(&out, SPIEGEL)
    );
    assert!(leaks(&out, BILD_ALT).is_empty(), "und der Ersatztext");
}

/// Ein `/Figure`-Abschnitt in einem Form-XObject, das **kein einziges Zeichen**
/// enthält — nur das Bild. Das Formular hat damit keinen Glyphenplan; es wird
/// nur dann überhaupt neu geschrieben, wenn die Bildfrage es hineinzieht.
#[test]
fn formular_nur_mit_bild_verliert_seinen_spiegel() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild(4, 4, BILD_ALT)));
    let form_res = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    let form_inhalt =
        format!("/Figure <</Alt ({SPIEGEL})>> BDC\nq 100 0 0 100 0 0 cm /Im0 Do Q\nEMC\n");
    let form_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => form_res,
        },
        form_inhalt.into_bytes(),
    ));
    let res = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => form_id },
    });
    let (mut doc, _) = seiten(
        doc,
        vec![(res, b"q 1 0 0 1 50 600 cm /Fm0 Do Q\n".to_vec())],
    );
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(60.0, 610.0, 80.0, 630.0))],
        false,
    );
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    assert!(
        leaks(&out, SPIEGEL).is_empty(),
        "der Spiegel im Formular fällt mit den Bildpunkten: {:?}",
        leaks(&out, SPIEGEL)
    );
    assert!(
        leaks(&out, BILD_ALT).is_empty(),
        "und der Ersatztext am Bild"
    );
}

/// Ein Spiegel mit `/ActualText` statt `/Alt`, und dazu noch über einem
/// **Inline-Bild** im Seitenstrom — der andere der beiden Wege in
/// `note_lost_pixels`.
#[test]
fn inline_bild_nimmt_seinen_actualtext_mit() {
    let mut doc = Document::with_version("1.5");
    let res = doc.add_object(dictionary! {});
    // 2 x 2 RGB, unkomprimiert: 12 Byte Nutzdaten.
    let pixel: Vec<u8> = vec![
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC,
    ];
    let mut inhalt = format!("/Span <</ActualText ({SPIEGEL})>> BDC\nq 100 0 0 100 50 600 cm\nBI /W 2 /H 2 /CS /RGB /BPC 8 ID ")
        .into_bytes();
    inhalt.extend_from_slice(&pixel);
    inhalt.extend_from_slice(b"\nEI\nQ\nEMC\n");
    let (mut doc, _) = seiten(doc, vec![(res, inhalt)]);
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(60.0, 610.0, 80.0, 630.0))],
        false,
    );
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    assert!(
        leaks(&out, SPIEGEL).is_empty(),
        "der /ActualText über dem Inline-Bild fällt mit: {:?}",
        leaks(&out, SPIEGEL)
    );
}

/// Dieselbe leere Hüllenecke in der **voreingestellten** Betriebsart, ohne
/// `--allow-undecodable-images`: dort endet der Lauf mit einem Fehler und
/// **ohne Ausgabedatei** — wegen eines Bildes, das die Schwärzung nachweislich
/// nicht berührt (`covers == false`, siehe oben).
///
/// Das ist der Vorfilter von `image.rs` (`ctm_bounds().intersects()`), also
/// älter als diese Runde; es steht hier, weil es dieselbe Naht ist und weil
/// eine Grenze, die gewöhnliche Arbeit ablehnt, genauso ein Fehler ist wie eine
/// Lücke: ein entzerrter `/JPXDecode`-Scan und eine Schwärzung daneben sind
/// gewöhnliches Material.
#[test]
fn leere_huellenecke_bricht_im_strengen_modus_nicht_ab() {
    let (mut doc, _) = seite_mit_bild(unlesbares_bild(BILD_ALT), GEDREHT, SPIEGEL);
    let ergebnis = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &[schwaerzung(0, leere_huellenecke())]);
    match ergebnis {
        Ok(_) => {}
        Err(e) => panic!("der Lauf bricht ab, obwohl die Schwärzung das Bild nicht berührt: {e}"),
    }
}
