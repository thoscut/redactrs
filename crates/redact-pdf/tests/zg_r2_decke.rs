//! Gegenprüfung R2 (nach Fix-Runde 6): **die Deckelung der Meldungen**.
//!
//! `LeakCheck::unchecked` ist je Sicht auf `MAX_UNCHECKED` (50) einzeln
//! genannte Stellen gedeckelt; was darüber liegt, steht als Summenzeile.
//! Gezählt wird in **zwei** Töpfen: `Budget::skip` (Entpackgrenze) und
//! `Budget::note` (unbekannter Filtername, Verschachtelungstiefe).
//!
//! Die Frage dieser Datei: kann ein Angreifer die Liste mit billigen
//! Meldungen füllen, bis die Zeile über **seinen** Strom mit dem Geheimnis
//! hinausfällt — und ist die Summenzeile ehrlich, wenn die Ursachen gemischt
//! sind?
//!
//! Antwort in zwei Teilen:
//!
//! * **Die Verdrängung gelingt nicht.** Die zwei Töpfe zählen getrennt: 60
//!   Ströme mit unbekanntem Filter füllen `note`, die Budget-Zeile des 61.
//!   Stroms steht in `skip` und bleibt wörtlich stehen. Und selbst im reinen
//!   Fall (61 Ströme an der Entpackgrenze) bleibt die Summenzeile mit der
//!   richtigen Restzahl, und `unchecked` ist nicht leer — der Rückgabewert
//!   bleibt 3.
//! * **BEFUND_R2_B:** die Zahl, die die Kommandozeile daraus macht, ist die
//!   Zahl der **Zeilen**, nicht der Stellen. 61 nicht geprüfte Ströme kommen
//!   als „52 Stelle(n) nicht geprüft“ heraus — direkt unter der Zeile, die
//!   „… und 11 weitere“ sagt.

mod common;

use std::io::Write;

use common::{page, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_pdf::{leaks_many_within, LeakCheck};

/// Eigener zlib-Packer.
fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("komprimierbar");
    e.finish().expect("komprimierbar")
}

/// Eigener RunLength-Kodierer (PDF 32000-1, 7.4.5): nur Literalläufe, dann
/// EOD (128). Er dient hier einem Zweck — die Nutzlast beginnt mit einem
/// Längenbyte und ist deshalb **kein** zlib-Strom, sodass die Rohsicht sie
/// nicht entpacken kann und allein die Objektsicht sie zu lesen hätte.
fn rl(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(128) {
        out.push(u8::try_from(chunk.len() - 1).expect("≤ 128"));
        out.extend_from_slice(chunk);
    }
    out.push(128);
    out
}

fn fuellung(size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size);
    while out.len() < size {
        out.extend_from_slice(b"% ABCABCABCABC ABCABCABCABC ABCABCABCABC\n");
    }
    out.truncate(size);
    out
}

/// Ein Strom mit unbekanntem Filternamen: billig, kostet kein Budget
/// (es wird nichts entpackt) — und erzeugt trotzdem eine `note`-Zeile.
fn fremder_filter(nummer: usize) -> Stream {
    Stream::new(
        dictionary! { "Filter" => Object::Name(b"ZgR2Fremd".to_vec()) },
        format!("Fuellstrom {nummer}, harmlos, nichts zu holen\n").into_bytes(),
    )
    .with_compression(false)
}

/// Ein Strom, der das Budget sprengt: `[/RunLengthDecode /FlateDecode]` über
/// zlib-gepacktem Klartext. Die Vorprüfung des Laders packt nur reine
/// Flate/LZW/ASCII85-Ketten aus und sieht ihn deshalb nicht.
fn ueber_budget(plain: &[u8]) -> Stream {
    Stream::new(
        dictionary! {
            "Filter" => Object::Array(vec!["RunLengthDecode".into(), "FlateDecode".into()]),
        },
        rl(&zlib(plain)),
    )
    .with_compression(false)
}

fn pdf(streams: Vec<Stream>) -> (Vec<u8>, Vec<ObjectId>) {
    let mut d = page(&["harmlos"]);
    let mut ids = Vec::new();
    for s in streams {
        ids.push(d.add(Object::Stream(s)));
    }
    (d.finish(), ids)
}

fn objekt(id: ObjectId) -> String {
    format!("Objekt {} {}", id.0, id.1)
}

/// Die Zahl aus einer Summenzeile („… und 11 weitere …“).
fn rest(check: &LeakCheck, schwanz: &str) -> Option<u64> {
    check
        .unchecked
        .iter()
        .find(|l| l.starts_with("… und ") && l.ends_with(schwanz))
        .and_then(|l| l.split_whitespace().nth(2))
        .and_then(|n| n.parse().ok())
}

// ---------------------------------------------------------------------------
// 1. Die Verdrängung gelingt nicht — zwei Töpfe
// ---------------------------------------------------------------------------

/// 60 Ströme mit unbekanntem Filter, danach der 61. mit dem Geheimnis an der
/// Entpackgrenze: seine Zeile steht **wörtlich** da, mit Objekt-Id und Grund.
/// Die 60 billigen Meldungen füllen einen anderen Topf (`note`), und der
/// deckelt sich selbst.
///
/// Mutationsnachweis (gefahren): in `audit_bytes::Budget` `skip` und `note`
/// denselben Zähler benutzen lassen (`self.skipped` in `note`) → rot, die
/// Budget-Zeile des Geheimnisstroms fehlt.
#[test]
fn r2_sechzig_fremde_filter_verdraengen_die_budgetzeile_nicht() {
    let geheim = fuellung(256 * 1024);
    let mut geheim = geheim;
    geheim.extend_from_slice(format!("\nIBAN {SECRET}\n").as_bytes());

    let mut streams: Vec<Stream> = (0..60).map(fremder_filter).collect();
    streams.push(ueber_budget(&geheim));
    let (bytes, ids) = pdf(streams);
    let opfer = objekt(*ids.last().expect("ein Strom"));

    let check = leaks_many_within(&bytes, &[SECRET], 64 * 1024);

    assert!(
        check.findings[0].is_empty(),
        "der Klartext liegt zlib-gepackt hinter RunLength — keine Sicht sieht ihn: {:#?}",
        check.findings[0]
    );
    assert!(
        check
            .unchecked
            .iter()
            .any(|l| l.starts_with(&format!("{opfer} <Stream>: nicht entpackt"))),
        "die Zeile über den Strom mit dem Geheimnis wurde verdrängt: {:#?}",
        check.unchecked
    );
    // Der andere Topf hat sich selbst gedeckelt und sagt es.
    let fremde = check
        .unchecked
        .iter()
        .filter(|l| l.contains("/ZgR2Fremd ist hier kein bekannter Filter"))
        .count();
    assert_eq!(fremde, 50, "{:#?}", check.unchecked);
    assert_eq!(
        rest(&check, "weitere Stellen nicht geprüft"),
        Some(10),
        "{:#?}",
        check.unchecked
    );
}

/// Der reine Fall: 61 Ströme an der Entpackgrenze, der letzte mit dem
/// Geheimnis. Seine eigene Zeile fällt heraus — aber die Summenzeile nennt
/// die Restzahl, `unchecked` bleibt nicht leer (Rückgabewert 3), und keine
/// Meldung behauptet, es sei alles geprüft.
#[test]
fn r2_bei_reiner_ueberschreitung_traegt_die_summenzeile_die_last() {
    let fuell = fuellung(256 * 1024);
    let mut geheim = fuell.clone();
    geheim.extend_from_slice(format!("\nIBAN {SECRET}\n").as_bytes());

    let mut streams: Vec<Stream> = (0..60).map(|_| ueber_budget(&fuell)).collect();
    streams.push(ueber_budget(&geheim));
    let (bytes, ids) = pdf(streams);
    let opfer = objekt(*ids.last().expect("ein Strom"));

    let check = leaks_many_within(&bytes, &[SECRET], 64 * 1024);

    assert!(check.findings[0].is_empty(), "{:#?}", check.findings[0]);
    assert!(
        !check
            .unchecked
            .iter()
            .any(|l| l.starts_with(&format!("{opfer} <Stream>:"))),
        "erwartet: der 61. fällt aus der Einzelnennung heraus"
    );
    assert_eq!(
        rest(&check, "weitere Ströme nicht entpackt"),
        Some(11),
        "{:#?}",
        check.unchecked
    );
    assert!(!check.unchecked.is_empty());
}

/// Gemischte Ursachen: 60 fremde Filter **und** 60 Ströme über dem Budget.
/// Beide Summenzeilen stehen da, jede mit ihrer eigenen Restzahl — keine
/// zählt die Stellen der anderen mit.
#[test]
fn r2_zwei_summenzeilen_bei_gemischten_ursachen() {
    let fuell = fuellung(256 * 1024);
    let mut streams: Vec<Stream> = (0..60).map(fremder_filter).collect();
    streams.extend((0..60).map(|_| ueber_budget(&fuell)));
    let (bytes, _) = pdf(streams);

    let check = leaks_many_within(&bytes, &[SECRET], 64 * 1024);

    assert_eq!(
        rest(&check, "weitere Stellen nicht geprüft"),
        Some(10),
        "{:#?}",
        check.unchecked
    );
    assert_eq!(
        rest(&check, "weitere Ströme nicht entpackt"),
        Some(10),
        "{:#?}",
        check.unchecked
    );
}

// ---------------------------------------------------------------------------
// 2. BEFUND_R2_B — die Zahl zählt Zeilen, nicht Stellen
// ---------------------------------------------------------------------------

/// **BEFUND_R2_B** — `unchecked.len()` ist die Zahl der **Meldungen**; die
/// Kommandozeile gibt sie als Zahl der **Stellen** aus
/// (`redact-cli/src/check.rs`: `let unchecked = check.unchecked.len();` →
/// „{unchecked} Stelle(n) nicht geprüft“).
///
/// Bei 61 übersprungenen Strömen sind das 50 Einzelzeilen + 1 Summenzeile
/// + 1 Zeile „Sicht 7 nicht gelaufen“ = 52 — für 61 nicht geprüfte Ströme.
/// Die Summenzeile darüber sagt „… und 11 weitere“, die Schlusszeile nennt
/// 52: zwei Zahlen zu derselben Sache, und die zusammenfassende ist die
/// kleinere. Das ist die Fehlerklasse „eine Decke zählt die falsche Einheit“.
///
/// Sicherheitswirkung: keine — `unchecked` ist nicht leer, der Rückgabewert
/// bleibt 3. Es ist eine falsche Aussage der Oberfläche über den eigenen
/// Umfang.
///
/// Vorschlag: `LeakCheck` trägt die Zahl der Stellen selbst
/// (`Budget::{skipped, noted}` aufsummiert), und die Kommandozeile gibt sie
/// aus statt `unchecked.len()`.
#[test]
fn befund_r2_b_die_schlusszahl_zaehlt_zeilen_nicht_stellen() {
    let fuell = fuellung(256 * 1024);
    let streams: Vec<Stream> = (0..61).map(|_| ueber_budget(&fuell)).collect();
    let (bytes, _) = pdf(streams);

    let check = leaks_many_within(&bytes, &[SECRET], 64 * 1024);

    let einzeln = check
        .unchecked
        .iter()
        .filter(|l| l.contains("<Stream>: nicht entpackt"))
        .count();
    let weitere = rest(&check, "weitere Ströme nicht entpackt").expect("Summenzeile");
    let wahr = einzeln as u64 + weitere;

    assert_eq!((einzeln, weitere), (50, 11), "{:#?}", check.unchecked);
    assert_eq!(wahr, 61, "61 Ströme wurden nicht entpackt");
    assert!(
        (check.unchecked.len() as u64) < wahr,
        "BEFUND_R2_B geschlossen? Dann diese Zusicherung umdrehen: {} Meldungen \
         gegen {wahr} Stellen",
        check.unchecked.len()
    );
    assert_eq!(
        check.unchecked.len(),
        52,
        "50 Einzelzeilen + Summenzeile + Sicht 7: {:#?}",
        check.unchecked
    );
}

/// Schreibt die Datei zu [`befund_r2_b_die_schlusszahl_zaehlt_zeilen_nicht_stellen`]
/// dorthin, wohin `ZG_R2_DECKE_PDF` zeigt — damit der Befund an der
/// **Kommandozeile** vorgeführt werden kann:
///
/// ```text
/// cargo test -p redact-pdf --test zg_r2_decke -- --ignored r2_schreibe
/// cargo run -p redact-cli -- --check-leaks "DE89 3704 0044 0532 0130 00" \
///     --max-decompressed-mb 1 <datei>
/// ```
#[test]
#[ignore = "schreibt nur eine Beispieldatei"]
fn r2_schreibe_beispieldatei() {
    let Ok(pfad) = std::env::var("ZG_R2_DECKE_PDF") else {
        return;
    };
    let fuell = fuellung(256 * 1024);
    let streams: Vec<Stream> = (0..61).map(|_| ueber_budget(&fuell)).collect();
    let (bytes, _) = pdf(streams);
    std::fs::write(&pfad, &bytes).expect("schreibbar");
    eprintln!("geschrieben: {pfad} ({} Byte)", bytes.len());
}

// ---------------------------------------------------------------------------
// 3. Die Gegenrichtung: meldet die Decke etwas, das gar nicht fehlt?
// ---------------------------------------------------------------------------

/// **Gegenrichtung** und **BEFUND_R2_D**: ein Strom, der **gar nicht gepackt**
/// ist, und ein knappes Budget.
///
/// Zwei Dinge werden hier auseinandergehalten:
///
/// * Die Rohsicht (Sicht 2) probiert an jedem `stream … endstream`-Block erst
///   zlib und dann rohes Deflate ab Byte 2 (`filters::inflate_within`). Sie
///   erzeugt an diesem Material **keine** Phantom-Zeile „nicht entpackt“ —
///   gut, denn an einem ungepackten Strom gibt es nichts zu entpacken, und
///   seine Bytes sind vollständig durchsucht.
/// * Die **Vorprüfung des Laders** (`document::prescan`) verbucht die Bytes
///   eines ungefilterten Stroms trotzdem gegen das Entpackbudget und lehnt
///   die Datei ab — mit der Begründung „Das ist das Muster einer
///   Dekompressionsbombe: eine kleine Datei, die sich beim Öffnen
///   vervielfacht.“ Diese Datei ist 2 MB groß und enthält 2 MB Klartext: sie
///   vervielfacht sich beim Öffnen um den Faktor 1. Die Zahl stimmt (das
///   Budget zählt die Summe der entpackten Bytes, und die ist hier gleich der
///   rohen), die **Begründung** nicht — und `leaks_many_within` reicht sie
///   wörtlich weiter.
///
/// Schwere: gering, keine Sicherheitswirkung — eine falsche Ursache in einer
/// Fehlermeldung, die den Leser eine Bombe suchen lässt, die es nicht gibt.
/// Vorschlag: die Bomben-Begründung nur nennen, wenn entpackt ≫ gepackt ist;
/// sonst „die Summe der Stromdaten überschreitet das Budget“.
#[test]
fn befund_r2_d_ungepackter_strom_heisst_dekompressionsbombe() {
    let text = fuellung(2 * 1024 * 1024);
    let (bytes, _) = pdf(vec![
        Stream::new(dictionary! {}, text).with_compression(false)
    ]);
    assert!(bytes.len() < 3 * 1024 * 1024, "{} Byte", bytes.len());

    // Weites Budget: kein Wort.
    let weit = leaks_many_within(&bytes, &[SECRET], u64::MAX);
    assert!(weit.unchecked.is_empty(), "{:#?}", weit.unchecked);

    // Knappes Budget: genau eine Meldung, und sie kommt vom Lader — die
    // Rohsicht erfindet keine zweite.
    let eng = leaks_many_within(&bytes, &[SECRET], 1024 * 1024);
    assert!(
        !eng.unchecked
            .iter()
            .any(|l| l.contains("Rohdaten-Stream") && l.contains("nicht entpackt")),
        "Phantom-Zeile der Rohsicht an einem ungepackten Strom: {:#?}",
        eng.unchecked
    );
    assert_eq!(eng.unchecked.len(), 1, "{:#?}", eng.unchecked);
    assert!(
        eng.unchecked[0].contains("Muster einer Dekompressionsbombe")
            && eng.unchecked[0].contains("eine kleine Datei, die sich beim Öffnen vervielfacht"),
        "BEFUND_R2_D geschlossen? Dann diese Zusicherung umdrehen: {:#?}",
        eng.unchecked
    );
}
