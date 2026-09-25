//! Gegenprüfung Q3 (Fix-Runde 5): die Decke über den Formularplatzierungen.
//!
//! Messungen; jeder Test hier ist `#[ignore]`, weil er Zeit und Speicher misst.
//! Lauf: `cargo test -p redact-pdf --test zf_q3_kombinatorik -- --ignored --nocapture`

mod common;

use common::{page, text_ops, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_pdf::{load_from_bytes, scan_page, PdfExtractor};

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn add_form(d: &mut Doc, holder: ObjectId, name: &str, body: &str) -> (ObjectId, ObjectId) {
    let font_id = d.font_id;
    let form_resources = d.add(Object::Dictionary(
        dictionary! { "Font" => dictionary! { "F1" => font_id } },
    ));
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                "Resources" => form_resources,
            },
            body.as_bytes().to_vec(),
        )
        .with_compression(false),
    ));
    let target = d.doc.get_dictionary_mut(holder).expect("Ressourcen");
    let mut xobjects = target
        .get(b"XObject")
        .and_then(|o| o.as_dict())
        .cloned()
        .unwrap_or_default();
    xobjects.set(name, form_id);
    target.set("XObject", xobjects);
    (form_id, form_resources)
}

fn text_at(y: i32, text: &str) -> String {
    format!("BT /F1 10 Tf 72 {y} Td ({}) Tj ET\n", escape(text))
}

fn analyse(bytes: &[u8]) -> Result<(usize, Vec<String>), String> {
    let doc = load_from_bytes(bytes).map_err(|e| e.to_string())?;
    let (runs, warnings) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .map_err(|e| e.to_string())?;
    Ok((runs.len(), warnings))
}

fn hwm() -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmHWM"))
        .unwrap_or("VmHWM: ?")
        .trim()
        .to_string()
}

// ---------------------------------------------------------------------------
// A) Kette: jedes Formular zeichnet das nächste zweimal
// ---------------------------------------------------------------------------

/// `Fm0` (Text) ← `Fm1` zeichnet `Fm0` zweimal ← `Fm2` zeichnet `Fm1` zweimal …
/// Die Seite zeichnet das oberste unter einem Spiegel.
fn doubling_chain(levels: usize) -> Vec<u8> {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (mut inner, mut inner_res) = add_form(&mut d, resources, "Fm0", &text_at(600, "A"));
    for level in 1..=levels {
        let body = format!("q /Fm{} Do Q\nq /Fm{} Do Q\n", level - 1, level - 1);
        let (outer, outer_res) = add_form(&mut d, resources, &format!("Fm{level}"), &body);
        let target = d.doc.get_dictionary_mut(outer_res).expect("Ressourcen");
        target.set(
            "XObject",
            dictionary! { format!("Fm{}", level - 1) => inner },
        );
        inner = outer;
        inner_res = outer_res;
    }
    let _ = (inner, inner_res);
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!("/Span <</ActualText (Alpha)>> BDC\nq /Fm{levels} Do Q\nEMC\n").as_bytes(),
    );
    d.set_content(&raw);
    d.finish()
}

#[test]
#[ignore = "Messung"]
fn mess_verdopplungskette() {
    for levels in [4usize, 8, 10, 16, 20] {
        let bytes = doubling_chain(levels);
        let start = std::time::Instant::now();
        let out = analyse(&bytes);
        let (runs, warnings) = match out {
            Ok(v) => v,
            Err(e) => {
                println!("{levels} Ebenen ({} B): abgelehnt: {e}", bytes.len());
                continue;
            }
        };
        println!(
            "{levels} Ebenen ({} B): {:?}, {runs} Läufe, {} Warnung(en), {}",
            bytes.len(),
            start.elapsed(),
            warnings.len(),
            hwm()
        );
        for w in warnings.iter().take(2) {
            println!("    → {w}");
        }
    }
}

// ---------------------------------------------------------------------------
// B) Verschachtelte BDC-Klammern über denselben `Do`
// ---------------------------------------------------------------------------

/// `brackets` verschachtelte Spiegel-Klammern, darin `dos` Platzierungen
/// desselben Formulars. Jede Klammer bringt **alle** `Do` in ihren
/// Datensatz — das sind `brackets × dos` Einträge, und die entstehen in
/// `scan_marked_text`, also vor jedem Aufwandskonto und unter keiner Decke.
fn nested_brackets(brackets: usize, dos: usize, nested_form: bool) -> Vec<u8> {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, "A"));
    if nested_form {
        let (_, outer_res) = add_form(&mut d, resources, "Fm0", "q /Fm1 Do Q\n");
        d.doc
            .get_dictionary_mut(outer_res)
            .expect("Ressourcen")
            .set("XObject", dictionary! { "Fm1" => inner });
    }
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    for i in 0..brackets {
        raw.extend_from_slice(format!("/Span <</ActualText (A{i})>> BDC\n").as_bytes());
    }
    raw.extend_from_slice("/Fm1 Do\n".repeat(dos).as_bytes());
    for _ in 0..brackets {
        raw.extend_from_slice(b"EMC\n");
    }
    d.set_content(&raw);
    d.finish()
}

#[test]
#[ignore = "Messung"]
fn mess_verschachtelte_klammern() {
    for (brackets, dos, nested) in [
        (100usize, 100usize, false),
        (500, 500, false),
        (1000, 1000, false),
        (2000, 2000, false),
        (2000, 2000, true),
        (3000, 3000, false),
    ] {
        let bytes = nested_brackets(brackets, dos, nested);
        let start = std::time::Instant::now();
        let out = analyse(&bytes);
        match out {
            Ok((runs, warnings)) => println!(
                "{brackets} Klammern × {dos} Do ({} B, verschachteltes Formular {nested}): \
                 {:?}, {runs} Läufe, {} Warnung(en), {}",
                bytes.len(),
                start.elapsed(),
                warnings.len(),
                hwm()
            ),
            Err(e) => println!(
                "{brackets} × {dos} ({} B): nach {:?} abgelehnt: {e} — {}",
                bytes.len(),
                start.elapsed(),
                hwm()
            ),
        }
    }
}

/// Dasselbe ohne Spiegel in den Klammern: dann entsteht kein Datensatz, und
/// dieselbe Datei ist harmlos. Der Unterschied liegt allein am Spiegel.
#[test]
#[ignore = "Messung"]
fn mess_verschachtelte_klammern_ohne_spiegel() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    add_form(&mut d, resources, "Fm1", &text_at(600, "A"));
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    for i in 0..2000 {
        raw.extend_from_slice(format!("/Span <</MCID {i}>> BDC\n").as_bytes());
    }
    raw.extend_from_slice("/Fm1 Do\n".repeat(2000).as_bytes());
    for _ in 0..2000 {
        raw.extend_from_slice(b"EMC\n");
    }
    d.set_content(&raw);
    let bytes = d.finish();
    let start = std::time::Instant::now();
    let out = analyse(&bytes);
    println!(
        "2000 × 2000 ohne Spiegel ({} B): {:?}, {:?}, {}",
        bytes.len(),
        start.elapsed(),
        out.map(|(r, w)| (r, w.len())),
        hwm()
    );
}

// ---------------------------------------------------------------------------
// C) Der Zyklus und der Diamant — nur, dass es endet
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Messung"]
fn mess_breite_faecherung() {
    // Sechs Ebenen, jede zeichnet die nächste fünfmal: 5^6 = 15 625.
    for (levels, width) in [(6usize, 5usize), (8, 5), (8, 8)] {
        let mut d = page(&[]);
        let resources = d.resources_id;
        let (mut inner, _) = add_form(&mut d, resources, "Fm0", &text_at(600, "A"));
        for level in 1..=levels {
            let body = format!("q /Fm{} Do Q\n", level - 1).repeat(width);
            let (outer, outer_res) = add_form(&mut d, resources, &format!("Fm{level}"), &body);
            d.doc
                .get_dictionary_mut(outer_res)
                .expect("Ressourcen")
                .set(
                    "XObject",
                    dictionary! { format!("Fm{}", level - 1) => inner },
                );
            inner = outer;
        }
        let mut raw = text_ops(&["Kontoinhaber"]);
        raw.extend_from_slice(
            format!("/Span <</ActualText (Alpha)>> BDC\nq /Fm{levels} Do Q\nEMC\n").as_bytes(),
        );
        d.set_content(&raw);
        let bytes = d.finish();
        let start = std::time::Instant::now();
        let out = analyse(&bytes);
        println!(
            "{levels} Ebenen × {width} breit ({} B): {:?}, {}, {}",
            bytes.len(),
            start.elapsed(),
            match &out {
                Ok((r, w)) => format!("{r} Läufe, {} Warnung(en)", w.len()),
                Err(e) => format!("abgelehnt: {e}"),
            },
            hwm()
        );
    }
    let _ = SECRET;
}

// ---------------------------------------------------------------------------
// D) Ein Fall je Prozess — nur so ist VmHWM die Spitze *dieses* Falls
// ---------------------------------------------------------------------------

/// `Q3_B` Klammern × `Q3_D` `Do`, `Q3_NESTED=1` für ein verschachteltes
/// Formular (damit `close_forms` überhaupt läuft).
#[test]
#[ignore = "Messung"]
fn mess_ein_fall() {
    let b: usize = std::env::var("Q3_B")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let d: usize = std::env::var("Q3_D")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let nested = std::env::var("Q3_NESTED").is_ok();
    let bytes = nested_brackets(b, d, nested);
    println!("Datei {} B, {} Objekte", bytes.len(), {
        let doc = load_from_bytes(&bytes).expect("ladbar");
        doc.objects.len()
    });
    let start = std::time::Instant::now();
    match analyse(&bytes) {
        Ok((runs, warnings)) => {
            println!(
                "{b} × {d} (nested {nested}): {:?}, {runs} Läufe, {} Warnung(en), {}",
                start.elapsed(),
                warnings.len(),
                hwm()
            );
            for w in &warnings {
                println!("    → {w}");
            }
        }
        Err(e) => println!(
            "{b} × {d}: abgelehnt nach {:?}: {e} [{}]",
            start.elapsed(),
            hwm()
        ),
    }
}

// ---------------------------------------------------------------------------
// E) Die Decke greift — und der Speicher ist längst da
// ---------------------------------------------------------------------------

/// Wie `nested_brackets`, aber die Klammern stehen über dem **äußeren**
/// Formular: jede Platzierung klappt sich in der Schließung noch einmal auf.
/// Damit läuft `MAX_MIRROR_FORM_PLACEMENTS` an — die Grundmenge
/// (`Klammern × Do`) hat den Speicher aber schon belegt, bevor die Decke
/// überhaupt gefragt wird.
fn nested_brackets_over_outer(brackets: usize, dos: usize) -> Vec<u8> {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, "A"));
    let (_, outer_res) = add_form(&mut d, resources, "Fm0", "q /Fm1 Do Q\n");
    d.doc
        .get_dictionary_mut(outer_res)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Fm1" => inner });
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    for i in 0..brackets {
        raw.extend_from_slice(format!("/Span <</ActualText (A{i})>> BDC\n").as_bytes());
    }
    raw.extend_from_slice("/Fm0 Do\n".repeat(dos).as_bytes());
    for _ in 0..brackets {
        raw.extend_from_slice(b"EMC\n");
    }
    d.set_content(&raw);
    d.finish()
}

#[test]
#[ignore = "Messung"]
fn mess_decke_greift_zu_spaet() {
    let b: usize = std::env::var("Q3_B")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let d: usize = std::env::var("Q3_D")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let bytes = nested_brackets_over_outer(b, d);
    println!("Datei {} B", bytes.len());
    let start = std::time::Instant::now();
    match analyse(&bytes) {
        Ok((runs, warnings)) => {
            println!(
                "{b} × {d} über dem äußeren Formular: {:?}, {runs} Läufe, {} Warnung(en), {}",
                start.elapsed(),
                warnings.len(),
                hwm()
            );
            for w in &warnings {
                println!("    → {}", &w[..w.len().min(200)]);
            }
        }
        Err(e) => println!(
            "{b} × {d}: abgelehnt nach {:?}: {e} [{}]",
            start.elapsed(),
            hwm()
        ),
    }
}

/// Schreibt das Material für den Lauf über die Kommandozeile — nur mit `Q3_OUT`.
#[test]
#[ignore = "Material"]
fn schreibt_material() {
    let Ok(dir) = std::env::var("Q3_OUT") else {
        return;
    };
    let b: usize = std::env::var("Q3_B")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);
    let d: usize = std::env::var("Q3_D")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);
    let bytes = nested_brackets_over_outer(b, d);
    std::fs::write(format!("{dir}/faecher_{b}x{d}.pdf"), &bytes).expect("schreibbar");
    println!("{dir}/faecher_{b}x{d}.pdf: {} B", bytes.len());
}

// ---------------------------------------------------------------------------
// F) Die Decke zählt jetzt schon beim Aufbau (Fix-Runde 6)
// ---------------------------------------------------------------------------

/// Wie viele Spiegel-Formular-Paare der Scan dieser Seite wirklich führt.
///
/// Das ist die Größe, die den Speicher belegt: `Σ|record.forms|`. Sie ist über
/// `scan_page` von außen ablesbar und braucht weder Uhr noch Speichermesser —
/// eine Messung wäre auf einer geteilten Maschine kein Beleg.
fn spiegel_formular_paare(bytes: &[u8]) -> usize {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    let page_id = doc.page_iter().next().expect("eine Seite");
    let scan = scan_page(&doc, page_id).expect("Scan");
    scan.marked.iter().map(|record| record.forms.len()).sum()
}

/// Die Decke aus `content::MAX_MIRROR_FORM_PLACEMENTS`.
const DECKE: usize = 100_000;

/// **Befund Q3-1a (Dienstverweigerung), in Fix-Runde 6 behoben.**
///
/// `scan_marked_text` läuft als erste Zeile von `scan_operations`, also **vor**
/// der Schleife mit dem Aufwandskonto. Jede Spiegel-Klammer nahm dort jedes
/// `Do` ihres Bereichs auf: `B` verschachtelte Klammern über `D` Platzierungen
/// ergaben `B × D` Einträge, gegen keine Decke und ohne jede Buchung.
///
/// Gemessen (Debug, je eigener Prozess, `mess_ein_fall`):
///
/// | Datei | vorher | nachher |
/// |---|---|---|
/// | 92 kB (2 000 × 2 000)  | 4,71 s / 268 MB   | 0,26 s / 22 MB |
/// | 184 kB (4 000 × 4 000) | 20,6 s / 1 036 MB | 0,41 s / 30 MB |
/// | 276 kB (6 000 × 6 000) | 41,7 s / 2 306 MB | 0,56 s / 38 MB |
///
/// Hier steht nicht die Zeit, sondern die Zahl, an der sie hing: 4 000 000
/// Paare vorher, höchstens `DECKE` nachher.
#[test]
fn verschachtelte_klammern_bleiben_unter_der_decke() {
    let paare = spiegel_formular_paare(&nested_brackets(2000, 2000, false));
    assert!(
        paare <= DECKE,
        "{paare} Spiegel-Formular-Paare aus einer Datei von 92 kB"
    );
}

/// Und die Decke ist auch wirklich erreicht — sonst bewiese der Test oben
/// nichts über eine Datei, die sie erreichen *will*.
#[test]
fn verschachtelte_klammern_erreichen_die_decke_und_sagen_es() {
    let bytes = nested_brackets(2000, 2000, false);
    assert_eq!(spiegel_formular_paare(&bytes), DECKE);
    let (_, warnings) = analyse(&bytes).expect("lesbar");
    let hits: Vec<&String> = warnings
        .iter()
        .filter(|w| w.contains("Zuordnungen zwischen einem Spiegel"))
        .collect();
    assert_eq!(hits.len(), 1, "{warnings:?}");
    assert!(hits[0].contains("unvollständig"), "{}", hits[0]);
}

/// Die Gegenrichtung: eine gewöhnliche getaggte Seite (jede Klammer über ihrem
/// eigenen `Do`, keine Verschachtelung) bleibt weit unter der Decke und gibt
/// **keine** Warnung. Eine Decke, die gewöhnliche Dateien ablehnt, wäre
/// genauso ein Fehler wie eine Lücke.
#[test]
fn eine_klammer_je_platzierung_gibt_keine_warnung() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    add_form(&mut d, resources, "Fm1", &text_at(600, "A"));
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    for i in 0..2000 {
        raw.extend_from_slice(
            format!("/Span <</ActualText (A{i})>> BDC\n/Fm1 Do\nEMC\n").as_bytes(),
        );
    }
    d.set_content(&raw);
    let bytes = d.finish();
    assert_eq!(spiegel_formular_paare(&bytes), 2000);
    let (_, warnings) = analyse(&bytes).expect("lesbar");
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("Zuordnungen zwischen einem Spiegel")),
        "{warnings:?}"
    );
}
