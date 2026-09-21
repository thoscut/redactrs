//! Gegenprobe zu Register #41 / Vertrag V5 unter der Linse **falscher Alarm**.
//!
//! Die Korrektur legt neben `LeakCheck::findings` ein `LeakCheck::sites` mit
//! je einem `LeakSite { view, page, object }`. Diese Datei fragt nicht, ob
//! der Ort genug **kann** — sie fragt, ob er gewöhnliches Material
//! beschuldigt:
//!
//! * Meldet der Ort etwas, wo kein Fund ist (ein Ort ohne Satz)?
//! * Nennt er eine **Seite** oder ein **Objekt**, das der Satz nicht nennt —
//!   also eine Stelle, die der Detektor nie durchsucht hat? Eine Oberfläche,
//!   die dem Ort folgt, würde damit eine saubere Stelle anzeigen.
//! * Wird eine gewöhnliche Datei dadurch zur „nicht geprüften“ Datei
//!   (`unchecked`) — das ist im Programm Rückgabewert 3?
//!
//! Das Material ist deshalb absichtlich **unauffällig**: ein getaggtes
//! Dokument mit Barrierefreiheit (`/Lang`, `/MarkInfo`, `/StructTreeRoot` mit
//! `/Alt` und `/ActualText`) und einem `/DCTDecode`-Bild, und ein Scan ohne
//! jeden Text. Beides kommt so aus gewöhnlichen Programmen.
//!
//! Der Maßstab ist der **Satz** der Fundstelle: er ist die Ausgabe, die
//! `docs/pruefung.txt` Byte für Byte festhält, und er hat sich durch die
//! Korrektur nicht geändert. Sagt der Ort etwas anderes als der Satz, ist
//! einer von beiden falsch — und der Ort ist der neue.

mod common;

use lopdf::{dictionary, Object, Stream};
use redact_pdf::audit_bytes::{LeakSite, LeakView};
use redact_pdf::{leaks_many_within, LeakCheck};

use common::SECRET;

/// Ein Begriff, der in keinem der Dokumente vorkommt.
const ABWESEND: &str = "Zaphod Beeblebrox";

/// Ein Baseline-JPEG-Rumpf: SOI, ein APP0-Segment, EOI. Mehr braucht es
/// nicht — `/DCTDecode` ist ein Bildfilter, das Orakel packt ihn nicht aus.
/// Entscheidend ist, dass hier ein Strom steht, den es **nicht** lesen kann,
/// ohne deswegen „nicht geprüft“ zu melden.
const JPEG: &[u8] = &[
    0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00, 0x01,
    0x00, 0x01, 0x00, 0x00, 0xff, 0xd9,
];

// ---------------------------------------------------------------------------
// Material
// ---------------------------------------------------------------------------

/// Ein **gewöhnliches getaggtes Dokument**: Barrierefreiheit vollständig,
/// ein `/DCTDecode`-Bild auf der Seite, das Geheimnis genau einmal im
/// Seitentext — so sieht eine Rechnung vor der Schwärzung aus.
///
/// `/Alt` und `/ActualText` sind hier **harmlos**: sie tragen den Text, den
/// ein Vorleseprogramm braucht, und kein Geheimnis. Sonst wäre das Material
/// nicht gewöhnlich, sondern ein Versteck.
fn getaggt() -> Vec<u8> {
    let mut d = common::page(&[
        "Kontoinhaber: Max Mustermann",
        &format!("IBAN: {SECRET}"),
        "Rechnung 2026-0042",
    ]);
    let bild = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 8_i64,
                "Height" => 8_i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8_i64,
                "Filter" => "DCTDecode",
            },
            JPEG.to_vec(),
        )
        .with_compression(false),
    ));
    let elem = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "Figure",
        "Alt" => Object::string_literal("Unterschrift der Sachbearbeiterin"),
        "ActualText" => Object::string_literal("Rechnung 2026-0042"),
    }));
    let root = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructTreeRoot",
        "K" => vec![Object::Reference(elem)],
    }));
    d.catalog_set("StructTreeRoot", Object::Reference(root));
    d.catalog_set(
        "MarkInfo",
        Object::Dictionary(dictionary! { "Marked" => true }),
    );
    d.catalog_set("Lang", Object::string_literal("de-DE"));
    // Das Bild gehört in die Ressourcen der Seite, sonst ist es ein
    // verwaistes Objekt und das Dokument nicht mehr gewöhnlich.
    let resources = d.resources_id;
    if let Ok(dict) = d.doc.get_dictionary_mut(resources) {
        dict.set(
            "XObject",
            Object::Dictionary(dictionary! { "Im1" => Object::Reference(bild) }),
        );
    }
    d.finish()
}

/// Ein **Scan**: eine Seite, ein `/DCTDecode`-Bild, kein Text. Die Datei, für
/// die das Programm „keine Textzeilen“ meldet — und über die es trotzdem
/// keine Unwahrheit sagen darf.
fn scan() -> Vec<u8> {
    let mut d = common::page(&[]);
    let bild = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 8_i64,
                "Height" => 8_i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8_i64,
                "Filter" => "DCTDecode",
            },
            JPEG.to_vec(),
        )
        .with_compression(false),
    ));
    let resources = d.resources_id;
    if let Ok(dict) = d.doc.get_dictionary_mut(resources) {
        dict.set(
            "XObject",
            Object::Dictionary(dictionary! { "Im0" => Object::Reference(bild) }),
        );
    }
    d.set_content(b"q 595 0 0 297 0 500 cm /Im0 Do Q\n");
    d.finish()
}

// ---------------------------------------------------------------------------
// Messwerkzeug
// ---------------------------------------------------------------------------

/// Satz und Ort gepaart, je Suchbegriff — mit der Paarungsprüfung, auf der
/// die Zusicherung von `LeakCheck::sites` beruht.
fn gepaart(check: &LeakCheck, needles: usize) -> Vec<Vec<(String, LeakSite)>> {
    assert_eq!(
        check.sites.len(),
        check.findings.len(),
        "je Suchbegriff eine Ortsliste"
    );
    assert_eq!(check.findings.len(), needles, "je Eingabe ein Platz");
    check
        .findings
        .iter()
        .zip(&check.sites)
        .map(|(hits, sites)| {
            assert_eq!(
                hits.len(),
                sites.len(),
                "je Fundstelle ein Ort — sonst gehört sites[i] nicht zu findings[i]"
            );
            hits.iter().cloned().zip(sites.iter().copied()).collect()
        })
        .collect()
}

/// Der Ortsteil einer Meldung: alles vor `" ["`. Die Meldung hat die Form
/// `"{Ort} [{wie}]: …{Umgebung}…"`; die Umgebung stammt aus der Datei und
/// darf hier nicht mitgelesen werden.
fn ortsteil(message: &str) -> &str {
    message
        .split_once(" [")
        .map(|(ort, _)| ort)
        .unwrap_or(message)
}

/// Die Seitenzahl, die der **Satz** nennt (`"Seite 3"` am Anfang des Ortes).
fn seite_im_satz(message: &str) -> Option<usize> {
    ortsteil(message).strip_prefix("Seite ")?.parse().ok()
}

/// Die **letzte** Objekt-Id, die der Satz nennt — `"Objekt N G"`, auch in
/// Klammern (`"(Objekt 11 0)"`). Die letzte, weil ein Objekt in einem
/// Objekt-Stream beide nennt: erst den Container, dann das enthaltene, und
/// `LeakSite::object` sagt zu, das **enthaltene** zu nennen.
///
/// `None` heißt: der Satz nennt kein Objekt — oder eine Zahl, die nicht in
/// ihren Zahlentyp passt. Über den zweiten Fall sagt diese Datei nichts, er
/// gehört den Einheitentests von `object_id`.
fn objekt_im_satz(message: &str) -> Option<(u32, u16)> {
    let ort = ortsteil(message);
    let mut letzte = None;
    for (i, _) in ort.match_indices("Objekt ") {
        let rest = &ort[i + "Objekt ".len()..];
        let mut felder = rest.split(|c: char| !c.is_ascii_digit());
        let nummer = felder.next().unwrap_or("");
        let generation = felder.next().unwrap_or("");
        if let (Ok(n), Ok(g)) = (nummer.parse::<u32>(), generation.parse::<u16>()) {
            letzte = Some((n, g));
        }
    }
    letzte
}

// ---------------------------------------------------------------------------
// 1. Ein Ort ohne Fund gibt es nicht
// ---------------------------------------------------------------------------

/// Ein Begriff, der nicht vorkommt, bekommt **keinen** Ort — und eine
/// gewöhnliche Datei wird durch die Korrektur nicht „nicht geprüft“.
///
/// Der zweite Teil ist der Rückgabewert: `unchecked` ist im Programm
/// Rückgabewert 3, auch ohne Fund. Ein getaggtes Dokument mit einem
/// `/DCTDecode`-Bild darf das nicht auslösen.
#[test]
fn zi_a_1_ein_abwesender_begriff_hat_keinen_ort() {
    for (name, bytes) in [("getaggt", getaggt()), ("scan", scan())] {
        let check = leaks_many_within(&bytes, &[SECRET, ABWESEND], u64::MAX);
        let paare = gepaart(&check, 2);
        assert!(
            paare[1].is_empty(),
            "{name}: „{ABWESEND}“ steht nicht in der Datei, hat aber {} Fundstelle(n): {:?}",
            paare[1].len(),
            check.findings[1]
        );
        assert!(
            !check.literal[1],
            "{name}: ein abwesender Begriff hat nicht wörtlich getroffen"
        );
        assert!(
            check.unchecked.is_empty() && check.unchecked_places == 0,
            "{name}: gewöhnliche Datei, nichts darf ungeprüft bleiben — sonst ist \
             der Rückgabewert 3 ohne einen einzigen Fund: {:?}",
            check.unchecked
        );
    }
}

/// Der Scan ohne Text: kein Fund, kein Ort, keine ungeprüfte Stelle. Das
/// `/DCTDecode`-Bild ist ein Strom, den das Orakel nicht lesen kann — und
/// genau darüber darf es nicht klagen.
#[test]
fn zi_a_2_ein_scan_ohne_text_meldet_nichts() {
    let bytes = scan();
    let check = leaks_many_within(&bytes, &[SECRET, ABWESEND], u64::MAX);
    let paare = gepaart(&check, 2);
    for (i, begriff) in [SECRET, ABWESEND].iter().enumerate() {
        assert!(
            paare[i].is_empty(),
            "Scan ohne Text: „{begriff}“ kommt nicht vor, gemeldet wird {:?}",
            check.findings[i]
        );
    }
    assert!(
        check.unchecked.is_empty(),
        "ein Bild ist keine ungeprüfte Stelle: {:?}",
        check.unchecked
    );
}

// ---------------------------------------------------------------------------
// 2. Kein Ort widerspricht seinem Satz
// ---------------------------------------------------------------------------

/// Der Ort sagt nie etwas anderes als der Satz.
///
/// Der Satz ist die Ausgabe, die `docs/pruefung.txt` Byte für Byte festhält;
/// er hat sich durch die Korrektur nicht geändert. Nennt der Ort eine andere
/// Seite oder ein anderes Objekt, zeigt eine Oberfläche, die ihm folgt, eine
/// Stelle an, über die der Detektor nichts gesagt hat.
#[test]
fn zi_a_3_der_ort_sagt_dasselbe_wie_sein_satz() {
    let material = [
        ("getaggt", getaggt()),
        ("Objekt-Stream", common::object_stream(SECRET)),
        (
            "Verkettung im Strom",
            common::filtered_stream(SECRET, "FlateDecode"),
        ),
        ("ActualText", common::struct_elem_actual_text(SECRET)),
    ];
    let mut geprueft = 0_usize;
    for (name, bytes) in material {
        let check = leaks_many_within(&bytes, &[SECRET], u64::MAX);
        let paare = gepaart(&check, 1);
        assert!(
            !paare[0].is_empty(),
            "{name}: das Orakel findet den Klartext"
        );
        for (satz, ort) in &paare[0] {
            geprueft += 1;
            assert_eq!(
                ort.page,
                seite_im_satz(satz),
                "{name}: Seite im Ort und im Satz — „{satz}“, Ort {ort:?}"
            );
            if let Some(id) = objekt_im_satz(satz) {
                assert_eq!(
                    ort.object,
                    Some(id),
                    "{name}: der Satz nennt Objekt {id:?}, der Ort ein anderes — „{satz}“"
                );
            } else {
                assert_eq!(
                    ort.object, None,
                    "{name}: der Satz nennt kein Objekt, der Ort schon — „{satz}“"
                );
            }
        }
    }
    assert!(
        geprueft >= 8,
        "zu wenig Fundstellen geprüft ({geprueft}) — die Zusicherung wäre wertlos"
    );
}

// ---------------------------------------------------------------------------
// 3. Nur die Sicht, die Seiten kennt, nennt eine Seite
// ---------------------------------------------------------------------------

/// Eine Seitenzahl gibt es genau in der Sicht, die über Seiten läuft.
///
/// Andernfalls trüge ein Fund in einem Strom, den mehrere Seiten benutzen,
/// eine Seitenzahl, die für ihn nicht gilt — eine Zusicherung, mitgenommen
/// an die nächste Stelle, wo sie nicht mehr stimmt.
#[test]
fn zi_a_4_eine_seite_nennt_nur_der_schriftdekoder() {
    let material = [
        ("getaggt", getaggt()),
        ("Objekt-Stream", common::object_stream(SECRET)),
        ("Altrevision", common::incremental_history(SECRET, "XXXX")),
    ];
    let mut mit_seite = 0_usize;
    for (name, bytes) in material {
        let check = leaks_many_within(&bytes, &[SECRET], u64::MAX);
        let paare = gepaart(&check, 1);
        for (satz, ort) in &paare[0] {
            if ort.page.is_some() {
                mit_seite += 1;
                assert_eq!(
                    ort.view,
                    LeakView::FontDecoder,
                    "{name}: nur der Schriftdekoder kennt eine Seite — „{satz}“"
                );
                assert_eq!(
                    ort.object, None,
                    "{name}: der Schriftdekoder liest eine Seite, kein Objekt — „{satz}“"
                );
            } else {
                assert_ne!(
                    ort.view,
                    LeakView::FontDecoder,
                    "{name}: der Schriftdekoder weiß immer, auf welcher Seite er liest \
                     — „{satz}“"
                );
            }
        }
    }
    assert!(
        mit_seite >= 1,
        "kein einziger Fund des Schriftdekoders geprüft ({mit_seite})"
    );
}

/// Die Nummer einer Sicht ist die des Modulkopfs — eine Oberfläche, die
/// „Sicht 3“ schreibt, muss dasselbe meinen wie die Doku.
#[test]
fn zi_a_5_die_sichten_tragen_die_nummern_des_modulkopfs() {
    let erwartet = [
        (LeakView::RawFile, 1),
        (LeakView::RawStream, 2),
        (LeakView::Stream, 3),
        (LeakView::ObjectStream, 4),
        (LeakView::StringObject, 5),
        (LeakView::StringConcat, 6),
        (LeakView::FontDecoder, 7),
    ];
    for (sicht, nummer) in erwartet {
        assert_eq!(sicht.number(), nummer, "{sicht:?}");
    }
}
