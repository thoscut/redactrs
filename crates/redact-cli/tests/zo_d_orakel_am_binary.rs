//! Spur-A-Runde 1, Prüfer D: dieselben Fragen wie
//! `crates/redact-pdf/tests/zo_d_altgeneration_und_kodierung.rs`, gemessen am
//! **gebauten Binary** — Rückgabewert und `NICHT GEPRÜFT`-Zeile, nicht die
//! Bibliotheksfunktion.
//!
//! * [`zo_d10_probenliste_filterkette_am_binary`] — eigener Lauf zur
//!   Probenlisten-Zeile „Filterkette“ (erwartet grün).
//! * [`zo_d11_altgeneration_am_binary_stilles_leck`] — **ABSICHTLICH ROT**:
//!   ein inkrementelles Update lässt die Vorgängerfassung in der Datei; unter
//!   `/ASCII85Decode` bzw. als oktal maskierte Zeichenkette sagt
//!   `--check-leaks` „nicht gefunden“, Rückgabewert 0.
//! * [`zo_d12_pdfdoc_in_der_ausgabedatei_stilles_leck`] — **ABSICHTLICH
//!   ROT**: ein `/ActualText` in PDFDocEncoding (`€` = 0xA0) überlebt den
//!   Schwärzungslauf in die **Ausgabedatei**, und `--check-leaks "Betrag 5 €"`
//!   an der Ausgabedatei sagt „nicht gefunden“, Rückgabewert 0.
//! * [`zo_d13_verschluesselt_wird_ehrlich_abgelehnt`] — Kontrolle: eine
//!   verschlüsselte Datei wird mit Meldung abgelehnt (Rückgabewert 1), nicht
//!   still durchgewunken; `--password` ist mit `--check-leaks` nicht
//!   kombinierbar (Rückgabewert 2).
//! * [`zo_d14_altlast_filter_ueber_16_mib_wird_abgelehnt`] — **ABSICHTLICH
//!   ROT (Klasse: abgelehnte Datei, Einordnung offen)**: ein Bild-Strom unter
//!   `/ASCII85Decode` mit mehr als 16 MiB Rohgröße lässt die Vorprüfung die
//!   ganze Datei ablehnen (Rückgabewert 1, keine Ausgabedatei) — auch
//!   `--check-leaks` lehnt sie ab, und `--max-decompressed-mb` ändert nichts.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "redact-zo-d-{}-{name}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .current_dir(dir)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .expect("Binary startbar")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("Prozess endet mit Rückgabewert")
}

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

// ---------------------------------------------------------------------------
// Rohe PDF-Dateien — Kopie der Bauweise aus dem Bibliothekstest
// ---------------------------------------------------------------------------

fn assemble(objects: &[(u32, Vec<u8>)]) -> (Vec<u8>, usize) {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (id, body) in objects {
        offsets.push((*id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let max = objects.iter().map(|(id, _)| *id).max().unwrap_or(0) + 1;
    out.extend_from_slice(format!("xref\n0 {max}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for id in 1..max {
        match offsets.iter().find(|(o, _)| *o == id) {
            Some((_, at)) => out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {max} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    (out, xref)
}

fn append_revision(mut file: Vec<u8>, prev_xref: usize, objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut offsets = Vec::new();
    for (id, body) in objects {
        offsets.push(file.len());
        file.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        file.extend_from_slice(body);
        file.extend_from_slice(b"\nendobj\n");
    }
    let first = objects[0].0;
    let xref = file.len();
    file.extend_from_slice(
        format!(
            "xref\n0 1\n0000000000 65535 f \n{first} {}\n",
            objects.len()
        )
        .as_bytes(),
    );
    for at in offsets {
        file.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes());
    }
    let size = first + objects.len() as u32;
    file.extend_from_slice(
        format!(
            "trailer\n<< /Size {size} /Root 1 0 R /Prev {prev_xref} >>\nstartxref\n{xref}\n%%EOF\n"
        )
        .as_bytes(),
    );
    file
}

fn stream_object(dict_entries: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} {dict_entries} >>\nstream\n", payload.len()).into_bytes();
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\nendstream");
    out
}

const FONT: &str = "/Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                    /Encoding /WinAnsiEncoding >> >>";

fn skeleton(annots: bool, resources: &str) -> Vec<(u32, Vec<u8>)> {
    let annots = if annots { "/Annots [5 0 R]" } else { "" };
    vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R {annots} \
                 /Resources << {resources} >> >>"
            )
            .into_bytes(),
        ),
    ]
}

const HARMLOS_CONTENT: &[u8] = b"BT /F1 10 Tf 72 700 Td (geschwaerzt) Tj ET";
const HARMLOS_ANNOT: &[u8] =
    b"<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents (geschwaerzt) >>";

fn a85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 5 / 4 + 2);
    for c in data.chunks(4) {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        let mut v = u32::from_be_bytes(w);
        let mut g = [0u8; 5];
        for s in g.iter_mut().rev() {
            *s = b'!' + u8::try_from(v % 85).expect("< 85");
            v /= 85;
        }
        out.extend_from_slice(&g[..c.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

fn xor(text: &str) -> Vec<u8> {
    text.bytes().map(|b| b ^ 0x5A).collect()
}

fn with_old_generation(old_stream: Vec<u8>, old_annot: Vec<u8>) -> Vec<u8> {
    let mut objects = skeleton(true, FONT);
    objects.push((4, old_stream));
    objects.push((5, old_annot));
    let (file, xref) = assemble(&objects);
    append_revision(
        file,
        xref,
        &[
            (4, stream_object("", HARMLOS_CONTENT)),
            (5, HARMLOS_ANNOT.to_vec()),
        ],
    )
}

fn found_or_unchecked(text: &str) -> bool {
    text.contains("  GEFUNDEN (") || text.contains("  NICHT GEPRÜFT: ")
}

// ---------------------------------------------------------------------------
// Probenliste
// ---------------------------------------------------------------------------

/// Eigener Lauf zur Probenlisten-Zeile am Binary: unbekannter Filter an
/// erster Stelle, `/Filter`-Wert kein Name, Kette mit namenlosem Glied — je
/// Rückgabewert 3 und eine Zeile, die den Grund nennt.
#[test]
fn zo_d10_probenliste_filterkette_am_binary() {
    let dir = workdir("probenliste");
    let plain = format!("BT (IBAN {SECRET}) Tj ET");
    let faelle: Vec<(&str, &str, Vec<u8>, bool, &str)> = vec![
        (
            "fremd_zuerst",
            "/ZoFremd",
            xor(&plain),
            false,
            "kein bekannter Filter",
        ),
        (
            "fremd_vorne",
            "[/ZoFremd /FlateDecode]",
            xor(&plain),
            false,
            "Glied 1 von 2",
        ),
        ("zahl", "42", xor(&plain), false, "kein Filtername"),
        (
            "dictionary",
            "<< /Name /FlateDecode >>",
            xor(&plain),
            false,
            "kein Filtername",
        ),
        (
            "kette_mit_null",
            "[/ASCII85Decode null]",
            a85(plain.as_bytes()),
            true,
            "kein Filtername",
        ),
    ];
    for (name, filter, payload, fund, wortlaut) in faelle {
        let mut objects = skeleton(true, FONT);
        objects[0].1 = b"<< /Type /Catalog /Pages 2 0 R /ZoWert 6 0 R >>".to_vec();
        objects.push((4, stream_object("", HARMLOS_CONTENT)));
        objects.push((5, HARMLOS_ANNOT.to_vec()));
        objects.push((6, stream_object(&format!("/Filter {filter}"), &payload)));
        let (pdf, _) = assemble(&objects);
        let datei = format!("{name}.pdf");
        std::fs::write(dir.join(&datei), pdf).unwrap();
        let out = run_in(&dir, &[&datei, "--check-leaks", SECRET]);
        let text = stdout(&out);
        assert_eq!(code(&out), 3, "{name} ({filter}): {text}\n{}", stderr(&out));
        assert_eq!(
            text.contains("  GEFUNDEN ("),
            fund,
            "{name} ({filter}): Fund erwartet = {fund}:\n{text}"
        );
        assert!(
            text.contains("  NICHT GEPRÜFT: ") && text.contains(wortlaut),
            "{name} ({filter}): „{wortlaut}“ fehlt:\n{text}"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Befunde
// ---------------------------------------------------------------------------

/// **ABSICHTLICH ROT — BEFUND ZO-D2/ZO-D3 am Binary.** Die Vorgängerfassung
/// eines Objekts (inkrementelles Update, `/Prev`) trägt das Geheimnis unter
/// `/ASCII85Decode` bzw. als oktal maskierte Zeichenkette. `--check-leaks`
/// sagt „nicht gefunden“ mit Rückgabewert 0 — ohne `NICHT GEPRÜFT`-Zeile.
///
/// Gegenprobe im selben Test: dieselbe Vorgängerfassung roh bzw. als
/// unmaskierte Zeichenkette wird gefunden (Rückgabewert 3) — die Rohsichten
/// lesen die Altgeneration also, nur nicht in diesen Kodierungen.
#[test]
#[ignore = "offen: Register #80 Altgeneration im Orakel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn zo_d11_altgeneration_am_binary_stilles_leck() {
    let dir = workdir("altgeneration");
    let content = format!("BT /F1 10 Tf 72 700 Td (IBAN {SECRET}) Tj ET").into_bytes();
    let octal: String = SECRET.bytes().map(|b| format!("\\{b:03o}")).collect();

    // Gegenproben: roh und unmaskiert — gefunden.
    for (name, pdf) in [
        (
            "kontrolle_roh",
            with_old_generation(stream_object("", &content), HARMLOS_ANNOT.to_vec()),
        ),
        (
            "kontrolle_literal",
            with_old_generation(
                stream_object("", HARMLOS_CONTENT),
                format!(
                    "<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents (IBAN {SECRET}) >>"
                )
                .into_bytes(),
            ),
        ),
    ] {
        let datei = format!("{name}.pdf");
        std::fs::write(dir.join(&datei), pdf).unwrap();
        let out = run_in(&dir, &[&datei, "--check-leaks", SECRET]);
        assert_eq!(code(&out), 3, "{name}: {}\n{}", stdout(&out), stderr(&out));
        assert!(
            stdout(&out).contains("  GEFUNDEN ("),
            "{name}: {}",
            stdout(&out)
        );
    }

    // Die Befunde.
    let mut still = Vec::new();
    for (name, pdf) in [
        (
            "alt_ascii85",
            with_old_generation(
                stream_object("/Filter /ASCII85Decode", &a85(&content)),
                HARMLOS_ANNOT.to_vec(),
            ),
        ),
        (
            "alt_oktal",
            with_old_generation(
                stream_object("", HARMLOS_CONTENT),
                format!("<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents ({octal}) >>")
                    .into_bytes(),
            ),
        ),
    ] {
        let datei = format!("{name}.pdf");
        std::fs::write(dir.join(&datei), pdf).unwrap();
        let out = run_in(&dir, &[&datei, "--check-leaks", SECRET]);
        let text = stdout(&out);
        if code(&out) == 0 && !found_or_unchecked(&text) {
            still.push(format!(
                "{name}: rc 0, {}",
                text.lines().nth(1).unwrap_or("")
            ));
        }
    }
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        still.is_empty(),
        "stilles Leck des Orakels an einer Altgeneration: {still:#?}"
    );
}

/// **ABSICHTLICH ROT — BEFUND ZO-D4 an der Ausgabedatei.** Ein Property-
/// Dictionary im Seiteninhalt trägt `/ActualText (Betrag 5 \240)` —
/// PDFDocEncoding, 0xA0 ist `€`. Der Schwärzungslauf (ohne Treffer) schreibt
/// den Strom unverändert in die Ausgabedatei; `--check-leaks "Betrag 5 €"`
/// an der **Ausgabedatei** sagt „nicht gefunden“, Rückgabewert 0.
///
/// Der Schwärzer selbst ist hier ehrlich: er meldet den Spiegel als
/// `NICHT GEPRÜFT` (Rückgabewert 3), weil er ihn anders liest als die Glyphen.
/// Das Orakel, das die Ausgabe nachprüfen soll, schweigt.
#[test]
fn zo_d12_pdfdoc_in_der_ausgabedatei_stilles_leck() {
    let dir = workdir("pdfdoc");
    let content = b"BT /F1 10 Tf 72 700 Td /Span << /ActualText (Betrag 5 \xA0) >> BDC \
                    (Betrag 5 EUR) Tj EMC ET";
    let mut objects = skeleton(false, FONT);
    objects.push((4, stream_object("", content)));
    let (pdf, _) = assemble(&objects);
    std::fs::write(dir.join("eingabe.pdf"), pdf).unwrap();

    let lauf = run_in(&dir, &["eingabe.pdf", "-o", "ausgabe.pdf"]);
    assert!(
        dir.join("ausgabe.pdf").exists(),
        "Vorlauf schreibt keine Ausgabe (rc {}): {}\n{}",
        code(&lauf),
        stdout(&lauf),
        stderr(&lauf)
    );
    let ausgabe = std::fs::read(dir.join("ausgabe.pdf")).unwrap();
    // Vorbedingung: die PDFDoc-Zeichenkette steht in der Ausgabedatei —
    // unkomprimiert oder in einem Flate-Strom; das Orakel selbst wäre der
    // falsche Zeuge, deshalb ein eigener Blick in die Bytes.
    let steht_drin = enthaelt(&ausgabe, b"Betrag 5 \xA0")
        || raw_streams(&ausgabe)
            .iter()
            .any(|s| inflate(s).is_some_and(|d| enthaelt(&d, b"Betrag 5 \xA0")));
    assert!(
        steht_drin,
        "Vorbedingung: /ActualText überlebt den Lauf nicht — dann prüft der Test nichts"
    );

    let out = run_in(&dir, &["ausgabe.pdf", "--check-leaks", "Betrag 5 €"]);
    let text = stdout(&out);
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        code(&out) != 0 || found_or_unchecked(&text),
        "stilles Leck des Orakels an der Ausgabedatei: PDFDocEncoding-Zeichen 0xA0 (€) — \
         rc {}, {}",
        code(&out),
        text.lines().nth(1).unwrap_or("")
    );
}

fn enthaelt(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn raw_streams(bytes: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(rel) = bytes[i..].windows(7).position(|w| w == b"stream\n") {
        let start = i + rel + 7;
        let Some(len) = bytes[start..].windows(9).position(|w| w == b"endstream") else {
            break;
        };
        out.push(&bytes[start..start + len]);
        i = start + len + 9;
    }
    out
}

/// zlib über `lopdf` — die Bibliothek liegt als Dev-Abhängigkeit ohnehin da.
fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut dict = lopdf::Dictionary::new();
    dict.set("Filter", lopdf::Object::Name(b"FlateDecode".to_vec()));
    lopdf::Stream::new(dict, data.to_vec())
        .decompressed_content()
        .ok()
}

/// Kontrolle: eine verschlüsselte Datei wird mit Meldung abgelehnt — kein
/// stilles „nicht gefunden“. `--password` ist mit `--check-leaks` nicht
/// kombinierbar (clap-Konflikt, Rückgabewert 2): wer eine verschlüsselte
/// Datei nachprüfen will, muss sie vorher entschlüsseln.
#[test]
fn zo_d13_verschluesselt_wird_ehrlich_abgelehnt() {
    let dir = workdir("verschluesselt");
    std::fs::write(dir.join("v.pdf"), redact_pipeline::testing::ENCRYPTED_PDF).unwrap();
    let out = run_in(
        &dir,
        &[
            "v.pdf",
            "--check-leaks",
            redact_pipeline::testing::ENCRYPTED_PDF_IBAN,
        ],
    );
    assert_eq!(code(&out), 1, "{}\n{}", stdout(&out), stderr(&out));
    assert!(stderr(&out).contains("verschlüsselt"), "{}", stderr(&out));
    let out = run_in(
        &dir,
        &[
            "v.pdf",
            "--password",
            redact_pipeline::testing::ENCRYPTED_PDF_PASSWORD,
            "--check-leaks",
            redact_pipeline::testing::ENCRYPTED_PDF_IBAN,
        ],
    );
    assert_eq!(code(&out), 2, "{}\n{}", stdout(&out), stderr(&out));
    std::fs::remove_dir_all(&dir).ok();
}

/// **ABSICHTLICH ROT — BEFUND ZO-D5 (Ablehnung; Einordnung als „gewöhnlich“
/// offen).** Ein Bild-XObject unter `/ASCII85Decode` allein, 2000×2300 RGB
/// (13,8 MB roh, 17,25 MB kodiert): die Vorprüfung lehnt die **ganze Datei**
/// ab (`MAX_LEGACY_STREAM_BYTES`, 16 MiB), Rückgabewert 1, keine Ausgabe —
/// obwohl ASCII85 beim Dekodieren **schrumpft** (4/5) und der Speicherbedarf
/// damit vorab bekannt ist. `--check-leaks` lehnt dieselbe Datei ebenso ab;
/// `--max-decompressed-mb` ändert nichts, die Grenze ist eine Konstante.
///
/// Ob ein solches Bild „gewöhnlich“ ist, entscheidet die Runde: Distiller mit
/// ASCII-Ausgabe schreibt `[/ASCII85Decode /FlateDecode]` bzw.
/// `[/ASCII85Decode /DCTDecode]`; die zweite Form ist nicht betroffen, die
/// erste ab rund 13 MiB gepacktem Bild.
#[test]
#[ignore = "offen: Register #82 ASCII85-Altlastgrenze — Spur-A-Runde 1, Beleg absichtlich rot"]
fn zo_d14_altlast_filter_ueber_16_mib_wird_abgelehnt() {
    let dir = workdir("altlast");
    // Unkomprimierbare Bilddaten ohne Abhängigkeit: xorshift.
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let mut raw = Vec::with_capacity(2000 * 2300 * 3);
    while raw.len() < 2000 * 2300 * 3 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        raw.extend_from_slice(&x.to_le_bytes());
    }
    raw.truncate(2000 * 2300 * 3);
    let img = stream_object(
        "/Type /XObject /Subtype /Image /Width 2000 /Height 2300 /ColorSpace /DeviceRGB \
         /BitsPerComponent 8 /Filter /ASCII85Decode",
        &a85(&raw),
    );
    let content =
        format!("q 595 0 0 842 0 0 cm /Im1 Do Q BT /F1 10 Tf 72 700 Td (IBAN {SECRET}) Tj ET");
    let mut objects = skeleton(false, &format!("/XObject << /Im1 6 0 R >> {FONT}"));
    objects.push((4, stream_object("", content.as_bytes())));
    objects.push((6, img));
    let (pdf, _) = assemble(&objects);
    std::fs::write(dir.join("bild.pdf"), pdf).unwrap();

    let out = run_in(&dir, &["bild.pdf", "-o", "aus.pdf"]);
    let ausgabe = dir.join("aus.pdf").exists();
    let text = format!("{}\n{}", stdout(&out), stderr(&out));
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        ausgabe && code(&out) != 1,
        "die Vorprüfung lehnt eine Datei mit einem 17-MB-ASCII85-Bild ab (rc {}): {text}",
        code(&out)
    );
}
