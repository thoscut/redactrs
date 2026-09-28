//! Gegenprobe zu Register #41 / Vertrag V5: taugt der **maschinenlesbare**
//! Ort einer Fundstelle für die Entscheidung, für die er eingeführt wurde?
//!
//! Die Korrektur legt neben `LeakCheck::findings` ein `LeakCheck::sites` mit
//! je einem `LeakSite { view, page, object }`. Ihre Begründung ist eine
//! Sicherheitsfolge: die Oberfläche soll „gewollt stehen geblieben“ von
//! „Schwärzung danebengegangen“ trennen können, ohne den Meldungstext zu
//! raten. `LeakSite` sagt dazu zu: „Gefüllt wird, was die Sicht **weiß**;
//! geraten wird nichts“, und zu `object`: „Objektnummer und Generation, wie
//! `lopdf` sie zählt.“
//!
//! Diese Datei prüft die Zusage an eigenem Material — nicht am Satz, gegen
//! den `tests/zh_a_ort_maschinenlesbar.rs` vergleicht. Denn Satz und Ort
//! kommen aus **derselben** Quelle: sagt die Quelle etwas Falsches, sagen
//! beide dasselbe Falsche, und ein Vergleich der beiden merkt nichts.
//!
//! Der Massstab hier ist deshalb das geladene Dokument: **trägt** das Objekt,
//! das der Ort nennt, den gefundenen Text wirklich?
//!
//! ## Was dabei herauskommt
//!
//! * `zi_a1` — **gewöhnliche Datei, inkrementelles Update** (jedes „Speichern“
//!   in Acrobat schreibt eines): der Klartext steht in der **alten** Revision,
//!   die neue Revision hat an derselben Objekt-Id den geschwärzten Text. Der
//!   einzige Ort, der überhaupt eine Objekt-Id nennt, nennt genau diese Id —
//!   und das Objekt dahinter ist **sauber**. Wer dem Ort folgt, landet auf
//!   „geschwärzt, also gewollt stehen geblieben“, während der Klartext
//!   unverändert in der Datei liegt.
//! * `zi_a2` — **Gegenrichtung**: in einer Datei ohne Altrevision stimmt die
//!   Id. Die Prüfung oben ist also nicht kaputt, sondern trifft eine Lage.
//! * `zi_a3` — der **Vertrag**, wie die Doku ihn formuliert, über beide
//!   Lagen. Er ist `#[ignore]`: der Lauf
//!   `cargo test -p redact-pdf --test zi_a_ort_gegenprobe -- --ignored`
//!   ist der Beleg des offenen Befundes und soll das Gate nicht rot machen.

mod common;

use lopdf::{dictionary, Document, Object, Stream};
use redact_pdf::audit_bytes::LeakView;
use redact_pdf::leaks_many_within;

use common::SECRET;

/// Der geschwärzte Ersatztext der neuen Revision — gleiche Länge, kein Klartext.
const ERSATZ: &str = "XXXX XXXX XXXX XXXX XXXX XX";

// ---------------------------------------------------------------------------
// Messwerkzeug: trägt das genannte Objekt den Text?
// ---------------------------------------------------------------------------

fn enthaelt(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Steht `needle` in diesem Objekt — in einer Zeichenkette, einem Namen, einem
/// Strom (roh oder entpackt) oder irgendwo darunter?
fn objekt_traegt(object: &Object, needle: &[u8]) -> bool {
    match object {
        Object::String(raw, _) => enthaelt(raw, needle),
        Object::Name(name) => enthaelt(name, needle),
        Object::Array(items) => items.iter().any(|i| objekt_traegt(i, needle)),
        Object::Dictionary(dict) => dict.iter().any(|(_, v)| objekt_traegt(v, needle)),
        Object::Stream(stream) => {
            enthaelt(&stream.content, needle)
                || stream
                    .decompressed_content()
                    .is_ok_and(|data| enthaelt(&data, needle))
                || stream.dict.iter().any(|(_, v)| objekt_traegt(v, needle))
        }
        _ => false,
    }
}

/// Trägt das Objekt mit dieser Id im **geladenen** Dokument den Text?
///
/// `false` heißt auch: es gibt gar keines mit dieser Id.
fn traegt(doc: &Document, id: (u32, u16), needle: &str) -> bool {
    doc.objects
        .get(&id)
        .is_some_and(|object| objekt_traegt(object, needle.as_bytes()))
}

/// Satz und Ort gepaart, für den ersten Suchbegriff.
fn funde(bytes: &[u8]) -> Vec<(String, redact_pdf::audit_bytes::LeakSite)> {
    let check = leaks_many_within(bytes, &[SECRET], u64::MAX);
    assert_eq!(check.sites.len(), check.findings.len());
    assert_eq!(check.findings[0].len(), check.sites[0].len());
    check.findings[0]
        .iter()
        .cloned()
        .zip(check.sites[0].iter().copied())
        .collect()
}

// ---------------------------------------------------------------------------
// Material 2: ein unkomprimiert eingebettetes PDF (PDF/A-3, ZUGFeRD)
// ---------------------------------------------------------------------------

/// Eine gewöhnliche Datei mit einem **eingebetteten PDF** ohne Filter.
///
/// Das innere Dokument hat eine eigene Objektzählung. Sein zweiter Strom trägt
/// den Klartext, und sein Kopf nennt absichtlich die Nummer der **Seite** des
/// äußeren Dokuments: so ist nachprüfbar, dass der Ort eine Id aus einer
/// fremden Zählung übernimmt.
fn eingebettetes_pdf(secret: &str) -> (Vec<u8>, (u32, u16)) {
    let mut d = common::page(&["Rechnung 2026-0042", "Anhang: rechnung.pdf"]);
    let seite = d.page_id;
    let nutzlast = format!("IBAN: {secret}");
    let inner = format!(
        "%PDF-1.4\n\
         1 0 obj\n<< /Length 8 >>\nstream\nharmlos!\nendstream\nendobj\n\
         {} 0 obj\n<< /Length {} >>\nstream\n{nutzlast}\nendstream\nendobj\n\
         %%EOF\n",
        seite.0,
        nutzlast.len()
    );
    let datei = d.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "EmbeddedFile",
            "Subtype" => "application/pdf",
        },
        inner.into_bytes(),
    )));
    let spec = d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("rechnung.pdf"),
        "EF" => dictionary! { "F" => Object::Reference(datei) },
    }));
    let baum = d.add(Object::Dictionary(dictionary! {
        "Names" => vec![Object::string_literal("rechnung.pdf"), Object::Reference(spec)],
    }));
    d.catalog_set(
        "Names",
        Object::Dictionary(dictionary! { "EmbeddedFiles" => Object::Reference(baum) }),
    );
    (d.finish(), seite)
}

// ---------------------------------------------------------------------------
// zi_a1 — der Befund
// ---------------------------------------------------------------------------

/// **Befund.** In einer Datei mit inkrementellem Update nennt der einzige Ort
/// mit einer Objekt-Id ein Objekt, das den Fund **nicht** trägt.
///
/// Dieser Test hält den heutigen Stand fest, damit er sichtbar ist. Wird der
/// Befund behoben — etwa indem die Rohsicht die Id nur dann nennt, wenn der
/// Block im geladenen Dokument auch zu diesem Objekt gehört —, wird er rot und
/// gehört dann gelöscht; `zi_a3` ist die Fassung, die danach gelten soll.
#[test]
fn zi_a1_altrevision_der_ort_nennt_ein_sauberes_objekt() {
    let pdf = common::incremental_history(SECRET, ERSATZ);
    let doc = Document::load_mem(&pdf).expect("die neue Revision ist ladbar");
    let funde = funde(&pdf);
    assert!(!funde.is_empty(), "der Klartext muss gefunden werden");

    // Der Klartext steht wirklich noch in der Datei — das Orakel ist nicht
    // blind, nur sein Ort ist es.
    assert!(
        funde.iter().any(|(text, _)| text.contains("Rohdatei @0x")),
        "{funde:#?}"
    );

    let mit_id: Vec<_> = funde.iter().filter(|(_, s)| s.object.is_some()).collect();
    assert!(
        !mit_id.is_empty(),
        "genau diese Orte sind der Gegenstand: {funde:#?}"
    );

    for (text, site) in &mit_id {
        let id = site.object.expect("gefiltert");
        // Nur die Rohsicht kann hier eine Id nennen — und die Verkettung, die
        // auf demselben Rohblock liest und seinen Ort mitnimmt.
        assert!(
            matches!(site.view, LeakView::RawStream | LeakView::StringConcat),
            "unerwartete Sicht {:?}: {text}",
            site.view
        );
        assert!(
            doc.objects.contains_key(&id),
            "die Id nennt ein Objekt, das es im aktuellen Dokument gibt: {text}"
        );
        assert!(
            !traegt(&doc, id, SECRET),
            "BEFUND WEG? Objekt {id:?} trägt den Klartext doch: {text}"
        );
        // Und dieses Objekt trägt statt des Klartextes den **geschwärzten**
        // Text: wer dem Ort folgt, liest „geschwärzt, also gewollt“.
        assert!(
            traegt(&doc, id, ERSATZ),
            "Objekt {id:?} trägt den Ersatztext: {text}"
        );
    }

    // Keine Seite, kein Rechteck, kein Offset: außer der Sicht bleibt dem
    // Aufrufer nichts als der Satz — der Offset `@0x…` steht **nur** dort.
    assert!(funde.iter().all(|(_, s)| s.page.is_none()), "{funde:#?}");
    assert!(
        funde
            .iter()
            .all(|(text, _)| text.starts_with("Rohdatei @0x")
                || text.starts_with("Rohdaten-Stream @0x")),
        "{funde:#?}"
    );
}

// ---------------------------------------------------------------------------
// zi_a2 — Gegenrichtung
// ---------------------------------------------------------------------------

/// **Gegenrichtung.** Ohne Altrevision stimmt die Id: derselbe Maßstab, andere
/// Antwort. Ohne diesen Test wäre `zi_a1` bloß eine kaputte Messung.
#[test]
fn zi_a2_ohne_altrevision_nennt_der_ort_den_traeger() {
    let pdf = common::page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]).finish();
    let doc = Document::load_mem(&pdf).expect("ladbar");
    let funde = funde(&pdf);
    assert!(!funde.is_empty());

    let mut geprueft = 0usize;
    for (text, site) in &funde {
        if let Some(id) = site.object {
            assert!(
                traegt(&doc, id, SECRET),
                "hier muss die Id den Träger nennen: {text}"
            );
            geprueft += 1;
        }
    }
    assert!(geprueft > 0, "keine Id geprüft: {funde:#?}");
}

// ---------------------------------------------------------------------------
// zi_a3 — der Vertrag, wie die Doku ihn formuliert
// ---------------------------------------------------------------------------

/// **Der Vertrag.** „Objektnummer und Generation, wie `lopdf` sie zählt“ — und
/// „geraten wird nichts“. Also: nennt ein Ort eine Id, trägt dieses Objekt im
/// geladenen Dokument den Fund.
///
/// Rot an zwei gewöhnlichen Lagen:
///
/// 1. **inkrementelles Update** — die Id der alten Revision zeigt auf das neue,
///    geschwärzte Objekt;
/// 2. **eingebettetes PDF ohne Filter** (PDF/A-3, ZUGFeRD) — der Kopf im
///    inneren Dokument gehört zu einer **fremden** Objektzählung; übernommen
///    wird er trotzdem, hier auf die Nummer der Seite des äußeren Dokuments.
///
/// Beides steht nur in Rohbytes; `object_header` liest 64 KiB zurück bis zum
/// letzten `N G obj` und fragt das geladene Dokument nicht.
#[test]
#[ignore = "offener Befund: die Rohsicht übernimmt eine Objekt-Id aus Rohbytes, ohne sie im Dokument zu prüfen"]
fn zi_a3_vertrag_eine_genannte_objekt_id_nennt_den_traeger() {
    let (eingebettet, seite) = eingebettetes_pdf(SECRET);
    let lagen: Vec<(&str, Vec<u8>)> = vec![
        (
            "inkrementelles Update",
            common::incremental_history(SECRET, ERSATZ),
        ),
        ("eingebettetes PDF", eingebettet),
    ];

    let mut klagen: Vec<String> = Vec::new();
    for (name, pdf) in &lagen {
        let doc = Document::load_mem(pdf).expect("ladbar");
        for (text, site) in funde(pdf) {
            if let Some(id) = site.object {
                if !traegt(&doc, id, SECRET) {
                    klagen.push(format!(
                        "{name}: Ort nennt Objekt {id:?}, das den Fund nicht trägt — {text}"
                    ));
                }
            }
        }
    }
    assert!(
        klagen.is_empty(),
        "die Seite des äußeren Dokuments ist {seite:?}:\n{}",
        klagen.join("\n")
    );
}

/// Hilfslauf (ignoriert): druckt Satz und Ort beider Lagen, damit im Bericht
/// steht, was das Orakel wirklich liefert.
#[test]
#[ignore = "Hilfslauf: druckt nur"]
fn zi_a4_dump() {
    let (eingebettet, seite) = eingebettetes_pdf(SECRET);
    println!("Seite des aeusseren Dokuments: {seite:?}");
    for (name, pdf) in [
        ("ALTREVISION", common::incremental_history(SECRET, ERSATZ)),
        ("EINGEBETTET", eingebettet),
    ] {
        let doc = Document::load_mem(&pdf).expect("ladbar");
        println!("=== {name} ===");
        for (text, site) in funde(&pdf) {
            let ok = site
                .object
                .map(|id| traegt(&doc, id, SECRET))
                .map(|b| if b { "TRAEGT" } else { "TRAEGT-NICHT" })
                .unwrap_or("ohne-id");
            println!(
                "  {:?} page={:?} object={:?} [{ok}]  {text}",
                site.view, site.page, site.object
            );
        }
    }
}
