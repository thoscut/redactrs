//! Gegenprüfung R2 (nach Fix-Runde 6): ein **eigenes Modell** dessen, was das
//! Leck-Orakel über einen gefilterten Strom sagen muss.
//!
//! Die Zusicherung nach Fix-Runde 6 lautet: die Entscheidung fällt am Filter,
//! an dem die Kette stehen blieb (`names[applied]`), nicht an seiner Position.
//! Daraus folgt für jeden Strom mit Geheimnis: **Fund oder `unchecked`, nie
//! stumm** — und **nie `unchecked` an einem Bildfilter**.
//!
//! Eigene Kodierungstabelle, eigene Kodierer und Dekoder, eigene naive Suche,
//! eigenes Kontextfenster. Das Modell sagt je Strom voraus: (a) ob der
//! Klartext sichtbar ist (roh, roh entpackt, oder hinter dem entzifferbaren
//! Anfang der Kette), (b) ob eine `unchecked`-Zeile stehen muss und **wie sie
//! wörtlich lautet**, (c) wie die Fundstelle der Objektsicht heißt — Zeichen
//! für Zeichen, Beschriftung und Kontextfenster.
//!
//! Material: Ketten, die am ersten, mittleren und letzten Glied stehen
//! bleiben; `/Crypt` mit `/Identity`; `/Filter` als Liste mit einem Element;
//! Groß-/Kleinschreibungsfehler; alle Kurzformen; `/Filter []`; `/Filter` als
//! Verweis auf eine Liste von Verweisen.

mod common;

use common::{lzw_encode, page, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_pdf::leaks_many_within;

// ---------------------------------------------------------------------------
// Meine Tabelle
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Art {
    /// Ein Filter, den das Programm dekodieren muss.
    Bekannt,
    /// Ein Bildfilter: benannter blinder Fleck, keine Meldung.
    Bild,
    /// Alles andere: muss in `unchecked` stehen.
    Fremd,
}

fn art(name: &str) -> Art {
    match name {
        "FlateDecode" | "Fl" | "LZWDecode" | "LZW" | "ASCIIHexDecode" | "AHx" | "ASCII85Decode"
        | "A85" | "RunLengthDecode" | "RL" => Art::Bekannt,
        "DCTDecode" | "DCT" | "JPXDecode" | "CCITTFaxDecode" | "CCF" | "JBIG2Decode" => Art::Bild,
        _ => Art::Fremd,
    }
}

// ---------------------------------------------------------------------------
// Meine Kodierer (zum Bauen) und Dekoder (zum Vorhersagen)
// ---------------------------------------------------------------------------

fn zlib(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("komprimierbar");
    e.finish().expect("komprimierbar")
}

fn ahx(data: &[u8]) -> Vec<u8> {
    let d = b"0123456789ABCDEF";
    let mut out: Vec<u8> = data
        .iter()
        .flat_map(|b| [d[(b >> 4) as usize], d[(b & 15) as usize]])
        .collect();
    out.push(b'>');
    out
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

fn rl(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in data.chunks(100) {
        out.push(c.len() as u8 - 1);
        out.extend_from_slice(c);
    }
    out.push(128);
    out
}

/// Kodiert `data` für den Filter `name`; ein fremder oder ein Bildfilter
/// ist im Modell ein Durchreicher (`/Crypt /Identity` ist genau das, und für
/// ein Bild spielt der Klartext die Rolle der „Bilddaten“).
fn enc(name: &str, data: &[u8]) -> Vec<u8> {
    match name {
        "FlateDecode" | "Fl" => zlib(data),
        "LZWDecode" | "LZW" => lzw_encode(data),
        "ASCIIHexDecode" | "AHx" => ahx(data),
        "ASCII85Decode" | "A85" => a85(data),
        "RunLengthDecode" | "RL" => rl(data),
        _ => data.to_vec(),
    }
}

fn dec_inflate(data: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    let _ = flate2::read::ZlibDecoder::new(data).read_to_end(&mut out);
    out
}

fn dec_ahx(data: &[u8]) -> Vec<u8> {
    let mut n = Vec::new();
    for &b in data {
        if b == b'>' {
            break;
        }
        let v = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => continue,
        };
        n.push(v);
    }
    if n.len() % 2 == 1 {
        n.push(0);
    }
    n.chunks(2).map(|c| (c[0] << 4) | c[1]).collect()
}

fn dec_a85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut g = [0u8; 5];
    let mut c = 0usize;
    for &b in data {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'~' {
            break;
        }
        if b == b'z' && c == 0 {
            out.extend_from_slice(&[0; 4]);
            continue;
        }
        g[c] = b - b'!';
        c += 1;
        if c == 5 {
            let v = g.iter().fold(0u32, |a, &d| a * 85 + u32::from(d));
            out.extend_from_slice(&v.to_be_bytes());
            c = 0;
        }
    }
    if c > 1 {
        for s in g.iter_mut().skip(c) {
            *s = 84;
        }
        let v = g
            .iter()
            .fold(0u32, |a, &d| a.wrapping_mul(85).wrapping_add(u32::from(d)));
        out.extend_from_slice(&v.to_be_bytes()[..c - 1]);
    }
    out
}

fn dec_rl(data: &[u8]) -> Vec<u8> {
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

fn dec_lzw(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut d = weezl::decode::Decoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8);
    let _ = d.into_stream(&mut out).decode_all(data);
    out
}

fn dec(name: &str, data: &[u8]) -> Option<Vec<u8>> {
    Some(match name {
        "FlateDecode" | "Fl" => dec_inflate(data),
        "LZWDecode" | "LZW" => dec_lzw(data),
        "ASCIIHexDecode" | "AHx" => dec_ahx(data),
        "ASCII85Decode" | "A85" => dec_a85(data),
        "RunLengthDecode" | "RL" => dec_rl(data),
        _ => return None,
    })
}

/// Die Rohbytes, die zu `chain` gehören: das letzte Glied kodiert zuerst.
fn rohbytes(chain: &[&str], plain: &[u8]) -> Vec<u8> {
    chain.iter().rev().fold(plain.to_vec(), |d, n| enc(n, &d))
}

/// Meine Kette: (Bytes nach dem letzten angewandten Filter, angewandte).
fn praefix(chain: &[&str], raw: &[u8]) -> (Vec<u8>, usize) {
    let mut data = Vec::new();
    for (i, n) in chain.iter().enumerate() {
        let input: &[u8] = if i == 0 { raw } else { &data };
        match dec(n, input) {
            Some(next) => data = next,
            None => return (data, i),
        }
    }
    (data, chain.len())
}

/// Was die Rohsicht (Sicht 2) an einem Block zusätzlich versucht: zlib,
/// sonst rohes Deflate ab Byte 0.
fn roh_entpackt(raw: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let out = dec_inflate(raw);
    if !out.is_empty() {
        return out;
    }
    let mut d = Vec::new();
    let _ = flate2::read::DeflateDecoder::new(raw).read_to_end(&mut d);
    d
}

/// Naive Suche, von vorn.
fn sichtbar(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

fn kontext(hay: &[u8], pos: usize, len: usize) -> String {
    let start = pos.saturating_sub(24);
    let end = (pos + len + 24).min(hay.len());
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

// ---------------------------------------------------------------------------
// Das Modell
// ---------------------------------------------------------------------------

struct Vorhersage {
    gefunden: bool,
    /// Die `unchecked`-Zeile, wörtlich — oder keine.
    unchecked: Option<String>,
    /// Die Fundstelle der Objektsicht, wörtlich — falls die Kette etwas
    /// Sichtbares liefert.
    fundstelle: Option<String>,
}

fn modell(objekt: &str, chain: &[&str], raw: &[u8]) -> Vorhersage {
    let total = chain.len();
    let needle = SECRET.as_bytes();
    let (data, applied) = praefix(chain, raw);

    let roh = sichtbar(raw, needle).is_some() || sichtbar(&roh_entpackt(raw), needle).is_some();
    let im_praefix = (applied > 0).then(|| sichtbar(&data, needle)).flatten();

    let label = if applied == total {
        format!("dekodiert: {}", chain.join("+"))
    } else {
        format!(
            "dekodiert: {} — bis Filter {applied} von {total}, danach /{} unbekannt",
            chain[..applied].join("+"),
            chain[applied]
        )
    };
    let fundstelle = im_praefix.map(|pos| {
        format!(
            "{objekt} <Stream, {label}> [Inhalt, UTF-8/ASCII]: …{}…",
            kontext(&data, pos, needle.len())
        )
    });

    let unchecked = match chain.get(applied) {
        Some(name) if art(name) == Art::Fremd && applied == 0 => Some(format!(
            "{objekt} <Stream>: gar nicht dekodiert — /{name} ist hier kein bekannter Filter \
             (Glied 1 von {total}); gelesen sind nur die rohen, gepackten Bytes"
        )),
        Some(name) if art(name) == Art::Fremd => Some(format!(
            "{objekt} <Stream>: nur bis Filter {applied} von {total} dekodiert — /{name} ist \
             hier kein bekannter Filter; was dahinter steht, hat keine Sicht gelesen"
        )),
        _ => None,
    };

    Vorhersage {
        gefunden: roh || im_praefix.is_some(),
        unchecked,
        fundstelle,
    }
}

// ---------------------------------------------------------------------------
// Material
// ---------------------------------------------------------------------------

fn nutzlast() -> Vec<u8> {
    format!("BT /F1 10 Tf 72 700 Td (Notiz zur IBAN {SECRET}) Tj ET").into_bytes()
}

/// Ein Fall: die effektive Kette (für das Modell), der `/Filter`-Wert, wie er
/// in die Datei geschrieben wird (als Funktion, damit er Verweise auf Objekte
/// bauen kann), und optional `/DecodeParms`.
struct Fall {
    name: &'static str,
    chain: Vec<&'static str>,
    filter: Box<dyn Fn(&mut common::Doc) -> Object>,
    parms: Option<Object>,
}

fn namen(chain: &[&str]) -> Object {
    match chain.len() {
        1 => Object::Name(chain[0].as_bytes().to_vec()),
        _ => Object::Array(
            chain
                .iter()
                .map(|f| Object::Name(f.as_bytes().to_vec()))
                .collect(),
        ),
    }
}

fn einfach(name: &'static str, chain: &[&'static str]) -> Fall {
    let c = chain.to_vec();
    Fall {
        name,
        chain: chain.to_vec(),
        filter: Box::new(move |_| namen(&c)),
        parms: None,
    }
}

/// Immer als Liste, auch mit einem Element.
fn als_liste(name: &'static str, chain: &[&'static str]) -> Fall {
    let c = chain.to_vec();
    Fall {
        name,
        chain: chain.to_vec(),
        filter: Box::new(move |_| {
            Object::Array(
                c.iter()
                    .map(|f| Object::Name(f.as_bytes().to_vec()))
                    .collect(),
            )
        }),
        parms: None,
    }
}

/// `/Filter N 0 R` → `[a 0 R b 0 R …]`, jedes Element ein Verweis auf einen
/// Namen (PDF 32000-1, 7.3.8.2 erlaubt beides).
fn ueber_verweise(name: &'static str, chain: &[&'static str]) -> Fall {
    let c = chain.to_vec();
    Fall {
        name,
        chain: chain.to_vec(),
        filter: Box::new(move |d| {
            let refs: Vec<Object> = c
                .iter()
                .map(|f| Object::Reference(d.add(Object::Name(f.as_bytes().to_vec()))))
                .collect();
            Object::Reference(d.add(Object::Array(refs)))
        }),
        parms: None,
    }
}

fn crypt_identity(name: &'static str, chain: &[&'static str]) -> Fall {
    let c = chain.to_vec();
    let mut parms: Vec<Object> = vec![Object::Dictionary(dictionary! {
        "Type" => "CryptFilterDecodeParms",
        "Name" => "Identity",
    })];
    parms.extend(std::iter::repeat_n(Object::Null, chain.len() - 1));
    Fall {
        name,
        chain: chain.to_vec(),
        filter: Box::new(move |_| namen(&c)),
        parms: Some(if parms.len() == 1 {
            parms.remove(0)
        } else {
            Object::Array(parms)
        }),
    }
}

fn faelle() -> Vec<Fall> {
    vec![
        // Wo die Kette stehen bleibt: erstes, mittleres, letztes Glied.
        einfach("fremd allein", &["R2Fremd"]),
        einfach("fremd zuerst", &["R2Fremd", "FlateDecode"]),
        einfach(
            "fremd zuerst, dahinter ASCII85",
            &["R2Fremd", "ASCII85Decode"],
        ),
        einfach(
            "fremd mitten",
            &["ASCIIHexDecode", "R2Fremd", "FlateDecode"],
        ),
        einfach(
            "fremd zuletzt",
            &["ASCIIHexDecode", "FlateDecode", "R2Fremd"],
        ),
        einfach(
            "fremd zuletzt, hinter LZW",
            &["ASCII85Decode", "LZWDecode", "R2Fremd"],
        ),
        // /Crypt mit /Identity: normgerechter Durchreicher.
        crypt_identity("Crypt allein", &["Crypt"]),
        crypt_identity("Crypt vor Flate", &["Crypt", "FlateDecode"]),
        crypt_identity("Crypt vor ASCII85", &["Crypt", "ASCII85Decode"]),
        // Liste mit einem Element.
        als_liste("[Flate]", &["FlateDecode"]),
        als_liste("[ASCII85]", &["ASCII85Decode"]),
        als_liste("[fremd]", &["R2Fremd"]),
        als_liste("[DCT]", &["DCTDecode"]),
        // Groß-/Kleinschreibung: kein bekannter Filter, kein Bildfilter.
        einfach("/flatedecode", &["flatedecode"]),
        einfach("/FLATEDECODE", &["FLATEDECODE"]),
        einfach("/Ascii85Decode", &["Ascii85Decode"]),
        einfach("/dctdecode", &["dctdecode"]),
        einfach("[/AHx /flatedecode]", &["AHx", "flatedecode"]),
        // Kurzformen.
        einfach("/Fl", &["Fl"]),
        einfach("/AHx", &["AHx"]),
        einfach("/A85", &["A85"]),
        einfach("/LZW", &["LZW"]),
        einfach("/RL", &["RL"]),
        einfach("/CCF", &["CCF"]),
        einfach("/DCT", &["DCT"]),
        einfach("[/AHx /Fl]", &["AHx", "Fl"]),
        einfach("[/A85 /LZW]", &["A85", "LZW"]),
        einfach("[/RL /Fl /AHx]", &["RL", "Fl", "AHx"]),
        einfach("[/Fl /DCT]", &["Fl", "DCT"]),
        einfach("[/A85 /CCF]", &["A85", "CCF"]),
        einfach("[/AHx /Fl /JPXDecode]", &["AHx", "Fl", "JPXDecode"]),
        einfach("[/Fl /JBIG2Decode]", &["Fl", "JBIG2Decode"]),
        einfach("[/RL /CCITTFaxDecode]", &["RL", "CCITTFaxDecode"]),
        // Lange Namen, alle fünf.
        einfach(
            "alle fünf",
            &[
                "ASCIIHexDecode",
                "ASCII85Decode",
                "RunLengthDecode",
                "FlateDecode",
                "LZWDecode",
            ],
        ),
        // Leere Liste: eine Kette der Länge 0.
        als_liste("[]", &[]),
        // Verweis auf eine Liste von Verweisen.
        ueber_verweise(
            "Verweis → [Verweis Verweis]",
            &["ASCIIHexDecode", "FlateDecode"],
        ),
        ueber_verweise("Verweis → [Verweis]", &["ASCII85Decode"]),
        ueber_verweise("Verweis → [Verweis fremd]", &["FlateDecode", "R2Fremd"]),
        ueber_verweise("Verweis → [fremd Verweis]", &["R2Fremd", "FlateDecode"]),
        ueber_verweise("Verweis → [Verweis Bild]", &["ASCII85Decode", "DCTDecode"]),
    ]
}

/// Baut die Datei zu einem Fall: Objekt N ist der Strom; zurück kommen die
/// Datei, die Objekt-Bezeichnung und die Rohbytes.
fn datei(fall: &Fall) -> (Vec<u8>, String, Vec<u8>) {
    let raw = rohbytes(&fall.chain, &nutzlast());
    let mut d = page(&["harmlos"]);
    let filter = (fall.filter)(&mut d);
    let mut dict = dictionary! { "Filter" => filter };
    if let Some(p) = &fall.parms {
        dict.set("DecodeParms", p.clone());
    }
    let id: ObjectId = d.add(Object::Stream(
        Stream::new(dict, raw.clone()).with_compression(false),
    ));
    d.catalog_set("R2Extra", Object::Reference(id));
    (d.finish(), format!("Objekt {} {}", id.0, id.1), raw)
}

// ---------------------------------------------------------------------------
// Der Vergleich
// ---------------------------------------------------------------------------

/// **Jeder** Fall: Fund oder `unchecked`, nie stumm; die `unchecked`-Zeile
/// wörtlich wie das Modell; nie `unchecked` an einem Bildfilter; und wo die
/// Kette etwas Sichtbares liefert, die Fundstelle der Objektsicht Zeichen für
/// Zeichen.
///
/// Mutationsnachweise (gefahren, je in `audit_bytes::decode_stream`):
/// * den Zweig `… && applied == 0` gestrichen → rot („fremd allein“,
///   „fremd zuerst“, „Crypt …“, „/flatedecode“: keine `unchecked`-Zeile);
/// * `is_image_filter` durch `applied + 1 == total` ersetzt (Entscheidung an
///   der Position) → rot („[/Fl /DCT]“ meldet, „fremd zuletzt“ schweigt);
/// * `filters::is_image_filter` liefert immer `false` (Bildfilterliste leer)
///   → rot („[DCT]“, „/CCF“, „[/Fl /DCT]“ …: `unchecked` an einem Bildfilter).
#[test]
fn r2_modell_fund_oder_unchecked_nie_stumm() {
    let mut geprueft = 0usize;
    let mut mit_fund = 0usize;
    let mut mit_unchecked = 0usize;
    for fall in faelle() {
        let (pdf, objekt, raw) = datei(&fall);
        let soll = modell(&objekt, &fall.chain, &raw);
        let ist = leaks_many_within(&pdf, &[SECRET], u64::MAX);
        let gefunden = !ist.findings[0].is_empty();

        assert!(
            gefunden || !ist.unchecked.is_empty(),
            "{}: STUMM — weder Fund noch unchecked: {:#?}",
            fall.name,
            ist
        );
        assert_eq!(
            gefunden, soll.gefunden,
            "{}: Fund weicht vom Modell ab (ist {gefunden}, soll {}): {:#?}",
            fall.name, soll.gefunden, ist
        );
        assert_eq!(
            ist.unchecked,
            soll.unchecked.iter().cloned().collect::<Vec<_>>(),
            "{}: unchecked weicht vom Modell ab",
            fall.name
        );
        if let Some(name) = fall.chain.iter().find(|n| art(n) != Art::Bekannt) {
            if art(name) == Art::Bild {
                assert!(
                    ist.unchecked.is_empty(),
                    "{}: unchecked an einem Bildfilter: {:#?}",
                    fall.name,
                    ist.unchecked
                );
            }
        }
        if let Some(fundstelle) = &soll.fundstelle {
            assert!(
                ist.findings[0].contains(fundstelle),
                "{}: die Fundstelle der Objektsicht fehlt oder lautet anders.\n\
                 soll: {fundstelle}\nist:  {:#?}",
                fall.name,
                ist.findings[0]
            );
        }
        geprueft += 1;
        mit_fund += usize::from(gefunden);
        mit_unchecked += usize::from(!ist.unchecked.is_empty());
    }
    // Das Material trägt: beide Ausgänge kommen vor, und nicht nur einer.
    assert!(geprueft >= 40, "{geprueft} Fälle");
    assert!(
        mit_fund >= 25 && mit_unchecked >= 12,
        "{mit_fund} Funde, {mit_unchecked} unchecked"
    );
}

/// Das Modell selbst, gegen bekannte Proben — sonst wäre der Vergleich oben
/// ein Vergleich mit einem Zufallsgenerator.
#[test]
fn r2_das_modell_ist_kalibriert() {
    let plain = nutzlast();
    for n in [
        "FlateDecode",
        "Fl",
        "LZWDecode",
        "AHx",
        "A85",
        "RL",
        "RunLengthDecode",
    ] {
        assert_eq!(
            dec(n, &enc(n, &plain)).as_deref(),
            Some(plain.as_slice()),
            "{n}"
        );
    }
    let chain = ["AHx", "Fl", "RL"];
    let raw = rohbytes(&chain, &plain);
    assert_eq!(praefix(&chain, &raw), (plain.clone(), 3));
    let chain = ["AHx", "R2Fremd", "Fl"];
    let raw = rohbytes(&chain, &plain);
    assert_eq!(praefix(&chain, &raw), (zlib(&plain), 1));
    assert_eq!(sichtbar(b"xx DE89 yy", b"DE89"), Some(3));
    assert_eq!(roh_entpackt(&zlib(&plain)), plain);
    assert_eq!(kontext(b"abc\x01def", 3, 1), "abc.def");
}
