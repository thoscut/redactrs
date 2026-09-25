//! Eine Dictionary-Bombe in einem **verschlüsselten Objekt-Stream** kam an den
//! Grenzen vorbei und lud fehlerfrei durch.
//!
//! # Der Befund (gemessen, Release)
//!
//! Die Vorprüfung der Rohbytes (`redact_pdf::document::prescan` in
//! `load_from_bytes_with_limits`) misst Streams als das, was sie in der Datei
//! sind. Bei einem RC4-verschlüsselten Objekt-Stream ist das Rauschen — sie
//! kann ihn nicht auspacken und sieht die Bombe nicht.
//!
//! `Document::load_mem_with_options` entschlüsselt den Objekt-Stream dann und
//! expandiert den darin steckenden Riesen-Dict in `document.objects` —
//! **bevor** die zweite Prüfung (`check_limits_after_decryption`) läuft. Und
//! diese zweite Prüfung serialisierte bisher nur über
//! `redact_pdf::document::save_to_bytes`, das **`prune_unreachable` vor dem
//! Serialisieren** ruft: ein Objekt, das nur über die Querverweistabelle
//! erreichbar ist und von keinem anderen referenziert wird — eine **Waise** —,
//! flog dabei heraus und wurde nie gemessen.
//!
//! Gemessen an einer als Waise gebauten Bombe (zwei Millionen Einträge, 4,7 MB
//! Datei): VmHWM **+824 MB**, und mit dem Vorgabebudget von 16 MB lief sie
//! **fehlerfrei durch**. Dieselbe Bombe *erreichbar* gebaut wurde bei genau
//! demselben Budget abgelehnt — der Unterschied war allein die Erreichbarkeit.
//!
//! # Die committete Fassung
//!
//! [`BOMBE`] ist die kleine, in den Baum gelegte Variante: RC4-verschlüsselt
//! (`/V 1 /R 2`, 40-Bit, Passwort `geheim123` — dieselbe Verschlüsselung wie
//! `verschluesselt.pdf`), ein einziger Dictionary aus 8 000 Einträgen mit
//! langen Schlüsseln, als **Waise** in einem Objekt-Stream. Lange Schlüssel,
//! weil flate den gemeinsamen Präfix wegkomprimiert (30 kB Datei), `lopdf` aber
//! jeden Schlüssel als eigenen `Vec<u8>` voller Länge im Speicher hält — die
//! entpackten Objekte ergeben rund 3,3 MB Syntax.
//!
//! # Die Abhilfe
//!
//! `check_limits_after_decryption` misst jetzt zuerst die **schon entpackten
//! Objekte im Speicher**, Waisen eingeschlossen, gegen dasselbe
//! `max_parsed_bytes`-Budget (`check_expanded_objects`). Was der Serialisierweg
//! wegschneidet, fällt hier auf.
//!
//! **Was das nicht heilt:** der Spitzenspeicher entsteht in
//! `load_mem_with_options`, bevor diese Prüfung beginnt; `lopdf` bietet keinen
//! Haken dagegen, und vor dem Entschlüsseln lässt sich der Stream nicht
//! einsehen. Die Datei wird jetzt **abgelehnt statt angenommen** und erreicht
//! die noch teurere Analyse gar nicht erst — der Rest ist eine Grenze von
//! `lopdf`, benannt in `check_expanded_objects`.

use redact_pdf::document::Limits;
use redact_pipeline::{load_document, Config, Secret};

/// Die kleine Waisen-Bombe. Passwort: `geheim123`.
const BOMBE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/testdata/bombe_objstm_verschluesselt.pdf"
));

/// Ein gewöhnliches, verschlüsseltes Dokument — für die Gegenprobe.
const HARMLOS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/testdata/verschluesselt.pdf"
));

const PASSWORT: &str = "geheim123";

fn config(parsed_mb: u64) -> Config {
    Config {
        password: Some(Secret::new(PASSWORT.to_string())),
        limits: Limits {
            max_parsed_bytes: parsed_mb * 1024 * 1024,
            ..Limits::default()
        },
        ..Config::default()
    }
}

/// **Der Befund.** Die verschlüsselte Waisen-Bombe wird abgelehnt statt geladen.
///
/// Der eigentliche Punkt steht im zweiten Teil: **es wird kein `Document`
/// zurückgegeben.** Vorher lief die Datei mit `Ok` durch (die Waise wurde vor
/// dem Messen weggeschnitten) — der Lauf hätte sie als gültiges, leeres
/// Dokument weiterverarbeitet.
///
/// Das Budget (2 MB) liegt weit über dem Rumpf der Datei — die Vorprüfung der
/// Rohbytes vor dem Entschlüsseln greift also nicht —, aber unter den 3,3 MB,
/// die der entpackte Dict ergibt. Die Ablehnung kommt damit nachweislich aus
/// der Prüfung *nach* dem Entschlüsseln.
#[test]
fn eine_verschluesselte_objstm_dictionary_bombe_wird_abgelehnt() {
    let err = load_document(BOMBE, &config(2))
        .expect_err("die Waisen-Bombe muss abgelehnt werden")
        .to_string();

    // Aus der Prüfung nach dem Entschlüsseln …
    assert!(
        err.contains("entschlüsselt gilt weiter"),
        "die Meldung stammt nicht aus der Prüfung nach dem Entschlüsseln: {err}"
    );
    // … und ausdrücklich aus der Messung der entpackten Objekte, nicht aus dem
    // Serialisierweg (der die Waise weggeschnitten hätte). Diese Zeile ist der
    // Mutationsnachweis: ohne den Aufruf von `check_expanded_objects` lädt die
    // Datei mit `Ok`, und schon `expect_err` oben schlägt fehl.
    assert!(
        err.contains("entpackten Objekte"),
        "die Ablehnung kommt nicht aus der In-Speicher-Messung: {err}"
    );
    assert!(err.contains("Budget"), "{err}");
}

/// **Gegenprobe 1.** Mit ausreichendem Budget lädt genau dieselbe Datei — die
/// Grenze lehnt nicht pauschal ab, sondern misst.
///
/// Ohne diese Zeile wäre der Test oben auch dann grün, wenn jede verschlüsselte
/// Datei abgelehnt würde.
#[test]
fn mit_ausreichendem_budget_laedt_dieselbe_datei() {
    let doc = load_document(BOMBE, &config(16))
        .expect("mit 16 MB Budget passt der entpackte Dict und die Datei lädt");
    assert_eq!(redact_pdf::page_count(&doc), 1);
}

/// **Gegenprobe 2.** Ein gewöhnliches verschlüsseltes Dokument läuft weiter
/// durch. Eine Grenze, die auch das ablehnt, ist keine Härtung, sondern ein
/// Ausfall.
#[test]
fn ein_gewoehnliches_verschluesseltes_dokument_laedt_weiter() {
    let doc = load_document(HARMLOS, &config(16)).expect("muss weiterhin laden");
    assert_eq!(redact_pdf::page_count(&doc), 1);
    // Und auch mit einem knappen Budget, unter dem die Bombe fällt: das
    // harmlose Dokument hat nur eine Handvoll kleiner Objekte.
    let doc = load_document(HARMLOS, &config(2)).expect("auch knapp muss es laden");
    assert_eq!(redact_pdf::page_count(&doc), 1);
}
