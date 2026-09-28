//! Gegenprüfung Q3 (Fix-Runde 5): die Schließung über den Formularen.
//!
//! `ScanResult::close_forms` zählt seit `2b92bee` **Platzierungen** statt
//! Formulare; der Zyklusschutz ist die Kette der Vorfahren des Pfades.
//! Hier steht eigenes Material dagegen, in beide Richtungen:
//!
//! * **lügend** — nach der Pipeline muss `leaks(out, SECRET)` leer sein;
//! * **deckungsgleich** — an ehrlichem Material darf keine Spiegelwarnung
//!   entstehen (jede ist eine Deckungslücke und damit Rückgabewert 3).

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

/// Ein Form-XObject mit eigenen Ressourcen. Liefert (Formular, Ressourcen).
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

/// Hängt ein bestehendes Objekt unter `name` in die `/XObject`-Liste von
/// `holder` (ohne die vorhandenen Einträge zu verlieren).
fn link(d: &mut Doc, holder: ObjectId, name: &str, target: ObjectId) {
    let dict = d.doc.get_dictionary_mut(holder).expect("Ressourcen");
    let mut xobjects = dict
        .get(b"XObject")
        .and_then(|o| o.as_dict())
        .cloned()
        .unwrap_or_default();
    xobjects.set(name, target);
    dict.set("XObject", xobjects);
}

fn text_at(y: i32, text: &str) -> String {
    format!("BT /F1 10 Tf 72 {y} Td ({}) Tj ET\n", escape(text))
}

/// Ein winziges Graustufenbild als XObject.
fn add_image(d: &mut Doc, holder: ObjectId, name: &str) {
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
    link(d, holder, name, image);
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

/// Ehrliches Material darf keine Spiegelwarnung geben.
fn assert_no_mirror_warning(bytes: &[u8], label: &str) {
    let (_, warnings) = analyse(bytes);
    assert!(
        mirror_warnings(&warnings).is_empty(),
        "{label}: {:?}",
        mirror_warnings(&warnings)
    );
}

/// Was unter dem Spiegel wirklich zusammenkommt — die Warnung nennt beide
/// Zahlen, also lässt sich der Vergleich daran ablesen.
fn glyphs_under_mirror(bytes: &[u8], mirror: &str) -> Option<usize> {
    let (_, warnings) = analyse(bytes);
    let w = warnings.iter().find(|w| w.contains("Textspiegel"))?;
    let _ = mirror;
    let tail = w.split(" Zeichen im Spiegel, ").nth(1)?;
    tail.split(" in den Glyphen")
        .next()?
        .trim()
        .parse::<usize>()
        .ok()
}

// ---------------------------------------------------------------------------
// 1. Dasselbe Formular drei- und viermal unter einem Spiegel
// ---------------------------------------------------------------------------

fn same_form_n_times(n: usize, mirror: &str, body: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let resources = d.resources_id;
    add_form(&mut d, resources, "Fm0", body);
    // Ein unbeteiligtes Formular-im-Formular: erst dann läuft `close_forms`.
    let (_, outer_res) = add_form(&mut d, resources, "Fm9", "q /Fm8 Do Q\n");
    add_form(&mut d, outer_res, "Fm8", &text_at(300, "Beiwerk"));

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(format!("/Span <</ActualText ({})>> BDC\n", escape(mirror)).as_bytes());
    for i in 0..n {
        raw.extend_from_slice(format!("q 1 0 0 1 0 {} cm /Fm0 Do Q\n", -20 * i as i32).as_bytes());
    }
    raw.extend_from_slice(b"EMC\nq /Fm9 Do Q\n");
    d.set_content(&raw);
    d.finish()
}

/// Drei- und viermal dasselbe Formular unter einem Spiegel: der Spiegel nennt
/// den Text drei- bzw. viermal, und das ist kein Widerspruch.
#[test]
fn dasselbe_formular_drei_und_viermal_warnt_nicht() {
    for n in [3usize, 4] {
        let bytes = same_form_n_times(n, &"Alpha".repeat(n), &text_at(600, "Alpha"));
        assert_no_mirror_warning(&bytes, &format!("{n}× dasselbe Formular"));
    }
}

/// Und die Zahl stimmt wirklich: ein zu kurzer Spiegel meldet genau
/// `n × 5` Glyphen darunter — nicht `5`.
#[test]
fn dasselbe_formular_dreimal_zaehlt_alle_glyphen() {
    let bytes = same_form_n_times(3, "XYZ", &text_at(600, "Alpha"));
    assert_eq!(
        glyphs_under_mirror(&bytes, "XYZ"),
        Some(15),
        "drei Platzierungen à 5 Glyphen"
    );
}

/// Lügender Spiegel über drei Platzierungen: er muss fallen.
#[test]
fn dasselbe_formular_dreimal_luegender_spiegel_faellt() {
    let bytes = same_form_n_times(3, &format!("Zahlung an {SECRET}"), &text_at(600, "Alpha"));
    assert_lie_is_gone(&bytes, "dreimal, lügender Spiegel");
}

// ---------------------------------------------------------------------------
// 2. Dasselbe Formular in zwei verschiedenen Abschnitten
// ---------------------------------------------------------------------------

/// Zwei Spiegel-Abschnitte, jeder mit **einer** Platzierung desselben
/// Formulars. Die Schließung darf die zweite nicht dem ersten Abschnitt
/// zuschlagen und nicht wegen des ersten Abschnitts wegwerfen.
fn same_form_two_sections(mirror_a: &str, mirror_b: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, "Alpha"));
    let (_, outer_res) = add_form(&mut d, resources, "Fm0", "q /Fm1 Do Q\n");
    link(&mut d, outer_res, "Fm1", inner);

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!(
            "/Span <</ActualText ({})>> BDC\nq /Fm0 Do Q\nEMC\n",
            escape(mirror_a)
        )
        .as_bytes(),
    );
    raw.extend_from_slice(
        format!(
            "/Span <</ActualText ({})>> BDC\nq 1 0 0 1 0 -30 cm /Fm0 Do Q\nEMC\n",
            escape(mirror_b)
        )
        .as_bytes(),
    );
    d.set_content(&raw);
    d.finish()
}

#[test]
fn dasselbe_formular_in_zwei_abschnitten_warnt_nicht() {
    assert_no_mirror_warning(&same_form_two_sections("Alpha", "Alpha"), "zwei Abschnitte");
}

#[test]
fn dasselbe_formular_in_zwei_abschnitten_luegender_zweiter_faellt() {
    let bytes = same_form_two_sections("Alpha", &format!("Zahlung an {SECRET}"));
    assert_lie_is_gone(&bytes, "zwei Abschnitte, der zweite lügt");
}

// ---------------------------------------------------------------------------
// 3. Der Diamant: zwei Wege zu demselben inneren Formular
// ---------------------------------------------------------------------------

/// `Fm0` zeichnet `Fm1` und `Fm2`, beide zeichnen `Fm3`. Ein Betrachter sieht
/// den Text von `Fm3` **zweimal** — der Spiegel nennt ihn zweimal.
fn diamond(mirror: &str, with_own_glyphs: bool) -> Vec<u8> {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (deep, _) = add_form(&mut d, resources, "Fm3", &text_at(600, "Delta"));
    let left_body = if with_own_glyphs {
        format!("{}q /Fm3 Do Q\n", text_at(580, "L"))
    } else {
        "q /Fm3 Do Q\n".into()
    };
    let right_body = if with_own_glyphs {
        format!("q /Fm3 Do Q\n{}", text_at(560, "R"))
    } else {
        "q /Fm3 Do Q\n".into()
    };
    let (left, left_res) = add_form(&mut d, resources, "Fm1", &left_body);
    let (right, right_res) = add_form(&mut d, resources, "Fm2", &right_body);
    link(&mut d, left_res, "Fm3", deep);
    link(&mut d, right_res, "Fm3", deep);
    let (_, top_res) = add_form(&mut d, resources, "Fm0", "q /Fm1 Do Q\nq /Fm2 Do Q\n");
    link(&mut d, top_res, "Fm1", left);
    link(&mut d, top_res, "Fm2", right);

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!(
            "/Span <</ActualText ({})>> BDC\nq /Fm0 Do Q\nEMC\n",
            escape(mirror)
        )
        .as_bytes(),
    );
    d.set_content(&raw);
    d.finish()
}

#[test]
fn diamant_zaehlt_beide_wege() {
    assert_no_mirror_warning(&diamond("DeltaDelta", false), "Diamant");
    assert_eq!(
        glyphs_under_mirror(&diamond("XYZ", false), "XYZ"),
        Some(10),
        "zwei Wege à 5 Glyphen"
    );
}

/// Derselbe Diamant mit eigenen Glyphen in den beiden Zwischenformularen:
/// `L` steht **vor** seinem `Do`, `R` **danach` — die Reihenfolge steht am
/// Pfad der `Do`-Indizes.
#[test]
fn diamant_mit_eigenen_glyphen_ist_kein_widerspruch() {
    assert_no_mirror_warning(
        &diamond("LDeltaDeltaR", true),
        "Diamant mit eigenen Glyphen",
    );
}

#[test]
fn diamant_luegender_spiegel_faellt() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (deep, _) = add_form(&mut d, resources, "Fm3", &text_at(600, SECRET));
    let (left, left_res) = add_form(&mut d, resources, "Fm1", "q /Fm3 Do Q\n");
    let (right, right_res) = add_form(&mut d, resources, "Fm2", "q 1 0 0 1 0 -20 cm /Fm3 Do Q\n");
    link(&mut d, left_res, "Fm3", deep);
    link(&mut d, right_res, "Fm3", deep);
    let (_, top_res) = add_form(&mut d, resources, "Fm0", "q /Fm1 Do Q\nq /Fm2 Do Q\n");
    link(&mut d, top_res, "Fm1", left);
    link(&mut d, top_res, "Fm2", right);

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
    assert_lie_is_gone(&bytes, "Diamant, lügender Spiegel");
}

// ---------------------------------------------------------------------------
// 4. Glyphen im äußeren **und** im inneren Formular, mit einem Bild dazwischen
// ---------------------------------------------------------------------------

/// `/Fm1 Do  /Im0 Do  /Fm2 Do` unter einem Spiegel, und das äußere Formular
/// trägt selbst Text. Das Bild sitzt zwischen den Formularen und verschiebt
/// die `Do`-Indizes — die Reihenfolge muss trotzdem stimmen.
fn image_between_forms(mirror: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (a, _) = add_form(&mut d, resources, "FmA", &text_at(600, "Alpha"));
    let (b, _) = add_form(&mut d, resources, "FmB", &text_at(580, "Beta"));
    let (_, outer_res) = add_form(
        &mut d,
        resources,
        "Fm0",
        &format!(
            "{}q /FmA Do Q\nq 20 0 0 20 300 700 cm /Im0 Do Q\nq /FmB Do Q\n{}",
            text_at(620, "Anfang"),
            text_at(560, "Ende")
        ),
    );
    link(&mut d, outer_res, "FmA", a);
    link(&mut d, outer_res, "FmB", b);
    add_image(&mut d, outer_res, "Im0");

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!(
            "/Span <</ActualText ({})>> BDC\nq /Fm0 Do Q\nEMC\n",
            escape(mirror)
        )
        .as_bytes(),
    );
    d.set_content(&raw);
    d.finish()
}

#[test]
fn bild_zwischen_den_formularen_stoert_die_reihenfolge_nicht() {
    assert_no_mirror_warning(
        &image_between_forms("AnfangAlphaBetaEnde"),
        "Bild dazwischen",
    );
}

#[test]
fn bild_zwischen_den_formularen_luegender_spiegel_faellt() {
    let bytes = image_between_forms(&format!("Zahlung an {SECRET}"));
    assert_lie_is_gone(&bytes, "Bild dazwischen, lügender Spiegel");
}

// ---------------------------------------------------------------------------
// 5. Zyklen: das Formular zeichnet sich selbst und zwei zeichnen einander
// ---------------------------------------------------------------------------

/// Zwei Formulare, die **einander** zeichnen (`Fm0 → Fm1 → Fm0`). Der
/// Interpreter betritt den Zyklus nicht, die Schließung darf nicht darin
/// hängen bleiben.
#[test]
fn wechselseitiger_zyklus_haelt_an() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (b, b_res) = add_form(
        &mut d,
        resources,
        "Fm1",
        &format!("{}q /Fm0 Do Q\n", text_at(580, "Beta")),
    );
    let (a, a_res) = add_form(
        &mut d,
        resources,
        "Fm0",
        &format!("{}q /Fm1 Do Q\n", text_at(600, "Alpha")),
    );
    link(&mut d, a_res, "Fm1", b);
    link(&mut d, b_res, "Fm0", a);

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"/Span <</ActualText (AlphaBeta)>> BDC\nq /Fm0 Do Q\nEMC\n");
    d.set_content(&raw);
    let bytes = d.finish();
    assert_no_mirror_warning(&bytes, "wechselseitiger Zyklus");
}

/// Derselbe Zyklus mit dem Geheimnis im Spiegel: er muss fallen — und der
/// Lauf muss enden.
#[test]
fn wechselseitiger_zyklus_luegender_spiegel_faellt() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (b, b_res) = add_form(
        &mut d,
        resources,
        "Fm1",
        &format!("{}q /Fm0 Do Q\n", text_at(580, "Beta")),
    );
    let (a, a_res) = add_form(
        &mut d,
        resources,
        "Fm0",
        &format!("{}q /Fm1 Do Q\n", text_at(600, "Alpha")),
    );
    link(&mut d, a_res, "Fm1", b);
    link(&mut d, b_res, "Fm0", a);

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
    assert_lie_is_gone(&bytes, "wechselseitiger Zyklus, lügender Spiegel");
}

/// Ein Formular, das sich selbst zeichnet — **zweimal**, und das Ganze steht
/// zweimal unter dem Spiegel. Genau hier greift die Kette der Vorfahren, und
/// nicht mehr eine Menge über den Datensatz.
#[test]
fn selbstzyklus_zweimal_platziert_haelt_an() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (self_form, self_res) = add_form(
        &mut d,
        resources,
        "Fm0",
        &format!("{}q /Fm0 Do Q\nq /Fm0 Do Q\n", text_at(600, "Alpha")),
    );
    link(&mut d, self_res, "Fm0", self_form);
    // Ein zweites, unbeteiligtes Formular-im-Formular für `close_forms`.
    let (_, nine_res) = add_form(&mut d, resources, "Fm9", "q /Fm8 Do Q\n");
    add_form(&mut d, nine_res, "Fm8", &text_at(300, "Beiwerk"));

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        b"/Span <</ActualText (AlphaAlpha)>> BDC\nq /Fm0 Do Q\nq /Fm0 Do Q\nEMC\nq /Fm9 Do Q\n",
    );
    d.set_content(&raw);
    let bytes = d.finish();
    assert_no_mirror_warning(&bytes, "Selbstzyklus, zweimal platziert");
}

// ---------------------------------------------------------------------------
// 6. Gegenrichtung: ehrliches Material mit allem zusammen
// ---------------------------------------------------------------------------

/// Eine getaggte Seite, die alles zusammenbringt: ein Baustein zweimal, ein
/// Diamant, ein Bild mit `/Alt`, ein Abschnitt ohne Formular. Kein Wort davon
/// darf eine Warnung geben, und die Pipeline ohne Schwärzung auch nicht.
#[test]
fn ehrliche_seite_mit_allem_gibt_keine_warnung() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (deep, _) = add_form(&mut d, resources, "Fm3", &text_at(600, "Musterbank AG"));
    let (left, left_res) = add_form(&mut d, resources, "Fm1", "q /Fm3 Do Q\n");
    let (right, right_res) = add_form(&mut d, resources, "Fm2", "q 1 0 0 1 0 -12 cm /Fm3 Do Q\n");
    link(&mut d, left_res, "Fm3", deep);
    link(&mut d, right_res, "Fm3", deep);
    let (_, top_res) = add_form(&mut d, resources, "Fm0", "q /Fm1 Do Q\nq /Fm2 Do Q\n");
    link(&mut d, top_res, "Fm1", left);
    link(&mut d, top_res, "Fm2", right);
    let (baustein, _) = add_form(&mut d, resources, "Fm4", &text_at(500, "Kontoauszug"));
    let _ = baustein;
    add_image(&mut d, resources, "Im0");

    let mut raw = Vec::new();
    raw.extend_from_slice(b"/P <</MCID 0>> BDC\n");
    raw.extend_from_slice(text_at(700, "Kontoauszug Januar 2026").as_bytes());
    raw.extend_from_slice(b"EMC\n");
    raw.extend_from_slice(b"/Figure <</Alt (Logo der Musterbank)>> BDC\n");
    raw.extend_from_slice(b"q 20 0 0 20 300 780 cm /Im0 Do Q\nEMC\n");
    raw.extend_from_slice(b"/Span <</ActualText (Musterbank AGMusterbank AG)>> BDC\n");
    raw.extend_from_slice(b"q /Fm0 Do Q\nEMC\n");
    raw.extend_from_slice(b"/Span <</ActualText (KontoauszugKontoauszug)>> BDC\n");
    raw.extend_from_slice(b"q /Fm4 Do Q\nq 1 0 0 1 0 -12 cm /Fm4 Do Q\nEMC\n");
    d.set_content(&raw);
    let bytes = d.finish();

    assert_no_mirror_warning(&bytes, "ehrliche Seite mit allem");
    let (_, report_warnings) = pipeline(&bytes, &[]);
    assert!(
        mirror_warnings(&report_warnings).is_empty(),
        "{report_warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// 7. Die Tiefengrenze in der Schließung
// ---------------------------------------------------------------------------

/// Eine Kette von zwölf Formularen (`F11 → F10 → … → F0`, Text ganz unten);
/// der Spiegel steht über `F11`. Der Interpreter liest nur acht Ebenen tief
/// und sagt das (Deckungslücke). **Zusätzlich** steht `F4` direkt auf der
/// Seite — dadurch sind auch die Kanten `F4 → F3 → … → F0` in
/// `ScanResult::nested_forms` verzeichnet, obwohl der Interpreter sie *unter
/// dem Spiegel* nie betreten hat.
///
/// Die Schließung darf deshalb nicht bis `F0` durchlaufen: sie schriebe dem
/// Spiegel Glyphen zu, die unter ihm gar nicht gezeichnet wurden. Genau dafür
/// steht `MAX_FORM_DEPTH` in `ScanResult::close_forms`.
///
/// Nimmt man diese Grenze heraus (`if way.len() >= MAX_FORM_DEPTH` → `if
/// false`), gilt der Spiegel „Tief“ plötzlich als deckungsgleich und die
/// Warnung fällt weg — die Datei sähe geprüfter aus, als sie ist.
#[test]
fn schliessung_haelt_an_der_tiefengrenze_des_interpreters() {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (mut prev, _) = add_form(&mut d, resources, "F0", &text_at(600, "Tief"));
    let mut level4 = None;
    for level in 1..12 {
        let (form, form_res) = add_form(
            &mut d,
            resources,
            &format!("F{level}"),
            &format!("q /F{} Do Q\n", level - 1),
        );
        link(&mut d, form_res, &format!("F{}", level - 1), prev);
        if level == 4 {
            level4 = Some(form);
        }
        prev = form;
    }
    let _ = level4;

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    // `F4` auch direkt auf der Seite: nur so kennt `nested_forms` die Kanten
    // unterhalb von `F4` überhaupt.
    raw.extend_from_slice(b"q 1 0 0 1 0 -100 cm /F4 Do Q\n");
    raw.extend_from_slice(b"/Span <</ActualText (Tief)>> BDC\nq /F11 Do Q\nEMC\n");
    d.set_content(&raw);
    let bytes = d.finish();

    let (_, warnings) = analyse(&bytes);
    assert!(
        warnings.iter().any(|w| w.contains("tiefer als 8 Ebenen")),
        "der Interpreter muss die Tiefe melden: {warnings:?}"
    );
    let mirror = mirror_warnings(&warnings);
    assert_eq!(
        mirror.len(),
        1,
        "der Spiegel über der Tiefengrenze muss als unbelegt gemeldet werden: {warnings:?}"
    );
    assert!(
        mirror[0].contains("0 in den Glyphen"),
        "unter dem Spiegel wurde nichts gelesen: {}",
        mirror[0]
    );
}

// ---------------------------------------------------------------------------
// 8. Die Decke an ihrer Grenze
// ---------------------------------------------------------------------------

/// Eine Seite mit `outer` Platzierungen von `Fm0` unter **einem** Spiegel;
/// `Fm0` zeichnet genau ein Blatt `Fm1`. Die Schließung klappt daraus genau
/// `outer` Platzierungen auf — `left` in `close_forms` geht also bei
/// `outer == MAX_MIRROR_FORM_PLACEMENTS` auf null, **ohne dass eine einzige
/// Platzierung verloren geht** (`Fm1` hat keine Kinder).
fn exactly_n_expansions(outer: usize) -> Vec<u8> {
    exactly_n_expansions_with_mirror(outer, "Alpha")
}

/// Dasselbe mit frei gewähltem Spiegel — mit `"A".repeat(outer)` ist der
/// Spiegel deckungsgleich, und die Decke ist dann die **einzige** Warnung.
fn exactly_n_expansions_with_mirror(outer: usize, mirror: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let resources = d.resources_id;
    let (inner, _) = add_form(&mut d, resources, "Fm1", &text_at(600, "A"));
    let (_, outer_res) = add_form(&mut d, resources, "Fm0", "/Fm1 Do\n");
    link(&mut d, outer_res, "Fm1", inner);

    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(format!("/Span <</ActualText ({})>> BDC\n", escape(mirror)).as_bytes());
    raw.extend_from_slice("/Fm0 Do\n".repeat(outer).as_bytes());
    raw.extend_from_slice(b"EMC\n");
    d.set_content(&raw);
    d.finish()
}

fn ceiling_warning(bytes: &[u8]) -> Vec<String> {
    let (_, warnings) = analyse(bytes);
    warnings
        .into_iter()
        .filter(|w| w.contains("Zuordnungen zwischen einem Spiegel"))
        .collect()
}

/// **Befund Q3-1b (falscher Alarm), in Fix-Runde 6 behoben.** Genau
/// `MAX_MIRROR_FORM_PLACEMENTS`
/// Aufklappungen: die Schließung hat **alle** Platzierungen zugeordnet, meldet
/// aber „ab dort wurden die Glyphen den Spiegeln nicht mehr zugeordnet … der
/// Vergleich ist unvollständig“. Das ist eine Deckungslücke und damit
/// Rückgabewert 3 an einer Datei, die vollständig verglichen wurde.
///
/// Ursache: in `close_forms` steht `if left == 0 { truncated = true; continue; }`
/// **vor** dem Nachschlagen in `nested_forms`. Ein Blatt, das nach der letzten
/// gezählten Aufklappung vom Stapel kommt, hat gar keine Kinder — und setzt die
/// Flagge trotzdem. Dieselbe Klasse wie der in dieser Runde behobene
/// ASCII85-Fall („lehnte einen Strom ab, der exakt ins Restbudget passte“).
///
/// Gefragt wird jetzt erst dort, wo eine Kante wirklich aufzuklappen ist.
#[test]
fn befund_decke_warnt_bei_genau_aufgehender_zahl() {
    let hits = ceiling_warning(&exactly_n_expansions(100_000));
    assert!(
        hits.is_empty(),
        "nichts ging verloren, trotzdem: {:?}",
        hits
    );
}

/// Die Gegenprobe, damit der Befund an der Grenze hängt und nicht am Aufbau:
/// eine Aufklappung weniger, und es bleibt still.
#[test]
fn eine_platzierung_unter_der_decke_bleibt_still() {
    let hits = ceiling_warning(&exactly_n_expansions(99_999));
    assert!(hits.is_empty(), "{hits:?}");
}

/// Material für den Lauf über die Kommandozeile: genau 100 000 Aufklappungen
/// unter einem **deckungsgleichen** Spiegel. Dann ist die Decke die einzige
/// Warnung, und der Rückgabewert 3 hängt allein an ihr.
/// Schreibt nur, wenn `Q3_OUT` gesetzt ist.
#[test]
#[ignore = "Material"]
fn schreibt_material_fuer_die_decke() {
    let Ok(dir) = std::env::var("Q3_OUT") else {
        return;
    };
    for outer in [99_999usize, 100_000] {
        let bytes = exactly_n_expansions_with_mirror(outer, &"A".repeat(outer));
        std::fs::write(format!("{dir}/decke_{outer}.pdf"), &bytes).expect("schreibbar");
        println!("{dir}/decke_{outer}.pdf: {} B", bytes.len());
    }
}
