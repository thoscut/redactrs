//! Gegenprüfung P1: eine **unabhängige** naive Referenz für die Rohdatei-Sicht
//! des Leck-Orakels — eigener Code, eigene Kodierungstabelle, eigene Suche.
//!
//! Sicht 1 (`Rohdatei @0x…`) ist die einzige, deren Meldungstext sich von außen
//! Zeichen für Zeichen vorhersagen lässt: Fundstelle, Kodierungsname,
//! Kontextfenster. Damit wird der Umbau von `memmem` auf einen
//! Aho-Corasick-Automaten (Commit f982c12) überprüfbar, ohne den Code des
//! Baums zu benutzen: findet der Automat *dieselben* Stellen in *derselben*
//! Reihenfolge mit *demselben* Kontext?

use redact_pdf::leaks_many;

const CONTEXT: usize = 24;
/// `scan_raw_file` nimmt je Muster höchstens acht Fundstellen.
const RAW_LIMIT: usize = 8;

// ---------------------------------------------------------------------------
// Die Referenz — bewusst dumm und langsam
// ---------------------------------------------------------------------------

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

/// Alle Kodierungen eines Begriffs, wie das Modul sie beschreibt.
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
    let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    out.push(("UTF-16BE".into(), utf16.clone()));
    out.push(("Hex-String (UTF-16BE, gross)".into(), hex(&utf16, true)));
    out.push(("Hex-String (UTF-16BE, klein)".into(), hex(&utf16, false)));
    let utf16le: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.push(("UTF-16LE".into(), utf16le.clone()));
    out.push(("Hex-String (UTF-16LE, gross)".into(), hex(&utf16le, true)));
    out.push(("Hex-String (UTF-16LE, klein)".into(), hex(&utf16le, false)));
    // Aufeinanderfolgende Doppelungen fallen weg (`variants.dedup_by`).
    let mut deduped: Vec<(String, Vec<u8>)> = Vec::new();
    for v in out {
        if deduped.last().map(|l: &(String, Vec<u8>)| &l.1) == Some(&v.1) {
            continue;
        }
        deduped.push(v);
    }
    deduped
}

/// Byte für Byte, überlappungsfrei je Muster, höchstens `limit`.
fn naive_positions(hay: &[u8], pat: &[u8], limit: usize) -> Vec<usize> {
    if pat.is_empty() || pat.len() > hay.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + pat.len() <= hay.len() {
        if &hay[i..i + pat.len()] == pat {
            out.push(i);
            if out.len() == limit {
                break;
            }
            i += pat.len();
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

/// Was Sicht 1 für `needle` in `bytes` melden **muss** — Reihenfolge inklusive.
fn expected_raw_hits(bytes: &[u8], needle: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (how, pat) in variants(needle) {
        for pos in naive_positions(bytes, &pat, RAW_LIMIT) {
            let msg = format!(
                "Rohdatei @0x{pos:x} [{how}]: …{}…",
                context(bytes, pos, pat.len())
            );
            if !out.contains(&msg) {
                out.push(msg);
            }
        }
    }
    out
}

/// Die Rohdatei-Sicht läuft als erste; ihre Meldungen stehen deshalb am Anfang
/// der Liste, in genau dieser Reihenfolge.
fn assert_raw_view_matches(label: &str, bytes: &[u8], needles: &[&str]) {
    let got = leaks_many(bytes, needles);
    assert_eq!(got.len(), needles.len(), "{label}");
    for (needle, hits) in needles.iter().zip(&got) {
        let want = expected_raw_hits(bytes, needle);
        let head: Vec<String> = hits.iter().take(want.len()).cloned().collect();
        assert_eq!(
            head, want,
            "{label}: Rohdatei-Sicht weicht ab für {needle:?}\nalle Fundstellen: {hits:#?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Material
// ---------------------------------------------------------------------------

/// Überlappende Begriffe, Präfixe, Doppelungen in der Liste, leerer Begriff,
/// Begriff mit Leerraum am Rand, Nicht-ASCII.
#[test]
fn die_rohdatei_sicht_findet_was_die_naive_suche_findet() {
    let mut material: Vec<u8> = Vec::new();
    material.extend_from_slice(b"%PDF-1.7\n(Max Mustermann und MAX und Mustermann)\n");
    material.extend_from_slice(b"(DE89 3704 0044 DE89 3704 DE89)\n");
    material.extend_from_slice(b"<4D6178204D75737465726D616E6E>\n");
    material.extend_from_slice(b"<4d6178204d75737465726d616e6e>\n");
    material.extend_from_slice(b"(");
    material.extend_from_slice(&[0xfe, 0xff]);
    material.extend("Max Mustermann".encode_utf16().flat_map(u16::to_be_bytes));
    material.extend_from_slice(b")\n");
    material.extend_from_slice(b"(");
    material.extend_from_slice(&[0xff, 0xfe]);
    material.extend("Max Mustermann".encode_utf16().flat_map(u16::to_le_bytes));
    material.extend_from_slice(b")\n");
    material.extend_from_slice("(Grüße Müller — 😀 e\u{0301})\n".as_bytes());
    material.extend_from_slice(b"aaaaaaaaaa\n");
    material.extend_from_slice(b"% Zeilenumbruch mitten im Text: Max\nMustermann\n");

    let needles = [
        "Max Mustermann",
        "Mustermann",
        "MAX",
        "Max",
        "DE89",
        "DE89 3704",
        "89 37",
        "Max Mustermann",
        "",
        " Max ",
        "Grüße Müller",
        "😀",
        "e\u{0301}",
        "aaaa",
        "kommtnichtvor",
    ];
    assert_raw_view_matches("gemischtes Material", &material, &needles);
    // Der Test misst wirklich etwas.
    let got = leaks_many(&material, &needles);
    assert!(!got[0].is_empty() && !got[2].is_empty() && !got[10].is_empty());
    assert!(got[8].is_empty(), "leerer Begriff: {:?}", got[8]);
    assert!(got[14].is_empty(), "Nicht-Treffer: {:?}", got[14]);
}

/// Ein Begriff, der ganz in einem anderen steckt, darf nicht verloren gehen —
/// `find_iter` von aho-corasick liefert je nach `MatchKind` nicht alle Treffer.
#[test]
fn ein_begriff_im_anderen_geht_nicht_verloren() {
    let pdf = b"%PDF-1.5\n(Konto von Max Mustermann)\n".to_vec();
    let got = leaks_many(
        &pdf,
        &[
            "MAX MUSTERMANN",
            "Max Mustermann",
            "Mustermann",
            "Max",
            "ustermann",
        ],
    );
    for (i, name) in [
        "MAX MUSTERMANN",
        "Max Mustermann",
        "Mustermann",
        "Max",
        "ustermann",
    ]
    .iter()
    .enumerate()
    {
        let expect_hit = *name != "MAX MUSTERMANN";
        assert_eq!(!got[i].is_empty(), expect_hit, "{name}: {:?}", got[i]);
    }
    assert_raw_view_matches(
        "verschachtelte Begriffe",
        &pdf,
        &["Max Mustermann", "Mustermann", "Max"],
    );
}

/// Der Begriff steht genau an der Grenze: am Dateianfang, am Dateiende, und
/// öfter als `MAX_HITS`/das Limit der Rohsicht erlaubt.
#[test]
fn grenzfaelle_anfang_ende_und_limit() {
    let am_anfang = b"GEHEIMxxx".to_vec();
    assert_raw_view_matches("am Anfang", &am_anfang, &["GEHEIM"]);
    let am_ende = b"xxxGEHEIM".to_vec();
    assert_raw_view_matches("am Ende", &am_ende, &["GEHEIM"]);
    let genau = b"GEHEIM".to_vec();
    assert_raw_view_matches("genau der Begriff", &genau, &["GEHEIM"]);
    let laenger = b"GEHEI".to_vec();
    assert_raw_view_matches("Begriff länger als die Datei", &laenger, &["GEHEIM"]);

    // 40 Vorkommen, die Rohsicht nimmt acht.
    let viele: Vec<u8> = "GEHEIM ".repeat(40).into_bytes();
    assert_raw_view_matches("viele Vorkommen", &viele, &["GEHEIM"]);
    let hits = leaks_many(&viele, &["GEHEIM"]);
    let roh = hits[0].iter().filter(|h| h.starts_with("Rohdatei")).count();
    assert_eq!(roh, 8, "die Rohsicht nimmt genau acht: {:?}", hits[0]);
}

/// Dieselben Begriffe an den Dateien, die im Baum liegen, und an der Vorlage.
#[test]
fn die_rohdatei_sicht_an_echten_dateien() {
    let needles = [
        "DE89 3704 0044 0532 0130 00",
        "Max Mustermann",
        "Musterbank",
        "532013000",
        "Kontonummer",
        "",
        "gibtesnicht",
    ];
    let demo = redact_pdf::testing::demo_statement();
    assert_raw_view_matches("demo_statement", &demo, &needles);
    let minimal = redact_pdf::testing::minimal_pdf("IBAN DE89 3704 0044 0532 0130 00");
    assert_raw_view_matches("minimal_pdf", &minimal, &needles);
}

/// Eine Zeichenkette mit UTF-16**LE**-BOM (`FF FE`) — `decode_pdf_string`
/// kennt diese Form ausdrücklich („in freier Wildbahn auch `FF FE`“).
///
/// Seit Fix-Runde 5 kennt auch [`variants`] eine LE-Bytefassung, damit
/// LE-Bytes **in einem Strom** nicht unsichtbar bleiben. Damit dieser Test
/// weiterhin den **Dekoder** misst und nicht bloß die neue Bytefassung,
/// verlangt er eine Fundstelle aus der Zeichenketten-Sicht: deren Meldung
/// trägt genau `[Zeichenkette, hex]`, während die Bytefassungen ihren
/// Kodierungsnamen anhängen (`[Zeichenkette, hex, UTF-16LE]`).
#[test]
fn utf16le_zeichenketten_werden_gefunden() {
    use lopdf::{dictionary, Document, Object, StringFormat};

    const GEHEIM: &str = "Max Mustermann";
    let mut le: Vec<u8> = vec![0xff, 0xfe];
    le.extend(GEHEIM.encode_utf16().flat_map(u16::to_le_bytes));

    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages });
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    let info =
        doc.add_object(dictionary! { "Title" => Object::String(le, StringFormat::Hexadecimal) });
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", info);
    let mut pdf = Vec::new();
    doc.save_to(&mut pdf).expect("PDF speicherbar");

    let hits = leaks_many(&pdf, &[GEHEIM, "kommtnichtvor"]);
    assert!(
        !hits[0].is_empty(),
        "UTF-16LE-Zeichenkette nicht gefunden: {:?}",
        hits[0]
    );
    assert!(
        hits[0].iter().any(|h| h.contains("[Zeichenkette, hex]")),
        "der Dekoder hat die LE-Zeichenkette nicht gelesen — gefunden wurde \
         sie nur als Bytefolge: {:?}",
        hits[0]
    );
    assert!(hits[1].is_empty(), "{:?}", hits[1]);
}

/// UTF-16**LE**-Bytes **in einem Strom** — ohne Zeichenketten-Syntax drumherum.
///
/// Hier hilft kein Dekoder: `decode_pdf_string` läuft nur an einem
/// Zeichenketten-**Objekt** und an `(…)`/`<…>` innerhalb eines Blocks. Diese
/// Bytes stehen nackt im entpackten Strom — gefunden werden sie nur, wenn
/// `Needle::new` eine LE-Bytefassung kennt (Fix-Runde 5). Der Strom ist
/// Flate-gepackt, damit auch die Rohdatei-Sicht ihn nicht sieht.
#[test]
fn utf16le_bytes_in_einem_strom_werden_gefunden() {
    use std::io::Write;

    use lopdf::{dictionary, Document, Object, Stream};

    const GEHEIM: &str = "Max Mustermann";
    let mut plain = b"BT ET % ".to_vec();
    plain.extend(GEHEIM.encode_utf16().flat_map(u16::to_le_bytes));
    // Gut komprimierbar, damit deflate wirklich packt statt „stored“ zu legen.
    plain.extend_from_slice(&b"ABCABCABCABC ".repeat(200));

    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&plain).expect("deflate");
    let packed = enc.finish().expect("deflate");

    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let content = doc.add_object(Object::Stream(
        Stream::new(dictionary! { "Filter" => "FlateDecode" }, packed).with_compression(false),
    ));
    let page =
        doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "Contents" => content });
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut pdf = Vec::new();
    doc.save_to(&mut pdf).expect("PDF speicherbar");

    // Die Gegenprobe: roh steht der Text nicht in der Datei.
    assert!(
        !pdf.windows(GEHEIM.len()).any(|w| w == GEHEIM.as_bytes()),
        "der Klartext steht unverpackt in der Datei — der Test misst nichts"
    );

    let hits = leaks_many(&pdf, &[GEHEIM, "kommtnichtvor"]);
    assert!(
        hits[0].iter().any(|h| h.contains("UTF-16LE")),
        "LE-Bytes im Strom nicht gefunden: {:?}",
        hits[0]
    );
    assert!(hits[1].is_empty(), "{:?}", hits[1]);
}

/// `leaks_many(b, &[x])[0] == leaks(b, x)` — die Zusicherung, an der alles
/// hängt: die Zusammenfassung mehrerer Begriffe in **einen** Automaten darf
/// keine einzige Fundstelle kosten. Geprüft an vielfältigem Material, nicht
/// nur an der Vorlage.
#[test]
fn ein_durchgang_meldet_dasselbe_wie_viele() {
    let mut material: Vec<(&str, Vec<u8>)> = vec![
        ("demo_statement", redact_pdf::testing::demo_statement()),
        (
            "minimal",
            redact_pdf::testing::minimal_pdf("IBAN DE89 3704 0044 0532 0130 00"),
        ),
        ("leer", Vec::new()),
        ("kein PDF", b"nur Text mit Max Mustermann drin".to_vec()),
        (
            "kaputter Kopf",
            b"%PDF-9.9\n1 0 obj\nstream\nDE89 3704\nendstream".to_vec(),
        ),
    ];
    // Die Dateien, die im Baum liegen.
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "pdf") {
            let name: &'static str = Box::leak(
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
                    .into_boxed_str(),
            );
            material.push((name, std::fs::read(&path).unwrap()));
        }
    }

    let needles = [
        "DE89 3704 0044 0532 0130 00",
        "DE89",
        "DE89 3704",
        "89 37",
        "Max Mustermann",
        "Mustermann",
        "Max",
        "",
        "Kontonummer",
        "kommtnichtvor",
        "Grüße",
    ];
    for (name, bytes) in &material {
        let many = leaks_many(bytes, &needles);
        assert_eq!(many.len(), needles.len(), "{name}");
        for (needle, hits) in needles.iter().zip(&many) {
            assert_eq!(
                hits,
                &redact_pdf::leaks(bytes, needle),
                "{name}: abweichende Fundstellen für {needle:?}"
            );
        }
    }
}

/// Die Decke für gemeldete Fundstellen (`MAX_HITS`, 200) gilt **je Begriff**,
/// nicht für den Lauf: ein Begriff mit sehr vielen Vorkommen darf einen
/// anderen nicht um seine Meldung bringen.
#[test]
fn die_deckelung_gilt_je_begriff() {
    let mut material = b"%PDF-1.5\n".to_vec();
    for i in 0..600 {
        material.extend_from_slice(format!("({i} VIELFACH x)\n").as_bytes());
    }
    material.extend_from_slice(b"(SELTEN)\n");
    let hits = leaks_many(&material, &["VIELFACH", "SELTEN"]);
    assert!(
        hits[0].len() <= 200,
        "die Decke greift nicht: {}",
        hits[0].len()
    );
    assert!(
        !hits[1].is_empty(),
        "der seltene Begriff ist unter die Räder gekommen: {:?}",
        hits[1]
    );
}
