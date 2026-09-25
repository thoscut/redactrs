//! Register #20, Runde III: **der Ersatztext eines Bildes hängt an gefallenen
//! Bildpunkten — nicht an einer Schätzung über die Platzierung.**
//!
//! Das Pendel schwang zweimal. Runde 1 entschied an der **Hülle** der
//! Bildplatzierung: ein um 45° gedrehtes Bild hat eine doppelt so große Hülle,
//! in deren Ecken gar kein Bild liegt — ein unversehrtes Bild verlor dort
//! seinen `/Alt`, ohne Warnung und ohne einen gewonnenen Bildpunkt. Runde 2
//! entschied am **Viereck** der Platzierung
//! ([`redact_pdf::content::ImagePlacement::covers`]) und riss damit das Leck
//! wieder auf: dieses Viereck vergleicht streng (Berührung zählt nicht),
//! `crate::image` füllt mit dem Rand eingeschlossen — wo eine Zellecke genau
//! auf dem Rand des Schwärzungsrechtecks liegt, fiel der Bildpunkt und der
//! Spiegel darüber blieb stehen.
//!
//! Beide Runden fragten *an der falschen Stelle*. Die Wahrheit kennt allein
//! `crate::image`: es überschreibt die Bildpunkte selbst und zählt sie
//! (`Work::filled`, geprüft an der Fläche jeder Pixelzelle im User-Space). Seit
//! dieser Runde gibt es sie heraus
//! ([`redact_pdf::image::ImageOutcome::page_image_hits`],
//! [`…::form_image_hits`](redact_pdf::image::ImageOutcome::form_image_hits),
//! [`…::undecodable_images`](redact_pdf::image::ImageOutcome::undecodable_images)),
//! und `crate::redact` entscheidet daran.
//!
//! Diese Datei nagelt die vier Aussagen fest, an denen sich die Runde messen
//! lässt:
//!
//! 1. **Was fällt, fällt vollständig** — auch am Rand, wo Runde 2 verneinte.
//! 2. **Was nicht fällt, bleibt ganz** — die leere Hüllenecke nimmt niemandem
//!    etwas, in beiden Betriebsarten.
//! 3. **Ein Rechteck ganz innerhalb einer Pixelzelle schwärzt diese Zelle.**
//!    Das war eine Lücke in `crate::image` selbst: ein grobes, groß gezogenes
//!    Bild behielt dort seine Bildpunkte, still und ohne Warnung — die
//!    Schwärzung lag nur obenauf.
//! 4. **Grob entscheiden ist erlaubt, wenn es gesagt wird**: beim unlesbaren
//!    Bild fällt der Ersatztext, und daneben steht eine Warnung. Ohne
//!    Zugeständnis bricht der Lauf ab.
//!
//! Gemessen wird am Leck-Orakel [`redact_pdf::leaks`] und an der geladenen
//! Ausgabedatei, nicht am Bericht des Programms.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfRedactor, RedactionReport};

/// Der Spiegel am `/Figure`-Abschnitt: was auf dem Bild zu lesen war.
const SPIEGEL: &str = "Kontoauszug, IBAN DE89 3704 0044 0532 0130 00";
/// Der Ersatztext am Bilddictionary selbst.
const BILD_ALT: &str = "Scan des Kontoauszugs";
/// Ein zweiter Spiegel für das Bild, das nichts verliert.
const SPIEGEL_B: &str = "Organigramm der Fachabteilung";

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

/// Ein RGB-Bild hinter `/FlateDecode`, mit `/Alt` am Dictionary.
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

/// Ein Bild, dessen Pixel sich **nicht** dekodieren lassen.
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

fn eine_seite(mut doc: Document, resources_id: ObjectId, content: Vec<u8>) -> (Document, ObjectId) {
    let content_id = doc.add_object(Stream::new(dictionary! {}, content));
    let pages_id = doc.new_object_id();
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

fn schwaerzung(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
            rect,
            None,
            Source::Manual {
                reason: "Register #20, Runde III".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Ohne `padding`: das Rechteck ist genau das, was der Test sagt.
fn schwaerze(
    doc: &mut Document,
    rects: &[Rect],
    zugestaendnis: bool,
) -> (RedactionReport, Vec<u8>) {
    let list: Vec<Redaction> = rects.iter().copied().map(schwaerzung).collect();
    let report = PdfRedactor::with_padding(0.0)
        .allowing_undecodable_images(zugestaendnis)
        .apply_with_report(doc, &list)
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

/// Der OCR-Hinweis steht in jedem Lauf mit einem Bild und hat nichts mit der
/// hier geprüften Entscheidung zu tun. Alles andere wäre eine neue Warnung.
fn nur_ocr_hinweis(report: &RedactionReport) -> bool {
    report
        .warnings
        .iter()
        .all(|w| w.contains("enthalten Rasterbilder"))
}

/// Ein Spiegel über genau einer Bildplatzierung, nichts sonst auf der Seite.
///
/// `ctm` ist der Text der `cm`-Operation; so lassen sich Drehung, Scherung und
/// Streckung im Test hinschreiben, wie ein Erzeuger sie schreibt.
fn seite_mit_bild(bild: Stream, ctm: &str, spiegel: &str) -> (Document, ObjectId, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    let content = format!("/Figure <</Alt ({spiegel})>> BDC\nq {ctm} cm /Im0 Do Q\nEMC\n");
    let (doc, page_id) = eine_seite(doc, resources_id, content.into_bytes());
    (doc, page_id, bild_id)
}

// ---------------------------------------------------------------------------
// 1. Was fällt, fällt vollständig — auch am Rand
// ---------------------------------------------------------------------------

/// Das Gegenstück zu `zj_b_flaeche_gegen_bildpunkt`: die CTM
/// `[200 100 -100 200 300 400]` über einem 2 x 2 Bild, und das Rechteck
/// `(400,400)-(480,450)`, dessen obere linke Ecke `(400,450)` **exakt** auf der
/// Kante P0→P1 des Vierecks liegt. Alle Zahlen sind in f64 exakt.
///
/// `crate::image` füllt dort (die Zellecke liegt im geschlossenen Rechteck),
/// also müssen Spiegel und Ersatztext mitgehen. Runde 2 verneinte hier, weil
/// sie dieselbe Frage mit strengen Vergleichen stellte.
///
/// Mutation, die diesen Test rot macht: in `redact.rs` `page_image_hits` wieder
/// aus `scan.images` und `placement.covers(rect)` bilden.
#[test]
fn randberuehrung_faellt_und_nimmt_spiegel_und_ersatztext_mit() {
    let (mut doc, _, bild_id) =
        seite_mit_bild(bild(2, 2, BILD_ALT), "200 100 -100 200 300 400", SPIEGEL);
    let vorher = pixel_bytes(&doc, bild_id);
    let (report, out) = schwaerze(&mut doc, &[Rect::new(400.0, 400.0, 480.0, 450.0)], false);

    assert_eq!(
        report.redacted_images, 1,
        "Vorbedingung: ein Bildpunkt fällt — {:?}",
        report.warnings
    );
    assert!(
        nur_ocr_hinweis(&report),
        "keine neue Warnung: {:?}",
        report.warnings
    );
    assert_ne!(
        pixel_bytes(&doc, bild_id),
        vorher,
        "Vorbedingung: die Bildpunkte im Objekt sind andere als vorher"
    );
    assert!(
        leaks(&out, SPIEGEL).is_empty(),
        "der Spiegel über den gefallenen Bildpunkten: {:?}",
        leaks(&out, SPIEGEL)
    );
    assert!(
        leaks(&out, BILD_ALT).is_empty(),
        "der Ersatztext am Bild: {:?}",
        leaks(&out, BILD_ALT)
    );
    assert_eq!(report.image_alt_texts_cleared, 1, "und der Bericht sagt es");
}

// ---------------------------------------------------------------------------
// 2. Was nicht fällt, bleibt ganz
// ---------------------------------------------------------------------------

/// Die andere Richtung, gleich gewichtet: ein um 45° gedrehtes Bild, die
/// Schwärzung in der **leeren Ecke seiner Hülle**. Dort liegt kein Bildpunkt;
/// es fällt keiner, es gibt keine Warnung — also behält das Bild seinen
/// Ersatztext und der Abschnitt seinen Spiegel. In **beiden** Betriebsarten:
/// die Entscheidung hängt nicht an `--allow-undecodable-images`.
///
/// Das ist der Befund aus Runde 1, hier als Wächter.
///
/// Mutation, die diesen Test rot macht: in `image.rs` `note_lost_pixels`
/// bedingungslos aufrufen, statt nur wenn `work.filled` gewachsen ist.
#[test]
fn leere_huellenecke_nimmt_niemandem_etwas() {
    for zugestaendnis in [false, true] {
        let (mut doc, _, bild_id) = seite_mit_bild(
            bild(32, 32, BILD_ALT),
            // Kantenlänge 100, 45°: Ecken (300,400) (370.7,470.7) (300,541.4)
            // (229.3,470.7). Die Hülle ist (229.3,400)-(370.7,541.4), ihre linke
            // untere Ecke ist leer.
            "70.7107 70.7107 -70.7107 70.7107 300 400",
            SPIEGEL,
        );
        let vorher = pixel_bytes(&doc, bild_id);
        let (report, out) = schwaerze(
            &mut doc,
            &[Rect::new(232.0, 402.0, 252.0, 422.0)],
            zugestaendnis,
        );

        assert_eq!(
            report.redacted_images, 0,
            "kein Bildpunkt fällt (Zugeständnis = {zugestaendnis}) — {:?}",
            report.warnings
        );
        assert!(
            nur_ocr_hinweis(&report),
            "keine Warnung (Zugeständnis = {zugestaendnis}): {:?}",
            report.warnings
        );
        assert_eq!(
            report.image_alt_texts_cleared, 0,
            "nichts fällt, also zählt nichts (Zugeständnis = {zugestaendnis})"
        );
        assert_eq!(
            pixel_bytes(&doc, bild_id),
            vorher,
            "die Bildpunkte sind unverändert (Zugeständnis = {zugestaendnis})"
        );
        let aus = load_from_bytes(&out).expect("Ausgabe lädt");
        assert_eq!(
            alt_am_objekt(&aus, bild_id).as_deref(),
            Some(BILD_ALT),
            "das unversehrte Bild behält seine Beschreibung \
             (Zugeständnis = {zugestaendnis})"
        );
        assert!(
            !leaks(&out, SPIEGEL).is_empty(),
            "der Spiegel über dem unversehrten Bild ist weg \
             (Zugeständnis = {zugestaendnis})"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Zwei Platzierungen desselben Bildes
// ---------------------------------------------------------------------------

/// Dasselbe Bildobjekt zweimal auf einer Seite, jedes Mal unter seinem eigenen
/// `/Figure`-Abschnitt; geschwärzt wird nur die erste Platzierung.
///
/// Geschwärzt werden die Bildpunkte **im Objekt** — und beide Platzierungen
/// zeichnen dasselbe Objekt. An der zweiten Stelle ist der schwarze Fleck
/// deshalb genauso zu sehen, und der Spiegel darüber beschreibt ein Bild, das
/// verloren hat. Er fällt mit. Das ist die bewusste Über-Schwärzung, dieselbe
/// wie bei einem mehrfach platzierten Form-XObject — nicht zu verwechseln mit
/// dem geteilten Logo über **zwei Seiten**, für das `crate::image` eine Kopie
/// anlegt und dessen unberührte Seite alles behält
/// (`zi_b_falschalarm_bild::geteiltes_logo_behaelt_seinen_ersatztext_auf_der_\
/// unberuehrten_seite`).
///
/// Mutation, die diesen Test rot macht: in `image.rs::note_lost_pixels` im
/// Zweig `Key::XObject` nur die Platzierungen mit Treffer vermerken statt alle
/// Platzierungen des Objekts.
#[test]
fn zweite_platzierung_desselben_objekts_zeigt_dieselben_gefallenen_bildpunkte() {
    let mut doc = Document::with_version("1.5");
    let im0 = doc.add_object(Object::Stream(bild(20, 20, BILD_ALT)));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    let content = format!(
        "/Figure <</Alt ({SPIEGEL})>> BDC\nq 100 0 0 100 50 600 cm /Im0 Do Q\nEMC\n\
         /Figure <</Alt ({SPIEGEL_B})>> BDC\nq 100 0 0 100 300 600 cm /Im0 Do Q\nEMC\n"
    );
    let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
    // Mitten in der ersten Platzierung.
    let (report, out) = schwaerze(&mut doc, &[Rect::new(80.0, 640.0, 110.0, 670.0)], false);

    assert_eq!(
        report.redacted_images, 1,
        "Vorbedingung: ein Bildobjekt verliert Bildpunkte — {:?}",
        report.warnings
    );
    assert!(
        leaks(&out, SPIEGEL).is_empty(),
        "der Spiegel über der geschwärzten Platzierung: {:?}",
        leaks(&out, SPIEGEL)
    );
    assert!(
        leaks(&out, SPIEGEL_B).is_empty(),
        "der Spiegel über der zweiten Platzierung **desselben** Objekts steht \
         noch in der Datei, obwohl dort derselbe schwarze Fleck zu sehen ist: {:?}",
        leaks(&out, SPIEGEL_B)
    );
    assert!(
        leaks(&out, BILD_ALT).is_empty(),
        "der Ersatztext am Bild: {:?}",
        leaks(&out, BILD_ALT)
    );
}

/// Die Gegenprobe dazu, damit der Test darüber nicht aus dem falschen Grund
/// grün wird: **zwei verschiedene** Bildobjekte, sonst dieselbe Seite. Das
/// unbeteiligte Bild behält alles.
#[test]
fn zweites_bildobjekt_daneben_behaelt_alles() {
    let mut doc = Document::with_version("1.5");
    let im0 = doc.add_object(Object::Stream(bild(20, 20, BILD_ALT)));
    let im1 = doc.add_object(Object::Stream(bild(20, 20, SPIEGEL_B)));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0, "Im1" => im1 },
    });
    let content = format!(
        "/Figure <</Alt ({SPIEGEL})>> BDC\nq 100 0 0 100 50 600 cm /Im0 Do Q\nEMC\n\
         /Figure <</Alt ({SPIEGEL_B})>> BDC\nq 100 0 0 100 300 600 cm /Im1 Do Q\nEMC\n"
    );
    let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
    let vorher = pixel_bytes(&doc, im1);
    let (report, out) = schwaerze(&mut doc, &[Rect::new(80.0, 640.0, 110.0, 670.0)], false);

    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    assert_eq!(report.image_alt_texts_cleared, 1, "genau ein Abschnitt");
    assert!(
        leaks(&out, SPIEGEL).is_empty(),
        "{:?}",
        leaks(&out, SPIEGEL)
    );
    assert_eq!(
        pixel_bytes(&doc, im1),
        vorher,
        "das unbeteiligte Bild behält seine Bildpunkte"
    );
    assert!(
        !leaks(&out, SPIEGEL_B).is_empty(),
        "der Spiegel über dem unbeteiligten Bild muss bleiben"
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(
        alt_am_objekt(&aus, im1).as_deref(),
        Some(SPIEGEL_B),
        "und sein /Alt am Dictionary auch"
    );
}

// ---------------------------------------------------------------------------
// 4. Ein Rechteck ganz innerhalb einer Pixelzelle
// ---------------------------------------------------------------------------

/// **Die Lücke in `crate::image` selbst.** Ein 2 x 2 Bild auf 100 x 100 Punkte
/// gezogen: eine Pixelzelle ist 50 Punkte groß, ihre Ecken liegen bei
/// x ∈ {50,100,150}, y ∈ {600,650,700}. Das Rechteck (145,645)-(149,649) liegt
/// **ganz innerhalb** der Zelle rechts oben — keine Zellecke liegt darin.
///
/// `Work::covers` prüfte nur die vier Ecken der Zelle gegen das Rechteck und
/// verneinte: kein Bildpunkt fiel, die Schwärzung lag nur obenauf, und es gab
/// keine Warnung. Genau die Umkehrung des Kernversprechens — und genau die
/// Zusicherung, die dieselbe Funktion gab („im Zweifel wird ein Pixel zu viel
/// geschwärzt statt eines zu wenig“).
///
/// Jetzt wird die Zelle als **Fläche** gegen das Rechteck geprüft: der
/// Bildpunkt fällt, und Spiegel und Ersatztext fallen mit ihm — in beiden
/// Betriebsarten gleich, denn hier ist nichts geschätzt.
///
/// Mutation, die diesen Test rot macht: in `image.rs::Work::covers` wieder
/// `[(u0,v0),(u1,v0),(u0,v1),(u1,v1)].any(|p| rect.contains(ctm.apply(p)))`
/// einsetzen.
#[test]
fn rechteck_innerhalb_einer_pixelzelle_schwaerzt_diese_zelle() {
    for zugestaendnis in [false, true] {
        let (mut doc, _, bild_id) =
            seite_mit_bild(bild(2, 2, BILD_ALT), "100 0 0 100 50 600", SPIEGEL);
        let vorher = pixel_bytes(&doc, bild_id);
        let (report, out) = schwaerze(
            &mut doc,
            &[Rect::new(145.0, 645.0, 149.0, 649.0)],
            zugestaendnis,
        );

        assert_eq!(
            report.redacted_images, 1,
            "die Bildpunkte unter der Schwärzung müssen fallen, auch wenn das \
             Rechteck kleiner ist als eine Pixelzelle (Zugeständnis = \
             {zugestaendnis}) — {:?}",
            report.warnings
        );
        assert!(
            nur_ocr_hinweis(&report),
            "keine Warnung (Zugeständnis = {zugestaendnis}): {:?}",
            report.warnings
        );
        assert_ne!(
            pixel_bytes(&doc, bild_id),
            vorher,
            "die Bildpunkte im Objekt sind andere als vorher \
             (Zugeständnis = {zugestaendnis})"
        );
        assert!(
            leaks(&out, SPIEGEL).is_empty(),
            "der Spiegel fällt mit den Bildpunkten (Zugeständnis = \
             {zugestaendnis}): {:?}",
            leaks(&out, SPIEGEL)
        );
        assert!(
            leaks(&out, BILD_ALT).is_empty(),
            "und der Ersatztext am Bild (Zugeständnis = {zugestaendnis}): {:?}",
            leaks(&out, BILD_ALT)
        );
        assert_eq!(
            report.image_alt_texts_cleared, 1,
            "ein Abschnitt, ein entfernter Schlüssel (Zugeständnis = {zugestaendnis})"
        );
    }
}

/// Die Gegenprobe: dasselbe grobe Bild, das Rechteck jetzt **neben** dem Bild.
/// Dann fällt kein Bildpunkt, und nichts wird weggenommen — damit der Test
/// darüber nicht bloß „schwärzt immer alles“ prüft.
#[test]
fn grobes_bild_neben_der_schwaerzung_behaelt_alles() {
    let (mut doc, _, bild_id) = seite_mit_bild(bild(2, 2, BILD_ALT), "100 0 0 100 50 600", SPIEGEL);
    let vorher = pixel_bytes(&doc, bild_id);
    // Das Bild liegt bei (50,600)-(150,700).
    let (report, out) = schwaerze(&mut doc, &[Rect::new(200.0, 645.0, 260.0, 649.0)], false);
    assert_eq!(report.redacted_images, 0, "{:?}", report.warnings);
    assert_eq!(report.image_alt_texts_cleared, 0);
    assert_eq!(pixel_bytes(&doc, bild_id), vorher);
    assert!(!leaks(&out, SPIEGEL).is_empty(), "der Spiegel muss bleiben");
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(alt_am_objekt(&aus, bild_id).as_deref(), Some(BILD_ALT));
}

// ---------------------------------------------------------------------------
// 5. Grob entscheiden ist erlaubt, wenn es gesagt wird
// ---------------------------------------------------------------------------

/// Das unlesbare Bild: niemand weiß, was unter der Schwärzung lag, weil es sich
/// nicht auspacken lässt. Hier **darf** grob entschieden werden — Spiegel und
/// Ersatztext fallen, obwohl kein Bildpunkt fiel —, und zwar weil daneben eine
/// Warnung steht, die das Bild benennt.
///
/// Der Unterschied zu Runde 1 ist genau dieser Satz: grob entscheiden ist
/// erlaubt, wenn es gesagt wird; still grob entscheiden nicht.
///
/// Mutation, die diesen Test rot macht: in `image.rs` den Aufruf von
/// `note_undecided` im Zweig `raster.placeholder` weglassen.
#[test]
fn unlesbares_bild_entscheidet_grob_und_sagt_es() {
    let (mut doc, _, bild_id) =
        seite_mit_bild(unlesbares_bild(BILD_ALT), "100 0 0 100 50 600", SPIEGEL);
    let vorher = pixel_bytes(&doc, bild_id);
    let (report, out) = schwaerze(&mut doc, &[Rect::new(75.0, 650.0, 100.0, 675.0)], true);

    assert_eq!(
        report.redacted_images, 0,
        "Vorbedingung: die Bildpunkte bleiben stehen"
    );
    assert_eq!(
        pixel_bytes(&doc, bild_id),
        vorher,
        "Vorbedingung: das Bildobjekt ist unverändert"
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("nicht dekodieren")),
        "und **es wird gesagt**: {:?}",
        report.warnings
    );
    assert!(
        leaks(&out, SPIEGEL).is_empty(),
        "der Spiegel fällt trotzdem: {:?}",
        leaks(&out, SPIEGEL)
    );
    assert!(
        leaks(&out, BILD_ALT).is_empty(),
        "und der Ersatztext am stehengebliebenen Dictionary: {:?}",
        leaks(&out, BILD_ALT)
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(alt_am_objekt(&aus, bild_id), None);
    assert_eq!(
        report.image_alt_texts_cleared, 2,
        "beides gezählt: der Spiegel im Strom und das /Alt am Dictionary"
    );
}

/// Ohne das Zugeständnis bricht derselbe Lauf ab: eine Datei, die aussieht wie
/// geschwärzt, ist schlimmer als keine. Nichts wird grob entschieden, weil
/// überhaupt nichts ausgegeben wird.
#[test]
fn unlesbares_bild_ohne_zugestaendnis_gibt_keine_datei_aus() {
    let (mut doc, _, _) = seite_mit_bild(unlesbares_bild(BILD_ALT), "100 0 0 100 50 600", SPIEGEL);
    let fehler = PdfRedactor::with_padding(0.0)
        .apply_with_report(
            &mut doc,
            &[schwaerzung(Rect::new(75.0, 650.0, 100.0, 675.0))],
        )
        .expect_err("der Lauf muss abbrechen");
    let text = fehler.to_string();
    assert!(
        text.contains("nicht dekodieren"),
        "der Grund muss dastehen: {text}"
    );
    assert!(
        text.contains("--allow-undecodable-images"),
        "und der Fluchtweg: {text}"
    );
}

/// Dasselbe unlesbare Bild auf einer Seite **ohne** Marked Content — ein
/// gewöhnliches, nicht getaggtes PDF. Der Ersatztext am Bilddictionary fällt
/// auch dort: die Liste der stehengebliebenen Bilder kommt aus `crate::image`
/// und nicht aus dem Seiten-Scan, hängt also nicht daran, dass die Datei
/// getaggt ist.
///
/// Mutation, die diesen Test rot macht: `blacked_images` in `redact.rs` wieder
/// aus `scan.images` füllen (`ScanResult::images` verwirft die Platzierung ohne
/// `BDC` im Strom nicht mehr — aber die Verkettung über den Scan hing daran).
#[test]
fn unlesbares_bild_ohne_auszeichnung_verliert_sein_alt() {
    let mut doc = Document::with_version("1.5");
    let im0 = doc.add_object(Object::Stream(unlesbares_bild(BILD_ALT)));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    let (mut doc, _) = eine_seite(
        doc,
        resources_id,
        b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
    );
    let (report, out) = schwaerze(&mut doc, &[Rect::new(75.0, 650.0, 100.0, 675.0)], true);
    assert_eq!(report.redacted_images, 0, "die Pixel bleiben (unlesbar)");
    assert!(
        leaks(&out, BILD_ALT).is_empty(),
        "der Ersatztext am Bilddictionary: {:?}",
        leaks(&out, BILD_ALT)
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(alt_am_objekt(&aus, im0), None);
    assert_eq!(
        report.image_alt_texts_cleared, 1,
        "nur das /Alt am Dictionary"
    );
}
