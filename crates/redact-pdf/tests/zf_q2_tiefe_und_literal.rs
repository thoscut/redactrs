//! Gegenprüfung Q2: die Tiefenmeldung (Befund P4-2) und `LeakCheck::literal`.
//!
//! Zwei Fragen:
//!
//! 1. Wird der Abbruch bei [`MAX_DEPTH`] = 32 **überall** gemeldet — im
//!    Trailer, im Dictionary eines Stream-Objekts, unter einem Namen, in einem
//!    `/ObjStm` —, und ist der Objektpfad nachrechenbar? Und: erreicht eine
//!    gewöhnliche Datei die Grenze (dann wäre die Meldung ein falscher Alarm)?
//! 2. Stimmt `literal` gegen eine unabhängige Regel — auch wenn beide
//!    Fassungen treffen, wenn nur eine trifft, und wenn der Begriff gar keinen
//!    Leerraum trägt?

mod common;

use common::{page, text_ops, SECRET};
use lopdf::{dictionary, Object, Stream, StringFormat};
use redact_pdf::{leaks_many_within, squeeze, LeakCheck};

/// Die Grenze aus `audit_bytes::MAX_DEPTH`.
const MAX_DEPTH: usize = 32;

fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("komprimierbar");
    e.finish().expect("komprimierbar")
}

fn pruefe(bytes: &[u8], needles: &[&str]) -> LeakCheck {
    leaks_many_within(bytes, needles, u64::MAX)
}

fn geheim() -> Object {
    Object::String(SECRET.as_bytes().to_vec(), StringFormat::Literal)
}

fn arrays(tief: usize, inner: Object) -> Object {
    let mut o = inner;
    for _ in 0..tief {
        o = Object::Array(vec![o]);
    }
    o
}

/// Die Zeile, die für `pfad` erwartet wird — mein eigener Text, nicht der des
/// Moduls: er muss Wort für Wort stimmen.
fn tiefenzeile(pfad: &str) -> String {
    format!(
        "{pfad}: nicht durchsucht — Verschachtelungstiefe {MAX_DEPTH} erreicht; \
         was tiefer liegt, hat keine Sicht gelesen"
    )
}

// ---------------------------------------------------------------------------
// 1. Die Tiefenmeldung
// ---------------------------------------------------------------------------

/// Unter einem Namen im Katalog. Der Wert unter `/Q2Tief` steht auf Tiefe 1,
/// das `k`-te Array darin auf Tiefe `1 + k`; gemeldet wird bei Tiefe 33, also
/// mit `k = 32` Klammerpaaren im Pfad. Nachgerechnet, nicht abgeschrieben.
#[test]
fn q2_tiefe_unter_einem_namen_wird_mit_pfad_gemeldet() {
    let mut d = page(&["harmlos"]);
    let katalog = d.catalog_id;
    d.catalog_set("Q2Tief", arrays(40, geheim()));
    let check = pruefe(&d.finish(), &[SECRET]);
    let pfad = format!(
        "Objekt {} {}/Q2Tief{}",
        katalog.0,
        katalog.1,
        "[0]".repeat(MAX_DEPTH)
    );
    assert!(
        check.unchecked.contains(&tiefenzeile(&pfad)),
        "erwartet: {pfad}\nbekommen: {:#?}",
        check.unchecked
    );
}

/// Ein Dictionary zwischendrin verbraucht eine Ebene: `/Q2Dict` (1),
/// `/Tiefer` (2), dann die Arrays — gemeldet wird mit `MAX_DEPTH - 1`
/// Klammerpaaren.
#[test]
fn q2_tiefe_im_verschachtelten_dictionary() {
    let mut d = page(&["harmlos"]);
    let katalog = d.catalog_id;
    d.catalog_set(
        "Q2Dict",
        Object::Dictionary(dictionary! { "Tiefer" => arrays(40, geheim()) }),
    );
    let check = pruefe(&d.finish(), &[SECRET]);
    let pfad = format!(
        "Objekt {} {}/Q2Dict/Tiefer{}",
        katalog.0,
        katalog.1,
        "[0]".repeat(MAX_DEPTH - 1)
    );
    assert!(
        check.unchecked.contains(&tiefenzeile(&pfad)),
        "erwartet: {pfad}\nbekommen: {:#?}",
        check.unchecked
    );
}

/// Im **Dictionary eines Stream-Objekts**.
#[test]
fn q2_tiefe_im_stream_dictionary() {
    let mut d = page(&["harmlos"]);
    let mut st = Stream::new(dictionary! {}, b"nichts".to_vec());
    st.dict.set("Q2Tief", arrays(40, geheim()));
    let id = d.add(Object::Stream(st));
    d.catalog_set("Q2S", Object::Reference(id));
    let check = pruefe(&d.finish(), &[SECRET]);
    let pfad = format!("Objekt {} {}/Q2Tief{}", id.0, id.1, "[0]".repeat(MAX_DEPTH));
    assert!(
        check.unchecked.contains(&tiefenzeile(&pfad)),
        "erwartet: {pfad}\nbekommen: {:#?}",
        check.unchecked
    );
}

/// Im **Trailer**.
#[test]
fn q2_tiefe_im_trailer() {
    let mut d = page(&["harmlos"]);
    d.doc.trailer.set("Q2Tief", arrays(40, geheim()));
    let check = pruefe(&d.finish(), &[SECRET]);
    let pfad = format!("Trailer/Q2Tief{}", "[0]".repeat(MAX_DEPTH));
    assert!(
        check.unchecked.contains(&tiefenzeile(&pfad)),
        "erwartet: {pfad}\nbekommen: {:#?}",
        check.unchecked
    );
}

/// In einem **Objekt-Stream**. Das enthaltene Objekt steht eine Ebene unter
/// dem Strom (Tiefe 1), also `MAX_DEPTH` Klammerpaare im Pfad.
///
/// `lopdf::Document::save_to` überspringt `/Type /ObjStm`-Ströme beim
/// Schreiben, deshalb ist diese Datei von Hand gebaut.
#[test]
fn q2_tiefe_in_einem_objektstrom() {
    let bytes = objstm_pdf(40);
    let check = pruefe(&bytes, &[SECRET]);
    let pfad = format!(
        "Objekt 5 0 <ObjStm> → Objekt 90 0{}",
        "[0]".repeat(MAX_DEPTH)
    );
    assert!(
        check.unchecked.contains(&tiefenzeile(&pfad)),
        "erwartet: {pfad}\nbekommen: {:#?}",
        check.unchecked
    );
    // Kalibrierung: flach findet die Objektsicht das Geheimnis im Container.
    let flach = pruefe(&objstm_pdf(1), &[SECRET]);
    assert!(
        flach.unchecked.is_empty(),
        "flach darf nichts melden: {:#?}",
        flach.unchecked
    );
    assert!(!flach.findings[0].is_empty(), "flach muss gefunden werden");
}

/// Eine Datei von Hand mit einem echten `/ObjStm`.
fn objstm_pdf(tief: usize) -> Vec<u8> {
    let inner = format!("{}({SECRET}){}", "[".repeat(tief), "]".repeat(tief));
    let header = "90 0 ";
    let mut plain = header.as_bytes().to_vec();
    plain.extend_from_slice(inner.as_bytes());
    plain.extend_from_slice(format!(" % {}", "A".repeat(512)).as_bytes());
    let packed = deflate(&plain);
    let content = b"BT /F1 10 Tf 72 700 Td (harmlos) Tj ET";
    let objekte: Vec<Vec<u8>> = vec![
        b"<</Type/Catalog/Pages 2 0 R/Q2ObjStm 5 0 R>>".to_vec(),
        b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
        b"<</Type/Page/Parent 2 0 R/Contents 4 0 R/MediaBox[0 0 595 842]>>".to_vec(),
        {
            let mut o = format!("<</Length {}>>\nstream\n", content.len()).into_bytes();
            o.extend_from_slice(content);
            o.extend_from_slice(b"\nendstream");
            o
        },
        {
            let mut o = format!(
                "<</Type/ObjStm/N 1/First {}/Filter/FlateDecode/Length {}>>\nstream\n",
                header.len(),
                packed.len()
            )
            .into_bytes();
            o.extend_from_slice(&packed);
            o.extend_from_slice(b"\nendstream");
            o
        },
    ];
    let mut out = b"%PDF-1.5\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objekte.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objekte.len() + 1);
    for off in &offsets {
        table.push_str(&format!("{off:010} 00000 n \n"));
    }
    table.push_str(&format!(
        "trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n",
        objekte.len() + 1
    ));
    out.extend_from_slice(table.as_bytes());
    out
}

/// Genau an der Grenze: 32 Arrays werden noch gelesen, 33 nicht mehr. Eine
/// Grenze, die eine Ebene zu früh zuschlägt, wäre ein falscher Alarm.
#[test]
fn q2_die_grenze_liegt_wo_sie_steht() {
    for (tief, gemeldet) in [(30usize, false), (31, false), (32, true), (33, true)] {
        let mut d = page(&["harmlos"]);
        d.catalog_set("Q2Tief", arrays(tief, geheim()));
        let check = pruefe(&d.finish(), &[SECRET]);
        let tiefen: Vec<_> = check
            .unchecked
            .iter()
            .filter(|m| m.contains("Verschachtelungstiefe"))
            .collect();
        assert_eq!(
            !tiefen.is_empty(),
            gemeldet,
            "{tief} Arrays: erwartet gemeldet={gemeldet}, bekommen {tiefen:#?}"
        );
        // Bis 31 muss die Objektsicht die Zeichenkette auch **finden**.
        if !gemeldet {
            assert!(
                check.findings[0]
                    .iter()
                    .any(|m| m.starts_with("Objekt") && m.contains("Zeichenkette")),
                "{tief} Arrays: die Objektsicht muss die Zeichenkette lesen: {:#?}",
                check.findings[0]
            );
        }
    }
}

/// Ein gewöhnliches Dokument darf die Grenze nicht erreichen: 500 Seiten in
/// einem **balancierten `/Kids`-Baum** (Verzweigung 4, also 5 Ebenen
/// Seitenbaum) mit Outline-Baum und `/StructTreeRoot`. Verweise verfolgt die
/// Objektsicht nicht — die Tiefe zählt nur *direkte* Verschachtelung —, und
/// genau das muss messbar sein.
#[test]
fn q2_ein_tiefer_seitenbaum_loest_keinen_falschen_alarm_aus() {
    let bytes = grosses_dokument(500);
    let check = pruefe(&bytes, &["Nichtvorhanden XYZ"]);
    assert!(
        check.unchecked.is_empty(),
        "500 Seiten mit tiefem /Kids-Baum: keine NICHT-GEPRÜFT-Zeile erwartet, \
         bekommen {:#?}",
        check.unchecked
    );
    assert!(
        check.findings[0].is_empty(),
        "der Begriff steht nicht darin"
    );
    // Und die Datei ist wirklich tief verschachtelt im Seitenbaum.
    let doc = lopdf::Document::load_mem(&bytes).expect("ladbar");
    assert_eq!(doc.get_pages().len(), 500);
}

/// `n` Seiten, `/Kids` mit Verzweigung 4 (bei 500 Seiten fünf Ebenen).
fn grosses_dokument(n: usize) -> Vec<u8> {
    let mut doc = lopdf::Document::with_version("1.5");
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let res = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut ebene: Vec<lopdf::ObjectId> = Vec::new();
    let mut seiten = Vec::new();
    for i in 0..n {
        let content = doc.add_object(Stream::new(
            dictionary! {},
            text_ops(&[&format!("Seite {} — Kontoauszug", i + 1), "Buchung 01"]),
        ));
        let id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Contents" => content,
            "Resources" => res,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        seiten.push(id);
        ebene.push(id);
    }
    // Von unten nach oben zu Knoten mit je vier Kindern bündeln.
    while ebene.len() > 1 {
        let mut naechste = Vec::new();
        for gruppe in ebene.chunks(4) {
            let kinder: Vec<Object> = gruppe.iter().map(|&k| Object::Reference(k)).collect();
            let knoten = doc.add_object(dictionary! {
                "Type" => "Pages",
                "Kids" => kinder,
                "Count" => seiten.len() as i64,
            });
            for &k in gruppe {
                doc.get_dictionary_mut(k)
                    .expect("Knoten")
                    .set("Parent", knoten);
            }
            naechste.push(knoten);
        }
        ebene = naechste;
    }
    let wurzel = ebene[0];
    doc.get_dictionary_mut(wurzel)
        .expect("Wurzel")
        .set("Count", seiten.len() as i64);
    let katalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => wurzel });
    doc.trailer.set("Root", katalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    out
}

// ---------------------------------------------------------------------------
// 2. `literal`
// ---------------------------------------------------------------------------

/// Meine eigene Regel: der Begriff hat **wörtlich** getroffen, wenn irgendeine
/// seiner Bytefassungen in der Rohdatei steht oder sein Text wörtlich in einem
/// der Texte steht, die das Orakel gelesen hat.
///
/// Sie ist grob, aber unabhängig: statt sie überall auszurechnen, wird sie an
/// Fällen geprüft, in denen die Antwort ohne Zweifel feststeht — und gegen die
/// Meldungstexte gekreuzt: eine Fundstelle ohne „ohne Leerraum“ **ist** ein
/// wörtlicher Treffer.
fn literal_aus_meldungen(hits: &[String]) -> bool {
    hits.iter().any(|m| !m.contains("ohne Leerraum"))
}

/// Ein Fall: Name, Datei, Suchbegriffe, erwartetes `literal`.
type Fall = (&'static str, Vec<u8>, Vec<&'static str>, Vec<bool>);

fn seite_mit(zeilen: &[&str]) -> Vec<u8> {
    let mut d = page(&[]);
    d.set_content(&text_ops(zeilen));
    d.finish()
}

#[test]
fn q2_literal_stimmt_mit_den_meldungen_zusammen() {
    let flach = squeeze(SECRET);
    let faelle: Vec<Fall> = vec![
        (
            "nur die gequetschte Fassung steht in der Datei",
            seite_mit(&[&format!("IBAN: {flach}")]),
            vec![SECRET],
            vec![false],
        ),
        (
            "beide Fassungen stehen darin",
            seite_mit(&[&format!("IBAN: {SECRET}"), &format!("IBAN: {flach}")]),
            vec![SECRET],
            vec![true],
        ),
        (
            "nur die wörtliche Fassung",
            seite_mit(&[&format!("IBAN: {SECRET}")]),
            vec![SECRET],
            vec![true],
        ),
        (
            "Begriff ohne Leerraum",
            seite_mit(&["Kontoinhaber: Mustermann"]),
            vec!["Mustermann"],
            vec![true],
        ),
        (
            "Begriff ohne Leerraum, nicht in der Datei",
            seite_mit(&["nichts"]),
            vec!["Mustermann"],
            vec![false],
        ),
        (
            "nur in TJ-Bruchstücken, also nur ohne Leerraum",
            {
                let mut d = page(&[]);
                d.set_content(
                    b"BT /F1 10 Tf 72 700 Td [(DE89) -100 (3704) -100 (0044) -100 \
                      (0532) -100 (0130) -100 (00)] TJ ET",
                );
                d.finish()
            },
            vec![SECRET],
            vec![false],
        ),
        (
            "mehrere Begriffe, gemischt",
            seite_mit(&[&format!("IBAN: {flach}"), "Mustermann"]),
            vec![SECRET, "Mustermann", "Nixda"],
            vec![false, true, false],
        ),
        (
            "leerer Begriff behält seinen Platz",
            seite_mit(&[&format!("IBAN: {SECRET}")]),
            vec!["", SECRET],
            vec![false, true],
        ),
    ];

    for (name, bytes, needles, soll) in faelle {
        let check = pruefe(&bytes, &needles);
        assert_eq!(check.literal, soll, "{name}: literal falsch");
        assert_eq!(
            check.literal.len(),
            check.findings.len(),
            "{name}: literal muss so lang sein wie findings"
        );
        // Kreuzprobe gegen die Meldungstexte.
        for (i, hits) in check.findings.iter().enumerate() {
            assert_eq!(
                check.literal[i],
                literal_aus_meldungen(hits),
                "{name}, Begriff {i}: literal={} passt nicht zu den Meldungen {hits:#?}",
                check.literal[i]
            );
        }
    }
}

/// Ein Begriff, dessen Leerraum nur *innen* wegfällt, darf nicht dadurch
/// „wörtlich“ werden, dass ein **anderer** Begriff wörtlich traf.
#[test]
fn q2_literal_wird_nicht_zwischen_begriffen_vermischt() {
    let flach = squeeze(SECRET);
    let check = pruefe(
        &seite_mit(&[&format!("IBAN: {flach}"), "Max Mustermann"]),
        &[SECRET, "Max Mustermann"],
    );
    assert_eq!(check.literal, vec![false, true]);
}
