//! Spur A, Runde 2, Prüfer D-7 (Register #99): gebucht wird die **Arbeit**
//! einer Filterkette, nicht die Ausgabe ihres letzten Glieds.
//!
//! `[/FlateDecode /FlateDecode /ASCIIHexDecode]` über einem Strom, dessen
//! zweites Flate-Glied Nullen liefert: ASCIIHex überliest Nullbytes als
//! Leerraum und gibt nichts aus. Die Vorprüfung, das Orakel und
//! `filters::decode_chain` buchten nur diese letzte Ausgabe — null Byte —,
//! und jedes Glied durfte für sich bis an die Grenze entpacken. Am Stand
//! `9bda3b6` brauchte eine 72-KB-Datei mit zweihundert solcher Ströme (je
//! 60 MiB Nullen) für `redact-rs … --max-decompressed-mb 64` 3 min 52 s mit
//! Rückgabewert 0; `--check-leaks` lief nach 400 s noch.
//!
//! Jetzt gilt die Grenze für die ganze Kette (jedes Glied bekommt, was die
//! davor übrig ließen), und Vorprüfung wie Orakel buchen die Summe.

use std::io::Write;

use lopdf::{dictionary, Document, Object, Stream};
use redact_pdf::document::{prescan, Limits};
use redact_pdf::filters::{decoded_content_within, Oversize};
use redact_pdf::leaks_many_within;

const MIB: usize = 1024 * 1024;

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("packbar");
    e.finish().expect("packbar")
}

/// Zweimal gepackte Nullen: das zweite Flate-Glied liefert `mib` MiB
/// Nullen, die ASCIIHex als Leerraum überliest.
fn zweimal_gepackt(mib: usize) -> Vec<u8> {
    zlib(&zlib(&vec![0u8; mib * MIB]))
}

/// Eine Seite und `anzahl` Ströme mit `kette` über `daten`.
fn datei(anzahl: usize, kette: &str, daten: &[u8]) -> Vec<u8> {
    let mut objekte: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>".to_vec(),
    ];
    for _ in 0..anzahl {
        let mut strom =
            format!("<< /Filter {kette} /Length {} >>\nstream\n", daten.len()).into_bytes();
        strom.extend_from_slice(daten);
        strom.extend_from_slice(b"\nendstream");
        objekte.push(strom);
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objekte.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objekte.len() + 1).as_bytes(),
    );
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objekte.len() + 1
        )
        .as_bytes(),
    );
    out
}

const SCHRUMPFEND: &str = "[/FlateDecode /FlateDecode /ASCIIHexDecode]";

#[test]
fn die_vorpruefung_bucht_die_arbeit_der_kette() {
    let pdf = datei(8, SCHRUMPFEND, &zweimal_gepackt(8));
    let limits = Limits {
        max_decompressed_bytes: (16 * MIB) as u64,
        ..Limits::default()
    };
    let fehler = prescan(&pdf, &limits).expect_err("acht mal acht MiB Arbeit gehen durch");
    assert!(
        fehler
            .to_string()
            .contains("überschreiten das Budget von 16 MB"),
        "{fehler}"
    );
}

/// Die Rohsicht des Orakels (Sicht 2) entpackt jeden Block über seine Kette
/// — und muss die Arbeit buchen: nach zwei Strömen ist das Budget leer, der
/// Rest wird als „nicht entpackt“ gemeldet.
#[test]
fn das_orakel_bucht_die_arbeit_der_kette() {
    let pdf = datei(8, SCHRUMPFEND, &zweimal_gepackt(8));
    let check = leaks_many_within(&pdf, &["GEHEIM"], (16 * MIB) as u64);
    let offen: Vec<&String> = check
        .unchecked
        .iter()
        .filter(|z| z.starts_with("Rohdaten-Stream") && z.contains("nicht entpackt"))
        .collect();
    assert!(
        !offen.is_empty(),
        "das Budget der Rohsicht griff nicht: {:?}",
        check.unchecked
    );
}

/// `decoded_content_within`: die Grenze gilt für die Kette, nicht je Glied.
#[test]
fn die_grenze_gilt_fuer_die_ganze_kette() {
    let innen = zlib(&vec![0u8; 4 * MIB]);
    let aussen = zlib(&innen);
    let stream = Stream::new(
        dictionary! { "Filter" => vec![Object::Name(b"FlateDecode".to_vec()), Object::Name(b"FlateDecode".to_vec())] },
        aussen,
    );
    let doc = Document::new();
    // Jedes Glied für sich passt unter 4 MiB, beide zusammen nicht.
    assert_eq!(
        decoded_content_within(&doc, &stream, 4 * MIB).map(|d| d.map(|d| d.len())),
        Err(Oversize)
    );
    assert_eq!(
        decoded_content_within(&doc, &stream, 4 * MIB + innen.len()).map(|d| d.map(|d| d.len())),
        Ok(Some(4 * MIB))
    );
}

/// Gegenprobe: eine gewöhnliche Kette (ASCIIHex vor Flate, wie ältere
/// Erzeuger sie schreiben) bleibt mit ihrer Arbeit unter dem Budget.
#[test]
fn eine_gewoehnliche_kette_bleibt_durchlaessig() {
    let text = b"BT /F1 12 Tf 72 700 Td (Kontoinhaber Max Mustermann) Tj ET\n".repeat(20_000);
    let hex: Vec<u8> = zlib(&text)
        .iter()
        .flat_map(|b| format!("{b:02X}").into_bytes())
        .chain(*b">")
        .collect();
    let pdf = datei(4, "[/ASCIIHexDecode /FlateDecode]", &hex);
    let limits = Limits {
        max_decompressed_bytes: (16 * MIB) as u64,
        ..Limits::default()
    };
    prescan(&pdf, &limits).expect("eine gewöhnliche Kette muss durchgehen");
    let check = leaks_many_within(&pdf, &["GEHEIM"], (16 * MIB) as u64);
    assert!(check.unchecked.is_empty(), "{:?}", check.unchecked);
}
