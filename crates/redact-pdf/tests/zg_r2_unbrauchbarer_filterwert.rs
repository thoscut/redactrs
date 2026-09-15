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
//! **BEFUND_R2_C**: `/Filter [/LZWDecode null]` über LZW-gepacktem Klartext
//! ist genau die Stelle, die Fix-Runde 6 schließen wollte — nur eine Ebene
//! früher. Kein Fund, keine Meldung, Rückgabewert 0. Derselbe Strom unter
//! `/Filter [/LZWDecode /R2Fremd]` wird gemeldet.

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

/// **BEFUND_R2_C** — ein `/Filter`-Wert, der kein Name (oder keine Liste von
/// Namen) ist, macht das Orakel **stumm**: kein Fund, keine `unchecked`-Zeile,
/// Rückgabewert 0.
///
/// Sechs Formen, alle über derselben LZW- bzw. ASCII85-Nutzlast, die keine
/// Rohsicht lesen kann. In fünf davon steht neben dem unbrauchbaren Glied ein
/// **bekannter** Filter, der den Klartext freilegen würde:
///
/// * `[/LZWDecode 999 0 R]` — Verweis ins Leere (PDF 32000-1, 7.3.9: ein
///   Verweis auf ein nicht vorhandenes Objekt ist `null`, kein Fehler),
/// * `[/LZWDecode null]` — `null` ausgeschrieben,
/// * `[/ASCII85Decode null]`,
/// * `[/LZWDecode 42]` — Zahl statt Name,
/// * `[/LZWDecode (LZWDecode)]` — Zeichenkette statt Name,
/// * `999 0 R` allein — der Einzelwert als Verweis ins Leere.
///
/// Der Unterschied zur gemeldeten Form liegt allein in der **Art** des
/// Wertes: `[/LZWDecode /R2Fremd]` ist derselbe Strom mit einem fremden
/// **Namen** und wird gemeldet. `filter_names` sammelt die Liste mit
/// `collect::<Option<Vec<_>>>()`; ein Glied, das kein Name ist, verwirft
/// deshalb die ganze Kette, und `decode_stream` sieht `names.is_empty()` —
/// denselben Zustand wie bei einem Strom ganz ohne `/Filter`, über den es zu
/// Recht schweigt.
///
/// Schwere: dieselbe wie bei Befund Q2-1/Q5, den Fix-Runde 6 geschlossen hat —
/// „nicht gefunden“ ohne Vorbehalt an einer Stelle, die niemand gelesen hat.
///
/// Vorschlag: `filter_names` gibt die Kette mit einem Platzhalter für jedes
/// nicht auflösbare Glied zurück (z. B. `b"?"`), statt sie zu verwerfen —
/// dann greift die Meldung der Fix-Runde 6 unverändert.
#[test]
fn befund_r2_c_ein_filterwert_der_kein_name_ist_macht_stumm() {
    let lzw = lzw_encode(&nutzlast());
    let faelle: Vec<(&str, Object, Vec<u8>)> = vec![
        (
            "[/LZWDecode 999 0 R]",
            Object::Array(vec![name("LZWDecode"), Object::Reference((999, 0))]),
            lzw.clone(),
        ),
        (
            "[/LZWDecode null]",
            Object::Array(vec![name("LZWDecode"), Object::Null]),
            lzw.clone(),
        ),
        (
            "[/ASCII85Decode null]",
            Object::Array(vec![name("ASCII85Decode"), Object::Null]),
            a85(&nutzlast()),
        ),
        (
            "[/LZWDecode 42]",
            Object::Array(vec![name("LZWDecode"), Object::Integer(42)]),
            lzw.clone(),
        ),
        (
            "[/LZWDecode (LZWDecode)]",
            Object::Array(vec![
                name("LZWDecode"),
                Object::string_literal("LZWDecode".to_string()),
            ]),
            lzw.clone(),
        ),
        ("999 0 R", Object::Reference((999, 0)), lzw.clone()),
    ];

    for (wie, filter, raw) in faelle {
        let (bytes, _) = pdf(filter, raw);
        let check = pruefe(&bytes);
        assert!(
            check.findings[0].is_empty(),
            "{wie}: unerwarteter Fund — dann ist der Aufbau schief: {:#?}",
            check.findings[0]
        );
        assert!(
            check.unchecked.is_empty(),
            "BEFUND_R2_C geschlossen für {wie}? Dann diese Zusicherung umdrehen: {:#?}",
            check.unchecked
        );
    }

    // Gegenprobe: derselbe Strom, ein fremder **Name** statt des
    // unbrauchbaren Wertes — gemeldet.
    let (bytes, _) = pdf(Object::Array(vec![name("LZWDecode"), name("R2Fremd")]), lzw);
    let check = pruefe(&bytes);
    assert!(
        check
            .unchecked
            .iter()
            .any(|m| m.contains("nur bis Filter 1 von 2 dekodiert — /R2Fremd")),
        "der Unterschied liegt allein in der Art des Wertes: {:#?}",
        check.unchecked
    );
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
