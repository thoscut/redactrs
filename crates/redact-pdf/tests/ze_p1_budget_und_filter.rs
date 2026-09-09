//! Gegenprüfung P1, Punkt 2 und 3: das Budget des Leck-Orakels und die neuen
//! begrenzten Filter (`weezl`/LZW, ASCII85, RunLength, Filterketten).
//!
//! Geprüft wird beides: die Grenze hält (keine Bombe, kein Absturz, keine
//! Endlosschleife) **und** sie sagt, was sie nicht geprüft hat — „nicht
//! gefunden“ darf nie stillschweigend „nicht gesucht“ heißen.
//!
//! Die Speichermessung (`VmHWM`) läuft im Kindprozess, weil der Spitzenwert
//! am Prozess hängt und die anderen Tests dieser Datei ihn sonst prägen. Sie
//! ist der einzige linuxspezifische Teil: `/proc/self/status` gibt es unter
//! Windows nicht. Dort prüft derselbe Test Frist, Fund und `unchecked` —
//! siehe [`peak_rss_bytes`].

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

/// Der Spitzenwert des Prozesses in Byte — `VmHWM` aus `/proc/self/status`.
///
/// `None` auf Zielen ohne `/proc` (Windows, macOS). Die Zusicherung „die
/// Bombe wurde nicht entpackt“ steht dort auf den Beinen, die überall
/// tragen: der Lauf bleibt in seiner Frist, findet nichts, und der Strom
/// steht mit seiner Objekt-Id in `unchecked`. Wer 512 MiB wirklich
/// auspackt, reißt die Frist auch ohne Speichermesser.
///
/// Unter Linux bleibt die Messung **scharf** — dort gibt es `/proc` immer.
/// Vorher las diese Funktion auf jedem Ziel `/proc` und brach mit `expect`
/// ab; der Windows-Job der CI war seit `f982c12` rot, obwohl der geprüfte
/// Code dort in Ordnung war.
#[cfg(target_os = "linux")]
fn peak_rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    Some(
        status
            .lines()
            .find_map(|l| l.strip_prefix("VmHWM:"))
            .and_then(|v| v.trim().strip_suffix("kB"))
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(|kb| kb * 1024)
            .expect("VmHWM"),
    )
}

#[cfg(not(target_os = "linux"))]
fn peak_rss_bytes() -> Option<u64> {
    None
}

/// Was der Kindprozess über seine Speichermessung sagt — der Elternprozess
/// liest daran ab, dass wirklich gemessen wurde.
fn peak_note(peak: Option<u64>) -> String {
    match peak {
        Some(bytes) => format!("VmHWM {} MB", bytes / 1_000_000),
        None => "ohne Speichermessung (kein /proc auf diesem Ziel)".to_string(),
    }
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
    let marke = if cfg!(target_os = "linux") {
        "VmHWM"
    } else {
        "ohne Speichermessung"
    };
    assert!(
        stderr.contains(marke),
        "der Kindprozess hat nicht gemessen (erwartet „{marke}“): {stderr}"
    );
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
             {elapsed:?}, {} (vorher {})",
            out_bytes / MIB,
            budget / MIB as u64,
            peak_note(peak),
            peak_note(before)
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
    // Die Speicherschranke gilt, wo sie messbar ist; die Prüfungen in der
    // Schleife oben (Frist, kein Fund, `unchecked` nennt den Strom) tragen
    // auf jedem Ziel.
    if let Some(peak) = peak_rss_bytes() {
        assert!(
            peak < 400_000_000,
            "VmHWM {} MB — eine der Bomben wurde entpackt",
            peak / 1_000_000
        );
    }
}

// ---------------------------------------------------------------------------
// Messung: was eine zusätzliche Kodierung je Begriff kostet
// ---------------------------------------------------------------------------

/// Wie teuer ist eine weitere Bytevariante in `Needle::new`?
///
/// Der Automat läuft **einmal** je Datenblock, gleich wie viele Muster er
/// kennt; teurer wird nur sein Bau. Gemessen wird beides zusammen: ein Lauf
/// über eine 8-MiB-Datei mit 1 und mit 200 Begriffen. Bleibt `#[ignore]` —
/// eine Zeitmessung auf einer geteilten Maschine gehört nicht ins Tor. Lauf:
/// `cargo test --release -p redact-pdf --test ze_p1_budget_und_filter -- --ignored --nocapture`
#[test]
#[ignore = "Messung"]
fn ze_p1_mess_kosten_je_kodierung() {
    let gross = fuellung(8 * MIB);
    let (pdf, _) = pdf_with_streams(vec![stream_with("FlateDecode".into(), deflate(&gross))]);
    eprintln!("Datei {} Byte", pdf.len());
    let viele: Vec<String> = (0..200)
        .map(|i| format!("Suchbegriff Nummer {i}"))
        .collect();
    let viele: Vec<&str> = viele.iter().map(String::as_str).collect();
    for (name, needles) in [("1 Begriff", &[SECRET][..]), ("200 Begriffe", &viele)] {
        let mut best = Duration::from_secs(999);
        for _ in 0..5 {
            let started = Instant::now();
            let r = leaks_many_within(&pdf, needles, u64::MAX);
            assert!(r.unchecked.is_empty());
            best = best.min(started.elapsed());
        }
        eprintln!("  {name}: {best:?}");
    }
}

// ---------------------------------------------------------------------------
// Fix-Runde 5: unbekanntes Kettenglied und Tiefengrenze
// ---------------------------------------------------------------------------

/// Bricht eine Filterkette an einem unbekannten Glied ab, bleibt **das
/// Entzifferte** durchsucht — und der Abbruch wird gesagt, wenn der Filter
/// wirklich unbekannt ist.
///
/// Bis Fix-Runde 4 warf `filters::decoded_content_within` die ganze Kette weg
/// (`Ok(None)`), und das Orakel bekam gar keine dekodierte Sicht; der
/// Klartext im Flate-Teil war weg (Befund P1-1).
///
/// Die Gegenrichtung steht daneben: `[/ASCII85Decode /DCTDecode]` ist die
/// gewöhnliche Ausgabe eines Distillers. Text in einem Rasterbild ist ein
/// benannter blinder Fleck — bei `/DCTDecode` allein genauso wie am Ende
/// einer Kette. Eine Datei mit einem Foto darf deshalb **nicht** als
/// „unvollständig geprüft“ zurückkommen.
#[test]
fn ein_unbekanntes_kettenglied_verliert_den_entzifferten_anfang_nicht() {
    let mut plain = text_ops(&[SECRET]);
    plain.extend_from_slice(&fuellung(4096));
    let content = common::ascii_hex_encode(&deflate(&plain));

    for (rest, gemeldet) in [("DCTDecode", false), ("PrivatFilter", true)] {
        let (pdf, ids) = pdf_with_streams(vec![stream_with(
            Object::Array(vec![
                "ASCIIHexDecode".into(),
                "FlateDecode".into(),
                rest.into(),
            ]),
            content.clone(),
        )]);
        let named = object(ids[0]);
        let result = leaks_many_within(&pdf, &[SECRET], u64::MAX);
        assert!(
            result.findings[0]
                .iter()
                .any(|h| h.contains(&named) && h.contains("danach /")),
            "{rest}: der entzifferte Anfang wurde nicht durchsucht: {:?}",
            result.findings[0]
        );
        let genannt = result
            .unchecked
            .iter()
            .any(|u| u.contains(&named) && u.contains(rest));
        assert_eq!(
            genannt, gemeldet,
            "{rest}: unchecked = {:?}",
            result.unchecked
        );
    }
}

/// Ein Strom, dessen **erstes** Glied schon unbekannt ist, bekommt keine
/// zweite, gleichlautende Meldung: die Rohbytes sind die Rohbytes, und die
/// Rohsicht hat sie schon durchsucht.
#[test]
fn ein_reines_bild_erzeugt_keine_doppelte_sicht() {
    let (pdf, ids) = pdf_with_streams(vec![stream_with("DCTDecode".into(), text_ops(&[SECRET]))]);
    let named = object(ids[0]);
    let result = leaks_many_within(&pdf, &[SECRET], u64::MAX);
    assert!(result.unchecked.is_empty(), "{:?}", result.unchecked);
    assert!(
        !result.findings[0].iter().any(|h| h.contains("dekodiert")),
        "ein nicht dekodierter Strom bekommt eine „dekodiert“-Sicht: {:?}",
        result.findings[0]
    );
    assert!(
        result.findings[0]
            .iter()
            .any(|h| h.contains(&named) && h.contains("Stream, roh")),
        "die Rohsicht fehlt: {:?}",
        result.findings[0]
    );
}

/// Ein Objekt tiefer als `audit_bytes::MAX_DEPTH` (32): die Objektsicht
/// bricht ab — und **sagt** es. Bis Fix-Runde 4 tat sie es stillschweigend,
/// und `--check-leaks` antwortete mit Rückgabewert 0 auf einen Text, den
/// keine Sicht gelesen hatte (Befund P4-2/B1).
///
/// Gemessen wird an der **Objektsicht**: nur ihre Fundstellen tragen den
/// Objektpfad. Dass die Rohsicht denselben Text findet, ändert nichts daran,
/// dass die Objektsicht abbricht — und genau dieser Abbruch muss in
/// `unchecked` stehen. (An der Kommandozeile ist der Fall schärfer: dort
/// steht der Text oktal maskiert in der Datei und keine Bytesuche sieht ihn,
/// `redact-cli/tests/ze_p4_check_leaks_grenzen.rs`.)
#[test]
fn die_tiefengrenze_der_objektsicht_meldet_sich() {
    for (tiefe, gefunden) in [(32usize, true), (33, false)] {
        let mut d = page(&[]);
        // Verschachtelt ist das **Objekt**, nicht ein Strominhalt: nur die
        // Objektsicht läuft hier in die Tiefe.
        let mut objekt = Object::String(SECRET.as_bytes().to_vec(), lopdf::StringFormat::Literal);
        for _ in 0..tiefe {
            objekt = Object::Array(vec![objekt]);
        }
        let tief_id = d.add(objekt);
        d.catalog_set("Tief", Object::Reference(tief_id));
        let pdf = d.finish();

        let result = leaks_many_within(&pdf, &[SECRET], u64::MAX);
        let objektsicht = result.findings[0]
            .iter()
            .any(|h| h.starts_with(&object(tief_id)));
        assert_eq!(
            objektsicht, gefunden,
            "Tiefe {tiefe}: {:?}",
            result.findings[0]
        );
        let gesagt = result
            .unchecked
            .iter()
            .any(|u| u.contains("Verschachtelungstiefe"));
        assert_eq!(
            gesagt, !gefunden,
            "Tiefe {tiefe}: unchecked = {:?}",
            result.unchecked
        );
    }
}

/// Was kostet die Tiefengrenze der Objektsicht?
///
/// Der Lader lässt 100 Ebenen zu, die Objektsicht läuft 32. Bevor man die
/// eine Zahl an die andere angleicht, muss man wissen, was tiefer Laufen
/// kostet: die Sicht baut je Knoten einen Pfad (`{path}[{i}]`), und der
/// wächst mit der Tiefe — die Arbeit ist Knoten × Tiefe, nicht Knoten.
///
/// Gemessen wird an einem bösartigen Objekt: `breite` Blätter auf **jeder**
/// Ebene bis `tiefe`. Bleibt `#[ignore]` (Zeitmessung). Lauf:
/// `cargo test --release -p redact-pdf --test ze_p1_budget_und_filter -- --ignored --nocapture mess_tiefe`
#[test]
#[ignore = "Messung"]
fn ze_p1_mess_tiefe_kostet() {
    // Höchstens 99 Wrapper: `lopdf` parst bis Tiefe 100 und lässt ein
    // tieferes Objekt ganz fallen — dann liefe die Sicht gar nicht.
    for (tiefe, breite) in [(31usize, 200usize), (99, 200), (99, 1000)] {
        let mut d = page(&[]);
        let blatt = || {
            Object::String(
                b"harmloser Text ohne Geheimnis".to_vec(),
                lopdf::StringFormat::Literal,
            )
        };
        let mut objekt = Object::Array((0..breite).map(|_| blatt()).collect());
        for _ in 0..tiefe {
            let mut ebene: Vec<Object> = (0..breite).map(|_| blatt()).collect();
            ebene.push(objekt);
            objekt = Object::Array(ebene);
        }
        let id = d.add(objekt);
        d.catalog_set("Tief", Object::Reference(id));
        let pdf = d.finish();
        // Gegenprobe: das Objekt hat den Lader überlebt.
        let doc = redact_pdf::load_from_bytes(&pdf).expect("ladbar");
        assert!(
            matches!(doc.get_object(id), Ok(Object::Array(_))),
            "Tiefe {tiefe}: `lopdf` hat das Objekt fallen lassen — die Messung misst nichts"
        );

        let mut best = Duration::from_secs(999);
        for _ in 0..3 {
            let started = Instant::now();
            let r = leaks_many_within(&pdf, &["kommtnichtvor"], u64::MAX);
            assert!(r.findings[0].is_empty());
            best = best.min(started.elapsed());
        }
        eprintln!(
            "Tiefe {tiefe}, Breite {breite}: Datei {} kB, {best:?}",
            pdf.len() / 1024
        );
    }
}
