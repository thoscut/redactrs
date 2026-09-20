//! Register #41 / Vertrag V5: der **Ort** einer Fundstelle kommt
//! maschinenlesbar mit — nicht nur als Satz.
//!
//! ## Der Befund
//!
//! `LeakCheck::findings` liefert Zeichenketten wie
//! `Objekt 7 0/Popup/Contents [Zeichenkette, literal]: …` oder
//! `Seite 3 [Schriftdekoder]: …`. Solange das **alles** ist, was eine
//! Oberfläche bekommt, kann sie „gewollt stehen geblieben“ nicht von
//! „Schwärzung danebengegangen“ unterscheiden: sie hat nur den Text des Fundes
//! und muss raten. Die Sicherheitsfolge: eine danebengegangene Schwärzung sieht
//! aus wie ein bewusst stehen gelassener Text, und aus einem Leck wird „keine
//! Aussage“.
//!
//! ## Was hier geprüft wird
//!
//! 1. Die **Seite** steht im Ort, wo eine Sicht eine kennt — und es ist die
//!    richtige, 1-basiert wie im Text (`zh_a1`).
//! 2. Die **Objekt-Id** steht im Ort, und es ist die des Objekts, in dem der
//!    Text wirklich steht (`zh_a2`).
//! 3. Ort und Text sagen **dasselbe**, Eintrag für Eintrag: ein eigenes Modell
//!    liest die Sicht, die Seite und die Objekt-Id aus dem Satz und vergleicht
//!    sie mit dem maschinenlesbaren Ort (`zh_a3`). Damit hängt die Zuordnung
//!    `findings[n][i]` ↔ `sites[n][i]` nicht an einem Kommentar.
//! 4. Der **Text bleibt Zeichen für Zeichen**, wie er war (`zh_a4`) — die
//!    Kommandozeile, die Oberfläche und `docs/pruefung.txt` geben ihn aus.
//! 5. **Gegenrichtung**: jede Sicht meldet sich weiter. Findet eine nach einem
//!    Umbau nichts mehr, ist der Ort hübsch und das Orakel blind (`zh_a5`).

mod common;

use std::collections::BTreeSet;

use lopdf::{dictionary, Document, Object};
use redact_pdf::audit_bytes::{LeakSite, LeakView};
use redact_pdf::testing::{build_pdf, TextItem};
use redact_pdf::{leaks_many_within, LeakCheck};

use common::SECRET;

/// Eine Fundstelle, wie dieser Test sie liest: Satz und Ort zusammen.
struct Fund<'a> {
    text: &'a str,
    site: LeakSite,
}

/// Die Fundstellen des ersten Suchbegriffs, Satz und Ort gepaart.
///
/// Hier fällt die Zusicherung aus `LeakCheck::sites` schon auf: sind die Listen
/// nicht gleich lang, gibt es keine Paarung und der Test sagt das, statt sich
/// eine zu erfinden.
fn funde(check: &LeakCheck) -> Vec<Fund<'_>> {
    assert_eq!(
        check.sites.len(),
        check.findings.len(),
        "je Suchbegriff eine Liste von Orten"
    );
    assert_eq!(
        check.literal.len(),
        check.findings.len(),
        "je Suchbegriff eine Wörtlich-Marke"
    );
    for (hits, sites) in check.findings.iter().zip(&check.sites) {
        assert_eq!(
            hits.len(),
            sites.len(),
            "je Fundstelle ein Ort — sonst ist die Paarung geraten:\n{hits:#?}\n{sites:#?}"
        );
    }
    check.findings[0]
        .iter()
        .zip(&check.sites[0])
        .map(|(text, site)| Fund {
            text: text.as_str(),
            site: *site,
        })
        .collect()
}

fn pruefe(bytes: &[u8]) -> LeakCheck {
    let check = leaks_many_within(bytes, &[SECRET], u64::MAX);
    assert!(
        check.unchecked.is_empty(),
        "diese Proben müssen vollständig geprüft werden: {:#?}",
        check.unchecked
    );
    check
}

// ---------------------------------------------------------------------------
// zh_a1 — die Seite
// ---------------------------------------------------------------------------

/// Der Klartext steht auf **Seite 3** von drei, und der Ort sagt es: die
/// Schriftdekoder-Sicht meldet `page == Some(3)`.
///
/// Warum das die Probe mit Aussagekraft ist: wäre die Seitenzahl 0-basiert
/// übernommen oder von der ersten Seite mitgeschleppt, stünde hier `2` bzw.
/// `1` — beides sind genau die Fehler, die eine Oberfläche in die falsche
/// Richtung schicken. Der Satz nennt die Seite weiter genauso.
#[test]
fn zh_a1_die_seite_steht_maschinenlesbar_im_ort() {
    let pdf = build_pdf(&[
        vec![TextItem::new(72.0, 700.0, 12.0, "Seite eins, harmlos")],
        vec![TextItem::new(72.0, 700.0, 12.0, "Seite zwei, harmlos")],
        vec![TextItem::new(72.0, 700.0, 12.0, format!("IBAN: {SECRET}"))],
    ]);
    let check = pruefe(&pdf);
    let funde = funde(&check);

    let seiten: Vec<Option<usize>> = funde
        .iter()
        .filter(|f| f.site.view == LeakView::FontDecoder)
        .map(|f| f.site.page)
        .collect();
    assert_eq!(
        seiten,
        vec![Some(3)],
        "Sicht 7 muss genau Seite 3 melden: {:#?}",
        check.findings[0]
    );

    // Und der Ort gehört zu **diesem** Satz, nicht zu einem anderen.
    let fund = funde
        .iter()
        .find(|f| f.site.view == LeakView::FontDecoder)
        .expect("Sicht 7 hat den Klartext gelesen");
    assert!(
        fund.text.starts_with("Seite 3 [Schriftdekoder]: "),
        "Satz und Ort müssen dieselbe Seite nennen: {}",
        fund.text
    );
    // Sicht 7 kennt kein Objekt und erfindet keines.
    assert_eq!(fund.site.object, None, "{}", fund.text);

    // Gegenrichtung innerhalb des Tests: die Sichten, die **keine** Seite
    // kennen, melden auch keine.
    for f in &funde {
        if f.site.view != LeakView::FontDecoder {
            assert_eq!(
                f.site.page, None,
                "diese Sicht kennt keine Seite und darf keine behaupten: {}",
                f.text
            );
        }
    }
}

// ---------------------------------------------------------------------------
// zh_a2 — die Objekt-Id
// ---------------------------------------------------------------------------

/// Der Klartext steht in der `/Contents`-Zeichenkette einer Annotation mit
/// **bekannter** Objekt-Id, und der Ort nennt genau diese Id.
#[test]
fn zh_a2_die_objekt_id_steht_maschinenlesbar_im_ort() {
    let grund = build_pdf(&[vec![TextItem::new(72.0, 700.0, 12.0, "Kontoauszug")]]);
    let mut doc = Document::load_mem(&grund).expect("Vorlage ladbar");
    let annot = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![100.into(), 100.into(), 200.into(), 120.into()],
        "Contents" => Object::string_literal(SECRET),
    });
    let seite = doc.page_iter().next().expect("eine Seite");
    doc.get_dictionary_mut(seite)
        .expect("Seitenverzeichnis")
        .set("Annots", vec![Object::Reference(annot)]);
    let mut pdf = Vec::new();
    doc.save_to(&mut pdf).expect("speicherbar");

    let check = pruefe(&pdf);
    let funde = funde(&check);

    // Der Satz, den die Ausgabe schon immer schreibt — Zeichen für Zeichen.
    let satz = format!(
        "Objekt {} {}/Contents [Zeichenkette, literal]: …{SECRET}…",
        annot.0, annot.1
    );
    let fund = funde
        .iter()
        .find(|f| f.text == satz)
        .unwrap_or_else(|| panic!("{satz:?} fehlt in {:#?}", check.findings[0]));

    assert_eq!(
        fund.site,
        LeakSite {
            view: LeakView::StringObject,
            page: None,
            object: Some((annot.0, annot.1)),
        },
        "der Ort muss die Annotation nennen: {}",
        fund.text
    );
}

// ---------------------------------------------------------------------------
// zh_a3 — Ort und Satz sagen dasselbe
// ---------------------------------------------------------------------------

/// Ein Korpus, der mehrere Sichten trifft.
fn korpus() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("Kontoauszug", redact_pdf::testing::demo_statement()),
        ("Objekt-Stream", common::object_stream(SECRET)),
        ("verwaistes Objekt", common::orphan_object(SECRET)),
        ("Formularfeld", common::form_field_value(SECRET)),
        ("Struct-Tree", common::struct_elem_actual_text(SECRET)),
        ("XMP-Metadaten", common::page_metadata_xmp(SECRET)),
        (
            "Flate-Strom",
            common::filtered_stream(SECRET, "FlateDecode"),
        ),
        (
            "alte Revision",
            common::incremental_history(SECRET, "XXXX XXXX XXXX XXXX XXXX XX"),
        ),
    ]
}

/// Mein eigenes Modell: welche Sicht der **Satz** nennt.
///
/// Der Satz ist `{Ort} [{Wie}]: …{Kontext}…`. Der vordere Teil sagt, auf
/// welcher Ebene die Bytes lagen, der mittlere, wie verglichen wurde — und
/// beides zusammen bestimmt die Sicht des Modulkopfs. Dieses Modell kennt den
/// Quelltext des Orakels nicht; es liest nur, was in der Ausgabe steht.
fn sicht_aus_dem_satz(satz: &str) -> (LeakView, &str) {
    let (ort, rest) = satz.split_once(" [").unwrap_or_else(|| {
        panic!("kein Satz der Form „Ort [Wie]: …“: {satz}");
    });
    let wie = rest.split_once("]: ").map_or(rest, |(wie, _)| wie);
    let view = if wie.starts_with("Schriftdekoder") {
        LeakView::FontDecoder
    } else if wie.starts_with("Zeichenketten-Verkettung") {
        LeakView::StringConcat
    } else if ort.starts_with("Rohdatei @0x") {
        LeakView::RawFile
    } else if ort.starts_with("Rohdaten-Stream @0x") {
        LeakView::RawStream
    } else if ort.contains("<ObjStm>") {
        LeakView::ObjectStream
    } else if ort.contains("<Stream, ") {
        LeakView::Stream
    } else {
        LeakView::StringObject
    };
    (view, ort)
}

/// Ort und Satz sagen dasselbe — Eintrag für Eintrag, über den ganzen Korpus.
///
/// Geprüft wird jede der drei Angaben gegen den Satz derselben Fundstelle:
/// die Sicht (eigenes Modell oben), die Seite (`Seite N` im Satz) und die
/// Objekt-Id (`Objekt N G` im Satz). Damit fällt sowohl eine falsche Angabe
/// auf als auch eine **verrutschte** Paarung: ein Ort, der zur nächsten
/// Fundstelle gehört, nennt fast immer ein anderes Objekt.
///
/// **Eine Stelle, an der Satz und Ort mit Absicht auseinandergehen** und die
/// deshalb nicht in diesem Korpus steht: ein Objektkopf in Rohbytes, dessen
/// Nummer in keinen `u32` passt. Dort bleibt das Etikett im Satz (er ist
/// Ausgabe und soll sich nicht ändern), und der Ort sagt `None` statt eine Id
/// zu raten. Geprüft wird das in `audit_bytes.rs`,
/// `raw_blocks_are_labelled_with_their_object_header`.
#[test]
fn zh_a3_ort_und_satz_sagen_dasselbe() {
    let mut geprueft = 0usize;
    for (name, pdf) in korpus() {
        let check = leaks_many_within(&pdf, &[SECRET, "kommtnichtvor", ""], u64::MAX);
        let funde = funde(&check);
        assert!(!funde.is_empty(), "{name}: keine Fundstelle");
        assert!(
            check.findings[1].is_empty() && check.sites[1].is_empty(),
            "{name}: ein Nicht-Treffer hat weder Satz noch Ort"
        );
        assert!(
            check.findings[2].is_empty() && check.sites[2].is_empty(),
            "{name}: der leere Begriff hat weder Satz noch Ort"
        );

        for fund in &funde {
            let (view, ort) = sicht_aus_dem_satz(fund.text);
            assert_eq!(
                fund.site.view, view,
                "{name}: Sicht im Ort und im Satz verschieden: {}",
                fund.text
            );
            match fund.site.page {
                Some(page) => assert!(
                    ort.starts_with(&format!("Seite {page}")),
                    "{name}: der Ort nennt Seite {page}, der Satz nicht: {}",
                    fund.text
                ),
                None => assert!(
                    !ort.starts_with("Seite "),
                    "{name}: der Satz nennt eine Seite, der Ort keine: {}",
                    fund.text
                ),
            }
            if let Some((number, generation)) = fund.site.object {
                assert!(
                    ort.contains(&format!("Objekt {number} {generation}")),
                    "{name}: der Ort nennt Objekt {number} {generation}, der Satz nicht: {}",
                    fund.text
                );
            } else {
                assert!(
                    !ort.contains("Objekt "),
                    "{name}: der Satz nennt ein Objekt, der Ort keines: {}",
                    fund.text
                );
            }
            geprueft += 1;
        }
    }
    assert!(
        geprueft > 50,
        "zu wenige Fundstellen verglichen ({geprueft}) — dieser Test würde zu viel durchwinken"
    );
}

// ---------------------------------------------------------------------------
// zh_a4 — der Satz bleibt
// ---------------------------------------------------------------------------

/// Der Text jeder Fundstelle bleibt Zeichen für Zeichen, wie er war.
///
/// Er ist **Ausgabe**: `--check-leaks` schreibt ihn, die Oberfläche zeigt ihn,
/// und `crates/redact-cli/tests/belege.rs` vergleicht Bytes von
/// `docs/pruefung.txt` damit. Ein zusätzliches Feld bricht das nicht; ein
/// geänderter Satz schon. Deshalb stehen hier drei Sätze wörtlich — einer je
/// Bauart.
#[test]
fn zh_a4_der_satz_einer_fundstelle_bleibt_woertlich() {
    let pdf = build_pdf(&[vec![TextItem::new(
        72.0,
        700.0,
        12.0,
        format!("IBAN: {SECRET}"),
    )]]);
    let check = pruefe(&pdf);
    let saetze = &check.findings[0];

    for erwartet in [
        "Seite 1 [Schriftdekoder]",
        "Rohdatei @0x",
        "[Zeichenketten-Verkettung]",
    ] {
        assert!(
            saetze.iter().any(|s| s.contains(erwartet)),
            "„{erwartet}“ fehlt: {saetze:#?}"
        );
    }

    // Und einer ganz wörtlich, samt Kontextfenster: die Seite, wie der
    // Schriftdekoder sie liest.
    let woertlich = format!("Seite 1 [Schriftdekoder]: …IBAN: {SECRET}…");
    assert!(
        saetze.contains(&woertlich),
        "{woertlich:?} fehlt: {saetze:#?}"
    );
}

// ---------------------------------------------------------------------------
// zh_a5 — Gegenrichtung
// ---------------------------------------------------------------------------

/// **Gegenrichtung.** Jede der sieben Sichten meldet sich über den Korpus
/// mindestens einmal.
///
/// Der Ort ist wertlos, wenn das Orakel dafür weniger findet. Dieser Test ist
/// der Kanarienvogel: verstummt eine Sicht — weil ein Umbau ihren Aufruf
/// verliert oder ihren Datenblock nicht mehr durchsucht —, ist er rot und nennt
/// die Sicht.
#[test]
fn zh_a5_jede_sicht_meldet_sich_weiter() {
    let mut gesehen: BTreeSet<LeakView> = BTreeSet::new();
    for (_, pdf) in korpus() {
        let check = leaks_many_within(&pdf, &[SECRET], u64::MAX);
        for site in &check.sites[0] {
            gesehen.insert(site.view);
        }
    }
    let alle = [
        LeakView::RawFile,
        LeakView::RawStream,
        LeakView::Stream,
        LeakView::ObjectStream,
        LeakView::StringObject,
        LeakView::StringConcat,
        LeakView::FontDecoder,
    ];
    let fehlend: Vec<LeakView> = alle
        .iter()
        .copied()
        .filter(|v| !gesehen.contains(v))
        .collect();
    assert!(
        fehlend.is_empty(),
        "diese Sichten melden nichts mehr: {fehlend:?} (gesehen: {gesehen:?})"
    );
    // Die Nummern sind die des Modulkopfs — die Oberfläche schreibt sie hin.
    assert_eq!(
        alle.iter().map(|v| v.number()).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6, 7]
    );
}
