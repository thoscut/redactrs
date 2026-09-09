//! Gegenprüfung P1, Punkt 2 und 3: das Budget des Leck-Orakels und die neuen
//! begrenzten Filter (`weezl`/LZW, ASCII85, RunLength, Filterketten).
//!
//! Geprüft wird beides: die Grenze hält (keine Bombe, kein Absturz, keine
//! Endlosschleife) **und** sie sagt, was sie nicht geprüft hat — „nicht
//! gefunden“ darf nie stillschweigend „nicht gesucht“ heißen.
//!
//! Die Speichermessung (`VmHWM`) läuft im Kindprozess, weil der Spitzenwert
//! am Prozess hängt und die anderen Tests dieser Datei ihn sonst prägen.

mod common;

use std::io::Write;
use std::time::{Duration, Instant};

use common::{page, text_ops, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_pdf::{leaks_many_within, LeakCheck};

const MIB: usize = 1024 * 1024;

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).expect("deflate");
    e.finish().expect("deflate")
}

fn stream_with(filter: Object, content: Vec<u8>) -> Stream {
    Stream::new(dictionary! { "Filter" => filter }, content).with_compression(false)
}

/// Gut komprimierbare Füllung — deflate muss wirklich packen, sonst stünde
/// der Klartext als „stored“ im Strom und jede Rohsicht fände ihn.
fn fuellung(size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size);
    while out.len() < size {
        out.extend_from_slice(b"% ABCABCABCABC ABCABCABCABC ABCABCABCABC\n");
    }
    out.truncate(size);
    out
}

/// `[/RunLengthDecode /FlateDecode]` — die Vorprüfung des Laders packt nur
/// reine Flate/LZW/ASCII85-Ketten aus und sieht diesen Strom deshalb nicht.
fn rl_flate(plain: &[u8]) -> Stream {
    stream_with(
        Object::Array(vec!["RunLengthDecode".into(), "FlateDecode".into()]),
        common::run_length_encode(&deflate(plain)),
    )
}

fn object(id: ObjectId) -> String {
    format!("Objekt {} {}", id.0, id.1)
}

/// Ein PDF mit einer leeren Seite und `extra` zusätzlichen Objekten.
fn pdf_with_streams(streams: Vec<Stream>) -> (Vec<u8>, Vec<ObjectId>) {
    let mut d = page(&[]);
    let mut ids = Vec::new();
    for s in streams {
        ids.push(d.add(Object::Stream(s)));
    }
    (d.finish(), ids)
}

// ---------------------------------------------------------------------------
// Punkt 2: das Budget ist eine Summe — viele kleine Ströme reichen
// ---------------------------------------------------------------------------

/// Das Budget gilt je Sicht als **Summe**. Viele kleine Ströme können es
/// deshalb aufbrauchen, sodass der Strom mit dem Geheimnis nicht mehr
/// entpackt wird. Das ist zulässig — aber es muss **gesagt** werden: der
/// übersprungene Strom steht mit seiner Objekt-Id in `unchecked`, und
/// Sicht 7 meldet sich ab.
///
/// Die Kette `[/RunLengthDecode /FlateDecode]` ist mit Absicht gewählt: die
/// Vorprüfung des Laders packt nur reine Flate/LZW/ASCII85-Ketten aus und
/// sieht diese Ströme deshalb gar nicht — die Sicht 3 des Orakels muss die
/// Grenze hier allein halten. Der Klartext ist zlib-verpackt und von der
/// Rohsicht nicht zu sehen, weil die Nutzlast mit RunLength-Längenbytes
/// beginnt und kein zlib-Strom ist.
#[test]
fn viele_kleine_stroeme_brauchen_das_budget_auf_und_das_wird_gesagt() {
    let harmless = fuellung(256 * 1024);
    let mut secret_plain = text_ops(&[SECRET]);
    secret_plain.extend_from_slice(&fuellung(64 * 1024));
    let mut streams: Vec<Stream> = (0..16).map(|_| rl_flate(&harmless)).collect();
    streams.push(rl_flate(&secret_plain));
    let (pdf, ids) = pdf_with_streams(streams);
    let last = object(*ids.last().unwrap());

    // Budget: genug für die sechzehn harmlosen Ströme, nicht mehr für den
    // siebzehnten.
    let budget = (16 * harmless.len()) as u64;
    let tight = leaks_many_within(&pdf, &[SECRET], budget);
    assert!(
        tight.findings[0].is_empty(),
        "das Geheimnis stand nur im übersprungenen Strom: {:?}",
        tight.findings[0]
    );
    assert!(
        tight
            .unchecked
            .iter()
            .any(|u| u.contains(&last) && u.contains("nicht entpackt")),
        "der übersprungene Strom wird nicht genannt: {:?}",
        tight.unchecked
    );
    assert!(
        tight.unchecked.iter().any(|u| u.contains("Sicht 7")),
        "der Verlust der Schriftdekoder-Sicht wird nicht gesagt: {:?}",
        tight.unchecked
    );

    // Gegenrichtung: reicht das Budget, ist nichts offen und alles gefunden.
    let full = leaks_many_within(&pdf, &[SECRET], u64::MAX);
    assert!(full.unchecked.is_empty(), "{:?}", full.unchecked);
    assert!(
        full.findings[0]
            .iter()
            .any(|h| h.contains(&last) && h.contains("dekodiert")),
        "{:?}",
        full.findings[0]
    );
}

/// Mehr als `MAX_UNCHECKED` (50) übersprungene Ströme: die ersten 50 werden
/// einzeln genannt, der Rest gezählt — geschwiegen wird über keinen.
#[test]
fn ueber_fuenfzig_uebersprungene_stroeme_werden_gezaehlt_nicht_verschwiegen() {
    let big = fuellung(256 * 1024);
    let streams: Vec<Stream> = (0..60).map(|_| rl_flate(&big)).collect();
    let (pdf, _) = pdf_with_streams(streams);

    let result = leaks_many_within(&pdf, &["kommtnichtvor"], 64 * 1024);
    assert!(
        !result.unchecked.iter().any(|u| u.contains("Vorprüfung")),
        "die Vorprüfung sollte diese Ketten nicht sehen: {:?}",
        result.unchecked
    );
    let einzeln = result
        .unchecked
        .iter()
        .filter(|u| u.starts_with("Objekt") && u.contains("<Stream>: nicht entpackt"))
        .count();
    let summen: Vec<&String> = result
        .unchecked
        .iter()
        .filter(|u| u.contains("weitere Ströme nicht entpackt"))
        .collect();
    assert_eq!(
        einzeln, 50,
        "höchstens 50 einzeln: {einzeln}\n{:#?}",
        result.unchecked
    );
    assert_eq!(summen.len(), 1, "eine Summenzeile: {summen:?}");
    assert!(
        summen[0].contains("10 weitere"),
        "die Summenzeile zählt falsch: {}",
        summen[0]
    );
}

/// `leaks`/`leaks_many` laufen ohne Budget (`u64::MAX`) — dann darf nie ein
/// Strom übersprungen werden, auch nicht an einer Bombe.
#[test]
fn ohne_budget_wird_kein_strom_uebersprungen() {
    let plain = text_ops(&[SECRET]);
    let (pdf, _) = pdf_with_streams(vec![
        stream_with("FlateDecode".into(), deflate(&plain)),
        stream_with("ASCII85Decode".into(), common::ascii85_encode(&plain)),
        stream_with("RunLengthDecode".into(), common::run_length_encode(&plain)),
        stream_with("ASCIIHexDecode".into(), common::ascii_hex_encode(&plain)),
    ]);
    let result = leaks_many_within(&pdf, &[SECRET], u64::MAX);
    assert!(result.unchecked.is_empty(), "{:?}", result.unchecked);
    for filter in [
        "FlateDecode",
        "ASCII85Decode",
        "RunLengthDecode",
        "ASCIIHexDecode",
    ] {
        assert!(
            result.findings[0]
                .iter()
                .any(|h| h.contains(&format!("dekodiert: {filter}"))),
            "{filter} nicht dekodiert durchsucht: {:?}",
            result.findings[0]
        );
    }
}

// ---------------------------------------------------------------------------
// Punkt 3: die neuen Filter — kein Absturz, keine Endlosschleife, Grenze hält
// ---------------------------------------------------------------------------

/// Kaputte, bösartige und entartete Eingaben für die drei neuen begrenzten
/// Dekoder. Erwartet wird: keine Panik, keine Endlosschleife, und die Datei
/// wird nicht grundlos abgelehnt (was dekodierbar ist, wird durchsucht).
#[test]
fn kaputte_filter_stuerzen_nicht_ab_und_haengen_nicht() {
    let plain = text_ops(&[SECRET]);
    let mut faelle: Vec<(&str, Object, Vec<u8>)> = Vec::new();

    // LZW: sofortiger Clear-Code, danach nichts.
    faelle.push(("LZW nur Clear", "LZWDecode".into(), vec![0x80, 0x00]));
    // LZW: lauter Einsbits — ungültige Codes, sofort maximale Codebreite.
    faelle.push(("LZW alles 1", "LZWDecode".into(), vec![0xff; 4096]));
    // LZW: Zufallsmüll.
    let mut state = 0x1234_5678_9abc_def0u64;
    let muell: Vec<u8> = (0..8192)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    faelle.push(("LZW Zufall", "LZWDecode".into(), muell.clone()));
    // LZW: leer.
    faelle.push(("LZW leer", "LZWDecode".into(), Vec::new()));

    // ASCII85: `z` mitten in einer Gruppe, `~>` mitten drin, ungültige Zeichen.
    faelle.push((
        "A85 z in Gruppe",
        "ASCII85Decode".into(),
        b"5sdz q,~>".to_vec(),
    ));
    faelle.push((
        "A85 EOD mitten drin",
        "ASCII85Decode".into(),
        b"5sdq,~>77Kd<~>".to_vec(),
    ));
    faelle.push((
        "A85 ungueltig",
        "ASCII85Decode".into(),
        b"5sd\x00q,\xff~>".to_vec(),
    ));
    faelle.push(("A85 nur z", "ASCII85Decode".into(), b"zzzzzzzz".to_vec()));
    faelle.push((
        "A85 uuuuu",
        "ASCII85Decode".into(),
        b"uuuuuuuuuu~>".to_vec(),
    ));
    faelle.push(("A85 leer", "ASCII85Decode".into(), b"~>".to_vec()));
    faelle.push((
        "A85 ohne EOD",
        "ASCII85Decode".into(),
        b"5sdq,77Kd<".to_vec(),
    ));

    // RunLength: 128 als erstes Byte, Länge ohne folgende Bytes, kein EOD.
    faelle.push(("RL nur EOD", "RunLengthDecode".into(), vec![128]));
    faelle.push(("RL Laenge ohne Daten", "RunLengthDecode".into(), vec![127]));
    faelle.push(("RL Lauf ohne Byte", "RunLengthDecode".into(), vec![129]));
    faelle.push((
        "RL ohne EOD",
        "RunLengthDecode".into(),
        vec![2, b'a', b'b', b'c'],
    ));
    faelle.push(("RL Zufall", "RunLengthDecode".into(), muell));

    // Ketten mit einem kaputten Glied.
    faelle.push((
        "Kette A85+Flate kaputt",
        Object::Array(vec!["ASCII85Decode".into(), "FlateDecode".into()]),
        common::ascii85_encode(b"kein zlib"),
    ));
    faelle.push((
        "Kette RL+LZW kaputt",
        Object::Array(vec!["RunLengthDecode".into(), "LZWDecode".into()]),
        common::run_length_encode(&plain),
    ));

    for (name, filter, content) in faelle {
        for budget in [0u64, 7, 4096, u64::MAX] {
            let (pdf, _) = pdf_with_streams(vec![stream_with(filter.clone(), content.clone())]);
            let started = Instant::now();
            let result: LeakCheck = leaks_many_within(&pdf, &[SECRET, "kommtnichtvor"], budget);
            let elapsed = started.elapsed();
            assert!(
                elapsed < Duration::from_secs(10),
                "{name} (Budget {budget}): {elapsed:?} — sieht nach Endlosschleife aus"
            );
            assert!(
                result.findings[1].is_empty(),
                "{name}: Fehltreffer {:?}",
                result.findings[1]
            );
        }
    }
}

/// Die Gegenrichtung zu oben: **gewöhnliche** Ströme in allen fünf Filtern,
/// mit einem Budget, das ihrer entpackten Größe entspricht, dürfen nicht
/// abgelehnt werden — jeder muss dekodiert durchsucht werden.
#[test]
fn gewoehnliche_stroeme_werden_bei_passendem_budget_nicht_abgelehnt() {
    let plain = text_ops(&[SECRET]);
    for (filter, content) in [
        ("FlateDecode", deflate(&plain)),
        ("ASCIIHexDecode", common::ascii_hex_encode(&plain)),
        ("RunLengthDecode", common::run_length_encode(&plain)),
    ] {
        let (pdf, ids) = pdf_with_streams(vec![stream_with(filter.into(), content)]);
        let named = object(ids[0]);
        // Budget: reichlich für diesen einen Strom, aber weit unter „unendlich“.
        let result = leaks_many_within(&pdf, &[SECRET], 4 * MIB as u64);
        assert!(
            !result.unchecked.iter().any(|u| u.contains(&named)),
            "{filter} grundlos übersprungen: {:?}",
            result.unchecked
        );
        assert!(
            result.findings[0]
                .iter()
                .any(|h| h.contains(&format!("dekodiert: {filter}"))),
            "{filter}: {:?}",
            result.findings[0]
        );
    }
}

// ---------------------------------------------------------------------------
// Bomben — Zeit und Speicher, im Kindprozess gemessen
// ---------------------------------------------------------------------------

const CHILD: &str = "ZE_P1_BOMBE_KIND";

fn peak_rss_bytes() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().strip_suffix("kB"))
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|kb| kb * 1024)
        .expect("VmHWM")
}

/// LZW-, ASCII85-, RunLength- und Kettenbombe: jede bleibt im Budget, in
/// Zeit und Speicher — und jede wird als übersprungener Strom genannt.
#[test]
fn die_neuen_filter_als_bombe_bleiben_im_budget() {
    if std::env::var_os(CHILD).is_some() {
        return bomben_im_kindprozess();
    }
    let exe = std::env::current_exe().expect("Testbinary");
    let output = std::process::Command::new(exe)
        .args([
            "--exact",
            "die_neuen_filter_als_bombe_bleiben_im_budget",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .expect("Kindprozess");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprint!("{stderr}");
    assert!(
        output.status.success(),
        "Kindprozess: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(stderr.contains("VmHWM"), "nicht gemessen: {stderr}");
}

fn bomben_im_kindprozess() {
    // Entpackt je Bombe; im Debug-Profil kleiner, weil schon das Erzeugen
    // sonst Minuten dauert. Die Grenze greift unabhängig von der Größe.
    let out_bytes = if cfg!(debug_assertions) {
        64 * MIB
    } else {
        512 * MIB
    };
    let budget = 4 * MIB as u64;
    let deadline = if cfg!(debug_assertions) {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(10)
    };
    let before = peak_rss_bytes();

    // Jede Bombe steckt hinter einem RunLength-Mantel: die Vorprüfung des
    // Laders packt nur reine Flate/LZW/ASCII85-Ketten aus und lehnt die Datei
    // sonst schon vorher ab — dann liefe der neue Dekoder gar nicht, und der
    // Test bewiese nichts über ihn.
    let mantel = |inner: &str, packed: Vec<u8>| -> (Object, Vec<u8>) {
        (
            Object::Array(vec!["RunLengthDecode".into(), inner.into()]),
            common::run_length_encode(&packed),
        )
    };

    let lzw = {
        use weezl::encode::Encoder;
        use weezl::BitOrder;
        Encoder::with_tiff_size_switch(BitOrder::Msb, 8)
            .encode(&vec![0u8; out_bytes])
            .expect("LZW-Bombe")
    };
    let flate = {
        let zeros = vec![0u8; MIB];
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        for _ in 0..out_bytes / MIB {
            e.write_all(&zeros).expect("deflate");
        }
        e.finish().expect("deflate")
    };
    // ASCII85 wächst nur um den Faktor vier (`z` = vier Nullbytes); die
    // Bombe ist entsprechend kleiner gehalten.
    // Die gepackten Bytes müssen zudem unter dem Budget bleiben: die
    // Vorprüfung des Laders verbucht einen Strom mit unbekanntem Filter roh.
    let a85 = vec![b'z'; out_bytes / 32];
    // RunLength allein: ein Lauf von 128 gleichen Bytes je zwei Byte.
    let rl: Vec<u8> = std::iter::repeat_n([129u8, b'0'], out_bytes / 128)
        .flatten()
        .collect();

    let mut faelle: Vec<(&str, Object, Vec<u8>)> = Vec::new();
    let (f, c) = mantel("LZWDecode", lzw);
    faelle.push(("RL+LZW", f, c));
    let (f, c) = mantel("FlateDecode", flate);
    faelle.push(("RL+Flate (letzter Filter bläht auf)", f, c));
    let (f, c) = mantel("ASCII85Decode", a85);
    faelle.push(("RL+ASCII85", f, c));
    faelle.push(("RunLength", "RunLengthDecode".into(), rl));

    for (name, filter, content) in faelle {
        let packed = content.len();
        let (pdf, ids) = pdf_with_streams(vec![stream_with(filter, content)]);
        let named = object(ids[0]);
        let started = Instant::now();
        let result = leaks_many_within(&pdf, &[SECRET], budget);
        let elapsed = started.elapsed();
        let peak = peak_rss_bytes();
        eprintln!(
            "Bombe {name}: bis {} MiB entpackt, {packed} Byte gepackt, Budget {} MiB: \
             {elapsed:?}, VmHWM {} MB (vorher {} MB)",
            out_bytes / MIB,
            budget / MIB as u64,
            peak / 1_000_000,
            before / 1_000_000
        );
        assert!(
            elapsed < deadline,
            "{name}: {elapsed:?} — die Bombe wurde entpackt"
        );
        assert!(
            result.findings[0].is_empty(),
            "{name}: {:?}",
            result.findings[0]
        );
        assert!(
            result
                .unchecked
                .iter()
                .any(|u| u.contains(&named) && u.contains("nicht entpackt")),
            "{name}: der Strom wird nicht als ungeprüft genannt: {:?}",
            result.unchecked
        );
        assert!(
            result.unchecked.iter().any(|u| u.contains("Sicht 7")),
            "{name}: der Verlust der Schriftdekoder-Sicht wird nicht gesagt: {:?}",
            result.unchecked
        );
    }
    let peak = peak_rss_bytes();
    assert!(
        peak < 400_000_000,
        "VmHWM {} MB — eine der Bomben wurde entpackt",
        peak / 1_000_000
    );
}
