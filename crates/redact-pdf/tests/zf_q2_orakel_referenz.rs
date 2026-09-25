//! Gegenprüfung Q2: eine **unabhängige** Referenz für die Objektsicht des
//! Leck-Orakels (Sicht 3, `Objekt N G <Stream, …>`).
//!
//! Eigener Code, eigene Kodierungstabelle, eigene Filterkette, eigener
//! Mini-Lexer für Zeichenketten, eigene naive Suche. Verglichen wird nicht
//! „ungefähr dasselbe“, sondern Zeichen für Zeichen: **Zahl**, **Reihenfolge**,
//! **Kontextfenster** und **Bezeichnung** jeder Fundstelle, die
//! [`redact_pdf::leaks_many_within`] für einen Strom meldet.
//!
//! Das ist der Maßstab des Maßstabs: findet das Orakel weniger als diese
//! bewusst dumme Referenz, ist jeder grüne Test des Projekts wertlos.
//!
//! Nicht abgedeckt (bewusst): `/LZWDecode` — der Dekoder dort ist `weezl`, und
//! eine „unabhängige“ LZW-Referenz wäre dieselbe Bibliothek noch einmal.

mod common;

use lopdf::{dictionary, Object, Stream};
use redact_pdf::leaks_many_within;

use common::{page, SECRET};

// ---------------------------------------------------------------------------
// Die Referenz — bewusst dumm und langsam
// ---------------------------------------------------------------------------

const CONTEXT: usize = 24;
/// `scan_raw_bytes` nimmt je Muster höchstens vier Fundstellen.
const BLOB_LIMIT: usize = 4;

fn hex(bytes: &[u8], upper: bool) -> Vec<u8> {
    let d: &[u8; 16] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    bytes
        .iter()
        .flat_map(|b| [d[(b >> 4) as usize], d[(b & 15) as usize]])
        .collect()
}

/// Alle Kodierungen eines Begriffs — meine eigene Tabelle, in der Reihenfolge,
/// in der die Fundstellen gemeldet werden. Aufeinanderfolgende Doppelungen
/// fallen weg (bei einem reinen Ziffern-Hex sind „gross“ und „klein“ gleich).
fn variants(text: &str) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let utf8 = text.as_bytes().to_vec();
    out.push(("UTF-8/ASCII".into(), utf8.clone()));
    if text.chars().all(|c| (c as u32) < 0x100) {
        let latin1: Vec<u8> = text.chars().map(|c| c as u8).collect();
        if latin1 != utf8 {
            out.push(("Latin-1/PDFDoc".into(), latin1.clone()));
        }
        out.push(("Hex-String (Latin-1, gross)".into(), hex(&latin1, true)));
        out.push(("Hex-String (Latin-1, klein)".into(), hex(&latin1, false)));
    }
    let be: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    out.push(("UTF-16BE".into(), be.clone()));
    out.push(("Hex-String (UTF-16BE, gross)".into(), hex(&be, true)));
    out.push(("Hex-String (UTF-16BE, klein)".into(), hex(&be, false)));
    let le: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.push(("UTF-16LE".into(), le.clone()));
    out.push(("Hex-String (UTF-16LE, gross)".into(), hex(&le, true)));
    out.push(("Hex-String (UTF-16LE, klein)".into(), hex(&le, false)));

    let mut deduped: Vec<(String, Vec<u8>)> = Vec::new();
    for v in out {
        if deduped.last().map(|l: &(String, Vec<u8>)| &l.1) == Some(&v.1) {
            continue;
        }
        deduped.push(v);
    }
    deduped
}

/// Naive Suche: von vorn, nach einem Treffer hinter ihm weiter, höchstens
/// `limit` Stellen.
fn find_all(hay: &[u8], needle: &[u8], limit: usize) -> Vec<usize> {
    let mut out = Vec::new();
    if needle.is_empty() || needle.len() > hay.len() {
        return out;
    }
    let mut i = 0usize;
    while i + needle.len() <= hay.len() && out.len() < limit {
        if &hay[i..i + needle.len()] == needle {
            out.push(i);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

fn context(hay: &[u8], pos: usize, len: usize) -> String {
    let start = pos.saturating_sub(CONTEXT);
    let end = (pos + len + CONTEXT).min(hay.len());
    hay[start..end]
        .iter()
        .map(|&b| {
            if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect()
}

fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Mein eigener Mini-Lexer: verkettet alle `(…)`- und `<…>`-Literale eines
/// Blocks, dekodiert wie eine PDF-Zeichenkette.
fn concat_strings(blob: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < blob.len() {
        match blob[i] {
            b'(' => {
                let (bytes, next) = lit(blob, i + 1);
                out.push_str(&decode_string(&bytes));
                i = next;
            }
            b'<' if blob.get(i + 1) != Some(&b'<') => {
                let (bytes, next) = hexstr(blob, i + 1);
                out.push_str(&decode_string(&bytes));
                i = next;
            }
            _ => i += 1,
        }
    }
    out
}

fn lit(blob: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut depth = 1usize;
    while i < blob.len() {
        match blob[i] {
            b'\\' => {
                i += 1;
                let Some(&e) = blob.get(i) else { break };
                match e {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'\n' => {}
                    b'\r' => {
                        if blob.get(i + 1) == Some(&b'\n') {
                            i += 1;
                        }
                    }
                    b'0'..=b'7' => {
                        let mut v = u32::from(e - b'0');
                        let mut taken = 1;
                        while taken < 3 {
                            match blob.get(i + 1) {
                                Some(&d @ b'0'..=b'7') => {
                                    v = v * 8 + u32::from(d - b'0');
                                    i += 1;
                                    taken += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(v as u8);
                    }
                    other => out.push(other),
                }
                i += 1;
            }
            b'(' => {
                depth += 1;
                out.push(b'(');
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
                out.push(b')');
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    (out, i)
}

fn nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn hexstr(blob: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    let start = i;
    let mut n = Vec::new();
    while i < blob.len() && blob[i] != b'>' {
        match nibble(blob[i]) {
            Some(v) => n.push(v),
            None if blob[i].is_ascii_whitespace() => {}
            None => return (Vec::new(), start),
        }
        i += 1;
    }
    if i < blob.len() {
        i += 1;
    }
    if n.len() % 2 == 1 {
        n.push(0);
    }
    (n.chunks(2).map(|c| (c[0] << 4) | c[1]).collect(), i)
}

fn decode_string(raw: &[u8]) -> String {
    if raw.len() >= 2 && raw[0] == 0xfe && raw[1] == 0xff {
        String::from_utf16_lossy(
            &raw[2..]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        )
    } else if raw.len() >= 2 && raw[0] == 0xff && raw[1] == 0xfe {
        String::from_utf16_lossy(
            &raw[2..]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        )
    } else {
        raw.iter().map(|&b| b as char).collect()
    }
}

// ---------------------------------------------------------------------------
// Meine eigene Filterkette
// ---------------------------------------------------------------------------

fn ref_inflate(data: &[u8]) -> Vec<u8> {
    use std::io::Read;
    if data.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let _ = flate2::read::ZlibDecoder::new(data).read_to_end(&mut out);
    if out.is_empty() && data.len() > 2 {
        let mut raw = Vec::new();
        let _ = flate2::read::DeflateDecoder::new(&data[2..]).read_to_end(&mut raw);
        return raw;
    }
    out
}

fn ref_ahx(data: &[u8]) -> Vec<u8> {
    let mut n = Vec::new();
    for &b in data {
        if b == b'>' {
            break;
        }
        if let Some(v) = nibble(b) {
            n.push(v);
        }
    }
    if n.len() % 2 == 1 {
        n.push(0);
    }
    n.chunks(2).map(|c| (c[0] << 4) | c[1]).collect()
}

fn ref_a85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut g = [0u8; 5];
    let mut c = 0usize;
    let mut i = if data.starts_with(b"<~") { 2 } else { 0 };
    let word = |g: &[u8; 5]| -> [u8; 4] {
        g.iter()
            .fold(0u32, |a, &d| a.wrapping_mul(85).wrapping_add(u32::from(d)))
            .to_be_bytes()
    };
    while i < data.len() {
        let b = data[i];
        i += 1;
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'~' {
            break;
        }
        if b == b'z' && c == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&b) {
            break;
        }
        g[c] = b - b'!';
        c += 1;
        if c == 5 {
            out.extend_from_slice(&word(&g));
            c = 0;
        }
    }
    if c > 1 {
        for s in g.iter_mut().skip(c) {
            *s = 84;
        }
        out.extend_from_slice(&word(&g)[..c - 1]);
    }
    out
}

fn ref_rl(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        let l = data[i];
        i += 1;
        match l {
            128 => break,
            0..=127 => {
                let end = (i + l as usize + 1).min(data.len());
                out.extend_from_slice(&data[i..end]);
                i = end;
            }
            _ => {
                if let Some(&b) = data.get(i) {
                    out.extend(std::iter::repeat_n(b, 257 - l as usize));
                }
                i += 1;
            }
        }
    }
    out
}

fn ref_bild(name: &str) -> bool {
    matches!(
        name,
        "DCTDecode" | "DCT" | "JPXDecode" | "CCITTFaxDecode" | "CCF" | "JBIG2Decode"
    )
}

/// Meine Kette: (Bytes nach dem letzten angewandten Filter, angewandte Filter).
fn ref_chain(filters: &[&str], raw: &[u8]) -> (Vec<u8>, usize) {
    let mut data = raw.to_vec();
    for (i, f) in filters.iter().enumerate() {
        data = match *f {
            "FlateDecode" | "Fl" => ref_inflate(&data),
            "ASCIIHexDecode" | "AHx" => ref_ahx(&data),
            "ASCII85Decode" | "A85" => ref_a85(&data),
            "RunLengthDecode" | "RL" => ref_rl(&data),
            _ => return (data, i),
        };
    }
    (data, filters.len())
}

/// Die Meldungen, die die Objektsicht für **diesen** Strom liefern muss.
fn ref_stream_hits(objekt: &str, filters: &[&str], raw: &[u8], needle: &str) -> Vec<String> {
    let mut out = Vec::new();
    let vars = variants(needle);
    let sq = squeeze(needle);
    let squeezed = (sq != needle && !sq.is_empty()).then_some(sq);

    let blob = |label: String, data: &[u8], out: &mut Vec<String>| {
        for (name, pattern) in &vars {
            for pos in find_all(data, pattern, BLOB_LIMIT) {
                out.push(format!(
                    "{objekt} <Stream, {label}> [Inhalt, {name}]: …{}…",
                    context(data, pos, pattern.len())
                ));
            }
        }
        let joined = concat_strings(data);
        if joined.is_empty() {
            return;
        }
        if let Some(&pos) = find_all(joined.as_bytes(), needle.as_bytes(), 1).first() {
            out.push(format!(
                "{objekt} <Stream, {label}> [Zeichenketten-Verkettung]: …{}…",
                context(joined.as_bytes(), pos, needle.len())
            ));
        }
        if let Some(s) = &squeezed {
            let flat = squeeze(&joined);
            if let Some(&pos) = find_all(flat.as_bytes(), s.as_bytes(), 1).first() {
                out.push(format!(
                    "{objekt} <Stream, {label}> [Zeichenketten-Verkettung, ohne Leerraum]: …{}…",
                    context(flat.as_bytes(), pos, s.len())
                ));
            }
        }
    };

    blob("roh".to_string(), raw, &mut out);

    let (data, applied) = ref_chain(filters, raw);
    if !filters.is_empty() && applied > 0 {
        let chain = |n: &[&str]| n.join("+");
        let label = if applied == filters.len() {
            format!("dekodiert: {}", chain(filters))
        } else {
            format!(
                "dekodiert: {} — bis Filter {applied} von {}, danach /{} unbekannt",
                chain(&filters[..applied]),
                filters.len(),
                filters[applied]
            )
        };
        blob(label, &data, &mut out);
    }
    // Doppelte Meldungen fallen im Orakel weg (`Report::seen`).
    let mut seen = std::collections::BTreeSet::new();
    out.retain(|m| seen.insert(m.clone()));
    out
}

// ---------------------------------------------------------------------------
// Material
// ---------------------------------------------------------------------------

fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).expect("komprimierbar");
    e.finish().expect("komprimierbar")
}

fn a85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in data.chunks(4) {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        let mut v = u32::from_be_bytes(w);
        let mut g = [0u8; 5];
        for s in g.iter_mut().rev() {
            *s = b'!' + (v % 85) as u8;
            v /= 85;
        }
        out.extend_from_slice(&g[..c.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

fn ahx(data: &[u8]) -> Vec<u8> {
    let mut out = hex(data, true);
    out.push(b'>');
    out
}

fn rl(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in data.chunks(127) {
        out.push(c.len() as u8 - 1);
        out.extend_from_slice(c);
    }
    out.push(128);
    out
}

/// Ein PDF mit **genau einem** zusätzlichen Strom (Objekt 7 0), der nicht
/// Seiteninhalt ist — damit Sicht 7 (Schriftdekoder) nichts beisteuert und
/// die Objektsicht allein misst.
fn pdf_mit_strom(filters: &[&str], raw: Vec<u8>) -> Vec<u8> {
    let value = match filters.len() {
        0 => None,
        1 => Some(Object::Name(filters[0].as_bytes().to_vec())),
        _ => Some(Object::Array(
            filters
                .iter()
                .map(|f| Object::Name(f.as_bytes().to_vec()))
                .collect(),
        )),
    };
    let mut dict = dictionary! {};
    if let Some(v) = value {
        dict.set("Filter", v);
    }
    let mut d = page(&["harmlos"]);
    let id = d.add(Object::Stream(
        Stream::new(dict, raw).with_compression(false),
    ));
    assert_eq!(id, (7, 0), "die Objekt-Id muss vorhersagbar sein");
    d.catalog_set("Q2Extra", Object::Reference(id));
    d.finish()
}

/// Nur die Meldungen der Objektsicht zu Objekt 7 0.
fn ist(pdf: &[u8], needle: &str) -> Vec<String> {
    leaks_many_within(pdf, &[needle], u64::MAX)
        .findings
        .remove(0)
        .into_iter()
        .filter(|m| m.starts_with("Objekt 7 0 <Stream,"))
        .collect()
}

fn vergleiche(fall: &str, filters: &[&str], raw: Vec<u8>, needle: &str) {
    let pdf = pdf_mit_strom(filters, raw.clone());
    let soll = ref_stream_hits("Objekt 7 0", filters, &raw, needle);
    let ist = ist(&pdf, needle);
    assert_eq!(
        ist,
        soll,
        "{fall}: das Orakel weicht von der unabhängigen Referenz ab\n\
         ist  ({}): {ist:#?}\nsoll ({}): {soll:#?}",
        ist.len(),
        soll.len()
    );
    assert!(
        !soll.is_empty(),
        "{fall}: die Referenz selbst findet nichts"
    );
}

// ---------------------------------------------------------------------------
// Die Vergleiche
// ---------------------------------------------------------------------------

fn nutzlast(text: &str) -> Vec<u8> {
    format!("BT /F1 10 Tf 72 700 Td (Notiz zur IBAN {text}) Tj ET").into_bytes()
}

#[test]
fn q2_referenz_deckt_die_filterketten() {
    let plain = nutzlast(SECRET);

    vergleiche("ohne Filter", &[], plain.clone(), SECRET);
    vergleiche("Flate", &["FlateDecode"], deflate(&plain), SECRET);
    vergleiche("ASCIIHex", &["ASCIIHexDecode"], ahx(&plain), SECRET);
    vergleiche("ASCII85", &["ASCII85Decode"], a85(&plain), SECRET);
    vergleiche("RunLength", &["RunLengthDecode"], rl(&plain), SECRET);
    vergleiche(
        "Kette AHx+Flate",
        &["ASCIIHexDecode", "FlateDecode"],
        ahx(&deflate(&plain)),
        SECRET,
    );
    vergleiche(
        "Kette A85+Flate+RL",
        &["ASCII85Decode", "FlateDecode", "RunLengthDecode"],
        a85(&deflate(&rl(&plain))),
        SECRET,
    );
    vergleiche(
        "Kurzformen Fl/AHx",
        &["AHx", "Fl"],
        ahx(&deflate(&plain)),
        SECRET,
    );
}

#[test]
fn q2_referenz_deckt_bildfilter_am_kettenende() {
    let plain = nutzlast(SECRET);
    vergleiche(
        "A85 + DCTDecode",
        &["ASCII85Decode", "DCTDecode"],
        a85(&plain),
        SECRET,
    );
    vergleiche(
        "AHx + Flate + JPXDecode",
        &["ASCIIHexDecode", "FlateDecode", "JPXDecode"],
        ahx(&deflate(&plain)),
        SECRET,
    );
    vergleiche(
        "Flate + CCITTFaxDecode",
        &["FlateDecode", "CCITTFaxDecode"],
        deflate(&plain),
        SECRET,
    );
    vergleiche(
        "Flate + JBIG2Decode",
        &["FlateDecode", "JBIG2Decode"],
        deflate(&plain),
        SECRET,
    );
    vergleiche(
        "Flate + unbekannter Name",
        &["FlateDecode", "Q2Phantasie"],
        deflate(&plain),
        SECRET,
    );
    assert!(
        ref_bild("DCT") && ref_bild("CCF"),
        "die Kurzformen gehören dazu"
    );
}

#[test]
fn q2_referenz_deckt_teilweise_dekodierbare_stroeme() {
    let plain = nutzlast(SECRET);
    // Flate mitten im Strom abgeschnitten — beide müssen dasselbe
    // Teilergebnis lesen (die letzten 10 % der gepackten Bytes fehlen).
    let packed = deflate(&plain);
    vergleiche(
        "Flate, abgeschnitten",
        &["FlateDecode"],
        packed[..packed.len() * 9 / 10].to_vec(),
        SECRET,
    );
    // Ein Filter liefert Müll, den der nächste nicht lesen kann.
    vergleiche(
        "AHx liefert Müll, Flate scheitert daran — Rohsicht trägt",
        &["ASCIIHexDecode", "FlateDecode"],
        ahx(&plain),
        SECRET,
    );
}

#[test]
fn q2_referenz_deckt_utf16_im_strom() {
    let be: Vec<u8> = format!("Notiz zur IBAN {SECRET}")
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    let le: Vec<u8> = format!("Notiz zur IBAN {SECRET}")
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    vergleiche(
        "UTF-16BE im Flate-Strom",
        &["FlateDecode"],
        deflate(&be),
        SECRET,
    );
    vergleiche(
        "UTF-16LE im Flate-Strom",
        &["FlateDecode"],
        deflate(&le),
        SECRET,
    );
    vergleiche(
        "UTF-16LE hinter ASCII85",
        &["ASCII85Decode"],
        a85(&le),
        SECRET,
    );
}

#[test]
fn q2_referenz_deckt_ueberlappende_und_leerraum_begriffe() {
    let text = "Max Mustermann, Mustermann & Co., IBAN DE89 3704 0044 0532 0130 00";
    let raw = format!("BT /F1 10 Tf 72 700 Td ({text}) Tj ET").into_bytes();
    for needle in [
        "Max Mustermann",
        "Mustermann",
        "usterman",
        SECRET,
        "DE893704004405320130 00",
    ] {
        vergleiche(
            &format!("überlappend/Leerraum: {needle:?}"),
            &["FlateDecode"],
            deflate(&raw),
            needle,
        );
    }
}

#[test]
fn q2_referenz_deckt_mehrfache_vorkommen() {
    // Sechs Vorkommen, gemeldet werden höchstens vier je Muster.
    let mut raw = Vec::new();
    for i in 0..6 {
        raw.extend_from_slice(format!("({SECRET}) Tj % Nr {i}\n").as_bytes());
    }
    vergleiche(
        "sechsmal derselbe Text",
        &["FlateDecode"],
        deflate(&raw),
        SECRET,
    );
    let ist = ist(&pdf_mit_strom(&["FlateDecode"], deflate(&raw)), SECRET);
    let inhalt = ist
        .iter()
        .filter(|m| m.contains("[Inhalt, UTF-8/ASCII]"))
        .count();
    // Die Rohsicht sieht nur gepackte Bytes; die vier Stellen kommen alle
    // aus der dekodierten Sicht — das Muster wird also wirklich gedeckelt.
    assert_eq!(inhalt, BLOB_LIMIT, "vier Stellen je Muster und Sicht");
}

// ---------------------------------------------------------------------------
// Zeichenketten-Objekte (Sicht 5)
// ---------------------------------------------------------------------------

/// Die Meldungen, die die Objektsicht für ein Zeichenketten-Objekt liefern
/// muss: erst der dekodierte Text, dann die Rohbytes in allen Kodierungen.
fn ref_string_hits(objekt: &str, how: &str, raw: &[u8], needle: &str) -> Vec<String> {
    let mut out = Vec::new();
    let text = decode_string(raw);
    let sq = squeeze(needle);
    let squeezed = (sq != needle && !sq.is_empty()).then_some(sq);
    let wörtlich = find_all(text.as_bytes(), needle.as_bytes(), 1);
    if let Some(&pos) = wörtlich.first() {
        out.push(format!(
            "{objekt} [{how}]: …{}…",
            context(text.as_bytes(), pos, needle.len())
        ));
    } else if let Some(s) = &squeezed {
        let flach = squeeze(&text);
        if let Some(&pos) = find_all(flach.as_bytes(), s.as_bytes(), 1).first() {
            out.push(format!(
                "{objekt} [{how}, ohne Leerraum]: …{}…",
                context(flach.as_bytes(), pos, s.len())
            ));
        }
    }
    for (name, pattern) in variants(needle) {
        for pos in find_all(raw, &pattern, BLOB_LIMIT) {
            out.push(format!(
                "{objekt} [{how}, {name}]: …{}…",
                context(raw, pos, pattern.len())
            ));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    out.retain(|m| seen.insert(m.clone()));
    out
}

/// UTF-16LE **in einem Zeichenketten-Objekt** — mit BOM `FF FE` (dann liest
/// der Dekoder den Text) und ohne (dann trägt die Bytesuche). Beides muss
/// kommen, und zwar genau so, wie meine Referenz es vorhersagt.
#[test]
fn q2_referenz_deckt_zeichenketten_objekte() {
    let text = format!("Notiz zur IBAN {SECRET}");
    let le: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let be: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut le_bom = vec![0xff, 0xfe];
    le_bom.extend_from_slice(&le);
    let mut be_bom = vec![0xfe, 0xff];
    be_bom.extend_from_slice(&be);

    for (name, raw) in [
        ("Latin-1", text.as_bytes().to_vec()),
        ("UTF-16LE ohne BOM", le),
        ("UTF-16LE mit BOM", le_bom),
        ("UTF-16BE ohne BOM", be),
        ("UTF-16BE mit BOM", be_bom),
    ] {
        for (format, how) in [
            (lopdf::StringFormat::Literal, "Zeichenkette, literal"),
            (lopdf::StringFormat::Hexadecimal, "Zeichenkette, hex"),
        ] {
            let mut d = page(&["harmlos"]);
            let id = d.add(Object::String(raw.clone(), format));
            assert_eq!(id, (7, 0));
            d.catalog_set("Q2Str", Object::Reference(id));
            let bytes = d.finish();
            let soll = ref_string_hits("Objekt 7 0", how, &raw, SECRET);
            let ist: Vec<String> = leaks_many_within(&bytes, &[SECRET], u64::MAX)
                .findings
                .remove(0)
                .into_iter()
                .filter(|m| m.starts_with("Objekt 7 0 [Zeichenkette"))
                .collect();
            assert_eq!(
                ist, soll,
                "{name} als {how}: Abweichung von der Referenz\nist {ist:#?}\nsoll {soll:#?}"
            );
            assert!(!soll.is_empty(), "{name} als {how}: Referenz findet nichts");
        }
    }
}
