//! Regressionstests aus der zweiten Prüfrunde für die Oberfläche — die
//! Sicherheitslogik hinter den Knöpfen, nicht die Optik:
//!
//! * ein auf dem **Bild** gezogenes Rechteck trifft den Text darunter auch
//!   auf gedrehten Seiten mit verschobener MediaBox, CropBox und UserUnit;
//! * der Export auf das **Original** wird abgelehnt — auch über Umweg,
//!   Symlink, Audit-Ziel und Hardlink;
//! * eine **Deckungslücke** im Export steht in der Statuszeile vorn und
//!   gekennzeichnet, nicht hinter einem Hinweis, der das Gegenteil sagt.
//!
//! ```text
//! cargo test -p redact-gui --test r2_oberflaeche
//! ```

use std::path::PathBuf;

use lopdf::{dictionary, Document, Object, Stream};
use redact_core::{Rect, Region, Source};
use redact_gui::state::{AnnotatedRegion, AppState};
use redact_gui::Config;
use redact_pdf::testing::{build_pdf, TextItem};

const IBAN: &str = "DE89370400440532013000";

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("r2-oberflaeche-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn manual(page: usize, rect: Rect) -> AnnotatedRegion {
    AnnotatedRegion::new(Region::new(
        page,
        rect,
        None,
        Source::Manual {
            reason: "r2-oberflaeche".into(),
        },
    ))
}

fn save(doc: &mut Document) -> Vec<u8> {
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

// ===========================================================================
// Rechteck auf gedrehter, verschobener Seite
// ===========================================================================

/// Eine Seite, deren Koordinaten an keiner Stelle „einfach“ sind: MediaBox
/// nicht bei null, CropBox darin verschoben, `UserUnit` 2 und die gegebene
/// Drehung. Genau dort fallen Bild und Modell auseinander, wenn eine der
/// Umrechnungen eine Annahme zu viel macht.
fn rotated_offset_pdf(rotate: i64) -> Vec<u8> {
    // Text liegt im User-Space bei (150, 500) — MediaBox beginnt bei (50, 100).
    let bytes = build_pdf(&[vec![
        TextItem::new(150.0, 500.0, 14.0, IBAN),
        TextItem::new(150.0, 300.0, 14.0, "Fuelltext bleibt"),
    ]]);
    let mut doc = Document::load_mem(&bytes).unwrap();
    let page_id = *doc.get_pages().values().next().unwrap();
    let page = doc.get_dictionary_mut(page_id).unwrap();
    page.set(
        "MediaBox",
        vec![50.into(), 100.into(), 650.into(), 900.into()],
    );
    page.set(
        "CropBox",
        vec![120.into(), 250.into(), 600.into(), 800.into()],
    );
    page.set("UserUnit", Object::Real(2.0));
    page.set("Rotate", rotate);
    save(&mut doc)
}

/// Alle dunklen Pixel des gerenderten Bildes als (x, y).
fn dark_pixels(page: &redact_render::RenderedPage) -> Vec<(u32, u32)> {
    let mut pts = Vec::new();
    for y in 0..page.height {
        for x in 0..page.width {
            let i = ((y * page.width + x) * 4) as usize;
            let (r, g, b) = (page.rgba[i], page.rgba[i + 1], page.rgba[i + 2]);
            if (r as u32 + g as u32 + b as u32) < 300 {
                pts.push((x, y));
            }
        }
    }
    pts
}

/// Zieht ein Rechteck dort auf, wo die IBAN **im Bild** steht — nicht dort,
/// wo das Modell sie vermutet —, rechnet es wie die Oberfläche in
/// PDF-Koordinaten um und exportiert. Die IBAN muss weg sein, der Fülltext
/// darunter muss bleiben.
fn draw_over_iban_and_export(rotate: i64) {
    let dir = tmp(&format!("rot{rotate}"));
    let bytes = rotated_offset_pdf(rotate);
    let input = dir.join("in.pdf");
    std::fs::write(&input, &bytes).unwrap();
    let mut state = AppState::with_config(Config {
        no_patterns: true,
        ..Config::default()
    });
    state.load_bytes(&bytes, Some(input)).unwrap();

    let doc = state.document.clone().unwrap();
    let mut renderer = redact_render::PageRenderer::new();
    let rendered = renderer.render(&doc, 0, &redact_render::RenderOptions::default());
    let view = state.page_view(0);
    assert_eq!(
        view.display_box(),
        rendered.page_box,
        "Bild und Ansicht decken sich"
    );
    let zoom = rendered.width as f32 / view.display_box().width() as f32;

    // Zwei Text-Cluster: IBAN (länger) und Fülltext. Wir bestimmen, wo im
    // Bild der IBAN-Text liegt, indem wir die Pixelcluster nach der
    // Anzeigeposition der beiden Zeilen trennen: die IBAN-Zeile liegt laut
    // Modell näher an user y=500.
    let iban_screen = redact_gui::pdf_to_screen(
        &Rect::new(150.0, 498.0, 151.0, 502.0),
        &view,
        zoom,
        egui::Pos2::ZERO,
    );
    let pts = dark_pixels(&rendered);
    assert!(!pts.is_empty(), "Text muss gerendert sein");
    // Pixel, die zur IBAN-Zeile gehören: entlang der Grundlinienrichtung
    // beliebig, quer dazu nahe an der Modellposition. Die Querrichtung ist
    // bei 0/180 die y-Achse des Bildes, bei 90/270 die x-Achse.
    let quarter = view.is_quarter_turned();
    let center = if quarter {
        iban_screen.center().x
    } else {
        iban_screen.center().y
    };
    let near: Vec<(u32, u32)> = pts
        .into_iter()
        .filter(|(x, y)| {
            let c = if quarter { *x as f32 } else { *y as f32 };
            (c - center).abs() < 40.0
        })
        .collect();
    assert!(!near.is_empty(), "IBAN-Pixel nahe der Modellposition");
    let x0 = near.iter().map(|p| p.0).min().unwrap() as f32 - 3.0;
    let x1 = near.iter().map(|p| p.0).max().unwrap() as f32 + 3.0;
    let y0 = near.iter().map(|p| p.1).min().unwrap() as f32 - 3.0;
    let y1 = near.iter().map(|p| p.1).max().unwrap() as f32 + 3.0;

    // Genau das tut die Oberfläche beim Aufziehen: Bildschirm -> PDF.
    let rect = redact_gui::screen_to_pdf(
        egui::pos2(x0, y0),
        egui::pos2(x1, y1),
        &view,
        zoom,
        egui::Pos2::ZERO,
    );
    state.add_manual_region(0, rect, "gezogen").expect("Region");

    let out = dir.join("out.pdf");
    state
        .plan_export(&out, Some(&dir.join("audit.json")))
        .unwrap()
        .run()
        .unwrap();
    let written = std::fs::read(&out).unwrap();
    assert!(
        redact_pdf::leaks(&written, IBAN).is_empty(),
        "Rotate {rotate}: das über das Bild der IBAN gezogene Rechteck ({rect:?}) hat sie nicht getroffen"
    );
    assert!(
        !redact_pdf::leaks(&written, "Fuelltext bleibt").is_empty(),
        "Rotate {rotate}: das Rechteck darf nicht die ganze Seite treffen"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Das Rechteck trifft die IBAN bei jeder Drehung, auch bei den nicht
/// normalisierten Werten -90 und 450.
#[test]
fn rechteck_trifft_auf_gedrehter_verschobener_seite() {
    for rotate in [0, 90, 180, 270, -90, 450] {
        draw_over_iban_and_export(rotate);
    }
}

// ===========================================================================
// Export auf das Original
// ===========================================================================

/// Das Original wird nie überschrieben: der Export dorthin wird abgelehnt —
/// direkt, über `sub/../`, über einen Symlink —, die Review-Datei ebenso,
/// und ein Audit-Ziel oder Hardlink auf das Original lässt es unverändert.
#[test]
fn export_auf_das_original_wird_abgelehnt() {
    let dir = tmp("orig");
    let bytes = build_pdf(&[vec![TextItem::new(72.0, 700.0, 12.0, IBAN)]]);
    let input = dir.join("in.pdf");
    std::fs::write(&input, &bytes).unwrap();
    let mut state = AppState::with_config(Config {
        no_patterns: true,
        ..Config::default()
    });
    state.load_bytes(&bytes, Some(input.clone())).unwrap();
    state
        .regions
        .push(manual(0, Rect::new(60.0, 690.0, 400.0, 715.0)));

    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut targets = vec![input.clone(), sub.join("../in.pdf")];
    #[cfg(unix)]
    {
        let link = dir.join("link.pdf");
        std::os::unix::fs::symlink(&input, &link).unwrap();
        targets.push(link);
    }
    for t in &targets {
        assert!(
            state.plan_export(t, Some(&dir.join("a.json"))).is_err(),
            "{t:?} muss abgelehnt werden"
        );
    }
    // „Review speichern unter“ auf das Original.
    let r = state.save_review_file(&input);
    assert!(r.is_err(), "Review auf das Original muss abgelehnt werden");
    assert_eq!(std::fs::read(&input).unwrap(), bytes);
    // Audit-Log, das auf das Original zeigt (--audit-log in.pdf): darf das
    // Original nicht ersetzen.
    let r = state
        .plan_export(&dir.join("o.pdf"), Some(&input))
        .unwrap()
        .run();
    assert_eq!(
        std::fs::read(&input).unwrap(),
        bytes,
        "Original verändert über Audit-Ziel ({r:?})"
    );
    // Hardlink: kein Pfadvergleich greift, aber das Original darf nicht
    // überschrieben werden.
    #[cfg(unix)]
    {
        let hard = dir.join("hard.pdf");
        std::fs::hard_link(&input, &hard).unwrap();
        let r = state
            .plan_export(&hard, Some(&dir.join("b.json")))
            .and_then(|p| p.run());
        assert_eq!(
            std::fs::read(&input).unwrap(),
            bytes,
            "Original über Hardlink verändert ({r:?})"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// Deckungslücke in der Statuszeile
// ===========================================================================

/// Ein „Scan“: ein JPX-Bild über die ganze Seite. Mit
/// `--allow-undecodable-images` wird es geschrieben, lässt sich aber nicht
/// dekodieren — die Pixel unter einem Rechteck bleiben in der Datei.
fn jpx_scan_pdf() -> Vec<u8> {
    let bytes = build_pdf(&[vec![TextItem::new(72.0, 100.0, 10.0, "Seite 1")]]);
    let mut doc = Document::load_mem(&bytes).unwrap();
    // Ein „Scan“: ein JPX-Bild über die ganze Seite, mit der IBAN im Bild.
    let img = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 10,
            "Height" => 10,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
            "Filter" => "JPXDecode",
        },
        vec![0u8; 64],
    );
    let img_id = doc.add_object(img);
    let page_id = *doc.get_pages().values().next().unwrap();
    // Der Zeilenumbruch vorn trennt den zweiten Teilstrom sauber vom ersten —
    // sonst meldet die Analyse zu Recht eine eigene Lücke an der Naht.
    let content = b"\nq 595 0 0 842 0 0 cm /Im1 Do Q\n".to_vec();
    let cid = doc.add_object(Stream::new(dictionary! {}, content));
    let font = doc
        .get_dictionary(page_id)
        .unwrap()
        .get(b"Resources")
        .unwrap()
        .clone();
    let res_id = font.as_reference().unwrap();
    let res = doc.get_dictionary_mut(res_id).unwrap();
    res.set("XObject", dictionary! { "Im1" => img_id });
    let page = doc.get_dictionary_mut(page_id).unwrap();
    let old = page.get(b"Contents").unwrap().clone();
    page.set("Contents", vec![old, Object::Reference(cid)]);
    save(&mut doc)
}

/// Der Fall aus der Prüfung: JPX-Scan, Rechteck über der IBAN im Bild,
/// Export. Der Schwärzer schreibt „… enthalten Rasterbilder. Geschwärzte
/// Bereiche werden im Bild selbst überschrieben …“ **vor** die Lücke „… lässt
/// sich nicht dekodieren … die Pixel blieben in der Datei“. Früher zeigte die
/// Statuszeile die erste Warnung — also das Gegenteil dessen, was geschehen
/// war —, und die Lücke stand nur im Tooltip.
#[test]
fn luecke_im_export_steht_vorn() {
    let dir = tmp("gap");
    let bytes = jpx_scan_pdf();
    let input = dir.join("scan.pdf");
    std::fs::write(&input, &bytes).unwrap();
    // `redact-rs --gui --allow-undecodable-images scan.pdf`
    let mut state = AppState::with_config(Config {
        allow_undecodable_images: true,
        ..Config::default()
    });
    state.load_bytes(&bytes, Some(input)).unwrap();
    // Rechteck über die IBAN im Scan.
    state
        .regions
        .push(manual(0, Rect::new(100.0, 600.0, 400.0, 640.0)));
    let out = dir.join("scan_geschwaerzt.pdf");
    let audit = state.audit_target(&out);
    let outcome = state
        .plan_export(&out, Some(&audit))
        .unwrap()
        .run()
        .unwrap();
    let gaps = outcome.coverage_gaps();
    assert!(!gaps.is_empty(), "die CLI endet hier mit 3 / NICHT GEPRÜFT");
    assert!(
        !redact_pipeline::is_coverage_gap(&outcome.warnings[0])
            && outcome
                .warnings
                .iter()
                .any(|w| w.contains("nicht dekodieren")),
        "Voraussetzung: der Schwärzer nennt eine Nicht-Lücke zuerst — sonst \
         prüft dieser Test nichts mehr: {:#?}",
        outcome.warnings
    );

    // Genau so baut `RedactApp::finish_export` die Statuszeile.
    let warning = redact_gui::app::export_status_warning(&outcome);
    let status = redact_gui::app::export_status(
        outcome.drawn_rects,
        outcome.removed_glyphs,
        &out,
        &audit,
        0,
        warning.as_deref(),
    );
    assert!(
        status.contains(&format!("⚠ NICHT GEPRÜFT: {}", gaps[0])),
        "die Statuszeile muss die Lücke gekennzeichnet zeigen: {status}"
    );
    assert!(
        status.contains("nicht vollständig geprüft"),
        "derselbe Satz wie in der CLI fehlt: {status}"
    );
    assert!(
        !status.contains(outcome.warnings[0].as_str()),
        "die Nicht-Lücke steht in der Statuszeile: {status}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Ohne Lücke bleibt es bei der ersten Warnung, ohne Kennzeichen.
#[test]
fn ohne_luecke_kein_kennzeichen() {
    assert_eq!(
        redact_gui::app::mark_coverage_gap(
            "3 Fläche(n) haben ein Deck-Rechteck gezeichnet, aber nichts entfernt"
        ),
        "3 Fläche(n) haben ein Deck-Rechteck gezeichnet, aber nichts entfernt"
    );
    assert!(
        redact_gui::app::mark_coverage_gap("Font ohne /ToUnicode auf Seite 1")
            .starts_with(redact_gui::app::COVERAGE_GAP_MARK)
    );
}
