//! Spur A, Runde 2, Prüfer D-5/D-6 (Register #98): zwei Stellen, an denen
//! das Orakel stumm nichts sah.
//!
//! * **Eine Seite, die der Interpreter ablehnt**, fehlte in der Sicht des
//!   Schriftdekoders ohne Meldung. Ein verirrtes `]`, `--1`, `{ }` (und bis
//!   Register #103 auch ein Seitenvorschub als Leerraum, `q\x0cQ`):
//!   `pdftotext` liest die Seite, das Orakel meldete „nicht gefunden“,
//!   Rückgabewert 0.
//! * **Ein Strom, den der Lader anders übernahm, als er in den Rohbytes
//!   steht** — eine falsche `/Length`, ein versetzter Querverweis —, fehlte in
//!   den Sichten 3–7 ohne Meldung.
//!
//! Beides steht jetzt in `unchecked`. Die Gegenproben: eine gewöhnliche
//! Datei, ein inkrementelles Update, das einen Strom ersetzt oder freigibt,
//! und `stream` mit Leerraum vor dem Zeilenende bleiben still; ein
//! Seiteninhalt hinter einem Bild- oder unbekannten Filter bekommt keine
//! zweite Zeile — außer ein lesbarer Strom steht daneben.

use std::io::Write;

use redact_pdf::leaks_many_within;

const GEHEIM: &str = "GEHEIMNIS";
const BUDGET: u64 = 64 * 1024 * 1024;

/// Schrift mit eigener Kodierung: die Codes 33 bis 39 sind G, E, H, I, M,
/// N, S — das Geheimnis steht in keinem Byte als Klartext.
const SCHRIFT: &[u8] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding \
    << /Type /Encoding /BaseEncoding /WinAnsiEncoding /Differences [33 /G /E /H /I /M /N /S] >> >>";

fn text() -> Vec<u8> {
    let mut t = b"BT /F1 12 Tf 72 700 Td (".to_vec();
    t.extend_from_slice(&[33, 34, 35, 34, 36, 37, 38, 36, 39]);
    t.extend_from_slice(b") Tj ET");
    t
}

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("packbar");
    e.finish().expect("packbar")
}

fn strom(kopf: &str, daten: &[u8], nach_stream: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {kopf} >>\nstream").into_bytes();
    out.extend_from_slice(nach_stream);
    out.extend_from_slice(daten);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Schreibt Objekte ab `start` in `out` und liefert ihre Offsets.
fn objekte(out: &mut Vec<u8>, objs: &[(u32, Vec<u8>)]) -> Vec<(u32, usize)> {
    let mut offsets = Vec::new();
    for (id, body) in objs {
        offsets.push((*id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    offsets
}

/// Eine Querverweistabelle über `eintraege` (Nummer, Offset oder `None` für
/// frei) und den Anhänger dazu.
fn querverweis(
    out: &mut Vec<u8>,
    eintraege: &[(u32, Option<usize>)],
    size: u32,
    prev: Option<usize>,
) {
    let xref = out.len();
    out.extend_from_slice(b"xref\n");
    if prev.is_none() {
        out.extend_from_slice(b"0 1\n0000000000 65535 f \n");
    }
    for (id, offset) in eintraege {
        out.extend_from_slice(format!("{id} 1\n").as_bytes());
        match offset {
            Some(o) => out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 00001 f \n"),
        }
    }
    let prev = prev.map_or(String::new(), |p| format!(" /Prev {p}"));
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R{prev} >>\nstartxref\n{xref}\n%%EOF\n")
            .as_bytes(),
    );
}

/// Die Seite mit dem Inhaltsstrom `inhalt` (Objekt 4) und der Schrift
/// (Objekt 5); dazu ein Objekt 6 als zweiter Strom.
fn seite(inhalt: Vec<u8>) -> (Vec<u8>, Vec<(u32, usize)>) {
    seite_mit("4 0 R", inhalt, strom("/Length 5", b"Hallo", b"\n"))
}

/// Wie [`seite`], nur mit `/Contents` und Objekt 6 nach Wahl.
fn seite_mit(contents: &str, inhalt: Vec<u8>, sechs: Vec<u8>) -> (Vec<u8>, Vec<(u32, usize)>) {
    let mut out = b"%PDF-1.7\n".to_vec();
    let offsets = objekte(
        &mut out,
        &[
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents {contents} \
                     /Resources << /Font << /F1 5 0 R >> >> >>"
                )
                .into_bytes(),
            ),
            (4, inhalt),
            (5, SCHRIFT.to_vec()),
            (6, sechs),
        ],
    );
    (out, offsets)
}

fn fertig(mut out: Vec<u8>, offsets: &[(u32, usize)]) -> Vec<u8> {
    let eintraege: Vec<(u32, Option<usize>)> =
        offsets.iter().map(|(id, o)| (*id, Some(*o))).collect();
    querverweis(&mut out, &eintraege, 7, None);
    out
}

fn gewoehnlich() -> Vec<u8> {
    let z = zlib(&text());
    let (out, offsets) = seite(strom(
        &format!("/Filter /FlateDecode /Length {}", z.len()),
        &z,
        b"\n",
    ));
    fertig(out, &offsets)
}

fn offen(pdf: &[u8]) -> Vec<String> {
    leaks_many_within(pdf, &[GEHEIM], BUDGET).unchecked
}

#[test]
fn eine_abgelehnte_seite_steht_in_nicht_geprueft() {
    let mut inhalt = b"q ] Q ".to_vec();
    inhalt.extend_from_slice(&text());
    let z = zlib(&inhalt);
    let (out, offsets) = seite(strom(
        &format!("/Filter /FlateDecode /Length {}", z.len()),
        &z,
        b"\n",
    ));
    let pdf = fertig(out, &offsets);
    let offen = offen(&pdf);
    assert!(
        offen
            .iter()
            .any(|z| z.contains("Sicht 7") && z.contains("Seite 1")),
        "die abgelehnte Seite fehlt stumm: {offen:?}"
    );
}

/// Ein Seiteninhalt hinter einem Bildfilter: dort konnte diese Sicht nie
/// lesen, und `SECURITY.md` sagt dafür „keine Meldung“ zu. Hinter einem
/// unbekannten Filter meldet die Objektsicht — eine Zeile, keine zweite.
#[test]
fn ein_unlesbarer_seiteninhalt_gibt_keine_zeile_der_sicht_7() {
    for (filter, meldungen) in [("/DCTDecode", 0), ("/FooDecode", 1)] {
        let (out, offsets) = seite(strom(
            &format!("/Filter {filter} /Length 5"),
            b"q ] Q",
            b"\n",
        ));
        let pdf = fertig(out, &offsets);
        let offen = offen(&pdf);
        assert!(
            !offen.iter().any(|z| z.contains("Sicht 7")),
            "{filter}: {offen:?}"
        );
        assert_eq!(offen.len(), meldungen, "{filter}: {offen:?}");
    }
}

/// Ein lesbarer Strom neben einem unlesbaren: der Interpreter lehnt die
/// Seite als Ganzes ab, und das Geheimnis im lesbaren Strom hat diese Sicht
/// nicht gesehen — die Seite behält ihre Zeile.
#[test]
fn ein_lesbarer_strom_neben_einem_unlesbaren_behaelt_die_zeile() {
    let z = zlib(&text());
    let (out, offsets) = seite_mit(
        "[4 0 R 6 0 R]",
        strom(
            &format!("/Filter /FlateDecode /Length {}", z.len()),
            &z,
            b"\n",
        ),
        strom("/Filter /DCTDecode /Length 5", b"q ] Q", b"\n"),
    );
    let pdf = fertig(out, &offsets);
    let offen = offen(&pdf);
    assert!(
        offen
            .iter()
            .any(|z| z.contains("Sicht 7") && z.contains("Seite 1")),
        "die Seite mit dem lesbaren Strom fehlt stumm: {offen:?}"
    );
}

#[test]
fn ein_verlesener_strom_steht_in_nicht_geprueft() {
    let z = zlib(&text());
    let (out, offsets) = seite(strom(
        &format!("/Filter /FlateDecode /Length {}", z.len() - 10),
        &z,
        b"\n",
    ));
    let pdf = fertig(out, &offsets);
    let offen = offen(&pdf);
    assert!(
        offen.iter().any(|z| z.starts_with("Objekt 4 0:")),
        "der verlesene Strom fehlt stumm: {offen:?}"
    );
}

#[test]
fn ein_versetzter_querverweis_steht_in_nicht_geprueft() {
    let z = zlib(&text());
    let (out, mut offsets) = seite(strom(
        &format!("/Filter /FlateDecode /Length {}", z.len()),
        &z,
        b"\n",
    ));
    for (id, offset) in &mut offsets {
        if *id == 4 {
            *offset += 3;
        }
    }
    let pdf = fertig(out, &offsets);
    let offen = offen(&pdf);
    assert!(
        offen.iter().any(|z| z.starts_with("Objekt 4 0:")),
        "der versetzte Strom fehlt stumm: {offen:?}"
    );
}

#[test]
fn eine_gewoehnliche_datei_bleibt_still_und_der_fund_steht() {
    let check = leaks_many_within(&gewoehnlich(), &[GEHEIM], BUDGET);
    assert!(check.unchecked.is_empty(), "{:?}", check.unchecked);
    assert!(
        !check.findings[0].is_empty(),
        "der Schriftdekoder fand nichts"
    );
}

/// Hängt an `pdf` ein inkrementelles Update mit `objs` und den freien
/// Nummern `frei`.
fn update(mut pdf: Vec<u8>, objs: &[(u32, Vec<u8>)], frei: &[u32]) -> Vec<u8> {
    let prev = {
        let s = String::from_utf8_lossy(&pdf).into_owned();
        let i = s.rfind("startxref\n").expect("startxref") + "startxref\n".len();
        s[i..]
            .lines()
            .next()
            .expect("Zahl")
            .trim()
            .parse::<usize>()
            .expect("Zahl")
    };
    let offsets = objekte(&mut pdf, objs);
    let mut eintraege: Vec<(u32, Option<usize>)> =
        offsets.iter().map(|(id, o)| (*id, Some(*o))).collect();
    eintraege.extend(frei.iter().map(|id| (*id, None)));
    querverweis(&mut pdf, &eintraege, 7, Some(prev));
    pdf
}

/// Ein inkrementelles Update ersetzt den Strom 4 durch einen neuen und den
/// Strom 6 durch ein Dictionary: die alten Ströme stehen weiter in den
/// Rohbytes, verlesen ist keiner.
#[test]
fn altrevisionen_bleiben_still() {
    let z = zlib(b"BT /F1 12 Tf 72 700 Td (neu) Tj ET");
    let pdf = update(
        gewoehnlich(),
        &[
            (
                4,
                strom(
                    &format!("/Filter /FlateDecode /Length {}", z.len()),
                    &z,
                    b"\n",
                ),
            ),
            (6, b"<< /Ersatz true >>".to_vec()),
        ],
        &[],
    );
    let offen = offen(&pdf);
    assert!(offen.is_empty(), "{offen:?}");
}

/// Ein inkrementelles Update gibt den Strom 6 frei.
#[test]
fn ein_freigegebener_strom_bleibt_still() {
    let pdf = update(gewoehnlich(), &[], &[6]);
    let offen = offen(&pdf);
    assert!(offen.is_empty(), "{offen:?}");
}

/// `stream` mit Leerraum vor dem Zeilenende (`stream \r\n`).
#[test]
fn leerraum_hinter_stream_bleibt_still() {
    let z = zlib(&text());
    let (out, offsets) = seite(strom(
        &format!("/Filter /FlateDecode /Length {}", z.len()),
        &z,
        b" \r\n",
    ));
    let pdf = fertig(out, &offsets);
    let offen = offen(&pdf);
    assert!(offen.is_empty(), "{offen:?}");
}

/// Objekt-Strom und Querverweis-Strom (PDF 1.5): den Objekt-Strom legt der
/// Lader entpackt ab — verlesen ist er deshalb nicht.
#[test]
fn objekt_und_querverweisstrom_bleiben_still() {
    let innen: [(u32, &[u8]); 3] = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R \
              /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
    ];
    let mut kopf = String::new();
    let mut rumpf = Vec::new();
    for (id, obj) in innen {
        kopf.push_str(&format!("{id} {} ", rumpf.len()));
        rumpf.extend_from_slice(obj);
        rumpf.push(b' ');
    }
    let mut objstm = kopf.clone().into_bytes();
    objstm.extend_from_slice(&rumpf);
    let objstm = zlib(&objstm);
    let inhalt = zlib(&text());

    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = objekte(
        &mut out,
        &[
            (
                4,
                strom(
                    &format!("/Filter /FlateDecode /Length {}", inhalt.len()),
                    &inhalt,
                    b"\n",
                ),
            ),
            (5, SCHRIFT.to_vec()),
            (
                6,
                strom(
                    &format!(
                        "/Type /ObjStm /N 3 /First {} /Filter /FlateDecode /Length {}",
                        kopf.len(),
                        objstm.len()
                    ),
                    &objstm,
                    b"\n",
                ),
            ),
        ],
    );
    let xref_at = out.len();
    offsets.push((7, xref_at));
    let mut zeilen = vec![[0u8, 0, 0, 0, 0, 0xFF, 0xFF]];
    for index in 0..3u16 {
        let [hi, lo] = index.to_be_bytes();
        zeilen.push([2, 0, 0, 0, 6, hi, lo]);
    }
    for (_, offset) in &offsets {
        let [a, b, c, d] = (*offset as u32).to_be_bytes();
        zeilen.push([1, a, b, c, d, 0, 0]);
    }
    let tabelle = zlib(&zeilen.concat());
    let body = strom(
        &format!(
            "/Type /XRef /Size 8 /W [1 4 2] /Root 1 0 R /Filter /FlateDecode /Length {}",
            tabelle.len()
        ),
        &tabelle,
        b"\n",
    );
    out.extend_from_slice(b"7 0 obj\n");
    out.extend_from_slice(&body);
    out.extend_from_slice(format!("\nendobj\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());

    let check = leaks_many_within(&out, &[GEHEIM], BUDGET);
    assert!(check.unchecked.is_empty(), "{:?}", check.unchecked);
    assert!(
        !check.findings[0].is_empty(),
        "der Schriftdekoder fand nichts"
    );
}
