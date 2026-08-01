//! Messungen zu Dateien, die aus wenigen Kilobyte Arbeitsspeicher machen.
//!
//! Gemeinsamer Nenner aller drei Fälle: die Datei hält **jede dokumentierte
//! Grenze** ein und bringt den Prozess trotzdem um. Gemessen wurde am
//! Release-Binary, Spitzenspeicher über `getrusage(RUSAGE_CHILDREN)`:
//!
//! | Datei | Größe | vorher | nachher |
//! |---|---|---|---|
//! | Content-Stream 64 MB, Dictionary mit `/Harmlos /Image` | 130 999 B | SIGABRT nach 10,6 s, 3 960 MB (ohne `ulimit`: SIGKILL, 15 406 MB) | Exit 1 nach 0,0 s, 22 MB |
//! | dieselbe Datei mit echtem `/Subtype /Image` | 130 999 B | SIGABRT nach 4,7 s, 3 961 MB | Exit 1 nach 0,0 s, 22 MB |
//! | 200 000 offene `[` im Seiteninhalt, `/Harmlos /Image` | 1 122 B | **Exit 0**, Geheimnis in der Ausgabe | Exit 1, 8 MB |
//! | Form-XObject-Fächerung n = 8 (2 097 152 Blätter) | 2 274 B | SIGABRT nach 21,7 s, 4 044 MB | Exit 1 nach 1,2 s, 282 MB |
//! | dieselbe Fächerung ganz ohne Text | 2 227 B | Exit 0 nach **43,8 s** | Exit 1 nach 5,1 s, 8 MB |
//!
//! Gegenprobe, unverändert angenommen: 40 Seiten mit je 50 platzierten
//! Form-XObjects (14 MB), 500 Seiten mit je 60 Buchungszeilen (135 MB), eine
//! Textzeile mit 8 000 Zeichen (11 MB).
//!
//! Warum die Tests keine Bytes messen: Spitzenspeicher lässt sich in einem
//! Testlauf nicht verlässlich beobachten (der Testläufer teilt sich den
//! Prozess mit allen anderen Tests). Gemessen wird deshalb das, was sich
//! messen lässt — angenommen oder abgelehnt, und wie viele Interpretationen
//! dabei entstanden sind.

mod common;

use lopdf::Document;
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, scan_page, PdfExtractor, PdfRedactor};

/// Das Geheimnis, das in jeder dieser Dateien steht.
const SECRET: &str = "532013000";

// ---------------------------------------------------------------------------
// Dateibau — von Hand, damit das Stream-Dictionary frei beschriftbar ist
// ---------------------------------------------------------------------------

/// Fügt Objekte zu einer Datei mit klassischer xref-Tabelle zusammen.
fn assemble(body: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (id, data) in body {
        offsets.push((*id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendobj\n");
    }
    let max_id = body.iter().map(|(id, _)| *id).max().unwrap_or(0);
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max_id + 1).as_bytes());
    for id in 1..=max_id {
        let offset = offsets
            .iter()
            .find(|(other, _)| *other == id)
            .map(|(_, o)| *o)
            .unwrap_or(0);
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            max_id + 1
        )
        .as_bytes(),
    );
    out
}

fn deflate(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use std::io::Write;
    let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(data).expect("komprimierbar");
    encoder.finish().expect("komprimierbar")
}

/// Ein Flate-Stream mit frei wählbaren Zusatzschlüsseln im Dictionary.
fn stream(extra_dict: &str, payload: &[u8]) -> Vec<u8> {
    packed_stream(extra_dict, &deflate(payload))
}

/// Wie [`stream`], aber mit bereits komprimierter Nutzlast — damit dieselben
/// 64 MB nicht neunmal durch den Kompressor müssen.
fn packed_stream(extra_dict: &str, packed: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "<< /Length {} /Filter /FlateDecode{extra_dict} >>\nstream\n",
        packed.len()
    )
    .into_bytes();
    out.extend_from_slice(packed);
    out.extend_from_slice(b"\nendstream");
    out
}

const FONT: &[u8] =
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";

/// Eine einseitige Datei; `extra` sind weitere Objekte (etwa Form-XObjects).
fn one_page(
    content: &[u8],
    extra_dict: &str,
    resources: &str,
    extra: &[(u32, Vec<u8>)],
) -> Vec<u8> {
    one_page_packed(&deflate(content), extra_dict, resources, extra)
}

fn one_page_packed(
    packed_content: &[u8],
    extra_dict: &str,
    resources: &str,
    extra: &[(u32, Vec<u8>)],
) -> Vec<u8> {
    let mut body: Vec<(u32, Vec<u8>)> = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
              /Contents 4 0 R /Resources 5 0 R >>"
                .to_vec(),
        ),
        (4, packed_stream(extra_dict, packed_content)),
        (5, resources.as_bytes().to_vec()),
        (6, FONT.to_vec()),
    ];
    body.extend(extra.iter().cloned());
    body.sort_by_key(|(id, _)| *id);
    assemble(&body)
}

fn secret_text() -> Vec<u8> {
    format!("BT /F1 12 Tf 72 700 Td (Kontonummer: {SECRET}) Tj ET\n").into_bytes()
}

/// Die komprimierte Nutzlast eines Seiteninhalts, der sich auf `megabytes`
/// aufbläht — die Dekompressionsbombe.
fn inflating_payload(megabytes: usize) -> Vec<u8> {
    let mut content = secret_text();
    let unit = b"0 0 0 rg\n";
    while content.len() < megabytes * 1024 * 1024 {
        content.extend_from_slice(unit);
    }
    deflate(&content)
}

fn inflating_page(megabytes: usize, extra_dict: &str) -> Vec<u8> {
    one_page_packed(
        &inflating_payload(megabytes),
        extra_dict,
        "<< /Font << /F1 6 0 R >> >>",
        &[],
    )
}

/// Seiteninhalt mit `depth` offenen `[` — die Verschachtelungsbombe.
fn nesting_page(depth: usize, extra_dict: &str) -> Vec<u8> {
    let mut content = secret_text();
    content.extend(std::iter::repeat_n(b'[', depth));
    content.extend(std::iter::repeat_n(b']', depth));
    content.push(b'\n');
    one_page(&content, extra_dict, "<< /Font << /F1 6 0 R >> >>", &[])
}

/// Sieben Ebenen Form-XObjects, jede zeichnet die nächste `fan`-mal.
///
/// Ergibt `fan^7` Durchläufe durch das unterste Formular — bei `fan = 8` über
/// zwei Millionen, aus gut zwei Kilobyte Datei. Jede dokumentierte Grenze ist
/// dabei eingehalten: neun Ströme, Verschachtelungstiefe 1, rund 100 Byte
/// entpackt je Strom.
fn form_fanout(fan: usize, leaf_draws_text: bool) -> Vec<u8> {
    const LEVELS: usize = 7;
    let mut extra = Vec::new();
    let mut xobjects = String::new();
    for level in 0..=LEVELS {
        xobjects.push_str(&format!("/X{level} {} 0 R ", 10 + level));
    }
    let resources = format!("<< /Font << /F1 6 0 R >> /XObject << {xobjects}>> >>");
    let form_dict = " /Type /XObject /Subtype /Form /BBox [0 0 595 842] /Resources 5 0 R";
    for level in 0..LEVELS {
        let payload = format!("/X{} Do\n", level + 1).repeat(fan);
        extra.push((10 + level as u32, stream(form_dict, payload.as_bytes())));
    }
    let leaf = if leaf_draws_text {
        secret_text()
    } else {
        b"0 0 0 rg\n".to_vec()
    };
    extra.push((10 + LEVELS as u32, stream(form_dict, &leaf)));
    one_page(b"/X0 Do\n", "", &resources, &extra)
}

// ---------------------------------------------------------------------------
// Werkzeug
// ---------------------------------------------------------------------------

fn error_of(bytes: &[u8]) -> String {
    match load_from_bytes(bytes) {
        Ok(doc) => {
            // Durchgekommen: dann muss wenigstens die Extraktion aussteigen.
            match PdfExtractor::new().extract_with_warnings(&doc) {
                Ok((runs, warnings)) => panic!(
                    "angenommen statt abgelehnt: {} Zeilen, Warnungen {warnings:?}",
                    runs.len()
                ),
                Err(e) => e.to_string(),
            }
        }
        Err(e) => e.to_string(),
    }
}

fn page_id(doc: &Document) -> lopdf::ObjectId {
    *doc.get_pages().values().next().expect("eine Seite")
}

// ---------------------------------------------------------------------------
// Befund 1 — das Stream-Dictionary entscheidet nichts mehr
// ---------------------------------------------------------------------------

/// Die acht Beschriftungen, mit denen sich früher beide Vorprüfungen
/// abschalten ließen — plus die echte, die es nie hätte tun dürfen.
const DISGUISES: &[&str] = &[
    "",
    " /Harmlos /Image",
    " /Subtype /Image",
    " /Length1 4711",
    " /Type /Metadata",
    " /Type /XRef",
    " /Subtype /Type1C",
    " /Subtype /OpenType",
    " /Type /EmbeddedFile",
];

/// Ein `/Image` im Dictionary hebt das enge Parse-Budget nicht mehr auf.
///
/// Vorher: mit `/Harmlos /Image` zählte der 64-MB-Seiteninhalt gegen das
/// 1-GB-Budget statt gegen das 16-MB-Budget, kam durch und riss den Prozess
/// mit — SIGABRT bei 3 960 MB unter `ulimit -v 6G`, ohne Begrenzung SIGKILL
/// durch den systemweiten OOM-Killer bei 15 406 MB.
#[test]
fn no_key_in_the_dictionary_buys_a_larger_stream_budget() {
    let packed = inflating_payload(64);
    for disguise in DISGUISES {
        let bytes = one_page_packed(&packed, disguise, "<< /Font << /F1 6 0 R >> >>", &[]);
        assert!(
            bytes.len() < 300 * 1024,
            "Testdatei ist keine Bombe: {} Byte",
            bytes.len()
        );
        let error = error_of(&bytes);
        assert!(
            error.contains("zu parsenden Streams"),
            "Dictionary „{disguise}“ wurde anders behandelt: {error}"
        );
    }
}

/// Dasselbe für die Tiefenprüfung.
///
/// Der Kommentar an `is_image` berief sich darauf, `lopdf` packe Streams mit
/// `/Subtype /Image` nicht aus. Das galt für `lopdf` 0.34; seit 0.36 prüft
/// `Stream::decompressed_content` den `/Subtype` nicht mehr, und
/// `Document::get_page_content` liest einen so beschrifteten Seiteninhalt ganz
/// normal. Ein *richtig* geschriebenes `/Subtype /Image` hätte die Lücke also
/// genauso geöffnet wie das erfundene `/Harmlos /Image` — deshalb steht hier
/// beides in derselben Liste.
#[test]
fn no_key_in_the_dictionary_switches_off_the_depth_check() {
    for disguise in DISGUISES {
        let bytes = nesting_page(200_000, disguise);
        assert!(
            bytes.len() < 8 * 1024,
            "Testdatei ist keine Bombe: {} Byte",
            bytes.len()
        );
        let error = error_of(&bytes);
        assert!(
            error.contains("Verschachtelungstiefe"),
            "Dictionary „{disguise}“ wurde anders behandelt: {error}"
        );
    }
}

/// Kalibrierung: in der Eingabe steht das Geheimnis wirklich.
///
/// Ohne diese Probe bewiese die Ablehnung oben nichts — eine Datei ohne
/// Geheimnis abzulehnen ist keine Leistung.
#[test]
fn the_bomb_really_carries_the_secret() {
    for bytes in [
        nesting_page(200_000, " /Subtype /Image"),
        inflating_page(1, ""),
    ] {
        assert!(
            !leaks(&bytes, SECRET).is_empty(),
            "die Testdatei enthält das Geheimnis gar nicht"
        );
    }
}

// ---------------------------------------------------------------------------
// Befund 1b — eine nicht durchsuchte Seite darf kein Erfolg sein
// ---------------------------------------------------------------------------

/// Ein Seiteninhalt, der sich nicht in Operationen zerlegen lässt, beendet den
/// Lauf mit einem Fehler statt mit einer Warnung.
///
/// Vorher lief genau diese Datei durch: „Schwärzungen: 0“, Rückgabewert 0, die
/// Warnung nur auf stderr — und `leaks` fand die Kontonummer in der Ausgabe an
/// vier Stellen. Im Stapelbetrieb zählte die Datei als verarbeitet.
#[test]
fn a_page_whose_content_cannot_be_decoded_ends_the_run() {
    // Ein Seiteninhalt, den die Vorprüfung durchlässt und `lopdf` trotzdem
    // fallen lässt. Der Weg dorthin ist der Rest der Lücke aus Befund 1:
    // Verschachtelungstiefe 150 liegt über der Grenze für Syntax (100), aber
    // unter der für binär aussehende Nutzlast (256) — und binär aussehen lässt
    // sich ein Content-Stream mit ein paar tausend Nullbytes am Ende. `lopdf`
    // liest ihn gleichwohl als Seiteninhalt und liefert **keine** Operation.
    let mut content = secret_text();
    content.extend(std::iter::repeat_n(b'[', 150));
    content.extend(std::iter::repeat_n(b']', 150));
    content.extend(std::iter::repeat_n(0u8, 4000));
    let bytes = one_page(&content, "", "<< /Font << /F1 6 0 R >> >>", &[]);
    let doc = load_from_bytes(&bytes).expect("die Vorprüfung hat hier nichts zu beanstanden");
    let error = scan_page(&doc, page_id(&doc)).expect_err("nicht zerlegbar, also Fehler");
    assert!(
        error.to_string().contains("nicht in Operationen zerlegen"),
        "unerwartete Begründung: {error}"
    );

    // Und der Weg, den die Kommandozeile nimmt, meldet denselben Fehler.
    let error = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect_err("die Extraktion darf hier nicht Ok liefern");
    assert!(
        error.to_string().contains("nicht in Operationen zerlegen"),
        "unerwartete Begründung: {error}"
    );

    // Kalibrierung: die Seite trägt das Geheimnis wirklich, und es wäre bei
    // Rückgabewert 0 unangetastet in der Ausgabe gelandet.
    assert!(!leaks(&bytes, SECRET).is_empty());
}

// ---------------------------------------------------------------------------
// Befund 2 — Fächerung statt Tiefe
// ---------------------------------------------------------------------------

/// Eine schmale Fächerung bleibt erlaubt — und wird vollständig ausgewertet.
///
/// Die Zahl der Textdatensätze ist das messbare Gegenstück zum
/// Spitzenspeicher: 3⁷ = 2 187 Durchläufe durch das unterste Formular, jeder
/// mit einer Textoperation.
#[test]
fn a_narrow_fanout_is_still_read_completely() {
    let bytes = form_fanout(3, true);
    let doc = load_from_bytes(&bytes).expect("ladbar");
    let scan = scan_page(&doc, page_id(&doc)).expect("angenommen");
    assert_eq!(
        scan.shows.len(),
        3usize.pow(7),
        "nicht jede Platzierung wurde ausgewertet"
    );
}

/// Ab einer gewissen Breite wird abgelehnt — mit Begründung, nicht mit SIGKILL.
///
/// Gemessen am Release-Binary: n = 8 belegte vorher 4 044 MB und endete unter
/// `ulimit -v 4G` mit SIGABRT, ohne Begrenzung mit SIGKILL durch den
/// systemweiten OOM-Killer bei 15,6 GB.
#[test]
fn a_wide_fanout_is_refused_instead_of_exhausting_the_machine() {
    // 5 ist die schmalste Breite, die das Konto reißt (5⁷ = 78 125 Blätter);
    // 8 ist die gemessene Bombe.
    for fan in [5, 8] {
        let bytes = form_fanout(fan, true);
        assert!(
            bytes.len() < 4 * 1024,
            "Testdatei ist keine Bombe: {} Byte",
            bytes.len()
        );
        let error = error_of(&bytes);
        assert!(
            error.contains("Zeichen"),
            "n = {fan} wurde anders abgelehnt: {error}"
        );
    }
}

/// Auch ohne einen einzigen Buchstaben: die Vervielfachung selbst ist der
/// Angriff.
///
/// Vorher lief diese Datei mit Rückgabewert 0 durch und beschäftigte den
/// Rechner 43,8 Sekunden — aus 2 227 Byte.
#[test]
fn a_fanout_without_any_text_is_refused_too() {
    let error = error_of(&form_fanout(8, false));
    assert!(
        error.contains("vervielfacht"),
        "unerwartete Begründung: {error}"
    );
}

/// Das Aufwandskonto wächst mit dem Inhalt, den die Datei mitbringt.
///
/// Sonst stünde es quer zu `--max-parsed-mb`: wer das Budget für geparste
/// Streams anhebt, will einen großen Seiteninhalt verarbeiten, keine
/// Fächerung. Ein einzelner Strom mit sehr vielen Operationen muss deshalb
/// durchlaufen, während dieselbe Zahl von *Durchläufen* durch einen kurzen
/// Strom abgelehnt wird.
#[test]
fn a_long_honest_content_stream_is_not_mistaken_for_a_fanout() {
    let mut content = secret_text();
    // Deutlich mehr Operationen, als eine abgelehnte Fächerung auswertet.
    content.extend(b"0 0 0 rg\n".repeat(1_500_000).iter());
    let bytes = one_page(&content, "", "<< /Font << /F1 6 0 R >> >>", &[]);
    let doc = load_from_bytes(&bytes).expect("innerhalb des Parse-Budgets");
    let scan = scan_page(&doc, page_id(&doc)).expect("ein langer Strom ist keine Bombe");
    assert_eq!(scan.shows.len(), 1);
}

// ---------------------------------------------------------------------------
// Gegenprobe — echte Dokumente
// ---------------------------------------------------------------------------

/// 40 Seiten mit je 50 platzierten Form-XObjects laufen unverändert durch.
///
/// Das ist die Bauform, die die Fächerung missbraucht: viele `Do` auf wenige
/// Ströme. Ein Formularsatz, ein Tabellenraster, ein Briefkopf auf jeder Seite
/// sehen genauso aus — und müssen weiterhin verarbeitet werden.
#[test]
fn a_real_document_with_many_placed_forms_still_runs() {
    let mut extra = Vec::new();
    let form_dict = " /Type /XObject /Subtype /Form /BBox [0 0 595 842] /Resources 5 0 R";
    let mut xobjects = String::new();
    for i in 0..20u32 {
        let text = format!(
            "BT /F1 9 Tf 10 20 Td (Posten {i}  DE89 3704 0044 {SECRET} 00  1.234,56 EUR) Tj ET\n"
        );
        extra.push((100 + i, stream(form_dict, text.as_bytes())));
        xobjects.push_str(&format!("/F{i} {} 0 R ", 100 + i));
    }
    let resources = format!("<< /Font << /F1 6 0 R >> /XObject << {xobjects}>> >>");

    let mut content = Vec::new();
    for j in 0..50 {
        content.extend_from_slice(
            format!(
                "q 1 0 0 1 20 {} cm /F{} Do Q\n",
                700 - (j % 30) * 20,
                j % 20
            )
            .as_bytes(),
        );
    }
    let bytes = one_page(&content, "", &resources, &extra);

    let doc = load_from_bytes(&bytes).expect("ladbar");
    let scan = scan_page(&doc, page_id(&doc)).expect("ein Formularsatz ist keine Bombe");
    assert_eq!(scan.shows.len(), 50, "nicht jede Platzierung wurde gelesen");
}

/// Und eine Seite mit sehr viel ehrlichem Text ebenso — samt Nachweis über
/// [`leaks`], dass die Schwärzung danach wirklich greift.
#[test]
fn a_dense_page_is_processed_and_the_secret_really_disappears() {
    let mut content = b"BT /F1 9 Tf\n".to_vec();
    for line in 0..60 {
        content.extend_from_slice(
            format!(
                "1 0 0 1 40 {} Tm (Buchung {line}  Kontonummer: {SECRET}  1.234,56 EUR) Tj\n",
                800 - line * 13
            )
            .as_bytes(),
        );
    }
    content.extend_from_slice(b"ET\n");
    let bytes = one_page(&content, "", "<< /Font << /F1 6 0 R >> >>", &[]);

    let mut doc = load_from_bytes(&bytes).expect("ladbar");
    let runs = PdfExtractor::new().extract(&doc).expect("Extraktion");
    assert_eq!(runs.len(), 60, "nicht jede Zeile wurde gelesen");
    assert!(!leaks(&bytes, SECRET).is_empty(), "Kalibrierung");

    // Die ganze Textfläche schwärzen — danach darf das Geheimnis nirgends mehr
    // in der Datei stehen, auf keiner Ebene.
    let redaction = Redaction::new(
        Region::new(
            0,
            Rect::new(0.0, 0.0, 595.0, 842.0),
            None,
            Source::Manual {
                reason: "Messung".into(),
            },
        ),
        Action::Blackout,
    );
    PdfRedactor::new()
        .apply_with_report(&mut doc, &[redaction])
        .expect("Schwärzung");
    let out = save_to_bytes(&doc).expect("speicherbar");
    assert!(
        leaks(&out, SECRET).is_empty(),
        "das Geheimnis steht noch in der Ausgabe: {:?}",
        leaks(&out, SECRET)
    );
}
