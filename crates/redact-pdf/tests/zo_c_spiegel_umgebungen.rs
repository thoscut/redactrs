//! Spur-A-Runde 1, Prüfer C: der Textspiegel über Form-XObjects — die
//! Probenlisten-Zeile „Spiegel über Formular: verschachtelt, mehrfach
//! platziert, ohne eigenes `/Resources`, in `/Properties`“ noch einmal mit
//! eigenem Material, und daneben die Suche nach Klassen, die nicht auf der
//! Liste stehen.
//!
//! Orakel überall: [`redact_pdf::leaks`] an der **Ausgabedatei**, nie der
//! Bericht des Programms.
//!
//! Absichtlich rote Tests (Stand dieser Runde) und was sie festhalten:
//!
//! * [`zwei_umgebungen_beide_mit_spiegel_auf_einer_seite`] und
//!   [`zwei_umgebungen_beide_mit_spiegel_auf_zwei_seiten`] — **Befund C-1**:
//!   ein Formular ohne eigenes `/Resources`, unter zwei Umgebungen platziert,
//!   in denen sich `/MC0` **beide Male** zu einem Spiegel auflöst. Der Scan
//!   läuft seit R1 je Umgebung (`first_marked_scan(stream, owner)`), aber die
//!   Senke behält je `(Strom, Operationsindex)` nur den **ersten** Datensatz
//!   (`ScanResult::marked` → `seen_marked`; über Seiten hinweg
//!   `redact::apply_with_report` → `form_marked_seen`). Der zweite Fundort —
//!   die andere Eigenschaftsliste, mit dem Geheimnis — wird nie bereinigt.
//!   Kein Wort davon im Bericht, Rückgabewert 0.
//! * [`spiegel_ueber_kachelmuster_mit_text`] und
//!   [`spiegel_ueber_formular_das_mit_kachelmuster_fuellt`] — **Befund C-2**:
//!   ein Spiegel im Seitenstrom über einer Fläche, die mit einem Kachelmuster
//!   gefüllt wird, das Text setzt. `MarkedTextRecord::forms` kennt nur `Do`;
//!   der Musterstrom hängt an `scn`/`f`, und `form_within` wird für ihn nicht
//!   gerufen. Die Glyphen fallen aus dem Muster, der Spiegel darüber bleibt
//!   mit dem Geheimnis stehen. Die einzige Warnung ist die allgemeine über das
//!   Muster („die übrigen Kacheln werden nicht einzeln vermessen“) — sie nennt
//!   den Spiegel nicht.
//! * [`formular_mit_eigenen_ressourcen_ohne_properties_name_aus_der_seite`] —
//!   **Befund C-3** (mit Vorbehalt): ein Formular mit eigenem `/Resources`,
//!   aber ohne `/Properties`, dessen `/Span /MC0 BDC` sich nur in den
//!   Ressourcen der **Seite** auflöst. Nach PDF 32000-1, 8.10.1 ist das nicht
//!   vorgesehen; Poppler (`GfxResources::lookupPropertiesNF`) sucht trotzdem
//!   die Kette hinauf und gibt den Spiegel aus. Der Scan legt keinen Datensatz
//!   an, die Liste in der Seite bleibt unangetastet.
//! * [`mess_spiegel_in_formularen_je_seite`] (Messung, `#[ignore]`) —
//!   **Befund C-4**, Speicher ohne Decke: Spiegel **in** Formularen werden von
//!   `apply_with_report` für jede Seite vollständig festgehalten
//!   (`form_marked`, je Formular einmal, mit `forms` und `shows`), bis zum
//!   Ende des Laufs. Die Decke `MAX_MIRROR_FORM_PLACEMENTS` gilt je Seite,
//!   `MAX_DEFERRED_MIRRORS` nur für Spiegel im **Seitenstrom**. Je Seite ein
//!   eigenes Formular mit 100 × 999 Paaren, gemessen (Debug, eigener Prozess):
//!   10 Seiten 78 MB, 40 Seiten 264 MB, 100 Seiten 636 MB — rund 6,2 MB und
//!   0,1 s je Seite, aus rund 10 kB Datei je Seite, ohne Schwärzung, ohne
//!   Warnung.

mod common;

use common::{page, text_ops, Doc, SECRET};
use lopdf::{dictionary, Dictionary, Object, ObjectId, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

// ---------------------------------------------------------------------------
// Bausteine (wie zg_r1_spiegel.rs — dieselbe Pipeline, damit die Messung
// vergleichbar ist)
// ---------------------------------------------------------------------------

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn text_at(y: i32, text: &str) -> String {
    format!("BT /F1 10 Tf 72 {y} Td ({}) Tj ET\n", escape(text))
}

fn analyse(bytes: &[u8]) -> (Vec<TextRun>, Vec<String>) {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion")
}

fn redaction_for(run: &TextRun, needle: &str) -> Option<Redaction> {
    let pos = run.text.find(needle)?;
    let rect = run.rect_for_byte_range(pos, pos + needle.len())?;
    Some(Redaction::new(
        Region::new(
            run.page,
            rect,
            Some(needle.to_string()),
            Source::Pattern {
                pattern_id: "iban_de".into(),
                confidence: 1.0,
            },
        ),
        Action::Blackout,
    ))
}

fn redactions_for(runs: &[TextRun], needle: &str) -> Vec<Redaction> {
    runs.iter()
        .filter_map(|run| redaction_for(run, needle))
        .collect()
}

fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> (Vec<u8>, Vec<String>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    (save_to_bytes(&doc).expect("Speichern"), report.warnings)
}

/// Schwärzt jede Fundstelle von `SECRET` (auf allen Seiten) und liefert
/// (Lecks in der Ausgabe, Warnungen des Redaktors).
fn nach_der_pipeline(bytes: &[u8]) -> (Vec<String>, Vec<String>) {
    nach_der_pipeline_auf(bytes, None)
}

/// Wie [`nach_der_pipeline`], schwärzt aber nur die Fundstellen auf `page`.
fn nach_der_pipeline_auf(bytes: &[u8], page: Option<usize>) -> (Vec<String>, Vec<String>) {
    let (runs, _) = analyse(bytes);
    let redactions: Vec<Redaction> = redactions_for(&runs, SECRET)
        .into_iter()
        .filter(|r| page.is_none_or(|p| r.region.page == p))
        .collect();
    assert!(!redactions.is_empty(), "nichts zu schwärzen: {runs:?}");
    let (out, warnings) = pipeline(bytes, &redactions);
    (leaks(&out, SECRET), warnings)
}

fn link(d: &mut Doc, holder: ObjectId, name: &str, target: ObjectId) {
    let holder = d.doc.get_dictionary_mut(holder).expect("Ressourcen");
    let mut xobjects = holder
        .get(b"XObject")
        .and_then(|o| o.as_dict())
        .cloned()
        .unwrap_or_default();
    xobjects.set(name, target);
    holder.set("XObject", xobjects);
}

fn set_properties(d: &mut Doc, holder: ObjectId, properties: Object) {
    d.doc
        .get_dictionary_mut(holder)
        .expect("Ressourcen")
        .set("Properties", properties);
}

/// Form-XObject **ohne** `/Resources` (PDF 32000-1, 8.10.1: die des Aufrufers
/// gelten).
fn add_bare_form(d: &mut Doc, holder: ObjectId, name: &str, body: &str) -> ObjectId {
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
            },
            body.as_bytes().to_vec(),
        )
        .with_compression(false),
    ));
    link(d, holder, name, form_id);
    form_id
}

/// Form-XObject mit eigenem Ressourcenobjekt. Liefert (Formular, Ressourcen).
fn add_form(d: &mut Doc, holder: ObjectId, name: &str, body: &str) -> (ObjectId, ObjectId) {
    let font_id = d.font_id;
    let form_resources = d.add(Object::Dictionary(
        dictionary! { "Font" => dictionary! { "F1" => font_id } },
    ));
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                "Resources" => form_resources,
            },
            body.as_bytes().to_vec(),
        )
        .with_compression(false),
    ));
    link(d, holder, name, form_id);
    (form_id, form_resources)
}

fn mirror_list(text: &str) -> Dictionary {
    dictionary! { "ActualText" => Object::string_literal(text.to_string()) }
}

fn lie() -> String {
    format!("Zahlung an {SECRET}")
}

/// Hängt eine zweite Seite mit eigenem Ressourcenobjekt an; liefert
/// (Seite, Ressourcen).
fn add_page(d: &mut Doc, content: &[u8]) -> (ObjectId, ObjectId) {
    let font_id = d.font_id;
    let resources = d.add(Object::Dictionary(
        dictionary! { "Font" => dictionary! { "F1" => font_id } },
    ));
    let content_id = d.add(Object::Stream(
        Stream::new(dictionary! {}, content.to_vec()).with_compression(false),
    ));
    let pages_id = d.pages_id;
    let page_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    }));
    let root = d.doc.get_dictionary_mut(pages_id).expect("Wurzel");
    let mut kids = root
        .get(b"Kids")
        .and_then(|k| k.as_array())
        .cloned()
        .unwrap_or_default();
    kids.push(Object::Reference(page_id));
    let count = kids.len() as i64;
    root.set("Kids", kids);
    root.set("Count", count);
    (page_id, resources)
}

// ===========================================================================
// A. Die Probenlisten-Zeile, mit eigenem Material
// ===========================================================================

/// verschachtelt: Spiegel in `Fm0` (eigene Ressourcen) über `/Fm1 Do`;
/// die Glyphen stehen in `Fm1`.
#[test]
fn probe_verschachtelt_spiegel_im_aeusseren_formular_ueber_dem_inneren() {
    let mut d = page(&[]);
    let res = d.resources_id;
    let (inner, _) = add_form(&mut d, res, "Fm1", &text_at(600, SECRET));
    let (_, outer_res) = add_form(
        &mut d,
        res,
        "Fm0",
        &format!(
            "/Span <</ActualText ({})>> BDC\nq /Fm1 Do Q\nEMC\n",
            escape(&lie())
        ),
    );
    link(&mut d, outer_res, "Fm1", inner);
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    let (found, warnings) = nach_der_pipeline(&d.finish());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// mehrfach platziert: dasselbe Formular (Spiegel inline im Formular) an zwei
/// Stellen der Seite; geschwärzt wird nur die zweite Platzierung.
#[test]
fn probe_mehrfach_platziert_nur_die_zweite_platzierung_geschwaerzt() {
    let mut d = page(&[]);
    let res = d.resources_id;
    add_form(
        &mut d,
        res,
        "Fm0",
        &format!(
            "/Span <</ActualText ({})>> BDC\n{}EMC\n",
            escape(&lie()),
            text_at(0, SECRET)
        ),
    );
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q 1 0 0 1 0 600 cm /Fm0 Do Q\nq 1 0 0 1 200 300 cm /Fm0 Do Q\n");
    d.set_content(&raw);
    let bytes = d.finish();
    let (runs, _) = analyse(&bytes);
    let second: Vec<Redaction> = runs
        .iter()
        .filter(|r| r.text.contains(SECRET) && (r.rect.ll.x - 272.0).abs() < 1.0)
        .filter_map(|r| redaction_for(r, SECRET))
        .collect();
    assert_eq!(second.len(), 1, "{runs:?}");
    let (out, warnings) = pipeline(&bytes, &second);
    let found = leaks(&out, SECRET);
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

#[derive(Clone, Copy, Debug)]
enum Liste {
    Direkt,
    PropertiesAlsVerweis,
    AlsObjekt,
}

fn properties_mit(d: &mut Doc, liste: Liste, list: Dictionary) -> Object {
    match liste {
        Liste::Direkt => Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(list) }),
        Liste::PropertiesAlsVerweis => {
            let id = d.add(Object::Dictionary(
                dictionary! { "MC0" => Object::Dictionary(list) },
            ));
            Object::Reference(id)
        }
        Liste::AlsObjekt => {
            let id = d.add(Object::Dictionary(list));
            Object::Dictionary(dictionary! { "MC0" => Object::Reference(id) })
        }
    }
}

/// ohne eigenes `/Resources`, in `/Properties` (drei Bauarten): `/MC0` steht
/// in der Seite, das Formular hat keine Ressourcen.
#[test]
fn probe_ohne_eigenes_resources_liste_in_der_seite_in_drei_bauarten() {
    let mut offen = Vec::new();
    for liste in [Liste::Direkt, Liste::PropertiesAlsVerweis, Liste::AlsObjekt] {
        let mut d = page(&[]);
        let res = d.resources_id;
        let props = properties_mit(&mut d, liste, mirror_list(&lie()));
        set_properties(&mut d, res, props);
        add_bare_form(
            &mut d,
            res,
            "Fm0",
            &format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)),
        );
        let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
        raw.extend_from_slice(b"q /Fm0 Do Q\n");
        d.set_content(&raw);
        let (found, warnings) = nach_der_pipeline(&d.finish());
        if !found.is_empty() {
            offen.push(format!("{liste:?}: {found:?} (Warnungen {warnings:?})"));
        }
    }
    assert!(offen.is_empty(), "{}", offen.join("\n"));
}

/// Spiegel über Formulargrenzen: im Seitenstrom über `/Fm0 Do`, Glyphen im
/// Formular; und dasselbe mit `/MC0` in `/Properties` der Seite.
#[test]
fn probe_spiegel_im_seitenstrom_ueber_dem_formular() {
    let mut offen = Vec::new();
    for liste in [None, Some(Liste::Direkt), Some(Liste::AlsObjekt)] {
        let mut d = page(&[]);
        let res = d.resources_id;
        add_form(&mut d, res, "Fm0", &text_at(600, SECRET));
        let open = match liste {
            None => format!("/Span <</ActualText ({})>> BDC\n", escape(&lie())),
            Some(liste) => {
                let props = properties_mit(&mut d, liste, mirror_list(&lie()));
                set_properties(&mut d, res, props);
                "/Span /MC0 BDC\n".to_string()
            }
        };
        let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
        raw.extend_from_slice(format!("{open}q /Fm0 Do Q\nEMC\n").as_bytes());
        d.set_content(&raw);
        let (found, warnings) = nach_der_pipeline(&d.finish());
        if !found.is_empty() {
            offen.push(format!("{liste:?}: {found:?} (Warnungen {warnings:?})"));
        }
    }
    assert!(offen.is_empty(), "{}", offen.join("\n"));
}

/// Formular unter zwei Umgebungen, **eine** davon mit Spiegel (Befund R1-2,
/// beide Reihenfolgen).
#[test]
fn probe_zwei_umgebungen_eine_mit_spiegel_beide_reihenfolgen() {
    let mut offen = Vec::new();
    for spiegel_zuerst in [true, false] {
        let mut d = page(&[]);
        let res = d.resources_id;
        let list_id = d.add(Object::Dictionary(mirror_list(&lie())));
        set_properties(
            &mut d,
            res,
            Object::Dictionary(dictionary! { "MC0" => Object::Reference(list_id) }),
        );
        let inner = add_bare_form(
            &mut d,
            res,
            "Fm1",
            &format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)),
        );
        let (_, outer_res) = add_form(&mut d, res, "Fm0", "q 1 0 0 1 0 -200 cm /Fm1 Do Q\n");
        link(&mut d, outer_res, "Fm1", inner);
        set_properties(
            &mut d,
            outer_res,
            Object::Dictionary(dictionary! {
                "MC0" => Object::Dictionary(dictionary! { "MCID" => 1_i64 })
            }),
        );
        let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
        raw.extend_from_slice(if spiegel_zuerst {
            b"q /Fm1 Do Q\nq /Fm0 Do Q\n"
        } else {
            b"q /Fm0 Do Q\nq /Fm1 Do Q\n"
        });
        d.set_content(&raw);
        let (found, warnings) = nach_der_pipeline(&d.finish());
        if !found.is_empty() {
            offen.push(format!(
                "spiegel_zuerst={spiegel_zuerst}: {found:?} (Warnungen {warnings:?})"
            ));
        }
    }
    assert!(offen.is_empty(), "{}", offen.join("\n"));
}

/// Zurückgestellter Spiegel mit Fundort im Verzeichnis: Seite 1 (ohne
/// Schwärzung) trägt `/Span /MC0 BDC /Fm0 Do EMC` mit `/MC0` **direkt** in
/// ihren Ressourcen; das Formular wird von Seite 2 aus geschwärzt.
#[test]
fn probe_zurueckgestellter_spiegel_mit_fundort_im_verzeichnis_der_seite_ohne_schwaerzung() {
    let mut d = page(&[]);
    let res = d.resources_id;
    set_properties(
        &mut d,
        res,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list(&lie())) }),
    );
    let (form, _) = add_form(&mut d, res, "Fm0", &text_at(600, SECRET));
    let mut raw = text_ops(&["Seite eins"]);
    raw.extend_from_slice(b"/Span /MC0 BDC\nq /Fm0 Do Q\nEMC\n");
    d.set_content(&raw);
    let mut second = text_ops(&["Seite zwei"]);
    second.extend_from_slice(b"q /Fm0 Do Q\n");
    let (_, res2) = add_page(&mut d, &second);
    link(&mut d, res2, "Fm0", form);
    let (found, warnings) = nach_der_pipeline_auf(&d.finish(), Some(1));
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// Zwei Ebenen ohne Ressourcen: Seite → `Fm0` (bar) → `Fm1` (bar) mit
/// `/Span /MC0 BDC`; `/MC0` steht nur in der Seite.
#[test]
fn probe_zwei_ebenen_ohne_ressourcen_name_aus_der_seite() {
    let mut d = page(&[]);
    let res = d.resources_id;
    set_properties(
        &mut d,
        res,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list(&lie())) }),
    );
    add_bare_form(
        &mut d,
        res,
        "Fm1",
        &format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)),
    );
    add_bare_form(&mut d, res, "Fm0", "q /Fm1 Do Q\n");
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    let (found, warnings) = nach_der_pipeline(&d.finish());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

// ===========================================================================
// B. Befund C-1: Formular unter zwei Umgebungen, beide mit Spiegel
// ===========================================================================

/// `Fm1` (ohne `/Resources`) trägt `/Span /MC0 BDC (Geheimnis) Tj EMC`.
/// `Fm0` (eigene Ressourcen) löst `/MC0` zu `unter_fm0` auf und zeichnet
/// `Fm1`; die Seite löst `/MC0` zu `unter_seite` auf und zeichnet `Fm1`
/// ebenfalls — `Fm0` zuerst.
fn zwei_umgebungen_beide_mit_spiegel(unter_fm0: &str, unter_seite: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    set_properties(
        &mut d,
        res,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list(unter_seite)) }),
    );
    let inner = add_bare_form(
        &mut d,
        res,
        "Fm1",
        &format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)),
    );
    let (_, outer_res) = add_form(&mut d, res, "Fm0", "q 1 0 0 1 0 -200 cm /Fm1 Do Q\n");
    link(&mut d, outer_res, "Fm1", inner);
    set_properties(
        &mut d,
        outer_res,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list(unter_fm0)) }),
    );
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\nq /Fm1 Do Q\n");
    d.set_content(&raw);
    d.finish()
}

/// **Befund C-1 (eine Seite), absichtlich rot.** Beide Umgebungen tragen einen
/// Spiegel; `seen_marked` behält den Datensatz der ersten (`Fm0`), der zweite
/// (Seite) wird verworfen — die Liste in der Seite bleibt mit dem Geheimnis
/// stehen. Zwei Ausprägungen: beide Spiegel lügen; nur der zweite lügt.
#[test]
#[ignore = "offen: Register #65 zwei Umgebungen, beide mit Spiegel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn zwei_umgebungen_beide_mit_spiegel_auf_einer_seite() {
    let mut offen = Vec::new();
    for (name, unter_fm0, unter_seite) in [
        ("beide lügen", lie(), lie()),
        ("harmlos zuerst", "Gruss".into(), lie()),
        // **still**: beide Spiegel sagen wortgleich, was die Glyphen setzen —
        // dann gibt es keine „sagt etwas anderes“-Warnung, und der Lauf
        // endet mit Rückgabewert 0.
        ("beide still", SECRET.into(), SECRET.into()),
    ] {
        let (found, warnings) =
            nach_der_pipeline(&zwei_umgebungen_beide_mit_spiegel(&unter_fm0, &unter_seite));
        if name == "beide still" {
            let spiegel: Vec<&String> = warnings
                .iter()
                .filter(|w| w.contains("Textspiegel"))
                .collect();
            assert!(
                spiegel.is_empty(),
                "unerwartete Spiegelwarnung: {spiegel:?}"
            );
        }
        if !found.is_empty() {
            offen.push(format!("{name}: {found:?} (Warnungen {warnings:?})"));
        }
    }
    assert!(offen.is_empty(), "{}", offen.join("\n"));
}

/// Gegenprobe zu C-1: steht die Umgebung mit dem Geheimnis **zuerst**, wird
/// sie gefunden und geleert (der harmlose zweite Spiegel bleibt — kein Leck).
#[test]
fn zwei_umgebungen_geheimnis_zuerst_wird_geleert() {
    let mut d = page(&[]);
    let res = d.resources_id;
    set_properties(
        &mut d,
        res,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list(&lie())) }),
    );
    let inner = add_bare_form(
        &mut d,
        res,
        "Fm1",
        &format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)),
    );
    let (_, outer_res) = add_form(&mut d, res, "Fm0", "q 1 0 0 1 0 -200 cm /Fm1 Do Q\n");
    link(&mut d, outer_res, "Fm1", inner);
    set_properties(
        &mut d,
        outer_res,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list("Gruss")) }),
    );
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm1 Do Q\nq /Fm0 Do Q\n");
    d.set_content(&raw);
    let (found, warnings) = nach_der_pipeline(&d.finish());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// Dasselbe Formular ohne Ressourcen auf zwei Seiten; jede Seite löst `/MC0`
/// in ihren eigenen Ressourcen auf (Seite 1: `unter_seite_1`, Seite 2:
/// `unter_seite_2`).
fn zwei_seiten_zwei_listen(unter_seite_1: &str, unter_seite_2: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    set_properties(
        &mut d,
        res,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list(unter_seite_1)) }),
    );
    let form = add_bare_form(
        &mut d,
        res,
        "Fm1",
        &format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)),
    );
    let mut raw = text_ops(&["Seite eins"]);
    raw.extend_from_slice(b"q /Fm1 Do Q\n");
    d.set_content(&raw);
    let mut second = text_ops(&["Seite zwei"]);
    second.extend_from_slice(b"q /Fm1 Do Q\n");
    let (_, res2) = add_page(&mut d, &second);
    link(&mut d, res2, "Fm1", form);
    set_properties(
        &mut d,
        res2,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list(unter_seite_2)) }),
    );
    d.finish()
}

/// **Befund C-1 (zwei Seiten), absichtlich rot.** `form_marked_seen` in
/// `apply_with_report` behält je `(Formular, Operationsindex)` den Datensatz
/// von Seite 1; die Liste in den Ressourcen von Seite 2 wird nie geräumt —
/// gleich, ob auf Seite 1, Seite 2 oder beiden geschwärzt wird.
#[test]
#[ignore = "offen: Register #65 zwei Umgebungen, beide mit Spiegel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn zwei_umgebungen_beide_mit_spiegel_auf_zwei_seiten() {
    let mut offen = Vec::new();
    for (name, unter_seite_1, unter_seite_2, seite) in [
        ("beide lügen, Schwärzung überall", lie(), lie(), None),
        (
            "harmlos auf Seite 1, Schwärzung Seite 2",
            "Gruss".into(),
            lie(),
            Some(1),
        ),
        (
            "harmlos auf Seite 1, Schwärzung Seite 1",
            "Gruss".into(),
            lie(),
            Some(0),
        ),
        // **still**: wortgleiche Spiegel, keine Spiegelwarnung, Rückgabewert 0.
        (
            "beide still, Schwärzung überall",
            SECRET.into(),
            SECRET.into(),
            None,
        ),
    ] {
        let (found, warnings) = nach_der_pipeline_auf(
            &zwei_seiten_zwei_listen(&unter_seite_1, &unter_seite_2),
            seite,
        );
        if name.starts_with("beide still") {
            let spiegel: Vec<&String> = warnings
                .iter()
                .filter(|w| w.contains("Textspiegel"))
                .collect();
            assert!(
                spiegel.is_empty(),
                "unerwartete Spiegelwarnung: {spiegel:?}"
            );
        }
        if !found.is_empty() {
            offen.push(format!("{name}: {found:?} (Warnungen {warnings:?})"));
        }
    }
    assert!(offen.is_empty(), "{}", offen.join("\n"));
}

// ===========================================================================
// C. Befund C-2: Spiegel über einem Kachelmuster mit Text
// ===========================================================================

/// Ein Kachelmuster mit eigenem Ressourcenobjekt, dessen Strom das Geheimnis
/// setzt. Liefert die Objekt-Id des Musters.
fn add_pattern(d: &mut Doc, holder: ObjectId, name: &str) -> ObjectId {
    let font_id = d.font_id;
    let pattern_res = d.add(Object::Dictionary(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    }));
    let body = text_at(20, SECRET);
    let pattern_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "Pattern",
                "PatternType" => 1_i64,
                "PaintType" => 1_i64,
                "TilingType" => 1_i64,
                "BBox" => vec![0.into(), 0.into(), 400.into(), 100.into()],
                "XStep" => 400_i64,
                "YStep" => 100_i64,
                "Resources" => pattern_res,
            },
            body.as_bytes().to_vec(),
        )
        .with_compression(false),
    ));
    let holder = d.doc.get_dictionary_mut(holder).expect("Ressourcen");
    let mut patterns = holder
        .get(b"Pattern")
        .and_then(|o| o.as_dict())
        .cloned()
        .unwrap_or_default();
    patterns.set(name, pattern_id);
    holder.set("Pattern", patterns);
    pattern_id
}

/// **Befund C-2, absichtlich rot.** `/Span <</ActualText (…)>> BDC` im
/// Seitenstrom über einer Fläche, die mit dem Textmuster gefüllt wird. Die
/// Glyphen fallen aus dem Musterstrom, der Spiegel im Seitenstrom nicht.
#[test]
#[ignore = "offen: Register #66 Spiegel ueber Kachelmuster — Spur-A-Runde 1, Beleg absichtlich rot"]
fn spiegel_ueber_kachelmuster_mit_text() {
    let mut offen = Vec::new();
    for (liste, key) in [
        (None, "ActualText"),
        (Some(Liste::AlsObjekt), "ActualText"),
        (None, "Alt"),
    ] {
        let mut d = page(&[]);
        let res = d.resources_id;
        add_pattern(&mut d, res, "P0");
        let open = match liste {
            // `/Alt` an `/Figure`: die Normalform der Barrierefreiheit — und
            // ohne die „sagt etwas anderes“-Warnung, die nur `/ActualText`
            // bekommt.
            None => format!("/Figure <</{key} ({})>> BDC\n", escape(&lie())),
            Some(liste) => {
                let props = properties_mit(&mut d, liste, mirror_list(&lie()));
                set_properties(&mut d, res, props);
                "/Span /MC0 BDC\n".to_string()
            }
        };
        let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
        raw.extend_from_slice(
            format!("{open}q /Pattern cs /P0 scn 72 500 400 100 re f Q\nEMC\n").as_bytes(),
        );
        d.set_content(&raw);
        let (found, warnings) = nach_der_pipeline(&d.finish());
        if !found.is_empty() {
            offen.push(format!(
                "{liste:?}/{key}: {found:?} (Warnungen {warnings:?})"
            ));
        }
    }
    assert!(offen.is_empty(), "{}", offen.join("\n"));
}

/// **Befund C-2 (über ein Formular), absichtlich rot.** Der Spiegel steht im
/// Seitenstrom über `/Fm0 Do`; `Fm0` füllt mit dem Textmuster. `forms` kennt
/// `Fm0`, aber `Fm0` verliert selbst kein Zeichen — der Plan liegt am
/// Musterstrom, und der ist kein `Do`-Kind von `Fm0`.
#[test]
#[ignore = "offen: Register #66 Spiegel ueber Kachelmuster — Spur-A-Runde 1, Beleg absichtlich rot"]
fn spiegel_ueber_formular_das_mit_kachelmuster_fuellt() {
    let mut d = page(&[]);
    let res = d.resources_id;
    let (_, form_res) = add_form(
        &mut d,
        res,
        "Fm0",
        "q /Pattern cs /P0 scn 72 500 400 100 re f Q\n",
    );
    add_pattern(&mut d, form_res, "P0");
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!(
            "/Span <</ActualText ({})>> BDC\nq /Fm0 Do Q\nEMC\n",
            escape(&lie())
        )
        .as_bytes(),
    );
    d.set_content(&raw);
    let (found, warnings) = nach_der_pipeline(&d.finish());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// Gegenprobe: der Spiegel **im Musterstrom** über den eigenen Glyphen fällt
/// (das ist die gedeckte Form aus zg_r1_spiegel).
#[test]
fn spiegel_im_musterstrom_selbst_wird_geleert() {
    let mut d = page(&[]);
    let res = d.resources_id;
    let font_id = d.font_id;
    let pattern_res = d.add(Object::Dictionary(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    }));
    let body = format!(
        "/Span <</ActualText ({})>> BDC\n{}EMC\n",
        escape(&lie()),
        text_at(20, SECRET)
    );
    let pattern_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "Pattern",
                "PatternType" => 1_i64,
                "PaintType" => 1_i64,
                "TilingType" => 1_i64,
                "BBox" => vec![0.into(), 0.into(), 400.into(), 100.into()],
                "XStep" => 400_i64,
                "YStep" => 100_i64,
                "Resources" => pattern_res,
            },
            body.into_bytes(),
        )
        .with_compression(false),
    ));
    d.doc
        .get_dictionary_mut(res)
        .expect("Ressourcen")
        .set("Pattern", dictionary! { "P0" => pattern_id });
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Pattern cs /P0 scn 72 500 400 100 re f Q\n");
    d.set_content(&raw);
    let (found, warnings) = nach_der_pipeline(&d.finish());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

// ===========================================================================
// D. Befund C-3: Formular mit eigenen Ressourcen, Name nur in der Seite
// ===========================================================================

/// **Befund C-3, absichtlich rot (mit Vorbehalt, siehe Kopf).** `Fm0` hat
/// eigene Ressourcen ohne `/Properties`; `/Span /MC0 BDC` darin löst sich
/// nach 8.10.1 nicht auf, Poppler löst es über die Seite auf. Der Klartext
/// steht in den Seitenressourcen und bleibt dort.
#[test]
#[ignore = "offen: Register #67 Formular mit /Resources ohne /Properties — Spur-A-Runde 1, Beleg absichtlich rot"]
fn formular_mit_eigenen_ressourcen_ohne_properties_name_aus_der_seite() {
    let mut offen = Vec::new();
    for (liste, spiegel) in [
        (Liste::Direkt, lie()),
        (Liste::AlsObjekt, lie()),
        // still: wortgleich, keine Spiegelwarnung — hier ohnehin nicht, denn
        // der Scan legt gar keinen Datensatz an.
        (Liste::Direkt, SECRET.into()),
    ] {
        let mut d = page(&[]);
        let res = d.resources_id;
        let props = properties_mit(&mut d, liste, mirror_list(&spiegel));
        set_properties(&mut d, res, props);
        add_form(
            &mut d,
            res,
            "Fm0",
            &format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)),
        );
        let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
        raw.extend_from_slice(b"q /Fm0 Do Q\n");
        d.set_content(&raw);
        let (found, warnings) = nach_der_pipeline(&d.finish());
        let spiegelwarnungen: Vec<&String> = warnings
            .iter()
            .filter(|w| w.contains("Textspiegel"))
            .collect();
        assert!(spiegelwarnungen.is_empty(), "{spiegelwarnungen:?}");
        if !found.is_empty() {
            offen.push(format!(
                "{liste:?}/{spiegel}: {found:?} (Warnungen {warnings:?})"
            ));
        }
    }
    assert!(offen.is_empty(), "{}", offen.join("\n"));
}

// ===========================================================================
// E. Randgebiete des Auftrags: Erscheinungsstrom, eigene Kodierung, Type3
// ===========================================================================

/// Erscheinungsstrom (`/AP /N`) mit eigenem `/Resources`, `/Properties /MC0`
/// **direkt** darin, Spiegel über den Glyphen im Erscheinungsstrom.
#[test]
fn erscheinungsstrom_mit_spiegel_in_eigenen_properties() {
    let mut d = page(&["Kontoinhaber: Max Mustermann"]);
    let font_id = d.font_id;
    let ap_content = format!(
        "/Span /MC0 BDC\nBT /F1 8 Tf 0 4 Td (Notiz: {}) Tj ET\nEMC\n",
        escape(SECRET)
    );
    let ap_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 240.into(), 20.into()],
                "Resources" => dictionary! {
                    "Font" => dictionary! { "F1" => font_id },
                    "Properties" => dictionary! {
                        "MC0" => Object::Dictionary(mirror_list(&lie()))
                    },
                },
            },
            ap_content.into_bytes(),
        )
        .with_compression(false),
    ));
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FreeText",
        "Rect" => vec![300.into(), 100.into(), 540.into(), 120.into()],
        "F" => 4_i64,
        "AP" => dictionary! { "N" => ap_id },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
    let (found, warnings) = nach_der_pipeline(&d.finish());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// Die Zeichen des Geheimnisses, je einmal.
fn zeichenvorrat() -> Vec<char> {
    let mut seen = Vec::new();
    for c in SECRET.chars() {
        if !seen.contains(&c) {
            seen.push(c);
        }
    }
    seen
}

fn glyph_name(c: char) -> &'static str {
    match c {
        '0' => "zero",
        '1' => "one",
        '2' => "two",
        '3' => "three",
        '4' => "four",
        '5' => "five",
        '6' => "six",
        '7' => "seven",
        '8' => "eight",
        '9' => "nine",
        ' ' => "space",
        'D' => "D",
        'E' => "E",
        _ => panic!("kein Glyphname für {c:?}"),
    }
}

/// Das Geheimnis in Codes ab 0x80 (eigene Kodierung).
fn codes_ab_0x80() -> Vec<u8> {
    let vorrat = zeichenvorrat();
    SECRET
        .chars()
        .map(|c| 0x80 + vorrat.iter().position(|v| *v == c).unwrap() as u8)
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Schrift mit eigener Kodierung (`/Differences` ab 0x80) in einem Formular,
/// Spiegel über `/Properties` des Formulars.
#[test]
fn eigene_kodierung_im_formular_unter_spiegel() {
    let mut d = page(&[]);
    let res = d.resources_id;
    let mut differences: Vec<Object> = vec![Object::Integer(0x80)];
    for c in zeichenvorrat() {
        differences.push(Object::Name(glyph_name(c).as_bytes().to_vec()));
    }
    let font_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => dictionary! {
            "Type" => "Encoding",
            "BaseEncoding" => "WinAnsiEncoding",
            "Differences" => Object::Array(differences),
        },
    }));
    let form_res = d.add(Object::Dictionary(dictionary! {
        "Font" => dictionary! { "F2" => font_id },
        "Properties" => dictionary! { "MC0" => Object::Dictionary(mirror_list(&lie())) },
    }));
    let body = format!(
        "/Span /MC0 BDC\nBT /F2 10 Tf 72 600 Td <{}> Tj ET\nEMC\n",
        hex(&codes_ab_0x80())
    );
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                "Resources" => form_res,
            },
            body.into_bytes(),
        )
        .with_compression(false),
    ));
    link(&mut d, res, "Fm0", form_id);
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    let (found, warnings) = nach_der_pipeline(&d.finish());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// Type3-Schrift (nur malende Glyphprozeduren) mit `/ToUnicode` in einem
/// Formular, Spiegel inline über den Glyphen.
#[test]
fn type3_mit_tounicode_im_formular_unter_spiegel() {
    let mut d = page(&[]);
    let res = d.resources_id;
    let vorrat = zeichenvorrat();
    let mut char_procs = Dictionary::new();
    let mut differences: Vec<Object> = vec![Object::Integer(0x80)];
    let mut bfchars = String::new();
    for (i, c) in vorrat.iter().enumerate() {
        let name = format!("g{i}");
        let id = d.add(Object::Stream(
            Stream::new(
                dictionary! {},
                b"1000 0 0 0 1000 1000 d1\n0 0 800 800 re f\n".to_vec(),
            )
            .with_compression(false),
        ));
        char_procs.set(name.as_bytes().to_vec(), Object::Reference(id));
        differences.push(Object::Name(name.into_bytes()));
        bfchars.push_str(&format!("<{:02X}> <{:04X}>\n", 0x80 + i, *c as u32));
    }
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CMapName /Custom def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n\
         {} beginbfchar\n{bfchars}endbfchar\nendcmap\n\
         CMapName currentdict /CMap defineresource pop\nend\nend\n",
        vorrat.len()
    );
    let cmap_id = d.add(Object::Stream(
        Stream::new(dictionary! {}, cmap.into_bytes()).with_compression(false),
    ));
    let widths: Vec<Object> = vorrat.iter().map(|_| Object::Integer(600)).collect();
    let font_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type3",
        "FontBBox" => vec![0.into(), 0.into(), 1000.into(), 1000.into()],
        "FontMatrix" => vec![
            Object::Real(0.001), Object::Real(0.0), Object::Real(0.0),
            Object::Real(0.001), Object::Real(0.0), Object::Real(0.0),
        ],
        "CharProcs" => char_procs,
        "Encoding" => dictionary! {
            "Type" => "Encoding",
            "Differences" => Object::Array(differences),
        },
        "FirstChar" => 0x80_i64,
        "LastChar" => (0x80 + vorrat.len() - 1) as i64,
        "Widths" => Object::Array(widths),
        "ToUnicode" => cmap_id,
        "Resources" => dictionary! {},
    }));
    let form_res = d.add(Object::Dictionary(dictionary! {
        "Font" => dictionary! { "T3" => font_id },
    }));
    let body = format!(
        "/Span <</ActualText ({})>> BDC\nBT /T3 10 Tf 72 600 Td <{}> Tj ET\nEMC\n",
        escape(&lie()),
        hex(&codes_ab_0x80())
    );
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                "Resources" => form_res,
            },
            body.into_bytes(),
        )
        .with_compression(false),
    ));
    link(&mut d, res, "Fm0", form_id);
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    let (found, warnings) = nach_der_pipeline(&d.finish());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

// ===========================================================================
// Material für die Kommandozeile (`redact-rs … --check-leaks`)
// ===========================================================================

/// Schreibt das Material der roten Befunde nach `ZO_C_OUT`.
#[test]
#[ignore = "Material"]
fn schreibt_material() {
    let Ok(dir) = std::env::var("ZO_C_OUT") else {
        return;
    };
    let mut kachel = page(&[]);
    let res = kachel.resources_id;
    add_pattern(&mut kachel, res, "P0");
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!(
            "/Span <</ActualText ({})>> BDC\nq /Pattern cs /P0 scn 72 500 400 100 re f Q\nEMC\n",
            escape(&lie())
        )
        .as_bytes(),
    );
    kachel.set_content(&raw);

    let mut eigene = page(&[]);
    let res = eigene.resources_id;
    set_properties(
        &mut eigene,
        res,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list(&lie())) }),
    );
    add_form(
        &mut eigene,
        res,
        "Fm0",
        &format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)),
    );
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    eigene.set_content(&raw);

    let mut kachel_alt = page(&[]);
    let res = kachel_alt.resources_id;
    add_pattern(&mut kachel_alt, res, "P0");
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!(
            "/Figure <</Alt ({})>> BDC\nq /Pattern cs /P0 scn 72 500 400 100 re f Q\nEMC\n",
            escape(&lie())
        )
        .as_bytes(),
    );
    kachel_alt.set_content(&raw);

    let mut eigene_still = page(&[]);
    let res = eigene_still.resources_id;
    set_properties(
        &mut eigene_still,
        res,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list(SECRET)) }),
    );
    add_form(
        &mut eigene_still,
        res,
        "Fm0",
        &format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)),
    );
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    eigene_still.set_content(&raw);

    for (name, bytes) in [
        (
            "c1_zwei_umgebungen_eine_seite.pdf",
            zwei_umgebungen_beide_mit_spiegel(&lie(), &lie()),
        ),
        (
            "c1_zwei_umgebungen_eine_seite_still.pdf",
            zwei_umgebungen_beide_mit_spiegel(SECRET, SECRET),
        ),
        (
            "c1_zwei_umgebungen_zwei_seiten.pdf",
            zwei_seiten_zwei_listen("Gruss", &lie()),
        ),
        (
            "c1_zwei_umgebungen_zwei_seiten_still.pdf",
            zwei_seiten_zwei_listen(SECRET, SECRET),
        ),
        ("c2_spiegel_ueber_kachelmuster.pdf", kachel.finish()),
        ("c2_spiegel_ueber_kachelmuster_alt.pdf", kachel_alt.finish()),
        ("c3_eigene_ressourcen_ohne_properties.pdf", eigene.finish()),
        (
            "c3_eigene_ressourcen_ohne_properties_still.pdf",
            eigene_still.finish(),
        ),
    ] {
        std::fs::write(format!("{dir}/{name}"), &bytes).expect("schreibbar");
        println!("{dir}/{name}: {} B", bytes.len());
    }
}

// ===========================================================================
// F. Speicher: Spiegel **in** Formularen, je Seite ein eigenes Formular
// ===========================================================================

/// Ein Dokument mit `pages` Seiten; jede Seite zeichnet ihr **eigenes**
/// Form-XObject, in dem `brackets` verschachtelte Spiegel über `dos`
/// Platzierungen eines winzigen inneren Formulars stehen — je Seite also
/// `brackets × dos` Paare, genau unter der Decke `MAX_MIRROR_FORM_PLACEMENTS`
/// (die gilt je Seite). Die Datensätze der Formulare sammelt
/// `apply_with_report` in `form_marked` — für **jede** Seite, bis zum Ende.
fn spiegel_in_formularen_je_seite(pages: usize, brackets: usize, dos: usize) -> Vec<u8> {
    use lopdf::Document;
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let inner = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        },
        b"BT /F1 10 Tf 72 600 Td (x) Tj ET\n".to_vec(),
    ));
    let mut body = String::new();
    for _ in 0..brackets {
        body.push_str("/Span <</ActualText (x)>> BDC\n");
    }
    for _ in 0..dos {
        body.push_str("/I Do\n");
    }
    for _ in 0..brackets {
        body.push_str("EMC\n");
    }
    let pages_id = doc.new_object_id();
    let mut page_ids = Vec::new();
    for _ in 0..pages {
        let outer = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "I" => inner } },
            },
            body.clone().into_bytes(),
        ));
        let resources = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "XObject" => dictionary! { "F" => outer },
        });
        let content = doc.add_object(Stream::new(dictionary! {}, b"/F Do\n".to_vec()));
        page_ids.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }));
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
            "Count" => pages as i64,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    out
}

fn hwm_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmHWM"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Messung: `ZO_C_PAGES=… cargo test -p redact-pdf --test zo_c_spiegel_umgebungen
/// -- --ignored --nocapture mess_spiegel_in_formularen_je_seite`. Eigener
/// Prozess je Wert, damit VmHWM die Messung ist.
#[test]
#[ignore = "Messung"]
fn mess_spiegel_in_formularen_je_seite() {
    let pages: usize = std::env::var("ZO_C_PAGES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    assert!(pages <= 2_000, "Decke der Messung");
    let bytes = spiegel_in_formularen_je_seite(pages, 100, 999);
    let before = hwm_kb();
    let start = std::time::Instant::now();
    let mut doc = load_from_bytes(&bytes).expect("ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[])
        .expect("Schwärzung");
    let elapsed = start.elapsed();
    println!(
        "Seiten {pages}: Datei {} B, Redaktor {:.1} s, VmHWM {} MB (vorher {} MB), Warnungen {}",
        bytes.len(),
        elapsed.as_secs_f64(),
        hwm_kb() / 1024,
        before / 1024,
        report.warnings.len()
    );
    for w in report.warnings.iter().take(3) {
        println!("  {w}");
    }
}
