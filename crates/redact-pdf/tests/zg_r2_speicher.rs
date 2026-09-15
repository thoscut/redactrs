//! Gegenprüfung R2 (nach Fix-Runde 6): **Speicher und Zeit** des Leck-Orakels
//! an großen Strömen — eigene Messung, eigene Dateien, eigener Kindprozess.
//!
//! `filters::decode_chain` klont seit Fix-Runde 6 erst, **nachdem**
//! `decode_one` etwas geliefert hat. Der Doc-Kommentar von
//! [`redact_pdf::leaks_many_within`] sagt dazu zu (Abschnitt
//! „Spitzenbelegung“):
//!
//! * gleichzeitig im Speicher: Dateibytes, geladenes Dokument, **ein Strom in
//!   Arbeit** (≤ verbleibendes Budget + 1 Byte), bei einer Kette gleichzeitig
//!   Eingabe **und** Ausgabe des laufenden Gliedes, dazu die
//!   Zeichenketten-Verkettung (noch einmal dieselbe Größe);
//! * die Rohbytes werden **nicht** kopiert: 64-MiB-Strom, `/Filter /DCTDecode`
//!   → 138 MB statt vorher 205 MB;
//! * das Budget ist keine Zusage über die Prozessgröße: derselbe Strom als
//!   `/FlateDecode` über nicht entpackbare Bytes kommt auf 621 MB.
//!
//! Diese Messung prüft alle drei Sätze an eigenen Dateien: ohne Filter,
//! `/DCTDecode`, `/FlateDecode` über nicht entpackbare Bytes und eine Kette
//! aus fünf Filtern. `VmHWM` ist der Spitzenwert des **Prozesses** — deshalb
//! misst je ein Kindprozess, und die Datei baut der Elternprozess.
//!
//! Lauf (Release, sonst misst man den Debug-Allokator):
//! `cargo test --release -p redact-pdf --test zg_r2_speicher -- --ignored --nocapture`

use std::process::Command;
use std::time::Instant;

use lopdf::{dictionary, Document, Object, Stream};
use redact_pdf::leaks_many_within;

const SECRET: &str = "DE89 3704 0044 0532 0130 00";
/// Ist die Variable gesetzt, ist dieser Prozess das Kind und misst die Datei.
const MESSDATEI: &str = "ZG_R2_MESSDATEI";
/// Dasselbe Budget wie die Messung der Fix-Runde 6 (512 MiB) — über
/// `ZG_R2_BUDGET_MB` umstellbar, denn die **Vorgabe** der Kommandozeile ist
/// `--max-decompressed-mb 1024`, also das Doppelte.
fn budget() -> u64 {
    std::env::var("ZG_R2_BUDGET_MB")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(512)
        << 20
}

// ---------------------------------------------------------------------------
// Messwerkzeug
// ---------------------------------------------------------------------------

/// Spitzenbelegung des **Prozesses** (`VmHWM`), in Byte. `None` ohne `/proc`.
#[cfg(target_os = "linux")]
fn spitze() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().strip_suffix("kB"))
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|kb| kb * 1024)
}

#[cfg(not(target_os = "linux"))]
fn spitze() -> Option<u64> {
    None
}

/// Füllmaterial ohne `(` und `<`: gemessen werden soll der Strom, nicht die
/// Zeichenketten-Verkettung, die aus zufälligen Klammern eine zweite große
/// Kopie bauen würde.
fn fuellung(mib: usize) -> Vec<u8> {
    let muster = b"0123456789 abcdefghij ABCDEFGHIJ .-_+*/=%$!? ";
    let ziel = mib << 20;
    let mut noise = Vec::with_capacity(ziel);
    while noise.len() < ziel {
        noise.extend_from_slice(muster);
    }
    noise.truncate(ziel);
    noise
}

/// Eine Datei mit genau **einem** großen Strom unter `chain` (leer = kein
/// `/Filter`). Ohne Kompression gespeichert: die Dateigröße ist die
/// Stromgröße.
fn datei_mit_einem_strom(mib: usize, chain: &[&str]) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1_i64,
        }),
    );
    let katalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", katalog);

    let mut dict = dictionary! {};
    match chain {
        [] => {}
        [one] => dict.set("Filter", Object::Name(one.as_bytes().to_vec())),
        many => dict.set(
            "Filter",
            Object::Array(
                many.iter()
                    .map(|f| Object::Name(f.as_bytes().to_vec()))
                    .collect(),
            ),
        ),
    }
    let id = doc.add_object(Object::Stream(
        Stream::new(dict, fuellung(mib)).with_compression(false),
    ));
    doc.get_dictionary_mut(katalog)
        .expect("Katalog")
        .set("ZgR2", Object::Reference(id));
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("speicherbar");
    bytes
}

/// Ein Messfall: Name, Filterkette.
struct Fall {
    name: &'static str,
    chain: &'static [&'static str],
}

const FAELLE: &[Fall] = &[
    Fall {
        name: "ohne_filter",
        chain: &[],
    },
    Fall {
        name: "DCTDecode",
        chain: &["DCTDecode"],
    },
    Fall {
        name: "FlateDecode",
        chain: &["FlateDecode"],
    },
    Fall {
        name: "fuenf_filter",
        chain: &[
            "ASCIIHexDecode",
            "ASCII85Decode",
            "RunLengthDecode",
            "LZWDecode",
            "FlateDecode",
        ],
    },
    Fall {
        name: "fuenf_flate",
        chain: &[
            "FlateDecode",
            "FlateDecode",
            "FlateDecode",
            "FlateDecode",
            "FlateDecode",
        ],
    },
];

/// Die Zeile, die das Kind schreibt und der Elternprozess liest.
fn messzeile(name: &str, bytes: u64, ms: u128, unchecked: usize, funde: usize) -> String {
    format!("ZGR2MESS {name} vmhwm={bytes} ms={ms} unchecked={unchecked} funde={funde}")
}

fn lies(zeilen: &str, name: &str, feld: &str) -> u128 {
    // Die Testlaufzeit schreibt `test <name> ... ` ohne Zeilenumbruch davor —
    // die Messzeile steht deshalb nicht am Zeilenanfang.
    zeilen
        .lines()
        .find(|l| l.contains(&format!("ZGR2MESS {name} ")))
        .and_then(|l| {
            l.split_whitespace()
                .find_map(|t| t.strip_prefix(&format!("{feld}=")))
        })
        .unwrap_or_else(|| panic!("keine Messzeile für {name}/{feld} in:\n{zeilen}"))
        .parse()
        .expect("Zahl")
}

// ---------------------------------------------------------------------------
// Die Messung
// ---------------------------------------------------------------------------

/// **Messung R2-S**: Spitzenbelegung und Zeit des Orakels an 64-MiB-Strömen.
///
/// Geprüft wird die Zusicherung im Doc-Kommentar von `leaks_many_within`:
///
/// 1. `/DCTDecode` (erstes Glied unbekannt, nichts entpackt) kostet **nicht
///    mehr** als derselbe Strom ohne `/Filter` — das ist die Korrektur der
///    Fix-Runde 6 (kein Klon vor `decode_one`). Toleranz 16 MiB.
/// 2. `/FlateDecode` über nicht entpackbare Bytes bläht den Prozess weit über
///    die Dateigröße auf (roher Deflate-Rückfall). Der Doc-Kommentar nennt
///    das ausdrücklich; hier steht die eigene Zahl daneben.
/// 3. Keine Spitze überschreitet, was der Doc-Kommentar zulässt: Datei +
///    Dokument + Eingabe und Ausgabe des laufenden Gliedes (je ≤ Budget) +
///    Verkettung (noch einmal dieselbe Größe). Das ist die Decke
///    `3 × Budget + 4 × Datei`; alles darüber wäre ein Befund.
///
/// Mutationsnachweis (gefahren, siehe Bericht): in `filters::decode_chain`
/// den Klon vor `decode_one` ziehen (`let mut data = stream.content.clone();`
/// wie vor Fix-Runde 6) → Fall `DCTDecode` steigt um eine Stromgröße über
/// `ohne_filter` und Zusicherung 1 wird rot.
#[test]
#[ignore = "Messung — Release, --nocapture, ~64 MiB je Fall"]
fn r2_mess_spitze_und_zeit() {
    let mib: usize = std::env::var("ZG_R2_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);

    // Kindlauf: eine Datei messen und die Zahlen schreiben.
    if let Ok(pfad) = std::env::var(MESSDATEI) {
        let name = std::path::Path::new(&pfad)
            .file_stem()
            .map_or_else(|| "?".into(), |n| n.to_string_lossy().into_owned());
        let pdf = std::fs::read(&pfad).expect("Messdatei");
        let start = Instant::now();
        let check = leaks_many_within(&pdf, &[SECRET], budget());
        let ms = start.elapsed().as_millis();
        let peak = spitze().unwrap_or(0);
        println!(
            "{}",
            messzeile(
                &name,
                peak,
                ms,
                check.unchecked.len(),
                check.findings[0].len()
            )
        );
        for zeile in &check.unchecked {
            println!("ZGR2GRUND {name}: {zeile}");
        }
        return;
    }

    let dir = std::env::temp_dir().join(format!("zg-r2-mess-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("Messverzeichnis");
    let exe = std::env::current_exe().expect("Testbinary");
    let mut ausgabe = String::new();
    let mut dateigroesse = 0u64;
    for fall in FAELLE {
        let pfad = dir.join(format!("{}.pdf", fall.name));
        {
            let pdf = datei_mit_einem_strom(mib, fall.chain);
            dateigroesse = pdf.len() as u64;
            std::fs::write(&pfad, &pdf).expect("Messdatei schreiben");
        }
        let output = Command::new(&exe)
            .args([
                "--exact",
                "r2_mess_spitze_und_zeit",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(MESSDATEI, &pfad)
            .output()
            .expect("Kindprozess");
        std::fs::remove_file(&pfad).ok();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        print!("{stdout}");
        assert!(output.status.success(), "Kindprozess: {}", output.status);
        ausgabe.push_str(&stdout);
    }
    std::fs::remove_dir_all(&dir).ok();

    if spitze().is_none() {
        eprintln!("ohne /proc: keine Speichermessung, nur die Zeiten gelten");
        return;
    }

    let ohne = lies(&ausgabe, "ohne_filter", "vmhwm") as u64;
    let dct = lies(&ausgabe, "DCTDecode", "vmhwm") as u64;
    let strom = (mib as u64) << 20;

    // 1. Kein Klon der Rohbytes mehr (Fix-Runde 6).
    assert!(
        dct <= ohne + (16 << 20),
        "/DCTDecode kostet {dct} Byte gegen {ohne} Byte ohne Filter — mehr als \
         16 MiB Unterschied heißt: die Rohbytes werden wieder kopiert \
         (Stromgröße {strom})"
    );

    // 3. Die Decke des Doc-Kommentars.
    let decke = 3 * budget() + 4 * dateigroesse;
    for fall in FAELLE {
        let peak = lies(&ausgabe, fall.name, "vmhwm") as u64;
        assert!(
            peak <= decke,
            "{}: Spitze {peak} Byte über der Decke {decke} Byte des \
             Doc-Kommentars (3 × Budget + 4 × Datei)",
            fall.name
        );
        assert!(peak > 0, "{}: keine Messung", fall.name);
    }
}

// ---------------------------------------------------------------------------
// Ohne Release und ohne Kindprozess: die Form der Zusicherung
// ---------------------------------------------------------------------------

/// Der Satz „die Rohbytes werden nicht kopiert“ ist auch ohne Waage prüfbar:
/// ein Strom mit unbekanntem erstem Filter und einem **winzigen** Budget darf
/// nicht daran scheitern, dass jemand seine Rohbytes gegen das Budget bucht —
/// und er muss trotzdem gemeldet werden.
///
/// Mutationsnachweis: in `filters::decode_chain` den Klon vor `decode_one`
/// ziehen → `decoded_prefix_within` liefert 1 MiB bei `limit = 8`; die
/// Buchung `budget.charge(data.len())` in `scan_stream` läuft zwar nur über
/// `view.data` (bei `applied == 0` `None`), aber `decode_stream` bekäme die
/// Kopie. Die harte Zusicherung steht deshalb in `filters`.
#[test]
fn r2_unbekannter_erster_filter_kostet_keine_stromkopie() {
    use redact_pdf::filters::decoded_prefix_within;

    let doc = Document::with_version("1.5");
    let gross = fuellung(1);
    let stream = Stream::new(
        dictionary! { "Filter" => Object::Name(b"ZgR2Phantasie".to_vec()) },
        gross.clone(),
    )
    .with_compression(false);

    // Winziges Budget: der Teil-Dekoder darf nichts zurückgeben, was größer
    // ist als es.
    let (data, applied) = decoded_prefix_within(&doc, &stream, 8).expect("kein Oversize");
    assert_eq!(applied, 0);
    assert!(
        data.len() <= 8,
        "bei limit = 8 kamen {} Byte zurück — das ist die Stromkopie",
        data.len()
    );
}
