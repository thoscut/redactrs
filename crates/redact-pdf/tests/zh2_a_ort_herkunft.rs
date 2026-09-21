//! Register #41 / Vertrag V5, Nachbesserung zu Befund ZI-A1: die **Herkunft**
//! der Objekt-Id einer Fundstelle steht am Typ — und damit gilt wieder, was
//! `LeakSite` zusichert.
//!
//! ## Der Einwand
//!
//! `LeakSite` sagte zu: „Objektnummer und Generation, wie `lopdf` sie zählt“
//! und „Gefüllt wird, was die Sicht **weiß**; geraten wird nichts“. Die
//! Rohsichten (2, und 6 auf demselben Block) zählen aber nicht mit `lopdf`:
//! `object_header` liest bis zu 64 KiB zurück bis zum letzten `N G obj` und
//! fragt das geladene Dokument nie. An zwei **gewöhnlichen** Dateien fällt
//! beides auseinander:
//!
//! 1. **inkrementelles Update** — jedes „Speichern“ schreibt eines. Der
//!    Klartext liegt in der alten Revision; unter derselben Nummer steht im
//!    geladenen Dokument das neue, **geschwärzte** Objekt. Wer der Id folgt,
//!    liest „geschwärzt, also gewollt stehen geblieben“ — genau die
//!    Verwechslung, gegen die der maschinenlesbare Ort eingeführt wurde.
//! 2. **eingebettetes PDF ohne Filter** (PDF/A-3, ZUGFeRD) — der Kopf im
//!    inneren Dokument gehört zu einer **fremden** Objektzählung.
//!
//! Beleg des Gegenprüfers: `tests/zi_a_ort_gegenprobe.rs` (`zi_a1`, `zi_a3`).
//!
//! ## Die Entscheidung, und warum nicht anders
//!
//! Gewählt: **den Ort typisieren**. `LeakSite::object_source` sagt, aus
//! welcher Quelle die Id stammt, und `LeakSite::document_object()` gibt nur
//! die Id heraus, mit der ein Aufrufer ins geladene Dokument greifen darf.
//!
//! * **Id gegen das Dokument prüfen** hinge Sicht 2 an der Ladbarkeit der
//!   Datei — diese Sicht ist aber genau dafür da, davon unabhängig zu sein:
//!   lädt `lopdf` die Datei nicht, ist sie die einzige Messung, die bleibt.
//!   Und sie verlöre eine **richtige** Id in den Lagen, um die es forensisch
//!   geht: Altrevision, Block ohne xref-Eintrag — dort gibt es im geladenen
//!   Dokument nichts zu bestätigen, obwohl der Kopf in den Bytes steht.
//! * **Id weglassen** wirft dieselbe Auskunft weg, nur immer.
//! * Falsch war nicht der **Wert**, sondern die **Zusicherung**. Die steht
//!   jetzt schwächer und wahr am Typ; die Auskunft bleibt beide Male da.
//!
//! ## Was hier geprüft wird
//!
//! * `zh2_a1` — inkrementelles Update: **jede** genannte Id ist als
//!   Rohbyte-Id gekennzeichnet, `document_object()` gibt keine heraus, und das
//!   Objekt hinter der Id trägt im geladenen Dokument den **Ersatztext**. Das
//!   ist der Befund ZI-A1, nur jetzt mit Etikett.
//! * `zh2_a2` — eingebettetes PDF: die Id aus der fremden Zählung ist
//!   ebenfalls Rohbyte-Id, und das Objekt dieser Nummer im äußeren Dokument
//!   trägt den Klartext nicht.
//! * `zh2_a3` — der **Vertrag, der jetzt gilt**, über einen Korpus: nennt ein
//!   Ort eine Id aus dem geladenen Dokument (`document_object()`), dann trägt
//!   dieses Objekt den Fund — ohne Ausnahme. Maßstab ist das geladene
//!   Dokument, nicht der Satz des Orakels (gemessen: 29 Dokument-Ids).
//! * `zh2_a4` — Id und Herkunft kommen **zusammen**: keine Id ohne Quelle,
//!   keine Quelle ohne Id, und jede Sicht nur mit der Quelle, die sie haben
//!   kann.
//! * `zh2_a5` — **Gegenrichtung**: die Auskunft ist nicht verschwunden. Die
//!   Rohsicht nennt ihre Id weiter (der Weg „einfach weglassen“ wäre hier
//!   rot), und an einer Datei ohne Altrevision stimmt sie auch.

mod common;

use lopdf::{dictionary, Document, Object, Stream};
use redact_pdf::audit_bytes::{LeakSite, LeakView, ObjectSource};
use redact_pdf::leaks_many_within;

use common::SECRET;

/// Der geschwärzte Ersatztext der neuen Revision — gleiche Länge, kein
/// Klartext.
const ERSATZ: &str = "XXXX XXXX XXXX XXXX XXXX XX";

// ---------------------------------------------------------------------------
// Maßstab: das geladene Dokument, nicht der Satz des Orakels
// ---------------------------------------------------------------------------

/// Die Bytefassungen, in denen das Orakel denselben Text sucht.
///
/// Ohne sie kennt der Maßstab die Datei schlechter als das Orakel und macht
/// aus einem **richtigen** Ort einen Fehlschlag: das `/V` eines
/// AcroForm-Feldes trägt das Geheimnis als UTF-16BE mit BOM, und darin steht
/// keine ASCII-Folge. Die BOM-Fassungen brauchen keinen eigenen Eintrag — sie
/// enthalten die Fassung ohne BOM.
fn fassungen(needle: &str) -> Vec<Vec<u8>> {
    vec![
        needle.as_bytes().to_vec(),
        needle.encode_utf16().flat_map(u16::to_be_bytes).collect(),
        needle.encode_utf16().flat_map(u16::to_le_bytes).collect(),
    ]
}

fn enthaelt(hay: &[u8], fassungen: &[Vec<u8>]) -> bool {
    fassungen
        .iter()
        .any(|f| hay.windows(f.len()).any(|w| w == f.as_slice()))
}

/// Steht der Text in diesem Objekt — Zeichenkette, Name, Strom (roh oder
/// entpackt) oder irgendwo darunter?
fn objekt_traegt(object: &Object, gesucht: &[Vec<u8>]) -> bool {
    match object {
        Object::String(raw, _) => enthaelt(raw, gesucht),
        Object::Name(name) => enthaelt(name, gesucht),
        Object::Array(items) => items.iter().any(|i| objekt_traegt(i, gesucht)),
        Object::Dictionary(dict) => dict.iter().any(|(_, v)| objekt_traegt(v, gesucht)),
        Object::Stream(stream) => {
            enthaelt(&stream.content, gesucht)
                || stream
                    .decompressed_content()
                    .is_ok_and(|data| enthaelt(&data, gesucht))
                || stream.dict.iter().any(|(_, v)| objekt_traegt(v, gesucht))
        }
        _ => false,
    }
}

/// Trägt das Objekt mit dieser Id im **geladenen** Dokument den Text?
///
/// `get_object` statt `objects.get`: ein Objekt aus einem Objekt-Strom soll
/// hier genauso zählen wie ein frei stehendes. `false` heißt auch: es gibt
/// gar keines mit dieser Id.
fn traegt(doc: &Document, id: (u32, u16), needle: &str) -> bool {
    let gesucht = fassungen(needle);
    doc.get_object(id)
        .is_ok_and(|object| objekt_traegt(object, &gesucht))
}

/// Satz und Ort gepaart, für den ersten Suchbegriff.
fn funde(bytes: &[u8]) -> Vec<(String, LeakSite)> {
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
// Material: ein unkomprimiert eingebettetes PDF mit fremder Objektzählung
// ---------------------------------------------------------------------------

/// Eine gewöhnliche Datei mit einem **eingebetteten PDF** ohne Filter.
///
/// Das innere Dokument zählt seine Objekte selbst. Sein Klartext-Strom trägt
/// absichtlich die Nummer der **Seite** des äußeren Dokuments: so ist
/// nachprüfbar, dass eine übernommene Id auf ein ganz anderes Objekt zeigt.
/// Zurück kommt die Datei und diese Nummer.
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

/// Dateien, die mehrere Sichten treffen — Rohsichten wie Objektsichten.
fn korpus() -> Vec<(&'static str, Vec<u8>)> {
    let (eingebettet, _) = eingebettetes_pdf(SECRET);
    vec![
        (
            "einfache Seite",
            common::page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]).finish(),
        ),
        (
            "inkrementelles Update",
            common::incremental_history(SECRET, ERSATZ),
        ),
        ("eingebettetes PDF", eingebettet),
        ("Objekt-Stream", common::object_stream(SECRET)),
        ("verwaistes Objekt", common::orphan_object(SECRET)),
        ("Formularfeld", common::form_field_value(SECRET)),
        ("Struct-Tree", common::struct_elem_actual_text(SECRET)),
        ("XMP-Metadaten", common::page_metadata_xmp(SECRET)),
    ]
}

// ---------------------------------------------------------------------------
// zh2_a1 — inkrementelles Update: die Id trägt jetzt ihr Etikett
// ---------------------------------------------------------------------------

/// Der Befund ZI-A1 mit Etikett: die Id der alten Revision wird weiter
/// genannt — aber als **Rohbyte-Id**, und `document_object()` gibt sie nicht
/// heraus.
#[test]
fn zh2_a1_inkrementell_jede_id_ist_als_rohbyte_id_gekennzeichnet() {
    let pdf = common::incremental_history(SECRET, ERSATZ);
    let doc = Document::load_mem(&pdf).expect("die neue Revision ist ladbar");
    let funde = funde(&pdf);
    assert!(!funde.is_empty(), "der Klartext muss gefunden werden");

    let mit_id: Vec<_> = funde.iter().filter(|(_, s)| s.object.is_some()).collect();
    assert!(
        !mit_id.is_empty(),
        "die Auskunft soll nicht verschwinden, nur ihr Etikett bekommen: {funde:#?}"
    );

    for (text, site) in &mit_id {
        let id = site.object.expect("gefiltert");
        assert_eq!(
            site.object_source,
            Some(ObjectSource::RawHeader),
            "diese Id kommt aus dem Objektkopf in den Rohbytes: {text}"
        );
        assert_eq!(
            site.document_object(),
            None,
            "eine ungeprüfte Id darf nicht als Dokument-Id herausgehen: {text}"
        );
        // Und der Grund, warum das zählt: wer der Id ins Dokument folgt,
        // landet auf dem geschwärzten Objekt und liest „gewollt“.
        assert!(
            !traegt(&doc, id, SECRET),
            "Lage geändert? Objekt {id:?} trägt den Klartext doch: {text}"
        );
        assert!(
            traegt(&doc, id, ERSATZ),
            "Objekt {id:?} trägt den Ersatztext der neuen Revision: {text}"
        );
    }

    // Kein Ort in dieser Datei behauptet, aus dem Dokument zu stammen: der
    // Klartext liegt allein in Rohbytes.
    assert!(
        funde.iter().all(|(_, s)| s.document_object().is_none()),
        "{funde:#?}"
    );
}

// ---------------------------------------------------------------------------
// zh2_a2 — eingebettetes PDF: fremde Zählung
// ---------------------------------------------------------------------------

/// Die Id aus der Zählung des **inneren** Dokuments ist ebenfalls
/// Rohbyte-Id — und zeigt im äußeren Dokument auf die Seite, die den Klartext
/// nicht trägt.
#[test]
fn zh2_a2_eingebettetes_pdf_die_fremde_zaehlung_ist_rohbyte_id() {
    let (pdf, seite) = eingebettetes_pdf(SECRET);
    let doc = Document::load_mem(&pdf).expect("ladbar");
    let funde = funde(&pdf);

    // Die Seite des äußeren Dokuments trägt den Klartext nicht — der steckt im
    // eingebetteten Anhang.
    assert!(
        !traegt(&doc, seite, SECRET),
        "Material kaputt: die Seite {seite:?} trägt den Klartext selbst"
    );

    let fremde: Vec<_> = funde
        .iter()
        .filter(|(_, s)| s.object == Some(seite))
        .collect();
    assert!(
        !fremde.is_empty(),
        "der Kopf des inneren Dokuments nennt {seite:?}; kein Ort übernimmt ihn: {funde:#?}"
    );
    for (text, site) in &fremde {
        assert_eq!(
            site.object_source,
            Some(ObjectSource::RawHeader),
            "aus den Rohbytes des Anhangs: {text}"
        );
        assert_eq!(site.document_object(), None, "{text}");
    }
}

// ---------------------------------------------------------------------------
// zh2_a3 — der Vertrag, der jetzt gilt
// ---------------------------------------------------------------------------

/// **Der Vertrag.** Nennt ein Ort eine Id aus dem geladenen Dokument, dann
/// trägt dieses Objekt den Fund — ohne Ausnahme.
///
/// Maßstab ist das geladene Dokument, und der Maßstab kennt die
/// Bytefassungen, die das Orakel sucht ([`fassungen`]) — sonst zählte seine
/// eigene Unkenntnis als Fehler des Ortes (das `/V` eines AcroForm-Feldes
/// steht als UTF-16BE da).
///
/// Ein Fund, der erst aus **zerlegten** Literalen entsteht
/// ([`LeakView::StringConcat`] über einen `TJ`-zerlegten Text), steht nicht
/// Byte für Byte im Objekt. Im Korpus hier kommt das nicht vor — gemessen:
/// alle geprüften Dokument-Ids bestehen den Wortlaut-Maßstab. Kommt einmal
/// eine solche Datei dazu, ist die richtige Antwort, den Maßstab um die
/// Verkettung zu erweitern, **nicht** die Ausnahme: eine Ausnahme im Vertrag
/// ist das Loch, durch das der nächste Ort auf ein fremdes Objekt zeigt.
#[test]
fn zh2_a3_vertrag_eine_dokument_id_nennt_den_traeger() {
    let mut geprueft = 0usize;
    let mut klagen: Vec<String> = Vec::new();

    for (name, pdf) in korpus() {
        let doc = Document::load_mem(&pdf).expect("ladbar");
        for (text, site) in funde(&pdf) {
            let Some(id) = site.document_object() else {
                continue;
            };
            if traegt(&doc, id, SECRET) {
                geprueft += 1;
                continue;
            }
            klagen.push(format!(
                "{name}: Ort nennt Objekt {id:?} aus dem Dokument, das den Fund nicht trägt \
                 (Sicht {:?}) — {text}",
                site.view
            ));
        }
    }

    assert!(klagen.is_empty(), "{}", klagen.join("\n"));
    eprintln!("{geprueft} Dokument-Id(s) am Träger geprüft");
    assert!(
        geprueft >= 5,
        "der Vertrag wäre leer geprüft: nur {geprueft} Dokument-Id(s) am Träger"
    );
}

// ---------------------------------------------------------------------------
// zh2_a4 — Id und Herkunft kommen zusammen
// ---------------------------------------------------------------------------

/// Keine Id ohne Quelle, keine Quelle ohne Id — und jede Sicht nur mit der
/// Quelle, die sie haben kann.
#[test]
fn zh2_a4_id_und_herkunft_kommen_zusammen() {
    let mut gesehen: Vec<(LeakView, Option<ObjectSource>)> = Vec::new();

    for (name, pdf) in korpus() {
        for (text, site) in funde(&pdf) {
            assert_eq!(
                site.object.is_some(),
                site.object_source.is_some(),
                "{name}: Id und Herkunft gehören zusammen — {site:?}: {text}"
            );
            match (site.view, site.object_source) {
                // Sicht 1 und Sicht 7 nennen kein Objekt.
                (LeakView::RawFile | LeakView::FontDecoder, quelle) => assert_eq!(
                    quelle, None,
                    "{name}: diese Sicht kennt kein Objekt: {text}"
                ),
                // Sicht 2 liest den Kopf aus den Rohbytes.
                (LeakView::RawStream, Some(quelle)) => assert_eq!(
                    quelle,
                    ObjectSource::RawHeader,
                    "{name}: die Rohsicht kann nur den Kopf aus den Bytes haben: {text}"
                ),
                // Sichten 3–5 lesen im geladenen Dokument.
                (
                    LeakView::Stream | LeakView::ObjectStream | LeakView::StringObject,
                    Some(quelle),
                ) => assert_eq!(
                    quelle,
                    ObjectSource::Document,
                    "{name}: diese Sicht läuft über den Objektgraphen: {text}"
                ),
                // Sicht 6 erbt den Ort des Blocks, auf dem sie liest — beides
                // ist möglich, aber nichts anderes.
                (LeakView::StringConcat, _) | (_, None) => {}
            }
            gesehen.push((site.view, site.object_source));
        }
    }

    // Gegenrichtung: beide Quellen kommen im Korpus wirklich vor, sonst prüft
    // die Tabelle oben eine Seite davon nicht.
    for quelle in [ObjectSource::Document, ObjectSource::RawHeader] {
        assert!(
            gesehen.iter().any(|(_, q)| *q == Some(quelle)),
            "{quelle:?} kommt im Korpus nicht vor"
        );
    }

    // Und für den Bericht: welche Paare der Korpus wirklich zeigt. Sicht 6
    // erbt den Ort ihres Blocks, kann also auf beiden Seiten auftauchen —
    // hier steht, welche Seite gemessen ist.
    gesehen.sort_unstable_by_key(|(view, quelle)| (view.number(), format!("{quelle:?}")));
    gesehen.dedup();
    eprintln!("Sicht × Quelle im Korpus: {gesehen:?}");
}

// ---------------------------------------------------------------------------
// zh2_a5 — Gegenrichtung: die Auskunft bleibt
// ---------------------------------------------------------------------------

/// Der Weg „für die Rohsichten einfach `object: None`“ wäre hier **rot**: die
/// Rohsicht nennt ihre Id weiter. An einer Datei ohne Altrevision zeigt sie
/// auch auf den Träger — nur zugesichert ist das nicht, und genau das sagt
/// jetzt die Quelle.
#[test]
fn zh2_a5_die_rohsicht_nennt_ihre_id_weiter() {
    let pdf = common::page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]).finish();
    let doc = Document::load_mem(&pdf).expect("ladbar");
    let funde = funde(&pdf);

    let roh: Vec<_> = funde
        .iter()
        .filter(|(_, s)| s.object_source == Some(ObjectSource::RawHeader))
        .collect();
    assert!(
        !roh.is_empty(),
        "die Rohsicht muss ihre Objekt-Id weiter nennen: {funde:#?}"
    );
    for (text, site) in &roh {
        let id = site.object.expect("Quelle gesetzt heißt Id gesetzt");
        assert!(
            traegt(&doc, id, SECRET),
            "hier trifft die Rohbyte-Id zufällig zu — die Auskunft ist brauchbar, \
             nur ungeprüft: {text}"
        );
        assert_eq!(
            site.document_object(),
            None,
            "ungeprüft bleibt ungeprüft: {text}"
        );
    }

    // Und die Objektsichten liefern daneben die geprüfte Fassung.
    let dokument: Vec<_> = funde
        .iter()
        .filter(|(_, s)| s.document_object().is_some())
        .collect();
    assert!(
        !dokument.is_empty(),
        "eine Datei ohne Altrevision muss auch Dokument-Ids liefern: {funde:#?}"
    );
}
