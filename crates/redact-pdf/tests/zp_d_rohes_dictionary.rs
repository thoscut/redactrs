//! Spur A, Runde 2, Prüfer D-1 (Register #95): die Rohsicht las das
//! Stream-Dictionary einer Altrevision schlecht.
//!
//! Die Altrevision eines inkrementellen Updates steht in keinem
//! Objektgraphen mehr; nur die Rohsicht (Sicht 2) liest sie, und sie liest
//! `/Filter` und `/DecodeParms` aus den Rohbytes. Bis zur Spur-A-Runde 2
//! trennte sie die Wörter dort nur an Leerraum:
//!
//! * `/Filter[/ASCII85Decode/FlateDecode]` — so schreibt es iText, so
//!   schreibt es `lopdf` — war **ein** Name, die Kette lief nicht;
//! * `/DecodeParms<</Predictor 12/Columns 8>>` war ein Prädiktor ohne Zahl,
//!   und `/DecodeParms 9 0 R` gar keiner — der Strom wurde entpackt, aber
//!   nicht rückgerechnet;
//! * hinter `stream \r\n` begann der Strom beim Leerzeichen, und Flate brach
//!   sofort ab.
//!
//! In allen Fällen kam das Geheimnis als „nicht gefunden“ zurück, und eine
//! Kette, die die Rohsicht gar nicht dekodieren konnte (`/FooDecode`), stand
//! in keiner Zeile. Jetzt steht sie in `unchecked` — wo keine Objektsicht
//! denselben Strom gelesen hat.

use std::io::Write;

use redact_pdf::leaks_many_within;

const GEHEIM: &str = "GEHEIMNIS";
const BUDGET: u64 = 64 * 1024 * 1024;
const SPALTEN: usize = 8;

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("packbar");
    e.finish().expect("packbar")
}

/// Lang und gleichförmig, damit Flate wirklich packt — das Geheimnis steht
/// dann in keinem Byte der Datei als Klartext.
fn inhalt() -> Vec<u8> {
    let mut text = "BT /F1 12 Tf 72 720 Td (Fuelltext Fuelltext) Tj ET\n".repeat(64);
    text.push_str(&format!("BT /F1 12 Tf 72 700 Td ({GEHEIM}) Tj ET\n"));
    text.into_bytes()
}

/// ASCII85 mit `~>` am Ende.
fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let mut group = [0u8; 4];
        group[..chunk.len()].copy_from_slice(chunk);
        let mut value = u32::from_be_bytes(group);
        if chunk.len() == 4 && value == 0 {
            out.push(b'z');
            continue;
        }
        let mut digits = [0u8; 5];
        for digit in digits.iter_mut().rev() {
            *digit = (value % 85) as u8 + b'!';
            value /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// PNG-Prädiktor „Up“ (Typ 2) über Zeilen zu [`SPALTEN`] Byte.
fn praediktor_up(data: &[u8]) -> Vec<u8> {
    let mut padded = data.to_vec();
    while !padded.len().is_multiple_of(SPALTEN) {
        padded.push(b' ');
    }
    let mut out = Vec::new();
    let mut vorige = vec![0u8; SPALTEN];
    for zeile in padded.chunks(SPALTEN) {
        out.push(2);
        out.extend(zeile.iter().zip(&vorige).map(|(a, b)| a.wrapping_sub(*b)));
        vorige = zeile.to_vec();
    }
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
        format!("trailer\n<< /Size 10 /Root 1 0 R{prev} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    xref
}

/// Ein Strom-Objekt, genau so geschrieben, wie es dasteht: `kopf` ist das
/// Dictionary, `nach_stream` das, was hinter dem Schlüsselwort kommt.
fn strom(kopf: &str, daten: &[u8], nach_stream: &[u8]) -> Vec<u8> {
    let mut out = kopf.as_bytes().to_vec();
    out.extend_from_slice(b"\nstream");
    out.extend_from_slice(nach_stream);
    out.extend_from_slice(daten);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Revision 1 trägt `alt` als Seiteninhalt (Objekt 6) und davor `dazu` als
/// weitere Objekte; Revision 2 ersetzt Objekt 6 durch einen harmlosen
/// Strom. Nur die Rohsicht erreicht `alt` noch.
fn altrevision(alt: Vec<u8>, dazu: &[(u32, Vec<u8>)]) -> Vec<u8> {
    altrevision_mit(alt, dazu, &[])
}

/// Wie [`altrevision`], dazu `spaeter` als weitere Objekte der Revision 2.
fn altrevision_mit(alt: Vec<u8>, dazu: &[(u32, Vec<u8>)], spaeter: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut objs = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
              /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>"
                .to_vec(),
        ),
        (
            5,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        ),
    ];
    // Vor dem Strom: so steht die Definition der Revision 1 links von ihm,
    // die der Revision 2 rechts — und die nächste muss gewählt werden, nicht
    // bloß die einzige auf einer Seite.
    objs.extend_from_slice(dazu);
    objs.push((6, alt));
    let offsets = objekte(&mut out, &objs);
    let prev = querverweis(&mut out, &offsets, None);
    let harmlos = b"BT /F1 12 Tf 72 700 Td (XXXXXXXXX) Tj ET";
    let mut neu = vec![(
        6,
        strom(&format!("<< /Length {} >>", harmlos.len()), harmlos, b"\n"),
    )];
    neu.extend_from_slice(spaeter);
    let ersatz = objekte(&mut out, &neu);
    querverweis(&mut out, &ersatz, Some(prev));
    assert!(
        !out.windows(GEHEIM.len()).any(|w| w == GEHEIM.as_bytes()),
        "der Prüfling trägt das Geheimnis als Klartext"
    );
    out
}

/// Gefunden — und sonst nichts offen: jede andere Zeile hieße, dass der
/// Prüfling etwas anderes prüft als gedacht.
fn gefunden(pdf: &[u8]) -> bool {
    let check = leaks_many_within(pdf, &[GEHEIM], BUDGET);
    assert!(check.unchecked.is_empty(), "{:?}", check.unchecked);
    !check.findings[0].is_empty()
}

#[test]
fn eine_kompakte_filterkette_wie_bei_itext_wird_entpackt() {
    let daten = ascii85(&zlib(&inhalt()));
    let pdf = altrevision(
        strom(
            &format!(
                "<</Filter[/ASCII85Decode/FlateDecode]/Length {}>>",
                daten.len()
            ),
            &daten,
            b"\n",
        ),
        &[],
    );
    assert!(gefunden(&pdf), "die kompakte Kette lief nicht");
}

#[test]
fn ein_kompaktes_decodeparms_rechnet_den_praediktor_zurueck() {
    let daten = zlib(&praediktor_up(&inhalt()));
    let pdf = altrevision(
        strom(
            &format!(
                "<</Filter/FlateDecode/DecodeParms<</Predictor 12/Columns {SPALTEN}>>/Length {}>>",
                daten.len()
            ),
            &daten,
            b"\n",
        ),
        &[],
    );
    assert!(gefunden(&pdf), "der Prädiktor wurde nicht zurückgerechnet");
}

#[test]
fn decodeparms_als_verweis_wird_aus_den_rohbytes_aufgeloest() {
    let daten = zlib(&praediktor_up(&inhalt()));
    let pdf = altrevision(
        strom(
            &format!(
                "<< /Filter /FlateDecode /DecodeParms 9 0 R /Length {} >>",
                daten.len()
            ),
            &daten,
            b"\n",
        ),
        &[(
            9,
            format!("<< /Predictor 12 /Columns {SPALTEN} >>").into_bytes(),
        )],
    );
    assert!(gefunden(&pdf), "der Verweis auf /DecodeParms blieb offen");
}

/// Gilt eine Nummer mehrmals, gilt die Definition nahe dem Strom: Revision 2
/// definiert Objekt 9 neu, mit anderen Spalten — die Altrevision meinte ihr
/// eigenes.
#[test]
fn die_definition_nahe_dem_strom_gilt() {
    let daten = zlib(&praediktor_up(&inhalt()));
    let pdf = altrevision_mit(
        strom(
            &format!(
                "<< /Filter /FlateDecode /DecodeParms 9 0 R /Length {} >>",
                daten.len()
            ),
            &daten,
            b"\n",
        ),
        &[(
            9,
            format!("<< /Predictor 12 /Columns {SPALTEN} >>").into_bytes(),
        )],
        &[(9, b"<< /Predictor 12 /Columns 3 >>".to_vec())],
    );
    assert!(gefunden(&pdf), "die spätere Definition hat gewonnen");
}

#[test]
fn leerzeichen_hinter_stream_gehoert_nicht_zum_strom() {
    let daten = zlib(&inhalt());
    let pdf = altrevision(
        strom(
            &format!("<< /Filter /FlateDecode /Length {} >>", daten.len()),
            &daten,
            b" \r\n",
        ),
        &[],
    );
    assert!(gefunden(&pdf), "der Strom hinter `stream \\r\\n` blieb zu");
}

/// Eine Kette, die die Rohsicht nicht dekodieren kann, steht in `unchecked`
/// — mit demselben Wortlaut wie in der Objektsicht.
#[test]
fn ein_unbekannter_filter_in_der_altrevision_wird_gemeldet() {
    let daten = zlib(&inhalt());
    let pdf = altrevision(
        strom(
            &format!("<</Filter/FooDecode/Length {}>>", daten.len()),
            &daten,
            b"\n",
        ),
        &[],
    );
    let check = leaks_many_within(&pdf, &[GEHEIM], BUDGET);
    assert_eq!(check.unchecked.len(), 1, "{:?}", check.unchecked);
    let zeile = &check.unchecked[0];
    assert!(
        zeile.starts_with("Rohdaten-Stream")
            && zeile.contains("(Objekt 6 0)")
            && zeile.contains("/FooDecode ist hier kein bekannter Filter"),
        "{zeile}"
    );
}

/// Ein Bildfilter am Kettenende ist der benannte blinde Fleck — auch in der
/// Rohsicht keine Zeile.
#[test]
fn ein_bildfilter_in_der_altrevision_bleibt_still() {
    let pdf = altrevision(
        strom(
            "<</Filter/DCTDecode/Length 5>>",
            b"\xff\xd8\xff\xd9 ",
            b"\n",
        ),
        &[],
    );
    let check = leaks_many_within(&pdf, &[GEHEIM], BUDGET);
    assert!(check.unchecked.is_empty(), "{:?}", check.unchecked);
}

/// Derselbe unbekannte Filter am **aktuellen** Objekt: die Objektsicht
/// meldet ihn, die Rohsicht nicht noch einmal.
#[test]
fn ein_unbekannter_filter_am_aktuellen_objekt_steht_einmal() {
    let mut out = b"%PDF-1.7\n".to_vec();
    let offsets = objekte(
        &mut out,
        &[
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 6 0 R >>".to_vec(),
            ),
            (6, strom("<</Filter/FooDecode/Length 5>>", b"Hallo", b"\n")),
        ],
    );
    querverweis(&mut out, &offsets, None);
    let check = leaks_many_within(&out, &[GEHEIM], BUDGET);
    let zeilen = check
        .unchecked
        .iter()
        .filter(|z| z.contains("/FooDecode ist hier kein bekannter Filter"))
        .count();
    assert_eq!(zeilen, 1, "{:?}", check.unchecked);
}
