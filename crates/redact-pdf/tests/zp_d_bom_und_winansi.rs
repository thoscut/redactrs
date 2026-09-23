//! Spur A, Runde 2, Prüfer D-3/D-4 (Register #97): zwei Kodierungen, die
//! das Orakel nicht las.
//!
//! * **UTF-8 mit BOM** (`EF BB BF`, PDF 2.0, 7.9.2.2): eine Zeichenkette
//!   `(\357\273\277Gr\303\274\303\237e)` las sich als PDFDocEncoding, also
//!   als „ï»¿GrÃ¼ÃŸe“.
//! * **WinAnsi in einem Inhaltsstrom**: `(Betrag 5 \200) Tj` mit einer
//!   Standardschrift heißt „Betrag 5 €“; die Verkettung las PDFDocEncoding
//!   („Betrag 5 •“), und die Bytesuche kannte keine WinAnsi-Form des
//!   Begriffs.
//!
//! Beides trifft vor allem die Altrevision, die nur noch die Rohsicht liest;
//! die Prüflinge legen das Geheimnis deshalb dorthin.

use std::io::Write;

use redact_pdf::leaks_many_within;

const BUDGET: u64 = 64 * 1024 * 1024;

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("packbar");
    e.finish().expect("packbar")
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
        format!("trailer\n<< /Size 8 /Root 1 0 R /Info 7 0 R{prev} >>\nstartxref\n{xref}\n%%EOF\n")
            .as_bytes(),
    );
    xref
}

fn strom(daten: &[u8], gepackt: bool) -> Vec<u8> {
    let (daten, filter) = if gepackt {
        (zlib(daten), "/Filter /FlateDecode ")
    } else {
        (daten.to_vec(), "")
    };
    let mut out = format!("<< {filter}/Length {} >>\nstream\n", daten.len()).into_bytes();
    out.extend_from_slice(&daten);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Revision 1 trägt `inhalt` (Objekt 6) und `info` (Objekt 7); Revision 2
/// ersetzt beide durch Harmloses. Nur die Rohsicht erreicht das Alte.
fn altrevision(inhalt: Vec<u8>, info: &[u8]) -> Vec<u8> {
    let mut out = b"%PDF-2.0\n".to_vec();
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
                5,
                b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                  /Encoding /WinAnsiEncoding >>"
                    .to_vec(),
            ),
            (6, inhalt),
            (7, info.to_vec()),
        ],
    );
    let prev = querverweis(&mut out, &offsets, None);
    let ersatz = objekte(
        &mut out,
        &[
            (6, strom(b"BT /F1 12 Tf 72 700 Td (XXXX) Tj ET", false)),
            (7, b"<< /Title (Ersetzt) >>".to_vec()),
        ],
    );
    querverweis(&mut out, &ersatz, Some(prev));
    out
}

fn fuellung() -> String {
    "BT /F1 12 Tf 72 720 Td (Fuelltext Fuelltext) Tj ET\n".repeat(64)
}

fn fund(pdf: &[u8], geheim: &str) -> Vec<String> {
    let check = leaks_many_within(pdf, &[geheim], BUDGET);
    assert!(check.unchecked.is_empty(), "{:?}", check.unchecked);
    check.findings[0].clone()
}

/// UTF-8 mit BOM in einer Zeichenkette der Altrevision, oktal maskiert.
#[test]
fn utf8_mit_bom_wird_gelesen() {
    let pdf = altrevision(
        strom(b"BT /F1 12 Tf 72 700 Td (Hallo) Tj ET", false),
        b"<< /Title (\\357\\273\\277Gr\\303\\274\\303\\237e an Max) >>",
    );
    let fund = fund(&pdf, "Grüße an Max");
    assert!(
        fund.iter().any(|f| f.contains("Rohdatei")),
        "UTF-8 mit BOM nicht gelesen: {fund:?}"
    );
}

/// WinAnsi, oktal maskiert, in einem gepackten Inhaltsstrom der Altrevision:
/// nur die Verkettung kann es lesen.
#[test]
fn winansi_im_inhaltsstrom_wird_gelesen() {
    let mut inhalt = fuellung();
    inhalt.push_str("BT /F1 12 Tf 72 700 Td (Betrag 5 \\200) Tj ET\n");
    let pdf = altrevision(strom(inhalt.as_bytes(), true), b"<< /Title (Hallo) >>");
    let fund = fund(&pdf, "Betrag 5 €");
    assert!(
        fund.iter().any(|f| f.contains("(WinAnsi)")),
        "WinAnsi in der Verkettung nicht gelesen: {fund:?}"
    );
}

/// WinAnsi als rohes Byte, außerhalb jeder Zeichenkette (ein Kommentar im
/// ungepackten Strom): nur die Bytesuche kann es finden.
#[test]
fn winansi_als_byte_wird_gefunden() {
    let mut inhalt = b"% Betrag 5 ".to_vec();
    inhalt.push(0x80);
    inhalt.extend_from_slice(b"\nBT /F1 12 Tf 72 700 Td (Hallo) Tj ET");
    let pdf = altrevision(strom(&inhalt, false), b"<< /Title (Hallo) >>");
    let fund = fund(&pdf, "Betrag 5 €");
    assert!(
        fund.iter().any(|f| f.contains("[WinAnsi]")),
        "die WinAnsi-Form des Begriffs fehlt der Bytesuche: {fund:?}"
    );
}

/// Gegenprobe: ein Byte, das WinAnsi und PDFDocEncoding gleich lesen, gibt
/// keine zweite Lesart — und ein Begriff, den WinAnsi wie Latin-1 schreibt,
/// keine eigene WinAnsi-Form.
#[test]
fn gleiche_lesart_gibt_keine_zweite() {
    let mut inhalt = fuellung();
    inhalt.push_str("BT /F1 12 Tf 72 700 Td (M\\374ller) Tj ET\n");
    let pdf = altrevision(strom(inhalt.as_bytes(), true), b"<< /Title (Hallo) >>");
    let fund = fund(&pdf, "Müller");
    assert!(!fund.is_empty(), "Müller nicht gefunden");
    assert!(
        !fund.iter().any(|f| f.contains("WinAnsi")),
        "eine zweite Lesart ohne Unterschied: {fund:?}"
    );
}
