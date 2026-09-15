//! Gegenprüfung R2 (nach Fix-Runde 6): ein `/Filter`-Wert, der **kein Name**
//! ist — und was das Orakel dazu sagt.
//!
//! Fix-Runde 6 hat die Lücke „unbekannter Filtername an erster Stelle“
//! geschlossen: `/Filter /FooDecode` erzeugt jetzt eine `unchecked`-Zeile
//! („gar nicht dekodiert — /FooDecode ist hier kein bekannter Filter …“).
//! Die Entscheidung fällt in [`redact_pdf::filters`]`::filter_names`, und die
//! sagt vorher schon etwas anderes:
//!
//! ```text
//! Object::Array(items) => items.iter().map(|item| {
//!         doc.dereference(item).ok().and_then(|(_, o)| o.as_name().ok()).map(<[u8]>::to_vec)
//!     }).collect(),        // Option<Vec<_>>: EIN Fehlschlag verwirft die GANZE Kette
//! _ => None,
//! ```
//!
//! `None` heißt für `decode_stream` „`names.is_empty()`“ — und das ist der
//! **eine** Weg, auf dem die Funktion ohne Sicht und ohne Grund
//! zurückkommt. Ein Glied, das kein Name ist (Verweis ins Leere, `null`,
//! Zahl, Zeichenkette), verwirft deshalb die ganze Kette **stumm** — auch
//! wenn die anderen Glieder tadellose, bekannte Filter sind.
//!
//! **BEFUND_R2_C — geschlossen (Fix-Runde 7).** `filter_names` verwirft die
//! Kette nicht mehr: jedes Glied, das sich nicht zu einem Namen auflösen
//! lässt, steht als **namenloses Glied** darin, und die Kette bleibt dort
//! stehen wie an jedem anderen unbekannten Filter. Aus
//! `/Filter [/LZWDecode null]` wird deshalb beides — die LZW-Sicht, die den
//! Klartext freilegt, **und** eine `unchecked`-Zeile über den Rest.

mod common;

use common::{lzw_encode, page, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_pdf::{leaks_many_within, LeakCheck};

/// Eigener ASCII85-Kodierer (PDF 32000-1, 7.4.3), ohne `z`-Kurzform.
fn a85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in data.chunks(4) {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        let mut v = u32::from_be_bytes(w);
        let mut g = [0u8; 5];
        for s in g.iter_mut().rev() {
            *s = b'!' + u8::try_from(v % 85).expect("< 85");
            v /= 85;
        }
        out.extend_from_slice(&g[..c.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

fn nutzlast() -> Vec<u8> {
    format!("Kontoauszug, IBAN {SECRET}, bitte vertraulich").into_bytes()
}

/// Objekt 7 0 trägt den Strom; `999 0 R` zeigt absichtlich ins Leere.
fn pdf(filter: Object, raw: Vec<u8>) -> (Vec<u8>, ObjectId) {
    let mut d = page(&["harmlos"]);
    let id = d.add(Object::Stream(
        Stream::new(dictionary! { "Filter" => filter }, raw).with_compression(false),
    ));
    d.catalog_set("R2Wert", Object::Reference(id));
    (d.finish(), id)
}

fn pruefe(pdf: &[u8]) -> LeakCheck {
    leaks_many_within(pdf, &[SECRET], u64::MAX)
}

fn name(n: &str) -> Object {
    Object::Name(n.as_bytes().to_vec())
}

// ---------------------------------------------------------------------------
// Der Maßstab: dieselbe Nutzlast, ein sauberer Filtername
// ---------------------------------------------------------------------------

/// Kalibrierung: LZW-gepackter Klartext unter `/LZWDecode` wird gefunden;
/// unter einem fremden Namen kommt eine `unchecked`-Zeile. Die Rohsicht
/// findet ihn in **keinem** der beiden Fälle — sie versucht nur zlib und
/// rohes Deflate, kein LZW. Das ist der Maßstab für BEFUND_R2_C.
#[test]
fn r2_lzw_gepackt_ist_der_massstab() {
    let (bytes, id) = pdf(name("LZWDecode"), lzw_encode(&nutzlast()));
    let check = pruefe(&bytes);
    assert!(
        check.findings[0]
            .iter()
            .any(|m| m.starts_with(&format!("Objekt {} {} <Stream, dekodiert", id.0, id.1))),
        "der Maßstab muss gefunden werden: {:#?}",
        check.findings[0]
    );
    assert!(check.unchecked.is_empty(), "{:#?}", check.unchecked);

    let (bytes, _) = pdf(name("R2Fremd"), lzw_encode(&nutzlast()));
    let check = pruefe(&bytes);
    assert!(check.findings[0].is_empty(), "{:#?}", check.findings[0]);
    assert!(
        check
            .unchecked
            .iter()
            .any(|m| m.contains("/R2Fremd ist hier kein bekannter Filter (Glied 1 von 1)")),
        "ein fremder Name wird gemeldet: {:#?}",
        check.unchecked
    );
}

// ---------------------------------------------------------------------------
// BEFUND_R2_C
// ---------------------------------------------------------------------------

/// **BEFUND_R2_C, geschlossen** — ein `/Filter`-Wert, der kein Name (oder
/// keine Liste von Namen) ist, machte das Orakel **stumm**: kein Fund, keine
/// `unchecked`-Zeile, Rückgabewert 0.
///
/// Sechs Formen, alle über derselben LZW- bzw. ASCII85-Nutzlast, die keine
/// Rohsicht lesen kann. In fünf davon steht neben dem unbrauchbaren Glied ein
/// **bekannter** Filter, der den Klartext freilegt:
///
/// * `[/LZWDecode 999 0 R]` — Verweis ins Leere (PDF 32000-1, 7.3.9: ein
///   Verweis auf ein nicht vorhandenes Objekt ist `null`, kein Fehler),
/// * `[/LZWDecode null]` — `null` ausgeschrieben,
/// * `[/ASCII85Decode null]`,
/// * `[/LZWDecode 42]` — Zahl statt Name,
/// * `[/LZWDecode (LZWDecode)]` — Zeichenkette statt Name,
/// * `999 0 R` allein — der Einzelwert als Verweis ins Leere.
///
/// Bis Fix-Runde 6 sammelte `filter_names` die Liste mit
/// `collect::<Option<Vec<_>>>()`; ein Glied, das kein Name ist, verwarf
/// deshalb die ganze Kette, und `decode_stream` sah `names.is_empty()` —
/// denselben Zustand wie bei einem Strom ganz ohne `/Filter`, über den es zu
/// Recht schweigt. Der Unterschied zur gemeldeten Form lag allein in der
/// **Art** des Wertes: `[/LZWDecode /R2Fremd]` ist derselbe Strom mit einem
/// fremden **Namen** und wurde gemeldet.
///
/// Seit Fix-Runde 7 steht jedes nicht auflösbare Glied als **namenloses
/// Glied** in der Kette. Damit gilt für alle sechs Formen dasselbe wie für
/// einen fremden Namen: die Kette läuft bis dorthin (die fünf ersten Formen
/// legen den Klartext frei — aus der stillen Lücke wird ein **Fund**), und
/// über den Rest steht eine `unchecked`-Zeile. Nur `999 0 R` allein hat kein
/// Glied vor sich: dort bleibt es bei den rohen Bytes plus Meldung.
#[test]
fn befund_r2_c_ein_filterwert_der_kein_name_ist_wird_gemeldet() {
    let lzw = lzw_encode(&nutzlast());
    // (Beschreibung, /Filter-Wert, Strominhalt, Sicht, die entsteht)
    let faelle: Vec<(&str, Object, Vec<u8>, Option<&str>)> = vec![
        (
            "[/LZWDecode 999 0 R]",
            Object::Array(vec![name("LZWDecode"), Object::Reference((999, 0))]),
            lzw.clone(),
            Some("dekodiert: LZWDecode — bis Filter 1 von 2"),
        ),
        (
            "[/LZWDecode null]",
            Object::Array(vec![name("LZWDecode"), Object::Null]),
            lzw.clone(),
            Some("dekodiert: LZWDecode — bis Filter 1 von 2"),
        ),
        (
            "[/ASCII85Decode null]",
            Object::Array(vec![name("ASCII85Decode"), Object::Null]),
            a85(&nutzlast()),
            Some("dekodiert: ASCII85Decode — bis Filter 1 von 2"),
        ),
        (
            "[/LZWDecode 42]",
            Object::Array(vec![name("LZWDecode"), Object::Integer(42)]),
            lzw.clone(),
            Some("dekodiert: LZWDecode — bis Filter 1 von 2"),
        ),
        (
            "[/LZWDecode (LZWDecode)]",
            Object::Array(vec![
                name("LZWDecode"),
                Object::string_literal("LZWDecode".to_string()),
            ]),
            lzw.clone(),
            Some("dekodiert: LZWDecode — bis Filter 1 von 2"),
        ),
        ("999 0 R", Object::Reference((999, 0)), lzw.clone(), None),
    ];

    for (wie, filter, raw, sicht) in faelle {
        let (bytes, id) = pdf(filter, raw);
        let check = pruefe(&bytes);
        match sicht {
            Some(beschriftung) => assert!(
                check.findings[0].iter().any(|m| m.starts_with(&format!(
                    "Objekt {} {} <Stream, {beschriftung}",
                    id.0, id.1
                ))),
                "{wie}: das bekannte Glied muss laufen und den Klartext freilegen: {:#?}",
                check.findings[0]
            ),
            None => assert!(
                check.findings[0].is_empty(),
                "{wie}: vor dem namenlosen Glied steht nichts, was entpacken könnte: {:#?}",
                check.findings[0]
            ),
        }
        assert!(
            check.unchecked.iter().any(|m| m.starts_with(&format!(
                "Objekt {} {} <Stream>:",
                id.0, id.1
            )) && m.contains("kein Filtername")),
            "{wie}: der Rest der Kette ist ungelesen und muss es sagen: {:#?}",
            check.unchecked
        );
        assert_eq!(check.unchecked_places, 1, "{wie}: {:#?}", check.unchecked);
    }

    // Gegenprobe: derselbe Strom, ein fremder **Name** statt des
    // unbrauchbaren Wertes — Wortlaut unverändert seit Fix-Runde 6.
    let (bytes, _) = pdf(Object::Array(vec![name("LZWDecode"), name("R2Fremd")]), lzw);
    let check = pruefe(&bytes);
    assert!(
        check
            .unchecked
            .iter()
            .any(|m| m.contains("nur bis Filter 1 von 2 dekodiert — /R2Fremd")),
        "der Wortlaut für einen Namen bleibt: {:#?}",
        check.unchecked
    );
}

/// Die Gegenrichtung zu BEFUND_R2_C: `/Filter null` **direkt** am Strom ist
/// kein namenloses Glied, sondern gar kein Filter.
///
/// PDF 32000-1, 7.3.9: ein Dictionary-Eintrag mit dem Wert `null` ist wie ein
/// fehlender Eintrag. Jeder Leser sieht hier denselben ungefilterten Strom,
/// es gibt nichts, worüber die Leser auseinanderliefen — und die Rohbytes
/// sind vollständig durchsucht. Eine Meldung wäre ein Fehlalarm.
///
/// Der Unterschied zum Verweis ins Leere (`999 0 R`, oben): dort ist aus
/// **dieser** Datei nicht zu erfahren, was an der Stelle steht; ein Leser mit
/// einer anderen Querverweistabelle — eine ältere Revision, eine
/// Wiederherstellung — kann dort sehr wohl einen Filternamen finden.
#[test]
fn r2_filter_null_am_strom_ist_kein_filter() {
    let (bytes, id) = pdf(Object::Null, b"IBAN steht hier im Klartext".to_vec());
    let check = pruefe(&bytes);
    assert!(check.unchecked.is_empty(), "{:#?}", check.unchecked);
    assert_eq!(check.unchecked_places, 0);

    // Und der Beleg, dass die Rohsicht wirklich liest: derselbe Aufbau mit
    // dem Geheimnis im Strom.
    let (bytes, id2) = pdf(
        Object::Null,
        format!("Kontoauszug, IBAN {SECRET}").into_bytes(),
    );
    assert_eq!(id, id2);
    let check = pruefe(&bytes);
    assert!(
        check.findings[0]
            .iter()
            .any(|m| m.starts_with(&format!("Objekt {} {} <Stream, roh>", id.0, id.1))),
        "{:#?}",
        check.findings[0]
    );
    assert!(check.unchecked.is_empty(), "{:#?}", check.unchecked);
}

/// Schreibt die Datei zu BEFUND_R2_C nach `ZG_R2_WERT_PDF`, damit der Befund
/// an der **Kommandozeile** vorgeführt werden kann (erwartet: „der Suchbegriff
/// steht nicht mehr in der Datei“, Rückgabewert 0).
#[test]
#[ignore = "schreibt nur eine Beispieldatei"]
fn r2_schreibe_beispieldatei() {
    let Ok(pfad) = std::env::var("ZG_R2_WERT_PDF") else {
        return;
    };
    let (bytes, _) = pdf(
        Object::Array(vec![name("LZWDecode"), Object::Null]),
        lzw_encode(&nutzlast()),
    );
    std::fs::write(&pfad, &bytes).expect("schreibbar");
    eprintln!("geschrieben: {pfad} ({} Byte)", bytes.len());
}
