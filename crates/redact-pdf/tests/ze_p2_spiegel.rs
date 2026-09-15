//! Gegenprüfung P2 (Fix-Runde 4): der Spiegel selbst.
//!
//! `MarkedTextRecord::forms` trägt seit `f982c12` den **Pfad** der
//! `Do`-Indizes, und `extract::mirror_runs` sortiert danach. Hier wird der
//! Spiegel in beide Richtungen geprüft: lügt er, muss er fallen (`leaks` leer
//! nach der Pipeline); sagt er die Wahrheit, darf keine Warnung entstehen —
//! eine Spiegelwarnung ist eine Deckungslücke und damit Rückgabewert 3.

mod common;

use common::{page, text_ops, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

// ---------------------------------------------------------------------------
// Bausteine
// ---------------------------------------------------------------------------

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

/// Ein Form-XObject mit eigenen Ressourcen (damit weitere Formulare
/// hineingehängt werden können). Liefert (Formular, Ressourcen).
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
    let target = d.doc.get_dictionary_mut(holder).expect("Ressourcen");
    let mut xobjects = target
        .get(b"XObject")
        .and_then(|o| o.as_dict())
        .cloned()
        .unwrap_or_default();
    xobjects.set(name, form_id);
    target.set("XObject", xobjects);
    (form_id, form_resources)
}

fn text_at(y: i32, text: &str) -> String {
    format!("BT /F1 10 Tf 72 {y} Td ({}) Tj ET\n", escape(text))
}

// ---------------------------------------------------------------------------
// Orakel
// ---------------------------------------------------------------------------

fn analyse(bytes: &[u8]) -> (Vec<TextRun>, Vec<String>) {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion")
}

fn mirror_warnings(warnings: &[String]) -> Vec<&String> {
    warnings
        .iter()
        .filter(|w| w.contains("Textspiegel"))
        .collect()
}

fn redactions_for(runs: &[TextRun], needle: &str) -> Vec<Redaction> {
    runs.iter()
        .filter_map(|run| {
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
        })
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

/// Ein lügender Spiegel muss nach der Pipeline verschwunden sein.
fn assert_lie_is_gone(bytes: &[u8], label: &str) {
    let (runs, _) = analyse(bytes);
    let redactions = redactions_for(&runs, SECRET);
    assert!(!redactions.is_empty(), "{label}: der Spiegel ist kein Lauf");
    let (out, _) = pipeline(bytes, &redactions);
    let found = leaks(&out, SECRET);
    assert!(found.is_empty(), "{label}: {found:?}");
}

/// Ein deckungsgleicher Spiegel an ehrlichem Material gibt **keine**
/// Warnung. Jede Spiegelwarnung ist eine Deckungslücke
/// (`redact_pipeline::coverage`) und damit Rückgabewert 3.
fn assert_no_mirror_warning(bytes: &[u8], label: &str) {
    let (_, warnings) = analyse(bytes);
    assert!(
        mirror_warnings(&warnings).is_empty(),
        "{label}: {:?}",
        mirror_warnings(&warnings)
    );
}

// ---------------------------------------------------------------------------
// Lügende Spiegel über mehreren Formularebenen
// ---------------------------------------------------------------------------

/// Spiegel über zwei bzw. drei Formularebenen; die Glyphen liegen ganz unten.
fn nested_levels(levels: usize, mirror: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let resources = d.resources_id;
    // Innerstes Formular trägt den Text.
    let (mut inner, mut inner_res) = add_form(&mut d, resources, "Fm0", &text_at(600, SECRET));
    for level in 1..levels {
        let holder = inner_res;
        let body = format!("q /Fm{} Do Q\n", level - 1);
        let (outer, outer_res) = add_form(&mut d, resources, &format!("Fm{level}"), &body);
        // Das innere Formular gehört in die Ressourcen des äußeren.
        let target = d.doc.get_dictionary_mut(outer_res).expect("Ressourcen");
        let mut xobjects = target
            .get(b"XObject")
            .and_then(|o| o.as_dict())
            .cloned()
            .unwrap_or_default();
        xobjects.set(format!("Fm{}", level - 1), inner);
        target.set("XObject", xobjects);
        let _ = holder;
        inner = outer;
        inner_res = outer_res;
    }
    let top = format!("Fm{}", levels - 1);
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!(
            "/Span <</ActualText ({})>> BDC\nq /{top} Do Q\nEMC\n",
            escape(mirror)
        )
        .as_bytes(),
    );
    d.set_content(&raw);
    d.finish()
}

#[test]
fn luegender_spiegel_ueber_zwei_und_drei_ebenen_faellt() {
    for levels in [2usize, 3] {
        let bytes = nested_levels(levels, &format!("Zahlung an {SECRET}"));
        assert_lie_is_gone(&bytes, &format!("{levels} Ebenen, lügend"));
    }
}

#[test]
fn deckungsgleicher_spiegel_ueber_zwei_und_drei_ebenen_warnt_nicht() {
    for levels in [2usize, 3] {
        let bytes = nested_levels(levels, SECRET);
        assert_no_mirror_warning(&bytes, &format!("{levels} Ebenen, deckungsgleich"));
    }
}

// ---------------------------------------------------------------------------
// Dasselbe Formular zweimal im selben Spiegel
// ---------------------------------------------------------------------------

/// Ein Formular, das der Spiegel **zweimal** überdeckt. Der Spiegel nennt
/// beide Vorkommen — das ist genau das, was ein Betrachter sieht.
///
/// `nested` schaltet ein von diesem Spiegel unabhängiges Formular-im-Formular
/// dazu: erst dann läuft `ScanResult::close_forms`, und nur dann greift
/// dessen `seen`-Menge.
fn same_form_twice(nested: bool) -> Vec<u8> {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (_, _) = add_form(&mut d, resources, "Fm0", &text_at(600, "Alpha"));
    if nested {
        // Ein zweites, vom Spiegel unberührtes Formular, das seinerseits ein
        // Formular zeichnet.
        let (_, outer_res) = add_form(&mut d, resources, "Fm9", "q /Fm8 Do Q\n");
        add_form(&mut d, outer_res, "Fm8", &text_at(400, "Beta"));
    }
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        b"/Span <</ActualText (AlphaAlpha)>> BDC\nq /Fm0 Do Q\nq 1 0 0 1 0 -20 cm /Fm0 Do Q\nEMC\n",
    );
    if nested {
        raw.extend_from_slice(b"q /Fm9 Do Q\n");
    }
    d.set_content(&raw);
    d.finish()
}

/// Ohne Verschachtelung im Dokument stimmt der Spiegel mit den Glyphen
/// überein und gibt keine Warnung — die Glyphen beider Platzierungen zählen.
#[test]
fn dasselbe_formular_zweimal_im_spiegel_warnt_nicht() {
    assert_no_mirror_warning(&same_form_twice(false), "zweimal, ohne Verschachtelung");
}

/// **Befund P2-3 (Fix-Runde 5 geschlossen, älter als Fix-Runde 4).** Sobald irgendwo im
/// Dokument ein Formular ein Formular zeichnet, läuft
/// `ScanResult::close_forms` — und dessen `seen`-Menge (eine Menge über
/// Objekt-Ids, geteilt über alle Platzierungen eines Abschnitts) wirft die
/// **zweite** Platzierung desselben Formulars weg. Derselbe ehrliche Spiegel
/// gilt dann als Widerspruch: „10 Zeichen im Spiegel, 5 in den Glyphen“.
/// Diese Warnung steht nicht in `redact_pipeline::coverage::NOT_A_COVERAGE_GAP`
/// und wurde deshalb zum Rückgabewert 3 an einer harmlosen Datei.
///
/// `close_forms` zählt seit Fix-Runde 5 **Platzierungen**: der Zyklusschutz
/// ist die Kette der Vorfahren des Pfades, nicht eine Menge über den ganzen
/// Datensatz.
#[test]
fn befund_dasselbe_formular_zweimal_im_spiegel_warnt_falsch() {
    assert_no_mirror_warning(&same_form_twice(true), "zweimal, mit Verschachtelung");
}

/// Die Gegenprobe zum Pfad (Befund G1-A3 der Fix-Runde 4): ein Formular
/// zeichnet **erst** ein inneres Formular und **dann** eigenen Text. Der
/// Spiegel nennt beide in Stromreihenfolge; nur der Pfad der `Do`-Indizes
/// ordnet das richtig ein.
#[test]
fn inneres_formular_vor_eigenem_text_ist_kein_widerspruch() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, "Alpha"));
    let (_, outer_res) = add_form(
        &mut d,
        resources,
        "Fm0",
        &format!("q /Fm1 Do Q\n{}", text_at(580, "Beta")),
    );
    d.doc
        .get_dictionary_mut(outer_res)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Fm1" => inner });

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"/Span <</ActualText (AlphaBeta)>> BDC\nq /Fm0 Do Q\nEMC\n");
    d.set_content(&raw);
    let bytes = d.finish();
    assert_no_mirror_warning(&bytes, "inneres Formular vor eigenem Text");
}

/// Und dieselbe Datei mit umgekehrter Reihenfolge im Formular: erst eigener
/// Text, dann das innere Formular.
#[test]
fn eigener_text_vor_innerem_formular_ist_kein_widerspruch() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, "Alpha"));
    let (_, outer_res) = add_form(
        &mut d,
        resources,
        "Fm0",
        &format!("{}q /Fm1 Do Q\n", text_at(580, "Beta")),
    );
    d.doc
        .get_dictionary_mut(outer_res)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Fm1" => inner });

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"/Span <</ActualText (BetaAlpha)>> BDC\nq /Fm0 Do Q\nEMC\n");
    d.set_content(&raw);
    let bytes = d.finish();
    assert_no_mirror_warning(&bytes, "eigener Text vor innerem Formular");
}

/// Material für den Befund als Datei — für den Lauf über die Kommandozeile.
/// Schreibt nur, wenn `ZE_P2_OUT` gesetzt ist.
#[test]
fn schreibt_material_fuer_den_befund() {
    let Ok(dir) = std::env::var("ZE_P2_OUT") else {
        return;
    };
    std::fs::write(
        format!("{dir}/zweimal_mit_verschachtelung.pdf"),
        same_form_twice(true),
    )
    .expect("schreibbar");
    std::fs::write(
        format!("{dir}/zweimal_ohne_verschachtelung.pdf"),
        same_form_twice(false),
    )
    .expect("schreibbar");
}

// ---------------------------------------------------------------------------
// Spiegel im Formular über einem Formular, das auch direkt auf der Seite steht
// ---------------------------------------------------------------------------

#[test]
fn spiegel_im_formular_ueber_einem_auch_direkt_platzierten_formular() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, SECRET));
    let (_, outer_res) = add_form(
        &mut d,
        resources,
        "Fm0",
        &format!(
            "/Span <</ActualText ({})>> BDC\nq /Fm1 Do Q\nEMC\n",
            escape(&format!("Zahlung an {SECRET}"))
        ),
    );
    let target = d.doc.get_dictionary_mut(outer_res).expect("Ressourcen");
    target.set("XObject", dictionary! { "Fm1" => inner });

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    // Dasselbe innere Formular auch direkt auf der Seite.
    raw.extend_from_slice(b"q 1 0 0 1 0 -40 cm /Fm1 Do Q\nq /Fm0 Do Q\n");
    d.set_content(&raw);
    let bytes = d.finish();
    assert_lie_is_gone(&bytes, "Spiegel im Formular über geteiltem Formular");
}

// ---------------------------------------------------------------------------
// /ActualText als Verweis
// ---------------------------------------------------------------------------

#[test]
fn actualtext_als_verweis_wird_gelesen_und_geleert() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    add_form(&mut d, resources, "Fm0", &text_at(600, "Alpha"));
    let secret_obj = d.add(Object::string_literal(format!("Zahlung an {SECRET}")));
    let props = d.add(Object::Dictionary(dictionary! {
        "ActualText" => Object::Reference(secret_obj),
    }));
    let target = d.doc.get_dictionary_mut(resources).expect("Ressourcen");
    target.set("Properties", dictionary! { "MC0" => props });

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"/Span /MC0 BDC\nq /Fm0 Do Q\nEMC\n");
    d.set_content(&raw);
    let bytes = d.finish();
    assert_lie_is_gone(&bytes, "/ActualText als Verweis");
}

// ---------------------------------------------------------------------------
// Spiegel über einem Bild und über einem Formular ohne Glyphen
// ---------------------------------------------------------------------------

/// `/Figure <</Alt …>> BDC /Im0 Do EMC` ist die Standardform der
/// Barrierefreiheit — sie darf nie eine Warnung geben.
#[test]
fn spiegel_ueber_einem_bild_warnt_nicht() {
    let mut d = page(&[]);
    let image = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2_i64,
                "Height" => 2_i64,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8_i64,
            },
            vec![0x00, 0xff, 0x7f, 0x30],
        )
        .with_compression(false),
    ));
    let resources = d.resources_id;
    d.doc
        .get_dictionary_mut(resources)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Im0" => image });

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        b"/Figure <</Alt (Logo der Musterbank)>> BDC\nq 20 0 0 20 300 780 cm /Im0 Do Q\nEMC\n",
    );
    d.set_content(&raw);
    let bytes = d.finish();
    assert_no_mirror_warning(&bytes, "Spiegel über /Im0");
}

/// Ein `/ActualText` über einem Formular **ohne** Glyphen: dafür gibt es die
/// Warnung mit dem ausdrücklichen Zusatz, dass nicht durchsucht wurde.
#[test]
fn spiegel_ueber_formular_ohne_glyphen_sagt_dass_nicht_durchsucht_wurde() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    add_form(&mut d, resources, "Fm0", "q 1 0 0 1 0 0 cm Q\n");
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!(
            "/Span <</ActualText ({})>> BDC\nq /Fm0 Do Q\nEMC\n",
            escape(SECRET)
        )
        .as_bytes(),
    );
    d.set_content(&raw);
    let bytes = d.finish();
    let (_, warnings) = analyse(&bytes);
    let mirror = mirror_warnings(&warnings);
    assert_eq!(mirror.len(), 1, "{warnings:?}");
    assert!(
        mirror[0].contains("wurde nicht durchsucht"),
        "{}",
        mirror[0]
    );
}

// ---------------------------------------------------------------------------
// Unregelmäßige BDC/EMC-Klammerung
// ---------------------------------------------------------------------------

/// Verschachtelte `BDC` mit verschiedenen Schlüsseln, `EMC` ohne `BDC`,
/// `BDC` ohne `EMC` bis Stromende — an jedem dieser Ströme darf das
/// Geheimnis nach der Pipeline nicht mehr stehen.
#[test]
fn unregelmaessige_klammerung_laesst_nichts_stehen() {
    let lie = escape(&format!("Zahlung an {SECRET}"));
    let cases: Vec<(&str, String)> = vec![
        (
            "verschachtelt, verschiedene Schlüssel",
            format!(
                "/Figure <</Alt (Bild)>> BDC\n/Span <</ActualText ({lie})>> BDC\n{}EMC\nEMC\n",
                text_at(600, SECRET)
            ),
        ),
        (
            "EMC ohne BDC",
            format!(
                "EMC\n/Span <</ActualText ({lie})>> BDC\n{}EMC\n",
                text_at(600, SECRET)
            ),
        ),
        (
            "BDC ohne EMC bis Stromende",
            format!(
                "/Span <</ActualText ({lie})>> BDC\n{}",
                text_at(600, SECRET)
            ),
        ),
    ];
    for (label, body) in cases {
        let mut d = page(&[]);
        let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
        raw.extend_from_slice(body.as_bytes());
        d.set_content(&raw);
        let bytes = d.finish();
        assert_lie_is_gone(&bytes, label);
    }
}

// ---------------------------------------------------------------------------
// Gegenrichtung: ehrliches, getaggtes Material
// ---------------------------------------------------------------------------

/// Eine Word-artige getaggte Seite: `/P`- und `/Span`-Abschnitte mit
/// deckungsgleichem `/ActualText`, ein Bild mit `/Alt`, ein Textbaustein in
/// einem Formular mit eigenem Spiegel. Kein Wort davon darf eine Warnung
/// geben.
#[test]
fn ehrliche_getaggte_datei_gibt_keine_warnung() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    add_form(&mut d, resources, "Fm0", &text_at(500, "Musterbank AG"));
    let image = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2_i64,
                "Height" => 2_i64,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8_i64,
            },
            vec![0x00, 0xff, 0x7f, 0x30],
        )
        .with_compression(false),
    ));
    let target = d.doc.get_dictionary_mut(resources).expect("Ressourcen");
    let mut xobjects = target
        .get(b"XObject")
        .and_then(|o| o.as_dict())
        .cloned()
        .unwrap_or_default();
    xobjects.set("Im0", image);
    target.set("XObject", xobjects);

    let mut raw = Vec::new();
    raw.extend_from_slice(b"/P <</MCID 0>> BDC\n");
    raw.extend_from_slice(text_at(700, "Kontoauszug Januar 2026").as_bytes());
    raw.extend_from_slice(b"EMC\n");
    // Ligatur: der Spiegel schreibt aus, was die Glyphen zeigen.
    raw.extend_from_slice(b"/Span <</ActualText (Auflage)>> BDC\n");
    raw.extend_from_slice(text_at(680, "Auflage").as_bytes());
    raw.extend_from_slice(b"EMC\n");
    raw.extend_from_slice(b"/Figure <</Alt (Logo der Musterbank)>> BDC\n");
    raw.extend_from_slice(b"q 20 0 0 20 300 780 cm /Im0 Do Q\n");
    raw.extend_from_slice(b"EMC\n");
    raw.extend_from_slice(b"/Span <</ActualText (Musterbank AG)>> BDC\n");
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    raw.extend_from_slice(b"EMC\n");
    d.set_content(&raw);
    let bytes = d.finish();

    let (_, warnings) = analyse(&bytes);
    assert!(
        mirror_warnings(&warnings).is_empty(),
        "{:?}",
        mirror_warnings(&warnings)
    );

    // Und der ganze Weg: nichts zu schwärzen, keine Ausgabe voller Warnungen.
    let (_, report_warnings) = pipeline(&bytes, &[]);
    assert!(
        mirror_warnings(&report_warnings).is_empty(),
        "{report_warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Dasselbe Formular zweimal **im** Formular unter dem Spiegel
// ---------------------------------------------------------------------------

/// Derselbe Befund eine Ebene tiefer: der Spiegel steht über **einem** `Do`,
/// und das Formular dahinter zeichnet das innere zweimal. Ein Betrachter
/// sieht „Alpha“ zweimal, der Spiegel schreibt „AlphaAlpha“ — kein
/// Widerspruch. Eine Entdopplung über Objekt-Ids (gleich ob je Datensatz oder
/// je Platzierung) zählte hier fünf statt zehn Zeichen.
#[test]
fn inneres_formular_zweimal_gezeichnet_warnt_nicht() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, "Alpha"));
    let (_, outer_res) = add_form(
        &mut d,
        resources,
        "Fm0",
        "q /Fm1 Do Q\nq 1 0 0 1 0 -20 cm /Fm1 Do Q\n",
    );
    d.doc
        .get_dictionary_mut(outer_res)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Fm1" => inner });

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"/Span <</ActualText (AlphaAlpha)>> BDC\nq /Fm0 Do Q\nEMC\n");
    d.set_content(&raw);
    let bytes = d.finish();
    assert_no_mirror_warning(&bytes, "inneres Formular zweimal gezeichnet");
}

/// Und dieselbe Datei mit einem **lügenden** Spiegel darüber: die Schließung
/// darf die Glyphen nicht nur richtig zählen, sie muss sie auch weiterhin
/// finden — der Spiegel fällt.
#[test]
fn inneres_formular_zweimal_gezeichnet_luegender_spiegel_faellt() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, SECRET));
    let (_, outer_res) = add_form(
        &mut d,
        resources,
        "Fm0",
        "q /Fm1 Do Q\nq 1 0 0 1 0 -20 cm /Fm1 Do Q\n",
    );
    d.doc
        .get_dictionary_mut(outer_res)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Fm1" => inner });

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!(
            "/Span <</ActualText ({})>> BDC\nq /Fm0 Do Q\nEMC\n",
            escape(&format!("Zahlung an {SECRET}"))
        )
        .as_bytes(),
    );
    d.set_content(&raw);
    let bytes = d.finish();
    assert_lie_is_gone(&bytes, "inneres Formular zweimal, lügender Spiegel");
}

/// Beides zusammen: der Spiegel überdeckt **zwei** Platzierungen desselben
/// Formulars, und dieses zeichnet seinerseits ein inneres. Zwei Platzierungen
/// × „Alpha“ = „AlphaAlpha“.
///
/// Der Fall hält die Kanten in `ScanResult::nested_forms` fest: das äußere
/// Formular wird zweimal durchlaufen und meldet sein `Do` dabei zweimal.
/// Stünde die Kante deshalb zweimal in der Liste, zählten die Glyphen des
/// inneren Formulars doppelt — zwanzig statt zehn Zeichen.
#[test]
fn zwei_platzierungen_mit_innerem_formular_warnen_nicht() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, "Alpha"));
    let (_, outer_res) = add_form(&mut d, resources, "Fm0", "q /Fm1 Do Q\n");
    d.doc
        .get_dictionary_mut(outer_res)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Fm1" => inner });

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        b"/Span <</ActualText (AlphaAlpha)>> BDC\nq /Fm0 Do Q\nq 1 0 0 1 0 -20 cm /Fm0 Do Q\nEMC\n",
    );
    d.set_content(&raw);
    let bytes = d.finish();
    assert_no_mirror_warning(&bytes, "zwei Platzierungen mit innerem Formular");
}

// ---------------------------------------------------------------------------
// Gegenrichtung: der Zyklus
// ---------------------------------------------------------------------------

/// Ein Formular, das **sich selbst** zeichnet, unter einem Spiegel: die
/// Schließung muss anhalten. Der Interpreter betritt den Zyklus über
/// `visiting` gar nicht erst, die Glyphen stehen also einmal da, und der
/// deckungsgleiche Spiegel warnt nicht.
///
/// Ohne den Zyklusschutz in `close_forms` läuft dieser Test nicht rot,
/// sondern gar nicht mehr zu Ende.
#[test]
fn formular_das_sich_selbst_zeichnet_haelt_an() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, "Alpha"));
    // Ein zweites Formular macht die Schließung überhaupt erst nötig.
    let (_, outer_res) = add_form(&mut d, resources, "Fm0", "q /Fm1 Do Q\n");
    d.doc
        .get_dictionary_mut(outer_res)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Fm1" => inner });
    // Und jetzt zeichnet das innere Formular sich selbst.
    let mut body = text_at(600, "Alpha").into_bytes();
    body.extend_from_slice(b"q /Fm1 Do Q\n");
    let inner_res = d
        .doc
        .get_object(inner)
        .and_then(|o| o.as_stream())
        .expect("Formular")
        .dict
        .get(b"Resources")
        .and_then(|o| o.as_reference())
        .expect("Ressourcen");
    d.doc
        .get_dictionary_mut(inner_res)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Fm1" => inner });
    let stream = d
        .doc
        .get_object_mut(inner)
        .and_then(|o| o.as_stream_mut())
        .expect("Formular");
    stream.set_content(body);

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"/Span <</ActualText (Alpha)>> BDC\nq /Fm0 Do Q\nEMC\n");
    d.set_content(&raw);
    let bytes = d.finish();
    assert_no_mirror_warning(&bytes, "Formular zeichnet sich selbst");
}

// ---------------------------------------------------------------------------
// Die Decke über den Platzierungen
// ---------------------------------------------------------------------------

/// Eine Seite mit `outer` Platzierungen eines Formulars unter **einem**
/// Spiegel; jedes davon zeichnet ein inneres Formular `inner_placements`-mal.
/// Die Schließung zählt daraus `outer * (1 + inner_placements)` Platzierungen.
fn many_placements(outer: usize, inner_placements: usize) -> Vec<u8> {
    many_placements_maybe_mirrored(outer, inner_placements, true)
}

/// Dieselbe Seite, wahlweise **ohne** den Spiegel darüber — dann läuft die
/// Schließung leer, und übrig bleibt der Aufwand des Interpreters selbst.
fn many_placements_maybe_mirrored(
    outer: usize,
    inner_placements: usize,
    mirrored: bool,
) -> Vec<u8> {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, "A"));
    let body = "q /Fm1 Do Q\n".repeat(inner_placements);
    let (_, outer_res) = add_form(&mut d, resources, "Fm0", &body);
    d.doc
        .get_dictionary_mut(outer_res)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Fm1" => inner });

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    if mirrored {
        raw.extend_from_slice(b"/Span <</ActualText (Alpha)>> BDC\n");
    }
    raw.extend_from_slice("q /Fm0 Do Q\n".repeat(outer).as_bytes());
    if mirrored {
        raw.extend_from_slice(b"EMC\n");
    }
    d.set_content(&raw);
    d.finish()
}

/// Über der Decke (100 000 Aufklappungen) sagt der Scan, dass er nicht mehr
/// zugeordnet hat — statt still einen falschen Vergleich zu ziehen.
///
/// 10 000 × 11 = 110 000 Aufklappungen aus einer Datei von wenigen Kilobyte.
/// Ohne Decke hinge der Umfang der Schließung an nichts mehr.
///
/// **Zahl in Fix-Runde 6 angehoben** (vorher 10 000 × 10). Die alte Datei
/// klappte genau 100 000 Kanten auf und verlor damit **nichts** — sie warnte
/// nur, weil die Decke schon am Blatt gefragt wurde, das gar keine Kinder
/// hat. Genau dieser falsche Alarm ist Befund Q3-1b
/// (`zf_q3_spiegel::befund_decke_warnt_bei_genau_aufgehender_zahl`); der Test
/// hier hing an ihm und hätte ihn sonst wieder eingefordert.
#[test]
fn ueber_der_decke_sagt_der_scan_es_an() {
    let (_, warnings) = analyse(&many_placements(10_000, 11));
    assert!(
        warnings.iter().any(
            |w| w.contains("Zuordnungen zwischen einem Spiegel") && w.contains("unvollständig")
        ),
        "keine Warnung über die Decke: {warnings:?}"
    );
}

/// Und darunter (10 000 × (1 + 1) = 20 000) läuft dieselbe Seite ohne ein
/// Wort durch — die Decke darf gewöhnliches Material nicht anfassen.
#[test]
fn unter_der_decke_bleibt_es_still() {
    let (_, warnings) = analyse(&many_placements(10_000, 1));
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("Zuordnungen zwischen einem Spiegel")),
        "Warnung unter der Decke: {warnings:?}"
    );
}

/// Ein Formular, das seinen Spiegel selbst trägt, wird viermal platziert.
///
/// `scan_marked_text` lief bis Fix-Runde 6 bei **jeder** Platzierung erneut
/// über denselben Strom — der Datensatz entdoppelt zwar (`ScanResult::marked`
/// über `(Strom, Operationsindex)`), die neue Decke hätte aber viermal
/// gezählt: 4 × 26 000 = 104 000 Paare aus einem Datensatz, der am Ende
/// 26 000 führt, und damit eine Warnung über eine Deckungslücke, die es nicht
/// gibt. Gezählt wird deshalb einmal je Strom.
#[test]
fn dasselbe_formular_viermal_platziert_zaehlt_seine_spiegel_einmal() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, "A"));
    let body = format!(
        "/Span <</ActualText ({})>> BDC\n{}EMC\n",
        "A".repeat(26_000),
        "/Fm1 Do\n".repeat(26_000)
    );
    let (_, outer_res) = add_form(&mut d, resources, "Fm0", &body);
    d.doc
        .get_dictionary_mut(outer_res)
        .expect("Ressourcen")
        .set("XObject", dictionary! { "Fm1" => inner });

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice("q /Fm0 Do Q\n".repeat(4).as_bytes());
    d.set_content(&raw);
    let bytes = d.finish();

    let (_, warnings) = analyse(&bytes);
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("Zuordnungen zwischen einem Spiegel")),
        "Warnung über die Decke an 26 000 Paaren: {warnings:?}"
    );
}

/// Messung (ignoriert): was 10 000 Platzierungen desselben Formulars unter
/// einem Spiegel kosten. Lauf:
/// `cargo test -p redact-pdf --test ze_p2_spiegel -- --ignored --nocapture`
#[test]
#[ignore = "Messung"]
fn mess_zehntausend_platzierungen() {
    for (outer, inner, mirrored) in [
        (10_000usize, 0usize, false),
        (10_000, 0, true),
        (10_000, 1, false),
        (10_000, 1, true),
        (10_000, 10, false),
        (10_000, 10, true),
    ] {
        let bytes = many_placements_maybe_mirrored(outer, inner, mirrored);
        let start = std::time::Instant::now();
        let (_, warnings) = analyse(&bytes);
        println!(
            "{outer} × (1 + {inner}) Platzierungen, Spiegel {mirrored}: {:?}, \
             {} Warnung(en), Spitzenspeicher {}",
            start.elapsed(),
            warnings.len(),
            std::fs::read_to_string("/proc/self/status")
                .unwrap_or_default()
                .lines()
                .find(|l| l.starts_with("VmHWM"))
                .unwrap_or("VmHWM: ?")
                .trim()
        );
    }
}

// ---------------------------------------------------------------------------
// Die Warnung über indirekte Verweise steht einmal
// ---------------------------------------------------------------------------

/// Eine Eigenschaftsliste, die über `/Resources /Properties` erreichbar ist,
/// **ohne** ein eigenes Objekt zu sein, darf neben dem Spiegel indirekte
/// Verweise führen; beim Neuschreiben inline in den Strom müssen sie
/// entfallen, und das wird gemeldet.
///
/// Seit `f982c12` kann diese Meldung an derselben Datei **zweimal** entstehen:
/// einmal in der Seitenschleife, einmal in der Formularschleife (und ein
/// drittes Mal für die nach der Formularschleife nachgereichten Abschnitte).
/// Hier steht dieselbe Liste im Seitenstrom **und** im Formular darunter, und
/// beide verlieren ihren Spiegel durch dieselbe Schwärzung. Der Bericht darf
/// den Satz trotzdem nur einmal führen.
///
/// `z8_warnungen_entdoppelt.rs` deckt diesen Weg nicht ab: dort geht es um
/// `ScanResult::warn` und `ops::note`, nicht um `redact::push_warning`.
fn shared_property_list(d: &mut Doc, holder: ObjectId, mirror: &str) {
    let target = d.add(Object::string_literal("Beiwerk"));
    let list = dictionary! {
        "ActualText" => Object::string_literal(mirror.to_string()),
        "Foo" => Object::Reference(target),
    };
    let holder_dict = d.doc.get_dictionary_mut(holder).expect("Ressourcen");
    holder_dict.set("Properties", dictionary! { "MC0" => list });
}

#[test]
fn warnung_ueber_indirekte_verweise_steht_genau_einmal() {
    let lie = format!("Zahlung an {SECRET}");
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (_, form_resources) = add_form(
        &mut d,
        resources,
        "Fm0",
        &format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)),
    );
    shared_property_list(&mut d, form_resources, &lie);
    shared_property_list(&mut d, resources, &lie);

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"/Span /MC0 BDC\nq /Fm0 Do Q\nEMC\n");
    d.set_content(&raw);
    let bytes = d.finish();

    let (runs, _) = analyse(&bytes);
    let redactions = redactions_for(&runs, SECRET);
    assert!(!redactions.is_empty(), "nichts zu schwärzen");
    let (out, warnings) = pipeline(&bytes, &redactions);

    let hits: Vec<&String> = warnings
        .iter()
        .filter(|w| w.contains("indirekte Verweise"))
        .collect();
    assert_eq!(hits.len(), 1, "{warnings:?}");
    assert!(hits[0].contains("/Foo"), "{}", hits[0]);
    // Seit Fix-Runde 6 ist auch das Ressourcenverzeichnis bereinigt — beide
    // Fundorte, der im Seitenstrom und der im Formular (siehe
    // `befund_direkte_eigenschaftsliste_behaelt_ihren_spiegel`).
    let found = leaks(&out, SECRET);
    assert!(found.is_empty(), "{found:?}");
}

/// **Befund aus Fix-Runde 5, in Runde 6 geschlossen.** Eine Eigenschaftsliste,
/// die als **direktes** Dictionary in `/Resources /Properties` steht, verlor
/// ihren Spiegel nur im Strom — im Ressourcenverzeichnis blieb er stehen:
///
/// ```text
/// Objekt 2 0/Properties/MC0/ActualText [Zeichenkette, literal]: …Zahlung an DE89 …
/// ```
///
/// Ohne Warnung, mit Rückgabewert 0. `mirror_property_list` liefert für so
/// eine Liste `property_id == None` (sie ist kein eigenes Objekt), und
/// `mirrors_to_clear` schickte sie deshalb denselben Weg wie eine inline im
/// Strom stehende Liste: `rebuild_marked` schrieb die bereinigte Fassung
/// inline an die Stelle des `/MC0` und ließ den Eintrag in den Ressourcen
/// stehen.
///
/// Der Datensatz trägt jetzt zusätzlich den **Ressourcennamen**
/// (`MarkedTextRecord::property_name`), und `content::property_list_home`
/// sucht dazu den Fundort im Dokument — dieselbe Suche für alle vier Wege
/// (Seite, Formular, geerbt vom Seitenbaum, geteiltes `/Properties`-Objekt,
/// siehe `zf_q3_properties.rs`).
#[test]
fn befund_direkte_eigenschaftsliste_behaelt_ihren_spiegel() {
    let lie = format!("Zahlung an {SECRET}");
    let mut d = page(&[]);
    let resources = d.resources_id;
    let list = dictionary! { "ActualText" => Object::string_literal(lie.clone()) };
    d.doc
        .get_dictionary_mut(resources)
        .expect("Ressourcen")
        .set("Properties", dictionary! { "MC0" => list });

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"/Span /MC0 BDC\n");
    raw.extend_from_slice(text_at(600, SECRET).as_bytes());
    raw.extend_from_slice(b"EMC\n");
    d.set_content(&raw);
    let bytes = d.finish();

    let (runs, _) = analyse(&bytes);
    let redactions = redactions_for(&runs, SECRET);
    assert!(!redactions.is_empty(), "nichts zu schwärzen");
    let (out, warnings) = pipeline(&bytes, &redactions);
    let found = leaks(&out, SECRET);
    assert!(found.is_empty(), "Warnungen {warnings:?}, Lecks {found:?}");
}
