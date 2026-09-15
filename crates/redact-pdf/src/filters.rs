//! Stromfilter, die der Schwärzer selbst dekodiert — mit Obergrenze.
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
//! nicht. Dazu löst `lopdf` keinen Verweis auf: weder in `/Filter` noch in
//! `/DecodeParms` (Befunde G1-C1, G1-C2).
//!
//! Das ehrliche Orakel ([`crate::audit_bytes`]) und der Schwärzer müssen
//! dasselbe lesen: eine Datei, die das Orakel dekodieren kann, muss der
//! Schwärzer auch dekodieren können, sonst lehnt er ab, was gar nicht
//! gefährlich ist. Beide lesen deshalb über **dieses** Modul.
//!
//! # Obergrenze
//!
//! [`decoded_content_within`] entpackt keinen Filter über `limit` Byte
//! hinaus — und zwar **beim** Entpacken, nicht erst hinterher gemessen:
//! Flate liest über `Read::take`, LZW schreibt in einen Puffer, der ab der
//! Grenze ablehnt, ASCII85 und RunLength prüfen in ihrer Schleife. Eine
//! Dekompressionsbombe (1 GiB Nullen, 1 MB gepackt) belegt so höchstens
//! `limit + 1` Byte, bevor sie als [`Oversize`] zurückkommt. Das ist die
//! Grundlage des Budgets in [`crate::audit_bytes::leaks_many_within`].
//! [`decoded_content`] ist dieselbe Kette ohne Grenze — der Schwärzer
//! bekommt seine Datei bereits durch [`crate::document::prescan`] gedeckelt.
//!
//! # Zwei Leser, zwei Ansprüche
//!
//! [`decoded_content_within`] ist **streng**: bleibt ein Glied der Kette
//! unbekannt, gibt es die ganze Kette auf (`Ok(None)`). Das ist die Sicht des
//! Interpreters — ein halb dekodierter Strom ist keine PDF-Syntax, und eine
//! Seite daraus wäre erfunden.
//!
//! [`decoded_prefix_within`] ist **nachsichtig**: es dekodiert so weit, wie es
//! kommt, und sagt, wo es stehen blieb. Das ist die Sicht des Orakels — es
//! will sehen, was sichtbar ist. Ein Klartext im Flate-Teil von
//! `[/ASCIIHexDecode /FlateDecode /DCTDecode]` ist genau so zu finden.
//!
//! # Was hier bewusst nicht steht
//!
//! * **Der LZW-Dekoder** selbst ist `weezl`, dieselbe Bibliothek mit
//!   denselben Einstellungen wie bei `lopdf` (`Msb`, 9 Bit, `EarlyChange`
//!   aus `/DecodeParms`); nur der Ausgabepuffer ist hier begrenzt.
//! * **Der PNG-Prädiktor** ist `lopdf::filters::png::decode_frame`; gelesen
//!   werden `/Predictor`, `/Columns`, `/Colors`, `/BitsPerComponent` mit den
//!   Vorgaben von `lopdf`, damit beide dasselbe Ergebnis liefern
//!   (`ein_praediktor_liest_sich_wie_bei_lopdf`).
//! * **Bildfilter** (`DCTDecode`, `JPXDecode`, `CCITTFaxDecode`, `JBIG2Decode`)
//!   sind hier ein Fehler wie bei `lopdf`. Sie stehen nie an einem
//!   Seiteninhalt; Bilder liest [`crate::image`] mit eigenem Weg.
//! * **Verkürzte Ströme** liefern wie bei `lopdf` das Teilergebnis. Ob ein
//!   Teilergebnis genügt, entscheidet nicht der Dekoder, sondern der Leser
//!   dahinter — [`crate::content::scan_page`] lehnt einen Strom ab, der sich
//!   nicht vollständig in Operationen zerlegen lässt.

use std::io::{self, Read, Write};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

/// Ein Strom wurde **nicht** entpackt: er hätte mehr als die zugelassenen
/// Bytes ergeben. Was bis dahin entpackt war, ist verworfen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Oversize;

/// Der ausgepackte Inhalt eines Stroms — über die ganze Filterkette, ohne
/// Grenze.
///
/// `None`, wenn ein Filter hier nicht bekannt ist. Ein Strom ohne `/Filter`
/// kommt unverändert zurück.
///
/// `doc` löst Verweise auf: `/Filter` (den Wert wie jedes Element einer
/// Liste) und `/DecodeParms` (die Liste, jeden Eintrag darin und jeden Wert
/// im Dictionary). Alles davon darf ein Verweis sein (PDF 32000-1, 7.3.8.2);
/// `lopdf` löst keinen davon auf — ein Verweis las sich dort wie „kein
/// Filter“ (Rohbytes als Klartext, Seite abgelehnt) oder wie „kein
/// Prädiktor“ / „`Columns` 1“ (Filterbytes im Text).
pub fn decoded_content(doc: &Document, stream: &Stream) -> Option<Vec<u8>> {
    // Ohne Grenze gibt es kein `Oversize`.
    decoded_content_within(doc, stream, usize::MAX).unwrap_or(None)
}

/// Wie [`decoded_content`], aber kein Filter der Kette erzeugt mehr als
/// `limit` Byte: darüber bricht das Entpacken ab und liefert [`Oversize`].
///
/// `Ok(None)` heißt wie oben „unbekannter Filter“. Ein Strom ohne `/Filter`
/// wird nicht gemessen — seine Bytes stehen ohnehin in der Datei.
pub fn decoded_content_within(
    doc: &Document,
    stream: &Stream,
    limit: usize,
) -> Result<Option<Vec<u8>>, Oversize> {
    let (data, applied, total) = decode_chain(doc, stream, limit)?;
    Ok((applied == total).then_some(data))
}

/// Der **Orakelweg**: dekodiert die Filterkette so weit, wie sie sich
/// dekodieren lässt, und sagt, wo sie stehen blieb.
///
/// Rückgabe: die Bytes nach dem letzten angewandten Filter und die **Anzahl
/// angewandter Filter**. Ist sie so groß wie die Kette, lief sie ganz durch;
/// ist sie kleiner, war das nächste Glied unbekannt (`names[applied]` nennt
/// es).
///
/// # Was bei `applied == 0` zurückkommt
///
/// Ein Strom **ohne** brauchbares `/Filter` (Schlüssel fehlt, Verweis ins
/// Leere, leere Liste) hat eine Kette der Länge 0: sie ist vollständig
/// gelaufen, und die Rohbytes kommen zurück.
///
/// Ist dagegen schon das **erste** Glied einer nicht leeren Kette unbekannt,
/// kommt ein **leerer** Puffer zurück — nicht die Rohbytes. Die hat der
/// Aufrufer ohnehin (`stream.content`), und eine Kopie davon wäre eine
/// Stromgröße Speicher, die niemand bestellt hat und die `limit` nicht deckt:
/// bis Fix-Runde 6 klonte diese Funktion die Rohbytes, **bevor** sie den
/// ersten Filter kannte, und gab bei `/Filter /DCTDecode` 8 000 000 Byte
/// zurück, obwohl `limit` 16 war (gemessen an einem 64-MiB-Strom: 205 MB
/// statt 138 MB Spitzenbelegung, `tests/zf_q2_teildekoder.rs`).
///
/// # Warum nicht über [`decoded_content_within`]
///
/// Die beiden Leser haben verschiedene Ansprüche. Der **Interpreter**
/// ([`crate::content`], [`crate::redact`]) darf einen halb dekodierten Strom
/// nicht als Seiteninhalt lesen: was hinter dem unbekannten Glied steht, ist
/// keine PDF-Syntax, und eine Seite, die daraus gelesen würde, wäre erfunden.
/// Dort ist „unbekannter Filter“ zu Recht ein Abbruch mit Warnung, und
/// [`decoded_content_within`] bleibt streng.
///
/// Das **Orakel** ([`crate::audit_bytes`]) will dagegen sehen, was sichtbar
/// ist. Bis Commit `f982c12` hatte es einen eigenen Dekoder, der am
/// unbekannten Filter abbrach und das bis dahin Entpackte **behielt**; der
/// Umbau auf dieses Modul warf es weg, und ein Klartext im Flate-Teil von
/// `[/ASCIIHexDecode /FlateDecode /DCTDecode]` war nicht mehr zu finden.
/// Diese Funktion stellt genau das wieder her — ohne die Strenge des
/// Interpreters anzurühren.
pub fn decoded_prefix_within(
    doc: &Document,
    stream: &Stream,
    limit: usize,
) -> Result<(Vec<u8>, usize), Oversize> {
    let (data, applied, _) = decode_chain(doc, stream, limit)?;
    Ok((data, applied))
}

/// Die Filterkette, so weit sie läuft: (Bytes, angewandte Filter, Kettenlänge).
///
/// Geklont wird erst, **nachdem** ein Filter wirklich etwas geliefert hat:
/// die Eingabe des ersten Gliedes sind die Rohbytes des Stroms, geborgt.
/// Wer gar nichts entpacken konnte, bekommt deshalb einen leeren Puffer und
/// nicht eine zweite Kopie des Stroms (siehe [`decoded_prefix_within`]).
fn decode_chain(
    doc: &Document,
    stream: &Stream,
    limit: usize,
) -> Result<(Vec<u8>, usize, usize), Oversize> {
    // Ohne `/Filter` (oder mit dem Wert `null`) ist der Strom nicht
    // gefiltert, und die Rohbytes sind das Einzige, was es zu lesen gibt.
    // Ein Wert, der da steht und **kein** Name ist, ist etwas anderes:
    // `filter_names` gibt dafür ein namenloses Glied zurück.
    let Some(filters) = filter_names(doc, &stream.dict) else {
        return Ok((stream.content.clone(), 0, 0));
    };
    let total = filters.len();
    if total == 0 {
        // Eine leere Kette ist vollständig gelaufen.
        return Ok((stream.content.clone(), 0, 0));
    }
    let mut data: Option<Vec<u8>> = None;
    for (index, filter) in filters.iter().enumerate() {
        let parms = decode_parms(doc, &stream.dict, index);
        let input = data.as_deref().unwrap_or(&stream.content);
        let Some(next) = decode_one(filter, input, parms.as_ref(), limit)? else {
            return Ok((data.unwrap_or_default(), index, total));
        };
        data = Some(next);
    }
    Ok((data.unwrap_or_default(), total, total))
}

/// Ein Filter, den dieses Modul **bewusst** nicht dekodiert: Bilddaten.
///
/// Text in einem Rasterbild ist ein im Modulkopf von [`crate::audit_bytes`]
/// benannter blinder Fleck — er ist es bei `/DCTDecode` allein genauso wie am
/// Ende einer Kette. Das Orakel meldet ihn deshalb **nicht** als „nicht
/// geprüft“: `[/ASCII85Decode /DCTDecode]` ist die gewöhnliche Ausgabe eines
/// Distillers, und eine Antwort, die daran „unvollständig“ sagt, säße an
/// jeder zweiten Datei mit einem Foto. Ein Filtername, den niemand kennt, ist
/// etwas anderes: dahinter kann alles stehen.
pub(crate) fn is_image_filter(name: &[u8]) -> bool {
    matches!(
        name,
        b"DCTDecode" | b"DCT" | b"JPXDecode" | b"CCITTFaxDecode" | b"CCF" | b"JBIG2Decode"
    )
}

/// Die Filterkette aus `/Filter` — aufgelöst, in Dekodierreihenfolge.
///
/// Wie `lopdf::Stream::filters`, nur dass der Wert und jedes Element einer
/// Liste ein Verweis sein dürfen (`/Filter 5 0 R`, Befund G1-C1).
///
/// `None` heißt **kein Filter**: der Schlüssel fehlt, oder sein Wert ist
/// `null` — nach PDF 32000-1, 7.3.9 ist ein Eintrag mit dem Wert `null` wie
/// ein fehlender. Die Rohbytes sind dann der ganze Inhalt, und jeder Leser
/// sieht dasselbe.
///
/// # Das namenlose Glied
///
/// Ein Glied, das sich **nicht** zu einem Namen auflösen lässt — ein Verweis
/// ins Leere (`[/LZWDecode 999 0 R]`), ein ausgeschriebenes `null`, eine
/// Zahl, eine Zeichenkette —, steht als **leerer** Name in der Kette und
/// bleibt damit ein Glied. Bis Fix-Runde 6 sammelte diese Funktion die Liste
/// mit `collect::<Option<Vec<_>>>()`: ein solches Glied verwarf die **ganze**
/// Kette, und der Aufrufer sah denselben Zustand wie bei einem Strom ganz
/// ohne `/Filter` — kein Filter, nichts zu entpacken, nichts zu melden.
/// `/Filter [/LZWDecode null]` über LZW-gepacktem Klartext kam so als „nicht
/// gefunden“ mit Rückgabewert 0 zurück, `/Filter [/LZWDecode /R2Fremd]` mit
/// Rückgabewert 3 (Befund R2-C, `tests/zg_r2_unbrauchbarer_filterwert.rs`).
///
/// Ein leerer Name kann kein bekannter Filter sein ([`decode_one`] kennt ihn
/// nicht, [`is_image_filter`] auch nicht): die Kette bleibt dort stehen wie
/// an jedem anderen unbekannten Namen. Der **strenge** Leser gibt sie damit
/// auf (`Ok(None)` — er täte es auch vorher, nur mit den Rohbytes als
/// vermeintlichem Klartext), der **nachsichtige** dekodiert bis dorthin und
/// sagt, woran er hängen blieb.
///
/// Warum der Verweis ins Leere **nicht** wie `null` behandelt wird, obwohl
/// 7.3.9 beide gleichsetzt: was an `999 0 R` steht, ist aus dieser Datei
/// nicht zu erfahren. Ein Leser mit einer anderen Querverweistabelle — eine
/// ältere Revision, eine Wiederherstellung — kann dort einen Filternamen
/// finden. „Ich weiß es nicht“ ist etwas anderes als „da ist nichts“.
pub(crate) fn filter_names(doc: &Document, dict: &Dictionary) -> Option<Vec<Vec<u8>>> {
    /// Ein Glied, das kein Name ist. Ein leerer Name ist nie ein bekannter
    /// Filter, und `/` (der leere Name) als Filter wäre es ebenso wenig.
    const NAMENLOS: Vec<u8> = Vec::new();

    let value = dict.get(b"Filter").ok()?;
    let Ok((_, filter)) = doc.dereference(value) else {
        // Verweis ins Leere (oder im Kreis): ein Glied, dessen Name hier
        // niemand kennt.
        return Some(vec![NAMENLOS]);
    };
    match filter {
        // 7.3.9: ein Eintrag mit dem Wert `null` ist wie ein fehlender.
        Object::Null => None,
        Object::Name(name) => Some(vec![name.clone()]),
        Object::Array(items) => Some(
            items
                .iter()
                .map(|item| {
                    doc.dereference(item)
                        .ok()
                        .and_then(|(_, o)| o.as_name().ok())
                        .map_or(NAMENLOS, <[u8]>::to_vec)
                })
                .collect(),
        ),
        // Zahl, Zeichenkette, Dictionary, Strom: ein Wert steht da, ein
        // Filtername ist es nicht.
        _ => Some(vec![NAMENLOS]),
    }
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

/// Ein einzelner Filter mit seinem `/DecodeParms`-Eintrag. `Ok(None)` für
/// alles, was hier nicht hingehört (Bildfilter, Unbekanntes, und das
/// namenlose Glied aus [`filter_names`] — ein leerer Name trifft keinen Arm).
fn decode_one(
    filter: &[u8],
    data: &[u8],
    parms: Option<&Dictionary>,
    limit: usize,
) -> Result<Option<Vec<u8>>, Oversize> {
    let out = match filter {
        b"FlateDecode" | b"Fl" => inflate_within(data, limit)?,
        b"LZWDecode" | b"LZW" => lzw_within(data, parms, limit)?,
        b"ASCIIHexDecode" | b"AHx" => ascii_hex_decode(data),
        b"ASCII85Decode" | b"A85" => ascii85_decode_within(data, limit)?,
        b"RunLengthDecode" | b"RL" => run_length_decode_within(data, limit)?,
        _ => return Ok(None),
    };
    // ASCIIHex halbiert; die anderen prüfen in ihrer Schleife. Trotzdem noch
    // einmal am Ergebnis, damit die Zusicherung nicht am Dekoder hängt.
    if out.len() > limit {
        return Err(Oversize);
    }
    Ok(match filter {
        // Ein Prädiktor gehört zu Flate und LZW — so liest es auch `lopdf`.
        b"FlateDecode" | b"Fl" | b"LZWDecode" | b"LZW" => png_predictor(out, parms),
        _ => Some(out),
    })
}

/// Der `/DecodeParms`-Eintrag, der zum `index`-ten Filter gehört — aufgelöst.
///
/// Die Liste, der Eintrag darin und jeder Wert im Dictionary dürfen Verweise
/// sein. Ein einzelnes Dictionary (die Form bei genau einem Filter) gilt für
/// jeden Index — so liest es auch `lopdf`, und ein Erzeuger, der zu einer
/// Kette nur ein Dictionary schreibt, meint damit den Filter, der Parameter
/// hat.
///
/// Die Werte werden aufgelöst, weil `lopdf` sie unaufgelöst liest
/// (`decompress_predictor`: `as_i64` auf `/Predictor` und `/Columns`, sonst
/// Vorgabe): `/Columns 7 0 R` hieße dort „1 Spalte“, `/Predictor 7 0 R`
/// „kein Prädiktor“ — die Filterbytes blieben im Text (Befund G1-C2).
/// `Document::dereference` folgt Verweisketten bis zu seiner Grenze
/// (`DEREF_LIMIT`, 128) und meldet einen Kreis als Fehler; ein Wert, der sich
/// nicht auflösen lässt, bleibt stehen und wirkt wie bei `lopdf` als Vorgabe.
fn decode_parms(doc: &Document, dict: &Dictionary, index: usize) -> Option<Dictionary> {
    let parms = dict.get(b"DecodeParms").or_else(|_| dict.get(b"DP")).ok()?;
    let (_, parms) = doc.dereference(parms).ok()?;
    let entry = match parms {
        Object::Array(items) => items.get(index)?,
        other => other,
    };
    let entry = doc.dereference(entry).ok()?.1.as_dict().ok()?;
    let mut resolved = Dictionary::new();
    for (key, value) in entry.iter() {
        let value = doc.dereference(value).map_or(value, |(_, o)| o);
        resolved.set(key.clone(), value.clone());
    }
    Some(resolved)
}

/// Entfernt den PNG-Prädiktor — genau wie `lopdf::Stream::decompress_predictor`:
/// `/Predictor` 10–15 mit `/Columns`, `/Colors`, `/BitsPerComponent` (Vorgaben
/// 1, 1, 8); alles andere lässt die Daten unverändert. `None`, wenn der
/// Zeilendekoder die Daten ablehnt — bei `lopdf` ist das derselbe Fehler.
fn png_predictor(data: Vec<u8>, parms: Option<&Dictionary>) -> Option<Vec<u8>> {
    let Some(parms) = parms else {
        return Some(data);
    };
    let int = |key: &[u8], default: i64| {
        parms
            .get(key)
            .ok()
            .and_then(|o| o.as_i64().ok())
            .unwrap_or(default)
    };
    if !(10..=15).contains(&int(b"Predictor", 1)) {
        return Some(data);
    }
    let columns = int(b"Columns", 1).max(1) as usize;
    let colors = int(b"Colors", 1).max(1) as usize;
    let bits = int(b"BitsPerComponent", 8).max(8) as usize;
    lopdf::filters::png::decode_frame(&data, colors * bits / 8, columns).ok()
}

/// Liest höchstens `limit` Byte; ein Byte mehr heißt [`Oversize`].
///
/// Ein Lesefehler unterwegs (verkürzter oder kaputter Strom) zählt nicht:
/// das Teilergebnis bleibt, wie bei `lopdf`.
pub(crate) fn read_within(reader: impl Read, limit: usize) -> Result<Vec<u8>, Oversize> {
    let mut out = Vec::new();
    let cap = u64::try_from(limit.saturating_add(1)).unwrap_or(u64::MAX);
    let _ = reader.take(cap).read_to_end(&mut out);
    if out.len() > limit {
        return Err(Oversize);
    }
    Ok(out)
}

/// Flate wie bei `lopdf`: zlib, bei Fehlschlag rohes Deflate hinter dem
/// 2-Byte-Kopf; ein Teilergebnis zählt. Nie mehr als `limit` Byte.
pub(crate) fn inflate_within(data: &[u8], limit: usize) -> Result<Vec<u8>, Oversize> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let out = read_within(flate2::read::ZlibDecoder::new(data), limit)?;
    if out.is_empty() && data.len() > 2 {
        return read_within(flate2::read::DeflateDecoder::new(&data[2..]), limit);
    }
    Ok(out)
}

/// Ein Schreibziel, das ab `limit` Byte ablehnt — für Dekoder, die in einen
/// `Write` schreiben (`weezl`).
struct Bounded {
    out: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl Write for Bounded {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.out.len().saturating_add(buf.len()) > self.limit {
            self.exceeded = true;
            return Err(io::Error::other("Obergrenze erreicht"));
        }
        self.out.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// `LZWDecode` über `weezl` mit den Einstellungen von `lopdf`: MSB zuerst,
/// 9-Bit-Startcodes, `/EarlyChange` (Vorgabe 1). Ein Fehler im Strom lässt
/// das Teilergebnis stehen — wie dort.
fn lzw_within(data: &[u8], parms: Option<&Dictionary>, limit: usize) -> Result<Vec<u8>, Oversize> {
    use weezl::{decode::Decoder, BitOrder};
    let early_change = parms
        .and_then(|p| p.get(b"EarlyChange").ok())
        .and_then(|o| o.as_i64().ok())
        .is_none_or(|v| v != 0);
    let mut decoder = if early_change {
        Decoder::with_tiff_size_switch(BitOrder::Msb, 8)
    } else {
        Decoder::new(BitOrder::Msb, 8)
    };
    let mut sink = Bounded {
        out: Vec::new(),
        limit,
        exceeded: false,
    };
    let _ = decoder.into_stream(&mut sink).decode_all(data);
    if sink.exceeded {
        return Err(Oversize);
    }
    Ok(sink.out)
}

/// `ASCIIHexDecode` (PDF 32000-1, 7.4.2): Leerraum wird übersprungen, `>`
/// beendet, eine ungerade letzte Ziffer zählt als `x0`. Halbiert — braucht
/// keine Grenze.
pub(crate) fn ascii_hex_decode(data: &[u8]) -> Vec<u8> {
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

pub(crate) fn hex_value(b: u8) -> Option<u8> {
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
/// beendet die Dekodierung — wie bei `lopdf`. Höchstens `limit` Byte (`z`
/// vervierfacht).
fn ascii85_decode_within(data: &[u8], limit: usize) -> Result<Vec<u8>, Oversize> {
    let mut out = Vec::with_capacity((data.len() * 4 / 5).min(limit));
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
            // Gebucht wird, was diese Gruppe wirklich liefert — vier Byte.
            if out.len() + 4 > limit {
                return Err(Oversize);
            }
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&b) {
            break;
        }
        group[count] = b - b'!';
        count += 1;
        if count == 5 {
            if out.len() + 4 > limit {
                return Err(Oversize);
            }
            out.extend_from_slice(&ascii85_group(&group));
            count = 0;
        }
    }
    if count > 1 {
        // Die angebrochene Schlussgruppe liefert `count - 1` Byte, nicht vier.
        if out.len() + count - 1 > limit {
            return Err(Oversize);
        }
        for slot in group.iter_mut().skip(count) {
            *slot = 84;
        }
        out.extend_from_slice(&ascii85_group(&group)[..count - 1]);
    }
    Ok(out)
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
/// `128` ist das Ende. Höchstens `limit` Byte (ein Lauf verhundertachtundzwanzigfacht).
fn run_length_decode_within(data: &[u8], limit: usize) -> Result<Vec<u8>, Oversize> {
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
                if out.len() + (end - i) > limit {
                    return Err(Oversize);
                }
                out.extend_from_slice(&data[i..end]);
                i = end;
            }
            _ => {
                if let Some(&b) = data.get(i) {
                    let n = 257 - length as usize;
                    if out.len() + n > limit {
                        return Err(Oversize);
                    }
                    out.extend(std::iter::repeat_n(b, n));
                }
                i += 1;
            }
        }
    }
    Ok(out)
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

    fn has_predictor(parms: Option<&Dictionary>) -> bool {
        parms
            .and_then(|d| d.get(b"Predictor").ok())
            .and_then(|p| p.as_i64().ok())
            .is_some_and(|p| p > 1)
    }

    fn run_length_decode(data: &[u8]) -> Vec<u8> {
        run_length_decode_within(data, usize::MAX).expect("ohne Grenze")
    }

    fn ascii85_decode(data: &[u8]) -> Vec<u8> {
        ascii85_decode_within(data, usize::MAX).expect("ohne Grenze")
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

    /// LZW über `weezl` liest dasselbe wie `lopdf` — auch hinter einem
    /// Filter, den `lopdf` nicht kennt.
    #[test]
    fn lzw_liest_dasselbe_wie_lopdf() {
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
    fn ein_praediktor_liest_sich_wie_bei_lopdf() {
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

    /// `/Filter 5 0 R` und `/Filter [6 0 R]`: der Filtername als Verweis
    /// (Befund G1-C1). `lopdf` liest das als „ungefiltert“.
    ///
    /// Dazu die beiden Fälle, in denen sich **nichts** auflösen lässt, und
    /// ihr Unterschied (Befund R2-C): ein Verweis ins Leere ist ein Glied
    /// ohne Namen, `null` ist gar kein Filter.
    #[test]
    fn filter_als_verweis_wird_aufgeloest() {
        let mut doc = doc();
        let name_id = doc.add_object(Object::Name(b"FlateDecode".to_vec()));
        let stream = with_filter(Object::Reference(name_id), deflate(PLAIN));
        assert!(
            stream.filters().is_err(),
            "lopdf allein: sonst prüft der Test nichts"
        );
        assert_eq!(decoded_content(&doc, &stream).as_deref(), Some(PLAIN));
        let stream = with_filter(
            Object::Array(vec![Object::Reference(name_id)]),
            deflate(PLAIN),
        );
        assert_eq!(decoded_content(&doc, &stream).as_deref(), Some(PLAIN));

        // Verweis ins Leere: ein Glied, dessen Name hier niemand kennt (seit
        // Fix-Runde 6, Befund R2-C). Der **strenge** Leser gibt die Kette auf,
        // statt die gepackten Bytes für Klartext zu halten — das tat er
        // vorher, und `scan_page` lehnte die Seite dann ohne Grund ab.
        let stream = with_filter(Object::Reference((999, 0)), deflate(PLAIN));
        assert_eq!(decoded_content(&doc, &stream), None);
        assert_eq!(
            filter_names(&doc, &stream.dict),
            Some(vec![Vec::new()]),
            "ein namenloses Glied, keine leere Kette"
        );
        // Der **nachsichtige** Leser sagt, dass er gar nicht erst anfangen
        // konnte: kein Filter angewandt, ein Glied insgesamt.
        assert_eq!(
            decoded_prefix_within(&doc, &stream, usize::MAX),
            Ok((Vec::new(), 0))
        );

        // `/Filter null` dagegen ist **kein** Filter (PDF 32000-1, 7.3.9: ein
        // Eintrag mit dem Wert `null` ist wie ein fehlender) — die Rohbytes
        // sind der ganze Inhalt, und jeder Leser sieht dasselbe.
        let stream = with_filter(Object::Null, deflate(PLAIN));
        assert_eq!(filter_names(&doc, &stream.dict), None);
        assert_eq!(decoded_content(&doc, &stream), Some(deflate(PLAIN)));
    }

    /// Werte im `/DecodeParms`-Dictionary als Verweis (Befund G1-C2) — auch
    /// über eine Kette von Verweisen; ein Kreis endet als „Vorgabe“ statt
    /// als Endlosschleife.
    #[test]
    fn werte_im_parms_dictionary_als_verweis_werden_aufgeloest() {
        let mut doc = doc();
        let columns = doc.add_object(Object::Integer(8));
        let columns_chain = doc.add_object(Object::Reference(columns));
        let predictor = doc.add_object(Object::Integer(12));
        let mut stream = with_filter("FlateDecode".into(), deflate(&png_rows()));
        stream.dict.set(
            "DecodeParms",
            dictionary! { "Predictor" => predictor, "Columns" => columns_chain },
        );
        assert_ne!(
            stream.decompressed_content().ok().as_deref(),
            Some(PLAIN),
            "lopdf allein: sonst prüft der Test nichts"
        );
        let parms = decode_parms(&doc, &stream.dict, 0).expect("Parms");
        assert_eq!(parms.get(b"Columns").and_then(Object::as_i64).ok(), Some(8));
        assert_eq!(decoded_content(&doc, &stream).as_deref(), Some(PLAIN));

        // Ein Kreis: `Columns` zeigt auf sich selbst.
        let loop_id = doc.new_object_id();
        doc.objects.insert(loop_id, Object::Reference(loop_id));
        stream.dict.set(
            "DecodeParms",
            dictionary! { "Predictor" => 12, "Columns" => loop_id },
        );
        let parms = decode_parms(&doc, &stream.dict, 0).expect("Parms");
        assert!(parms.get(b"Columns").and_then(Object::as_i64).is_err());
        // Mit `Columns` 1 liest sich der Prädiktor falsch — aber er endet.
        let _ = decoded_content(&doc, &stream);
    }

    /// Die Grenze greift **beim** Entpacken: 64 MiB Nullen (gepackt ein paar
    /// Kilobyte) kommen mit Grenze 1 MiB als `Oversize` zurück, und die
    /// Grenze genau in Stromgröße lässt den Strom durch.
    #[test]
    fn flate_wird_nur_bis_zur_grenze_entpackt() {
        let zeros = vec![0u8; 64 * 1024 * 1024];
        let stream = with_filter("FlateDecode".into(), deflate(&zeros));
        assert_eq!(
            decoded_content_within(&doc(), &stream, 1024 * 1024),
            Err(Oversize)
        );
        assert_eq!(
            decoded_content_within(&doc(), &stream, zeros.len() - 1),
            Err(Oversize)
        );
        assert_eq!(
            decoded_content_within(&doc(), &stream, zeros.len()).map(|d| d.map(|d| d.len())),
            Ok(Some(zeros.len()))
        );
        // Hinter einem eigenen Filter gilt dieselbe Grenze.
        let chain = Object::Array(vec!["ASCIIHexDecode".into(), "FlateDecode".into()]);
        let stream = with_filter(chain, hex(&deflate(&zeros)));
        assert_eq!(
            decoded_content_within(&doc(), &stream, 1024 * 1024),
            Err(Oversize)
        );
    }

    /// RunLength (ein Byte → 128), ASCII85 (`z` → 4) und LZW halten die
    /// Grenze ebenfalls ein.
    #[test]
    fn die_anderen_filter_halten_die_grenze_ein() {
        // 1000 Läufe à 128 Nullen = 128 000 Byte aus 2 000 Byte.
        let rl: Vec<u8> = [129u8, 0].repeat(1000);
        assert_eq!(run_length_decode_within(&rl, 100_000), Err(Oversize));
        assert_eq!(
            run_length_decode_within(&rl, 128_000).map(|d| d.len()),
            Ok(128_000)
        );
        let a85 = b"zzzz~>";
        assert_eq!(ascii85_decode_within(a85, 15), Err(Oversize));
        assert_eq!(ascii85_decode_within(a85, 16).map(|d| d.len()), Ok(16));
        // LZW: Clear, `A`, EOD — ein Byte; Grenze 0 lehnt ab, Grenze 1 nicht.
        let lzw = [0x80, 0x10, 0x60, 0x20];
        assert_eq!(lzw_within(&lzw, None, 0), Err(Oversize));
        assert_eq!(lzw_within(&lzw, None, 1).as_deref(), Ok(&b"A"[..]));
    }

    // -----------------------------------------------------------------
    // Fix-Runde 5
    // -----------------------------------------------------------------

    /// Kodiert `plain` mit dem jeweiligen Filter — nur für die Tests.
    fn encode(filter: &str, plain: &[u8]) -> Vec<u8> {
        match filter {
            "FlateDecode" => {
                use std::io::Write;
                let mut e =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(plain).expect("deflate");
                e.finish().expect("deflate")
            }
            "LZWDecode" => weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
                .encode(plain)
                .expect("LZW"),
            "ASCIIHexDecode" => {
                let mut out = hex_ascii_upper(plain);
                out.push(b'>');
                out
            }
            "ASCII85Decode" => ascii85(plain),
            "RunLengthDecode" => {
                // Wörtliche Läufe zu höchstens 128 Byte, dann das Endebyte.
                let mut out = Vec::new();
                for chunk in plain.chunks(128) {
                    out.push(chunk.len() as u8 - 1);
                    out.extend_from_slice(chunk);
                }
                out.push(128);
                out
            }
            other => panic!("unbekannter Filter {other}"),
        }
    }

    fn hex_ascii_upper(bytes: &[u8]) -> Vec<u8> {
        bytes
            .iter()
            .flat_map(|b| {
                let d = b"0123456789ABCDEF";
                [d[(b >> 4) as usize], d[(b & 15) as usize]]
            })
            .collect()
    }

    fn ascii85(plain: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in plain.chunks(4) {
            let mut group = [0u8; 4];
            group[..chunk.len()].copy_from_slice(chunk);
            let mut value = u32::from_be_bytes(group);
            let mut digits = [0u8; 5];
            for slot in digits.iter_mut().rev() {
                *slot = b'!' + (value % 85) as u8;
                value /= 85;
            }
            out.extend_from_slice(&digits[..chunk.len() + 1]);
        }
        out.extend_from_slice(b"~>");
        out
    }

    /// **Jeder** Filter nimmt genau `limit` Byte an und lehnt bei `limit - 1`
    /// ab — kein Filter darf gewöhnliches Material an seiner eigenen Größe
    /// scheitern lassen (Befund P1-2).
    ///
    /// `ASCII85Decode` prüfte vor jeder Fünfergruppe `out.len() + 4 > limit`
    /// und unterstellte damit, dass auch die letzte, angebrochene Gruppe vier
    /// Byte liefert; sie liefert eins bis drei. Ein Strom, dessen entpackte
    /// Länge exakt ins Restbudget passte, kam als [`Oversize`] zurück — an
    /// der Kommandozeile Rückgabewert 3 an harmlosem Material.
    ///
    /// `ASCIIHexDecode` braucht in der Schleife keine Grenze: es **halbiert**
    /// (zwei Ziffern je Byte), das Ergebnis passt also immer in die Hälfte
    /// der ohnehin schon geladenen Rohbytes, und `decode_one` misst am
    /// Ergebnis mit `>` — exakt an der Grenze. Flate (`read_within`, liest
    /// `limit + 1` und misst) und LZW (`Bounded`, bucht vor dem Schreiben)
    /// sind ebenso exakt.
    #[test]
    fn jeder_filter_nimmt_genau_seine_grenze_an() {
        let doc = doc();
        for filter in [
            "FlateDecode",
            "LZWDecode",
            "ASCIIHexDecode",
            "ASCII85Decode",
            "RunLengthDecode",
        ] {
            // Auch Längen, die kein Vielfaches von vier sind — dort lag der Fehler.
            for n in [1usize, 2, 3, 4, 5, 6, 7, 8, 9, 63, 64, 65] {
                let plain: Vec<u8> = (0..n).map(|i| b'A' + (i % 26) as u8).collect();
                let stream = with_filter(filter.into(), encode(filter, &plain));
                assert_eq!(
                    decoded_content_within(&doc, &stream, n),
                    Ok(Some(plain.clone())),
                    "{filter}: {n} Byte passen nicht in eine Grenze von {n} Byte"
                );
                if n > 0 {
                    assert_eq!(
                        decoded_content_within(&doc, &stream, n - 1),
                        Err(Oversize),
                        "{filter}: {n} Byte gehen in eine Grenze von {} Byte",
                        n - 1
                    );
                }
            }
        }
    }

    /// Der Orakelweg behält, was er entziffert hat; der Interpreterweg bleibt
    /// streng (Befund P1-1).
    #[test]
    fn der_orakelweg_behaelt_den_entzifferten_anfang() {
        let doc = doc();
        let plain = b"BT (Konto DE89) Tj ET";
        let content = encode("ASCIIHexDecode", &encode("FlateDecode", plain));
        let stream = with_filter(
            Object::Array(vec![
                "ASCIIHexDecode".into(),
                "FlateDecode".into(),
                "DCTDecode".into(),
            ]),
            content,
        );

        // Streng: die Kette lief nicht durch, also gibt es keinen Inhalt.
        assert_eq!(decoded_content_within(&doc, &stream, usize::MAX), Ok(None));

        // Nachsichtig: zwei Filter liefen, und ihr Ergebnis ist der Klartext.
        let (data, applied) =
            decoded_prefix_within(&doc, &stream, usize::MAX).expect("kein Oversize");
        assert_eq!(applied, 2);
        assert_eq!(data, plain);
    }

    /// Schon das erste Glied unbekannt: nichts entpackt — und deshalb auch
    /// **keine** Kopie der Rohbytes. Die stehen beim Aufrufer.
    #[test]
    fn der_orakelweg_meldet_wenn_er_gar_nicht_erst_anfangen_konnte() {
        let doc = doc();
        let stream = with_filter("DCTDecode".into(), b"\xff\xd8roh".to_vec());
        let (data, applied) =
            decoded_prefix_within(&doc, &stream, usize::MAX).expect("kein Oversize");
        assert_eq!(applied, 0);
        assert_eq!(data, Vec::<u8>::new(), "kein Klon des Stroms");
        // Eine leere Kette dagegen ist ganz gelaufen: dort sind die Rohbytes
        // das Ergebnis.
        let ohne = Stream::new(dictionary! {}, b"roh".to_vec()).with_compression(false);
        assert_eq!(
            decoded_prefix_within(&doc, &ohne, usize::MAX),
            Ok((b"roh".to_vec(), 0))
        );
    }

    /// Ohne `/Filter` gibt es nichts zu entpacken — beide Wege liefern die
    /// Rohbytes, und der Orakelweg zählt null angewandte Filter.
    #[test]
    fn ohne_filter_liefern_beide_wege_die_rohbytes() {
        let doc = doc();
        let stream = Stream::new(dictionary! {}, b"roh".to_vec()).with_compression(false);
        assert_eq!(
            decoded_content_within(&doc, &stream, usize::MAX),
            Ok(Some(b"roh".to_vec()))
        );
        assert_eq!(
            decoded_prefix_within(&doc, &stream, usize::MAX),
            Ok((b"roh".to_vec(), 0))
        );
    }

    /// Die Liste der bewusst nicht dekodierten Bildfilter — dahinter steht
    /// Bildinhalt, kein ungelesener Text (siehe [`is_image_filter`]).
    #[test]
    fn bildfilter_sind_benannt_und_nichts_sonst() {
        for name in [
            &b"DCTDecode"[..],
            b"DCT",
            b"JPXDecode",
            b"CCITTFaxDecode",
            b"CCF",
            b"JBIG2Decode",
        ] {
            assert!(is_image_filter(name), "{}", String::from_utf8_lossy(name));
        }
        for name in [&b"Crypt"[..], b"PrivatFilter", b"FlateDecode", b""] {
            assert!(!is_image_filter(name), "{}", String::from_utf8_lossy(name));
        }
    }
}
