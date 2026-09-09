//! Stromfilter, die der Schwärzer selbst dekodiert.
//!
//! # Warum nicht `lopdf::Stream::decompressed_content`
//!
//! `lopdf` 0.42 kennt beim Auspacken genau drei Filter: `FlateDecode`,
//! `LZWDecode` und `ASCII85Decode` (`object.rs`, `decompressed_content`).
//! Alles andere ist dort ein Fehler — und ein Seiteninhalt mit
//! `/ASCIIHexDecode`, `/RunLengthDecode` oder der Kette
//! `[/ASCIIHexDecode /FlateDecode]` wurde deshalb mit Rückgabewert 1
//! abgelehnt („ließ sich nicht in Operationen zerlegen“). Gemessen an einer
//! 900-Byte-Datei je Filter: `ASCII85Decode` ging durch, die drei anderen
//! nicht.
//!
//! Das ehrliche Orakel ([`crate::audit_bytes`]) kann diese Filter seit jeher —
//! es hat einen eigenen Dekoder (`manual_decode`), damit `--check-leaks` auch
//! dort sucht, wo `lopdf` passt. Schwärzer und Orakel müssen aber dasselbe
//! lesen: eine Datei, die das Orakel dekodieren kann, muss der Schwärzer auch
//! dekodieren können, sonst lehnt er ab, was gar nicht gefährlich ist. Dieses
//! Modul ist deshalb die Kopie jener Logik, um `ASCII85Decode` ergänzt und
//! **vor** `lopdf` in den Inhaltsleser gezogen.
//!
//! # Was hier bewusst nicht steht
//!
//! * **`LZWDecode`** und **Prädiktoren** (`/DecodeParms /Predictor`) bleiben
//!   bei `lopdf`: dafür gibt es dort einen geprüften Dekoder (`weezl`), und
//!   ihn zu kopieren wäre Abstraktion auf Vorrat. Trifft die Kette auf einen
//!   solchen Filter, geht der **Rest der Kette** an `lopdf` — auch das ist
//!   mehr, als `lopdf` allein kann (`[/ASCIIHexDecode /LZWDecode]`). Dabei
//!   bekommt `lopdf` **genau den** `/DecodeParms`-Eintrag, der zum ersten
//!   Restfilter gehört, als einzelnes Dictionary — denn `lopdf` liest
//!   `/DecodeParms` nur als ein Dictionary (nie als Liste, nie über einen
//!   Verweis) und wendet es auf jeden Filter an, den es dekodiert. Eine
//!   Liste ungekürzt weiterzureichen hieße, dass der Prädiktor hinter
//!   `[/ASCIIHexDecode /FlateDecode]` stillschweigend verloren ginge.
//! * **Bildfilter** (`DCTDecode`, `JPXDecode`, `CCITTFaxDecode`, `JBIG2Decode`)
//!   sind hier ein Fehler wie bei `lopdf`. Sie stehen nie an einem
//!   Seiteninhalt; Bilder liest [`crate::image`] mit eigenem Weg.
//! * **Verkürzte Ströme** liefern wie bei `lopdf` das Teilergebnis. Ob ein
//!   Teilergebnis genügt, entscheidet nicht der Dekoder, sondern der Leser
//!   dahinter — [`crate::content::scan_page`] lehnt einen Strom ab, der sich
//!   nicht vollständig in Operationen zerlegen lässt.

use std::io::Read;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

/// Der ausgepackte Inhalt eines Stroms — über die ganze Filterkette.
///
/// `None`, wenn ein Filter weder hier noch bei `lopdf` bekannt ist. Ein Strom
/// ohne `/Filter` kommt unverändert zurück.
///
/// `doc` löst `/DecodeParms` auf — die Liste wie jeden Eintrag darin. Beides
/// darf ein Verweis sein (PDF 32000-1, 7.3.8.2), und ein nicht aufgelöster
/// Verweis läse sich wie „kein Prädiktor“.
pub fn decoded_content(doc: &Document, stream: &Stream) -> Option<Vec<u8>> {
    // `filters()` ist `Err` sowohl ohne `/Filter` als auch bei einem
    // unbrauchbaren Wert — `lopdf` liest beides als „nicht gefiltert“, und
    // die Rohbytes sind dann das Einzige, was es zu lesen gibt.
    let Ok(filters) = stream.filters() else {
        return Some(stream.content.clone());
    };
    let mut data = stream.content.clone();
    for (index, filter) in filters.iter().enumerate() {
        let parms = decode_parms(doc, &stream.dict, index);
        let Some(next) = decode_one(filter, &data, has_predictor(parms.as_ref())) else {
            return decode_rest_with_lopdf(stream, &filters[index..], parms, data);
        };
        data = next;
    }
    Some(data)
}

/// Der Seiteninhalt — alle `/Contents`-Ströme in Reihenfolge, je durch
/// [`decoded_content`], mit Zeilenumbruch dazwischen.
///
/// Die Kopie von `lopdf::Document::get_page_content`, nur mit diesem Dekoder.
/// Wie dort bleiben die Rohbytes stehen, wenn ein Strom sich nicht auspacken
/// lässt: sie enthalten dann keine Operationen, und [`crate::content::scan_page`]
/// lehnt die Seite deshalb ab, statt sie still als leer zu lesen.
pub fn page_content(doc: &Document, page_id: ObjectId) -> Vec<u8> {
    let mut content = Vec::new();
    for id in doc.get_page_contents(page_id) {
        let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) else {
            continue;
        };
        match decoded_content(doc, stream) {
            Some(data) => content.extend_from_slice(&data),
            None => content.extend_from_slice(&stream.content),
        }
        content.push(b'\n');
    }
    content
}

/// Ein einzelner Filter. `None` für alles, was `lopdf` besser kann oder was
/// hier nicht hingehört.
fn decode_one(filter: &[u8], data: &[u8], predictor: bool) -> Option<Vec<u8>> {
    match filter {
        b"FlateDecode" | b"Fl" if !predictor => Some(inflate(data)),
        b"ASCIIHexDecode" | b"AHx" => Some(ascii_hex_decode(data)),
        b"ASCII85Decode" | b"A85" => Some(ascii85_decode(data)),
        b"RunLengthDecode" | b"RL" => Some(run_length_decode(data)),
        _ => None,
    }
}

/// Reicht den Rest der Kette an `lopdf` weiter — als Strom, der nur noch die
/// verbliebenen Filter trägt.
///
/// `parms` ist der aufgelöste `/DecodeParms`-Eintrag des **ersten**
/// Restfilters, und genau der wird als einzelnes Dictionary eingetragen —
/// so, wie `lopdf` ihn liest (`Stream::decompressed_content`: ein
/// `as_dict()` auf `/DecodeParms`, angewandt auf jeden Filter der Kette).
/// Die ursprüngliche Liste stünde dort mit dem Index der **ungekürzten**
/// Kette und käme bei `lopdf` als „kein Dictionary“ an; `/DP` wird ebenfalls
/// entfernt, damit nicht der alte Eintrag unter dem Kurznamen weiterwirkt.
/// Ein zweiter Restfilter mit eigenen Parametern bekäme hier die des ersten
/// mit — das ist die Lesart von `lopdf`, und mehr als einen `lopdf`-Filter
/// (`LZWDecode`, Flate mit Prädiktor) hintereinander schreibt kein Erzeuger.
fn decode_rest_with_lopdf(
    stream: &Stream,
    rest: &[&[u8]],
    parms: Option<Dictionary>,
    data: Vec<u8>,
) -> Option<Vec<u8>> {
    let mut dict = stream.dict.clone();
    let names: Vec<Object> = rest.iter().map(|f| Object::Name(f.to_vec())).collect();
    dict.set("Filter", Object::Array(names));
    dict.remove(b"DecodeParms");
    dict.remove(b"DP");
    if let Some(parms) = parms {
        dict.set("DecodeParms", Object::Dictionary(parms));
    }
    Stream::new(dict, data)
        .with_compression(false)
        .decompressed_content()
        .ok()
}

/// Der `/DecodeParms`-Eintrag, der zum `index`-ten Filter gehört — aufgelöst.
///
/// Sowohl die Liste als auch der Eintrag darin dürfen Verweise sein. Ein
/// einzelnes Dictionary (die Form bei genau einem Filter) gilt für jeden
/// Index — so liest es auch `lopdf`, und ein Erzeuger, der zu einer Kette
/// nur ein Dictionary schreibt, meint damit den Filter, der Parameter hat.
fn decode_parms(doc: &Document, dict: &Dictionary, index: usize) -> Option<Dictionary> {
    let parms = dict.get(b"DecodeParms").or_else(|_| dict.get(b"DP")).ok()?;
    let (_, parms) = doc.dereference(parms).ok()?;
    let entry = match parms {
        Object::Array(items) => items.get(index)?,
        other => other,
    };
    doc.dereference(entry).ok()?.1.as_dict().ok().cloned()
}

/// Steht in diesem `/DecodeParms`-Eintrag ein Prädiktor?
fn has_predictor(parms: Option<&Dictionary>) -> bool {
    parms
        .and_then(|d| d.get(b"Predictor").ok())
        .and_then(|p| p.as_i64().ok())
        .is_some_and(|p| p > 1)
}

/// Flate wie bei `lopdf`: zlib, bei Fehlschlag rohes Deflate hinter dem
/// 2-Byte-Kopf; ein Teilergebnis zählt.
fn inflate(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if data.is_empty() {
        return out;
    }
    if flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .is_err()
        && out.is_empty()
        && data.len() > 2
    {
        let _ = flate2::read::DeflateDecoder::new(&data[2..]).read_to_end(&mut out);
    }
    out
}

/// `ASCIIHexDecode` (PDF 32000-1, 7.4.2): Leerraum wird übersprungen, `>`
/// beendet, eine ungerade letzte Ziffer zählt als `x0`.
fn ascii_hex_decode(data: &[u8]) -> Vec<u8> {
    let mut nibbles = Vec::with_capacity(data.len());
    for &b in data {
        if b == b'>' {
            break;
        }
        if let Some(v) = hex_value(b) {
            nibbles.push(v);
        }
    }
    if nibbles.len() % 2 == 1 {
        nibbles.push(0);
    }
    nibbles.chunks(2).map(|c| (c[0] << 4) | c[1]).collect()
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `ASCII85Decode` (PDF 32000-1, 7.4.3): Fünfergruppen aus `!`..`u`, `z` für
/// vier Nullbytes, `~>` beendet; eine angebrochene Schlussgruppe wird mit `u`
/// aufgefüllt und liefert `n-1` Bytes. Ein Zeichen außerhalb des Alphabets
/// beendet die Dekodierung — wie bei `lopdf`.
fn ascii85_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 4 / 5);
    let mut group = [0u8; 5];
    let mut count = 0usize;
    let mut i = 0usize;
    // Ein führendes `<~` schreiben manche Erzeuger — es ist kein PDF, stört
    // aber niemanden, wenn es übersprungen wird.
    if data.starts_with(b"<~") {
        i = 2;
    }
    while i < data.len() {
        let b = data[i];
        i += 1;
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'~' {
            break;
        }
        if b == b'z' && count == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&b) {
            break;
        }
        group[count] = b - b'!';
        count += 1;
        if count == 5 {
            out.extend_from_slice(&ascii85_group(&group));
            count = 0;
        }
    }
    if count > 1 {
        for slot in group.iter_mut().skip(count) {
            *slot = 84;
        }
        out.extend_from_slice(&ascii85_group(&group)[..count - 1]);
    }
    out
}

fn ascii85_group(group: &[u8; 5]) -> [u8; 4] {
    // Mit `u32::wrapping_*`: die größte gültige Gruppe `s8W-!` ergibt genau
    // `u32::MAX`; eine ungültige darüber (`uuuuu`) darf nicht abstürzen.
    let value = group
        .iter()
        .fold(0u32, |acc, &d| acc.wrapping_mul(85).wrapping_add(d as u32));
    value.to_be_bytes()
}

/// `RunLengthDecode` (PDF 32000-1, 7.4.5): Länge `0..=127` heißt „die nächsten
/// `n+1` Bytes wörtlich“, `129..=255` heißt „das nächste Byte `257-n`-mal“,
/// `128` ist das Ende.
fn run_length_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        let length = data[i];
        i += 1;
        match length {
            128 => break,
            0..=127 => {
                let n = length as usize + 1;
                let end = (i + n).min(data.len());
                out.extend_from_slice(&data[i..end]);
                i = end;
            }
            _ => {
                if let Some(&b) = data.get(i) {
                    out.extend(std::iter::repeat_n(b, 257 - length as usize));
                }
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    const PLAIN: &[u8] = b"BT /F1 10 Tf 72 700 Td (IBAN: DE89 3704 0044 0532 0130 00) Tj ET";

    /// Ein leeres Dokument — genug, um `/DecodeParms` ohne Verweise
    /// aufzulösen.
    fn doc() -> Document {
        Document::with_version("1.5")
    }

    /// `PLAIN`, zeilenweise mit dem PNG-Prädiktor „None“ (ein Filterbyte 0
    /// vor jeder Zeile von 8 Byte) — was ein `/Predictor 12 /Columns 8`
    /// beim Dekodieren wieder entfernt.
    fn png_rows() -> Vec<u8> {
        let mut rows = Vec::new();
        for chunk in PLAIN.chunks(8) {
            rows.push(0u8);
            rows.extend_from_slice(chunk);
        }
        rows
    }

    fn with_filter(filter: Object, content: Vec<u8>) -> Stream {
        Stream::new(dictionary! { "Filter" => filter }, content).with_compression(false)
    }

    fn deflate(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(data).unwrap();
        enc.finish().unwrap()
    }

    fn hex(data: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = data
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<String>()
            .into();
        out.push(b'>');
        out
    }

    fn run_length(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in data.chunks(128) {
            out.push((chunk.len() - 1) as u8);
            out.extend_from_slice(chunk);
        }
        out.push(128);
        out
    }

    fn a85(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in data.chunks(4) {
            let mut buf = [0u8; 4];
            buf[..chunk.len()].copy_from_slice(chunk);
            let mut value = u32::from_be_bytes(buf);
            let mut digits = [0u8; 5];
            for slot in digits.iter_mut().rev() {
                *slot = (value % 85) as u8 + b'!';
                value /= 85;
            }
            out.extend_from_slice(&digits[..chunk.len() + 1]);
        }
        out.extend_from_slice(b"~>");
        out
    }

    #[test]
    fn ein_strom_ohne_filter_kommt_unveraendert_zurueck() {
        let stream = Stream::new(dictionary! {}, PLAIN.to_vec());
        assert_eq!(decoded_content(&doc(), &stream).as_deref(), Some(PLAIN));
    }

    #[test]
    fn asciihex_wird_dekodiert() {
        let stream = with_filter("ASCIIHexDecode".into(), hex(PLAIN));
        assert_eq!(decoded_content(&doc(), &stream).as_deref(), Some(PLAIN));
        // Leerraum und Kleinbuchstaben, ungerade letzte Ziffer.
        assert_eq!(ascii_hex_decode(b"4 1\n4 2 4>"), b"AB@");
    }

    #[test]
    fn runlength_wird_dekodiert() {
        let stream = with_filter("RunLengthDecode".into(), run_length(PLAIN));
        assert_eq!(decoded_content(&doc(), &stream).as_deref(), Some(PLAIN));
        assert_eq!(
            run_length_decode(&[2, b'X', b'Y', b'Z', 253, b'A', 128]),
            b"XYZAAAA"
        );
    }

    #[test]
    fn ascii85_wird_dekodiert() {
        let stream = with_filter("ASCII85Decode".into(), a85(PLAIN));
        assert_eq!(decoded_content(&doc(), &stream).as_deref(), Some(PLAIN));
        // Vier Nullbytes als `z`, angebrochene Schlussgruppe, Leerraum.
        assert_eq!(
            ascii85_decode(b"z87cU\nRD]i,\"Ebo80~>"),
            b"\0\0\0\0Hello World!"
        );
        // Deckungsgleich mit lopdf an derselben Eingabe.
        assert_eq!(
            decoded_content(&doc(), &stream),
            stream.decompressed_content().ok(),
            "ASCII85: eigener Dekoder und lopdf müssen dasselbe lesen"
        );
    }

    #[test]
    fn die_kette_asciihex_flate_wird_in_reihenfolge_ausgepackt() {
        let chain = Object::Array(vec!["ASCIIHexDecode".into(), "FlateDecode".into()]);
        let stream = with_filter(chain, hex(&deflate(PLAIN)));
        assert!(
            stream.decompressed_content().is_err(),
            "lopdf kann die Kette nicht"
        );
        assert_eq!(decoded_content(&doc(), &stream).as_deref(), Some(PLAIN));
    }

    #[test]
    fn die_kette_runlength_flate_wird_in_reihenfolge_ausgepackt() {
        let chain = Object::Array(vec!["RunLengthDecode".into(), "FlateDecode".into()]);
        let stream = with_filter(chain, run_length(&deflate(PLAIN)));
        assert_eq!(decoded_content(&doc(), &stream).as_deref(), Some(PLAIN));
    }

    #[test]
    fn flate_allein_liest_dasselbe_wie_lopdf() {
        let stream = with_filter("FlateDecode".into(), deflate(PLAIN));
        assert_eq!(decoded_content(&doc(), &stream).as_deref(), Some(PLAIN));
        assert_eq!(
            decoded_content(&doc(), &stream),
            stream.decompressed_content().ok()
        );
    }

    /// LZW bleibt bei `lopdf` — auch hinter einem eigenen Filter.
    #[test]
    fn der_rest_der_kette_geht_an_lopdf() {
        // `lzw_encode` aus den Integrationstests ist hier nicht erreichbar;
        // stattdessen der kleinste gültige LZW-Strom: Clear-Code, `A`, EOD —
        // 9-Bit-Codes 256, 65, 257, MSB-first: 100000000 001000001 100000001.
        let lzw = vec![0x80, 0x10, 0x60, 0x20];
        let stream = with_filter("LZWDecode".into(), lzw.clone());
        let via_lopdf = stream.decompressed_content().expect("lopdf kann LZW");
        assert_eq!(via_lopdf, b"A");
        assert_eq!(
            decoded_content(&doc(), &stream).as_deref(),
            Some(b"A".as_slice())
        );
        let chain = Object::Array(vec!["ASCIIHexDecode".into(), "LZWDecode".into()]);
        let stream = with_filter(chain, hex(&lzw));
        assert!(
            stream.decompressed_content().is_err(),
            "lopdf kann die Kette nicht"
        );
        assert_eq!(
            decoded_content(&doc(), &stream).as_deref(),
            Some(b"A".as_slice())
        );
    }

    #[test]
    fn ein_praediktor_bleibt_bei_lopdf() {
        let mut stream = with_filter("FlateDecode".into(), deflate(&png_rows()));
        stream.dict.set(
            "DecodeParms",
            dictionary! { "Predictor" => 12, "Columns" => 8 },
        );
        assert!(has_predictor(
            decode_parms(&doc(), &stream.dict, 0).as_ref()
        ));
        assert_eq!(
            decoded_content(&doc(), &stream),
            stream.decompressed_content().ok()
        );
        assert_eq!(decoded_content(&doc(), &stream).as_deref(), Some(PLAIN));
    }

    /// `[/ASCIIHexDecode /FlateDecode]` mit `/DecodeParms [null <<…>>]`: der
    /// Prädiktor gehört zum **zweiten** Filter. Früher ging die Liste
    /// ungekürzt an `lopdf`, das sie nicht als Dictionary lesen kann — der
    /// Prädiktor fiel still weg, und die Filterbytes blieben im Text stehen.
    #[test]
    fn eine_kette_mit_praediktor_hinter_asciihex_wird_richtig_zerlegt() {
        let chain = Object::Array(vec!["ASCIIHexDecode".into(), "FlateDecode".into()]);
        let mut stream = with_filter(chain, hex(&deflate(&png_rows())));
        stream.dict.set(
            "DecodeParms",
            Object::Array(vec![
                Object::Null,
                Object::Dictionary(dictionary! { "Predictor" => 12, "Columns" => 8 }),
            ]),
        );
        assert!(!has_predictor(
            decode_parms(&doc(), &stream.dict, 0).as_ref()
        ));
        assert!(has_predictor(
            decode_parms(&doc(), &stream.dict, 1).as_ref()
        ));
        assert_eq!(decoded_content(&doc(), &stream).as_deref(), Some(PLAIN));
        // Dasselbe unter dem Kurznamen, wie er in Inline-Bildern und bei
        // manchen Erzeugern steht.
        let parms = stream.dict.remove(b"DecodeParms").expect("gesetzt");
        stream.dict.set("DP", parms);
        assert_eq!(decoded_content(&doc(), &stream).as_deref(), Some(PLAIN));
    }

    /// `/DecodeParms 5 0 R` und `/DecodeParms [null 6 0 R]`: Liste und
    /// Eintrag als Verweis (PDF 32000-1, 7.3.8.2). Unaufgelöst läse sich ein
    /// Verweis wie „kein Prädiktor“ — mit demselben Ergebnis wie oben.
    #[test]
    fn decodeparms_als_verweis_wird_aufgeloest() {
        let mut doc = doc();
        let parms_id = doc.add_object(dictionary! { "Predictor" => 12, "Columns" => 8 });

        // Ein Filter, die Liste selbst ist der Verweis.
        let mut stream = with_filter("FlateDecode".into(), deflate(&png_rows()));
        stream.dict.set("DecodeParms", Object::Reference(parms_id));
        assert!(
            stream
                .decompressed_content()
                .map(|d| d != PLAIN)
                .unwrap_or(true),
            "lopdf allein löst den Verweis nicht auf — sonst prüfte der Test nichts"
        );
        assert_eq!(decoded_content(&doc, &stream).as_deref(), Some(PLAIN));

        // Eine Kette, der Eintrag in der Liste ist der Verweis.
        let chain = Object::Array(vec!["ASCIIHexDecode".into(), "FlateDecode".into()]);
        let mut stream = with_filter(chain, hex(&deflate(&png_rows())));
        stream.dict.set(
            "DecodeParms",
            Object::Array(vec![Object::Null, Object::Reference(parms_id)]),
        );
        assert_eq!(decoded_content(&doc, &stream).as_deref(), Some(PLAIN));

        // Und die Liste als Verweis auf ein Array mit einem Verweis darin.
        let list_id = doc.add_object(Object::Array(vec![
            Object::Null,
            Object::Reference(parms_id),
        ]));
        stream.dict.set("DecodeParms", Object::Reference(list_id));
        assert_eq!(decoded_content(&doc, &stream).as_deref(), Some(PLAIN));
    }

    #[test]
    fn ein_bildfilter_ist_keine_inhaltsdekodierung() {
        let stream = with_filter("DCTDecode".into(), vec![0xff, 0xd8]);
        assert_eq!(decoded_content(&doc(), &stream), None);
    }

    #[test]
    fn ein_verkuerzter_flate_strom_liefert_das_teilergebnis_wie_lopdf() {
        let mut data = deflate(&PLAIN.repeat(40));
        data.truncate(data.len() / 2);
        let stream = with_filter("FlateDecode".into(), data);
        let ours = decoded_content(&doc(), &stream).expect("Teilergebnis");
        let theirs = stream.decompressed_content().expect("lopdf: Teilergebnis");
        assert_eq!(ours, theirs);
    }

    #[test]
    fn ascii85_stuerzt_an_einer_ueberlaufenden_gruppe_nicht_ab() {
        let _ = ascii85_decode(b"uuuuu~>");
        let _ = ascii85_decode(b"s8W-!~>");
    }
}
