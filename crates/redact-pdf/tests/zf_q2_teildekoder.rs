//! Gegenprüfung Q2: was `filters::decoded_prefix_within` liefert — und was es
//! dafür belegt.
//!
//! # Befund Q2-2 (Speicher, gemessen)
//!
//! `decode_chain` klont die Rohbytes des Stroms **bevor** es weiß, ob der erste
//! Filter überhaupt bekannt ist. Bei `applied == 0` wirft
//! `audit_bytes::decode_stream` diesen Klon sofort weg — bezahlt ist er
//! trotzdem. Gemessen (Release, Kindprozess, `VmHWM`) an einer Datei mit einem
//! 64-MiB-Strom:
//!
//! ```text
//! ohne /Filter          Datei 67 MB   VmHWM 138 MB
//! /Filter /DCTDecode    Datei 67 MB   VmHWM 205 MB
//! ```
//!
//! Dazu passt die Zusicherung in `leaks_many_within` nicht: „Spitzenbelegung:
//! ein Strom in Arbeit (höchstens Budget + 1 Byte)“. Der Klon hängt nicht am
//! Budget, sondern an der Stromgröße — `decoded_prefix_within` gibt bei einem
//! unbekannten ersten Filter 8 000 000 Byte zurück, obwohl `limit` 16 war.
//! Gefährlich ist es (heute) nicht: `document::prescan` verbucht denselben
//! Strom vorher gegen dasselbe Budget, die Datei wird also ohnehin abgelehnt,
//! wenn sie zu groß ist. Aber diese Deckung steht nirgends geschrieben.

use lopdf::{dictionary, Document, Object, Stream};
use redact_pdf::filters::{decoded_content_within, decoded_prefix_within, Oversize};
use redact_pdf::leaks_many_within;

fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).expect("komprimierbar");
    e.finish().expect("komprimierbar")
}

fn kette(filters: &[&str], raw: Vec<u8>) -> Stream {
    let value = match filters.len() {
        1 => Object::Name(filters[0].as_bytes().to_vec()),
        _ => Object::Array(
            filters
                .iter()
                .map(|f| Object::Name(f.as_bytes().to_vec()))
                .collect(),
        ),
    };
    Stream::new(dictionary! { "Filter" => value }, raw).with_compression(false)
}

const KLARTEXT: &[u8] = b"IBAN: DE89 3704 0044 0532 0130 00 -- Klartext";

// ---------------------------------------------------------------------------
// Was liefert der Teil-Dekoder?
// ---------------------------------------------------------------------------

/// Erster Filter unbekannt: **Rohbytes**, `applied == 0` — nicht „nichts“.
/// Der strenge Leser sagt dazu `Ok(None)`.
#[test]
fn q2_erster_filter_unbekannt_liefert_die_rohbytes() {
    let doc = Document::with_version("1.5");
    let s = kette(&["Q2Phantasie"], KLARTEXT.to_vec());
    let (data, applied) = decoded_prefix_within(&doc, &s, usize::MAX).expect("kein Oversize");
    assert_eq!(applied, 0);
    assert_eq!(data, KLARTEXT, "die Rohbytes, unverändert");
    assert_eq!(decoded_content_within(&doc, &s, usize::MAX), Ok(None));
}

/// Ein Strom ohne `/Filter` und einer mit leerem `/Filter []`: dieselben
/// Rohbytes, `applied == 0`, aber der strenge Leser gibt sie **her** (die Kette
/// ist ja vollständig gelaufen — sie ist leer).
#[test]
fn q2_ohne_filter_und_mit_leerer_kette() {
    let doc = Document::with_version("1.5");
    for s in [
        Stream::new(dictionary! {}, KLARTEXT.to_vec()).with_compression(false),
        Stream::new(
            dictionary! { "Filter" => Object::Array(vec![]) },
            KLARTEXT.to_vec(),
        )
        .with_compression(false),
    ] {
        let (data, applied) = decoded_prefix_within(&doc, &s, usize::MAX).expect("kein Oversize");
        assert_eq!((data.as_slice(), applied), (KLARTEXT, 0));
        assert_eq!(
            decoded_content_within(&doc, &s, usize::MAX),
            Ok(Some(KLARTEXT.to_vec()))
        );
    }
}

/// Ein Strom, der mitten im Filter abbricht (halbe Flate-Daten): das
/// Teilergebnis bleibt stehen, `applied` zählt den Filter als angewandt — und
/// der strenge Leser bekommt dasselbe Teilergebnis. Beide Leser lesen hier
/// dasselbe; der Unterschied liegt allein am **unbekannten** Glied.
#[test]
fn q2_abgebrochener_filter_liefert_das_teilergebnis() {
    let doc = Document::with_version("1.5");
    let packed = deflate(KLARTEXT);
    let s = kette(&["FlateDecode"], packed[..packed.len() / 2].to_vec());
    let (data, applied) = decoded_prefix_within(&doc, &s, usize::MAX).expect("kein Oversize");
    assert_eq!(applied, 1);
    assert!(!data.is_empty() && data.len() < KLARTEXT.len());
    assert!(KLARTEXT.starts_with(&data), "ein echter Anfang");
    assert_eq!(
        decoded_content_within(&doc, &s, usize::MAX),
        Ok(Some(data)),
        "der strenge Leser sieht dasselbe"
    );
}

/// Ein Filter liefert Müll, den der nächste nicht lesen kann: die Kette läuft
/// **durch** (`applied == total`), das Ergebnis ist Müll. Das Orakel meldet
/// dann keinen unbekannten Filter — es hat ja alle angewandt —, und die
/// Rohsicht trägt den Fund.
#[test]
fn q2_muell_zwischen_zwei_filtern_bricht_die_kette_nicht_ab() {
    let doc = Document::with_version("1.5");
    let s = kette(&["FlateDecode", "ASCII85Decode"], deflate(KLARTEXT));
    let (data, applied) = decoded_prefix_within(&doc, &s, usize::MAX).expect("kein Oversize");
    assert_eq!(applied, 2, "beide Filter gelten als angewandt");
    assert!(!data.starts_with(b"IBAN"), "das Ergebnis ist Müll");

    // A85 liefert etwas, was kein zlib-Strom ist → Flate gibt nichts her.
    let s = kette(
        &["ASCII85Decode", "FlateDecode"],
        b"87cURD]j7BEbo80".to_vec(),
    );
    let (data, applied) = decoded_prefix_within(&doc, &s, usize::MAX).expect("kein Oversize");
    assert_eq!((data.len(), applied), (0, 2));
}

/// Jede Stufe wird an der Grenze gemessen — auch die zweite.
#[test]
fn q2_jede_stufe_bleibt_im_budget() {
    let doc = Document::with_version("1.5");
    let gross = deflate(&vec![b'A'; 1_000_000]);
    assert_eq!(
        decoded_prefix_within(&doc, &kette(&["FlateDecode"], gross.clone()), 1000),
        Err(Oversize)
    );
    assert_eq!(
        decoded_prefix_within(
            &doc,
            &kette(&["FlateDecode", "FlateDecode"], deflate(&gross)),
            1000
        ),
        Err(Oversize),
        "die zweite Stufe muss genauso messen"
    );
    // ASCII85: die Fehlalarm-Korrektur. Gebucht werden muss, was eine Gruppe
    // wirklich liefert — die **angebrochene** Schlussgruppe drei Byte, nicht
    // vier. 999 Byte Nutzlast (249 volle Gruppen plus eine von drei) dürfen
    // bei einem Budget von genau 999 kein Oversize sein; wer pauschal vier
    // bucht, landet bei 1000 und lehnt ab.
    for len in [999usize, 1000] {
        let (data, applied) =
            decoded_prefix_within(&doc, &kette(&["ASCII85Decode"], a85(&vec![b'Q'; len])), len)
                .unwrap_or_else(|e| panic!("{len} Byte bei Budget {len}: {e:?}"));
        assert_eq!((data.len(), applied), (len, 1));
    }
    // Ein Byte weniger Budget muss dagegen Oversize sein — die Grenze ist
    // scharf, nicht großzügig.
    assert_eq!(
        decoded_prefix_within(&doc, &kette(&["ASCII85Decode"], a85(&vec![b'Q'; 999])), 998),
        Err(Oversize)
    );
    // Und `z` (vier Nullbytes) bucht vier.
    let mut vier_nullen = b"z".to_vec();
    vier_nullen.extend_from_slice(b"~>");
    assert_eq!(
        decoded_prefix_within(&doc, &kette(&["ASCII85Decode"], vier_nullen.clone()), 4),
        Ok((vec![0, 0, 0, 0], 1))
    );
    assert_eq!(
        decoded_prefix_within(&doc, &kette(&["ASCII85Decode"], vier_nullen), 3),
        Err(Oversize)
    );
}

/// Mein eigener ASCII85-Kodierer.
fn a85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in data.chunks(4) {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        let mut v = u32::from_be_bytes(w);
        let mut g = [0u8; 5];
        for slot in g.iter_mut().rev() {
            *slot = b'!' + (v % 85) as u8;
            v /= 85;
        }
        out.extend_from_slice(&g[..c.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// **BEFUND_Q2_2**: bei unbekanntem erstem Filter kommt mehr zurück als
/// `limit` erlaubt — die Rohbytes wurden geklont, bevor jemand nach dem Filter
/// gefragt hat.
#[test]
fn q2_teilpuffer_kann_das_budget_ueberschreiten() {
    let doc = Document::with_version("1.5");
    let s = kette(&["Q2Phantasie"], vec![b'X'; 8_000_000]);
    let (data, applied) = decoded_prefix_within(&doc, &s, 16).expect("kein Oversize");
    assert_eq!(applied, 0);
    assert_eq!(
        data.len(),
        8_000_000,
        "gemessener Zustand: 8 MB zurück bei limit = 16"
    );
}

/// Die Deckung, die den Befund heute harmlos macht: derselbe Strom wird von
/// `prescan` gegen dasselbe Budget verbucht, die Datei fliegt vorher heraus.
#[test]
fn q2_prescan_deckt_den_klon_ab() {
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1_i64,
        }),
    );
    let katalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", katalog);
    let id = doc.add_object(Object::Stream(kette(&["DCTDecode"], vec![b'Q'; 4_000_000])));
    doc.get_dictionary_mut(katalog)
        .expect("Katalog")
        .set("Q2Big", Object::Reference(id));
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("speicherbar");

    let check = leaks_many_within(&bytes, &["DE89 3704 0044 0532 0130 00"], 1024 * 1024);
    assert!(
        check.unchecked.iter().any(|m| m.contains("Vorprüfung")),
        "die Vorprüfung muss den 4-MB-Strom gegen das 1-MB-Budget verbuchen: {:#?}",
        check.unchecked
    );
}

/// Die Gründe, die `unchecked` kennt, sind mehr als drei.
///
/// SECURITY.md („Was die Nachprüfung der Oberfläche zusichert“) zählt sie auf:
/// „entpackte Ströme über `--max-decompressed-mb`, eine vom Lader abgelehnte
/// Vorprüfung, und Objekte tiefer als 32 Ebenen“. Der Code kennt zwei weitere,
/// und beide sind erreichbar — hier die vierte und die fünfte in **einem** Lauf:
/// ein `/RunLengthDecode`-Strom, den `prescan` nur mit seiner **Rohgröße**
/// verbucht (dort steht er nicht in der auspackbaren Liste), sprengt beim
/// Orakel das Budget → „nicht entpackt“ **und** „Sicht 7 nicht gelaufen“.
#[test]
fn q2_unchecked_kennt_mehr_gruende_als_die_doku_aufzaehlt() {
    // 1 KB Rohbytes → 128 KB entpackt (jeder Lauf verhundertachtundzwanzigfacht).
    let mut rle = Vec::new();
    for _ in 0..1000 {
        rle.push(129u8); // „das nächste Byte 128-mal“
        rle.push(b'Q');
    }
    rle.push(128);

    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1_i64,
        }),
    );
    let katalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", katalog);
    let id = doc.add_object(Object::Stream(kette(&["RunLengthDecode"], rle)));
    doc.get_dictionary_mut(katalog)
        .expect("Katalog")
        .set("Q2Rle", Object::Reference(id));
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("speicherbar");

    // Budget 16 KiB: die Vorprüfung nimmt die Datei (2 KB Rohbytes), das Orakel
    // kann den Strom nicht entpacken.
    let check = leaks_many_within(&bytes, &["DE89 3704 0044 0532 0130 00"], 16 * 1024);
    assert!(
        !check.unchecked.iter().any(|m| m.contains("Vorprüfung")),
        "die Vorprüfung muss die Datei annehmen: {:#?}",
        check.unchecked
    );
    assert!(
        check.unchecked.iter().any(|m| m.contains("nicht entpackt")),
        "Grund 1 (Budget) fehlt: {:#?}",
        check.unchecked
    );
    assert!(
        check
            .unchecked
            .iter()
            .any(|m| m.contains("Sicht 7 (Schriftdekoder) nicht gelaufen")),
        "der fünfte Grund fehlt: {:#?}",
        check.unchecked
    );
}
