//! Gegenprüfung R2 (nach Fix-Runde 6): **die Bildfilter-Ausnahme als
//! Angriffsfläche.**
//!
//! Die Ausnahme sagt: hinter einem Bildfilter liegen Bilddaten, und was in
//! der Kette dahinter steht, ist ohnehin nicht entpackbar — deshalb keine
//! `unchecked`-Zeile, an **jeder** Stelle der Kette (Fix-Runde 6,
//! `zf_q2_filterkette::q2_bildfilter_an_erster_stelle_schweigt_auch_mit_kette_dahinter`).
//! Hier wird geprüft, wo das hält und wo nicht:
//!
//! * Text im Kommentarfeld eines JPEG (COM-Segment) oder in der XML-Box eines
//!   JP2 sind rohe Bytes — die Rohsichten lesen sie. Kein blinder Fleck.
//! * Gepackter Text unter `/DCTDecode` (zlib wie rohes Deflate) — die Rohsicht
//!   versucht beides an jedem Block und findet ihn. Kein blinder Fleck.
//! * `/DCTDecode` allein über Bytes, die weder Bild noch zlib noch Deflate
//!   sind: stumm — das ist der benannte blinde Fleck („ein Strom hinter
//!   `/DCTDecode`“, Modulkopf von `audit_bytes`).
//! * **BEFUND_R2_A (behoben, Fix-Runde 7):** `[/DCTDecode /FlateDecode]` mit
//!   zwei Byte „JPEG“ vor zlib-gepacktem Text war stumm — obwohl hinter dem
//!   Bildfilter kein Bild, sondern ein **weiterer Filter** stand, den das
//!   Programm kennt und weder anwandte noch meldete. Die Begründung der
//!   Ausnahme („dahinter liegen Bilddaten“) trägt nur, wenn der Bildfilter
//!   das **letzte** Glied ist; genau darauf ist sie jetzt beschränkt
//!   (`applied + 1 == total`).

mod common;

use common::{page, SECRET};
use lopdf::{dictionary, Object, Stream};
use redact_pdf::{leaks_many_within, LeakCheck};

fn zlib(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("komprimierbar");
    e.finish().expect("komprimierbar")
}

fn deflate_roh(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
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

fn text() -> Vec<u8> {
    format!("Notiz zur IBAN {SECRET}").into_bytes()
}

/// Ein JPEG-Rumpf mit einem Kommentarsegment (`FF FE`), das `text` trägt:
/// SOI, COM, EOI. Für die Frage hier genügt der Rahmen — das Orakel dekodiert
/// kein JPEG.
fn jpeg_mit_kommentar(text: &[u8]) -> Vec<u8> {
    let mut out = vec![0xff, 0xd8, 0xff, 0xfe];
    out.extend_from_slice(&u16::try_from(text.len() + 2).expect("kurz").to_be_bytes());
    out.extend_from_slice(text);
    out.extend_from_slice(&[0xff, 0xd9]);
    out
}

/// Ein JP2-Rumpf: Signatur-Box, `ftyp`, dann eine `xml `-Box mit `text`.
fn jp2_mit_xml(text: &[u8]) -> Vec<u8> {
    let mut out = vec![
        0, 0, 0, 0x0c, b'j', b'P', b' ', b' ', 0x0d, 0x0a, 0x87, 0x0a,
    ];
    let ftyp = b"jp2 \0\0\0\0jp2 ";
    out.extend_from_slice(&u32::try_from(8 + ftyp.len()).expect("kurz").to_be_bytes());
    out.extend_from_slice(b"ftyp");
    out.extend_from_slice(ftyp);
    let xml = format!("<meta>{}</meta>", String::from_utf8_lossy(text));
    out.extend_from_slice(&u32::try_from(8 + xml.len()).expect("kurz").to_be_bytes());
    out.extend_from_slice(b"xml ");
    out.extend_from_slice(xml.as_bytes());
    out
}

fn name(n: &str) -> Object {
    Object::Name(n.as_bytes().to_vec())
}

fn kette(names: &[&str]) -> Object {
    Object::Array(names.iter().map(|n| name(n)).collect())
}

/// Objekt 7 0 ist der Strom unter `filter`; kein Seiteninhalt.
fn pdf(filter: Object, raw: Vec<u8>) -> Vec<u8> {
    let mut d = page(&["harmlos"]);
    let id = d.add(Object::Stream(
        Stream::new(dictionary! { "Filter" => filter }, raw).with_compression(false),
    ));
    assert_eq!(id, (7, 0));
    d.catalog_set("R2Bild", Object::Reference(id));
    d.finish()
}

fn pruefe(pdf: &[u8]) -> LeakCheck {
    leaks_many_within(pdf, &[SECRET], u64::MAX)
}

/// Text im Kommentarfeld eines JPEG und in der XML-Box eines JP2: rohe
/// Bytes, von der Objektsicht als `<Stream, roh>` gelesen; Flate-gepackt
/// trägt der entzifferbare Anfang das Kommentarfeld. Nie `unchecked`.
///
/// Mutationsnachweis: in `audit_bytes::scan_stream` die Zeile
/// `scan_blob(&stream.content, … <Stream, roh>…)` gestrichen → rot (die
/// Fundstelle `Objekt 7 0 <Stream, roh>` fehlt; die Rohdatei-Sichten finden
/// den Text zwar noch, aber nicht als Objektsicht).
#[test]
fn r2_kommentarfelder_von_jpeg_und_jp2_werden_roh_gefunden() {
    for (wie, filter, raw) in [
        ("JPEG/COM", name("DCTDecode"), jpeg_mit_kommentar(&text())),
        (
            "JPEG/COM, Kurzform",
            name("DCT"),
            jpeg_mit_kommentar(&text()),
        ),
        ("JP2/xml", name("JPXDecode"), jp2_mit_xml(&text())),
    ] {
        let check = pruefe(&pdf(filter, raw));
        assert!(
            check.findings[0]
                .iter()
                .any(|m| m.starts_with("Objekt 7 0 <Stream, roh>")),
            "{wie}: das Kommentarfeld muss als rohe Bytes gefunden werden: {:#?}",
            check.findings[0]
        );
        assert!(check.unchecked.is_empty(), "{wie}: {:#?}", check.unchecked);
    }
    let check = pruefe(&pdf(
        kette(&["FlateDecode", "DCTDecode"]),
        zlib(&jpeg_mit_kommentar(&text())),
    ));
    assert!(
        check.findings[0].iter().any(|m| m.starts_with(
            "Objekt 7 0 <Stream, dekodiert: FlateDecode — bis Filter 1 von 2, danach /DCTDecode unbekannt>"
        )),
        "Flate-gepacktes JPEG: der entzifferbare Anfang trägt das Kommentarfeld: {:#?}",
        check.findings[0]
    );
    assert!(check.unchecked.is_empty(), "{:#?}", check.unchecked);
}

/// Unter `/DCTDecode` liegt kein JPEG, sondern gepackter Text — zlib oder
/// rohes Deflate. Die Rohsicht (Sicht 2) versucht beides an jedem Block, ohne
/// aufs Dictionary zu schauen, und findet ihn.
///
/// Mutationsnachweis: `audit_bytes::inflate_raw` liefert `Ok(None)` → rot.
#[test]
fn r2_gepackter_text_unter_bildfilter_wird_von_der_rohsicht_entpackt() {
    for (wie, raw) in [
        ("zlib", zlib(&text())),
        ("rohes Deflate", deflate_roh(&text())),
    ] {
        let check = pruefe(&pdf(name("DCTDecode"), raw));
        assert!(
            check.findings[0]
                .iter()
                .any(|m| m.contains("(Objekt 7 0) (inflate)")),
            "{wie} unter /DCTDecode: die Rohsicht muss entpacken: {:#?}",
            check.findings[0]
        );
        assert!(check.unchecked.is_empty(), "{wie}: {:#?}", check.unchecked);
    }
}

/// Der benannte blinde Fleck, so wie er benannt ist: `/DCTDecode` allein
/// über Bytes, die weder Bild noch zlib noch Deflate sind (zwei Byte „JPEG“,
/// dahinter zlib). Stumm — kein Fund, keine Meldung. Das steht so im
/// Modulkopf („ein Strom hinter `/DCTDecode`“) und ist kein Befund; es ist
/// der Maßstab, an dem BEFUND_R2_A hängt.
#[test]
fn r2_der_benannte_blinde_fleck_allein_bleibt_stumm() {
    let mut raw = vec![0xff, 0xd8];
    raw.extend_from_slice(&zlib(&text()));
    let check = pruefe(&pdf(name("DCTDecode"), raw));
    assert!(check.findings[0].is_empty(), "{:#?}", check.findings[0]);
    assert!(check.unchecked.is_empty(), "{:#?}", check.unchecked);
}

/// **BEFUND_R2_A, geschlossen** — ein Bildfilter, der **nicht** das letzte
/// Glied ist, deckte einen Packfilter dahinter zu.
///
/// `[/DCTDecode /FlateDecode]`, Strominhalt zwei Byte „JPEG“ (`FF D8`) vor
/// zlib-gepacktem Text. Die Kette bleibt am ersten Glied stehen; das ist ein
/// Bildfilter, also keine Meldung. Hinter ihm liegt aber kein Bild, sondern
/// `/FlateDecode` — ein Filter, den das Programm kennt, hier weder anwendet
/// noch nannte. Kein Fund, keine `unchecked`-Zeile: stumm. Derselbe Inhalt
/// unter `[/R2Fremd /FlateDecode]` wurde gemeldet — der Unterschied lag
/// allein am Namen des ersten Gliedes, und der Bildfilter-Name kaufte so eine
/// meldungsfreie Zone für beliebige Bytes.
///
/// Die Begründung der Ausnahme („dahinter liegen Bilddaten … entpacken lässt
/// sich davon ohnehin nichts“) gilt nur, wenn der Bildfilter das **letzte**
/// Glied ist: die Reihenfolge im `/Filter`-Array ist die Dekodierreihenfolge
/// (PDF 32000-1, 7.4.1), und die Ausgabe eines Bildfilters sind Abtastwerte —
/// kein Erzeuger hängt dahinter noch einen Filter. Eine Meldung an
/// `[/DCTDecode /X]` trifft deshalb keine gewöhnliche Datei mit einem Foto.
///
/// Seit Fix-Runde 7 gilt die Ausnahme genau dort: `applied + 1 == total`.
#[test]
fn befund_r2_a_bildfilter_vor_einem_packfilter_wird_gemeldet() {
    let mut raw = vec![0xff, 0xd8];
    raw.extend_from_slice(&zlib(&text()));

    let check = pruefe(&pdf(kette(&["DCTDecode", "FlateDecode"]), raw.clone()));
    // Entpackt ist weiterhin nichts — aber die Stelle steht jetzt in der Liste.
    assert!(check.findings[0].is_empty(), "{:#?}", check.findings[0]);
    assert!(
        check
            .unchecked
            .iter()
            .any(|m| m.starts_with("Objekt 7 0 <Stream>:")
                && m.contains("/DCTDecode ist ein Bildfilter")
                && m.contains("(Glied 1 von 2)")),
        "{:#?}",
        check.unchecked
    );
    assert_eq!(check.unchecked_places, 1, "{:#?}", check.unchecked);

    // Gegenprobe: derselbe Inhalt, ein fremder Name an erster Stelle.
    let fremd = pruefe(&pdf(kette(&["R2Fremd", "FlateDecode"]), raw));
    assert!(fremd.findings[0].is_empty());
    assert!(
        fremd
            .unchecked
            .iter()
            .any(|m| m.contains("/R2Fremd ist hier kein bekannter Filter (Glied 1 von 2)")),
        "{:#?}",
        fremd.unchecked
    );

    // Und die Kette, die die Fix-Runde 6 ausdrücklich für richtig erklärt
    // hatte: `[/DCTDecode /ASCII85Decode]` über ASCII85-Text — ebenfalls ein
    // Bildfilter mit einem Glied dahinter, also ebenfalls gemeldet.
    let check = pruefe(&pdf(kette(&["DCTDecode", "ASCII85Decode"]), a85(&text())));
    assert!(check.findings[0].is_empty(), "{:#?}", check.findings[0]);
    assert!(
        check
            .unchecked
            .iter()
            .any(|m| m.contains("/DCTDecode ist ein Bildfilter")),
        "{:#?}",
        check.unchecked
    );
}

/// Die Gegenrichtung zu BEFUND_R2_A — und der Grund, warum die Ausnahme
/// überhaupt besteht: der Bildfilter am **Ende** der Kette schweigt weiter.
///
/// `[/ASCII85Decode /DCTDecode]` ist die gewöhnliche Ausgabe eines
/// Distillers. Der Klartext davor wird gefunden, hinter `/DCTDecode` liegen
/// Abtastwerte — der im Modulkopf von `audit_bytes` benannte blinde Fleck,
/// und **keine** `NICHT GEPRÜFT`-Zeile. Dasselbe gilt für den Bildfilter
/// allein und für seine Kurzform.
#[test]
fn r2_bildfilter_als_letztes_glied_schweigt_weiter() {
    let check = pruefe(&pdf(kette(&["ASCII85Decode", "DCTDecode"]), a85(&text())));
    assert!(!check.findings[0].is_empty(), "{:#?}", check.findings[0]);
    assert!(check.unchecked.is_empty(), "{:#?}", check.unchecked);
    assert_eq!(check.unchecked_places, 0);

    for allein in [
        "DCTDecode",
        "DCT",
        "JPXDecode",
        "CCITTFaxDecode",
        "CCF",
        "JBIG2Decode",
    ] {
        let mut raw = vec![0xff, 0xd8];
        raw.extend_from_slice(&zlib(&text()));
        let check = pruefe(&pdf(name(allein), raw));
        assert!(
            check.unchecked.is_empty(),
            "/{allein} allein ist der benannte blinde Fleck: {:#?}",
            check.unchecked
        );
    }

    // Auch mitten in der Kette, solange nichts dahinter steht, was das
    // Programm anwenden könnte — hier ist der Bildfilter das letzte Glied.
    let check = pruefe(&pdf(
        kette(&["FlateDecode", "DCTDecode"]),
        zlib(&jpeg_mit_kommentar(&text())),
    ));
    assert!(check.unchecked.is_empty(), "{:#?}", check.unchecked);
}
