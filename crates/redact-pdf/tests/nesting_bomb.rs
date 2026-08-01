//! Messung zu RUSTSEC-2026-0187 — tief verschachtelte PDF-Objektstrukturen.
//!
//! ## Warum dieser Test das Binary *nicht* fragt
//!
//! Ob `redact-rs` eine solche Datei ablehnt, prüfen die End-to-End-Tests in
//! `crates/redact-cli/tests/cli.rs`. Die messen aber die Vorprüfung
//! [`redact_pdf::document::prescan`] — sie sagen nichts darüber, ob `lopdf`
//! selbst noch abstürzt. Genau das ist hier die Frage, und deshalb geht dieser
//! Test **an der Vorprüfung vorbei** und ruft `lopdf::Document::load_mem`
//! direkt auf.
//!
//! ## Was ein Fehlschlag bedeutet
//!
//! Ein Stapelüberlauf ist kein Fehlerwert: der Prozess bricht mit SIGABRT ab.
//! Der Testläufer meldet das als abgestürzten Test. Dieser Test besteht also
//! genau dann, wenn `lopdf` einen Rückgabewert liefert — egal welchen.
//!
//! ## Messwerte
//!
//! Datei: 200 000 offene `[`, in allen drei Varianten, in denen sich
//! Verschachtelung unterbringen lässt.
//!
//! | Variante                         | lopdf 0.34        | lopdf 0.42            |
//! |----------------------------------|-------------------|-----------------------|
//! | gewöhnliches Objekt              | SIGABRT, Exit 134 | lädt, Objekt entfällt |
//! | Flate-komprimierter Objekt-Stream| SIGABRT, Exit 134 | lädt, Objekt entfällt |
//! | Seiteninhalt                     | SIGABRT, Exit 134 | sauberer Parse-Fehler |

use lopdf::Document;
use redact_pdf::document::{load_from_bytes, Limits};

/// So viele offene `[` wie in der ursprünglichen Meldung.
const BOMB: usize = 200_000;

// ---------------------------------------------------------------------------
// Testdateien
// ---------------------------------------------------------------------------

fn nest(n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n * 2);
    out.extend(std::iter::repeat_n(b'[', n));
    out.extend(std::iter::repeat_n(b']', n));
    out
}

/// Fügt Objekte zu einer Datei mit klassischer xref-Tabelle zusammen.
fn assemble(body: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (id, data) in body {
        offsets.push(out.len());
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", body.len() + 1).as_bytes());
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            body.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Die vier Objekte, die jede dieser Dateien zu einem gültigen PDF machen.
fn skeleton() -> Vec<(u32, Vec<u8>)> {
    vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R /Junk 5 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, b"<< /Length 5 >>\nstream\nBT ET\nendstream".to_vec()),
    ]
}

/// Variante 1: die Verschachtelung steht offen als gewöhnliches Objekt da.
fn plain(depth: usize) -> Vec<u8> {
    let mut body = skeleton();
    body.push((5, nest(depth)));
    assemble(&body)
}

/// Variante 2: dieselbe Verschachtelung, versteckt in einem
/// Flate-komprimierten Objekt-Stream. In den Rohbytes der Datei ist davon
/// nichts zu sehen.
fn object_stream(depth: usize) -> Vec<u8> {
    use lopdf::{dictionary, Stream};

    let mut payload = b"6 0 ".to_vec();
    payload.extend_from_slice(&nest(depth));
    payload.push(b' ');
    let mut stream = Stream::new(
        dictionary! { "Type" => "ObjStm", "N" => 1_i64, "First" => 4_i64 },
        payload,
    );
    stream.compress().expect("komprimierbar");

    let mut container = format!(
        "<< /Type /ObjStm /N 1 /First 4 /Filter /FlateDecode /Length {} >>\nstream\n",
        stream.content.len()
    )
    .into_bytes();
    container.extend_from_slice(&stream.content);
    container.extend_from_slice(b"\nendstream");

    let mut body = skeleton();
    body[0].1 = b"<< /Type /Catalog /Pages 2 0 R /Junk 6 0 R >>".to_vec();
    body.push((5, container));
    assemble(&body)
}

/// Variante 3: die Verschachtelung steht im Seiteninhalt — der wird nicht als
/// Objektgraph, sondern als Operatorenfolge geparst.
fn page_content(depth: usize) -> Vec<u8> {
    let mut content = b"BT ET\n".to_vec();
    content.extend_from_slice(&nest(depth));
    content.extend_from_slice(b" TJ\n");

    let mut object = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    object.extend_from_slice(&content);
    object.extend_from_slice(b"\nendstream");

    let mut body = skeleton();
    body[0].1 = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    body[3] = (4, object);
    assemble(&body)
}

fn variants(depth: usize) -> [(&'static str, Vec<u8>); 3] {
    [
        ("gewöhnliches Objekt", plain(depth)),
        ("Flate-komprimierter Objekt-Stream", object_stream(depth)),
        ("Seiteninhalt", page_content(depth)),
    ]
}

// ---------------------------------------------------------------------------
// Die Messung
// ---------------------------------------------------------------------------

/// Der Kern der Sache: `lopdf` **ohne** Vorprüfung, mit voller Bombe.
///
/// Bis 0.34 endete jede dieser drei Zeilen mit „fatal runtime error: stack
/// overflow, aborting“ und Exit 134. Dass dieser Test überhaupt bis zum Ende
/// läuft, ist das Ergebnis.
#[test]
fn lopdf_survives_all_three_bomb_variants_without_the_prescan() {
    for (name, bytes) in variants(BOMB) {
        // Der Rückgabewert ist gleichgültig — Hauptsache, es *gibt* einen.
        match Document::load_mem(&bytes) {
            Ok(doc) => {
                // Wenn geladen wird, dann ohne die Bombe: das zu tiefe Objekt
                // lässt `lopdf` fallen, und der Seiteninhalt lässt sich nicht
                // zu Operatoren dekodieren. Beides ist ein Ergebnis, kein
                // Absturz.
                for (_, page_id) in doc.get_pages() {
                    if let Ok(content) = doc.get_page_content(page_id) {
                        let _ = lopdf::content::Content::decode(&content);
                    }
                }
            }
            Err(_) => { /* sauberer Fehler, ebenfalls in Ordnung */ }
        }
        println!("{name}: kein Abbruch");
    }
}

/// Und die Vorprüfung fängt dieselben drei Dateien weiterhin vorher ab — mit
/// einer Meldung, die den Grund nennt.
#[test]
fn the_prescan_still_refuses_all_three_bomb_variants_with_a_message() {
    for (name, bytes) in variants(BOMB) {
        let err = load_from_bytes(&bytes).expect_err(name);
        assert!(
            err.to_string().contains("Verschachtelungstiefe"),
            "{name}: {err}"
        );
    }
}

/// Die Grenze der Vorprüfung ist kein gegriffener Wert, sondern genau das, was
/// `lopdf` noch vollständig einliest.
///
/// Eine Ebene mehr, und `lopdf` lässt das Objekt **kommentarlos** weg: das
/// Dokument lädt, die Referenz darauf zeigt ins Leere, und die geschriebene
/// Datei wäre unauffällig kaputt. Genau deshalb liegt die Grenze dort und
/// nicht höher.
#[test]
fn the_depth_limit_is_exactly_what_lopdf_still_parses() {
    let limit = Limits::default().max_nesting_depth;

    // Genau auf der Grenze: `lopdf` liest alle fünf Objekte ein …
    let ok = plain(limit);
    let doc = Document::load_mem(&ok).expect("Tiefe an der Grenze ist ladbar");
    assert!(
        doc.objects.contains_key(&(5, 0)),
        "lopdf verliert schon bei Tiefe {limit} ein Objekt — die Grenze in \
         Limits::max_nesting_depth ist zu hoch angesetzt"
    );
    // … und die Vorprüfung lässt sie durch.
    assert!(load_from_bytes(&ok).is_ok(), "Tiefe {limit} abgelehnt");

    // Eine Ebene darüber: `lopdf` schluckt das Objekt stillschweigend …
    let too_deep = plain(limit + 1);
    let doc = Document::load_mem(&too_deep).expect("lädt trotzdem");
    assert!(
        !doc.objects.contains_key(&(5, 0)),
        "lopdf parst inzwischen tiefer als {limit} — die Grenze in \
         Limits::max_nesting_depth darf mitwachsen"
    );
    // … und genau deshalb lehnt die Vorprüfung die Datei ab.
    let err = load_from_bytes(&too_deep).unwrap_err();
    assert!(
        err.to_string().contains("Verschachtelungstiefe"),
        "Tiefe {} nicht abgelehnt: {err}",
        limit + 1
    );
}
