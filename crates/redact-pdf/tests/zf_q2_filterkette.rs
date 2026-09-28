//! Gegenprüfung Q2: die Ausnahme für Bildfilter und der unbekannte Filtername.
//!
//! # Befund Q2-1 (Leck, still) — behoben in Fix-Runde 6
//!
//! `audit_bytes::decode_stream` gab bei `applied == 0` **ohne jede Meldung**
//! `Ok(None)` zurück. Ob das erste Kettenglied ein Bildfilter war (Absicht) oder
//! ein Filtername, den das Programm nicht kennt (dann soll es laut Modulkopf
//! und laut SECURITY.md „sehr wohl“ in `unchecked` stehen), wurde dort **nicht**
//! unterschieden — [`filters::is_image_filter`] wurde nur im `applied > 0`-Zweig
//! befragt.
//!
//! Folge: `/Filter [/Crypt /ASCII85Decode]` mit `/Name /Identity` — ein
//! normgerechter Durchreicher (PDF 32000-1, 7.4.10), den MuPDF anstandslos zum
//! Klartext auspackt — lieferte **0 Fundstellen und 0 `unchecked`-Zeilen**, an
//! der Kommandozeile „nicht gefunden“ mit Rückgabewert 0. Dasselbe galt für
//! jeden Phantasienamen an erster Stelle; dieselbe Kette umgestellt
//! (`[/ASCII85Decode /Q2Phantasie]`) wurde gemeldet, Rückgabewert 3 — die
//! Meldung hing allein an der Position.
//!
//! Seit der Korrektur entscheidet allein der Filter, an dem die Kette stehen
//! blieb: ein Bildfilter schweigt (an **jeder** Stelle), jeder andere
//! unbekannte Name steht in `unchecked`. Die mit `BEFUND_Q2_1` markierten
//! Zusicherungen sind umgedreht; sie halten jetzt die Korrektur.

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

/// Die Kehrseite: ein Filtername, den niemand kennt, **muss** gemeldet werden.
/// (An erster Stelle ebenso — siehe Befund Q2-1 weiter unten.)
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
/// Orakel schwieg (bis Fix-Runde 6) und sagt es jetzt.
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
        "keine Sicht kommt an den Klartext heran (ASCII85 hinter /Crypt)"
    );
    // BEFUND_Q2_1, umgedreht: das Orakel sagt jetzt, dass es nichts gelesen hat.
    assert!(
        check.unchecked.iter().any(
            |m| m.contains("Objekt 7 0") && m.contains("/Crypt ist hier kein bekannter Filter")
        ),
        "„nicht gefunden“ ohne einen Blick in den Strom: {:#?}",
        check.unchecked
    );
}

/// **BEFUND_Q2_1**, zweite Hälfte: derselbe Strom, einmal mit dem unbekannten
/// Filter an erster und einmal an zweiter Stelle. Die Meldung hing allein an
/// der **Position**; jetzt hängt sie am **Filternamen**, und beide Fassungen
/// werden gemeldet. Gefunden wird der Klartext in keiner von beiden — die
/// Stelle ist unlesbar, und genau das steht jetzt in `unchecked`.
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
    // BEFUND_Q2_1, umgedreht: vorn wird jetzt genauso gemeldet wie hinten.
    assert!(
        vorn.unchecked
            .iter()
            .any(|m| m.contains("/Q2Phantasie ist hier kein bekannter Filter")),
        "vorn schweigt: {:#?}",
        vorn.unchecked
    );
    assert!(
        vorn.findings[0].is_empty(),
        "vorn kommt keine Sicht an den Klartext (ASCII85 ist nicht entpackt)"
    );
    assert!(
        !hinten.unchecked.is_empty() && !hinten.findings[0].is_empty(),
        "hinten wird gefunden und gemeldet"
    );
}

/// Ein Bildfilter an **erster** Stelle, mit einer Kette dahinter: **gemeldet**
/// (Befund R2-A, Fix-Runde 7 — diese Zusicherung ist umgedreht).
///
/// Ein Prüfer maß `[/DCTDecode /ASCII85Decode]`: 0 Funde, 0 `unchecked`,
/// Rückgabewert 0. Fix-Runde 6 erklärte das für richtig: hinter `/DCTDecode`
/// lägen Bilddaten, und was dahinter noch in der Kette stehe, ändere daran
/// nichts. Die Begründung trägt aber nur, wenn der Bildfilter das **letzte**
/// Glied ist. Die Reihenfolge im `/Filter`-Array ist die Dekodierreihenfolge
/// (PDF 32000-1, 7.4.1), und die Ausgabe eines Bildfilters sind Abtastwerte —
/// kein Erzeuger hängt dahinter noch einen Filter. Steht doch einer dahinter,
/// liegt dort kein Bild, sondern ein Glied, das niemand angewandt hat: bei
/// `[/DCTDecode /FlateDecode]` sogar eines, das dieses Programm **kennt**.
/// Schweigen verkaufte den Bildfilternamen als meldungsfreie Zone für
/// beliebige Bytes (`tests/zg_r2_bildfilter.rs`).
///
/// Die Distiller-Kette `[/ASCII85Decode /DCTDecode]` — der Fall, auf den die
/// Ausnahme zielt — schweigt weiter: [`q2_bildfilter_am_kettenende_meldet_nichts_und_findet_den_anfang`]
/// hält das für alle sechs Bildfilternamen fest.
#[test]
fn q2_bildfilter_an_erster_stelle_wird_mit_kette_dahinter_gemeldet() {
    let check = pruefe(&pdf(
        dictionary! { "Filter" => kette(&["DCTDecode", "ASCII85Decode"]) },
        ascii85_encode(&nutzlast()),
    ));
    assert!(
        check.unchecked.iter().any(|m| m
            .contains("/DCTDecode ist ein Bildfilter und wird nicht dekodiert")
            && m.contains("(Glied 1 von 2)")),
        "ein Bildfilter mit einem Glied dahinter wird gemeldet: {:#?}",
        check.unchecked
    );
    assert!(
        check.findings[0].is_empty(),
        "entpackt wurde nichts: {:#?}",
        check.findings[0]
    );
    // Die Gegenprobe an derselben Kette: derselbe Bau, nur mit einem Namen,
    // den niemand kennt, an erster Stelle — derselbe Satzbau, anderer Grund.
    let fremd = pruefe(&pdf(
        dictionary! { "Filter" => kette(&["Q2Phantasie", "ASCII85Decode"]) },
        ascii85_encode(&nutzlast()),
    ));
    assert!(
        fremd
            .unchecked
            .iter()
            .any(|m| m.contains("/Q2Phantasie ist hier kein bekannter Filter")),
        "{:#?}",
        fremd.unchecked
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
