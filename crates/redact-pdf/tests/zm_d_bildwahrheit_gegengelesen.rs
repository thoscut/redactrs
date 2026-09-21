//! Gegenprüfung D der Runde 9 zum CHANGELOG-Abschnitt **Fix-Runde 8**:
//! die Behauptungen über Bild, Hülle, Pixelzelle, Viereck und Spiegel — an
//! eigenem Material, gemessen an den **Bytes des Bildobjekts** und am
//! Leck-Orakel [`redact_pdf::leaks`], nie am Bericht des Programms.
//!
//! Geprüfte Sätze des Abschnitts:
//!
//! * „Bei einem gedrehten Bild ist die **Hülle** größer als das Bild, und in
//!   ihren Ecken liegt gar kein Bildpunkt" → [`huellenecke_ohne_bildpunkt`].
//! * „die Prüfung je Pixelzelle sah nur deren **Ecken**: ein
//!   Schwärzungsrechteck ganz zwischen den Gitterlinien … traf keine" → der
//!   **neue** Zustand in [`rechteck_zwischen_den_gitterlinien_trifft_jetzt`];
//!   derselbe Test rechnet die **alte** Vier-Ecken-Frage im Test nach und
//!   zeigt, dass sie hier nichts getroffen hätte.
//! * „Berührung zählt als Treffer" → [`beruehrung_am_rand_haengt_am_groben_vorfilter`]:
//!   der Satz gilt **nur hinter** dem groben Vorfilter, der die Hülle mit
//!   `Rect::intersects` fragt („Berührung zählt nicht").
//! * „Jetzt entscheidet **eine** Flächenfrage" → gebrochen in
//!   [`zwei_flaechenfragen_brechen_den_lauf_ab`]: es sind zwei, sie antworten am
//!   Rand verschieden, und der Unterschied kostet die Ausgabedatei.
//! * „der Kandidatenfilter fragt das **Viereck** der Platzierung statt ihrer
//!   Hülle" → [`viereck_statt_huelle_beim_unlesbaren_bild`] (ohne
//!   Zugeständnis: am Viereck läuft der Lauf durch, an der Hülle bräche er ab).
//! * „`crate::image` berichtet je Platzierung, ob dort Bildpunkte fielen" →
//!   [`der_bildlauf_berichtet_je_platzierung`] (öffentliches
//!   [`redact_pdf::image::redact_images`], kein Umweg über einen Bericht).
//! * „`repoint_page` setzt genau **einen** Namen" / „eine Stelle beantwortet
//!   die Frage, wohin die geschwärzten Bildpunkte kommen" →
//!   [`zweiter_name_behaelt_bildpunkte_und_spiegel`].
//! * „Das `/Alt` am **Bilddictionary** fällt weiter für alle Platzierungen" →
//!   [`alt_am_bilddictionary_faellt_fuer_alle_platzierungen`].
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-pdf --test zm_d_bildwahrheit_gegengelesen`

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Point, Rect, Redaction, Region, Source};
use redact_pdf::image::{redact_images, ImageOptions};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfRedactor, RedactionReport};

/// Der Spiegel über der ersten Platzierung — was auf dem Bild zu lesen war.
const SPIEGEL_A: &str = "Auszug Maerz, IBAN DE02 1203 0000 0000 2020 51";
/// Der Spiegel über der zweiten Platzierung.
const SPIEGEL_B: &str = "Wappen der Sparkasse Musterstadt";
/// Der Spiegel auf der zweiten Seite.
const SPIEGEL_C: &str = "Dasselbe Wappen, Seite zwei";
/// Der Ersatztext am Bilddictionary selbst.
const BILD_ALT: &str = "Gescanntes Deckblatt";

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

/// Ein RGB-Bild hinter `/FlateDecode`, mit `/Alt` am Dictionary.
fn bild(width: u32, height: u32, alt: &str) -> Stream {
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            data.extend_from_slice(&[(x * 17 + 9) as u8, (y * 5 + 21) as u8, 0x7a]);
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

/// Ein Bild, dessen Bildpunkte sich **nicht** dekodieren lassen (`JPXDecode`).
fn unlesbares_bild(alt: &str) -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 24_i64,
            "Height" => 24_i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Filter" => "JPXDecode",
            "Alt" => Object::string_literal(alt),
        },
        b"das ist kein JPEG-2000-Strom".to_vec(),
    )
    .with_compression(false)
}

/// Eine Seite mit den übergebenen Ressourcen und dem übergebenen Strom.
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

/// Eine Seite, ein Bild, ein Spiegel darüber. `ctm` ist der Text der
/// `cm`-Operation, damit Drehung und Streckung im Test hinschreibbar sind.
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

/// Eine Seite, **zwei** Platzierungen desselben Objekts unter zwei Namen, je
/// mit eigenem Spiegel.
fn seite_mit_zwei_namen(
    bild: Stream,
    ctm_a: &str,
    ctm_b: &str,
) -> (Document, ObjectId, ObjectId, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id, "ImZ" => bild_id },
    });
    let content = format!(
        "/Figure <</Alt ({SPIEGEL_A})>> BDC\nq {ctm_a} cm /Im0 Do Q\nEMC\n\
         /Figure <</Alt ({SPIEGEL_B})>> BDC\nq {ctm_b} cm /ImZ Do Q\nEMC\n"
    );
    let (doc, page_id) = eine_seite(doc, resources_id, content.into_bytes());
    (doc, page_id, bild_id, resources_id)
}

/// Zwei Seiten: die erste zeichnet das Objekt unter **zwei** Namen, die zweite
/// unter einem. Damit hängt das Bild an mehreren Seiten — der Fall, in dem
/// `crate::image` kopiert und `repoint_page` einen Namen umbiegt.
fn zwei_seiten_zwei_namen(bild: Stream) -> (Document, ObjectId, ObjectId, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild));
    let res1 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id, "ImZ" => bild_id },
    });
    let res2 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    let inhalt1 = format!(
        "/Figure <</Alt ({SPIEGEL_A})>> BDC\nq 120 0 0 120 100 500 cm /Im0 Do Q\nEMC\n\
         /Figure <</Alt ({SPIEGEL_B})>> BDC\nq 120 0 0 120 100 200 cm /ImZ Do Q\nEMC\n"
    );
    let inhalt2 =
        format!("/Figure <</Alt ({SPIEGEL_C})>> BDC\nq 120 0 0 120 100 500 cm /Im0 Do Q\nEMC\n");
    let c1 = doc.add_object(Stream::new(dictionary! {}, inhalt1.into_bytes()));
    let c2 = doc.add_object(Stream::new(dictionary! {}, inhalt2.into_bytes()));
    let pages_id = doc.new_object_id();
    let seite1 = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => c1,
        "Resources" => res1,
        "MediaBox" => vec![0.into(), 0.into(), 842.into(), 842.into()],
    });
    let seite2 = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => c2,
        "Resources" => res2,
        "MediaBox" => vec![0.into(), 0.into(), 842.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(seite1), Object::Reference(seite2)],
            "Count" => 2_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, seite1, bild_id, res1)
}

fn schwaerzung(seite: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            seite,
            rect,
            None,
            Source::Manual {
                reason: "Gegenprüfung D, Runde 9".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Ohne `padding`: das Rechteck ist genau das, was der Test sagt.
fn schwaerze(
    doc: &mut Document,
    rects: &[(usize, Rect)],
    zugestaendnis: bool,
) -> (RedactionReport, Vec<u8>) {
    let list: Vec<Redaction> = rects
        .iter()
        .map(|(seite, rect)| schwaerzung(*seite, *rect))
        .collect();
    let report = PdfRedactor::with_padding(0.0)
        .allowing_undecodable_images(zugestaendnis)
        .apply_with_report(doc, &list)
        .expect("Schwärzung läuft");
    let bytes = save_to_bytes(doc).expect("Speichern");
    (report, bytes)
}

fn pixel_bytes(doc: &Document, id: ObjectId) -> Vec<u8> {
    match doc.get_object(id).expect("Bild") {
        Object::Stream(stream) => stream.content.clone(),
        other => panic!("kein Strom: {other:?}"),
    }
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

/// Der OCR-Hinweis steht in jedem Lauf mit einem Bild und gehört nicht zur
/// hier geprüften Entscheidung.
fn nur_ocr_hinweis(report: &RedactionReport) -> bool {
    report
        .warnings
        .iter()
        .all(|w| w.contains("enthalten Rasterbilder"))
}

/// Die vier Ecken des Einheitsquadrats durch `[a b c d e f]` — das
/// **Viereck** der Platzierung.
fn viereck(m: [f64; 6]) -> [Point; 4] {
    let [a, b, c, d, e, f] = m;
    let p = |u: f64, v: f64| Point::new(a * u + c * v + e, b * u + d * v + f);
    [p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)]
}

/// Die **Hülle** desselben Vierecks.
fn huelle(m: [f64; 6]) -> Rect {
    let q = viereck(m);
    let mut r = Rect::from_corners(q[0], q[0]);
    for p in &q[1..] {
        r = r.union(&Rect::from_corners(*p, *p));
    }
    r
}

/// Liegt `rect` ganz **außerhalb** des Vierecks? Über die trennenden Achsen,
/// hier von Hand — der Test darf die Antwort des Prüflings nicht abschreiben.
fn ausserhalb(q: &[Point; 4], rect: &Rect) -> bool {
    let ecken = [
        rect.ll,
        Point::new(rect.ur.x, rect.ll.y),
        rect.ur,
        Point::new(rect.ll.x, rect.ur.y),
    ];
    let span = |punkte: &[Point; 4], achse: Point| {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for p in punkte {
            let d = p.x * achse.x + p.y * achse.y;
            lo = lo.min(d);
            hi = hi.max(d);
        }
        (lo, hi)
    };
    let mut achsen = vec![Point::new(1.0, 0.0), Point::new(0.0, 1.0)];
    for i in 0..4 {
        let (von, zu) = (q[i], q[(i + 1) % 4]);
        achsen.push(Point::new(von.y - zu.y, zu.x - von.x));
    }
    achsen.into_iter().any(|achse| {
        let (qlo, qhi) = span(q, achse);
        let (rlo, rhi) = span(&ecken, achse);
        qhi < rlo || rhi < qlo
    })
}

// ---------------------------------------------------------------------------
// 1 — Die leere Hüllenecke (Satz: „in ihren Ecken liegt gar kein Bildpunkt")
// ---------------------------------------------------------------------------

/// **Der Satz hält.** CTM `[60 80 -80 60 200 300]` ist eine Drehung (3-4-5,
/// alle Zahlen in f64 exakt) mit Kantenlänge 100: das Viereck hat die Ecken
/// (200,300) (260,380) (180,440) (120,360), seine Hülle ist
/// (120,300)-(260,440). Das Rechteck (122,302)-(140,320) liegt **in der
/// Hülle** und **außerhalb** des Vierecks — dort gibt es keinen Bildpunkt.
///
/// Gemessen wird an den Bytes des Bildobjekts (unverändert) und am Orakel
/// (Spiegel und Ersatztext stehen noch in der Ausgabe).
#[test]
fn huellenecke_ohne_bildpunkt() {
    const M: [f64; 6] = [60.0, 80.0, -80.0, 60.0, 200.0, 300.0];
    let rect = Rect::new(122.0, 302.0, 140.0, 320.0);
    let q = viereck(M);
    let h = huelle(M);
    eprintln!("Viereck: {q:?}");
    eprintln!("Hülle:   {h:?}");
    assert!(
        h.contains(rect.ll) && h.contains(rect.ur),
        "die Schwärzung liegt ganz in der Hülle"
    );
    assert!(
        ausserhalb(&q, &rect),
        "und außerhalb des Vierecks — sonst prüft dieser Test etwas anderes"
    );

    let (mut doc, _, bild_id) =
        seite_mit_bild(bild(16, 16, BILD_ALT), "60 80 -80 60 200 300", SPIEGEL_A);
    let vorher = pixel_bytes(&doc, bild_id);
    let (report, out) = schwaerze(&mut doc, &[(0, rect)], false);

    assert_eq!(
        pixel_bytes(&doc, bild_id),
        vorher,
        "kein Byte des Bildobjekts hat sich geändert"
    );
    assert!(
        !leaks(&out, SPIEGEL_A).is_empty(),
        "der Spiegel steht noch in der Ausgabe — das unversehrte Bild behält ihn"
    );
    assert!(
        !leaks(&out, BILD_ALT).is_empty(),
        "und der Ersatztext am Bilddictionary ebenso"
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(alt_am_objekt(&aus, bild_id).as_deref(), Some(BILD_ALT));
    assert_eq!(report.redacted_images, 0, "{:?}", report.warnings);
    assert_eq!(report.image_alt_texts_cleared, 0);
    assert!(nur_ocr_hinweis(&report), "{:?}", report.warnings);
}

// ---------------------------------------------------------------------------
// 2 — Das Rechteck zwischen den Gitterlinien
// ---------------------------------------------------------------------------

/// **Der neue Zustand hält, und die alte Frage ist im Test nachgerechnet.**
///
/// Ein 2 × 2-Bild auf 120 × 120 Punkte: die Zellen sind 60 Punkte groß, die
/// Gitterlinien liegen bei x = 100/160/220 und y = 500/560/620. Das Rechteck
/// (110,510)-(150,550) liegt ganz **in** der Zelle links unten und berührt
/// keine Gitterlinie — keine Zellecke liegt darin (im Test nachgerechnet).
/// Die alte Prüfung („liegt eine der vier Ecken im Rechteck?") hätte hier
/// nichts gefüllt: die Bildpunkte wären in der Datei geblieben, ohne Warnung.
///
/// Heute fällt die Zelle: die Bytes des Bildobjekts sind andere, Spiegel und
/// Ersatztext sind fort.
#[test]
fn rechteck_zwischen_den_gitterlinien_trifft_jetzt() {
    const M: [f64; 6] = [120.0, 0.0, 0.0, 120.0, 100.0, 500.0];
    let rect = Rect::new(110.0, 510.0, 150.0, 550.0);

    // Die alte Frage, hier von Hand gestellt: keine der vier Ecken der vier
    // Pixelzellen liegt im Rechteck.
    let mut ecken_treffer = 0usize;
    for zy in 0..2u32 {
        for zx in 0..2u32 {
            let u0 = f64::from(zx) / 2.0;
            let u1 = f64::from(zx + 1) / 2.0;
            let v0 = 1.0 - f64::from(zy + 1) / 2.0;
            let v1 = 1.0 - f64::from(zy) / 2.0;
            for (u, v) in [(u0, v0), (u1, v0), (u1, v1), (u0, v1)] {
                let x = M[0] * u + M[2] * v + M[4];
                let y = M[1] * u + M[3] * v + M[5];
                if x >= rect.ll.x && x <= rect.ur.x && y >= rect.ll.y && y <= rect.ur.y {
                    ecken_treffer += 1;
                }
            }
        }
    }
    eprintln!("Zellecken im Rechteck (alte Prüfung): {ecken_treffer}");
    assert_eq!(
        ecken_treffer, 0,
        "Vorbedingung: die alte Vier-Ecken-Prüfung hätte hier nichts getroffen"
    );

    let (mut doc, _, bild_id) =
        seite_mit_bild(bild(2, 2, BILD_ALT), "120 0 0 120 100 500", SPIEGEL_A);
    let vorher = pixel_bytes(&doc, bild_id);
    let (report, out) = schwaerze(&mut doc, &[(0, rect)], false);
    // Steht vor der Behauptung, damit der **Mutationslauf** (alte
    // Vier-Ecken-Frage in `Work::covers`) zeigt, dass der alte Zustand
    // schwieg: geschwärzte Bilder 0, nur der OCR-Hinweis, Bildpunkte in der
    // Datei.
    eprintln!(
        "geschwärzte Bilder: {}; Warnungen: {:?}",
        report.redacted_images, report.warnings
    );

    assert_ne!(
        pixel_bytes(&doc, bild_id),
        vorher,
        "die Bildpunkte im Objekt sind andere als vorher"
    );
    assert!(
        leaks(&out, SPIEGEL_A).is_empty(),
        "der Spiegel über gefallenen Bildpunkten fällt: {:?}",
        leaks(&out, SPIEGEL_A)
    );
    assert!(
        leaks(&out, BILD_ALT).is_empty(),
        "und der Ersatztext am Bilddictionary mit: {:?}",
        leaks(&out, BILD_ALT)
    );
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    assert!(nur_ocr_hinweis(&report), "{:?}", report.warnings);
}

// ---------------------------------------------------------------------------
// 3 — Berührung zählt als Treffer
// ---------------------------------------------------------------------------

/// **Berührung zählt als Treffer — und jetzt überall, nicht nur hinter dem
/// groben Vorfilter.**
///
/// Dieser Test hat den Widerspruch gehalten, und er hält jetzt seine Auflösung.
/// Dasselbe Bild, ein Rechteck, das die linke Kante der Platzierung **genau**
/// berührt: (40,510)-(100,550) gegen ein Viereck, das bei x = 100 beginnt.
///
/// Vor der Korrektur entschied fremder Inhalt der Seite über diese Geometrie:
///
/// * allein auf der Seite fiel **kein** Bildpunkt — `redact_images` verwarf die
///   Seite vorher mit `ctm_bounds(ctm).intersects(zone)`, wo Berührung nicht
///   zählt;
/// * kam eine **zweite** Schwärzung über eine **andere** Platzierung hinzu,
///   wurde die Seite gefüllt, und dasselbe berührende Rechteck traf plötzlich
///   seine Pixelzelle.
///
/// Dieselbe Geometrie, zwei Ergebnisse. Jetzt fragen Vorfilter,
/// `Collector::touches_a_zone` und `fill_page` dasselbe Viereck mit
/// `cell_meets_rect`: beide Fälle gehen gleich aus, und was auf der Seite sonst
/// noch steht, ändert daran nichts.
///
/// Mutation, die ihn rot macht: den Seitenvorfilter in `redact_images` wieder
/// auf `ctm_bounds(&p.ctm).intersects(&z.rect)` stellen — dann fällt der erste
/// Fall auseinander.
#[test]
fn beruehrung_am_rand_faellt_unabhaengig_vom_rest_der_seite() {
    let beruehrt = Rect::new(40.0, 510.0, 100.0, 550.0);

    // 1 — allein auf der Seite.
    let (mut doc, _, bild_id, _) = seite_mit_zwei_namen(
        bild(2, 2, BILD_ALT),
        "120 0 0 120 100 500",
        "120 0 0 120 100 200",
    );
    let vorher = pixel_bytes(&doc, bild_id);
    let (report_allein, out) = schwaerze(&mut doc, &[(0, beruehrt)], false);
    let allein = pixel_bytes(&doc, bild_id);
    assert_ne!(
        allein, vorher,
        "die Berührung trifft, auch wenn sonst nichts auf der Seite steht —          {:?}",
        report_allein.warnings
    );
    assert_eq!(
        report_allein.redacted_images, 1,
        "{:?}",
        report_allein.warnings
    );
    assert!(
        leaks(&out, SPIEGEL_A).is_empty(),
        "und der Spiegel über der getroffenen Platzierung fällt mit: {:?}",
        leaks(&out, SPIEGEL_A)
    );

    // 2 — dasselbe berührende Rechteck, dazu eine zweite Schwärzung über der
    //     zweiten Platzierung. Am ersten Bildpunkt ändert das nichts.
    let (mut doc, _, bild_id, _) = seite_mit_zwei_namen(
        bild(2, 2, BILD_ALT),
        "120 0 0 120 100 500",
        "120 0 0 120 100 200",
    );
    let (report_zwei, _out) = schwaerze(
        &mut doc,
        &[(0, beruehrt), (0, Rect::new(110.0, 210.0, 150.0, 250.0))],
        false,
    );
    assert_ne!(
        pixel_bytes(&doc, bild_id),
        vorher,
        "und mit der zweiten Schwärzung erst recht — {:?}",
        report_zwei.warnings
    );
}

// ---------------------------------------------------------------------------
// 4 — Der Kandidatenfilter: Viereck, nicht Hülle
// ---------------------------------------------------------------------------

/// **Der Satz hält, und zwar am Verhalten ohne Zugeständnis.** Ein
/// **unlesbares** Bild, gedreht wie in Test 1, die Schwärzung in der leeren
/// Hüllenecke. Entschied der Kandidatenfilter an der Hülle, wäre diese
/// Platzierung getroffen — und ein unlesbares getroffenes Bild bricht den
/// Lauf ohne `--allow-undecodable-images` ab. Der Lauf geht durch, ohne
/// Warnung, und Spiegel wie Ersatztext bleiben.
#[test]
fn viereck_statt_huelle_beim_unlesbaren_bild() {
    let rect = Rect::new(122.0, 302.0, 140.0, 320.0);
    let (mut doc, _, bild_id) =
        seite_mit_bild(unlesbares_bild(BILD_ALT), "60 80 -80 60 200 300", SPIEGEL_A);
    let vorher = pixel_bytes(&doc, bild_id);
    let liste = vec![schwaerzung(0, rect)];
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &liste)
        .expect(
            "ohne Zugeständnis: der Lauf darf NICHT abbrechen — sonst fragt der Filter die Hülle",
        );
    let out = save_to_bytes(&doc).expect("Speichern");

    assert_eq!(
        pixel_bytes(&doc, bild_id),
        vorher,
        "das Objekt ist unberührt"
    );
    assert!(
        nur_ocr_hinweis(&report),
        "keine Warnung: {:?}",
        report.warnings
    );
    assert!(!leaks(&out, SPIEGEL_A).is_empty(), "der Spiegel bleibt");
    assert!(!leaks(&out, BILD_ALT).is_empty(), "der Ersatztext bleibt");
    assert_eq!(report.image_alt_texts_cleared, 0);
}

// ---------------------------------------------------------------------------
// 5 — `crate::image` berichtet je Platzierung
// ---------------------------------------------------------------------------

/// **Der Satz hält.** [`redact_images`] gibt die Platzierungen heraus, unter
/// denen Bildpunkte fielen — je Operationsindex im Seitenstrom, nicht je
/// Bildobjekt.
///
/// Zwei Platzierungen desselben Objekts auf **einer** Seite (also
/// überschreiben, nicht kopieren): die Schwärzung liegt auf der ersten, und
/// **beide** Platzierungen zeigen danach die geschwärzten Bildpunkte — beide
/// stehen im Bericht. Das ist die Auskunft, aus der der Aufrufer nicht mehr
/// schätzen muss.
#[test]
fn der_bildlauf_berichtet_je_platzierung() {
    let (mut doc, seite, _bild_id, _) = seite_mit_zwei_namen(
        bild(2, 2, BILD_ALT),
        "120 0 0 120 100 500",
        "120 0 0 120 100 200",
    );
    let liste = vec![schwaerzung(0, Rect::new(110.0, 510.0, 150.0, 550.0))];
    let outcome = redact_images(&mut doc, &liste, 0.0, &ImageOptions::default()).expect("Bildlauf");
    eprintln!("page_image_hits: {:?}", outcome.page_image_hits);
    let treffer = outcome
        .page_image_hits
        .get(&seite)
        .expect("die Seite steht im Bericht");
    assert_eq!(
        treffer.len(),
        2,
        "überschrieben heißt: beide Platzierungen zeigen die geschwärzten \
         Bildpunkte — {treffer:?}"
    );
    assert!(outcome.undecodable_images.is_empty());
    assert_eq!(outcome.redacted_images, 1, "ein Objekt, einmal geschrieben");

    // Die Gegenrichtung: liegt die Schwärzung auf keiner Platzierung, steht
    // auch keine im Bericht.
    let (mut doc, seite, _bild_id, _) = seite_mit_zwei_namen(
        bild(2, 2, BILD_ALT),
        "120 0 0 120 100 500",
        "120 0 0 120 100 200",
    );
    let liste = vec![schwaerzung(0, Rect::new(400.0, 700.0, 420.0, 720.0))];
    let outcome = redact_images(&mut doc, &liste, 0.0, &ImageOptions::default()).expect("Bildlauf");
    assert!(
        !outcome.page_image_hits.contains_key(&seite),
        "nichts getroffen, nichts berichtet: {:?}",
        outcome.page_image_hits
    );
}

// ---------------------------------------------------------------------------
// 6 — Zwei Namen, ein Objekt, eine Kopie
// ---------------------------------------------------------------------------

/// **Der Satz hält: `repoint_page` setzt genau einen Namen — und der Vermerk
/// hält sich daran.**
///
/// Das Bild hängt an zwei Seiten (also wird kopiert) und wird auf der ersten
/// Seite unter zwei Namen gezeichnet. Die Schwärzung trifft nur `/Im0`.
/// Danach gilt:
///
/// * `/Im0` zeigt auf ein **neues** Objekt mit geschwärzten Bildpunkten,
/// * `/ImZ` zeigt weiter auf das **unveränderte** Original (Bytes gleich),
/// * der Spiegel über `/Im0` ist fort, der über `/ImZ` steht noch da, und der
///   auf Seite 2 ebenso.
///
/// Fiele der Spiegel über `/ImZ` mit, stünden Fehlalarm und Leck in einer
/// Datei: die Beschreibung weg, die Bildpunkte sichtbar.
#[test]
fn zweiter_name_behaelt_bildpunkte_und_spiegel() {
    let (mut doc, seite1, bild_id, _res1) = zwei_seiten_zwei_namen(bild(2, 2, BILD_ALT));
    let vorher = pixel_bytes(&doc, bild_id);
    let (report, out) = schwaerze(
        &mut doc,
        &[(0, Rect::new(110.0, 510.0, 150.0, 550.0))],
        false,
    );
    eprintln!("Warnungen: {:?}", report.warnings);

    assert_eq!(
        pixel_bytes(&doc, bild_id),
        vorher,
        "das Original bleibt unberührt — es wird kopiert, nicht überschrieben"
    );
    assert_eq!(report.copied_images, 1, "genau eine Kopie");

    // Welche Namen zeigen jetzt wohin? Gefragt sind die Ressourcen **der
    // Seite**: `repoint_page` hängt sie als eigenes Verzeichnis an die Seite,
    // damit die zweite Seite ihr geteiltes Original behält.
    let seiten_res = doc
        .get_dictionary(seite1)
        .expect("Seite")
        .get(b"Resources")
        .and_then(|o| doc.dereference(o).map(|(_, v)| v.clone()))
        .and_then(|o| o.as_dict().cloned())
        .expect("Ressourcen der Seite");
    let xobjects = seiten_res
        .get(b"XObject")
        .and_then(|o| doc.dereference(o).map(|(_, v)| v.clone()))
        .and_then(|o| o.as_dict().cloned())
        .expect("XObject-Verzeichnis");
    let ziel = |name: &[u8]| match xobjects.get(name) {
        Ok(Object::Reference(id)) => *id,
        other => panic!("{name:?} zeigt nicht auf ein Objekt: {other:?}"),
    };
    let im0 = ziel(b"Im0");
    let imz = ziel(b"ImZ");
    eprintln!("Im0 -> {im0:?}, ImZ -> {imz:?}, Original {bild_id:?}");
    assert_eq!(
        imz, bild_id,
        "der zweite Name zeigt weiter auf das Original"
    );
    assert_ne!(im0, bild_id, "der getroffene Name ist umgebogen");
    assert_ne!(
        pixel_bytes(&doc, im0),
        vorher,
        "die Kopie trägt die geschwärzten Bildpunkte"
    );
    assert_eq!(
        pixel_bytes(&doc, imz),
        vorher,
        "der zweite Name zeigt buchstäblich dieselben Bildpunkte wie vorher"
    );
    let _ = seite1;

    assert!(
        leaks(&out, SPIEGEL_A).is_empty(),
        "der Spiegel über den gefallenen Bildpunkten fällt: {:?}",
        leaks(&out, SPIEGEL_A)
    );
    assert!(
        !leaks(&out, SPIEGEL_B).is_empty(),
        "der Spiegel über den sichtbar gebliebenen Bildpunkten bleibt stehen"
    );
    assert!(
        !leaks(&out, SPIEGEL_C).is_empty(),
        "und der auf der zweiten Seite ebenso"
    );
    assert!(
        !leaks(&out, BILD_ALT).is_empty(),
        "das Original behält sein /Alt — es beschreibt sichtbare Bildpunkte"
    );
}

// ---------------------------------------------------------------------------
// 7 — Das `/Alt` am Bilddictionary beim unlesbaren Bild
// ---------------------------------------------------------------------------

/// **Der Satz hält, samt seiner Ehrlichkeit über den Preis.** Ein unlesbares
/// Bild, zweimal auf **einer** Seite, die Schwärzung auf der ersten
/// Platzierung, mit `--allow-undecodable-images`:
///
/// * die Warnung steht da,
/// * der Spiegel über der getroffenen Platzierung fällt (grob entschieden,
///   aber gesagt),
/// * der Spiegel über der **unberührten** Platzierung bleibt,
/// * das `/Alt` am **Bilddictionary** fällt — und damit für **beide**
///   Platzierungen. Das ist die Über-Schwärzung, die der Abschnitt nennt.
#[test]
fn alt_am_bilddictionary_faellt_fuer_alle_platzierungen() {
    let (mut doc, _, bild_id, _) = seite_mit_zwei_namen(
        unlesbares_bild(BILD_ALT),
        "120 0 0 120 100 500",
        "120 0 0 120 100 200",
    );
    let vorher = pixel_bytes(&doc, bild_id);
    let (report, out) = schwaerze(
        &mut doc,
        &[(0, Rect::new(110.0, 510.0, 150.0, 550.0))],
        true,
    );
    eprintln!("Warnungen: {:?}", report.warnings);

    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("nicht dekodieren")),
        "die Über-Schwärzung steht neben einer Warnung: {:?}",
        report.warnings
    );
    assert_eq!(
        pixel_bytes(&doc, bild_id),
        vorher,
        "die Bildpunkte bleiben in der Datei — das ist der Preis des Schalters"
    );
    assert!(
        leaks(&out, SPIEGEL_A).is_empty(),
        "der Spiegel der getroffenen Platzierung fällt: {:?}",
        leaks(&out, SPIEGEL_A)
    );
    assert!(
        !leaks(&out, SPIEGEL_B).is_empty(),
        "der Spiegel der unberührten Platzierung bleibt — ihr Bild zeigt \
         buchstäblich dasselbe wie vorher"
    );
    assert!(
        leaks(&out, BILD_ALT).is_empty(),
        "das /Alt am Bilddictionary fällt, und zwar für beide Platzierungen: {:?}",
        leaks(&out, BILD_ALT)
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(
        alt_am_objekt(&aus, bild_id),
        None,
        "am Objekt selbst steht kein Ersatztext mehr"
    );
}

// ---------------------------------------------------------------------------
// 8 — Was die zwei Flächenfragen kosten: ein „kann nicht vorkommen"
// ---------------------------------------------------------------------------

/// **Eine Flächenfrage, und deshalb entsteht die Ausgabedatei.**
///
/// Dieser Test hat einen Lauf gehalten, der mit einem Fehler und **ohne
/// Ausgabedatei** endete, und er hält jetzt seine Auflösung.
///
/// `Collector::touches_a_zone` behielt die Rohdaten eines Inline-Bildes genau
/// dann, wenn eine Zone seine **Hülle** schneidet — mit `Rect::intersects`, wo
/// Berührung **nicht** zählt. `fill_page` nahm die Platzierung dagegen über
/// `cell_meets_rect(quad, zone)` auf, wo Berührung **zählt**. Für ein
/// Inline-Bild, dessen Fläche eine Zone nur berührt, gingen die beiden
/// Antworten auseinander:
///
/// * Rohdaten nicht mitgeführt (`data == None`),
/// * Platzierung trotzdem als getroffen aufgenommen.
///
/// `decode_placement` nennt diesen Fall „Kann nicht vorkommen" und liefert
/// einen Platzhalter; ohne `--allow-undecodable-images` brach der Lauf dann ab.
/// Erreichbar war er, sobald auf derselben Seite **irgendeine** Schwärzung eine
/// **andere** Platzierung wirklich überlappte — denn nur dann kam die Seite
/// überhaupt bis `fill_page`.
///
/// Eine Grenze, die eine gewöhnliche Datei ablehnt, ist in diesem Projekt
/// genauso ein Fehler wie eine Lücke. Jetzt stellt `touches_a_zone` dieselbe
/// Frage wie `fill_page`, „Kann nicht vorkommen" ist wieder wahr, und der Lauf
/// schreibt seine Ausgabe.
///
/// Mutation, die ihn rot macht: `touches_a_zone` wieder auf
/// `ctm_bounds(ctm)` und `Rect::intersects` stellen.
#[test]
fn eine_flaechenfrage_laesst_den_lauf_durch() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild(2, 2, BILD_ALT)));
    let res = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    // 2 x 2 RGB unkomprimiert: 12 Byte Nutzdaten.
    let pixel: Vec<u8> = vec![
        0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xA0, 0xB0, 0xC0,
    ];
    let mut inhalt =
        format!("/Span <</ActualText ({SPIEGEL_A})>> BDC\nq 120 0 0 120 100 500 cm\nBI /W 2 /H 2 /CS /RGB /BPC 8 ID ")
            .into_bytes();
    inhalt.extend_from_slice(&pixel);
    inhalt.extend_from_slice(b"\nEI\nQ\nEMC\n");
    inhalt.extend_from_slice(
        format!("/Figure <</Alt ({SPIEGEL_B})>> BDC\nq 120 0 0 120 100 200 cm /Im0 Do Q\nEMC\n")
            .as_bytes(),
    );
    let (mut doc, _) = eine_seite(doc, res, inhalt);

    // Zone 1 berührt die Fläche des Inline-Bildes bei x = 100; Zone 2 überlappt
    // die Platzierung des XObjects wirklich.
    let liste = vec![
        schwaerzung(0, Rect::new(40.0, 510.0, 100.0, 550.0)),
        schwaerzung(0, Rect::new(110.0, 210.0, 150.0, 250.0)),
    ];
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &liste)
        .expect("der Lauf geht durch und schreibt seine Ausgabe");
    let out = save_to_bytes(&doc).expect("Ausgabe speicherbar");
    assert!(
        !out.is_empty(),
        "und es gibt Bytes — der Fehler kostete genau die"
    );
    // Die Rohdaten des berührten Inline-Bildes waren da: es wurde neu kodiert.
    assert!(
        !out.windows(pixel.len()).any(|f| f == pixel.as_slice()),
        "die unveränderten Bildpunkte des Inline-Bildes stehen noch in der \
         Ausgabe — dann ist es gar nicht angefasst worden. Warnungen: {:?}",
        report.warnings
    );
}
