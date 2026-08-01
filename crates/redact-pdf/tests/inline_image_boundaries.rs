//! #72 — wo ein Inline-Bild **endet**, entscheidet über den Rest der Seite.
//!
//! Ein Inline-Bild trägt seine Nutzdaten mitten im Content-Stream. Wird die
//! Grenze falsch bestimmt, geht es in beide Richtungen schief:
//!
//! * **zu früh** (ein `EI` steht zufällig in den Binärdaten): das Bild ist
//!   abgeschnitten, und der Rest des Stroms ist Binärmüll, den lopdf nicht
//!   mehr zerlegen kann. Beim Neuschreiben wird nur zurückgeschrieben, was
//!   übrig blieb — der Text dahinter ist **ersatzlos weg**. Kein Leck, aber
//!   Datenverlust ohne Ansage.
//! * **zu spät** (ein `/L`, das der Erzeuger zu groß angegeben hat): der
//!   nachfolgende Text landet in der Bild-Nutzlast, wird nie durchsucht und
//!   wandert wortwörtlich in die Ausgabe zurück — ein Leck.
//!
//! Gemessen wird deshalb **beides**: dass das Geheimnis weg ist (Orakel
//! [`leaks`]) *und* dass der Text, der bleiben sollte, nachweislich noch in
//! der Datei steht (dasselbe Orakel, andere Richtung).

mod common;

use common::SECRET;
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{
    leaks, load_from_bytes, page_ops, save_to_bytes, strip_metadata, DrawOp, PdfRedactor,
    RasterImage, RedactionReport,
};

/// Die Zeile, die der Nutzer behalten will. Sie steht **hinter** dem
/// Inline-Bild und ist von der Schwärzung nicht berührt.
const KEEP: &str = "Kontoinhaber: Max Mustermann";

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

/// 8×8 Graustufen, jeder Wert einmalig (0, 4, 8 … 252) — so fällt eine
/// verrutschte Zuordnung sofort auf. `ei_at` setzt an der genannten Stelle
/// die vier Bytes ` EI `, wie sie in echten Binärdaten zufällig vorkommen.
fn gray_payload(ei_at: Option<usize>) -> Vec<u8> {
    let mut data: Vec<u8> = (0u8..64).map(|v| v.wrapping_mul(4)).collect();
    if let Some(at) = ei_at {
        data[at..at + 4].copy_from_slice(b" EI ");
    }
    data
}

/// Alles, was hinter den Bilddaten im Strom steht: der Abschluss des Bildes
/// und die beiden Textzeilen (Grundlinien y = 700 und y = 685).
fn tail_behind_the_image() -> Vec<u8> {
    let mut tail = Vec::from(&b"\nEI\nQ\n"[..]);
    tail.extend_from_slice(&common::text_ops(&[KEEP, &format!("IBAN: {SECRET}")]));
    tail
}

/// Seite mit Inline-Bild bei (300,780)–(320,800) und zwei Textzeilen dahinter.
fn page_with_inline_image(extra_dict: &str, payload: &[u8]) -> Vec<u8> {
    let mut d = common::page(&[]);
    let mut raw = Vec::from(&b"q 20 0 0 20 300 780 cm\n"[..]);
    raw.extend_from_slice(format!("BI /W 8 /H 8 /CS /G /BPC 8{extra_dict} ID ").as_bytes());
    raw.extend_from_slice(payload);
    raw.extend_from_slice(&tail_behind_the_image());
    d.set_content(&raw);
    d.finish()
}

/// Nur die IBAN-Zeile (Grundlinie y = 685), nicht die Zeile darüber.
fn over_the_iban_line() -> Redaction {
    Redaction::new(
        Region::new(
            0,
            Rect::new(40.0, 680.0, 560.0, 696.0),
            Some(SECRET.to_string()),
            Source::Manual {
                reason: "#72".into(),
            },
        ),
        Action::Blackout,
    )
}

fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> (RedactionReport, Vec<u8>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    (report, save_to_bytes(&doc).expect("Speichern"))
}

fn inline_image_of(bytes: &[u8]) -> RasterImage {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    let ops = page_ops(&doc, 0).expect("Seitenoperationen");
    ops.ops
        .iter()
        .find_map(|op| match op {
            DrawOp::Image { image, .. } => Some(ops.images[*image].clone()),
            _ => None,
        })
        .expect("Inline-Bild auf der Seite")
}

#[track_caller]
fn assert_gone(bytes: &[u8], needle: &str, what: &str) {
    let hits = leaks(bytes, needle);
    assert!(
        hits.is_empty(),
        "{what}: „{needle}“ steht noch {} mal in der Ausgabe:\n{}",
        hits.len(),
        hits.join("\n")
    );
}

#[track_caller]
fn assert_still_there(bytes: &[u8], needle: &str, what: &str) {
    assert!(
        !leaks(bytes, needle).is_empty(),
        "{what}: „{needle}“ ist aus der Ausgabe verschwunden — Datenverlust \
         ohne Ansage. Der Nutzer bekommt eine Datei, die er für vollständig hält."
    );
}

// ---------------------------------------------------------------------------
// #72a — ` EI ` in den Nutzdaten
// ---------------------------------------------------------------------------

/// Die Schwärzung liegt **nur** über der IBAN-Zeile; das Bild wird gar nicht
/// angefasst. Trotzdem entschied bislang das erste `EI` in den Binärdaten
/// über das Ende des Bildes — und damit über den ganzen Rest der Seite.
#[test]
fn an_ei_inside_the_image_data_does_not_swallow_the_rest_of_the_page() {
    let pdf = page_with_inline_image("", &gray_payload(Some(8)));

    // Vorbedingung: vor der Schwärzung steht beides in der Datei.
    assert_still_there(&pdf, SECRET, "Vorbedingung");
    assert_still_there(&pdf, KEEP, "Vorbedingung");

    let (report, out) = pipeline(&pdf, &[over_the_iban_line()]);

    assert!(
        report.removed_glyphs > 0,
        "kein einziges Zeichen entfernt — die Zeile wurde gar nicht erst \
         gefunden. Warnungen: {:?}",
        report.warnings
    );
    assert_gone(&out, SECRET, "IBAN hinter dem Inline-Bild");
    assert_still_there(&out, KEEP, "Zeile über der Schwärzung");
}

/// Und das Bild selbst bleibt vollständig: acht mal acht Pixel, nicht acht.
#[test]
fn an_ei_inside_the_image_data_does_not_truncate_the_image() {
    let payload = gray_payload(Some(8));
    let pdf = page_with_inline_image("", &payload);

    let before = inline_image_of(&pdf);
    assert!(!before.placeholder, "Vorbedingung: Bild ist dekodierbar");
    assert_eq!((before.width, before.height), (8, 8));

    let (_, out) = pipeline(&pdf, &[over_the_iban_line()]);
    let after = inline_image_of(&out);
    assert_eq!(
        (after.width, after.height),
        (8, 8),
        "das Bild hat seine Größe verloren"
    );
    assert!(!after.placeholder, "das Bild ist zum Platzhalter geworden");
    // Letztes Pixel: Graustufe 252 — es steht hinter dem falschen `EI`.
    let last = ((after.height - 1) * after.width + after.width - 1) as usize * 4;
    assert_eq!(
        &after.rgba[last..last + 4],
        &[252, 252, 252, 255],
        "die Bilddaten hinter dem falschen `EI` fehlen"
    );
}

// ---------------------------------------------------------------------------
// #72b — ein `/L`, dem man nicht glauben darf
// ---------------------------------------------------------------------------

/// Ein `/L`, das bis zum Stromende reicht, bei 64 Byte echten Nutzdaten: der
/// Text dahinter landete in der Bild-Nutzlast, wurde nie durchsucht und
/// wanderte wortwörtlich zurück in die Ausgabe.
#[test]
fn a_too_large_declared_length_does_not_swallow_the_text_behind_it() {
    let payload = gray_payload(None);
    // Genau so viel, wie im Strom hinter dem `ID` überhaupt noch steht.
    let declared = payload.len() + tail_behind_the_image().len();
    let pdf = page_with_inline_image(&format!(" /L {declared}"), &payload);

    let (report, out) = pipeline(&pdf, &[over_the_iban_line()]);

    assert!(
        report.removed_glyphs > 0,
        "kein einziges Zeichen entfernt — der Text steckte in der Bild-Nutzlast. \
         Warnungen: {:?}",
        report.warnings
    );
    assert_gone(&out, SECRET, "IBAN in der aufgeblähten Bild-Nutzlast");
    assert_still_there(&out, KEEP, "Zeile über der Schwärzung");
}

/// Gegenprobe: ein `/L`, das **stimmt**, wird weiter geglaubt — und das Bild
/// bleibt heil, obwohl in seinen Daten ein ` EI ` steht.
#[test]
fn a_correct_declared_length_still_wins_over_an_ei_in_the_data() {
    let pdf = page_with_inline_image(" /L 64", &gray_payload(Some(8)));

    let (report, out) = pipeline(&pdf, &[over_the_iban_line()]);

    assert!(report.removed_glyphs > 0, "{:?}", report.warnings);
    assert_gone(&out, SECRET, "IBAN bei korrektem /L");
    assert_still_there(&out, KEEP, "Zeile über der Schwärzung");
    assert_eq!(inline_image_of(&out).width, 8);
}

// ---------------------------------------------------------------------------
// Der Rest eines Stroms darf nicht stillschweigend verschwinden
// ---------------------------------------------------------------------------

/// Seite mit Inline-Bild und einem angehängten Reststück.
fn page_with_trailing(rest: &[u8]) -> Vec<u8> {
    let mut d = common::page(&[]);
    let mut raw = Vec::from(&b"q 20 0 0 20 300 780 cm\n"[..]);
    raw.extend_from_slice(b"BI /W 8 /H 8 /CS /G /BPC 8 ID ");
    raw.extend_from_slice(&gray_payload(None));
    raw.extend_from_slice(b"\nEI\nQ\n");
    raw.extend_from_slice(rest);
    d.set_content(&raw);
    d.finish()
}

/// Hinter dem Inline-Bild steht etwas, das sich nicht mehr zerlegen lässt.
///
/// `lopdf` gibt dafür kein `Err` zurück, sondern ein arglos aussehendes `Ok`
/// mit den Operationen **vor** der Bruchstelle — der Rest ist einfach weg.
/// Früher fiel das lautlos unter den Tisch, und mit ihm jeder Text, der darin
/// stand. Jetzt wird die Datei abgelehnt.
#[test]
fn an_undecodable_remainder_is_not_dropped_in_silence() {
    // Ein Nullbyte gilt in PDF als Leerraum, in lopdfs Content-Parser nicht:
    // ab dort bricht die Zerlegung ab, und `(IBAN …) Tj` fehlt im Ergebnis.
    let mut rest = Vec::from(&b"BT /F1 10 Tf 72 685 Td\x00 "[..]);
    rest.extend_from_slice(format!("(IBAN: {SECRET}) Tj ET\n").as_bytes());
    let pdf = page_with_trailing(&rest);

    let mut doc = load_from_bytes(&pdf).expect("PDF ladbar");
    let error = PdfRedactor::new()
        .apply_with_report(&mut doc, &[over_the_iban_line()])
        .expect_err("ein abgeschnittener Rest wurde stillschweigend verworfen");
    assert!(
        error.to_string().contains("nicht in Operationen zerlegen"),
        "unerwartete Begründung: {error}"
    );

    // Gegenprobe: ohne das Nullbyte läuft genau dieselbe Seite durch.
    let mut clean = Vec::from(&b"BT /F1 10 Tf 72 685 Td "[..]);
    clean.extend_from_slice(format!("(IBAN: {SECRET}) Tj ET\n").as_bytes());
    let pdf = page_with_trailing(&clean);
    let (report, out) = pipeline(&pdf, &[over_the_iban_line()]);
    assert!(report.removed_glyphs > 0, "{:?}", report.warnings);
    assert_gone(&out, SECRET, "IBAN hinter dem Inline-Bild");
}
