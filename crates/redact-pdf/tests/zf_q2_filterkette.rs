//! Gegenprüfung Q2: die Ausnahme für Bildfilter und der unbekannte Filtername.
//!
//! # Befund Q2-1 (Leck, still)
//!
//! `audit_bytes::decode_stream` gibt bei `applied == 0` **ohne jede Meldung**
//! `Ok(None)` zurück. Ob das erste Kettenglied ein Bildfilter ist (Absicht) oder
//! ein Filtername, den das Programm nicht kennt (dann soll es laut Modulkopf
//! und laut SECURITY.md „sehr wohl“ in `unchecked` stehen), wird dort **nicht**
//! unterschieden — [`filters::is_image_filter`] wird nur im `applied > 0`-Zweig
//! befragt.
//!
//! Folge: `/Filter [/Crypt /ASCII85Decode]` mit `/Name /Identity` — ein
//! normgerechter Durchreicher (PDF 32000-1, 7.4.10), den MuPDF anstandslos zum
//! Klartext auspackt — liefert **0 Fundstellen und 0 `unchecked`-Zeilen**, an
//! der Kommandozeile „nicht gefunden“ mit Rückgabewert 0. Dasselbe gilt für
//! jeden Phantasienamen an erster Stelle.
//!
//! Diese Tests halten den **gemessenen Zustand** fest, damit die Korrektur ihn
//! umdreht: die mit `BEFUND_Q2_1` markierten Zusicherungen müssen nach dem Fix
//! umgeschrieben werden (dann steht der Filtername in `unchecked`).

mod common;

use common::{ascii85_encode, page, SECRET};
use lopdf::{dictionary, Dictionary, Object, Stream};
use redact_pdf::{leaks_many_within, LeakCheck};

fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).expect("komprimierbar");
    e.finish().expect("komprimierbar")
}

/// Ein PDF, dessen Objekt 7 0 ein Strom mit diesem Dictionary ist (kein
/// Seiteninhalt — Sicht 7 trägt also nichts bei).
fn pdf(dict: Dictionary, raw: Vec<u8>) -> Vec<u8> {
    let mut d = page(&["harmlos"]);
    let id = d.add(Object::Stream(
        Stream::new(dict, raw).with_compression(false),
    ));
    assert_eq!(id, (7, 0));
    d.catalog_set("Q2Extra", Object::Reference(id));
    d.finish()
}

fn kette(filters: &[&str]) -> Object {
    Object::Array(
        filters
            .iter()
            .map(|f| Object::Name(f.as_bytes().to_vec()))
            .collect(),
    )
}

fn pruefe(pdf: &[u8]) -> LeakCheck {
    leaks_many_within(pdf, &[SECRET], u64::MAX)
}

fn nutzlast() -> Vec<u8> {
    format!("BT /F1 10 Tf 72 700 Td (Notiz zur IBAN {SECRET}) Tj ET").into_bytes()
}

// ---------------------------------------------------------------------------
// Die Ausnahme wirkt: ein Bildfilter erzeugt keine `unchecked`-Zeile
// ---------------------------------------------------------------------------

/// `[/ASCII85Decode /DCTDecode]` ist die gewöhnliche Ausgabe eines Distillers.
/// Der entzifferbare Anfang muss durchsucht werden, und es darf **keine**
/// `unchecked`-Zeile geben — sonst käme jede Datei mit einem Foto als
/// „unvollständig geprüft“ zurück.
#[test]
fn q2_bildfilter_am_kettenende_meldet_nichts_und_findet_den_anfang() {
    for bild in [
        "DCTDecode",
        "DCT",
        "JPXDecode",
        "CCITTFaxDecode",
        "CCF",
        "JBIG2Decode",
    ] {
        let check = pruefe(&pdf(
            dictionary! { "Filter" => kette(&["ASCII85Decode", bild]) },
            ascii85_encode(&nutzlast()),
        ));
        assert!(
            check.findings[0]
                .iter()
                .any(|m| m.contains(&format!("danach /{bild} unbekannt"))),
            "/{bild}: der entzifferbare Anfang muss durchsucht werden: {:#?}",
            check.findings[0]
        );
        assert_eq!(
            check.unchecked,
            Vec::<String>::new(),
            "/{bild} ist ein benannter blinder Fleck und darf keine \
             NICHT-GEPRÜFT-Zeile erzeugen"
        );
    }
}

/// Die Kehrseite: ein Filtername, den niemand kennt, **muss** gemeldet werden —
/// solange er nicht an erster Stelle steht (siehe Befund Q2-1).
#[test]
fn q2_unbekannter_filtername_an_zweiter_stelle_steht_in_unchecked() {
    let check = pruefe(&pdf(
        dictionary! { "Filter" => kette(&["ASCII85Decode", "Q2Phantasie"]) },
        ascii85_encode(&nutzlast()),
    ));
    assert!(
        check
            .unchecked
            .iter()
            .any(|m| m.contains("/Q2Phantasie ist hier kein bekannter Filter")),
        "unbekannter Filtername muss in unchecked stehen: {:#?}",
        check.unchecked
    );
    // `/RunLengthDecode` ist kein Bildfilter, sondern ein Filter, den dieses
    // Modul kennt — er darf nichts melden.
    let check = pruefe(&pdf(
        dictionary! { "Filter" => kette(&["ASCII85Decode", "RunLengthDecode"]) },
        ascii85_encode(&nutzlast()),
    ));
    assert_eq!(check.unchecked, Vec::<String>::new());
}

// ---------------------------------------------------------------------------
// Befund Q2-1
// ---------------------------------------------------------------------------

/// **BEFUND_Q2_1** — normgerechte Datei, jeder Leser sieht den Klartext, das
/// Orakel schweigt.
///
/// `/Filter [/Crypt /ASCII85Decode]`, `/DecodeParms [<</Name /Identity>> null]`.
/// `/Crypt` mit `/Identity` verändert die Daten nicht (PDF 32000-1, 7.4.10);
/// danach steht ASCII85 und darin der Klartext. Gegengemessen mit MuPDF
/// (`pymupdf`, `xref_stream(7)`): `b'Notiz zur IBAN DE89 3704 0044 0532 0130 00'`.
#[test]
fn q2_crypt_identity_an_erster_stelle_ist_ein_stilles_leck() {
    let check = pruefe(&pdf(
        dictionary! {
            "Filter" => kette(&["Crypt", "ASCII85Decode"]),
            "DecodeParms" => Object::Array(vec![
                Object::Dictionary(dictionary! {
                    "Type" => "CryptFilterDecodeParms",
                    "Name" => "Identity",
                }),
                Object::Null,
            ]),
        },
        ascii85_encode(&nutzlast()),
    ));
    assert_eq!(
        check.findings[0],
        Vec::<String>::new(),
        "gemessener Zustand: das Orakel findet nichts"
    );
    // BEFUND_Q2_1: hier muss nach der Korrektur eine Zeile über /Crypt stehen.
    assert_eq!(
        check.unchecked,
        Vec::<String>::new(),
        "gemessener Zustand: und es sagt auch nicht, dass es nichts gelesen hat"
    );
}

/// **BEFUND_Q2_1**, zweite Hälfte: derselbe Strom, nur der unbekannte Filter
/// wandert an die zweite Stelle — dann wird gemeldet. Die Meldung hängt also
/// allein an der Position, nicht am Filternamen.
#[test]
fn q2_dieselbe_kette_umgestellt_wird_gemeldet() {
    let raw = ascii85_encode(&nutzlast());
    let vorn = pruefe(&pdf(
        dictionary! { "Filter" => kette(&["Q2Phantasie", "ASCII85Decode"]) },
        raw.clone(),
    ));
    let hinten = pruefe(&pdf(
        dictionary! { "Filter" => kette(&["ASCII85Decode", "Q2Phantasie"]) },
        raw,
    ));
    assert!(
        vorn.unchecked.is_empty(),
        "gemessener Zustand: vorn schweigt"
    );
    assert!(
        vorn.findings[0].is_empty(),
        "gemessener Zustand: vorn findet nichts"
    );
    assert!(
        !hinten.unchecked.is_empty() && !hinten.findings[0].is_empty(),
        "hinten wird gefunden und gemeldet"
    );
}

/// Ein Bildfilter **allein** ist der benannte blinde Fleck: nichts gefunden,
/// nichts gemeldet. Das ist Absicht — und der Grund, warum die Korrektur von
/// Befund Q2-1 die Ausnahme nicht einfach streichen darf.
#[test]
fn q2_bildfilter_allein_bleibt_still() {
    let check = pruefe(&pdf(
        dictionary! { "Filter" => Object::Name(b"DCTDecode".to_vec()) },
        deflate(&nutzlast()),
    ));
    assert_eq!(check.unchecked, Vec::<String>::new());
}
