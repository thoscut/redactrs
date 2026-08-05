//! Was die Tiefenprüfung in **binärer** Nutzlast messen soll — und was nicht.
//!
//! [`redact_pdf::document::prescan`] zählt die Klammertiefe nicht nur in der
//! Datei selbst, sondern auch in jedem ausgepackten Strom. In PDF-Syntax ist
//! das eine sinnvolle Messgröße; in Nutzlast war es keine.
//!
//! ## Der Fehler
//!
//! In Nutzlast sind `[` und `]` gewöhnliche Datenbytes. Der Zähler sank mit
//! `saturating_sub` nur bis null, lief also in Rauschen nach oben davon: sein
//! Höchststand wuchs mit der **Länge** der Nutzlast, nicht mit ihrer Struktur.
//! Die Grenze von 256 war damit in Wahrheit eine Größengrenze — und eine, die
//! nach Bytemustern ohne Bedeutung mal zuschlug und mal nicht.
//!
//! Getroffen hat es den gewöhnlichsten großen Strom, den es gibt: den
//! **Querverweis-Strom** (`/W [1 4 2]`), den `lopdf` ab PDF 1.5 selbst
//! schreibt. Sieben Byte je Eintrag, darin Offsets — und in denen kommen
//! `[`-Bytes (0x5B) rein zufällig vor. Gemessen an 128 Dateigrößen zwischen
//! 0,25 MB und 32 MB, abgelehnt wurden:
//!
//! | Einträge | Nutzlast | vorher | nachher |
//! |---------:|---------:|-------:|--------:|
//! |   20 000 |  140 kB  |   0    |    0    |
//! |   50 000 |  350 kB  |   1    |    0    |
//! |  100 000 |  700 kB  |  12    |    0    |
//! |  200 000 |  1,4 MB  |  35    |    0    |
//!
//! ## Die Antwort
//!
//! Eine Verschachtelung, die `lopdf` überhaupt in die Tiefe führt, ist ein
//! **zusammenhängender Lauf gültiger Syntax**: nach jedem `[` muss ein Objekt
//! folgen, sonst bricht der Parser ab und verschachtelt nichts. Also zählt in
//! Nutzlast nur noch, was diese Form hat. Rauschen daneben zählt nicht mehr
//! mit — es setzt den Zähler zurück.
//!
//! Beide Richtungen stehen hier nebeneinander, weil nur beide zusammen etwas
//! aussagen: eine Prüfung, die nichts mehr ablehnt, wäre genauso falsch wie
//! eine, die alles ablehnt.

use redact_pdf::document::{prescan, Limits};

// ---------------------------------------------------------------------------
// Werkzeug
// ---------------------------------------------------------------------------

/// Die Nutzlast eines Querverweis-Stroms, Byte für Byte so, wie `lopdf` sie
/// schreibt: Typ (1 Byte), Offset (4 Byte groß-endian), Generation (2 Byte).
fn xref_nutzlast(eintraege: usize, dateigroesse: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(eintraege * 7);
    for k in 0..eintraege {
        let offset = (dateigroesse * k as u64 / eintraege as u64) as u32;
        out.push(1);
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
    }
    out
}

/// Eine Datei mit genau einem unkomprimierten Strom.
fn datei_mit_strom(dict: &str, payload: &[u8]) -> Vec<u8> {
    let mut raw = format!(
        "%PDF-1.7\n1 0 obj\n<< {dict} /Length {} >>\nstream\n",
        payload.len()
    )
    .into_bytes();
    raw.extend_from_slice(payload);
    raw.extend_from_slice(b"\nendstream\nendobj\n");
    raw
}

fn tiefe_beanstandet(bytes: &[u8]) -> bool {
    match prescan(bytes, &Limits::default()) {
        Ok(()) => false,
        Err(e) => e.to_string().contains("Verschachtelungstiefe"),
    }
}

// ---------------------------------------------------------------------------
// Verfügbarkeit: gewöhnliche Dateien laufen durch
// ---------------------------------------------------------------------------

/// Ein Querverweis-Strom ist keine Verschachtelungsbombe — auch nicht zufällig.
///
/// Der Test führt 96 gewöhnliche Ströme vor: drei Objektzahlen, wie sie große
/// echte Dokumente haben, mal 32 Dateigrößen. **Keiner** davon darf an der
/// Tiefenprüfung scheitern. Vor der Korrektur scheiterten mehrere — welche,
/// hing allein daran, wo die Objekte zufällig lagen.
#[test]
fn ein_querverweis_strom_ist_keine_verschachtelung() {
    let mut abgelehnt = Vec::new();
    for eintraege in [50_000usize, 100_000, 200_000] {
        for mb in 1..=32u64 {
            let groesse = mb * 1024 * 1024;
            let raw = datei_mit_strom("/Type /XRef /W [1 4 2]", &xref_nutzlast(eintraege, groesse));
            if tiefe_beanstandet(&raw) {
                abgelehnt.push(format!("{eintraege} Einträge / {mb} MB"));
            }
        }
    }
    assert!(
        abgelehnt.is_empty(),
        "gewöhnliche Querverweis-Ströme an der Tiefenprüfung gescheitert: {abgelehnt:?}"
    );
}

/// Und dasselbe für die übrige Nutzlast, die jede echte Datei mitbringt.
#[test]
fn gewoehnliche_nutzlast_laeuft_durch() {
    let mut zufall = Vec::with_capacity(6_200_000);
    let mut z = 0x1234_5678u32;
    for _ in 0..6_200_000 {
        z = z.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        zufall.push((z >> 16) as u8);
    }
    // Eine schriftartige Nutzlast: ASCII-Tabellenmarken zwischen Binärdaten.
    let mut schrift = Vec::new();
    for k in 0..40_000u32 {
        schrift.extend_from_slice(b"glyf");
        schrift.extend_from_slice(&k.to_be_bytes());
        schrift.extend_from_slice(b"[loca]");
        schrift.extend_from_slice(&(k * 7).to_le_bytes());
    }
    for (name, payload) in [
        ("6,2 MB gleichverteiltes Rauschen", zufall),
        (
            "2 MB Verlauf",
            (0..2_000_000u32).map(|i| (i * 37 % 256) as u8).collect(),
        ),
        (
            "2 MB Sägezahn",
            (0..2_000_000u32).map(|i| (i % 251) as u8).collect(),
        ),
        ("schriftartig", schrift),
    ] {
        assert!(
            !tiefe_beanstandet(&datei_mit_strom("", &payload)),
            "{name} an der Tiefenprüfung gescheitert"
        );
    }
}

// ---------------------------------------------------------------------------
// Sicherheit: eine getarnte Bombe bleibt eine Bombe
// ---------------------------------------------------------------------------

/// Rauschen ist keine Tarnkappe.
///
/// Jede dieser sieben Nutzlasten gilt der Vorprüfung als Binärdaten (bis auf
/// die erste, die ohne Tarnung auskommt) und trägt eine Verschachtelung, die
/// `lopdf` stillschweigend fallen ließe. Alle sieben müssen abgelehnt werden.
///
/// Die letzten drei sind die interessanten: `[/…` mit einem Byte über 126
/// dahinter und `[` mit einem Nullbyte dahinter sind für `lopdf` gültige
/// Syntax (`is_regular` lässt hohe Bytes in Namen zu, NUL zählt als
/// Zwischenraum) — eine Rückstellung, die stur auf „nicht druckbar“ hört,
/// ließe genau sie durch.
#[test]
fn getarnte_verschachtelung_wird_weiterhin_abgelehnt() {
    let rauschen: Vec<u8> = (0..80_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let bombe = |n: usize| -> Vec<u8> {
        let mut v = vec![b'['; n];
        v.extend(std::iter::repeat_n(b']', n));
        v
    };

    let faelle: Vec<(&str, Vec<u8>)> = vec![
        ("ohne Tarnung", bombe(1000)),
        ("Rauschen davor", [rauschen.clone(), bombe(1000)].concat()),
        (
            "Rauschen dahinter",
            [bombe(1000), rauschen.clone()].concat(),
        ),
        ("Zahlen zwischen den Klammern", {
            let mut v = rauschen.clone();
            for _ in 0..1000 {
                v.extend_from_slice(b"[0 ");
            }
            v.extend(std::iter::repeat_n(b']', 1000));
            v
        }),
        ("Namen aus hohen Bytes", {
            let mut v = Vec::new();
            for _ in 0..1000 {
                v.extend_from_slice(&[b'[', b'/', 0xC7]);
            }
            v.extend(std::iter::repeat_n(b']', 1000));
            v
        }),
        ("Nullbytes zwischen den Klammern", {
            let mut v = Vec::new();
            for _ in 0..1000 {
                v.extend_from_slice(&[b'[', 0x00]);
            }
            v.extend(std::iter::repeat_n(b']', 1000));
            v
        }),
        ("Dictionaries statt Arrays", {
            let mut v = rauschen.clone();
            for _ in 0..1000 {
                v.extend_from_slice(b"<<");
            }
            v
        }),
    ];

    for (name, payload) in faelle {
        assert!(
            tiefe_beanstandet(&datei_mit_strom("", &payload)),
            "Tarnung „{name}“ ist durchgekommen"
        );
    }
}
