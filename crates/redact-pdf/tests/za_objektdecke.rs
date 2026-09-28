//! Der Aufblähfaktor lässt sich **nicht** an Dateibytes festmachen.
//!
//! # Der Befund
//!
//! Eine Datei aus reinem Rumpf — 16 761 999 Byte, also *innerhalb* des
//! 16-MB-Parse-Budgets —, gefüllt mit 19 920 Objekten zu je 400 **leeren**
//! Arrays `[[][][]…]`, lief mit Rückgabewert 0 durch und belegte dabei
//! 5 739 MB (über mehrere Läufe 11 bis 26 s — die Wanduhr schwankt mit der
//! Fremdlast, der Speicher nicht). Dieselbe Bauart mit 21,4 MB wurde
//! abgelehnt: die Decke griff, sie hing nur an der falschen Größe. Jetzt wird
//! dieselbe Datei bei 22 MB abgelehnt.
//!
//! # Die Messreihe, die die Frage beantwortet
//!
//! Sieben Formen, dieselbe Dateigröße (4 MB), `Document` nach `load_mem` mit
//! zählendem Allokator gemessen — die Reihe steht als Testlauf in
//! `za_objektspeicher_gerechnet.rs` und druckt genau diese Zahlen:
//!
//! | Form (4 MB Datei) | `Document` gemessen | gerechnet | Byte je Dateibyte |
//! |---|---:|---:|---:|
//! | `[[][][]…]` — leere Arrays | 1 204,7 MB | 1 224,5 MB (1,02×) | **301,6** |
//! | `[/a/a…]` — Namen | 300,2 MB | 312,4 MB (1,04×) | 75,2 |
//! | `[0 0 0…]` — Zahlen | 292,6 MB | 312,4 MB (1,07×) | 73,3 |
//! | `<</ab 0 …>>` — Dictionary-Einträge | 176,1 MB | 248,7 MB (1,41×) | 44,1 |
//! | `[<<>><<>>…]` — leere Dictionaries | 146,7 MB | — | 36,7 |
//! | `[1 0 R …]` — Verweise | 146,0 MB | 312,4 MB (2,14×) | 36,5 |
//! | eine 800-Byte-Zeichenkette | 4,6 MB | — | 1,1 |
//!
//! Je **Dateibyte** schwankt der Preis um den Faktor 274 (1,1 bis 301,6), je
//! **Objekt** nur um 4 (153,6 Byte für eine Zahl, 632,4 für ein leeres Array).
//! Damit ist die Frage beantwortet: an Dateibytes lässt sich der Faktor nicht
//! festmachen, an der Zahl und Art der Objekte schon. Genau das rechnet die
//! Vorprüfung jetzt aus — `OBJEKT_BYTES` je Objekt, `ARRAY_BYTES` je Array —
//! und deckelt es bei `max_parsed_bytes × OBJEKTSPEICHER_JE_BUDGETBYTE`.
//!
//! Dass die Rechnung nie **unter** dem wirklich belegten Speicher liegt, hält
//! `za_objektspeicher_gerechnet.rs` fest; das ist die Eigenschaft, an der die
//! Decke hängt. Hier steht die andere Hälfte: beide Richtungen des Verhaltens.

use redact_pdf::document::{load_from_bytes_with_limits, prescan, Limits};
use redact_pdf::testing::{build_pdf, demo_statement, TextItem};

// ---------------------------------------------------------------------------
// Bauplätze
// ---------------------------------------------------------------------------

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

/// Das kleinste vollständige Seitengerüst.
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

/// Ein Objektkörper der gewünschten Form, jeweils rund 800 Byte lang.
///
/// Alle Formen sind gleich groß — der Unterschied zwischen ihnen ist genau
/// das, worum es geht.
fn form(name: &str) -> Vec<u8> {
    let mut v = b"[".to_vec();
    match name {
        "leere Arrays" => (0..400).for_each(|_| v.extend_from_slice(b"[]")),
        "Zahlen" => (0..400).for_each(|_| v.extend_from_slice(b"0 ")),
        "Namen" => (0..400).for_each(|_| v.extend_from_slice(b"/a")),
        "leere Dictionaries" => (0..200).for_each(|_| v.extend_from_slice(b"<<>>")),
        "Dictionaries" => {
            let b: Vec<u8> = (b'a'..=b'z').chain(b'A'..=b'Z').collect();
            let mut d = b"<<".to_vec();
            for k in 0..160usize {
                d.push(b'/');
                d.push(b[k / b.len() % b.len()]);
                d.push(b[k % b.len()]);
                d.extend_from_slice(b" 0");
            }
            d.extend_from_slice(b">>");
            return d;
        }
        _ => unreachable!("unbekannte Form {name}"),
    }
    v.push(b']');
    v
}

/// Eine Datei aus reinem Rumpf der Form `name`, rund `ziel` Byte groß und
/// **ohne einen einzigen Stream**, in dem sich etwas verstecken könnte.
fn rumpf(name: &str, ziel: usize) -> Vec<u8> {
    let o = form(name);
    // Je Objekt kommen Kopf, `endobj` und ein Eintrag der Querverweistabelle
    // dazu — zusammen rund 40 Byte.
    let n = ziel / (o.len() + 40);
    let mut alle = geruest();
    alle.extend(std::iter::repeat_n(o, n));
    datei(&alle)
}

/// Kommt `nadel` in `heu` vor?
fn enthaelt(heu: &[u8], nadel: &[u8]) -> bool {
    heu.windows(nadel.len()).any(|w| w == nadel)
}

fn budget(mb: u64) -> Limits {
    Limits {
        max_parsed_bytes: mb * 1024 * 1024,
        ..Limits::default()
    }
}

/// Sagt die Meldung, dass die **Objektdecke** angeschlagen hat?
fn wegen_objektdecke(e: &redact_core::RedactError) -> bool {
    e.to_string().contains("gerechnet mehr")
}

/// Das kleinste `--max-parsed-mb` in Byte, mit dem die Datei durch die
/// Vorprüfung kommt, und welches der beiden Budgets zuletzt gebunden hat.
///
/// Deterministisch und ohne neue öffentliche API: gefragt wird nur das
/// Verhalten von [`prescan`], halbiert wird über die Grenze.
fn schwelle(bytes: &[u8]) -> (u64, &'static str) {
    let versuch = |n: u64| {
        prescan(
            bytes,
            &Limits {
                max_parsed_bytes: n,
                ..Limits::default()
            },
        )
    };
    let (mut lo, mut hi) = (0u64, 1u64);
    while versuch(hi).is_err() {
        lo = hi;
        hi = hi.checked_mul(2).expect("Schwelle liegt jenseits von u64");
    }
    while lo + 1 < hi {
        let m = lo + (hi - lo) / 2;
        if versuch(m).is_ok() {
            hi = m;
        } else {
            lo = m;
        }
    }
    let bindet = match versuch(hi - 1) {
        Ok(()) => "keines",
        Err(e) if wegen_objektdecke(&e) => "Objektdecke",
        Err(_) => "Byte-Budget",
    };
    (hi, bindet)
}

// ---------------------------------------------------------------------------
// Der Befund
// ---------------------------------------------------------------------------

/// **Der Release-Blocker.** Eine Datei, die das Byte-Budget einhält, und
/// trotzdem Gigabytes belegt.
///
/// Der Test prüft beides zusammen, sonst beweist er nichts: die Datei ist
/// kleiner als das Budget in Byte, **und** sie wird abgelehnt, **und** zwar
/// mit der Begründung, um die es geht. Gemessen am Release-Binary vor der
/// Korrektur: Rückgabewert 0 und 5 739 MB Spitzenspeicher; danach
/// Rückgabewert 1 und 22 MB.
#[test]
fn eine_datei_aus_leeren_arrays_wird_abgelehnt_obwohl_sie_ins_byte_budget_passt() {
    let bytes = rumpf("leere Arrays", 15_900_000);
    let grenzen = Limits::default();
    assert!(
        (bytes.len() as u64) < grenzen.max_parsed_bytes,
        "die Datei muss ins Byte-Budget passen, sonst prüft der Test das Falsche: \
         {} Byte gegen {} Byte",
        bytes.len(),
        grenzen.max_parsed_bytes
    );

    let fehler = load_from_bytes_with_limits(&bytes, &grenzen)
        .expect_err("eine Datei aus über sieben Millionen leeren Arrays darf nicht laden");
    assert!(
        wegen_objektdecke(&fehler),
        "abgelehnt, aber nicht wegen der Objektdecke: {fehler}"
    );
}

/// **Die Mutationsprobe von der anderen Seite.** Dieselbe Datei läuft durch,
/// sobald jemand das Budget ausdrücklich anhebt.
///
/// Ohne diesen Test wäre der obige auch dann grün, wenn die Datei aus einem
/// ganz anderen Grund abgelehnt würde — und die Decke wäre eine Mauer statt
/// einer Voreinstellung.
#[test]
fn dieselbe_datei_laeuft_mit_angehobenem_budget_durch() {
    let bytes = rumpf("leere Arrays", 15_900_000);
    load_from_bytes_with_limits(&bytes, &budget(128))
        .expect("mit ausdrücklich angehobenem --max-parsed-mb muss dieselbe Datei laden");
}

/// **Die Frage aus der Überschrift, als Zusicherung.** Zwei Dateien
/// derselben Größe, zwei verschiedene Antworten.
///
/// `[[][][]…]` und `[<<>><<>>…]` sind auf das Byte gleich lang und sehen sich
/// zum Verwechseln ähnlich. Im Speicher kostet das leere Array 632 Byte, das
/// leere Dictionary 154 — Faktor 4. Wäre der Aufblähfaktor an Dateibytes
/// festzumachen, müssten beide dieselbe Antwort bekommen; sie tun es nicht.
/// Genau deshalb kann die Decke nicht an Bytes hängen.
///
/// (Bei dieser Größe ebenfalls abgelehnt, gemessen: `[0 0 0…]` und
/// `[/a/a…]`. Sie stehen nicht in der Zusicherung, weil sie dichter an der
/// Decke liegen und der Test dann die Decke nachzeichnete statt die Frage.)
#[test]
fn die_antwort_haengt_an_der_form_nicht_an_der_groesse() {
    let teuer = rumpf("leere Arrays", 15_900_000);
    let billig = rumpf("leere Dictionaries", 15_900_000);
    assert_eq!(
        teuer.len(),
        billig.len(),
        "die beiden Formen müssen gleich groß sein, sonst misst der Test die Größe"
    );

    let fehler = load_from_bytes_with_limits(&teuer, &Limits::default())
        .expect_err("lauter leere Arrays müssen abgelehnt werden");
    assert!(
        wegen_objektdecke(&fehler),
        "abgelehnt, aber nicht wegen der Objektdecke: {fehler}"
    );
    load_from_bytes_with_limits(&billig, &Limits::default())
        .expect("dieselbe Größe aus leeren Dictionaries kostet ein Viertel und muss laden");
}

// ---------------------------------------------------------------------------
// Die andere Richtung: gewöhnliche Dateien
// ---------------------------------------------------------------------------

/// 200 Seiten Scan: je Seite ein Graustufenbild, überwiegend weiß.
fn scan(seiten: usize) -> Vec<u8> {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    let (w, h) = (850usize, 1100usize);
    for s in 0..seiten {
        let mut roh = vec![0xffu8; w * h];
        for zeile in 0..40 {
            let y = 40 + zeile * 26 + (s % 3);
            for x in 60..790 {
                if (x + zeile) % 7 < 4 {
                    roh[y * w + x] = 0x20;
                }
            }
        }
        let bild = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image",
                "Width" => w as i64, "Height" => h as i64,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            roh,
        ));
        let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild } });
        let inhalt = doc.add_object(Stream::new(
            dictionary! {},
            b"q 595 0 0 770 0 36 cm /Im0 Do Q\n".to_vec(),
        ));
        let seite = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id,
            "Contents" => inhalt, "Resources" => res,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        kids.push(Object::Reference(seite));
    }
    let n = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n }),
    );
    let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", cat);
    doc.compress();
    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("speicherbar");
    buf
}

fn textseiten(seiten: usize, zeilen: usize) -> Vec<u8> {
    let s: Vec<Vec<TextItem>> = (0..seiten)
        .map(|p| {
            (0..zeilen)
                .map(|z| {
                    TextItem::new(
                        72.0,
                        800.0 - 14.0 * z as f64,
                        10.0,
                        format!(
                            "01.03.2026 Ueberweisung DE89370400440532013000 -1.234,56 EUR {p}/{z}"
                        ),
                    )
                })
                .collect()
        })
        .collect();
    build_pdf(&s)
}

/// Dieselben 500 Textseiten, aber mit **Querverweis-Strom** statt Tabelle.
fn mit_querverweis_strom(roh: &[u8]) -> Vec<u8> {
    let doc = lopdf::Document::load_mem(roh).expect("ladbar");
    let mut kopie = doc.clone();
    kopie.version = "1.5".to_string();
    let mut buf = Vec::new();
    kopie.save_modern(&mut buf).expect("speicherbar");
    assert!(
        enthaelt(&buf, b"/Type/XRef") || enthaelt(&buf, b"/Type /XRef"),
        "diese Gegenprobe braucht einen Querverweis-Strom, sonst prüft sie nichts"
    );
    buf
}

/// Alle Pflicht-Gegenproben, mit der Vorgabe.
#[test]
fn gewoehnliche_dokumente_gehen_mit_der_vorgabe_durch() {
    let text500 = textseiten(500, 40);
    let proben: Vec<(&str, Vec<u8>)> = vec![
        ("--write-demo (Kontoauszug)", demo_statement()),
        ("500 Textseiten", text500.clone()),
        ("2 000 Textseiten", textseiten(2_000, 40)),
        ("2 937 Textseiten", textseiten(2_937, 40)),
        ("200 Scanseiten mit Bildern", scan(200)),
        ("Querverweis-Strom", mit_querverweis_strom(&text500)),
    ];
    for (name, bytes) in &proben {
        load_from_bytes_with_limits(bytes, &Limits::default())
            .unwrap_or_else(|e| panic!("{name} ({} Byte) muss laden: {e}", bytes.len()));
    }
}

/// **Die Schwelle darf nicht sinken.** Bei gewöhnlichen Dokumenten entscheidet
/// weiterhin das Byte-Budget, nicht die neue Decke.
///
/// Das ist die schärfere Aussage als „läuft durch“: solange das Byte-Budget
/// bindet, ist die Grenze, ab der ein Dokument zu groß wird, exakt dieselbe
/// wie vorher. Eine Decke, die gewöhnliche Dateien früher ablehnt als bisher,
/// wäre ein Verfügbarkeitsfehler — und dieser Test fiele dann auf.
#[test]
fn bei_gewoehnlichen_dokumenten_bindet_weiter_das_byte_budget() {
    let text500 = textseiten(500, 40);
    let proben: Vec<(&str, Vec<u8>)> = vec![
        ("--write-demo (Kontoauszug)", demo_statement()),
        ("500 Textseiten", text500.clone()),
        ("2 000 Textseiten", textseiten(2_000, 40)),
        ("2 937 Textseiten", textseiten(2_937, 40)),
        ("200 Scanseiten mit Bildern", scan(200)),
        ("Querverweis-Strom", mit_querverweis_strom(&text500)),
    ];
    for (name, bytes) in &proben {
        let (kleinstes, bindet) = schwelle(bytes);
        assert_eq!(
            bindet, "Byte-Budget",
            "{name}: die Objektdecke bindet früher als das Byte-Budget — \
             die Schwelle für gewöhnliche Dokumente ist gesunken"
        );
        assert!(
            kleinstes <= bytes.len() as u64 + 64 * 1024,
            "{name}: braucht {kleinstes} Byte Budget für {} Byte Datei",
            bytes.len()
        );
    }
}

/// Ein **langer, ehrlicher Seiteninhalt** läuft weiter durch.
///
/// 1,5 Millionen Operationen in einem einzigen Strom, 13,5 MB: gerechnet
/// 915 MB Objektspeicher, gemessen 2 178 MB Spitzenspeicher. Das ist der
/// teuerste Fall, den dieser Baum ausdrücklich zulässt (siehe
/// `resource_bombs::a_long_honest_content_stream_is_not_mistaken_for_a_fanout`)
/// — und damit die Zahl, die die Decke nach unten festhält.
#[test]
fn ein_langer_ehrlicher_seiteninhalt_laeuft_weiter_durch() {
    let mut inhalt =
        b"BT /F1 12 Tf 1 0 0 1 72 700 Tm (DE89 3704 0044 0532 0130 00) Tj ET\n".to_vec();
    inhalt.extend(b"0 0 0 rg\n".repeat(1_500_000));
    let objekte = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
           /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        {
            let mut s = format!("<< /Length {} >>\nstream\n", inhalt.len()).into_bytes();
            s.extend_from_slice(&inhalt);
            s.extend_from_slice(b"\nendstream");
            s
        },
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    ];
    let bytes = datei(&objekte);
    load_from_bytes_with_limits(&bytes, &Limits::default())
        .expect("ein einzelner langer Strom ist keine Bombe und muss laden");
}
