//! Der **Rumpf** einer Datei zählt zum Parse-Budget — nicht nur ihre Streams.
//!
//! # Der Befund
//!
//! `--max-parsed-mb` verbuchte ausschließlich *Streams*, deren Inhalt wie
//! PDF-Syntax aussieht (Seiteninhalt, Objekt-Streams). Was unkomprimiert im
//! Rumpf der Datei steht — Objektköpfe, Dictionaries, Arrays, die
//! Querverweistabelle —, sah das Budget überhaupt nicht. `lopdf` parst genau
//! das aber zu `Object`-Werten, und zwar **vollständig und sofort**.
//!
//! # Die Messung
//!
//! Ein Dictionary-Eintrag `/ab 0` kostet 5 Byte in der Datei und **197 Byte** im
//! Speicher (`size_of::<lopdf::Object>()` ist allein 120; dazu der Schlüssel als
//! `Vec<u8>` und die Reserve der `IndexMap`). Der Preis hängt an der *Zahl* der
//! Einträge, nicht an ihrer Schreibweise — je knapper geschrieben, desto
//! schlimmer:
//!
//! | Datei (44 MB, unkomprimiert) | `Document` | Faktor |
//! |---|---:|---:|
//! | 5-Byte-Einträge, dicht gepackt | 1 804 MB | **40,7** |
//! | 7-Byte-Einträge | 1 535 MB | 30,8 |
//! | nur Zahlen, keine Dictionaries | 299 MB | 3,7 |
//! | ein einziger Content-Stream | 44 MB | 1,0 |
//!
//! Über die Kommandozeile gemessen (Release, `ru_maxrss`): die dichte Datei
//! lief mit Rückgabewert 0 durch und belegte dabei **3 756 MB** — mit einem
//! Parse-Budget von 16 MB und einem Dekompressionsbudget von 1 GB, von denen
//! keins etwas gesehen hat.
//!
//! # Warum kein Konto nach dem Parsen
//!
//! `lopdf::Document::load_mem` parst alle Objekte auf einmal
//! (`reader.rs::load_objects_raw`); abbrechen lässt sich dabei nicht. Der
//! einzige Haken ist `LoadOptions::filter` — ein zustandsloser Funktionszeiger,
//! der Objekte nur **wegwerfen** kann. Ein weggeworfenes Objekt hinterlässt
//! genau das, was [`redact_pdf::document::Limits::max_nesting_depth`] als
//! untragbar beschreibt: ein Dokument, das lädt und unauffällig kaputt ist.
//!
//! Ein Konto **nach** dem Laden wäre ehrlicher als gar keins, käme aber zu
//! spät — die Spitze *ist* das fertige `Document`. Die Vorprüfung dagegen läuft
//! ohnehin schon über jedes Byte der Datei und noch vor `load_mem`. Dort kostet
//! die Auskunft nichts.
//!
//! # Warum es keine neue Grenze braucht
//!
//! Dieselbe Bombe **komprimiert** wurde seit jeher abgelehnt: ein Objekt-Stream
//! wird ausgepackt und als Syntax verbucht. Die Lücke war also nicht die Bauart
//! der Bombe, sondern allein die Frage, ob sie komprimiert war — und das sucht
//! sich ein Angreifer als Erstes aus. Es fehlte kein Budget, sondern eine
//! Klasse in einem vorhandenen. Der Test
//! [`beide_bauarten_derselben_bombe_werden_gleich_behandelt`] hält das fest.

use redact_pdf::document::{load_from_bytes_with_limits, Limits};
use redact_pdf::testing::{build_pdf, TextItem};

/// Baut ein PDF aus vorgegebenen Objektkörpern (1-basiert nummeriert).
fn datei(objekte: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objekte.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objekte.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
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

/// Das kleinste vollständige Seitengerüst — Katalog, Seitenbaum, Seite, Inhalt.
fn geruest() -> Vec<Vec<u8>> {
    vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
           /Resources << >> /Contents 4 0 R >>"
            .to_vec(),
        b"<< /Length 6 >>\nstream\nBT ET\nendstream".to_vec(),
    ]
}

/// Ein Dictionary mit `n` verschiedenen zweibuchstabigen Schlüsseln —
/// 5 Byte je Eintrag, die dichteste in PDF schreibbare Bauart.
fn dichtes_dictionary(n: usize) -> Vec<u8> {
    let buchstaben: Vec<u8> = (b'a'..=b'z').chain(b'A'..=b'Z').collect();
    let mut d = b"<<".to_vec();
    for k in 0..n {
        d.push(b'/');
        d.push(buchstaben[k / buchstaben.len() % buchstaben.len()]);
        d.push(buchstaben[k % buchstaben.len()]);
        d.extend_from_slice(b" 0");
    }
    d.extend_from_slice(b">>");
    d
}

/// Eine Datei, deren Rumpf ungefähr `mb` Megabyte Dictionaries enthält —
/// **ohne** einen einzigen Stream, in dem sie sich verstecken könnten.
fn rumpfbombe(mb: usize) -> Vec<u8> {
    let mut objekte = geruest();
    let d = dichtes_dictionary(600);
    let n = (mb * 1024 * 1024) / (d.len() + 20);
    for _ in 0..n {
        objekte.push(d.clone());
    }
    datei(&objekte)
}

fn limits(parsed_mb: u64) -> Limits {
    Limits {
        max_parsed_bytes: parsed_mb * 1024 * 1024,
        ..Limits::default()
    }
}

/// **Der Befund.** Ein Rumpf jenseits des Budgets wird abgelehnt — und zwar
/// mit der Begründung, um die es geht.
#[test]
fn ein_zu_grosser_rumpf_wird_abgelehnt() {
    let bytes = rumpfbombe(4);
    let fehler = load_from_bytes_with_limits(&bytes, &limits(1))
        .expect_err("eine Datei mit 4 MB Dictionaries kam durch ein Budget von 1 MB");
    let text = fehler.to_string();
    assert!(
        text.contains("Budget von 1 MB"),
        "die Meldung nennt das Budget nicht: {text}"
    );
    assert!(
        text.contains("Objektköpfe") || text.contains("Querverweistabelle"),
        "die Meldung sagt nicht, welcher Teil der Datei es war: {text}"
    );
}

/// **Die Mutationsprobe von der anderen Seite.** Genau dieselbe Datei geht
/// durch, sobald das Budget reicht. Ohne diesen Test wäre der obige auch dann
/// grün, wenn die Datei aus einem ganz anderen Grund abgelehnt würde.
#[test]
fn dieselbe_datei_geht_mit_grossem_budget_durch() {
    let bytes = rumpfbombe(4);
    load_from_bytes_with_limits(&bytes, &limits(64))
        .expect("mit ausreichendem Budget muss dieselbe Datei laden");
}

/// **Die eigentliche Begründung.** Komprimiert wurde diese Bombe seit jeher
/// abgelehnt, unkomprimiert nicht. Beide Bauarten tragen denselben Inhalt und
/// kosten denselben Speicher; sie müssen dieselbe Antwort bekommen.
#[test]
fn beide_bauarten_derselben_bombe_werden_gleich_behandelt() {
    let d = dichtes_dictionary(600);
    let n = (4 * 1024 * 1024) / d.len();

    // (a) offen im Rumpf.
    let mut offen = geruest();
    for _ in 0..n {
        offen.push(d.clone());
    }
    let offen = datei(&offen);

    // (b) dieselben Dictionaries in einem komprimierten Objekt-Stream.
    let mut kopf = Vec::new();
    let mut koerper = Vec::new();
    for k in 0..n {
        kopf.extend_from_slice(format!("{} {} ", 10 + k, koerper.len()).as_bytes());
        koerper.extend_from_slice(&d);
        koerper.push(b'\n');
    }
    kopf.push(b'\n');
    let mut inhalt = kopf.clone();
    inhalt.extend_from_slice(&koerper);
    let gepackt = {
        use std::io::Write;
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(&inhalt).unwrap();
        enc.finish().unwrap()
    };
    let mut verpackt = geruest();
    let mut objstm = format!(
        "<< /Type /ObjStm /N {n} /First {} /Filter /FlateDecode /Length {} >>\nstream\n",
        kopf.len(),
        gepackt.len()
    )
    .into_bytes();
    objstm.extend_from_slice(&gepackt);
    objstm.extend_from_slice(b"\nendstream");
    verpackt.push(objstm);
    let verpackt = datei(&verpackt);

    for (name, bytes) in [("offen im Rumpf", offen), ("im Objekt-Stream", verpackt)] {
        let fehler = load_from_bytes_with_limits(&bytes, &limits(1))
            .err()
            .unwrap_or_else(|| {
                panic!("{name}: dieselbe Bombe muss in beiden Bauarten abgelehnt werden")
            });
        assert!(
            fehler.to_string().contains("Budget von 1 MB"),
            "{name}: abgelehnt, aber nicht wegen des Parse-Budgets: {fehler}"
        );
    }
}

/// **Die Gegenprobe.** Gewöhnliche Dokumente gehen mit der Vorgabe durch.
///
/// Gemessen an erzeugten Dateien: der Rumpf eines komprimierten PDFs macht
/// 1 bis 3 % seiner Bytes aus (2 000 Textseiten: 0,48 MB Rumpf; 300 Scanseiten
/// mit 600 MB Bilddaten: 0,13 MB Rumpf). Vom 16-MB-Budget ist das ein
/// Bruchteil, und **kein** Aufschlag gegenüber vorher, der ins Gewicht fällt.
#[test]
fn gewoehnliche_dokumente_gehen_mit_der_vorgabe_durch() {
    // 200 Seiten mit je 40 Zeilen — mehr, als ein Kontoauszug je hat.
    let seiten: Vec<Vec<TextItem>> = (0..200)
        .map(|s| {
            (0..40)
                .map(|z| {
                    TextItem::new(
                        72.0,
                        800.0 - 14.0 * z as f64,
                        10.0,
                        format!(
                            "01.03.2026 Ueberweisung DE89370400440532013000 -1.234,56 EUR {s}/{z}"
                        ),
                    )
                })
                .collect()
        })
        .collect();
    let bytes = build_pdf(&seiten);
    load_from_bytes_with_limits(&bytes, &Limits::default())
        .expect("ein gewöhnliches 200-Seiten-Dokument muss mit der Vorgabe laden");

    // Und die kleinste gewöhnliche Datei, die es gibt.
    let klein = redact_pdf::testing::minimal_pdf("IBAN DE89 3704 0044 0532 0130 00");
    load_from_bytes_with_limits(&klein, &Limits::default()).expect("einseitiges PDF muss laden");
}

/// Ein großer **Stream** bleibt ein großer Stream: die Nutzlast eines Bildes
/// zählt weiterhin nicht ins Parse-Budget. Sonst wäre aus der Korrektur eine
/// Grenze für Scans geworden, und die haben ihr eigenes Budget.
#[test]
fn bildnutzlast_zaehlt_weiterhin_nicht_ins_parse_budget() {
    let mut objekte = geruest();
    // 8 MB Bilddaten — weit über dem Parse-Budget von 1 MB, aber Nutzlast.
    let roh: Vec<u8> = (0..8 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    let mut bild = format!(
        "<< /Type /XObject /Subtype /Image /Width 2048 /Height 4096 \
         /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
        roh.len()
    )
    .into_bytes();
    bild.extend_from_slice(&roh);
    bild.extend_from_slice(b"\nendstream");
    objekte.push(bild);
    let bytes = datei(&objekte);

    load_from_bytes_with_limits(&bytes, &limits(1))
        .expect("8 MB Bildnutzlast dürfen ein Parse-Budget von 1 MB nicht reißen");
}
