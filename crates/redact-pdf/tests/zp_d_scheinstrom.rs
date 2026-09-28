//! Spur A, Runde 2, Prüfer D-2 (Register #96): „stream“ als Wortteil öffnete
//! in der Rohsicht einen Scheinblock.
//!
//! Die Rohsicht (Sicht 2) findet Ströme an den Bytes `stream` … `endstream`
//! — unabhängig von der Querverweistabelle, und genau so liest sie die
//! Altrevisionen eines inkrementellen Updates, die sonst keine Sicht mehr
//! erreicht. Sie prüfte aber nicht, ob `stream` ein Schlüsselwort ist: ein
//! Titel „Protokoll Livestream“ oder eine Schrift `/BitstreamVeraSans` vor
//! einem Strom öffnete einen Block, der bis zum nächsten `endstream` reichte
//! — und den echten Strom darin verschluckte. Ein Geheimnis im Flate-Strom
//! einer Altrevision kam als „nicht gefunden“ zurück, Rückgabewert 0.
//!
//! Jetzt ist `stream` nur ein Schlüsselwort, wenn davor Leerraum oder ein
//! Trennzeichen steht und dahinter das Zeilenende (oder Leerraum davor).

use std::io::Write;

use redact_pdf::leaks;

const GEHEIM: &str = "GEHEIMNIS";

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("packbar");
    e.finish().expect("packbar")
}

fn strom(kopf: &str, daten: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {kopf}/Length {} >>\nstream\n", daten.len()).into_bytes();
    out.extend_from_slice(daten);
    out.extend_from_slice(b"\nendstream");
    out
}

fn objekte(out: &mut Vec<u8>, objs: &[(u32, Vec<u8>)]) -> Vec<(u32, usize)> {
    let mut offsets = Vec::new();
    for (id, body) in objs {
        offsets.push((*id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    offsets
}

fn querverweis(out: &mut Vec<u8>, offsets: &[(u32, usize)], prev: Option<usize>) -> usize {
    let xref = out.len();
    out.extend_from_slice(b"xref\n");
    if prev.is_none() {
        out.extend_from_slice(b"0 1\n0000000000 65535 f \n");
    }
    for (id, offset) in offsets {
        out.extend_from_slice(format!("{id} 1\n{offset:010} 00000 n \n").as_bytes());
    }
    let prev = prev.map_or(String::new(), |p| format!(" /Prev {p}"));
    out.extend_from_slice(
        format!("trailer\n<< /Size 7 /Root 1 0 R /Info 4 0 R{prev} >>\nstartxref\n{xref}\n%%EOF\n")
            .as_bytes(),
    );
    xref
}

/// Revision 1 trägt das Geheimnis im Flate-Strom 6, Revision 2 ersetzt ihn.
/// Vor dem Strom steht Objekt 4 mit `titel`, Objekt 5 ist die Schrift
/// `schrift`.
fn altrevision(titel: &str, schrift: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    // Lang und gleichförmig, damit Flate wirklich packt: ein kurzer Text
    // landete in einem ungepackten Block, und das Geheimnis stünde als
    // Klartext in der Datei — die Rohsicht fände es ohne jeden Strom.
    let mut inhalt = "BT /F1 12 Tf 72 720 Td (Fuelltext Fuelltext) Tj ET\n".repeat(64);
    inhalt.push_str(&format!("BT /F1 12 Tf 72 700 Td ({GEHEIM}) Tj ET"));
    let geheim = zlib(inhalt.as_bytes());
    let offsets = objekte(
        &mut out,
        &[
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
                  /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>"
                    .to_vec(),
            ),
            (
                4,
                format!("<< /Title ({titel}) /Producer (Test) >>").into_bytes(),
            ),
            (
                5,
                format!("<< /Type /Font /Subtype /Type1 /BaseFont /{schrift} >>").into_bytes(),
            ),
            (6, strom("/Filter /FlateDecode ", &geheim)),
        ],
    );
    let prev = querverweis(&mut out, &offsets, None);
    let ersatz = objekte(
        &mut out,
        &[(6, strom("", b"BT /F1 12 Tf 72 700 Td (XXXXXXXXX) Tj ET"))],
    );
    querverweis(&mut out, &ersatz, Some(prev));
    assert!(
        !out.windows(GEHEIM.len()).any(|w| w == GEHEIM.as_bytes()),
        "der Prüfling trägt das Geheimnis als Klartext"
    );
    out
}

#[test]
fn ein_wort_mit_stream_verschluckt_die_altrevision_nicht() {
    let pdf = altrevision("Protokoll Livestream 12.3.", "Helvetica");
    let found = leaks(&pdf, GEHEIM);
    assert!(
        !found.is_empty(),
        "das Geheimnis der Altrevision fehlt stumm"
    );
}

#[test]
fn ein_schriftname_mit_stream_verschluckt_die_altrevision_nicht() {
    let pdf = altrevision("Protokoll", "BitstreamVeraSans");
    let found = leaks(&pdf, GEHEIM);
    assert!(
        !found.is_empty(),
        "das Geheimnis der Altrevision fehlt stumm"
    );
}

/// Gegenprobe: ohne das Wort wird die Altrevision gefunden — vorher wie
/// nachher.
#[test]
fn ohne_das_wort_wird_die_altrevision_gefunden() {
    let pdf = altrevision("Protokoll Livesendung 12.3.", "Helvetica");
    let found = leaks(&pdf, GEHEIM);
    assert!(!found.is_empty(), "{found:?}");
}

/// Ein Wort auf `stream` am Zeilenende eines Literals: dahinter steht ein
/// Zeilenende wie hinter dem Schlüsselwort, davor aber ein Buchstabe.
#[test]
fn ein_wort_mit_stream_am_zeilenende_verschluckt_die_altrevision_nicht() {
    let pdf = altrevision("Protokoll Livestream\nTeil 2", "Helvetica");
    let found = leaks(&pdf, GEHEIM);
    assert!(
        !found.is_empty(),
        "das Geheimnis der Altrevision fehlt stumm"
    );
}

/// Das Wort `stream` für sich, mit Leerraum davor und Text dahinter: kein
/// Schlüsselwort, denn dahinter kommt kein Zeilenende.
#[test]
fn das_wort_stream_mitten_im_titel_verschluckt_die_altrevision_nicht() {
    let pdf = altrevision("Live stream heute", "Helvetica");
    let found = leaks(&pdf, GEHEIM);
    assert!(
        !found.is_empty(),
        "das Geheimnis der Altrevision fehlt stumm"
    );
}
