//! Gegenprüfung Fix-Runde 5 (Q1): `save_to_bytes` **beschneidet den Trailer**
//! auf `/Root`, `/Info`, `/Encrypt`, `/ID`, `/Size`.
//!
//! Zwei Fragen: nimmt die Beschneidung dem Ergebnis etwas, das ein Betrachter
//! braucht (XRef-Strom, `/ID`, `/Prev`, `/Info` direkt, kein `/ID`)? Und
//! greift sie überhaupt — verschwindet ein Objekt, das nur ein erfundener
//! Trailerschlüssel gehalten hat?
//!
//! Dass die Ausgabe **außerhalb** von `lopdf` öffnet, ist an denselben
//! Proben mit `pdftotext`/`pdftoppm` (poppler) und `redact-render` gemessen;
//! die Läufe stehen im Bericht.

mod common;

use common::{page, Doc, SECRET};
use lopdf::xref::XrefType;
use lopdf::{dictionary, Object};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata};

fn trailer_keys(bytes: &[u8]) -> Vec<String> {
    load_from_bytes(bytes)
        .expect("Ausgabe lädt")
        .trailer
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
        .collect()
}

/// Ein Objekt, das nur ein erfundener Trailerschlüssel gehalten hat, fällt —
/// samt Klartext. Das ist der Zweck der Beschneidung.
///
/// Mutationsnachweis: in `document.rs` die Filterzeile
/// `.filter(|key| !TRAILER_KEYS.contains(&key.as_slice()))` durch
/// `.filter(|_| false)` ersetzen — dann steht die IBAN wieder in der Ausgabe
/// und der Test ist rot.
#[test]
fn ein_erfundener_trailerschluessel_haelt_nichts_mehr_am_leben() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let versteck = d.add(Object::Dictionary(dictionary! {
        "Notiz" => Object::string_literal(format!("Konto {SECRET}")),
    }));
    d.doc.trailer.set("Zusatz", Object::Reference(versteck));
    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "Probe trägt das Geheimnis"
    );

    let mut doc = load_from_bytes(&bytes).expect("ladbar");
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");

    assert!(
        leaks(&out, SECRET).is_empty(),
        "das Versteck steht noch: {:?}",
        leaks(&out, SECRET)
    );
    assert!(
        !trailer_keys(&out).iter().any(|k| k == "Zusatz"),
        "der erfundene Schlüssel steht noch im Trailer"
    );
}

/// `/ID` überlebt (Betrachter und Signaturprüfer erwarten es), `/Prev` und
/// `/XRefStm` fallen — die Ausgabe ist genau eine Revision.
#[test]
fn id_bleibt_prev_und_xrefstm_fallen() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    d.doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::String(vec![0xAA; 16], lopdf::StringFormat::Hexadecimal),
            Object::String(vec![0xBB; 16], lopdf::StringFormat::Hexadecimal),
        ]),
    );
    // `/Prev` und `/XRefStm` werden am Dokument im Speicher gesetzt: als
    // Rohbytes wären es Zeiger auf eine Vorgeschichte, die diese Probe nicht
    // hat, und der Lader lehnte sie zu Recht ab. Beschnitten wird in
    // `save_to_bytes`, also genau hier.
    let bytes = d.finish();
    let mut doc = load_from_bytes(&bytes).expect("ladbar");
    doc.trailer.set("Prev", Object::Integer(1234));
    doc.trailer.set("XRefStm", Object::Integer(5678));
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");

    let keys = trailer_keys(&out);
    assert!(keys.iter().any(|k| k == "ID"), "/ID fehlt: {keys:?}");
    assert!(keys.iter().any(|k| k == "Root"), "/Root fehlt: {keys:?}");
    for weg in ["Prev", "XRefStm"] {
        assert!(!keys.iter().any(|k| k == weg), "{weg} steht noch: {keys:?}");
    }
    assert_eq!(load_from_bytes(&out).expect("lädt").get_pages().len(), 1);
}

/// Eine Datei **ohne** `/ID`: die Ausgabe bekommt keines erfunden und lädt
/// trotzdem.
#[test]
fn ohne_id_in_der_eingabe_laedt_die_ausgabe_weiter() {
    let d: Doc = page(&["Rechnung 4711"]);
    let bytes = d.finish();
    assert!(!trailer_keys(&bytes).iter().any(|k| k == "ID"));

    let mut doc = load_from_bytes(&bytes).expect("ladbar");
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    assert_eq!(load_from_bytes(&out).expect("lädt").get_pages().len(), 1);
}

/// `/Info` als **direktes** Dictionary im Trailer: es fällt wie ein Verweis,
/// und sein Klartext ist weg.
#[test]
fn ein_direktes_info_dictionary_faellt_mit() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    d.doc.trailer.set(
        "Info",
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal(format!("Kontoauszug {SECRET}")),
        }),
    );
    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "Probe trägt das Geheimnis"
    );

    let mut doc = load_from_bytes(&bytes).expect("ladbar");
    let report = strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");

    assert!(report.info_removed, "der Bericht muss /Info melden");
    assert!(leaks(&out, SECRET).is_empty(), "{:?}", leaks(&out, SECRET));
}

/// Eine Datei, die als **XRef-Strom** geschrieben wird: die Beschneidung nimmt
/// dem Trailer `/Type`, `/W`, `/Index`, `/Length` — `lopdf` setzt sie beim
/// Schreiben neu, und die Ausgabe lädt wieder.
///
/// (Der Strom trägt `/Type /XRef`; ohne diesen Schlüssel wäre er nach
/// PDF 32000-1, 7.5.8.2, Tabelle 17 kein Querverweisstrom.)
#[test]
fn eine_datei_mit_xref_strom_laedt_nach_der_beschneidung_wieder() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    d.doc.reference_table.cross_reference_type = XrefType::CrossReferenceStream;
    // So, wie `lopdf` es beim Laden einer solchen Datei hinterlässt: die
    // Schlüssel des Stroms stehen im Trailer.
    d.doc.trailer.set("Type", "XRef");
    d.doc
        .trailer
        .set("W", Object::Array(vec![1.into(), 4.into(), 2.into()]));
    d.doc
        .trailer
        .set("Index", Object::Array(vec![0.into(), 9.into()]));
    d.doc.trailer.set("Length", Object::Integer(42));
    d.doc.trailer.set("Filter", "FlateDecode");
    let bytes = d.finish();

    let mut doc = load_from_bytes(&bytes).expect("ladbar");
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");

    let wieder = load_from_bytes(&out).expect("Ausgabe mit XRef-Strom lädt");
    assert_eq!(wieder.get_pages().len(), 1, "Seitenzahl");
    assert_eq!(
        wieder.trailer.get(b"Type").and_then(Object::as_name).ok(),
        Some(b"XRef".as_slice()),
        "der XRef-Strom der Ausgabe braucht /Type /XRef: {:?}",
        trailer_keys(&out)
    );
}
