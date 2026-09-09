//! Budget des Leck-Orakels (Befund G5-A2, Fix-Runde 4).
//!
//! `leaks_many_within(bytes, needles, max_decompressed_bytes)` entpackt je
//! Sicht höchstens `max_decompressed_bytes` — **beim** Entpacken begrenzt,
//! nicht hinterher gemessen. Ein Strom darüber steht in `unchecked` mit
//! Objekt-Id und Grund, seine gepackten Bytes werden roh trotzdem
//! durchsucht. Die Messtests (`zd_mess_*`) bleiben `#[ignore]`; Lauf:
//! `cargo test --release -p redact-pdf --test zd_orakel_budget -- --ignored --nocapture`.

mod common;

use std::io::Write;
use std::time::{Duration, Instant};

use common::{page, text_ops, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_pdf::{leaks_many_within, LeakCheck};

/// Steht im Klartext **hinter** dem zlib-Strom im selben `stream`-Block —
/// die Rohsicht muss ihn auch dann sehen, wenn der Strom nicht entpackt wird.
const MARKER: &str = "MARKER-HINTER-DEM-ZLIB-STROM";
/// Steht im Klartext in `/Info /Title`.
const OTHER: &str = "Max Mustermann";

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).expect("deflate");
    e.finish().expect("deflate")
}

/// Seiteninhalt mit dem Geheimnis, auf `size` Byte mit Kommentarzeilen
/// aufgefüllt (die Extraktion liest sie als Leerraum).
fn content(size: usize) -> Vec<u8> {
    let mut out = text_ops(&[SECRET]);
    while out.len() < size {
        out.extend_from_slice(b"% 0123456789 0123456789 0123456789 0123456789 0123456789\n");
    }
    out.truncate(size);
    if let Some(last) = out.last_mut() {
        *last = b'\n';
    }
    out
}

/// Eine Seite, deren Inhalt `stream` ist; `/Info /Title` trägt `OTHER`.
fn pdf_with(stream: Stream) -> (Vec<u8>, ObjectId) {
    let mut d = page(&[]);
    d.doc.objects.insert(d.content_id, Object::Stream(stream));
    let info = d.add(Object::Dictionary(
        dictionary! { "Title" => Object::string_literal(OTHER) },
    ));
    d.doc.trailer.set("Info", info);
    (d.finish(), d.content_id)
}

fn flate_stream(packed: Vec<u8>) -> Stream {
    Stream::new(dictionary! { "Filter" => "FlateDecode" }, packed).with_compression(false)
}

fn object(id: ObjectId) -> String {
    format!("Objekt {} {}", id.0, id.1)
}

fn check(bytes: &[u8], needles: &[&str], budget: u64) -> LeakCheck {
    leaks_many_within(bytes, needles, budget)
}

// ---------------------------------------------------------------------------
// (a) Budget unter der Stromgröße
// ---------------------------------------------------------------------------

/// Ein Flate-Strom, der mehr ergäbe als das Budget: er wird nicht entpackt,
/// `unchecked` nennt ihn mit Objekt-Id, das Geheimnis darin bleibt
/// ungefunden — und der Klartext hinter dem zlib-Strom im selben Block wie
/// der in `/Info` wird trotzdem gefunden. Ohne Budget ist alles da.
#[test]
fn ein_strom_ueber_dem_budget_wird_nicht_entpackt_aber_roh_durchsucht() {
    let plain = content(200 * 1024);
    let mut packed = deflate(&plain);
    packed.extend_from_slice(format!("\n{MARKER}\n").as_bytes());
    let (pdf, content_id) = pdf_with(flate_stream(packed));

    let budget = (plain.len() / 2) as u64;
    let result = check(&pdf, &[SECRET, MARKER, OTHER], budget);
    assert!(
        result.findings[0].is_empty(),
        "das Geheimnis stand nur im Strom, der nicht entpackt wurde: {:?}",
        result.findings[0]
    );
    assert!(
        result.findings[1].iter().any(|h| h.contains("(roh)")),
        "die Rohsicht auf die gepackten Bytes ist nicht gelaufen: {:?}",
        result.findings[1]
    );
    assert!(
        !result.findings[2].is_empty(),
        "der Klartext in /Info wurde nicht gefunden"
    );
    let named = object(content_id);
    assert!(
        result
            .unchecked
            .iter()
            .any(|u| u.contains(&named) && u.contains("nicht entpackt")),
        "unchecked nennt den Strom nicht als {named}: {:?}",
        result.unchecked
    );
    assert!(
        result.unchecked.iter().any(|u| u.contains("Objektgraph")),
        "der Objektgraph wurde trotz Bombe geladen: {:?}",
        result.unchecked
    );

    // Gegenrichtung: ohne Budget wird alles entpackt und gefunden.
    let full = check(&pdf, &[SECRET, MARKER, OTHER], u64::MAX);
    assert!(full.unchecked.is_empty(), "{:?}", full.unchecked);
    assert!(
        full.findings[0].iter().any(|h| h.contains(&named)),
        "{:?}",
        full.findings[0]
    );
}

/// RunLength (ein Byte → 128) ist für die Rohsicht kein zlib und kostet
/// dort nichts; die Objektsicht entpackt ihn — bis zum Budget. Darüber
/// nennt `unchecked` den Strom als Objekt, der Objektgraph selbst bleibt
/// durchsucht. Das Geheimnis steht in Zweierläufen, die keine Kodierung
/// roh trifft.
#[test]
fn ein_runlength_strom_ueber_dem_budget_wird_als_objekt_genannt() {
    let ops = text_ops(&[SECRET]);
    let mut rl = Vec::new();
    for pair in ops.chunks(2) {
        rl.push((pair.len() - 1) as u8);
        rl.extend_from_slice(pair);
    }
    const RUNS: usize = 16 * 1024;
    for _ in 0..RUNS {
        rl.extend_from_slice(&[129, b' ']);
    }
    rl.push(128);
    let decoded_len = ops.len() + RUNS * 128;
    let stream =
        Stream::new(dictionary! { "Filter" => "RunLengthDecode" }, rl).with_compression(false);
    let (pdf, content_id) = pdf_with(stream);
    let named = object(content_id);

    let tight = check(&pdf, &[SECRET, OTHER], (decoded_len - 1) as u64);
    assert!(tight.findings[0].is_empty(), "{:?}", tight.findings[0]);
    assert!(
        tight
            .unchecked
            .iter()
            .any(|u| u.starts_with(&format!("{named} <Stream>")) && u.contains("nicht entpackt")),
        "{:?}",
        tight.unchecked
    );
    assert!(
        !tight.unchecked.iter().any(|u| u.contains("Objektgraph")),
        "der Objektgraph war ladbar und musste durchsucht werden: {:?}",
        tight.unchecked
    );
    assert!(
        tight.unchecked.iter().any(|u| u.contains("Sicht 7")),
        "{:?}",
        tight.unchecked
    );

    let enough = check(&pdf, &[SECRET, OTHER], decoded_len as u64);
    assert!(enough.unchecked.is_empty(), "{:?}", enough.unchecked);
    assert!(
        enough.findings[0]
            .iter()
            .any(|h| h.contains("dekodiert: RunLengthDecode")),
        "{:?}",
        enough.findings[0]
    );
}

// ---------------------------------------------------------------------------
// (b) Budget = Summe
// ---------------------------------------------------------------------------

/// Das kleinste Budget, mit dem die Vorprüfung des Laders (`prescan`) die
/// Datei durchlässt — dieselbe Zahl, die `--max-decompressed-mb` an der
/// Schwärzung entscheidet. Sie liegt über `n`: der Lader zählt ungefilterte
/// Ströme (den Querverweis-Strom, den `lopdf` schreibt) roh mit.
fn loader_sum(pdf: &[u8], n: u64) -> u64 {
    use redact_pdf::document::{prescan, Limits};
    let passes = |b: u64| {
        prescan(
            pdf,
            &Limits {
                max_decompressed_bytes: b,
                max_parsed_bytes: u64::MAX,
                ..Limits::default()
            },
        )
        .is_ok()
    };
    let (mut lo, mut hi) = (n, n + pdf.len() as u64);
    assert!(
        !passes(lo) && passes(hi),
        "Vorbedingung: Schwelle zwischen n und n + Dateigröße"
    );
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if passes(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// Das Budget zählt je Sicht die **Summe** der entpackten Bytes — dieselbe
/// Einheit wie `Limits::max_decompressed_bytes`. Zwei Flate-Ströme (`A`,
/// `B`, zusammen `n` Byte) und zwei Grenzen, beide auf das Byte genau: die
/// eigene (`n`: beide entpackt; `n - 1`: der zweite nicht mehr) und die der
/// Vorprüfung des Laders (die ungefilterte Ströme roh mitzählt und deshalb
/// etwas mehr braucht). Mit der Summe des Laders ist alles geprüft, ein
/// Byte weniger nicht.
#[test]
fn budget_gleich_der_summe_prueft_alles_ein_byte_weniger_nicht() {
    let a = content(300 * 1024);
    let b = content(100 * 1024);
    let mut d = page(&[]);
    d.doc
        .objects
        .insert(d.content_id, Object::Stream(flate_stream(deflate(&a))));
    let second = d.add(Object::Stream(flate_stream(deflate(&b))));
    d.page_dict_set(
        "Contents",
        Object::Array(vec![
            Object::Reference(d.content_id),
            Object::Reference(second),
        ]),
    );
    let pdf = d.finish();
    let n = (a.len() + b.len()) as u64;
    let sum = loader_sum(&pdf, n);
    let (first, second) = (object(d.content_id), object(second));

    let exact = check(&pdf, &[SECRET], sum);
    assert!(exact.unchecked.is_empty(), "{:?}", exact.unchecked);
    assert!(
        exact.findings[0].iter().any(|h| h.contains(&first))
            && exact.findings[0].iter().any(|h| h.contains(&second)),
        "{:?}",
        exact.findings[0]
    );

    // Ein Byte unter der Summe des Laders: die eigenen Sichten kommen noch
    // durch (kein Eintrag für einen Strom), nur der Lader lehnt ab.
    let short = check(&pdf, &[SECRET], sum - 1);
    assert_eq!(short.unchecked.len(), 1, "{:?}", short.unchecked);
    assert!(
        short.unchecked[0].contains("Objektgraph") && short.unchecked[0].contains("Vorprüfung"),
        "{:?}",
        short.unchecked
    );

    // Genau `n`: die Rohsicht entpackt beide Ströme; ein Byte weniger, und
    // der zweite passt nach dem ersten nicht mehr hinein — die Summe zählt,
    // nicht der einzelne Strom.
    let own = check(&pdf, &[SECRET], n);
    assert!(
        !own.unchecked
            .iter()
            .any(|u| u.contains(&first) || u.contains(&second)),
        "{:?}",
        own.unchecked
    );
    assert!(
        [&first, &second].iter().all(|id| own.findings[0]
            .iter()
            .any(|h| h.contains(id.as_str()) && h.contains("(inflate)"))),
        "{:?}",
        own.findings[0]
    );
    let below = check(&pdf, &[SECRET], n - 1);
    assert!(
        below
            .unchecked
            .iter()
            .any(|u| u.contains(&second) && u.contains("nicht entpackt")),
        "{:?}",
        below.unchecked
    );
    assert!(
        !below.unchecked.iter().any(|u| u.contains(&first)),
        "{:?}",
        below.unchecked
    );
}

// ---------------------------------------------------------------------------
// (c) Die Bombe
// ---------------------------------------------------------------------------

const MIB: u64 = 1024 * 1024;
/// Größe der Bombe entpackt. Im Debug-Profil kleiner, weil schon das
/// Erzeugen (unoptimiertes `miniz_oxide`) sonst Minuten dauert; die Grenze
/// greift unabhängig von der Größe.
const BOMB_BYTES: u64 = if cfg!(debug_assertions) {
    256 * MIB
} else {
    1024 * MIB
};

/// Nullen, gepackt — streamend, damit die Erzeugung selbst keinen Speicher
/// in Bombengröße braucht.
fn bomb() -> Vec<u8> {
    let zeros = vec![0u8; MIB as usize];
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    for _ in 0..BOMB_BYTES / MIB {
        e.write_all(&zeros).expect("deflate");
    }
    e.finish().expect("deflate")
}

/// `VmHWM` aus `/proc/self/status`, in Byte — der Spitzenwert des Prozesses.
fn peak_rss_bytes() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().strip_suffix("kB"))
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|kb| kb * 1024)
        .expect("VmHWM")
}

/// Umgebungsvariable, mit der sich der Bombentest als Kindprozess erkennt.
const CHILD: &str = "ZD_ORAKEL_BUDGET_KIND";

/// 1 GiB Nullen, 1 MB gepackt, Budget 16 MiB: als Seiteninhalt **und** als
/// Objekt-Strom (`/Type /ObjStm` — den entpackt `lopdf` beim Laden selbst,
/// die Vorprüfung des Laders muss ihn vorher abfangen). Spitzenbelegung
/// unter 100 MB, unter 2 s (Release).
///
/// `VmHWM` ist der Spitzenwert des **Prozesses**; damit die anderen Tests
/// dieser Datei ihn nicht mitprägen, misst ein Kindprozess, in dem nur
/// dieser Test läuft.
#[test]
fn eine_flate_bombe_bleibt_im_budget() {
    if std::env::var_os(CHILD).is_some() {
        return bombe_im_kindprozess();
    }
    let exe = std::env::current_exe().expect("Testbinary");
    let output = std::process::Command::new(exe)
        .args([
            "--exact",
            "eine_flate_bombe_bleibt_im_budget",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .expect("Kindprozess");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprint!("{stderr}");
    assert!(
        output.status.success(),
        "Kindprozess: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        stderr.contains("VmHWM"),
        "der Kindprozess hat nicht gemessen: {stderr}"
    );
}

/// Eine Datei von Hand, deren Querverweis-Strom sagt, Objekt 6 liege im
/// Objekt-Strom 5: genau dann entpackt `lopdf` den Strom 5 beim Laden
/// (`ObjectStream::new`, ohne Grenze). `lopdf` selbst schreibt keinen
/// `/Type /ObjStm`, den man ihm gäbe — deshalb Bytes statt `Document`.
fn objstm_bomb_pdf(packed: &[u8]) -> (Vec<u8>, ObjectId) {
    let mut out = b"%PDF-1.5\n".to_vec();
    let mut offsets = Vec::new();
    let mut add = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(body);
    };
    add(
        &mut out,
        b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
    );
    add(
        &mut out,
        b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
    );
    add(
        &mut out,
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>\nendobj\n",
    );
    add(
        &mut out,
        b"4 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n",
    );
    let mut objstm = format!(
        "5 0 obj\n<< /Type /ObjStm /N 1 /First 0 /Filter /FlateDecode /Length {} >>\nstream\n",
        packed.len()
    )
    .into_bytes();
    objstm.extend_from_slice(packed);
    objstm.extend_from_slice(b"\nendstream\nendobj\n");
    add(&mut out, &objstm);

    // Querverweis-Strom 7: /W [1 4 2] — Typ, Feld 2, Feld 3.
    let xref_offset = out.len();
    let mut rows = vec![0u8, 0, 0, 0, 0, 0xff, 0xff];
    for &offset in &offsets {
        rows.push(1);
        rows.extend_from_slice(&(offset as u32).to_be_bytes());
        rows.extend_from_slice(&[0, 0]);
    }
    rows.push(2);
    rows.extend_from_slice(&5u32.to_be_bytes());
    rows.extend_from_slice(&[0, 0]);
    rows.push(1);
    rows.extend_from_slice(&(xref_offset as u32).to_be_bytes());
    rows.extend_from_slice(&[0, 0]);
    let mut xref = format!(
        "7 0 obj\n<< /Type /XRef /Size 8 /W [1 4 2] /Root 1 0 R /Length {} >>\nstream\n",
        rows.len()
    )
    .into_bytes();
    xref.extend_from_slice(&rows);
    xref.extend_from_slice(b"\nendstream\nendobj\n");
    out.extend_from_slice(&xref);
    out.extend_from_slice(format!("startxref\n{xref_offset}\n%%EOF\n").as_bytes());
    (out, (5, 0))
}

fn bombe_im_kindprozess() {
    let packed = bomb();
    assert!(
        packed.len() < 2 * MIB as usize,
        "gepackt {} Byte",
        packed.len()
    );
    let budget = 16 * MIB;
    let before = peak_rss_bytes();
    let deadline = if cfg!(debug_assertions) {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(2)
    };

    // Der Objekt-Strom zuerst: ohne die Vorprüfung entpackt ihn `lopdf` beim
    // Laden, und das muss die Speichermessung zeigen, nicht ein Folgefehler.
    for objstm in [true, false] {
        let (pdf, id) = if objstm {
            objstm_bomb_pdf(&packed)
        } else {
            let mut d = page(&[]);
            d.doc
                .objects
                .insert(d.content_id, Object::Stream(flate_stream(packed.clone())));
            (d.finish(), d.content_id)
        };

        let started = Instant::now();
        let result = check(&pdf, &[SECRET], budget);
        let elapsed = started.elapsed();
        let peak = peak_rss_bytes();
        eprintln!(
            "Bombe (objstm={objstm}): {} MiB entpackt, {} Byte gepackt, Budget {} MiB: {elapsed:?}, \
             VmHWM {} MB (vorher {} MB)",
            BOMB_BYTES / MIB,
            packed.len(),
            budget / MIB,
            peak / 1_000_000,
            before / 1_000_000
        );
        assert!(
            peak < 100_000_000,
            "objstm={objstm}: VmHWM {} MB — die Bombe wurde entpackt",
            peak / 1_000_000
        );
        assert!(
            elapsed < deadline,
            "objstm={objstm}: {elapsed:?} — die Bombe wurde entpackt"
        );
        assert!(result.findings[0].is_empty());
        assert!(
            result
                .unchecked
                .iter()
                .any(|u| u.contains(&object(id)) && u.contains("nicht entpackt")),
            "objstm={objstm}: {:?}",
            result.unchecked
        );
        assert!(
            result.unchecked.iter().any(|u| u.contains("Objektgraph")),
            "objstm={objstm}: {:?}",
            result.unchecked
        );
    }
}

// ---------------------------------------------------------------------------
// Messung — bleibt ignoriert
// ---------------------------------------------------------------------------

/// Ein Bildstrom aus Zufallsbytes (nicht komprimierbar, mit vielen `(`
/// und `<`, die die Zeichenketten-Verkettung beschäftigen) — Größe in MiB
/// über `ZD_MB`, Vorgabe 64.
fn noise_pdf() -> Vec<u8> {
    let mib: u64 = std::env::var("ZD_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut noise = Vec::with_capacity((mib * MIB) as usize);
    while (noise.len() as u64) < mib * MIB {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        noise.extend_from_slice(&state.to_le_bytes());
    }
    let mut d = page(&[SECRET]);
    let image = Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image",
            "Width" => 4096, "Height" => (mib * MIB / 4096 / 3) as i64,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
            "Filter" => "FlateDecode",
        },
        deflate(&noise),
    )
    .with_compression(false);
    let image_id = d.add(Object::Stream(image));
    d.doc
        .get_dictionary_mut(d.resources_id)
        .expect("Resources")
        .set("XObject", dictionary! { "Im0" => image_id });
    d.finish()
}

/// Vorher (memmem je Begriff und Kodierung): 64 MiB, 1 Begriff 0,26 s,
/// 1 000 Begriffe 65,7 s. Ziel: 1 000 Begriffe kosten ungefähr so viel wie
/// einer — ein Automat über alle Muster, ein Durchgang je Datenblock.
#[test]
#[ignore = "Messung — Release, --nocapture"]
fn zd_mess_1000_begriffe_kosten_wie_einer() {
    let pdf = noise_pdf();
    let many: Vec<String> = (0..1000)
        .map(|i| format!("DE{:02} 1234 5678 9012 3456 {:02}", i % 100, i / 10))
        .collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();

    let started = Instant::now();
    let one = check(&pdf, &[SECRET], u64::MAX);
    let t1 = started.elapsed();
    let started = Instant::now();
    let thousand = check(&pdf, &many, u64::MAX);
    let t1000 = started.elapsed();
    eprintln!(
        "{} MiB Datei: 1 Begriff {t1:?}; 1000 Begriffe {t1000:?}; Verhältnis {:.2}",
        pdf.len() / MIB as usize,
        t1000.as_secs_f64() / t1.as_secs_f64()
    );
    assert!(!one.findings[0].is_empty());
    assert!(one.unchecked.is_empty() && thousand.unchecked.is_empty());
    assert!(
        t1000 < t1 * 4,
        "1000 Begriffe ({t1000:?}) kosten mehr als das Vierfache von einem ({t1:?})"
    );
}
