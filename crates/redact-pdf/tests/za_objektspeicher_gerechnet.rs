//! Die **Rechnung** der Vorprüfung darf nie unter dem liegen, was `lopdf`
//! wirklich belegt.
//!
//! ## Warum das die tragende Eigenschaft ist
//!
//! Die Objektdecke (`OBJEKTSPEICHER_JE_BUDGETBYTE` in `document.rs`) deckelt
//! nicht Dateibytes, sondern den **gerechneten** Speicher der Objekte:
//! `OBJEKT_BYTES` je Objekt, `ARRAY_BYTES` je Array. Eine Decke über einer
//! Rechnung ist nur so gut wie die Rechnung. Rechnet sie zu **hoch**, werden
//! gewöhnliche Dateien abgelehnt — das prüft `za_objektdecke.rs`. Rechnet sie
//! zu **niedrig**, ist die Decke wirkungslos, und genau das prüft diese Datei:
//! für jede Form muss `gerechnet ≥ gemessen` gelten.
//!
//! ## Warum eine eigene Datei mit einem einzigen Test
//!
//! Gemessen wird mit einem zählenden Allokator, und der zählt den **ganzen
//! Prozess**. Liefe daneben ein zweiter Test, mäße dieser hier dessen Speicher
//! mit. Ein Test je Testprogramm ist die einzige Anordnung, in der die Zahl
//! etwas bedeutet — dieselbe Begründung wie in `rev4_speicher_je_bereich.rs`.
//!
//! ## Warum keine Uhr
//!
//! Beide Zahlen sind deterministisch: die gerechnete hängt an den Bytes der
//! Datei, die gemessene an dem, was `lopdf` anfordert. Dieselbe Eingabe ergibt
//! Lauf für Lauf dieselben Werte.
//!
//! ## Gemessen (Release, 4-MB-Dateien)
//!
//! | Form (4 MB Datei) | gemessen | gerechnet | Byte je Dateibyte |
//! |---|---:|---:|---:|
//! | `[[][][]…]` — leere Arrays | 1 204,7 MB | 1 224,5 MB (1,02×) | **301,6** |
//! | `[/a/a…]` — Namen | 300,2 MB | 312,4 MB (1,04×) | 75,2 |
//! | `[0 0 0…]` — Zahlen | 292,6 MB | 312,4 MB (1,07×) | 73,3 |
//! | `<</ab 0 …>>` — Dictionary-Einträge | 176,1 MB | 248,7 MB (1,41×) | 44,1 |
//! | `[<<>><<>>…]` — leere Dictionaries | 146,7 MB | — | 36,7 |
//! | `[1 0 R …]` — Verweise | 146,0 MB | 312,4 MB (2,14×) | 36,5 |
//! | eine 800-Byte-Zeichenkette | 4,6 MB | — | 1,1 |
//!
//! Die Rechnung liegt überall darüber, am dichtesten (2 %) ausgerechnet bei der
//! Form, die den Befund ausgelöst hat. Der Aufschlag bei Verweisen ist
//! gewollt: `1 0 R` ist **ein** Objekt und zählt drei Wörter. Ein Abzug für
//! das `R` ließe sich mit `R R R R …` dazu missbrauchen, echte Objekte
//! wegzurechnen — Wörter dürfen nur addiert werden.
//!
//! Bei zwei Formen steht kein „gerechnet“: ihr Eigenfaktor (36,7 bzw. 1,1)
//! liegt unter `FAKTOR`, die Objektdecke bindet bei ihnen also nie, und die
//! Rechnung ist von außen weder ablesbar noch tragend — bei ihnen entscheidet
//! das Byte-Budget.
//!
//! Dieselben Messwerte je **Objekt** statt je Datei (802-Byte-Körper mit je
//! 401 Objekten): ein leeres Array 632,4 Byte, jedes andere Objekt 153,6.
//! Daher `ARRAY_BYTES = 640` und `OBJEKT_BYTES = 160`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use redact_pdf::document::{prescan, Limits};

// ---------------------------------------------------------------------------
// Der zählende Allokator
// ---------------------------------------------------------------------------

static LIVE: AtomicUsize = AtomicUsize::new(0);

struct Zaehlend;

// SAFETY: Jede Anforderung wird unverändert an den Systemallokator
// weitergereicht; gezählt wird nur nebenher.
unsafe impl GlobalAlloc for Zaehlend {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let neu = unsafe { System.realloc(ptr, layout, new_size) };
        if !neu.is_null() {
            if new_size >= layout.size() {
                LIVE.fetch_add(new_size - layout.size(), Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        neu
    }
}

#[global_allocator]
static ALLOC: Zaehlend = Zaehlend;

// ---------------------------------------------------------------------------
// Bauplätze — bewusst dieselben wie in `za_objektdecke.rs`, damit beide Tests
// über dieselben Formen reden.
// ---------------------------------------------------------------------------

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

fn form(name: &str) -> Vec<u8> {
    let mut v = b"[".to_vec();
    match name {
        "leere Arrays" => (0..400).for_each(|_| v.extend_from_slice(b"[]")),
        "Zahlen" => (0..400).for_each(|_| v.extend_from_slice(b"0 ")),
        "Namen" => (0..400).for_each(|_| v.extend_from_slice(b"/a")),
        "leere Dictionaries" => (0..200).for_each(|_| v.extend_from_slice(b"<<>>")),
        "Verweise" => (0..134).for_each(|_| v.extend_from_slice(b"1 0 R ")),
        // Die Gegenprobe am unteren Ende: eine einzige lange Zeichenkette ist
        // 800 Byte Datei und **ein** Objekt.
        "eine Zeichenkette" => {
            let mut z = b"(".to_vec();
            z.extend(std::iter::repeat_n(b'a', 800));
            z.push(b')');
            return z;
        }
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

fn rumpf(name: &str, ziel: usize) -> Vec<u8> {
    let o = form(name);
    let n = ziel / (o.len() + 40);
    let mut alle = geruest();
    alle.extend(std::iter::repeat_n(o, n));
    datei(&alle)
}

/// Was die Vorprüfung für diese Datei **rechnet**, in Byte — oder `None`,
/// wenn sich die Zahl von außen nicht ablesen lässt.
///
/// Abgelesen am Verhalten, ohne neue öffentliche API: die Decke ist
/// `max_parsed_bytes × OBJEKTSPEICHER_JE_BUDGETBYTE`; gesucht ist das kleinste
/// `max_parsed_bytes`, bei dem die Objektdecke nicht mehr anschlägt.
///
/// Gesucht wird **erst ab der Dateigröße**: darunter schlägt das Byte-Budget
/// an, und dessen Meldung sähe hier wie „die Objektdecke hält“ aus. Liegt der
/// Eigenfaktor einer Form unter `faktor`, bindet die Objektdecke nie — dann
/// gibt es nichts abzulesen, und die Rechnung trägt bei dieser Form auch
/// nichts, weil ohnehin das Byte-Budget entscheidet.
fn gerechnet(bytes: &[u8], faktor: u64) -> Option<u64> {
    let objektdecke_haelt = |n: u64| match prescan(
        bytes,
        &Limits {
            max_parsed_bytes: n,
            ..Limits::default()
        },
    ) {
        Ok(()) => true,
        Err(e) => !e.to_string().contains("gerechnet mehr"),
    };
    let mut lo = bytes.len() as u64;
    if objektdecke_haelt(lo) {
        return None;
    }
    let mut hi = lo.saturating_mul(2);
    while !objektdecke_haelt(hi) {
        lo = hi;
        hi = hi.checked_mul(2).expect("jenseits von u64");
    }
    while lo + 1 < hi {
        let m = lo + (hi - lo) / 2;
        if objektdecke_haelt(m) {
            hi = m;
        } else {
            lo = m;
        }
    }
    Some(hi.saturating_mul(faktor))
}

/// Der Faktor aus `document.rs`, hier noch einmal — er ist dort nicht
/// öffentlich, und vier Zeilen öffentliche API für einen Test wären der
/// falsche Handel. Läuft er auseinander, fällt der Test unten auf: die
/// gerechnete Zahl passte dann nicht mehr zur gemessenen.
const FAKTOR: u64 = 60;

// ---------------------------------------------------------------------------

#[test]
fn die_rechnung_liegt_nie_unter_dem_gemessenen_speicher() {
    // Alle Formen sind gleich groß (rund 800 Byte je Objektkörper, 4 MB je
    // Datei) — der Unterschied zwischen ihnen ist genau das, worum es geht.
    let formen = [
        "leere Arrays",
        "Zahlen",
        "Namen",
        "Dictionaries",
        "leere Dictionaries",
        "Verweise",
        "eine Zeichenkette",
    ];
    let mut engste = f64::MAX;
    let mut je_dateibyte: Vec<f64> = Vec::new();
    let mut gepruefte = 0usize;
    // Für die beiden Formen, aus denen `ARRAY_BYTES` und `OBJEKT_BYTES`
    // stammen: Byte je **Objekt**. Beide Körper sind 802 Byte lang und
    // enthalten 401 Objekte (ein äußeres Array und 400 Elemente).
    let mut je_objekt: Vec<(&str, f64)> = Vec::new();

    println!(
        "{:<20} {:>9} {:>12} {:>12} {:>10} {:>6}",
        "Form", "Datei", "gemessen", "gerechnet", "B/Datei-B", "Verh."
    );
    for name in formen {
        let bytes = rumpf(name, 4 * 1024 * 1024);

        let vorher = LIVE.load(Ordering::Relaxed);
        let doc = lopdf::Document::load_mem(&bytes).expect("ladbar");
        let gemessen = LIVE.load(Ordering::Relaxed).saturating_sub(vorher);
        drop(doc);
        je_dateibyte.push(gemessen as f64 / bytes.len() as f64);

        // Die Rechnung ist nur dort von außen ablesbar, wo die Objektdecke
        // wirklich bindet — bei Formen mit einem Eigenfaktor unter `FAKTOR`
        // entscheidet ohnehin das Byte-Budget.
        let rechnung = gerechnet(&bytes, FAKTOR);
        println!(
            "{name:<20} {:>9} {gemessen:>12} {:>12} {:>10.1} {:>6}",
            bytes.len(),
            rechnung.map(|r| r.to_string()).unwrap_or("—".into()),
            gemessen as f64 / bytes.len() as f64,
            rechnung
                .map(|r| format!("{:.2}", r as f64 / gemessen as f64))
                .unwrap_or("—".into())
        );
        if matches!(name, "leere Arrays" | "Zahlen") {
            let koerper = 4 * 1024 * 1024 / (802 + 40);
            je_objekt.push((name, gemessen as f64 / (koerper * 401) as f64));
        }
        if let Some(r) = rechnung {
            assert!(
                r >= gemessen as u64,
                "{name}: die Vorprüfung rechnet {r} Byte, `lopdf` belegt {gemessen} — \
                 eine Decke über einer zu kleinen Zahl ist keine Decke"
            );
            engste = engste.min(r as f64 / gemessen as f64);
            gepruefte += 1;
        }
    }
    assert!(gepruefte >= 5, "zu wenige Formen geprüft: {gepruefte}");

    // Die Rechnung darf auch nicht *beliebig* großzügig werden: sonst wäre die
    // Zusicherung oben wertlos, weil sie jede Datei ablehnte, lange bevor
    // Speicher entsteht. Gemessen liegt die engste Form bei 1,02.
    assert!(
        engste < 1.5,
        "keine Form liegt mehr dicht an der Rechnung (engste {engste:.2}) — \
         die Decke misst dann nicht mehr, was sie begrenzen soll"
    );

    // **Woher die beiden Beträge kommen.** Ein leeres Array kostet gemessen
    // 632 Byte, jedes andere Objekt 154 — Faktor 4, und genau dieses
    // Verhältnis steht in `ARRAY_BYTES` (640) und `OBJEKT_BYTES` (160).
    for (name, wert) in &je_objekt {
        println!("{name:<20} {wert:.1} Byte je Objekt");
    }
    let arrays = je_objekt
        .iter()
        .find(|(n, _)| *n == "leere Arrays")
        .unwrap()
        .1;
    let zahlen = je_objekt.iter().find(|(n, _)| *n == "Zahlen").unwrap().1;
    assert!(
        (600.0..=640.0).contains(&arrays),
        "ein leeres Array kostet {arrays:.1} Byte statt der 632, mit denen \
         ARRAY_BYTES (640) begründet ist"
    );
    assert!(
        (140.0..=160.0).contains(&zahlen),
        "ein gewöhnliches Objekt kostet {zahlen:.1} Byte statt der 154, mit \
         denen OBJEKT_BYTES (160) begründet ist"
    );

    // **Der Befund selbst, als Zusicherung.** Bei identischer Dateigröße
    // schwankt der Speicher je Dateibyte um mehr als das Hundertfache. Genau
    // deshalb kann eine Decke, die Dateibytes zählt, keinen Aufblähfaktor
    // unterstellen. Gemessen: 1,1 bis 301,6.
    let min = je_dateibyte.iter().cloned().fold(f64::MAX, f64::min);
    let max = je_dateibyte.iter().cloned().fold(0.0, f64::max);
    assert!(
        max / min > 100.0,
        "der Preis je Dateibyte schwankt nur um {:.0}× ({min:.1} bis {max:.1}) — \
         dann wäre ein Aufblähfaktor je Dateibyte plötzlich doch eine Zahl",
        max / min
    );
}
