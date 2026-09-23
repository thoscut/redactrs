//! Spur-A-Runde 1, Prüfer D: das Leck-Orakel an Filterketten, Altgenerationen
//! und Kodierungen des Suchbegriffs — mit **eigenem** Material, nicht mit den
//! Belegdateien der Probenliste.
//!
//! Drei Fragen:
//!
//! 1. **Probenliste** „Filterkette: unbekannter Filter an erster Stelle,
//!    `/Filter`-Wert kein Name, Kette weggeworfen“ — tritt die Klasse noch auf?
//!    ([`zo_d0_probenliste_filterkette_bleibt_gruen`], erwartet grün.)
//! 2. **Altgeneration.** Ein inkrementelles Update lässt die Vorgängerfassung
//!    eines Objekts in der Datei stehen; die neue Querverweistabelle nennt sie
//!    nicht mehr. Der Modulkopf von `audit_bytes` sagt zu, dass die Rohsichten
//!    „die Historie inkrementeller Updates“ lesen. Das gilt für Rohbytes und
//!    Flate — und sonst? ([`zo_d1_altgeneration_kontrollen`] grün,
//!    [`zo_d2_altgeneration_stroeme_stilles_leck`] und
//!    [`zo_d3_altgeneration_zeichenketten_stilles_leck`] **ABSICHTLICH ROT**.)
//! 3. **PDFDocEncoding.** Der Modulkopf liest Zeichenketten „als
//!    PDFDocEncoding, das im hier interessanten Bereich mit Latin-1
//!    zusammenfällt“. Im Bereich 0x80–0xA0 fällt es nicht zusammen: dort
//!    stehen `€` (0xA0), `–` (0x85), `‘’“”` (0x8F–0x8E), `…` (0x83), `•` (0x80).
//!    ([`zo_d4_pdfdoc_zeichen_stilles_leck`] **ABSICHTLICH ROT**.)
//!
//! Was „stilles Leck des Orakels“ hier heißt: der Suchbegriff steht in der
//! Datei, `findings` ist leer **und** `unchecked` ist leer — an der
//! Kommandozeile „nicht gefunden“, Rückgabewert 0.

mod common;

use std::io::Write;

use common::{lzw_encode, SECRET};
use redact_pdf::{leaks_many_within, LeakCheck};

// ---------------------------------------------------------------------------
// Rohe PDF-Dateien mit Querverweistabelle — und mit inkrementellem Update
// ---------------------------------------------------------------------------

/// Eine Datei aus fertigen Objektrümpfen. Rückgabe: Bytes und Offset der
/// Querverweistabelle (für `/Prev` einer angehängten Revision).
fn assemble(objects: &[(u32, Vec<u8>)]) -> (Vec<u8>, usize) {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (id, body) in objects {
        offsets.push((*id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let max = objects.iter().map(|(id, _)| *id).max().unwrap_or(0) + 1;
    out.extend_from_slice(format!("xref\n0 {max}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for id in 1..max {
        match offsets.iter().find(|(o, _)| *o == id) {
            Some((_, at)) => out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {max} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    (out, xref)
}

/// Hängt eine Revision an: neue Fassungen der genannten Objekte (Ids müssen
/// aufeinanderfolgen), eine Querverweistabelle nur für sie, `/Prev` auf die
/// vorige. Genau das schreibt jedes „Speichern“ eines Betrachters.
fn append_revision(mut file: Vec<u8>, prev_xref: usize, objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut offsets = Vec::new();
    for (id, body) in objects {
        offsets.push(file.len());
        file.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        file.extend_from_slice(body);
        file.extend_from_slice(b"\nendobj\n");
    }
    let first = objects[0].0;
    for (i, (id, _)) in objects.iter().enumerate() {
        assert_eq!(
            *id,
            first + i as u32,
            "Ids der Revision müssen aufeinanderfolgen"
        );
    }
    let xref = file.len();
    file.extend_from_slice(
        format!(
            "xref\n0 1\n0000000000 65535 f \n{first} {}\n",
            objects.len()
        )
        .as_bytes(),
    );
    for at in offsets {
        file.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes());
    }
    let size = first + objects.len() as u32;
    file.extend_from_slice(
        format!(
            "trailer\n<< /Size {size} /Root 1 0 R /Prev {prev_xref} >>\nstartxref\n{xref}\n%%EOF\n"
        )
        .as_bytes(),
    );
    file
}

fn stream_object(dict_entries: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} {dict_entries} >>\nstream\n", payload.len()).into_bytes();
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Gerüst: Katalog 1, Seitenbaum 2, Seite 3 mit Inhalt 4 und Anmerkung 5.
fn skeleton() -> Vec<(u32, Vec<u8>)> {
    vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R \
              /Annots [5 0 R] /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 \
              /BaseFont /Helvetica /Encoding /WinAnsiEncoding >> >> >> >>"
                .to_vec(),
        ),
    ]
}

fn content_with_secret() -> Vec<u8> {
    format!("BT /F1 10 Tf 72 700 Td (IBAN {SECRET}) Tj ET").into_bytes()
}

const HARMLOS_CONTENT: &[u8] = b"BT /F1 10 Tf 72 700 Td (geschwaerzt) Tj ET";
const HARMLOS_ANNOT: &[u8] =
    b"<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents (geschwaerzt) >>";

/// Revision 1 trägt das Geheimnis in Objekt 4 (Strom) und Objekt 5 (Anmerkung);
/// Revision 2 ersetzt beide durch harmlose Fassungen. Die alten Fassungen
/// stehen unverändert in der Datei, die neue Querverweistabelle nennt sie
/// nicht mehr.
fn with_old_generation(old_stream: Vec<u8>, old_annot: Vec<u8>) -> Vec<u8> {
    let mut objects = skeleton();
    objects.push((4, old_stream));
    objects.push((5, old_annot));
    let (file, xref) = assemble(&objects);
    append_revision(
        file,
        xref,
        &[
            (4, stream_object("", HARMLOS_CONTENT)),
            (5, HARMLOS_ANNOT.to_vec()),
        ],
    )
}

fn check(pdf: &[u8], needle: &str) -> LeakCheck {
    leaks_many_within(pdf, &[needle], u64::MAX)
}

/// Fund **oder** Meldung — alles andere ist die stille Entwarnung.
fn ist_still(check: &LeakCheck) -> bool {
    check.findings[0].is_empty() && check.unchecked.is_empty()
}

// ---------------------------------------------------------------------------
// Kodierer — nur für die Tests
// ---------------------------------------------------------------------------

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).expect("deflate");
    e.finish().expect("deflate")
}

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

fn run_length(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(128) {
        out.push(u8::try_from(chunk.len() - 1).expect("<= 127"));
        out.extend_from_slice(chunk);
    }
    out.push(128);
    out
}

fn hex(data: &[u8], line_len: Option<usize>) -> Vec<u8> {
    let mut out = String::new();
    for (i, b) in data.iter().enumerate() {
        if let Some(n) = line_len {
            if i > 0 && i % n == 0 {
                out.push('\n');
            }
        }
        out.push_str(&format!("{b:02X}"));
    }
    out.push('>');
    out.into_bytes()
}

/// PNG-Prädiktor „None“ zeilenweise: ein Filterbyte 0 vor jeder Zeile von
/// `columns` Byte — was `/Predictor 12 /Columns n` beim Dekodieren entfernt.
fn png_rows(data: &[u8], columns: usize) -> Vec<u8> {
    let mut rows = Vec::new();
    for chunk in data.chunks(columns) {
        rows.push(0u8);
        rows.extend_from_slice(chunk);
        // Letzte Zeile auffüllen, damit jede Zeile `columns` Byte hat.
        rows.resize(rows.len() + (columns - chunk.len()), b' ');
    }
    rows
}

/// TIFF-Prädiktor 2 (PDF 32000-1, 7.4.4.4): jedes Byte als Differenz zum
/// linken Nachbarn, zeilenweise.
fn tiff_rows(data: &[u8], columns: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(columns) {
        let mut prev = 0u8;
        for &b in chunk {
            out.push(b.wrapping_sub(prev));
            prev = b;
        }
    }
    out
}

fn octal_escaped(text: &str) -> String {
    text.bytes().map(|b| format!("\\{b:03o}")).collect()
}

fn xor(text: &str) -> Vec<u8> {
    text.bytes().map(|b| b ^ 0x5A).collect()
}

// ---------------------------------------------------------------------------
// 1. Probenliste: Filterkette
// ---------------------------------------------------------------------------

/// Eigener Lauf zur Probenlisten-Zeile. Jede Form eines unbrauchbaren
/// `/Filter`-Wertes muss entweder einen Fund oder eine `unchecked`-Zeile
/// ergeben; die Nutzlast ist so kodiert, dass keine Rohsicht sie liest.
#[test]
fn zo_d0_probenliste_filterkette_bleibt_gruen() {
    let plain = format!("BT (IBAN {SECRET}) Tj ET");
    // (Beschreibung, /Filter-Wert als Rohtext, Nutzlast, erwarteter Fund?, Wortlaut in unchecked)
    let faelle: Vec<(&str, &str, Vec<u8>, bool, &str)> = vec![
        (
            "fremd allein",
            "/ZoFremd",
            xor(&plain),
            false,
            "kein bekannter Filter (Glied 1 von 1)",
        ),
        (
            "fremd zuerst",
            "[/ZoFremd /FlateDecode]",
            xor(&plain),
            false,
            "Glied 1 von 2",
        ),
        (
            "fremd zuletzt",
            "[/FlateDecode /ZoFremd]",
            deflate(&xor(&plain)),
            false,
            "nur bis Filter 1 von 2",
        ),
        ("Zahl", "42", xor(&plain), false, "kein Filtername"),
        (
            "Zeichenkette",
            "(FlateDecode)",
            xor(&plain),
            false,
            "kein Filtername",
        ),
        (
            "Dictionary",
            "<< /Name /FlateDecode >>",
            xor(&plain),
            false,
            "kein Filtername",
        ),
        ("Boolean", "true", xor(&plain), false, "kein Filtername"),
        (
            "Verweis ins Leere",
            "999 0 R",
            xor(&plain),
            false,
            "kein Filtername",
        ),
        (
            "null hinter A85",
            "[/ASCII85Decode null]",
            a85(plain.as_bytes()),
            true,
            "kein Filtername",
        ),
        (
            "Verweis hinter A85",
            "[/ASCII85Decode 999 0 R]",
            a85(plain.as_bytes()),
            true,
            "kein Filtername",
        ),
        (
            "Zahl hinter LZW",
            "[/LZWDecode 42]",
            lzw_encode(plain.as_bytes()),
            true,
            "kein Filtername",
        ),
        (
            "null vor A85",
            "[null /ASCII85Decode]",
            a85(plain.as_bytes()),
            false,
            "Glied 1 von 2",
        ),
        (
            "Bildfilter mit Rest",
            "[/DCTDecode /FlateDecode]",
            xor(&plain),
            false,
            "Bildfilter",
        ),
    ];
    for (wie, filter, payload, fund, wortlaut) in faelle {
        let mut objects = skeleton();
        objects.push((4, stream_object("", HARMLOS_CONTENT)));
        objects.push((5, HARMLOS_ANNOT.to_vec()));
        objects.push((6, stream_object(&format!("/Filter {filter}"), &payload)));
        objects[0].1 = b"<< /Type /Catalog /Pages 2 0 R /ZoWert 6 0 R >>".to_vec();
        let (pdf, _) = assemble(&objects);
        let c = check(&pdf, SECRET);
        assert!(!ist_still(&c), "{wie} ({filter}): stille Entwarnung");
        assert_eq!(
            !c.findings[0].is_empty(),
            fund,
            "{wie} ({filter}): Fund erwartet = {fund}, Funde: {:#?}",
            c.findings[0]
        );
        assert!(
            c.unchecked
                .iter()
                .any(|m| m.starts_with("Objekt 6 0 <Stream>") && m.contains(wortlaut)),
            "{wie} ({filter}): unchecked ohne „{wortlaut}“: {:#?}",
            c.unchecked
        );
    }
}

// ---------------------------------------------------------------------------
// 2. Altgeneration
// ---------------------------------------------------------------------------

/// Kontrolle: was die Rohsichten an einer Altgeneration lesen — Rohbytes,
/// Flate, ASCIIHex am Stück; literal, hex am Stück, UTF-16BE.
#[test]
fn zo_d1_altgeneration_kontrollen() {
    let content = content_with_secret();
    // UTF-16BE **roh** in der Literal-Zeichenkette (BOM FE FF, dann die
    // Einheiten als Bytes) — so schreibt es ein Erzeuger, der nicht maskiert.
    let mut utf16_annot =
        b"<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents (\xFE\xFF".to_vec();
    utf16_annot.extend(SECRET.encode_utf16().flat_map(u16::to_be_bytes));
    utf16_annot.extend_from_slice(b") >>");
    let faelle: Vec<(&str, Vec<u8>, Vec<u8>)> = vec![
        (
            "Strom roh",
            stream_object("", &content),
            HARMLOS_ANNOT.to_vec(),
        ),
        (
            "Strom Flate",
            stream_object("/Filter /FlateDecode", &deflate(&content)),
            HARMLOS_ANNOT.to_vec(),
        ),
        (
            "Strom ASCIIHex am Stück",
            stream_object("/Filter /ASCIIHexDecode", &hex(&content, None)),
            HARMLOS_ANNOT.to_vec(),
        ),
        (
            "Zeichenkette literal",
            stream_object("", HARMLOS_CONTENT),
            format!(
                "<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents (IBAN {SECRET}) >>"
            )
            .into_bytes(),
        ),
        (
            "Zeichenkette hex am Stück",
            stream_object("", HARMLOS_CONTENT),
            format!(
                "<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents <{}> >>",
                String::from_utf8(hex(SECRET.as_bytes(), None))
                    .unwrap()
                    .trim_end_matches('>')
            )
            .into_bytes(),
        ),
        (
            "Zeichenkette UTF-16BE",
            stream_object("", HARMLOS_CONTENT),
            utf16_annot,
        ),
    ];
    for (wie, old_stream, old_annot) in faelle {
        let pdf = with_old_generation(old_stream, old_annot);
        let c = check(&pdf, SECRET);
        assert!(c.unchecked.is_empty(), "{wie}: {:#?}", c.unchecked);
        assert!(
            !c.findings[0].is_empty(),
            "{wie}: Altgeneration nicht gefunden"
        );
    }
}

/// **ABSICHTLICH ROT — BEFUND ZO-D2.** Ein Strom der Altgeneration unter
/// `/LZWDecode`, `/ASCII85Decode`, `/RunLengthDecode`, `/ASCIIHexDecode` mit
/// Zeilenumbrüchen oder `/FlateDecode` mit PNG-Prädiktor: die Rohsicht
/// (Sicht 2) versucht an jedem Block nur zlib und rohes Deflate; das
/// Objekt steht in keiner Querverweistabelle mehr und damit in keiner
/// Objektsicht. Kein Fund, keine `unchecked`-Zeile — Rückgabewert 0.
///
/// Warum das eine gewöhnliche Datei ist: jedes „Speichern“ eines Betrachters
/// schreibt ein inkrementelles Update; wer eine in einem anderen Werkzeug
/// geschwärzte Datei mit `--check-leaks` nachprüft, hat genau diese Datei.
/// LZW und ASCII85 sind die Filter der Distiller-Generation, ASCIIHex mit
/// Zeilenumbrüchen ist die Form, in der jeder ASCIIHex-Kodierer schreibt.
#[test]
#[ignore = "offen: Register #80 Altgeneration im Orakel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn zo_d2_altgeneration_stroeme_stilles_leck() {
    let content = content_with_secret();
    let faelle: Vec<(&str, Vec<u8>)> = vec![
        (
            "RunLength (Kontrolle, wird gefunden)",
            stream_object("/Filter /RunLengthDecode", &run_length(&content)),
        ),
        (
            "LZW",
            stream_object("/Filter /LZWDecode", &lzw_encode(&content)),
        ),
        (
            "ASCII85",
            stream_object("/Filter /ASCII85Decode", &a85(&content)),
        ),
        // `/RunLengthDecode` fehlt hier mit Absicht: ein wörtlicher Lauf trägt
        // den Klartext unverändert, und die Rohsicht liest ihn (geprüft, grün).
        (
            "ASCIIHex mit Zeilenumbruch",
            stream_object("/Filter /ASCIIHexDecode", &hex(&content, Some(16))),
        ),
        (
            "Flate mit PNG-Prädiktor",
            stream_object(
                "/Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 8 >>",
                &deflate(&png_rows(&content, 8)),
            ),
        ),
        (
            "ASCII85 vor Flate (Distiller)",
            stream_object(
                "/Filter [/ASCII85Decode /FlateDecode]",
                &a85(&deflate(&content)),
            ),
        ),
    ];
    let mut still = Vec::new();
    for (wie, old_stream) in faelle {
        let pdf = with_old_generation(old_stream, HARMLOS_ANNOT.to_vec());
        // Vorbedingung: in der aktuellen Revision (Objektsicht) fände das
        // Orakel denselben Strom — die Lücke ist die Altgeneration.
        let mut objects = skeleton();
        objects.push((4, pdf_stream_copy(&pdf)));
        objects.push((5, HARMLOS_ANNOT.to_vec()));
        let (aktuell, _) = assemble(&objects);
        let c_aktuell = check(&aktuell, SECRET);
        assert!(
            !c_aktuell.findings[0].is_empty(),
            "{wie}: Vorbedingung — als aktuelles Objekt wird der Strom gefunden: {:#?} / {:#?}",
            c_aktuell.findings[0],
            c_aktuell.unchecked
        );
        let c = check(&pdf, SECRET);
        if ist_still(&c) {
            still.push(wie);
        }
    }
    assert!(
        still.is_empty(),
        "stilles Leck des Orakels an einer Altgeneration: {still:?} — kein Fund, keine \
         NICHT-GEPRÜFT-Zeile"
    );
}

/// Objekt 4 der ersten Revision, Byte für Byte aus der Datei — damit die
/// Vorbedingung denselben Strom misst.
fn pdf_stream_copy(pdf: &[u8]) -> Vec<u8> {
    let start = pdf
        .windows(8)
        .position(|w| w == b"4 0 obj\n")
        .expect("Objekt 4")
        + 8;
    let end = start
        + pdf[start..]
            .windows(7)
            .position(|w| w == b"\nendobj")
            .expect("endobj");
    pdf[start..end].to_vec()
}

/// **ABSICHTLICH ROT — BEFUND ZO-D3.** Eine Zeichenkette der Altgeneration
/// mit oktalen Escapes (`\104\105…`, auch UTF-16BE so maskiert, wie pdfTeX
/// es schreibt), als Hex-String mit Leerraum
/// (`<4445 3839 …>`) oder mit Zeilenfortsetzung (`\` + Zeilenumbruch):
/// Sicht 1 vergleicht nur Bytes, Sicht 5 kennt das Objekt nicht mehr. Kein
/// Fund, keine Meldung.
///
/// Oktale Escapes schreibt jeder Erzeuger, der Bytes über 0x7E oder Klammern
/// maskiert; Hex-Strings mit Zeilenumbruch schreiben Erzeuger mit fester
/// Zeilenlänge (pdfTeX, Ghostscript).
#[test]
#[ignore = "offen: Register #80 Altgeneration im Orakel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn zo_d3_altgeneration_zeichenketten_stilles_leck() {
    let hex_ws: String = SECRET
        .as_bytes()
        .chunks(2)
        .map(|c| c.iter().map(|b| format!("{b:02X}")).collect::<String>())
        .collect::<Vec<_>>()
        .join(" ");
    let (a, b) = SECRET.split_at(10);
    let utf16_oktal: String = std::iter::once(0xFEu8)
        .chain(std::iter::once(0xFF))
        .chain(SECRET.encode_utf16().flat_map(u16::to_be_bytes))
        .map(|b| format!("\\{b:03o}"))
        .collect();
    let faelle: Vec<(&str, String)> = vec![
        (
            "UTF-16BE oktal maskiert (pdfTeX-Form)",
            format!("({utf16_oktal})"),
        ),
        ("oktal maskiert", format!("({})", octal_escaped(SECRET))),
        ("hex mit Leerraum", format!("<{hex_ws}>")),
        ("Zeilenfortsetzung", format!("({a}\\\n{b})")),
    ];
    let mut still = Vec::new();
    for (wie, contents) in faelle {
        let annot =
            format!("<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents {contents} >>");
        // Vorbedingung: als aktuelles Objekt liest Sicht 5 die Zeichenkette.
        let mut objects = skeleton();
        objects.push((4, stream_object("", HARMLOS_CONTENT)));
        objects.push((5, annot.clone().into_bytes()));
        let (aktuell, _) = assemble(&objects);
        assert!(
            !check(&aktuell, SECRET).findings[0].is_empty(),
            "{wie}: Vorbedingung — als aktuelles Objekt gefunden"
        );
        let pdf = with_old_generation(stream_object("", HARMLOS_CONTENT), annot.into_bytes());
        if ist_still(&check(&pdf, SECRET)) {
            still.push(wie);
        }
    }
    assert!(
        still.is_empty(),
        "stilles Leck des Orakels an einer Altgeneration: {still:?} — kein Fund, keine \
         NICHT-GEPRÜFT-Zeile"
    );
}

// ---------------------------------------------------------------------------
// 3. PDFDocEncoding und TIFF-Prädiktor in der aktuellen Revision
// ---------------------------------------------------------------------------

/// **ABSICHTLICH ROT — BEFUND ZO-D4.** Zeichenketten-Objekte in
/// PDFDocEncoding (PDF 32000-1, Anhang D.2) mit einem Zeichen aus 0x80–0xA0:
/// `€` ist 0xA0, `–` ist 0x85, `’` ist 0x90. `decode_pdf_string` liest jedes
/// Byte als Latin-1 (0xA0 → U+00A0, 0x85 → U+0085); die Byte-Muster des
/// Begriffs (UTF-8, UTF-16) treffen ebenso nicht, und ein Latin-1-Muster wird
/// für Zeichen über U+00FF gar nicht gebildet. Kein Fund, keine Meldung.
///
/// Das Objekt steht in der **aktuellen** Revision und würde vom Schwärzer
/// ohne Änderung in die Ausgabedatei geschrieben, wenn es einen Träger hat,
/// den er nicht ausräumt (etwa ein `/ActualText` in einem Property-Dictionary
/// des Seiteninhalts) — hier steht es an einer Anmerkung, weil es um das
/// Orakel geht. **Behoben** (Register #81): `decode_pdf_string` liest die
/// Abweichungen von Latin-1 nach Anhang D.2. Je Begriff zwei Schreibweisen:
/// die Bytes roh im Literal, und oktal maskiert (`\240`) — die zweite trifft
/// keine Rohsicht, nur der Dekoder der Objektsicht.
#[test]
fn zo_d4_pdfdoc_zeichen_stilles_leck() {
    // (Suchbegriff, Zeichenkette in PDFDocEncoding, roh)
    let faelle: Vec<(&str, Vec<u8>)> = vec![
        ("Betrag 5 €", b"(Betrag 5 \xA0 an Max)".to_vec()),
        ("Betrag 5 €", b"(Betrag 5 \\240 an Max)".to_vec()),
        ("Müller–Meier", b"(Konto M\xFCller\x85Meier)".to_vec()),
        ("Müller–Meier", b"(Konto M\\374ller\\205Meier)".to_vec()),
        ("O’Brien", b"(Inhaber O\x90Brien)".to_vec()),
        ("O’Brien", b"(Inhaber O\\220Brien)".to_vec()),
        ("Preis 1 000 €", b"(Preis 1\x20000\x20\xA0)".to_vec()),
    ];
    let mut still = Vec::new();
    for (needle, raw) in faelle {
        let mut annot = b"<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents ".to_vec();
        annot.extend_from_slice(&raw);
        annot.extend_from_slice(b" >>");
        let mut objects = skeleton();
        objects.push((4, stream_object("", HARMLOS_CONTENT)));
        objects.push((5, annot));
        let (pdf, _) = assemble(&objects);
        // Gegenprobe: derselbe Begriff als UTF-16BE wird gefunden.
        let mut utf16 = vec![0xFE, 0xFF];
        utf16.extend(needle.encode_utf16().flat_map(u16::to_be_bytes));
        let mut annot16 = b"<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents (".to_vec();
        for b in utf16 {
            annot16.extend_from_slice(format!("\\{b:03o}").as_bytes());
        }
        annot16.extend_from_slice(b") >>");
        let mut objects16 = skeleton();
        objects16.push((4, stream_object("", HARMLOS_CONTENT)));
        objects16.push((5, annot16));
        let (pdf16, _) = assemble(&objects16);
        assert!(
            !check(&pdf16, needle).findings[0].is_empty(),
            "{needle}: Gegenprobe UTF-16BE muss gefunden werden"
        );
        if ist_still(&check(&pdf, needle)) {
            still.push(String::from_utf8_lossy(&raw).into_owned());
        }
    }
    assert!(
        still.is_empty(),
        "stilles Leck des Orakels an PDFDocEncoding: {still:?} — kein Fund, keine \
         NICHT-GEPRÜFT-Zeile"
    );
}

/// Kontrolle: Namensobjekte mit `#20`-Maskierung liest Sicht 5 entmaskiert.
#[test]
fn zo_d5_name_mit_hash_maskierung_wird_gefunden() {
    let mut objects = skeleton();
    objects.push((4, stream_object("", HARMLOS_CONTENT)));
    objects.push((
        5,
        b"<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /T /Max#20Mustermann >>".to_vec(),
    ));
    let (pdf, _) = assemble(&objects);
    let c = check(&pdf, "Max Mustermann");
    assert!(!c.findings[0].is_empty(), "{:#?}", c.unchecked);
}

/// **ABSICHTLICH ROT — BEFUND ZO-D6 (Randfall).** `/Predictor 2` (TIFF) ist
/// ein gültiger Prädiktor für Flate und LZW (PDF 32000-1, 7.4.4.4); der
/// Dekoder kennt nur 10–15 und lässt die Differenzbytes stehen. Der Text steht
/// dekodierbar in der aktuellen Revision, kein Fund, keine Meldung. Randfall,
/// weil kein bekannter Erzeuger Text so schreibt — er steht hier, weil die
/// Runde nach jedem Filter fragt, unter dem das Orakel schweigt.
#[test]
#[ignore = "offen: Register #82 TIFF-Praediktor 2 — Spur-A-Runde 1, Beleg absichtlich rot"]
fn zo_d6_tiff_praediktor_stilles_leck() {
    let content = content_with_secret();
    let mut objects = skeleton();
    objects.push((
        4,
        stream_object(
            "/Filter /FlateDecode /DecodeParms << /Predictor 2 /Colors 1 /BitsPerComponent 8 /Columns 8 >>",
            &deflate(&tiff_rows(&content, 8)),
        ),
    ));
    objects.push((5, HARMLOS_ANNOT.to_vec()));
    let (pdf, _) = assemble(&objects);
    let c = check(&pdf, SECRET);
    assert!(
        !ist_still(&c),
        "stilles Leck des Orakels: TIFF-Prädiktor 2 — kein Fund, keine NICHT-GEPRÜFT-Zeile"
    );
}
