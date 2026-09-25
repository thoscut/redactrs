//! Gegenprüfung Q5 der Fix-Runde 5: **was `SECURITY.md` über unbekannte
//! Filter zusichert — und wo die Zusicherung nicht gilt.**
//!
//! Die Fix-Runde 5 hat dem Leck-Orakel beigebracht, eine Filterkette so weit
//! zu dekodieren, wie es kommt, und den Filter zu nennen, an dem es stehen
//! blieb. `SECURITY.md` schreibt das im Abschnitt „Was ein sauberer Lauf nicht
//! ausschließt“ als Zusicherung auf:
//!
//! > Text hinter einem Bildfilter (`/DCTDecode`, …), allein oder am Ende einer
//! > Filterkette: ein benannter blinder Fleck und **keine**
//! > `NICHT GEPRÜFT`-Zeile — sonst käme jede Datei mit einem Foto als
//! > unvollständig geprüft zurück. **Ein Filtername, den das Programm gar
//! > nicht kennt, steht sehr wohl darin.**
//!
//! Der letzte Satz gilt nur, wenn **vorher schon ein Filter angewendet wurde**.
//! `audit_bytes::decode_stream` steigt aus, bevor es etwas zu melden gibt:
//!
//! ```text
//! let (data, applied) = filters::decoded_prefix_within(doc, stream, room)?;
//! if applied == 0 {
//!     return Ok(None);          // <- kein `unchecked`-Eintrag, kein Grund
//! }
//! ```
//!
//! Am gebauten Binary gemessen (Gegenprüfung Q5, Release 0.6.0), derselbe
//! verschleierte Seiteninhalt (XOR 0x5A, also weder roh noch als Zeichenkette
//! lesbar) unter zwei Filterangaben:
//!
//! ```text
//! $ redact-rs filt_unknown_opaque.pdf --check-leaks GEHEIM   # /Filter /FooDecode
//!   nicht gefunden: GEHEIM
//! Ergebnis: der Suchbegriff steht nicht mehr in der Datei.       -> 0
//!
//! $ redact-rs filt_flate_unknown_opaque.pdf --check-leaks GEHEIM
//!                                       # /Filter [/FlateDecode /FooDecode]
//!   nicht gefunden: GEHEIM
//!   NICHT GEPRÜFT: Objekt 4 0 <Stream>: nur bis Filter 1 von 2 dekodiert —
//!   /FooDecode ist hier kein bekannter Filter; …                 -> 3
//! ```
//!
//! Der erste Lauf ist die stille Entwarnung, die die Runde 5 abstellen wollte:
//! keine Sicht hat den Strom gelesen, und der Lauf sagt es nicht. Der
//! Schwärzungsweg ist an derselben Datei streng (Rückgabewert 1, „Ein Teil des
//! Seiteninhalts ließ sich nicht in Operationen zerlegen“) — nur die
//! Nachprüfung winkt durch.
//!
//! **Fix-Runde 6: der Befund ist geschlossen.** `decode_stream` meldet den
//! unbekannten Filternamen jetzt **an jeder Stelle der Kette**, auch als
//! erstem Glied (`gar nicht dekodiert — /FooDecode ist hier kein bekannter
//! Filter (Glied 1 von N)`); `#[ignore]` ist bei
//! [`befund_q5_unbekannter_filter_allein_bleibt_stumm`] entfernt. Dazu
//! [`die_zusage_ueber_unbekannte_filter_gilt_an_jeder_stelle_der_kette`]: es
//! fährt alle fünf Ketten, über die `SECURITY.md` etwas zusagt — unbekannter
//! Filter allein, an erster und an letzter Stelle, Bildfilter allein und am
//! Kettenende — und hält das Ergebnis gegen den Wortlaut der Zusage.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "redact-zf-q5-{}-{name}-{:?}",
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

/// Eine PDF-Datei aus fertigen Objektrümpfen, mit Querverweistabelle.
fn assemble(objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
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
    out
}

/// Der Klartext, verschleiert (XOR 0x5A): so findet ihn **keine** Rohbyte- und
/// keine Zeichenkettensicht — nur wer den Strom auspackt, sieht ihn. Genau
/// darum geht es hier.
fn verschleiert(text: &str) -> Vec<u8> {
    text.bytes().map(|b| b ^ 0x5A).collect()
}

/// Eine Datei mit einem Seiteninhalt unter `filter`; der Strominhalt ist
/// `daten`.
fn pdf_mit_stromfilter(filter: &str, daten: &[u8]) -> Vec<u8> {
    let mut stream =
        format!("<< /Length {} /Filter {filter} >>\nstream\n", daten.len()).into_bytes();
    stream.extend_from_slice(daten);
    stream.extend_from_slice(b"\nendstream");
    assemble(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
              /Resources << >> >>"
                .to_vec(),
        ),
        (4, stream),
    ])
}

/// Flate-packen, ohne eine Abhängigkeit dafür: `flate2` liegt über `lopdf`
/// ohnehin im Baum — hier genügt aber ein **unkomprimierter** Deflate-Block
/// (`BTYPE = 00`) in einem zlib-Rahmen. Das ist gültiges `FlateDecode` und
/// braucht keine Bibliothek.
fn flate_stored(daten: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01]; // zlib-Kopf, „stored“-freundlich
    for (i, block) in daten.chunks(65_535).enumerate() {
        let letzter = (i + 1) * 65_535 >= daten.len();
        out.push(u8::from(letzter));
        let len = u16::try_from(block.len()).expect("Block ≤ 65 535");
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    // Adler-32 über die unkomprimierten Daten.
    let (mut a, mut b) = (1u32, 0u32);
    for byte in daten {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

/// **Die Hälfte, die trägt.** Ein unbekannter Filter **hinter** einem
/// bekannten wird beim Namen genannt, und der Lauf endet mit 3 — auch ohne
/// Fund.
///
/// Mutationsnachweis (Gegenprüfung Q5, gefahren): in
/// `crates/redact-pdf/src/audit_bytes.rs` in `decode_stream` das
/// `(!filters::is_image_filter(&names[applied])).then_some(rest)` auf `None`
/// gesetzt → dieser Test ist rot (kein `NICHT GEPRÜFT`, Rückgabewert 0).
#[test]
fn unbekannter_filter_in_der_kette_wird_benannt() {
    let dir = workdir("kette");
    let geheim = "GEHEIM Max Mustermann";
    let daten = flate_stored(&verschleiert(geheim));
    std::fs::write(
        dir.join("kette.pdf"),
        pdf_mit_stromfilter("[/FlateDecode /FooDecode]", &daten),
    )
    .unwrap();

    let out = run_in(&dir, &["kette.pdf", "--check-leaks", "GEHEIM"]);
    let text = stdout(&out);
    assert!(
        text.contains("  NICHT GEPRÜFT: "),
        "der unbekannte Filter wird nicht benannt:\n{text}"
    );
    assert!(
        text.contains("/FooDecode ist hier kein bekannter Filter"),
        "die Stelle nennt ihren Grund nicht:\n{text}"
    );
    assert_eq!(
        out.status.code(),
        Some(3),
        "eine unlesbare Stelle und trotzdem kein „sieh hin“:\n{text}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// **Befund Q5 — offen.** Ist der **erste** Filter schon unbekannt, sagt der
/// Lauf nichts: „nicht gefunden“, Rückgabewert 0, keine `NICHT GEPRÜFT`-Zeile.
/// `SECURITY.md` sagt an dieser Stelle das Gegenteil („Ein Filtername, den das
/// Programm gar nicht kennt, steht sehr wohl darin“).
///
/// **Geschlossen in der Fix-Runde 6** (Agent A, `audit_bytes::decode_stream`):
/// der Grund steht jetzt auch bei `applied == 0`, eine dekodierte Sicht gibt
/// es weiterhin nicht (die Rohbytes hat die Rohsicht gelesen, eine zweite
/// gleichlautende Meldung brächte nichts). Der Test ist deshalb **scharf**.
///
/// Mutationsnachweis (gefahren): in `decode_stream` den Zweig
/// `Some(rest) if … && applied == 0` gestrichen → dieser Test ist rot
/// („stille Entwarnung“, Rückgabewert 0).
#[test]
fn befund_q5_unbekannter_filter_allein_bleibt_stumm() {
    let dir = workdir("allein");
    let geheim = "GEHEIM Max Mustermann";
    std::fs::write(
        dir.join("allein.pdf"),
        pdf_mit_stromfilter("/FooDecode", &verschleiert(geheim)),
    )
    .unwrap();

    let out = run_in(&dir, &["allein.pdf", "--check-leaks", "GEHEIM"]);
    let text = stdout(&out);
    assert!(
        text.contains("  NICHT GEPRÜFT: "),
        "stille Entwarnung: keine Sicht hat den Strom gelesen, und der Lauf \
         sagt es nicht:\n{text}"
    );
    assert_eq!(
        out.status.code(),
        Some(3),
        "Rückgabewert 0 über einen Strom, den niemand aufgemacht hat:\n{text}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// **E5 — die Zusage von `SECURITY.md`, an fünf Ketten gemessen.**
///
/// `SECURITY.md` sagt zu: Text hinter einem **Bildfilter** ist ein benannter
/// blinder Fleck und **keine** `NICHT GEPRÜFT`-Zeile (sonst käme jede Datei
/// mit einem Foto als unvollständig geprüft zurück) — ein Filtername, den das
/// Programm **gar nicht kennt**, steht sehr wohl darin, an jeder Stelle der
/// Kette.
///
/// Bis zur Fix-Runde 6 galt die zweite Hälfte nur, wenn vorher schon ein
/// Filter gelaufen war. Dieser Test fährt alle fünf Fälle, über die die
/// Zusage etwas sagt, und verlangt dazu, dass der Satz in `SECURITY.md`
/// wörtlich dasteht — eine Zusage ohne Lauf ist eine Behauptung, ein Lauf
/// ohne Zusage ist ein Zufall.
#[test]
fn die_zusage_ueber_unbekannte_filter_gilt_an_jeder_stelle_der_kette() {
    let dir = workdir("zusage");
    let geheim = "GEHEIM Max Mustermann";
    let roh = verschleiert(geheim);
    let gepackt = flate_stored(&roh);

    // (Name, Filterangabe, Strominhalt, muss benannt werden?)
    let faelle: [(&str, &str, &[u8], bool); 5] = [
        ("foo_allein", "/FooDecode", &roh, true),
        ("foo_zuerst", "[/FooDecode /FlateDecode]", &roh, true),
        ("foo_zuletzt", "[/FlateDecode /FooDecode]", &gepackt, true),
        ("bild_allein", "/DCTDecode", &roh, false),
        ("bild_zuletzt", "[/FlateDecode /DCTDecode]", &gepackt, false),
    ];

    for (name, filter, daten, benannt) in faelle {
        let datei = format!("{name}.pdf");
        std::fs::write(dir.join(&datei), pdf_mit_stromfilter(filter, daten)).unwrap();
        let out = run_in(&dir, &[&datei, "--check-leaks", "GEHEIM"]);
        let text = stdout(&out);
        assert_eq!(
            text.contains("  NICHT GEPRÜFT: "),
            benannt,
            "{name} ({filter}): die Stelle ist {}benannt:\n{text}",
            if benannt { "nicht " } else { "zu Unrecht " }
        );
        assert_eq!(
            out.status.code(),
            Some(if benannt { 3 } else { 0 }),
            "{name} ({filter}): falscher Rückgabewert:\n{text}"
        );
        if benannt {
            assert!(
                text.contains("/FooDecode ist hier kein bekannter Filter"),
                "{name}: der unbekannte Filtername fehlt in der Meldung:\n{text}"
            );
        }
    }

    // Und die Zusage steht so in `SECURITY.md`.
    let wurzel = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let security = std::fs::read_to_string(wurzel.join("SECURITY.md"))
        .expect("SECURITY.md lesbar")
        .replace("\r\n", "\n");
    let security = security.split_whitespace().collect::<Vec<_>>().join(" ");
    for satz in [
        "ein benannter blinder Fleck und **keine** `NICHT GEPRÜFT`-Zeile",
        "Ein Filtername, den das Programm gar nicht kennt, steht sehr wohl darin — \
         **an jeder Stelle der Kette**, auch als erstes Glied.",
    ] {
        assert!(
            security.contains(satz),
            "SECURITY.md sagt „{satz}“ nicht mehr zu"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}
