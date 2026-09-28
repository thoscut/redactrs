//! Gegenprüfung (Linse: die Korrektur **verschiebt** den Befund) zur
//! Nachbesserung von Befund ZI-A1 — `ObjectSource` / `LeakSite::document_object`.
//!
//! ## Was die Nachbesserung zusichert
//!
//! * `ObjectSource::Document` — „`lopdf` zählt das Objekt so, und der Fund
//!   steht in genau diesem Objekt. **Nur hier** darf ein Aufrufer mit der Id
//!   ins geladene Dokument greifen.“
//! * `LeakSite::object`, zu Sicht 4 — „(Quelle ist trotzdem
//!   `ObjectSource::Document`: das enthaltene Objekt steht mit dieser Id im
//!   geladenen Dokument.)“
//! * `LeakSite::document_object` — „`site.object` allein würde ihr eine
//!   **Altrevision** als geschwärztes, gewolltes Objekt verkaufen“, also:
//!   `document_object()` tut das nicht.
//! * Vertrag `tests/zh2_a_ort_herkunft.rs::zh2_a3` — „nennt ein Ort eine Id
//!   aus dem geladenen Dokument (`document_object()`), dann trägt dieses
//!   Objekt den Fund — **ohne Ausnahme**.“
//!
//! ## Der Einwand
//!
//! Der Befund ist nicht geschlossen, sondern **verschoben**: von Sicht 2, wo
//! die Id jetzt ehrlich als `RawHeader` etikettiert ist, auf **Sicht 4**, wo
//! dieselbe Verwechslung das **starke** Etikett `Document` trägt.
//!
//! Der Grund steht in `lopdf` 0.42 (`reader.rs::load_objects_raw`): die
//! Objekte **in** einem `/ObjStm` werden nur mit
//! `objects.entry(id).or_insert(…)` eingefügt — im Quelltext kommentiert mit
//! „Only add entries, but never replace entries“. Redefiniert eine **neuere**
//! Revision dieselbe Nummer direkt, gewinnt sie; die Fassung aus dem
//! Objekt-Strom der alten Revision landet **nicht** im geladenen Dokument.
//! Der Container-Strom bleibt aber ein gewöhnliches Objekt des geladenen
//! Dokuments, und `audit_bytes::scan_stream` packt ihn aus und meldet
//! **jedes** darin enthaltene Objekt mit `with_object(…)`, also als
//! `ObjectSource::Document` — ohne das geladene Dokument zu fragen. Genau der
//! Schritt, den die Rohsicht jetzt nicht mehr tun darf.
//!
//! Damit gilt wieder, wogegen der maschinenlesbare Ort eingeführt wurde, nur
//! diesmal hinter dem starken Etikett: `document_object()` nennt eine Nummer,
//! unter der im geladenen Dokument der **Ersatztext** steht.
//!
//! Zu haben wäre hier sogar eine **richtige** Dokument-Id: der Container
//! trägt den Klartext im geladenen Dokument wirklich (`lopdf` hält den
//! Objekt-Strom nach dem Laden entpackt). Die Auskunft muss also nicht
//! verschwinden — nur ihr Etikett muss stimmen.
//!
//! ## Warum das eine gewöhnliche Datei ist
//!
//! Objekt-Ströme sind ab PDF 1.5 der Normalfall (Wörterbücher,
//! Formularfelder, Strukturbaum liegen dort), und jedes „Speichern“ eines
//! Betrachters schreibt ein inkrementelles Update. Wer ein Feld schwärzt, das
//! in einem Objekt-Strom liegt, und inkrementell speichert, hat genau diese
//! Datei.
//!
//! ## Was hier geprüft wird
//!
//! * `zj_a1` — die **Vorbedingungen** des Materials: das Orakel findet den
//!   Klartext in Sicht 4 (richtig, soll so bleiben), im geladenen Dokument
//!   trägt die Nummer des enthaltenen Objekts den Ersatztext, und der
//!   Container trägt den Klartext. Alle drei bleiben nach jeder Nachbesserung
//!   wahr; das heutige Etikett wird nur protokolliert, nicht zugesichert.
//! * `zj_a2` — **der Vertrag von `zh2_a3`**, an dieser Datei. Heute rot,
//!   deshalb `#[ignore]` mit dem Befund im Grund: der geteilte Baum soll grün
//!   bleiben, der Befund aber sichtbar (`-- --ignored`).
//! * `zj_a3` — **Gegenprobe** ohne die angehängte Revision: dort hält derselbe
//!   Vertrag. `zj_a2` misst also die Revision und nicht sich selbst.

mod common;

use lopdf::{Document, Object};
use redact_pdf::audit_bytes::{LeakSite, LeakView};
use redact_pdf::leaks_many_within;

use common::SECRET;

/// Der geschwärzte Ersatztext: gleiche Länge, kein Klartext.
const ERSATZ: &str = "XXXX XXXX XXXX XXXX XXXX XX";

// ---------------------------------------------------------------------------
// Maßstab: das geladene Dokument, nicht der Satz des Orakels
// ---------------------------------------------------------------------------

fn enthaelt(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Steht der Text in diesem Objekt oder irgendwo darunter?
fn objekt_traegt(object: &Object, needle: &[u8]) -> bool {
    match object {
        Object::String(raw, _) => enthaelt(raw, needle),
        Object::Name(name) => enthaelt(name, needle),
        Object::Array(items) => items.iter().any(|i| objekt_traegt(i, needle)),
        Object::Dictionary(dict) => dict.iter().any(|(_, v)| objekt_traegt(v, needle)),
        Object::Stream(stream) => {
            enthaelt(&stream.content, needle)
                || stream
                    .decompressed_content()
                    .is_ok_and(|data| enthaelt(&data, needle))
                || stream.dict.iter().any(|(_, v)| objekt_traegt(v, needle))
        }
        _ => false,
    }
}

/// Trägt das Objekt mit dieser Id im **geladenen** Dokument den Text?
/// `false` heißt auch: es gibt gar keines mit dieser Id.
fn traegt(doc: &Document, id: (u32, u16), needle: &str) -> bool {
    doc.get_object(id)
        .is_ok_and(|object| objekt_traegt(object, needle.as_bytes()))
}

/// Satz und Ort gepaart, für den ersten Suchbegriff.
fn funde(bytes: &[u8]) -> Vec<(String, LeakSite)> {
    let check = leaks_many_within(bytes, &[SECRET], u64::MAX);
    assert_eq!(check.sites.len(), check.findings.len());
    assert_eq!(check.findings[0].len(), check.sites[0].len());
    check.findings[0]
        .iter()
        .cloned()
        .zip(check.sites[0].iter().copied())
        .collect()
}

// ---------------------------------------------------------------------------
// Material: Objekt-Strom in der Altrevision, dieselbe Nummer neu geschwärzt
// ---------------------------------------------------------------------------

fn last_startxref(bytes: &[u8]) -> usize {
    let key = b"startxref";
    let pos = bytes
        .windows(key.len())
        .enumerate()
        .rfind(|(_, w)| *w == key)
        .map(|(i, _)| i)
        .expect("startxref");
    let digits: String = bytes[pos + key.len()..]
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|&b| b as char)
        .collect();
    digits.parse().expect("Offset")
}

/// Die Id des Objekts **im** Objekt-Strom (`/Type /Vergessen`) und die des
/// Containers (`/Type /ObjStm`) — aus dem geladenen Dokument gelesen, nicht
/// geraten.
fn ids(doc: &Document) -> ((u32, u16), (u32, u16)) {
    let mut inner = None;
    let mut container = None;
    for (id, object) in &doc.objects {
        match object {
            Object::Dictionary(dict) if dict.has_type(b"Vergessen") => inner = Some(*id),
            Object::Stream(stream) if stream.dict.has_type(b"ObjStm") => container = Some(*id),
            _ => {}
        }
    }
    (
        inner.expect("das enthaltene Objekt"),
        container.expect("der Objekt-Strom"),
    )
}

/// Hängt eine Revision an, die `id` **direkt** (also nicht im Objekt-Strom)
/// mit dem geschwärzten Ersatztext redefiniert — ein gewöhnliches
/// inkrementelles Update mit klassischer xref-Sektion und `/Prev`.
fn redefiniere(mut out: Vec<u8>, id: (u32, u16), root: (u32, u16), size: u32) -> Vec<u8> {
    let prev = last_startxref(&out);
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let body = format!("<</Type/Vergessen/ActualText({ERSATZ})>>");
    let offset = out.len();
    out.extend_from_slice(format!("{} {} obj\n{body}\nendobj\n", id.0, id.1).as_bytes());

    let xref_offset = out.len();
    let xref = format!(
        "xref\n0 1\n0000000000 65535 f \n{} 1\n{offset:010} 00000 n \n\
         trailer\n<</Size {size} /Root {} {} R /Prev {prev}>>\nstartxref\n{xref_offset}\n%%EOF\n",
        id.0, root.0, root.1
    );
    out.extend_from_slice(xref.as_bytes());
    out
}

/// Die Datei des Einwands. Zurück kommen Bytes, die Id des enthaltenen Objekts
/// und die des Containers.
fn objstm_altrevision() -> (Vec<u8>, (u32, u16), (u32, u16)) {
    let zwei = common::object_stream(SECRET);
    let doc = Document::load_mem(&zwei).expect("die Basisdatei ist ladbar");
    let (inner, container) = ids(&doc);
    assert_ne!(
        inner, container,
        "Container und Inhalt müssen verschiedene Ids haben, sonst misst diese \
         Datei nichts"
    );
    let root = doc
        .trailer
        .get(b"Root")
        .expect("Root")
        .as_reference()
        .expect("Root ist ein Verweis");
    let drei = redefiniere(zwei, inner, root, doc.max_id + 2);
    (drei, inner, container)
}

// ---------------------------------------------------------------------------
// zj_a1 — die Vorbedingungen des Materials
// ---------------------------------------------------------------------------

/// **Das Material ist, was es behauptet.** Sicht 4 findet den Klartext im
/// Objekt-Strom der Altrevision (richtig), und im geladenen Dokument trägt
/// die von Sicht 4 genannte Nummer den **Ersatztext**.
#[test]
fn zj_a1_altrevision_im_objektstrom_die_vorbedingungen() {
    let (pdf, inner, container) = objstm_altrevision();
    let doc = Document::load_mem(&pdf).expect("die neue Revision ist ladbar");

    // Das geladene Dokument führt unter dieser Nummer das geschwärzte Objekt.
    assert!(
        traegt(&doc, inner, ERSATZ),
        "Objekt {inner:?} muss im geladenen Dokument der Ersatztext sein: {:?}",
        doc.get_object(inner)
    );
    assert!(
        !traegt(&doc, inner, SECRET),
        "Objekt {inner:?} darf im geladenen Dokument den Klartext nicht tragen"
    );
    // Der Container dagegen trägt ihn — es gäbe hier also eine richtige
    // Dokument-Id.
    assert!(
        traegt(&doc, container, SECRET),
        "der Container {container:?} trägt den Klartext im geladenen Dokument"
    );

    // Das Orakel ist nicht blind: Sicht 4 findet den Klartext. Das soll so
    // bleiben — der Einwand gilt dem Ort, nicht dem Fund.
    let funde = funde(&pdf);
    let sicht_vier: Vec<_> = funde
        .iter()
        .filter(|(_, s)| s.view == LeakView::ObjectStream)
        .collect();
    assert!(
        !sicht_vier.is_empty(),
        "Sicht 4 muss den Klartext im Objekt-Strom der Altrevision finden: \
         {funde:#?}"
    );
    // Was Sicht 4 heute als Ort liefert, wird hier nur **protokolliert**,
    // nicht zugesichert: welches Etikett nach der Nachbesserung richtig ist,
    // entscheidet `zj_a2`. Ein Test, der den heutigen Zustand festschreibt,
    // ginge bei der Korrektur rot und wäre kein Maßstab, sondern ein Abdruck.
    for (text, site) in &sicht_vier {
        eprintln!(
            "Sicht 4: object={:?} source={:?} document_object={:?} — {text}",
            site.object,
            site.object_source,
            site.document_object()
        );
    }
}

// ---------------------------------------------------------------------------
// zj_a2 — der Vertrag, wie die Nachbesserung ihn formuliert
// ---------------------------------------------------------------------------

/// **Der Vertrag von `zh2_a3`, an dieser Datei.** „Nennt ein Ort eine Id aus
/// dem geladenen Dokument (`document_object()`), dann trägt dieses Objekt den
/// Fund — ohne Ausnahme.“
///
/// Rot: Sicht 4 gibt die Nummer des Objekts aus dem Objekt-Strom der
/// **Altrevision** als Dokument-Id heraus; im geladenen Dokument steht dort
/// der Ersatztext.
#[test]
#[ignore = "offener Befund: Sicht 4 etikettiert die Id eines Objekts aus dem \
            Objekt-Strom einer Altrevision als ObjectSource::Document, ohne \
            das geladene Dokument zu fragen"]
fn zj_a2_vertrag_eine_dokument_id_nennt_den_traeger() {
    let (pdf, _, _) = objstm_altrevision();
    let doc = Document::load_mem(&pdf).expect("ladbar");
    let funde = funde(&pdf);
    assert!(!funde.is_empty(), "der Klartext muss gefunden werden");

    for (text, site) in &funde {
        eprintln!(
            "Sicht {} object={:?} source={:?} — {text}",
            site.view.number(),
            site.object,
            site.object_source
        );
    }

    let mut geprueft = 0usize;
    for (text, site) in &funde {
        if let Some(id) = site.document_object() {
            assert!(
                traegt(&doc, id, SECRET),
                "document_object() nennt Objekt {id:?}; im geladenen Dokument \
                 steht dort der Ersatztext, nicht der Fund — die Verwechslung \
                 von ZI-A1, nur hinter dem starken Etikett. Sicht {}, Satz: \
                 {text}",
                site.view.number()
            );
            geprueft += 1;
        }
    }
    assert!(geprueft > 0, "keine Dokument-Id geprüft: {funde:#?}");
}

// ---------------------------------------------------------------------------
// zj_a3 — Gegenprobe
// ---------------------------------------------------------------------------

/// **Gegenprobe.** Dieselbe Messung ohne die angehängte Revision: dort hält
/// der Vertrag. `zj_a2` misst also die Revision und nicht sich selbst.
#[test]
fn zj_a3_ohne_die_neue_revision_haelt_der_vertrag() {
    let pdf = common::object_stream(SECRET);
    let doc = Document::load_mem(&pdf).expect("ladbar");
    let funde = funde(&pdf);
    assert!(!funde.is_empty(), "der Klartext muss gefunden werden");

    let mut geprueft = 0usize;
    for (text, site) in &funde {
        if let Some(id) = site.document_object() {
            assert!(
                traegt(&doc, id, SECRET),
                "ohne Altrevision: document_object() nennt Objekt {id:?}, das \
                 den Fund nicht trägt — {text}"
            );
            geprueft += 1;
        }
    }
    assert!(geprueft > 0, "keine Dokument-Id geprüft: {funde:#?}");
}
