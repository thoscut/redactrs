//! Gegenprüfung A, Runde 9 — der **vierte** Schlag des Bild-Pendels.
//!
//! Die Runde 8 hat `enum Fate` eingeführt, damit zwei Leser (`write_work` beim
//! Schreiben, `fill_page` beim Vermerken) **dieselbe** Antwort benutzen. Die
//! Antwort ist `Fate::Copy(&work.name)` — und `work.name` ist der Name der
//! **ersten getroffenen** Platzierung der Gruppe (`image.rs::fill_page`,
//! `first.name()`).
//!
//! Hier wird gefragt, was die Korrektur nicht vorgesehen hat: derselbe
//! Bildstrom, auf **derselben Seite** unter **zwei Namen** gezeichnet, und
//! **beide** Platzierungen liegen unter einer Schwärzung. Dann wird eine Kopie
//! geschrieben und genau **ein** Name umgebogen (`repoint_page` setzt einen
//! Schlüssel); der zweite Name zeigt weiter auf das unversehrte Original.
//!
//! Maßstab ist nicht der Bericht, sondern die Ausgabebytes: die Bildpunkte, die
//! die Ausgabeseite an der Stelle der Schwärzung **zeigt** (über
//! `redact_pdf::page_ops`, denselben Dekoder, den der Renderer benutzt), und
//! die Rohbytes des Stroms, auf den der zweite Name zeigt.
//!
//! Alle Zahlen sind in f64 exakt (ganze Zahlen, Zweierpotenzen).

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Point, Rect, Redaction, Region, Source};
use redact_pdf::{load_from_bytes, page_ops, save_to_bytes, DrawOp, PdfRedactor, RedactionReport};

/// Steht buchstäblich in den Bildpunkten: 27 Zeichen = 9 RGB-Bildpunkte.
const GEHEIM: &str = "DE89 3704 0044 0532 0130 00";

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

/// Ein 9 x 4 RGB-Bild, **unkomprimiert**, dessen zweite Bildpunktzeile die
/// Zeichen von `GEHEIM` als Abtastwerte trägt. Unkomprimiert, damit die
/// Gegenprobe an den Rohbytes der Ausgabedatei ohne Entpacken auskommt.
fn bild_mit_klartext_bildpunkten() -> Stream {
    let breite = 9u32;
    let hoehe = 4u32;
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
    assert_eq!(data.len(), (breite * hoehe * 3) as usize);
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

/// Ohne `padding`: das Rechteck ist genau das, was der Test hinschreibt.
fn schwaerze(doc: &mut Document, list: &[Redaction]) -> (RedactionReport, Vec<u8>) {
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(doc, list)
        .expect("Schwärzung läuft");
    let bytes = save_to_bytes(doc).expect("Speichern");
    (report, bytes)
}

/// **Das ehrliche Maß.** Welche Bildpunkte zeigt diese Seite der Ausgabedatei
/// innerhalb von `rect`?
///
/// Gelesen wird die Ausgabedatei, nicht der Bericht: `page_ops` löst jede
/// Bildplatzierung auf, dekodiert ihr Bild und nennt die CTM. Für jeden
/// Bildpunkt wird sein **Mittelpunkt** in den User-Space abgebildet; liegt er
/// im Rechteck, zählt seine Farbe als sichtbar.
fn sichtbare_bildpunkte(bytes: &[u8], seite: usize, rect: Rect) -> Vec<[u8; 4]> {
    let doc = load_from_bytes(bytes).expect("Ausgabe lädt");
    let ops = page_ops(&doc, seite).expect("page_ops läuft");
    let mut farben = Vec::new();
    for op in &ops.ops {
        let DrawOp::Image { image, ctm, .. } = op else {
            continue;
        };
        let raster = &ops.images[*image];
        let (w, h) = (raster.width as f64, raster.height as f64);
        for y in 0..raster.height as usize {
            for x in 0..raster.width as usize {
                let u = (x as f64 + 0.5) / w;
                let v = 1.0 - (y as f64 + 0.5) / h;
                let p = ctm.apply(u, v);
                if !rect.contains(Point::new(p.x, p.y)) {
                    continue;
                }
                let off = (y * raster.width as usize + x) * 4;
                farben.push([
                    raster.rgba[off],
                    raster.rgba[off + 1],
                    raster.rgba[off + 2],
                    raster.rgba[off + 3],
                ]);
            }
        }
    }
    farben
}

/// Die Rohbytes des Stroms, auf den `name` in den Ressourcen dieser Seite zeigt.
fn bytes_hinter_dem_namen(bytes: &[u8], seite: ObjectId, name: &[u8]) -> Vec<u8> {
    let doc = load_from_bytes(bytes).expect("Ausgabe lädt");
    let res = doc
        .get_dictionary(seite)
        .and_then(|d| d.get(b"Resources"))
        .and_then(|o| doc.dereference(o).map(|(_, r)| r))
        .and_then(|o| o.as_dict())
        .expect("Ressourcen der Seite")
        .clone();
    let xobjects = res
        .get(b"XObject")
        .and_then(|o| doc.dereference(o).map(|(_, r)| r))
        .and_then(|o| o.as_dict())
        .expect("XObject-Verzeichnis")
        .clone();
    let entry = xobjects.get(name).expect("Name im Verzeichnis");
    let (_, resolved) = doc.dereference(entry).expect("auflösbar");
    resolved.as_stream().expect("Bildstrom").content.clone()
}

// ===========================================================================
// Der Befund
// ===========================================================================

/// **Der vierte Schlag.** Dasselbe Bildobjekt wird auf Seite 1 unter zwei Namen
/// gezeichnet (`/Im0` links, `/Im1` rechts) und außerdem auf Seite 2 — es hängt
/// damit an zwei Seiten, also wird kopiert statt überschrieben
/// (`Fate::Copy`). **Beide** Platzierungen der Seite 1 liegen vollständig unter
/// einer Schwärzung.
///
/// `fate` antwortet `Fate::Copy(&work.name)`, und `work.name` ist der Name der
/// **ersten** getroffenen Platzierung, also `/Im0`. `write_work` biegt genau
/// diesen einen Namen um. `/Im1` zeigt danach weiter auf das unversehrte
/// Original — und zeichnet auf Seite 1, mitten in der Schwärzung, die
/// ungeschwärzten Bildpunkte.
///
/// Richtige Erwartung: nach dem Lauf darf **kein** Bildpunkt, den Seite 1
/// innerhalb des Schwärzungsrechtecks zeigt, noch seinen Eingabewert haben. Die
/// Runde 8 hat die Frage „wer zeigt die geschwärzten Bildpunkte?“ an **eine**
/// Stelle gelegt; sie hat aber nicht dafür gesorgt, dass **alle** Namen, die
/// unter der Schwärzung liegen, auf die Kopie zeigen. Ein zweiter
/// `repoint_page`-Aufruf je getroffenem Namen — oder, KISS, `Fate::Overwrite`,
/// sobald mehr als ein Name derselben Seite getroffen ist — schließt es.
#[test]
fn zweiter_name_desselben_bildes_zeigt_ungeschwaerzte_bildpunkte() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext_bildpunkten()));
    let res1 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id, "Im1" => bild_id },
    });
    let res2 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    let inhalt1 =
        b"q 100 0 0 100 50 600 cm /Im0 Do Q\nq 100 0 0 100 400 600 cm /Im1 Do Q\n".to_vec();
    let inhalt2 = b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec();
    let (mut doc, ids) = seiten(doc, vec![(res1, inhalt1), (res2, inhalt2)]);

    // Ein Rechteck über BEIDEN Platzierungen der Seite 1.
    let zone = Rect::new(40.0, 590.0, 510.0, 710.0);

    // Vorbedingung: beide Platzierungen liegen unter der Schwärzung.
    let scan = redact_pdf::scan_page(&doc, ids[0]).expect("Scan läuft");
    assert_eq!(scan.images.len(), 2, "Vorbedingung: zwei Platzierungen");
    assert!(
        scan.images.iter().all(|p| p.covers(&zone)),
        "Vorbedingung: beide Platzierungen liegen unter der Schwärzung"
    );
    // Vorbedingung: vor dem Lauf sind die Klartext-Bildpunkte sichtbar.
    let vorher = save_to_bytes(&doc).expect("Speichern");
    assert!(
        sichtbare_bildpunkte(&vorher, 0, zone).len() >= 72,
        "Vorbedingung: beide Bilder zeigen ihre 36 Bildpunkte"
    );

    let (report, out) = schwaerze(&mut doc, &[schwaerzung(0, zone)]);

    // Vorbedingung: kopiert, nicht überschrieben (das Bild hängt an zwei Seiten).
    assert_eq!(
        report.copied_images, 1,
        "Vorbedingung: eine Kopie — {:?}",
        report.warnings
    );

    // Der Befund, am ehrlichen Maß: kein Bildpunkt unter der Schwärzung darf
    // noch seinen Eingabewert tragen.
    let sichtbar = sichtbare_bildpunkte(&out, 0, zone);
    let klartext: Vec<[u8; 4]> = GEHEIM
        .as_bytes()
        .chunks(3)
        .map(|c| [c[0], c[1], c[2], 255])
        .collect();
    let stehen_geblieben: Vec<&[u8; 4]> =
        sichtbar.iter().filter(|p| klartext.contains(p)).collect();
    assert!(
        stehen_geblieben.is_empty(),
        "Seite 1 zeigt innerhalb der Schwärzung noch {} ungeschwärzte \
         Klartext-Bildpunkte: {:?} — der zweite Name zeigt weiter auf das \
         unversehrte Original (image.rs::fate liefert Fate::Copy(work.name), \
         work.name ist der Name der ERSTEN getroffenen Platzierung; \
         repoint_page setzt genau einen Schlüssel). Warnungen: {:?}",
        stehen_geblieben.len(),
        stehen_geblieben,
        report.warnings
    );

    // Dieselbe Aussage an den Bytes: hinter /Im1 stehen die Klartextbytes noch.
    let hinter_im1 = bytes_hinter_dem_namen(&out, ids[0], b"Im1");
    assert!(
        !hinter_im1
            .windows(GEHEIM.len())
            .any(|w| w == GEHEIM.as_bytes()),
        "der Strom hinter /Im1 trägt die Klartext-Bildpunkte unverändert"
    );
}

/// Dieselbe Wurzel, über **Stromgrenzen**: `write_work` läuft über
/// `work.streams` und biegt in **jedem** Strom denselben `work.name` um.
///
/// Material: Seite 1 zeichnet das Bild einmal unmittelbar (Name `/Im0`, im
/// Seitenstrom) und einmal über ein Form-XObject, dessen eigene Ressourcen es
/// unter dem Namen `/ImA` führen. Beide Platzierungen liegen unter der
/// Schwärzung; das Bild hängt außerdem an Seite 2, wird also kopiert.
///
/// `work.name` ist `/Im0` (die erste getroffene Platzierung). `repoint_form`
/// setzt daraufhin `/Im0` in die Ressourcen des **Formulars** — einen Namen, den
/// das Formular nie zeichnet. Das Formular zeichnet weiter `/ImA`, und `/ImA`
/// zeigt weiter auf das unversehrte Original.
///
/// Richtige Erwartung: kein Bildpunkt innerhalb der Schwärzung trägt nach dem
/// Lauf noch seinen Eingabewert. Umzubiegen ist der Name **je getroffener
/// Platzierung** (Strom *und* Name gehören zusammen), nicht ein Name für alle
/// Ströme.
#[test]
fn formularname_und_seitenname_desselben_bildes_eine_kopie_reicht_nicht() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_klartext_bildpunkten()));
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
        "XObject" => dictionary! { "Im0" => bild_id, "Fm0" => form_id },
    });
    let res2 = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    let inhalt1 = b"q 100 0 0 100 50 600 cm /Im0 Do Q\nq 1 0 0 1 400 600 cm /Fm0 Do Q\n".to_vec();
    let inhalt2 = b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec();
    let (mut doc, ids) = seiten(doc, vec![(res1, inhalt1), (res2, inhalt2)]);

    let zone = Rect::new(40.0, 590.0, 510.0, 710.0);
    let scan = redact_pdf::scan_page(&doc, ids[0]).expect("Scan läuft");
    assert_eq!(
        scan.images.len(),
        2,
        "Vorbedingung: zwei Platzierungen (Seitenstrom und Formular)"
    );
    assert!(
        scan.images.iter().all(|p| p.covers(&zone)),
        "Vorbedingung: beide liegen unter der Schwärzung"
    );

    let (report, out) = schwaerze(&mut doc, &[schwaerzung(0, zone)]);
    assert_eq!(
        report.copied_images, 1,
        "Vorbedingung: eine Kopie — {:?}",
        report.warnings
    );

    let sichtbar = sichtbare_bildpunkte(&out, 0, zone);
    let klartext: Vec<[u8; 4]> = GEHEIM
        .as_bytes()
        .chunks(3)
        .map(|c| [c[0], c[1], c[2], 255])
        .collect();
    let stehen_geblieben = sichtbar.iter().filter(|p| klartext.contains(p)).count();
    assert_eq!(
        stehen_geblieben, 0,
        "Seite 1 zeigt innerhalb der Schwärzung noch {stehen_geblieben} \
         ungeschwärzte Klartext-Bildpunkte: der Name, den write_work umbiegt, \
         gehört zum Seitenstrom, umgebogen wird er aber auch im Formular \
         (image.rs::write_work über work.streams). Warnungen: {:?}",
        report.warnings
    );
}
