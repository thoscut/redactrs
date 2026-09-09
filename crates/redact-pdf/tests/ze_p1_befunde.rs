//! Gegenprüfung P1: die beiden Befunde, die der Umbau aus Commit `f982c12`
//! hinterlassen hat — als lauffähige Belege.
//!
//! Diese Tests sind **absichtlich `#[ignore]`**: sie schlagen fehl, solange
//! die Befunde offen sind, und sollen das Tor nicht rot machen. Lauf:
//! `cargo test -p redact-pdf --test ze_p1_befunde -- --ignored --nocapture`
//!
//! Sind die Befunde behoben, gehört `#[ignore]` weg — dann sind es die Tests,
//! die die Korrektur festhalten.

mod common;

use std::io::Write;

use lopdf::{dictionary, Document, Object, Stream};
use redact_pdf::filters::decoded_content_within;
use redact_pdf::{leaks, leaks_many_within};

const SECRET: &str = "GEHEIMNIS-DE89370400440532013000";

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).expect("deflate");
    e.finish().expect("deflate")
}

/// Ein Seiteninhalt, den Deflate wirklich packt — sonst stünde der Klartext
/// als „stored“ im Strom und jede Rohsicht fände ihn ohne Dekoder.
fn payload() -> Vec<u8> {
    let fuellung = "ABCABCABCABC ".repeat(200);
    format!("BT {fuellung} (Konto {SECRET}) Tj {fuellung} ET").into_bytes()
}

fn pdf_with(filter: Object, content: Vec<u8>) -> Vec<u8> {
    let mut d = common::page(&[]);
    d.doc.objects.insert(
        d.content_id,
        Object::Stream(
            Stream::new(dictionary! { "Filter" => filter }, content).with_compression(false),
        ),
    );
    d.finish()
}

// ---------------------------------------------------------------------------
// Befund P1-1
// ---------------------------------------------------------------------------

/// Der **entzifferbare Anfang** einer Filterkette wird weggeworfen, sobald ein
/// späteres Glied unbekannt ist.
///
/// `filters::decoded_content_within` gibt für die ganze Kette `Ok(None)`
/// zurück, sobald ein Filter unbekannt ist (`decode_one` → `Ok(None)` →
/// `return Ok(None)`). `audit_bytes::decode_stream` reicht das durch: der
/// Strom bekommt **gar keine** dekodierte Sicht. Bis `f982c12` hatte
/// `audit_bytes` einen eigenen Dekoder (`manual_decode`), der bei einem
/// unbekannten Filter **abbrach und das bis dahin Entpackte zurückgab** —
/// und genau darin fand das Orakel das Geheimnis.
///
/// Die Rohsicht deckt den Fall nicht ab: sie versucht nur zlib an den
/// Rohbytes, und die sind hier ASCII-Hex.
#[test]
#[ignore = "Befund P1-1 — offen"]
fn p1_1_kette_mit_unbekanntem_filter_verliert_die_dekodierten_sichten() {
    let content = common::ascii_hex_encode(&deflate(&payload()));
    let pdf = pdf_with(
        Object::Array(vec![
            "ASCIIHexDecode".into(),
            "FlateDecode".into(),
            "DCTDecode".into(),
        ]),
        content.clone(),
    );

    // Die unabhängige Referenz: der alte Dekoder von `audit_bytes`,
    // nachgebaut — bricht am unbekannten Filter ab und behält das Ergebnis.
    let alt = alter_manual_decode(&content, &["ASCIIHexDecode", "FlateDecode", "DCTDecode"]);
    assert!(
        alt.as_deref()
            .is_some_and(|d| String::from_utf8_lossy(d).contains(SECRET)),
        "die Referenz selbst findet nichts — dann taugt der Beleg nicht"
    );

    let hits = leaks(&pdf, SECRET);
    assert!(
        !hits.is_empty(),
        "das Orakel findet das Geheimnis nicht mehr, der alte Dekoder fand es: {hits:?}"
    );
}

/// `audit_bytes::manual_decode`, Stand `f982c12^` — Zeile für Zeile.
fn alter_manual_decode(content: &[u8], filters: &[&str]) -> Option<Vec<u8>> {
    let mut data = content.to_vec();
    let mut decoded_any = false;
    for filter in filters {
        let next = match *filter {
            "FlateDecode" | "Fl" => {
                let mut out = Vec::new();
                let _ = std::io::Read::read_to_end(
                    &mut flate2::read::ZlibDecoder::new(&data[..]),
                    &mut out,
                );
                (!out.is_empty()).then_some(out)
            }
            "ASCIIHexDecode" | "AHx" => {
                let mut nibbles = Vec::new();
                for &b in &data {
                    if b == b'>' {
                        break;
                    }
                    match b {
                        b'0'..=b'9' => nibbles.push(b - b'0'),
                        b'a'..=b'f' => nibbles.push(b - b'a' + 10),
                        b'A'..=b'F' => nibbles.push(b - b'A' + 10),
                        _ => {}
                    }
                }
                if nibbles.len() % 2 == 1 {
                    nibbles.push(0);
                }
                Some(nibbles.chunks(2).map(|c| (c[0] << 4) | c[1]).collect())
            }
            _ => None,
        };
        match next {
            Some(decoded) => {
                data = decoded;
                decoded_any = true;
            }
            None => break,
        }
    }
    decoded_any.then_some(data)
}

// ---------------------------------------------------------------------------
// Befund P1-2
// ---------------------------------------------------------------------------

/// `ascii85_decode_within` lehnt einen Strom ab, der **genau** so groß ist
/// wie die Grenze — sofern seine Länge kein Vielfaches von vier ist.
///
/// Die Prüfung `if out.len() + 4 > limit` steht vor jeder Fünfergruppe und
/// unterstellt, dass jede Gruppe vier Byte liefert. Die letzte,
/// angebrochene Gruppe liefert aber ein bis drei. Jeder andere Filter
/// (Flate, LZW, ASCIIHex, RunLength) nimmt exakt `limit` Byte an.
#[test]
#[ignore = "Befund P1-2 — offen"]
fn p1_2_ascii85_wird_beim_eigenen_umfang_abgelehnt() {
    let doc = Document::with_version("1.5");
    for n in 1..=12usize {
        let plain: Vec<u8> = (0..n as u8).map(|b| b.wrapping_add(b'A')).collect();
        let stream = Stream::new(
            dictionary! { "Filter" => "ASCII85Decode" },
            common::ascii85_encode(&plain),
        )
        .with_compression(false);
        assert_eq!(
            decoded_content_within(&doc, &stream, n).map(|o| o.map(|v| v.len())),
            Ok(Some(n)),
            "ASCII85: {n} Byte passen nicht in eine Grenze von {n} Byte"
        );
    }
}

/// Dieselbe Rechnung im Orakel: ein gewöhnlicher ASCII85-Strom landet in
/// `unchecked`, obwohl das Budget für ihn reicht — die Kommandozeile
/// antwortet darauf mit Rückgabewert 3 („nicht geprüft“) an harmlosem
/// Material.
///
/// Aufbau: ein erster Strom verbraucht das Budget bis auf genau `n` Byte,
/// der zweite ist ein ASCII85-Strom von genau `n` Byte (`n % 4 == 3`).
/// Der erste hängt hinter `RunLengthDecode`, damit die Vorprüfung des
/// Laders ihn nicht selbst auspackt und das Budget vorwegnimmt.
#[test]
#[ignore = "Befund P1-2 — offen"]
fn p1_2_ascii85_strom_wird_grundlos_uebersprungen() {
    const BUDGET: usize = 64 * 1024;
    const N: usize = 1023; // 1023 % 4 == 3

    let mut fuellung = Vec::new();
    while fuellung.len() < BUDGET - N {
        fuellung.extend_from_slice(b"% 0123456789 0123456789 0123456789\n");
    }
    fuellung.truncate(BUDGET - N);

    let mut klartext = format!("BT (Konto {SECRET}) Tj ET\n").into_bytes();
    while klartext.len() < N {
        klartext.push(b' ');
    }
    klartext.truncate(N);

    let mut d = common::page(&[]);
    d.doc.objects.insert(
        d.content_id,
        Object::Stream(
            Stream::new(
                dictionary! { "Filter" => Object::Array(vec![
                "RunLengthDecode".into(), "FlateDecode".into()]) },
                common::run_length_encode(&deflate(&fuellung)),
            )
            .with_compression(false),
        ),
    );
    let a85 = d.add(Object::Stream(
        Stream::new(
            dictionary! { "Filter" => "ASCII85Decode" },
            common::ascii85_encode(&klartext),
        )
        .with_compression(false),
    ));
    let pdf = d.finish();

    let result = leaks_many_within(&pdf, &[SECRET], BUDGET as u64);
    assert!(
        !result
            .unchecked
            .iter()
            .any(|u| u.contains(&format!("Objekt {} {}", a85.0, a85.1))),
        "ein {N}-Byte-Strom passt in die verbleibenden {N} Byte des Budgets: {:?}",
        result.unchecked
    );
}
