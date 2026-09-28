//! Die **Bildfrage** geht denselben Weg wie die Glyphenfrage — und sie hört an
//! derselben Stelle auf.
//!
//! Drei Gegenprüfer haben die erste Fassung von Register #20 gebrochen. Diese
//! Datei hält fest, was seither gilt:
//!
//! 1. **Der Spiegel eine Ebene über dem Bild.** `/Figure <</Alt (Kontoauszug,
//!    IBAN …)>> BDC /Fm0 Do EMC` im Seitenstrom, `/Im0 Do` in `Fm0`. Gefragt
//!    wurde nur die eigene Spanne des Stroms (`image_hits` kennt allein
//!    Platzierungen desselben Stroms) und danach `form_plans`, also die
//!    **Glyphen**pläne. `Fm0` verliert kein Zeichen, hat keinen Plan — der
//!    Abschnitt galt als unberührt, die Pixel fielen, der Klartext blieb. Jetzt
//!    fragt `touches_form_image` die Formulare im Geltungsbereich, genau wie
//!    `touches_form_plan` es für die Glyphen tut; über Seitengrenzen hinweg
//!    `DeferredMirror::touched_by_image`.
//! 2. **Der Ersatztext am Bilddictionary in einer nicht getaggten Datei.** Die
//!    Liste der Bildplatzierungen wurde nur „in Strömen mit Spiegel“ gefüllt.
//!    Eine Seite ohne `BDC` lieferte nichts, und das `/Alt` eines unlesbaren
//!    Bildes blieb stehen. Der Filter ist weg.
//! 3. **Die Hülle ist nicht die Fläche.** Bei einem um 45° gedrehten Bild ist
//!    die Hülle doppelt so groß wie das Bild; in ihren Ecken steht kein
//!    Bildpunkt, sondern gewöhnlich Text. Eine Schwärzung dort nimmt dem Bild
//!    nichts — `crate::image` fasst es nicht an, und es gibt **keine** Warnung.
//!    Entschieden wird deshalb an der Fläche (`ImagePlacement::covers`).
//!
//! Gemessen wird am Leck-Orakel [`redact_pdf::leaks`] und an der Ausgabedatei,
//! nicht am Bericht des Programms.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfRedactor, RedactionReport};

/// Was auf dem Bild zu lesen war — und in seiner Beschreibung steht.
const GEHEIM: &str = "Kontoauszug, IBAN DE89 3704 0044 0532 0130 00";
/// Eine gewöhnliche Beschreibung, die nichts verrät.
const HARMLOS: &str = "Firmenlogo der Musterbank";

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

fn bild(width: u32, height: u32) -> Stream {
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
        },
        data,
    );
    let _ = stream.compress();
    stream
}

/// Ein Bild, dessen Pixel sich **nicht** dekodieren lassen.
fn unlesbares_bild() -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 20_i64,
            "Height" => 20_i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Filter" => "JPXDecode",
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
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
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

fn zwei_seiten(mut doc: Document, seiten: &[(ObjectId, Vec<u8>)]) -> (Document, Vec<ObjectId>) {
    let pages_id = doc.new_object_id();
    let mut ids = Vec::new();
    for (resources_id, content) in seiten {
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.clone()));
        ids.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => *resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }));
    }
    let kids: Vec<Object> = ids.iter().map(|id| Object::Reference(*id)).collect();
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
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
                reason: "Bildfläche".into(),
            },
        ),
        Action::Blackout,
    )
}

fn schwaerze_mit(
    doc: &mut Document,
    list: &[Redaction],
    allow_undecodable: bool,
) -> (RedactionReport, Vec<u8>) {
    let report = PdfRedactor::with_padding(0.0)
        .allowing_undecodable_images(allow_undecodable)
        .apply_with_report(doc, list)
        .expect("Schwärzung läuft");
    let bytes = save_to_bytes(doc).expect("Speichern");
    (report, bytes)
}

fn schwaerze(doc: &mut Document, list: &[Redaction]) -> (RedactionReport, Vec<u8>) {
    schwaerze_mit(doc, list, false)
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

/// Der OCR-Hinweis steht in jedem Lauf mit einem Bild und hat mit der hier
/// geprüften Frage nichts zu tun. Alles andere wäre eine neue Warnung.
fn nur_ocr_hinweis(report: &RedactionReport) -> bool {
    report
        .warnings
        .iter()
        .all(|w| w.contains("enthalten Rasterbilder"))
}

/// Ein Form-XObject, das genau ein Bild bei (50,600)-(150,700) zeichnet.
fn formular_mit_bild(doc: &mut Document) -> (ObjectId, ObjectId) {
    let im0 = doc.add_object(Object::Stream(bild(20, 20)));
    let form = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => im0 } },
            },
            b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
        )
        .with_compression(false),
    ));
    (form, im0)
}

/// Mitten im Bild bei (50,600)-(150,700).
fn ueber_dem_bild() -> Rect {
    Rect::new(80.0, 640.0, 110.0, 670.0)
}

// ---------------------------------------------------------------------------
// 1. Der Spiegel eine Ebene über dem Bild
// ---------------------------------------------------------------------------

/// **Register #20, Einwand 1.** Der Spiegel steht im Seitenstrom, das Bild eine
/// Ebene tiefer im Formular. Für Glyphen war dieser Weg schon gedeckt
/// (`MarkedTextRecord::forms`, `touches_form_plan`); die Bildfrage ging ihn
/// nicht mit.
///
/// Mutation, die diesen Test rot macht: **beide** neuen Wege weglassen — das
/// `|| touches_form_image(record, form_image_hits)` in `mirrors_to_clear` *und*
/// das `open.touched_by_image(&form_image_hits)` in der Schleife über
/// `deferred`. Einer allein genügt hier nicht, und das ist so gewollt: liegt die
/// Schwärzung auf derselben Seite wie der Spiegel, entscheidet der erste Weg;
/// liegt sie auf einer anderen, der zweite. Wer nur einen prüfen will, nimmt
/// `spiegel_im_aeusseren_formular_ueber_dem_inneren_faellt` (erster Weg) oder
/// `spiegel_auf_seite_eins_faellt_wenn_seite_zwei_das_bild_schwaerzt` (zweiter).
#[test]
fn spiegel_im_seitenstrom_ueber_dem_formular_faellt() {
    let mut doc = Document::with_version("1.5");
    let (form, _) = formular_mit_bild(&mut doc);
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => form },
    });
    let content = format!("/Figure <</Alt ({GEHEIM})>> BDC\nq /Fm0 Do Q\nEMC\n");
    let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
    let (report, out) = schwaerze(&mut doc, &[schwaerzung(0, ueber_dem_bild())]);
    assert_eq!(
        report.redacted_images, 1,
        "Vorbedingung: die Pixel fallen wirklich — {:?}",
        report.warnings
    );
    assert_eq!(
        report.image_alt_texts_cleared, 1,
        "und der Verlust steht im Bericht"
    );
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Spiegel im Seitenstrom über dem Formular: {:?}",
        leaks(&out, GEHEIM)
    );
}

/// Dieselbe Lage zwei Ebenen tief: Spiegel in `Fm0`, Bild in `Fm1`. Trägt
/// `MarkedTextRecord::forms` die transitive Schließung, dann gilt hier
/// dasselbe — und das Formular muss dafür überhaupt neu geschrieben werden.
///
/// Zwei Mutationen machen diesen Test rot, und er braucht beide Stellen: in der
/// Bildung von `form_ids` das `|| touches_form_image(record, &form_image_hits)`
/// weglassen (dann bleibt `Fm0` ungeschrieben), oder dasselbe in
/// `mirrors_to_clear` (dann wird `Fm0` geschrieben, aber mit seinem Spiegel).
/// Die Schleife über `deferred` hilft hier nicht: sie gilt für Spiegel im
/// **Seiten**strom, und dieser steht in einem Formular.
#[test]
fn spiegel_im_aeusseren_formular_ueber_dem_inneren_faellt() {
    let mut doc = Document::with_version("1.5");
    let (innen, _) = formular_mit_bild(&mut doc);
    let aussen = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Fm1" => innen } },
            },
            format!("/Figure <</Alt ({GEHEIM})>> BDC\nq /Fm1 Do Q\nEMC\n").into_bytes(),
        )
        .with_compression(false),
    ));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => aussen },
    });
    let (mut doc, _) = eine_seite(doc, resources_id, b"q /Fm0 Do Q\n".to_vec());
    let (report, out) = schwaerze(&mut doc, &[schwaerzung(0, ueber_dem_bild())]);
    assert_eq!(report.redacted_images, 1, "Vorbedingung: die Pixel fallen");
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Spiegel im äußeren Formular: {:?}",
        leaks(&out, GEHEIM)
    );
}

/// **Über Seitengrenzen.** Zwei Seiten zeichnen dasselbe Formular; der Spiegel
/// steht auf Seite 1, geschwärzt wird auf Seite 2. Für Glyphen ist das Befund
/// G1-A2 und über `DeferredMirror` gedeckt — die Bildfrage geht diesen Weg
/// jetzt mit, **samt der Zahl im Bericht**: der Abschnitt auf Seite 1 fällt,
/// und dass er fällt, ist ein Verlust an Barrierefreiheit.
///
/// Mutation, die diesen Test rot macht: in der Schleife über `deferred` das
/// `open.touched_by_image(&form_image_hits)` weglassen.
#[test]
fn spiegel_auf_seite_eins_faellt_wenn_seite_zwei_das_bild_schwaerzt() {
    let mut doc = Document::with_version("1.5");
    let (form, _) = formular_mit_bild(&mut doc);
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => form },
    });
    let eins = format!("/Figure <</Alt ({GEHEIM})>> BDC\nq /Fm0 Do Q\nEMC\n").into_bytes();
    let zwei = b"q /Fm0 Do Q\n".to_vec();
    let (mut doc, _) = zwei_seiten(doc, &[(resources_id, eins), (resources_id, zwei)]);
    // Die Schwärzung liegt auf **Seite 2**.
    let (report, out) = schwaerze(&mut doc, &[schwaerzung(1, ueber_dem_bild())]);
    assert_eq!(report.redacted_images, 1, "Vorbedingung: die Pixel fallen");
    assert_eq!(
        report.image_alt_texts_cleared, 1,
        "der zurückgestellte Abschnitt wird auch gezählt"
    );
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Spiegel auf Seite 1: {:?}",
        leaks(&out, GEHEIM)
    );
}

/// **Die Gegenrichtung.** Dasselbe Dokument, die Schwärzung weit weg vom Bild:
/// der Spiegel über dem Formular bleibt, und es gibt keine neue Warnung. Eine
/// Grenze, die gewöhnliche Barrierefreiheit abräumt, wäre genauso ein Fehler
/// wie die Lücke.
#[test]
fn formular_ohne_treffer_behaelt_seinen_spiegel() {
    let mut doc = Document::with_version("1.5");
    let (form, im0) = formular_mit_bild(&mut doc);
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => form },
    });
    let content = format!("/Figure <</Alt ({HARMLOS})>> BDC\nq /Fm0 Do Q\nEMC\n");
    let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
    // Das Bild liegt bei (50,600)-(150,700).
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(400.0, 100.0, 450.0, 150.0))],
    );
    assert_eq!(report.redacted_images, 0, "kein Bild wird angefasst");
    assert_eq!(report.image_alt_texts_cleared, 0);
    assert!(
        nur_ocr_hinweis(&report),
        "keine Warnung außer dem OCR-Hinweis: {:?}",
        report.warnings
    );
    assert!(
        !leaks(&out, HARMLOS).is_empty(),
        "der unbeteiligte Spiegel muss bleiben"
    );
    // Und das Bild selbst ist unverändert.
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    let pixel = |doc: &Document, id: ObjectId| match doc.get_object(id).expect("Bild") {
        Object::Stream(stream) => stream.content.clone(),
        other => panic!("kein Strom: {other:?}"),
    };
    assert_eq!(pixel(&aus, im0), pixel(&doc, im0));
}

// ---------------------------------------------------------------------------
// 2. Die nicht getaggte Datei
// ---------------------------------------------------------------------------

/// **Register #20, Einwand 2.** Ein unlesbares Bild mit `/Alt` am Dictionary,
/// auf einer Seite **ohne** Marked Content. Die Pixel bleiben (das sagt die
/// Warnung); der Ersatztext darf nicht bleiben.
///
/// Mutation, die diesen Test rot macht: in `ScanResult::image` den Filter
/// `if !self.mirror_streams.contains(&cx.stream) { return; }` wieder einsetzen
/// — dann liefert eine Seite ohne `BDC` keine Platzierung, `blacked_images`
/// bleibt leer, und `clear_image_alternates` läuft nie.
#[test]
fn unlesbares_bild_auf_nicht_getaggter_seite_verliert_sein_alt() {
    let mut doc = Document::with_version("1.5");
    let mut roh = unlesbares_bild();
    roh.dict.set("Alt", Object::string_literal(GEHEIM));
    let im0 = doc.add_object(Object::Stream(roh));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    let (mut doc, _) = eine_seite(
        doc,
        resources_id,
        b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
    );
    let (report, out) = schwaerze_mit(&mut doc, &[schwaerzung(0, ueber_dem_bild())], true);
    assert_eq!(report.redacted_images, 0, "die Pixel bleiben (unlesbar)");
    assert!(
        report.warnings.iter().any(|w| w.contains("dekodieren")),
        "und es wird gesagt: {:?}",
        report.warnings
    );
    assert_eq!(
        report.image_alt_texts_cleared, 1,
        "das /Alt am stehengebliebenen Dictionary, kein Spiegel im Strom"
    );
    assert_eq!(alt_am_objekt(&doc, im0), None);
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Ersatztext am Bilddictionary: {:?}",
        leaks(&out, GEHEIM)
    );
}

/// Dieselbe Seite mit einem **lesbaren** Bild und ohne Zugeständnis: das `/Alt`
/// fällt hier mit den Pixeln, weil `crate::image` das Dictionary eines neu
/// kodierten Bildes aus den Bildeigenschaften neu aufbaut.
///
/// Der Test steht hier, weil die Begründung für den engen Zuschnitt von
/// `clear_image_alternates` **genau diese** Zusicherung ist. Fällt sie weg,
/// muss es auffallen: dann bleibt der Klartext in der Datei, und die
/// Entscheidung „nur mit `--allow-undecodable-images` greifen“ wäre falsch.
///
/// `image_alt_texts_cleared` bleibt dabei **0** — die Zahl zählt entfernte
/// Schlüssel dieser Stelle, und dieser fiel eine Ebene tiefer. Der Verlust ist
/// damit nirgends beziffert; das ist bekannt und steht am Feld.
#[test]
fn lesbares_bild_auf_nicht_getaggter_seite_verliert_sein_alt_mit_den_pixeln() {
    let mut doc = Document::with_version("1.5");
    let mut roh = bild(20, 20);
    roh.dict.set("Alt", Object::string_literal(GEHEIM));
    let im0 = doc.add_object(Object::Stream(roh));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    let (mut doc, _) = eine_seite(
        doc,
        resources_id,
        b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
    );
    let (report, out) = schwaerze(&mut doc, &[schwaerzung(0, ueber_dem_bild())]);
    assert_eq!(report.redacted_images, 1, "die Pixel fallen");
    assert_eq!(report.image_alt_texts_cleared, 0);
    assert_eq!(alt_am_objekt(&doc, im0), None, "/Alt fiel mit den Pixeln");
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Ersatztext am Bilddictionary: {:?}",
        leaks(&out, GEHEIM)
    );
}

/// **Warum der enge Zuschnitt keine Lücke ist.** Ohne
/// `--allow-undecodable-images` gibt es den Fall „getroffen, Pixel bleiben,
/// Dictionary bleibt“ gar nicht: der Lauf bricht ab und gibt keine Datei aus.
/// Es bleibt also kein Bild mit `/Alt` und unversehrten Pixeln stillschweigend
/// zurück.
///
/// Mutation, die diesen Test rot macht: keine in `redact.rs` — der Test hält
/// die Zusicherung von `crate::image` fest, auf der `keep_image_dicts` ruht.
#[test]
fn unlesbares_bild_unter_der_schwaerzung_bricht_ohne_zugestaendnis_ab() {
    let mut doc = Document::with_version("1.5");
    let mut roh = unlesbares_bild();
    roh.dict.set("Alt", Object::string_literal(GEHEIM));
    let im0 = doc.add_object(Object::Stream(roh));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    let (mut doc, _) = eine_seite(
        doc,
        resources_id,
        b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
    );
    let fehler = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &[schwaerzung(0, ueber_dem_bild())])
        .expect_err("ohne Zugeständnis muss der Lauf abbrechen");
    let text = fehler.to_string();
    assert!(
        text.contains("dekodieren"),
        "und der Grund wird benannt: {text}"
    );
}

// ---------------------------------------------------------------------------
// 3. Die Hülle ist nicht die Fläche
// ---------------------------------------------------------------------------

/// Ein um 45° gedrehtes Bild mit seiner Beschreibung, geschwärzt wird der Text
/// in der **leeren Ecke seiner Hülle**. Die Raute hat dort keinen Bildpunkt;
/// `crate::image` fasst das Bild richtigerweise nicht an, `redacted_images`
/// bleibt 0, und es gibt keine Warnung. Also darf auch nichts fallen.
///
/// Geprüft wird **beide** Betriebsarten: dass der Fall nicht bloß deshalb gut
/// ausgeht, weil `clear_image_alternates` ohne `--allow-undecodable-images`
/// gar nicht gefüttert wird. Der Spiegel im Strom hängt davon ohnehin nicht ab.
///
/// Mutation, die diesen Test rot macht: in der Seitenschleife
/// `placement.covers(rect)` durch `placement.bounds.intersects(rect)` ersetzen.
#[test]
fn gedrehtes_bild_behaelt_in_der_leeren_huellenecke_alles() {
    // 45°, Kantenlänge 100: Ecken (300,400), (370.7,470.7), (300,541.4),
    // (229.3,470.7). Hülle (229.3,400)-(370.7,541.4); die linke untere Ecke
    // ist leer. Die Raute ist dort, wo x + y >= 700 gilt.
    for allow_undecodable in [false, true] {
        let mut doc = Document::with_version("1.5");
        let mut gedreht = bild(32, 32);
        gedreht.dict.set("Alt", Object::string_literal(HARMLOS));
        let im0 = doc.add_object(Object::Stream(gedreht));
        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { "Im0" => im0 },
        });
        let content = format!(
            "/Figure <</Alt ({HARMLOS})>> BDC\n\
             q 70.7107 70.7107 -70.7107 70.7107 300 400 cm /Im0 Do Q\nEMC\n"
        );
        let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
        // (231,401)-(250,420): in der Hülle, außerhalb der Raute (250+420 = 670).
        let (report, out) = schwaerze_mit(
            &mut doc,
            &[schwaerzung(0, Rect::new(231.0, 401.0, 250.0, 420.0))],
            allow_undecodable,
        );
        assert_eq!(
            report.redacted_images, 0,
            "Vorbedingung: kein Bildpunkt fällt (allow_undecodable = {allow_undecodable}) — {:?}",
            report.warnings
        );
        assert!(
            nur_ocr_hinweis(&report),
            "keine Warnung außer dem OCR-Hinweis: {:?}",
            report.warnings
        );
        assert_eq!(
            report.image_alt_texts_cleared, 0,
            "nichts fällt, also zählt nichts"
        );
        let aus = load_from_bytes(&out).expect("Ausgabe lädt");
        assert_eq!(
            alt_am_objekt(&aus, im0).as_deref(),
            Some(HARMLOS),
            "das unversehrte gedrehte Bild behält sein /Alt \
             (allow_undecodable = {allow_undecodable})"
        );
        assert!(
            !leaks(&out, &format!("/Figure <</Alt({HARMLOS})>> BDC")).is_empty(),
            "der Spiegel über dem unversehrten Bild bleibt \
             (allow_undecodable = {allow_undecodable})"
        );
    }
}

/// Die Gegenrichtung, damit der Test darüber nicht aus dem falschen Grund grün
/// wird: dasselbe gedrehte Bild, das Rechteck jetzt **mitten** in der Raute.
/// Dann fallen Bildpunkte, und Spiegel und Ersatztext gehören mit ihnen.
#[test]
fn gedrehtes_bild_mitten_in_der_raute_verliert_alles() {
    let mut doc = Document::with_version("1.5");
    let mut gedreht = bild(32, 32);
    gedreht.dict.set("Alt", Object::string_literal(GEHEIM));
    let im0 = doc.add_object(Object::Stream(gedreht));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    let content = format!(
        "/Figure <</Alt ({GEHEIM})>> BDC\n\
         q 70.7107 70.7107 -70.7107 70.7107 300 400 cm /Im0 Do Q\nEMC\n"
    );
    let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
    // (290,460)-(310,480) liegt ganz in der Raute um (300, 470.7).
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(290.0, 460.0, 310.0, 480.0))],
    );
    assert_eq!(
        report.redacted_images, 1,
        "Vorbedingung: Bildpunkte fallen — {:?}",
        report.warnings
    );
    assert_eq!(report.image_alt_texts_cleared, 1, "der Spiegel im Strom");
    assert!(leaks(&out, GEHEIM).is_empty(), "{:?}", leaks(&out, GEHEIM));
    assert_eq!(alt_am_objekt(&doc, im0), None);
}

/// Dieselbe Frage eine Ebene tiefer: das gedrehte Bild steckt im Formular, der
/// Spiegel steht im Seitenstrom. Auch der neue Weg über `touches_form_image`
/// darf sich nicht an der Hülle entscheiden.
#[test]
fn gedrehtes_bild_im_formular_behaelt_in_der_huellenecke_seinen_spiegel() {
    let mut doc = Document::with_version("1.5");
    let im0 = doc.add_object(Object::Stream(bild(32, 32)));
    let form = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => im0 } },
            },
            b"q 70.7107 70.7107 -70.7107 70.7107 300 400 cm /Im0 Do Q\n".to_vec(),
        )
        .with_compression(false),
    ));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => form },
    });
    let content = format!("/Figure <</Alt ({HARMLOS})>> BDC\nq /Fm0 Do Q\nEMC\n");
    let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(231.0, 401.0, 250.0, 420.0))],
    );
    assert_eq!(report.redacted_images, 0, "kein Bildpunkt fällt");
    assert_eq!(report.image_alt_texts_cleared, 0);
    assert!(
        !leaks(&out, HARMLOS).is_empty(),
        "der Spiegel über dem unversehrten Formularbild bleibt"
    );
}

// ---------------------------------------------------------------------------
// Das geteilte Bild: wen der Ersatztext am Dictionary trifft
// ---------------------------------------------------------------------------

/// Zwei Seiten, **ein** Logo im Briefkopf, geschwärzt nur auf Seite 1. Die
/// Gegenprobe der Gegenprüfer, hier mit `--allow-undecodable-images` — also in
/// der Betriebsart, in der `clear_image_alternates` überhaupt greift.
///
/// `crate::image` legt für die Schwärzung eine **Kopie** an: Seite 1 zeigt auf
/// das neue Objekt mit den geschwärzten Pixeln, Seite 2 weiter auf das alte.
/// Der Seitenscan läuft **nach** der Bildschwärzung und findet deshalb die
/// Kopie; das alte Objekt, das Seite 2 bedient, wird nicht angefasst und behält
/// seine Beschreibung. Kein Pixel verloren, also kein Ersatztext verloren.
#[test]
fn geteiltes_logo_behaelt_auf_der_unberuehrten_seite_seinen_ersatztext() {
    let mut doc = Document::with_version("1.5");
    let mut logo = bild(32, 32);
    logo.dict.set("Alt", Object::string_literal(HARMLOS));
    let logo_id = doc.add_object(Object::Stream(logo));
    let res1 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Logo" => logo_id },
    });
    let res2 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Logo" => logo_id },
    });
    let inhalt = format!(
        "/Figure <</Alt ({HARMLOS})>> BDC
q 100 0 0 100 50 600 cm /Logo Do Q
EMC
"
    );
    let (mut doc, seiten) = zwei_seiten(
        doc,
        &[
            (res1, inhalt.clone().into_bytes()),
            (res2, inhalt.into_bytes()),
        ],
    );
    let (report, out) = schwaerze_mit(&mut doc, &[schwaerzung(0, ueber_dem_bild())], true);
    assert_eq!(
        report.redacted_images, 1,
        "Vorbedingung: ein Bild verliert Pixel"
    );
    assert_eq!(
        report.copied_images, 1,
        "Vorbedingung: das geteilte Bild wird kopiert"
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(
        alt_am_objekt(&aus, logo_id).as_deref(),
        Some(HARMLOS),
        "das Objekt, das Seite 2 bedient, behält seine Beschreibung —          image_alt_texts_cleared = {}",
        report.image_alt_texts_cleared
    );
    // Der Spiegel auf Seite 2 bleibt auch: dort ist nichts geschwärzt.
    assert!(
        !leaks(&out, HARMLOS).is_empty(),
        "die Beschreibung des unberührten Bildes ist ganz aus der Datei verschwunden"
    );
    let _ = seiten;
}

/// **Wo die grobe Richtung wirklich zu viel nimmt — und dass es dasteht.** Ein
/// **unlesbares** Bild, von zwei Seiten benutzt, geschwärzt nur auf Seite 1.
/// `crate::image` kann es nicht auspacken, legt also keine Kopie an und rührt
/// das Objekt nicht an — beide Seiten zeigen weiter darauf. Das `/Alt` fällt
/// deshalb **für beide**, obwohl Seite 2 keine Schwärzung hat.
///
/// Das ist Absicht und die einzige Stelle, an der diese Über-Schwärzung
/// bleibt: was von einem unlesbaren Bild unter der Schwärzung lag, weiß
/// niemand. Bedingung ist, dass es **gesagt** wird — dieser Test prüft die
/// Warnung mit. Ohne `--allow-undecodable-images` gibt es den Fall gar nicht
/// (siehe `unlesbares_bild_unter_der_schwaerzung_bricht_ohne_zugestaendnis_ab`).
#[test]
fn geteiltes_unlesbares_bild_verliert_seinen_ersatztext_fuer_beide_seiten() {
    let mut doc = Document::with_version("1.5");
    let mut logo = unlesbares_bild();
    logo.dict.set("Alt", Object::string_literal(HARMLOS));
    let logo_id = doc.add_object(Object::Stream(logo));
    let res1 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Logo" => logo_id },
    });
    let res2 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Logo" => logo_id },
    });
    let inhalt = b"q 100 0 0 100 50 600 cm /Logo Do Q
"
    .to_vec();
    let (mut doc, _) = zwei_seiten(doc, &[(res1, inhalt.clone()), (res2, inhalt)]);
    let (report, out) = schwaerze_mit(&mut doc, &[schwaerzung(0, ueber_dem_bild())], true);
    assert_eq!(report.redacted_images, 0, "die Pixel bleiben (unlesbar)");
    assert!(
        report.warnings.iter().any(|w| w.contains("dekodieren")),
        "der Verlust ohne Gegenwert muss gesagt werden: {:?}",
        report.warnings
    );
    assert_eq!(report.image_alt_texts_cleared, 1, "das /Alt am Dictionary");
    assert_eq!(alt_am_objekt(&doc, logo_id), None);
    assert!(
        leaks(&out, HARMLOS).is_empty(),
        "{:?}",
        leaks(&out, HARMLOS)
    );
}

/// **Ein unlesbares Bild, zweimal auf einer Seite: nur die getroffene
/// Platzierung verliert ihren Spiegel.**
///
/// `note_undecided` vermerkte *jede* Platzierung des Objekts auf der Seite, mit
/// derselben Begründung wie `note_lost_pixels`: das Objekt zeige jetzt
/// geschwärzte Bildpunkte, also auch dort, wo keine Zone lag. Die Begründung
/// greift hier gerade **nicht**. Unlesbar heißt, dass *kein* Bildpunkt fällt:
/// das Objekt wird weder überschrieben noch kopiert, und die zweite Platzierung
/// zeigt buchstäblich dasselbe Bild wie vor dem Lauf. Ihr Spiegel ist wahr, sein
/// Verlust hat keinen Gegenwert — Fehlalarm, und zwar derselbe, den
/// `zl_b_pendel_beide_richtungen` eine Ebene weiter vorn gefunden hat.
///
/// Die **getroffene** Platzierung verliert ihren Spiegel weiter: dort sollte
/// etwas verschwinden und verschwindet nicht, und der Text darüber beschreibt
/// möglicherweise genau das. Das ist die grobe Entscheidung, für die
/// `--allow-undecodable-images` einsteht, und sie steht neben einer Warnung.
///
/// Das `/Alt` am **Bilddictionary** fällt davon unberührt weiter für beide: es
/// hängt an der Objekt-Id und ist nicht je Platzierung zu haben — siehe
/// `geteiltes_unlesbares_bild_verliert_seinen_ersatztext_fuer_beide_seiten`.
/// Hier trägt das Dictionary deshalb gar kein `/Alt`, damit die Zahl allein die
/// Spiegel im Strom zählt.
///
/// Mutation, die diesen Test rot macht: in `note_undecided` wieder alle
/// Platzierungen vermerken (`note_lost_pixels(outcome, page_id, placements,
/// key, None)` statt der Schleife über die getroffenen).
#[test]
fn unlesbares_bild_zweimal_auf_einer_seite_verliert_nur_den_getroffenen_spiegel() {
    let mut doc = Document::with_version("1.5");
    let logo_id = doc.add_object(Object::Stream(unlesbares_bild()));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Logo" => logo_id },
    });
    // Zwei Platzierungen desselben Objekts, jede unter ihrem eigenen Spiegel.
    // Die Schwärzung liegt über der ersten, bei (50,600)-(150,700); die zweite
    // steht bei (400,600) und wird von ihr nicht berührt.
    let content = format!(
        "/Figure <</Alt ({GEHEIM})>> BDC
q 100 0 0 100 50 600 cm /Logo Do Q
EMC
/Figure <</Alt ({HARMLOS})>> BDC
q 100 0 0 100 400 600 cm /Logo Do Q
EMC
"
    );
    let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
    let (report, out) = schwaerze_mit(&mut doc, &[schwaerzung(0, ueber_dem_bild())], true);

    assert_eq!(
        report.redacted_images, 0,
        "Vorbedingung: kein Bildpunkt fällt (das Bild ist unlesbar) — {:?}",
        report.warnings
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("dekodieren")),
        "Vorbedingung: der Verlust ohne Gegenwert wird gesagt — {:?}",
        report.warnings
    );
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "der Spiegel über der getroffenen Platzierung fällt: {:?}",
        leaks(&out, GEHEIM)
    );
    assert!(
        !leaks(&out, HARMLOS).is_empty(),
        "der Spiegel über der unberührten Platzierung bleibt stehen — die Bildpunkte, \
         die sie zeichnet, sind dieselben wie vorher"
    );
    assert_eq!(
        report.image_alt_texts_cleared, 1,
        "genau einer, nicht beide"
    );
}

// ---------------------------------------------------------------------------
// Der feine Rest: die Fläche ist nicht das Pixelgitter
// ---------------------------------------------------------------------------

/// **Der feine Rest ist keiner mehr.** Entschieden wurde einmal an der *Fläche*
/// der Platzierung und an den vier *Ecken* jeder Pixelzelle. Dazwischen lag ein
/// Rest: ein Schwärzungsrechteck, das ganz **zwischen** den Gitterlinien eines
/// Bildes liegt, traf die Fläche, aber keine Zellecke. `redacted_images` blieb
/// 0, es gab keine Warnung — und der Spiegel fiel trotzdem. Fehlalarm und Leck
/// in einem, und genau so stand der Fall hier beziffert, mit dem Satz „behoben
/// ist er erst, wenn `crate::image` je Platzierung berichtet, ob dort
/// Bildpunkte fielen".
///
/// Das tut es jetzt. Derselbe Fall, und er geht anders aus: ein **2 x 2** Bild
/// auf 100 x 100 Punkte gezogen — eine Bildzelle ist 50 Punkte groß. Das
/// Rechteck (145,645)-(149,649) liegt mitten im Bild und zwischen allen
/// Gitterpunkten (x ∈ {50,100,150}, y ∈ {600,650,700}), also ganz **in** einer
/// Zelle, ohne eine ihrer Ecken zu enthalten. Die Flächenprüfung je Zelle
/// (`cell_meets_rect`, trennende Achsen statt Ecken) trifft sie: ein Bildpunkt
/// fällt, das Bild wird neu geschrieben, und der Spiegel darüber fällt mit
/// Recht.
///
/// **Der `/Alt` am Bilddictionary fällt mit, ohne Zugeständnis.** Das neu
/// kodierte Bild entsteht aus einem frischen Dictionary (`encode_xobject`); ein
/// `/Alt` der Eingabe wird dort nicht wieder eingetragen. Deshalb steht in
/// beiden Betriebsarten dieselbe Zahl, und `--allow-undecodable-images` ändert
/// hier nichts: das Bild ließ sich dekodieren. Das Zugeständnis steht trotzdem
/// als Schleife da — es war die Stelle, an der die beiden Betriebsarten früher
/// auseinanderliefen.
///
/// Mutation, die diesen Test rot macht: in `cell_meets_rect` die trennenden
/// Achsen durch die alte Vier-Ecken-Prüfung ersetzen (liegt eine Zellecke im
/// Rechteck?). Dann fällt kein Bildpunkt und `redacted_images` ist 0. Gelaufen:
/// 25 Tests in fünf Dateien werden davon rot, dieser darunter.
#[test]
fn rechteck_ganz_zwischen_den_gitterlinien_schwaerzt_seine_zelle() {
    for allow_undecodable in [false, true] {
        let mut doc = Document::with_version("1.5");
        let mut grob = bild(2, 2);
        grob.dict.set("Alt", Object::string_literal(HARMLOS));
        let im0 = doc.add_object(Object::Stream(grob));
        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { "Im0" => im0 },
        });
        let content = format!(
            "/Figure <</Alt ({HARMLOS})>> BDC
q 100 0 0 100 50 600 cm /Im0 Do Q
EMC
"
        );
        let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
        let (report, out) = schwaerze_mit(
            &mut doc,
            &[schwaerzung(0, Rect::new(145.0, 645.0, 149.0, 649.0))],
            allow_undecodable,
        );
        assert_eq!(
            report.redacted_images, 1,
            "der Bildpunkt unter dem Rechteck fällt (allow_undecodable = \
             {allow_undecodable}) — {:?}",
            report.warnings
        );
        assert!(
            nur_ocr_hinweis(&report),
            "und niemand muss etwas einräumen: {:?}",
            report.warnings
        );
        assert_eq!(
            report.image_alt_texts_cleared, 1,
            "genau der eine Spiegel über der Platzierung (allow_undecodable = \
             {allow_undecodable})"
        );
        // Er fällt — und diesmal ist das die Wahrheit über die Bildpunkte.
        assert!(
            leaks(&out, &format!("/Figure <</Alt({HARMLOS})>> BDC")).is_empty(),
            "der Spiegel fällt, weil ein Bildpunkt fiel"
        );
        let aus = load_from_bytes(&out).expect("Ausgabe lädt");
        assert_eq!(
            alt_am_objekt(&aus, im0),
            None,
            "/Alt am Bilddictionary: das neu kodierte Bild trägt es nicht mehr \
             (allow_undecodable = {allow_undecodable})"
        );
    }
}

// ---------------------------------------------------------------------------
// Was der weggefallene Filter kostet
// ---------------------------------------------------------------------------

/// Der Filter „nur in Strömen mit Spiegel“ war mit den Kosten begründet. Hier
/// steht die Zahl: eine Seite mit vielen Bildplatzierungen und **ohne** jeden
/// Textspiegel — genau der Fall, für den der Filter gedacht war.
///
/// Gemessen (Debug, dieselbe Maschine, Filter abwechselnd wieder eingesetzt und
/// weggelassen):
///
/// | Platzierungen | mit Filter | ohne Filter |
/// |--------------:|-----------:|------------:|
/// |        10 000 |    1,223 s |     1,232 s |
/// |        50 000 |    6,176 s |     6,180 s |
/// |       100 000 |   12,208 s |    12,301 s |
///
/// Unter einem Prozent, und die Zeit gehört dem Interpreter, nicht der Liste:
/// sie wächst **linear** in den Platzierungen, und jede davon ist eine
/// Operation, die schon aus `Budget::operation` zahlt. Der Filter kostete
/// nichts ein und deckte zwei Lecks zu.
///
/// Kein Urteil, nur eine Messung, deshalb `#[ignore]`. Lauf:
/// `cargo test -p redact-pdf --test zh2_b_bildfrage -- --ignored --nocapture`.
#[test]
#[ignore = "Messung"]
fn mess_bildplatzierungen_ohne_spiegel() {
    for anzahl in [10_000usize, 50_000, 100_000] {
        let mut doc = Document::with_version("1.5");
        let im0 = doc.add_object(Object::Stream(bild(8, 8)));
        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { "Im0" => im0 },
        });
        let mut content = Vec::new();
        for index in 0..anzahl {
            let y = 10.0 + (index % 700) as f64;
            content.extend_from_slice(format!("q 10 0 0 10 400 {y} cm /Im0 Do Q\n").as_bytes());
        }
        let (mut doc, _) = eine_seite(doc, resources_id, content);
        let start = std::time::Instant::now();
        let report = PdfRedactor::with_padding(0.0)
            .allowing_undecodable_images(true)
            .apply_with_report(
                &mut doc,
                &[schwaerzung(0, Rect::new(10.0, 10.0, 20.0, 20.0))],
            )
            .expect("Lauf");
        println!(
            "{anzahl} Platzierungen ohne Spiegel: {:?}, überschriebene Bilder {}",
            start.elapsed(),
            report.redacted_images
        );
    }
}
