//! Gegenprüfung J-B, Linse **WIDERLEGEN**: schließt die Nachbesserung den
//! Einwand, oder verschiebt sie ihn?
//!
//! Geprüft wird die Antwort auf den FEHLALARM-Einwand: die Entscheidung
//! „trifft die Schwärzung das Bild?“ läuft jetzt über
//! `content::ImagePlacement::covers` (trennende Achsen gegen das **Viereck**)
//! statt über `placement.bounds.intersects` (die Hülle). Die Dokumentation an
//! `covers` sagt dazu zu:
//!
//! > Wo `filled > 0` gilt, liegt eine Zellecke im Rechteck und damit auch das
//! > Viereck darin — ein Bild, das Bildpunkte verliert, wird also **nie**
//! > übersehen.
//!
//! Das ist die Zusicherung, auf der LECK 1 („Spiegel über geschwärztem Bild
//! fällt“) ruht. Sie hält nicht: `crate::image` entscheidet je Pixelzelle mit
//! `Rect::contains` — **Rand eingeschlossen** —, `covers` entscheidet mit den
//! strengen Vergleichen aus `Rect::intersects` — **Berührung zählt nicht**.
//! Wo eine Zellecke genau auf dem Rand des Schwärzungsrechtecks liegt, fällt
//! der Bildpunkt und der Spiegel darüber bleibt stehen.
//!
//! Alle Zahlen hier sind in f64 **exakt** (Zweierpotenzen und ganze Zahlen,
//! keine Drehung um 45°): der Befund ist keine Rundungslotterie.
//!
//! Gemessen wird am Leck-Orakel (`redact_pdf::leaks`) und an den beiden
//! Entscheidern selbst, nicht am Bericht.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfRedactor, RedactionReport};

/// Der Spiegel am `/Figure`-Abschnitt — das, was nach Register #20 fallen muss,
/// sobald die Bildpunkte darunter fallen.
const SPIEGEL: &str = "Kontoauszug, IBAN DE89 3704 0044 0532 0130 00";
/// Der Ersatztext am Bilddictionary selbst.
const BILD_ALT: &str = "Scan des Kontoauszugs";

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

/// Ein 2 x 2 RGB-Bild hinter `/FlateDecode`, mit `/Alt` am Dictionary.
fn bild_2x2() -> Stream {
    let data: Vec<u8> = vec![
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, // obere Zeile
        0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, // untere Zeile
    ];
    let mut stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 2_i64,
            "Height" => 2_i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Alt" => Object::string_literal(BILD_ALT),
        },
        data,
    );
    let _ = stream.compress();
    stream
}

/// Eine Seite: `/Figure <</Alt …>> BDC q <CTM> cm /Im0 Do Q EMC`.
///
/// Die CTM ist `[200 100 -100 200 300 400]` — eine Drehung mit Streckung, wie
/// sie ein entzerrter Scan mitbringt. Alle Einträge sind ganze Zahlen, das
/// Viereck im User-Space also **exakt**:
///
/// ```text
/// P0 (300,400)   P1 (500,500)   P2 (400,700)   P3 (200,600)
/// ```
///
/// Die Hülle ist `(200,400)-(500,700)`, deutlich größer als die Fläche.
fn seite_mit_gedrehtem_bild() -> (Document, ObjectId, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_2x2()));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    let content =
        format!("/Figure <</Alt ({SPIEGEL})>> BDC\nq 200 100 -100 200 300 400 cm /Im0 Do Q\nEMC\n");
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
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
    (doc, page_id, bild_id)
}

fn schwaerzung(page: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "Gegenprüfung J-B".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Ohne `padding`: das Rechteck ist genau das, was der Test sagt.
fn schwaerze(doc: &mut Document, list: &[Redaction]) -> (RedactionReport, Vec<u8>) {
    let report = PdfRedactor::with_padding(0.0)
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

// ---------------------------------------------------------------------------
// Vorbedingung: die beiden Entscheider widersprechen sich
// ---------------------------------------------------------------------------

/// Das Rechteck `(400,400)-(480,450)`.
///
/// Seine obere **linke** Ecke `(400,450)` liegt genau auf der Kante P0→P1 des
/// Vierecks (Gerade `x - 2y + 500 = 0`), alle anderen drei Ecken liegen
/// außerhalb. Fläche und Rechteck haben also genau **einen** Punkt gemeinsam.
///
/// Derselbe Punkt ist bei einem 2 x 2 Bild die Ecke `(u,v) = (0.5, 0)` der
/// rechten unteren Pixelzelle: `ctm.apply(0.5, 0.0) = (400, 450)`, exakt.
const BERUEHRT: Rect = Rect {
    ll: redact_core::Point { x: 400.0, y: 400.0 },
    ur: redact_core::Point { x: 480.0, y: 450.0 },
};

/// Dasselbe Rechteck, einen Punkt weiter nach links: `(399,450)` liegt **im**
/// Viereck. Die Kontrolle zum Befund.
const UEBERLAPPT: Rect = Rect {
    ll: redact_core::Point { x: 399.0, y: 400.0 },
    ur: redact_core::Point { x: 480.0, y: 450.0 },
};

/// Die Vorbedingungen des Befunds — beide gelten **auch nach** seiner Behebung,
/// dieser Test ist also kein Wächter über den Fehler, sondern über das Material:
/// der Vorfilter von `crate::image` (`ctm_bounds().intersects()`) lässt das
/// Rechteck durch, und einen Punkt weiter links gilt dieselbe Fläche als
/// getroffen.
#[test]
fn die_huelle_wird_getroffen_und_die_kontrolle_greift() {
    let (doc, page_id, _) = seite_mit_gedrehtem_bild();
    let scan = redact_pdf::scan_page(&doc, page_id).expect("Scan läuft");
    assert_eq!(
        scan.images.len(),
        1,
        "Vorbedingung: genau eine Bildplatzierung"
    );
    let placement = &scan.images[0];
    assert_eq!(
        placement.quad[0],
        redact_core::Point::new(300.0, 400.0),
        "Vorbedingung: das Viereck ist exakt"
    );
    assert_eq!(placement.quad[1], redact_core::Point::new(500.0, 500.0));
    assert!(
        placement.bounds.intersects(&BERUEHRT),
        "Vorbedingung: die Hülle wird getroffen — der Vorfilter von crate::image \
         (ctm_bounds().intersects()) lässt die Platzierung durch"
    );
    assert!(
        placement.covers(&UEBERLAPPT),
        "Kontrolle: einen Punkt weiter links gilt dieselbe Fläche als getroffen"
    );
}

// ---------------------------------------------------------------------------
// Der Befund
// ---------------------------------------------------------------------------

/// **Der Befund.** Der Bildpunkt fällt (`Überschriebene Bilder: 1`), der
/// Spiegel darüber bleibt stehen — genau das Leck aus Register #20, das diese
/// Runde geschlossen haben wollte.
///
/// `crate::image::Work::covers` prüfte die vier **Ecken** jeder Pixelzelle mit
/// `Rect::contains` (Rand eingeschlossen) und füllte; `ImagePlacement::covers`
/// prüft mit strengen Vergleichen (Berührung zählt nicht) und verneinte. Damit
/// war die Zusicherung „ein Bild, das Bildpunkte verliert, wird nie übersehen“
/// widerlegt. Seit der Fix-Runde 8 fragt `Work::covers` die **Fläche** der
/// Zelle (`cell_meets_rect`), und dieser Test hält, dass die beiden Antworten
/// zusammenpassen.
#[test]
fn bildpunkt_faellt_und_der_spiegel_darueber_bleibt_stehen() {
    let (mut doc, page_id, bild_id) = seite_mit_gedrehtem_bild();
    // Die Entscheidung des Redaktors, vor dem Lauf abgelesen — sie steht in der
    // Fehlermeldung unten, nicht in einer eigenen Behauptung: dieser Test soll
    // grün werden, wenn der Widerspruch behoben ist, und nicht bloß wandern.
    let entscheidung = redact_pdf::scan_page(&doc, page_id)
        .expect("Scan läuft")
        .images[0]
        .covers(&BERUEHRT);
    let (report, out) = schwaerze(&mut doc, &[schwaerzung(0, BERUEHRT)]);

    assert_eq!(
        report.redacted_images, 1,
        "Vorbedingung: crate::image hat Bildpunkte überschrieben (filled > 0)"
    );

    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(
        alt_am_objekt(&aus, bild_id),
        None,
        "Vorbedingung: das Bilddictionary wurde neu aufgebaut, sein /Alt ist mit \
         den Bildpunkten gefallen"
    );
    assert!(
        leaks(&out, BILD_ALT).is_empty(),
        "Vorbedingung: der Ersatztext am Bild ist aus der Datei verschwunden"
    );

    let fund = leaks(&out, SPIEGEL);
    assert!(
        fund.is_empty(),
        "LECK: die Bildpunkte fielen, der Spiegel darüber steht noch in der Datei. \
         placement.covers(rect) == {entscheidung} (dort liegt die Ursache), \
         image_alt_texts_cleared == {}, redacted_images == {}, Warnungen == {:?}, \
         Fundstellen == {fund:?}",
        report.image_alt_texts_cleared,
        report.redacted_images,
        report.warnings
    );
}

/// Die Kontrolle: dasselbe Bild, dasselbe Rechteck **einen Punkt** weiter
/// links. Jetzt überlappt die Fläche wirklich, und der Spiegel fällt. Ein
/// Punkt Unterschied im Rechteck entscheidet über Klartext in der Ausgabe.
#[test]
fn ein_punkt_weiter_links_faellt_der_spiegel() {
    let (mut doc, _, _) = seite_mit_gedrehtem_bild();
    let (report, out) = schwaerze(&mut doc, &[schwaerzung(0, UEBERLAPPT)]);
    assert_eq!(report.redacted_images, 1);
    assert!(
        leaks(&out, SPIEGEL).is_empty(),
        "Kontrolle: hier räumt die Nachbesserung den Spiegel ab — \
         image_alt_texts_cleared == {}",
        report.image_alt_texts_cleared
    );
    assert_eq!(
        report.image_alt_texts_cleared, 1,
        "Kontrolle: und sagt es im Bericht"
    );
}
