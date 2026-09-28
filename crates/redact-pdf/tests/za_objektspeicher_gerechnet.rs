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
//! Daher `ARRAY_BYTES = 640` und `OBJEKT_BYTES = 160`. Beide Konstanten sind
//! öffentlich, und jede „gerechnete“ Zahl in diesem Test wird **aus ihnen**
//! gebildet — nie als Literal getragen. Sonst bliebe der Test bei einer
//! Änderung in `document.rs` grün und druckte eine Zahl, die der Code nicht
//! mehr rechnet (gemessen: bei `OBJEKT_BYTES = 155` stünde hier weiter 800,
//! wo der Code 775 rechnet, und 775 läge **unter** den gemessenen 777,9).
//!
//! ## Objekt-Streams (Debug, 200 000 Objekte je Datei, Querverweis-Strom)
//!
//! Die Rohkörper oben legen ihre Objekte in ein Array. In einem `/ObjStm`
//! wird jedes Objekt dagegen **eigenständig** in `Document::objects`
//! eingetragen — eigener Platz im Baum, bei `<</a 0>>` ein eigenes
//! Dictionary — und der Querverweis-Strom hält je Objekt einen Eintrag. Das
//! ist eine andere Speicherform, und die Zusicherung „`OBJEKT_BYTES` liegt
//! über dem, was ein Objekt kostet“ muss auch dort gelten.
//!
//! | Form je Objekt | gerechnet | gemessen brutto | gemessen ohne Nutzlast | Verh. |
//! |---|---:|---:|---:|---:|
//! | `<</a 0>>` | 800 | 809,8 | **777,9** | 1,028 |
//! | `[]` | 960 | 771,2 | 745,9 | 1,29 |
//! | `0` | 480 | 290,1 | 265,9 | 1,81 |
//! | `/a` | 480 | 295,2 | 269,9 | 1,78 |
//! | `<<>>` | 480 | 293,3 | 265,9 | 1,81 |
//! | `1 0 R` | 800 | 294,5 | 265,9 | 3,01 |
//! | `(a)` | 480 | 300,2 | 273,9 | 1,75 |
//!
//! „Gerechnet“ ist hier nicht am Verhalten ablesbar (siehe den Kommentar im
//! Test), sondern nach der Regel von `walk` gebildet: zwei Kopfzahlen und der
//! Körper, `[` als `ARRAY_BYTES`, jedes andere Wort als `OBJEKT_BYTES` (die
//! Tabelle zeigt die Werte für 640 und 160). „Brutto“ ist alles, was
//! `lopdf` nach `load_mem` hält; „ohne Nutzlast“ zieht die Dateibytes ab, die
//! `lopdf` als Inhalt des Objekt- und des Querverweis-Stroms behält. Die
//! decken Byte-Budget und Dekompressionsbudget, nicht die Objektdecke — und
//! brutto hinge die Zahl an der Stellenzahl der Kopfzahlen, nicht an den
//! Objekten. Verglichen wird deshalb ohne Nutzlast. Brutto läge `<</a 0>>`
//! 1,2 % **über** der Rechnung; wer die Decke auch brutto halten will, braucht
//! `OBJEKT_BYTES = 162` (809,8 / 5 Wörter, aufgerundet).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use redact_pdf::document::{prescan, Limits, ARRAY_BYTES, OBJEKT_BYTES};

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

/// Dieselbe Art Objekt, `anzahl`-mal in **einem Objekt-Stream** (`/ObjStm`),
/// mit Querverweis-Strom — so, wie jede echte Datei mit Objekt-Streams
/// gebaut ist.
///
/// Unkomprimiert, damit die Datei zeigt, was `lopdf` sieht: der Strom wird
/// beim Laden ausgepackt, und jedes Objekt darin steht danach einzeln in
/// `Document::objects` — ein Objekt je Kopfpaar, nicht eines je Stream. Die
/// Vorprüfung sieht den Inhalt als Syntax und zählt je Objekt auch die beiden
/// Kopfzahlen (Nummer und Versatz) als Wörter mit.
///
/// Der Querverweis-Strom ist kein Beiwerk: je Objekt trägt er einen Eintrag
/// vom Typ 2, und den hält `lopdf` als `XrefEntry` im Dokument — ein Preis je
/// Objekt, der nicht aus der Syntax des Objekts kommt. Eine klassische
/// `xref`-Tabelle kennt keine Typ-2-Einträge; mit ihr fehlte diese Menge in
/// der Messung. Objektnummern beginnen hinter dem Gerüst und dem Container.
fn objstm(koerper: &[u8], anzahl: usize) -> Vec<u8> {
    let container = geruest().len() + 1;
    let erste = container + 1;
    let mut kopf = String::new();
    let mut inhalt = Vec::new();
    for k in 0..anzahl {
        kopf.push_str(&format!("{} {} ", erste + k, inhalt.len()));
        inhalt.extend_from_slice(koerper);
        inhalt.push(b'\n');
    }
    let mut strom = format!(
        "<< /Type /ObjStm /N {anzahl} /First {} /Length {} >>\nstream\n",
        kopf.len(),
        kopf.len() + inhalt.len()
    )
    .into_bytes();
    strom.extend_from_slice(kopf.as_bytes());
    strom.extend_from_slice(&inhalt);
    strom.extend_from_slice(b"\nendstream");
    let mut alle = geruest();
    alle.push(strom);

    let mut out: Vec<u8> = b"%PDF-1.5\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in alle.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    // Querverweis-Strom, `/W [1 4 4]`: Typ, Versatz bzw. Container, Generation
    // bzw. Index. Vier Byte für den Index, weil 200 000 nicht in zwei passen.
    let xref_id = erste + anzahl;
    let xref_offset = out.len();
    let mut eintraege: Vec<u8> = Vec::with_capacity((xref_id + 1) * 9);
    let eintrag = |e: &mut Vec<u8>, typ: u8, a: u32, b: u32| {
        e.push(typ);
        e.extend_from_slice(&a.to_be_bytes());
        e.extend_from_slice(&b.to_be_bytes());
    };
    eintrag(&mut eintraege, 0, 0, 0);
    for off in &offsets {
        eintrag(&mut eintraege, 1, *off as u32, 0);
    }
    for k in 0..anzahl {
        eintrag(&mut eintraege, 2, container as u32, k as u32);
    }
    eintrag(&mut eintraege, 1, xref_offset as u32, 0);
    out.extend_from_slice(
        format!(
            "{xref_id} 0 obj\n<< /Type /XRef /Size {} /W [1 4 4] /Root 1 0 R /Length {} >>\nstream\n",
            xref_id + 1,
            eintraege.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&eintraege);
    out.extend_from_slice(
        format!("\nendstream\nendobj\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes(),
    );
    out
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

/// Was die Vorprüfung für **ein** Objekt in einem Objekt-Stream rechnet —
/// nach der Regel von `walk`, mit den Konstanten aus `document.rs`: die zwei
/// Kopfzahlen (Nummer und Versatz) und jedes andere Wort des Körpers kosten
/// `OBJEKT_BYTES`; eine öffnende `[` kostet **stattdessen** `ARRAY_BYTES` und
/// zählt nicht noch einmal als Wort.
fn gerechnet_je_objekt(andere_woerter: u64, klammern: u64) -> u64 {
    (2 + andere_woerter) * OBJEKT_BYTES + klammern * ARRAY_BYTES
}

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

    // ---------------------------------------------------------- Objekt-Streams
    //
    // Dieselben Objekte, nur in einem `/ObjStm` verpackt. Hier ist die
    // Rechnung von außen **nicht** ablesbar: je Objekt kostet der Kopf
    // (Nummer und Versatz) rund 15 Dateibyte, und damit bleibt der
    // Eigenfaktor jeder Form unter `FAKTOR` — die Objektdecke bindet bei
    // diesen Dateien nie, das Byte-Budget entscheidet. Was trotzdem gelten
    // muss, ist die Zusicherung **je Wort**: `OBJEKT_BYTES` „liegt über“ dem,
    // was ein Objekt kostet. Ein Objekt-Stream ist die eine Bauart, bei der
    // ein einzelnes `<</a 0>>` als eigenständiges Objekt in `Document::objects`
    // landet — mit eigenem Platz im Baum und eigenem Dictionary —, und genau
    // das prüfen die Rohkörper oben nicht: dort stehen die Objekte in einem
    // Array.
    //
    // Gerechnet wird je Objekt wie in `walk` (siehe `gerechnet_je_objekt`):
    // die zwei Kopfzahlen und jedes Wort des Körpers als `OBJEKT_BYTES`, `[`
    // als `ARRAY_BYTES` — aus den Konstanten, nicht als Literal. Gemessen
    // wird der Speicher des Dokuments **ohne** die Nutzlast des Containers:
    // `lopdf` behält den Stream-Inhalt (Dateibytes), und die deckt das
    // Byte-Budget, nicht die Objektdecke.
    let anzahl = 200_000usize;
    // (Anzeige, Körper, Wörter im Körper außer `[`, öffnende `[`)
    let objstm_formen: [(&str, &[u8], u64, u64); 7] = [
        ("<</a 0>>", b"<</a 0>>", 3, 0), // `<<`, `/a`, `0`
        ("[]", b"[]", 0, 1),             // nur die `[`; `]` kostet nichts
        ("0", b"0", 1, 0),
        ("/a", b"/a", 1, 0),
        ("<<>>", b"<<>>", 1, 0),
        ("1 0 R", b"1 0 R", 3, 0), // drei Wörter, ein Objekt — gewollt
        ("(a)", b"(a)", 1, 0),
    ];
    println!();
    println!(
        "{:<12} {:>9} {:>12} {:>12} {:>10} {:>10} {:>6}",
        "ObjStm-Form", "Datei", "gemessen", "o. Nutzl.", "je Objekt", "gerechnet", "Verh."
    );
    let mut engste_objstm = f64::MAX;
    for (name, koerper, andere_woerter, klammern) in objstm_formen {
        let gerechnet_je_objekt = gerechnet_je_objekt(andere_woerter, klammern);
        let bytes = objstm(koerper, anzahl);
        let nutzlast = bytes.len() - geruest().iter().map(Vec::len).sum::<usize>();

        let vorher = LIVE.load(Ordering::Relaxed);
        let doc = lopdf::Document::load_mem(&bytes).expect("ladbar");
        let gemessen = LIVE.load(Ordering::Relaxed).saturating_sub(vorher);
        // Sonst misst der Test einen Strom, dessen Objekte nie angekommen sind.
        assert_eq!(
            doc.objects.len(),
            anzahl + geruest().len() + 2,
            "{name}: `lopdf` hat die Objekte des Streams nicht ausgepackt"
        );
        drop(doc);

        let ohne_nutzlast = gemessen.saturating_sub(nutzlast);
        let je_objekt = ohne_nutzlast as f64 / anzahl as f64;
        let verhaeltnis = gerechnet_je_objekt as f64 / je_objekt;
        println!(
            "{name:<12} {:>9} {gemessen:>12} {ohne_nutzlast:>12} {je_objekt:>10.1} \
             {gerechnet_je_objekt:>10} {verhaeltnis:>6.3}",
            bytes.len()
        );
        assert!(
            verhaeltnis >= 1.0,
            "{name} im Objekt-Stream: die Vorprüfung rechnet {gerechnet_je_objekt} Byte je \
             Objekt, `lopdf` belegt {je_objekt:.1} — OBJEKT_BYTES liegt hier unter dem \
             gemessenen Preis"
        );
        engste_objstm = engste_objstm.min(verhaeltnis);
    }
    println!("engste ObjStm-Form: {engste_objstm:.3}");
    // Wie bei den Rohkörpern: dicht genug, dass die Rechnung noch misst.
    assert!(
        engste_objstm < 1.5,
        "keine ObjStm-Form liegt mehr dicht an der Rechnung (engste {engste_objstm:.2})"
    );

    // **Woher die beiden Beträge kommen.** Ein leeres Array kostet gemessen
    // 632 Byte, jedes andere Objekt 154 — Faktor 4, und genau dieses
    // Verhältnis steht in `ARRAY_BYTES` und `OBJEKT_BYTES`. Die Obergrenzen
    // sind die Konstanten selbst: die Messung muss **unter** dem liegen, was
    // der Code je Objekt verbucht, sonst ist die Begründung der Konstante
    // hinfällig.
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
        (600.0..=ARRAY_BYTES as f64).contains(&arrays),
        "ein leeres Array kostet {arrays:.1} Byte statt der 632, mit denen \
         ARRAY_BYTES ({ARRAY_BYTES}) begründet ist"
    );
    assert!(
        (140.0..=OBJEKT_BYTES as f64).contains(&zahlen),
        "ein gewöhnliches Objekt kostet {zahlen:.1} Byte statt der 154, mit \
         denen OBJEKT_BYTES ({OBJEKT_BYTES}) begründet ist"
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
