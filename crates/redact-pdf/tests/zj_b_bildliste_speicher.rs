//! Gegenprüfung J-B, dritte Linse: was kostet die **Streichung des Filters**
//! (LECK 2) an Speicher?
//!
//! Die Begründung, die jetzt an `ScanResult::images` steht, ist eine
//! Kostenrechnung: „**Keine eigene Decke, und warum keine nötig ist.** Die
//! Liste wächst *linear* in den Platzierungen — eine je `Do`/`BI` —, und jede
//! davon ist eine Operation, die aus demselben Aufwandskonto zahlt“. Gemessen
//! hat der Bericht dazu die **Wanduhr** (12,208 s gegen 12,301 s bei 100 000
//! Platzierungen). Der Speicher wurde nicht gemessen, obwohl er die Größe ist,
//! um derentwillen dieselbe Datei für **Glyphen** eine feste Decke hat
//! (`MAX_GLYPHS_PER_SCAN`, dort ausdrücklich „die eigentliche Speichergröße“).
//!
//! Dieser Lauf misst sie. Er ist `#[ignore]` — eine Messung, kein Wächter:
//!
//! ```text
//! flock /tmp/redactrs-cargo.lock cargo test -p redact-pdf \
//!   --test zj_b_bildliste_speicher -- --ignored --nocapture
//! ```

use lopdf::{dictionary, Document, Object, Stream};
use redact_pdf::content::ImagePlacement;

/// Spitzenwert des Adressraums dieses Prozesses (`VmHWM`, in KB).
fn hochwasser_kb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for zeile in status.lines() {
        if let Some(rest) = zeile.strip_prefix("VmHWM:") {
            if let Some(zahl) = rest.split_whitespace().next() {
                return zahl.parse().unwrap_or(0);
            }
        }
    }
    0
}

/// Ein 1 x 1 Bild — die Platzierung soll zählen, nicht das Bild.
fn winzbild() -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 1_i64,
            "Height" => 1_i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8_i64,
        },
        vec![0x80],
    )
    .with_compression(false)
}

/// `aussen` mal `innen` Bildplatzierungen, **ohne einen einzigen Spiegel**:
/// ein Formular mit `innen` mal `/Im0 Do`, von der Seite `aussen` mal
/// gezeichnet. Je Platzierung genau **eine** Operation, damit das
/// Aufwandskonto (`BASE_OPERATIONS` + 16 x Inhalt) nicht vorher aussteigt.
fn fanout(aussen: usize, innen: usize) -> (Document, lopdf::ObjectId, usize) {
    let mut doc = Document::with_version("1.5");
    let im0 = doc.add_object(Object::Stream(winzbild()));
    let form_inhalt = "/Im0 Do\n".repeat(innen).into_bytes();
    let form = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => im0 } },
            },
            form_inhalt.clone(),
        )
        .with_compression(false),
    ));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "F0" => form },
    });
    let seiten_inhalt = "/F0 Do\n".repeat(aussen).into_bytes();
    let bytes = form_inhalt.len() + seiten_inhalt.len();
    let content_id = doc.add_object(Stream::new(dictionary! {}, seiten_inhalt));
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
    (doc, page_id, bytes)
}

/// Die Messung: eine Datei mit rund 20 kB Zeichenanweisungen und **ohne
/// Textspiegel** lässt `ScanResult::images` auf eine Million Einträge wachsen.
#[test]
#[ignore = "Messung"]
fn mess_speicher_der_bildliste_ohne_spiegel() {
    eprintln!(
        "size_of::<ImagePlacement>() = {} Byte",
        std::mem::size_of::<ImagePlacement>()
    );
    for (aussen, innen) in [(100usize, 100usize), (1000, 500), (1000, 1000)] {
        let (doc, page_id, bytes) = fanout(aussen, innen);
        let vorher = hochwasser_kb();
        let begonnen = std::time::Instant::now();
        match redact_pdf::scan_page(&doc, page_id) {
            Ok(scan) => {
                let dauer = begonnen.elapsed();
                let nachher = hochwasser_kb();
                eprintln!(
                    "{aussen} x {innen}: {} Anweisungsbytes -> {} Platzierungen, \
                     {} Spiegel, VmHWM {vorher} -> {nachher} KB (+{} KB), {dauer:?}",
                    bytes,
                    scan.images.len(),
                    scan.marked.len(),
                    nachher.saturating_sub(vorher)
                );
            }
            Err(e) => eprintln!("{aussen} x {innen}: {} Bytes -> abgelehnt: {e}", bytes),
        }
    }
}
