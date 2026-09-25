//! Gegenprüfung (Linse Scheintest) zu Register #41 / Vertrag V5: **Sicht 4**
//! des maschinenlesbaren Ortes war von keinem Test gedeckt.
//!
//! `LeakSite::object` sagt zu, bei [`LeakView::ObjectStream`] das
//! **enthaltene** Objekt zu nennen — „dort steht der Text; welcher Container es
//! trägt, sagt der Meldungstext“. Genau diese Zusage hielt kein Test:
//!
//! * `tests/zh_a_ort_maschinenlesbar.rs::zh_a3` vergleicht Ort und Satz mit
//!   `ort.contains("Objekt N G")`. Der Satz eines Fundes in einem Objekt-Strom
//!   nennt **beide** Ids („Objekt 8 0 <ObjStm> → Objekt 9 0/ActualText …“),
//!   also ist diese Prüfung auch mit der Container-Id erfüllt: die zwei Fälle,
//!   die auseinandergehalten werden müssen, sind für sie derselbe.
//! * Nimmt man `.with_object((id.0, id.1))` aus der `<ObjStm>`-Schleife heraus,
//!   sodass der Ort den **Container** nennt, bleiben alle 251 Tests von
//!   `--lib`, `zh_a_ort_maschinenlesbar`, `leak_detector` und
//!   `zb_orakel_schriftdekoder` grün.
//!
//! Warum das mehr ist als Kosmetik: eine Oberfläche folgt dem Ort, um „gewollt
//! stehen geblieben“ von „Schwärzung danebengegangen“ zu trennen. Zeigt er auf
//! den Container, landet sie auf dem gepackten Strom statt auf dem Feld, in dem
//! der Klartext steht — genau das Raten, das der Ort abschaffen sollte.
//!
//! ## Der Maßstab, und was er nicht kann
//!
//! Naheliegend wäre, allein am geladenen Dokument zu messen: trägt das genannte
//! Objekt den Fund? Das ist **notwendig, aber nicht hinreichend** — `lopdf`
//! hält den Objekt-Strom nach dem Laden **entpackt**, also trägt auch der
//! Container den Klartext, und die Frage „Container oder Inhalt?“ bleibt offen.
//! Entschieden wird sie deshalb an den beiden Ids, die der Satz selbst nennt:
//! der Ort muss die **hintere** nennen (das enthaltene Objekt), nicht die
//! vordere. Geprüft wird beides.

mod common;

use lopdf::{Document, Object};
use redact_pdf::audit_bytes::LeakView;
use redact_pdf::leaks_many_within;

use common::SECRET;

/// Trägt dieses Objekt den Text — in einer Zeichenkette, einem Namen oder den
/// Strombytes?
fn objekt_traegt(object: &Object, needle: &[u8]) -> bool {
    let enthaelt = |hay: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
    match object {
        Object::String(raw, _) => enthaelt(raw),
        Object::Name(name) => enthaelt(name),
        Object::Array(items) => items.iter().any(|i| objekt_traegt(i, needle)),
        Object::Dictionary(dict) => dict.iter().any(|(_, v)| objekt_traegt(v, needle)),
        Object::Stream(stream) => {
            enthaelt(&stream.content) || stream.dict.iter().any(|(_, v)| objekt_traegt(v, needle))
        }
        _ => false,
    }
}

/// Trägt das Objekt mit dieser Id den Text? Ein Objekt, das es nicht gibt,
/// trägt nichts.
fn traegt(doc: &Document, id: (u32, u16), needle: &str) -> bool {
    doc.get_object(id)
        .map(|o| objekt_traegt(o, needle.as_bytes()))
        .unwrap_or(false)
}

/// Die Objekt-Id am Anfang von `s` („Objekt N G…“), als Zahlen.
fn id_am_anfang(s: &str) -> (u32, u16) {
    let rest = s
        .strip_prefix("Objekt ")
        .unwrap_or_else(|| panic!("hier muss „Objekt N G“ stehen: {s}"));
    let mut zahlen = rest
        .split(|c: char| !c.is_ascii_digit())
        .filter(|t| !t.is_empty());
    let mut naechste = || {
        zahlen
            .next()
            .unwrap_or_else(|| panic!("zwei Zahlen erwartet: {s}"))
    };
    let number: u32 = naechste().parse().expect("Objektnummer");
    let generation: u16 = naechste().parse().expect("Generation");
    (number, generation)
}

/// Fundstellen mit ihrem Ort, gepaart — die Paarung ist die Zusage von
/// `LeakCheck::sites`.
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

/// **Sicht 4 nennt das enthaltene Objekt, nicht seinen Container.**
#[test]
fn zi_a_s1_sicht_vier_nennt_das_enthaltene_objekt() {
    let pdf = common::object_stream(SECRET);
    let doc = Document::load_mem(&pdf).expect("ladbar");
    let funde = funde(&pdf);

    let sicht_vier: Vec<_> = funde
        .iter()
        .filter(|(_, s)| s.view == LeakView::ObjectStream)
        .collect();
    assert!(
        !sicht_vier.is_empty(),
        "Sicht 4 muss den Klartext im Objekt-Strom finden: {funde:#?}"
    );

    for (text, site) in &sicht_vier {
        let ort = site
            .object
            .unwrap_or_else(|| panic!("Sicht 4 kennt ihr Objekt: {text}"));
        let (vorne, hinten) = text
            .split_once(" <ObjStm> → ")
            .unwrap_or_else(|| panic!("Satz der Sicht 4 nennt Container und Inhalt: {text}"));
        let container = id_am_anfang(vorne);
        let enthalten = id_am_anfang(hinten);
        assert_ne!(
            container, enthalten,
            "Container und Inhalt müssen verschiedene Ids haben, sonst misst \
             dieser Test nichts: {text}"
        );

        // Die Entscheidung: die hintere Id, nicht die vordere.
        assert_eq!(
            ort, enthalten,
            "der Ort muss das enthaltene Objekt {enthalten:?} nennen, nicht den \
             Container {container:?} — {text}"
        );
        // Notwendig, nicht hinreichend (siehe Modulkopf): das genannte Objekt
        // trägt den Fund im geladenen Dokument wirklich.
        assert!(
            traegt(&doc, ort, SECRET),
            "der Ort nennt Objekt {ort:?}, dort steht der Klartext nicht — {text}"
        );
    }
}

/// **Gegenrichtung.** Derselbe Maßstab an einer gewöhnlichen Datei ohne
/// Objekt-Strom: jede genannte Id trägt ihren Fund. Ohne diesen Test wäre der
/// obere bloß eine Messung, die immer anschlägt.
#[test]
fn zi_a_s2_gewoehnliche_datei_besteht_denselben_massstab() {
    let pdf = common::page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]).finish();
    let doc = Document::load_mem(&pdf).expect("ladbar");
    let funde = funde(&pdf);
    assert!(!funde.is_empty(), "der Klartext muss gefunden werden");

    let mut geprueft = 0usize;
    for (text, site) in &funde {
        if let Some(id) = site.object {
            assert!(
                traegt(&doc, id, SECRET),
                "gewöhnliche Datei: der Ort nennt Objekt {id:?}, das den Fund \
                 nicht trägt — {text}"
            );
            geprueft += 1;
        }
    }
    assert!(geprueft > 0, "keine Id geprüft: {funde:#?}");
}
