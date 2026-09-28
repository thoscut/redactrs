//! Gegenprüfung D der Runde 9 zum CHANGELOG-Abschnitt **Fix-Runde 8**, Satz:
//!
//! > Jetzt kommt zu jeder Fundstelle ein Ort: die Sicht, die Seite (wo eine
//! > Sicht eine kennt) und die Objekt-Id (wo sie eine kennt) — samt ihrer
//! > Herkunft, damit niemand eine geratene Seite für eine gelesene nimmt.
//!
//! Geprüft an eigenem Material und am Orakel selbst
//! ([`redact_pdf::leaks_many`]), nicht am Bericht des Programms. Vier
//! Behauptungen:
//!
//! 1. **zu jeder** Fundstelle einer — `sites[n]` ist so lang wie
//!    `findings[n]`, für **jeden** Suchbegriff (auch den ohne Fund).
//! 2. Die **Sicht** steht immer da, und sie ist die, die der Satz nennt.
//! 3. Die **Herkunft** ist genau dann gesetzt, wenn eine Objekt-Id dasteht —
//!    und sie unterscheidet die gelesene Id (`Document`) von der aus dem
//!    Objektkopf der Rohbytes (`RawHeader`).
//! 4. Die **Seite** steht da, wo die Sicht eine kennt: der Schriftdekoder
//!    (Sicht 7) nennt sie immer.
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-pdf --test zm_d_ort_der_fundstelle`

use lopdf::{dictionary, Document, Object};
use redact_pdf::audit_bytes::{LeakView, ObjectSource};
use redact_pdf::testing::{build_pdf, TextItem};
use redact_pdf::{leaks_many_within, save_to_bytes};

const GEHEIM: &str = "DE02 1203 0000 0000 2020 51";
/// Ein zweiter Begriff, den es nirgends gibt — seine Liste muss leer sein und
/// **trotzdem** eine sein.
const OHNE_FUND: &str = "NICHTS-DAVON-STEHT-HIER";

/// Zwei Seiten Text, das Geheimnis auf der zweiten; dazu `/Info /Title` mit
/// demselben Text (ein Zeichenkettenobjekt, Sicht 5).
fn material() -> Vec<u8> {
    let bytes = build_pdf(&[
        vec![TextItem::new(72.0, 700.0, 10.0, "Seite eins, harmlos")],
        vec![TextItem::new(
            72.0,
            700.0,
            10.0,
            format!("Konto {GEHEIM} Ende"),
        )],
    ]);
    let mut doc = Document::load_mem(&bytes).expect("lädt");
    let info = doc.add_object(Object::Dictionary(dictionary! {
        "Title" => Object::string_literal(format!("Auszug {GEHEIM}")),
    }));
    doc.trailer.set("Info", info);
    save_to_bytes(&doc).expect("speichert")
}

#[test]
fn jede_fundstelle_traegt_ihren_ort() {
    let bytes = material();
    let check = leaks_many_within(&bytes, &[GEHEIM, OHNE_FUND], u64::MAX);

    assert_eq!(check.findings.len(), 2, "je Suchbegriff eine Liste");
    assert_eq!(
        check.sites.len(),
        check.findings.len(),
        "und je Suchbegriff eine Liste von Orten"
    );
    for (n, (saetze, orte)) in check.findings.iter().zip(&check.sites).enumerate() {
        assert_eq!(
            saetze.len(),
            orte.len(),
            "Begriff {n}: Satz und Ort Eintrag für Eintrag"
        );
    }
    assert!(
        check.findings[1].is_empty() && check.sites[1].is_empty(),
        "der Begriff ohne Fund hat eine leere Liste — und sie existiert"
    );
    assert!(
        !check.findings[0].is_empty(),
        "Vorbedingung: das Orakel findet das Geheimnis"
    );

    for (satz, ort) in check.findings[0].iter().zip(&check.sites[0]) {
        eprintln!(
            "Sicht {} | Seite {:?} | Objekt {:?} ({:?}) | {}",
            ort.view.number(),
            ort.page,
            ort.object,
            ort.object_source,
            &satz.chars().take(70).collect::<String>()
        );
        // 3 — Herkunft genau dann, wenn eine Id dasteht.
        assert_eq!(
            ort.object.is_some(),
            ort.object_source.is_some(),
            "Id und Herkunft werden zusammen gesetzt: {satz}"
        );
        // 2 — die Sicht des Ortes ist die des Satzes.
        let erwartet = match ort.view {
            LeakView::RawFile => "Rohdatei @",
            LeakView::RawStream => "Rohdaten-Stream @",
            LeakView::Stream => "<Stream",
            LeakView::ObjectStream => "<ObjStm>",
            LeakView::StringObject => "[Zeichenkette",
            LeakView::StringConcat => "[Zeichenketten-Verkettung",
            LeakView::FontDecoder => "Schriftdekoder",
        };
        assert!(
            satz.contains(erwartet),
            "der Satz nennt die Sicht {:?} nicht: {satz}",
            ort.view
        );
        // 4 — der Schriftdekoder kennt seine Seite, und es ist die zweite.
        if ort.view == LeakView::FontDecoder {
            assert_eq!(ort.page, Some(2), "Sicht 7 nennt die Seite: {satz}");
            assert!(
                satz.contains("Seite 2"),
                "und der Satz sagt dieselbe: {satz}"
            );
        }
        // Die gelesene Id ist die einzige, mit der ein Aufrufer ins Dokument
        // greifen darf.
        match ort.object_source {
            Some(ObjectSource::Document) => assert!(
                ort.document_object().is_some(),
                "eine gelesene Id gibt `document_object` heraus: {satz}"
            ),
            Some(ObjectSource::RawHeader) => assert!(
                ort.document_object().is_none(),
                "eine Id aus dem Objektkopf ist ungeprüft und wird nicht \
                 als gelesene ausgegeben: {satz}"
            ),
            None => {}
        }
    }

    // Und jede der drei Auskünfte kommt wenigstens einmal wirklich vor —
    // sonst prüfte dieser Test eine leere Zusage.
    let orte = &check.sites[0];
    assert!(
        orte.iter().any(|o| o.page.is_some()),
        "irgendeine Sicht nennt eine Seite"
    );
    assert!(
        orte.iter()
            .any(|o| o.object_source == Some(ObjectSource::Document)),
        "irgendeine Sicht nennt eine gelesene Objekt-Id"
    );
    assert!(
        orte.iter()
            .any(|o| o.object_source == Some(ObjectSource::RawHeader)),
        "und irgendeine eine aus dem Objektkopf — sonst wäre die Herkunft \
         eine Unterscheidung ohne Unterschied"
    );
}
