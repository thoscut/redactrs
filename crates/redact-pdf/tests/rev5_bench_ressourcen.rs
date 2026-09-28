//! Messreihe zum geteilten `/Resources`-Verzeichnis — **nicht** Teil des Gates.
//!
//! Alle Tests hier sind `#[ignore]`: sie messen Zeit und Spitzenspeicher und
//! flattern deshalb mit der Fremdlast der Maschine. Gemessen wird in **zwei**
//! Prozessen, sonst zählt der Spitzenspeicher des Bauens in die Messung des
//! Scannens hinein:
//!
//! ```text
//! MESS_N=5000 MESS_JUNK=16384 MESS_OUT=/tmp/f.pdf \
//!   cargo test --release -p redact-pdf --test rev5_bench_ressourcen -- --ignored --nocapture bauen
//! MESS_IN=/tmp/f.pdf \
//!   cargo test --release -p redact-pdf --test rev5_bench_ressourcen -- --ignored --nocapture messen
//! ```
//!
//! `VmHWM` ist ein Höchststand des **ganzen Prozesses** und fällt nie wieder;
//! deshalb ein Fall je Prozess.

use redact_pdf::scan_page;

fn kb(name: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(name))
                .and_then(|l| l.split_whitespace().nth(1).map(str::to_string))
        })
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn env_usize(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

/// *n* Form-XObjects, die sich **ein** `/Resources` per Referenz teilen — und
/// in diesem Verzeichnis steht eine Zeichenkette von `junk` Byte.
///
/// Genau der Fall aus der Prüfung: `dictionary_objects` zählt die Zeichenkette
/// als *eins*, jeder gemerkte Strom klont sie aber vollständig.
///
/// Geschrieben wird von Hand, mit einer **klassischen** Querverweis-Tabelle:
/// `lopdf` legt sonst einen Querverweis-*Stream* an, und dessen binäre Nutzlast
/// enthält bei 50 000 Objekten mehr `[` als `]`. Die Vorprüfung in
/// `document.rs` liest das als Verschachtelung und lehnt die Datei ab
/// (Nachzugsliste) — ein eigener Befund, der diese Messung sonst verdeckt.
///
/// `distinct` dreht den Fall um: dann bekommt **jedes** Formular ein eigenes
/// `/Resources`-Objekt mit eigener Zeichenkette. Hier gibt es nichts zu teilen;
/// geprüft wird, dass die Decke diese Kopien dann wirklich sieht.
fn geteilte_ressourcen(n: usize, junk: usize, distinct: bool) -> Vec<u8> {
    // Objektnummern: 1 Inhalt, 2 geteiltes /Resources, 3..3+n Formulare,
    // dann (bei `distinct`) n eigene Verzeichnisse, Seitenressourcen, Seite,
    // Seitenbaum, Katalog.
    let content_id = 1usize;
    let shared_id = 2usize;
    let form0 = 3usize;
    let res0 = form0 + n;
    let page_res_id = if distinct { res0 + n } else { res0 };
    let page_id = page_res_id + 1;
    let pages_id = page_id + 1;
    let catalog_id = pages_id + 1;
    let last = catalog_id;

    let mut content = Vec::new();
    for i in 0..n {
        content.extend_from_slice(format!("q /X{i} Do Q\n").as_bytes());
    }

    let mut out: Vec<u8> = b"%PDF-1.4\n%\xbb\xad\xc0\xde\n".to_vec();
    let mut offsets = vec![0usize; last + 1];
    let put = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, id: usize, body: &[u8]| {
        offsets[id] = out.len();
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };

    let mut body = format!("<</Length {}>>stream\n", content.len()).into_bytes();
    body.extend_from_slice(&content);
    body.extend_from_slice(b"\nendstream");
    put(&mut out, &mut offsets, content_id, &body);

    let junk_dict = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, id: usize| {
        let mut body = b"<</Junk (".to_vec();
        body.extend(std::iter::repeat_n(b'A', junk));
        body.extend_from_slice(b")>>");
        put(out, offsets, id, &body);
    };
    junk_dict(&mut out, &mut offsets, shared_id);

    for i in 0..n {
        let res = if distinct { res0 + i } else { shared_id };
        let form_body = format!(
            "<</Type/XObject/Subtype/Form/BBox[0 0 10 10]/Resources {res} 0 R/Length 0>>\
             stream\n\nendstream"
        );
        put(&mut out, &mut offsets, form0 + i, form_body.as_bytes());
    }
    if distinct {
        for i in 0..n {
            junk_dict(&mut out, &mut offsets, res0 + i);
        }
    }

    let mut body = b"<</XObject<<".to_vec();
    for i in 0..n {
        body.extend_from_slice(format!("/X{i} {} 0 R", form0 + i).as_bytes());
    }
    body.extend_from_slice(b">>>>");
    put(&mut out, &mut offsets, page_res_id, &body);

    put(
        &mut out,
        &mut offsets,
        page_id,
        format!(
            "<</Type/Page/Parent {pages_id} 0 R/Contents {content_id} 0 R\
             /Resources {page_res_id} 0 R/MediaBox[0 0 595 842]>>"
        )
        .as_bytes(),
    );
    put(
        &mut out,
        &mut offsets,
        pages_id,
        format!("<</Type/Pages/Kids[{page_id} 0 R]/Count 1>>").as_bytes(),
    );
    put(
        &mut out,
        &mut offsets,
        catalog_id,
        format!("<</Type/Catalog/Pages {pages_id} 0 R>>").as_bytes(),
    );

    let startxref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", last + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().take(last + 1).skip(1) {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<</Size {}/Root {catalog_id} 0 R>>\nstartxref\n{startxref}\n%%EOF\n",
            last + 1
        )
        .as_bytes(),
    );
    out
}

#[test]
#[ignore = "Messung"]
fn bauen() {
    let n = env_usize("MESS_N", 5000);
    let junk = env_usize("MESS_JUNK", 16384);
    let out = std::env::var("MESS_OUT").expect("MESS_OUT");
    let distinct = std::env::var("MESS_DISTINCT").is_ok();
    let bytes = geteilte_ressourcen(n, junk, distinct);
    println!(
        "gebaut: n={n} junk={junk} distinct={distinct} -> {:.2} MB",
        bytes.len() as f64 / (1024.0 * 1024.0)
    );
    std::fs::write(&out, &bytes).expect("schreibbar");
}

#[test]
#[ignore = "Messung"]
fn messen() {
    let path = std::env::var("MESS_IN").expect("MESS_IN");
    let bytes = std::fs::read(&path).expect("lesbar");
    println!("Datei: {:.2} MB", bytes.len() as f64 / (1024.0 * 1024.0));
    let vor = kb("VmHWM:");
    let start = std::time::Instant::now();
    let doc = redact_pdf::load_from_bytes(&bytes).expect("ladbar");
    let nach_laden = kb("VmHWM:");
    let pages = doc.get_pages();
    let (_, page_id) = pages.iter().next().expect("eine Seite");
    let scan = scan_page(&doc, *page_id).expect("lesbar");
    let dauer = start.elapsed();
    let hwm = kb("VmHWM:");
    println!(
        "VmHWM vorher {} kB | nach dem Laden {} kB | nach dem Scan {} kB ({:.0} MB)",
        vor,
        nach_laden,
        hwm,
        hwm as f64 / 1024.0
    );
    println!("Zeit: {dauer:?}");
    println!(
        "VmRSS nach dem Scan: {} kB | Ströme: {} | erklärte Formulare: {}",
        kb("VmRSS:"),
        scan.form_placements.len(),
        scan.declared_forms.len()
    );
    println!(
        "Warnungen: {} | marked: {} | shows: {}",
        scan.warnings.len(),
        scan.marked.len(),
        scan.shows.len()
    );
    if let Some(first) = scan.warnings.first() {
        println!("erste Warnung: {first}");
    }
    println!(
        "Aufwand: decoded_streams={} loaded_font_maps={} parsed_fonts={} declared_resources={} \
         retained_weight={}",
        scan.effort.decoded_streams,
        scan.effort.loaded_font_maps,
        scan.effort.parsed_fonts,
        scan.effort.declared_resources,
        scan.effort.retained_weight,
    );
    println!("Datensätze: {}", scan.shows.len());
}

// ---------------------------------------------------------------------------
// Punkt 3 — was `font_from_dict` **baut**, bevor die Decke gefragt wird
// ---------------------------------------------------------------------------

/// Eine Seite mit `fonts` CID-Schriften, deren `/W` je `ranges` Bereiche zu je
/// 65 536 Codes aufzählt.
///
/// Ein Bereich ist ein Zahlentripel von rund 17 Byte und ergibt 65 536
/// Einträge in der Breitentabelle. `parse_cid_widths` deckelt den **einzelnen**
/// Bereich auf 65 535 — die **Summe** über viele Bereiche deckelt nichts.
fn breitenbombe(fonts: usize, ranges: usize) -> (lopdf::Document, lopdf::ObjectId) {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.7");
    let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));

    let mut font_dict = lopdf::Dictionary::new();
    for f in 0..fonts {
        let mut w: Vec<Object> = Vec::new();
        for r in 0..ranges {
            let first = (r as i64) * 65_536;
            w.push(first.into());
            w.push((first + 65_535).into());
            w.push(500_i64.into());
        }
        let descendant = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "CIDFontType2",
            "BaseFont" => "Test",
            "CIDSystemInfo" => dictionary! {
                "Registry" => Object::string_literal("Adobe"),
                "Ordering" => Object::string_literal("Identity"),
                "Supplement" => 0_i64,
            },
            "DW" => 1000_i64,
            "W" => Object::Array(w),
        });
        let font = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "BaseFont" => "Test",
            "Encoding" => "Identity-H",
            "DescendantFonts" => vec![Object::Reference(descendant)],
        });
        font_dict.set(format!("F{f}"), font);
    }

    let resources_id = doc.add_object(dictionary! { "Font" => font_dict });
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.objects.insert(
        content_id,
        Object::Stream(Stream::new(
            dictionary! {},
            b"BT /F0 12 Tf 10 10 Td <0041> Tj ET\n".to_vec(),
        )),
    );
    (doc, page_id)
}

#[test]
#[ignore = "Messung"]
fn messen_breiten() {
    let fonts = env_usize("MESS_FONTS", 1);
    let ranges = env_usize("MESS_RANGES", 100);
    let (doc, page_id) = breitenbombe(fonts, ranges);
    let bytes = redact_pdf::save_to_bytes(&doc).expect("speicherbar");
    println!(
        "fonts={fonts} ranges={ranges} Datei: {:.3} MB",
        bytes.len() as f64 / (1024.0 * 1024.0)
    );
    let vor = kb("VmHWM:");
    let start = std::time::Instant::now();
    let scan = scan_page(&doc, page_id).expect("lesbar");
    let dauer = start.elapsed();
    println!(
        "VmHWM {} -> {} kB ({:.0} MB), Zeit {dauer:?}, parsed_fonts={}",
        vor,
        kb("VmHWM:"),
        kb("VmHWM:") as f64 / 1024.0,
        scan.effort.parsed_fonts
    );
}
