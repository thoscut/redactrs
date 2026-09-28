//! Spur A, Runde 2, Prüfer D-8 (Register #100): die Rohsicht arbeitete je
//! Block, nicht je Byte.
//!
//! Jeder rohe `stream`-Block suchte seinen Objektkopf und sein Dictionary
//! bis zu 64 KiB rückwärts, zweimal, und baute für den blinden
//! Entpackversuch zwei neue Dekoder. Eine Datei aus lauter `stream` /
//! `endstream` kostete so je Block das ganze Fenster: der Lauf wuchs mit
//! Blöcken mal Fenster statt mit der Datei. Dieselbe Klasse stand im
//! Vergleich mit dem Lader (Register #98): ob zwischen Querverweis und Block
//! ein `endobj` steht, suchte er je Objekt im ganzen Bereich.
//!
//! Die Rückwärtssuche endet jetzt am Ende des vorigen Blocks — dort beginnt
//! frühestens der Kopf des nächsten —, die Dekoder werden einmal gebaut und
//! je Block zurückgesetzt, und die `endobj`-Stellen stehen einmal in einem
//! Verzeichnis. Belegt wird, was sich daran beobachten lässt: ein Block
//! ohne eigenen Kopf heißt nicht mehr wie das Objekt davor, zwei rohe
//! zlib-Blöcke hintereinander werden beide entpackt, und ein verwaistes
//! Objekt derselben Nummer bleibt still.

use std::io::Write;

use redact_pdf::leaks_many_within;

const BUDGET: u64 = 64 * 1024 * 1024;

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("packbar");
    e.finish().expect("packbar")
}

/// Der zweite Block hat keinen eigenen Kopf; der erste gehört Objekt 1. Bis
/// Register #100 fand die Rückwärtssuche des zweiten Blocks den Kopf des
/// ersten und nannte den Fund „Objekt 1 0“.
#[test]
fn ein_block_ohne_kopf_heisst_nicht_wie_das_objekt_davor() {
    let mut pdf = b"%PDF-1.7\n1 0 obj\n<< /Length 5 >>\nstream\nHallo\nendstream\n".to_vec();
    pdf.extend_from_slice(b"stream\nGEHEIMNIS\nendstream\nendobj\n");
    let check = leaks_many_within(&pdf, &["GEHEIMNIS"], BUDGET);
    let stroeme: Vec<&String> = check.findings[0]
        .iter()
        .filter(|f| f.starts_with("Rohdaten-Stream"))
        .collect();
    assert!(!stroeme.is_empty(), "{:?}", check.findings[0]);
    assert!(
        stroeme.iter().all(|f| !f.contains("Objekt 1 0")),
        "der Block ohne Kopf trägt den Namen des Objekts davor: {stroeme:?}"
    );
}

/// Und das Dictionary davor erbt er auch nicht: der erste Block steht hinter
/// `/FooDecode` und gehört in `unchecked`, der zweite nicht — bis Register
/// #100 las die Rückwärtssuche das Dictionary des ersten Blocks auch für den
/// zweiten.
#[test]
fn ein_block_ohne_kopf_erbt_nicht_das_dictionary_davor() {
    let mut pdf =
        b"%PDF-1.7\n1 0 obj\n<< /Filter /FooDecode /Length 5 >>\nstream\nHallo\nendstream\n"
            .to_vec();
    pdf.extend_from_slice(b"stream\nWelt\nendstream\nendobj\n");
    let check = leaks_many_within(&pdf, &["GEHEIMNIS"], BUDGET);
    let zeilen = check
        .unchecked
        .iter()
        .filter(|z| z.contains("/FooDecode"))
        .count();
    assert_eq!(zeilen, 1, "{:?}", check.unchecked);
}

/// Zwei rohe zlib-Blöcke ohne `/Filter` hintereinander: der blinde Versuch
/// entpackt beide — mit demselben, zurückgesetzten Dekoder.
#[test]
fn zwei_rohe_zlib_bloecke_werden_beide_entpackt() {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    for (id, geheim) in [(1, "ERSTGEHEIM"), (2, "ZWEITGEHEIM")] {
        let mut text = "Fuelltext Fuelltext Fuelltext\n".repeat(32);
        text.push_str(geheim);
        let daten = zlib(text.as_bytes());
        pdf.extend_from_slice(
            format!("{id} 0 obj\n<< /Length {} >>\nstream\n", daten.len()).as_bytes(),
        );
        pdf.extend_from_slice(&daten);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
    }
    assert!(
        !pdf.windows(10).any(|w| w == b"ERSTGEHEIM"),
        "der Prüfling trägt Klartext"
    );
    let check = leaks_many_within(&pdf, &["ERSTGEHEIM", "ZWEITGEHEIM"], BUDGET);
    for (i, name) in ["ERSTGEHEIM", "ZWEITGEHEIM"].iter().enumerate() {
        assert!(
            check.findings[i].iter().any(|f| f.contains("(inflate)")),
            "{name} nicht entpackt: {:?}",
            check.findings[i]
        );
    }
}

/// Ein verwaistes Objekt derselben Nummer hinter dem aktuellen — kein
/// Querverweis nennt es: zwischen dem aktuellen Objekt und dem Block steht
/// ein `endobj`, der Block ist nicht der des aktuellen Objekts, und nichts
/// ist verlesen.
#[test]
fn ein_verwaistes_objekt_derselben_nummer_bleibt_still() {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (id, body) in [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>".to_vec(),
        ),
        (6, b"<< /Aktuell true >>".to_vec()),
    ] {
        offsets.push((id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(&body);
        out.extend_from_slice(b"\nendobj\n");
    }
    // Verwaist: dieselbe Nummer, ein Strom, in keinem Querverweis.
    out.extend_from_slice(b"6 0 obj\n<< /Length 5 >>\nstream\nHallo\nendstream\nendobj\n");
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 1\n0000000000 65535 f \n");
    for (id, offset) in &offsets {
        out.extend_from_slice(format!("{id} 1\n{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    let check = leaks_many_within(&out, &["GEHEIMNIS"], BUDGET);
    assert!(check.unchecked.is_empty(), "{:?}", check.unchecked);
}
