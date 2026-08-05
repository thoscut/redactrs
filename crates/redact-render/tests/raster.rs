//! Integrationstests für die Seitenrasterisierung.
//!
//! Der Schwerpunkt liegt auf zwei Fragen:
//!
//! 1. **Ist überhaupt etwas zu sehen?** Genau daran ist die alte Vorschau
//!    gescheitert: sie zeichnete nur Textzeilen, weshalb grafiklastige und
//!    gescannte Seiten als leeres weißes Blatt erschienen.
//! 2. **Bleibt der Renderer unter allen Umständen gutmütig?** `render` darf
//!    weder `Err` liefern noch panicken — im schlimmsten Fall ein weißes Blatt
//!    mit `degraded = true`.
//!
//! Jedes Prüfstück entsteht hier im Speicher. Diese Datei liest **keine**
//! Datei von der Maschine, auf der sie läuft — drei taten es einmal, und wenn
//! sie fehlten (auf jedem CI-Runner der Normalfall), kehrte der betreffende
//! Test mit „übersprungen“ grün zurück, ohne etwas geprüft zu haben. Siehe
//! [`a_page_without_a_single_glyph_still_shows_its_drawing`].
//!
//! Zusätzlich schreibt [`writes_reference_pngs`] ein paar gerenderte Seiten als
//! PNG heraus, damit ein Mensch sie ohne Bildschirm begutachten kann.

use std::path::Path;

use lopdf::{dictionary, Dictionary, Document, Object, Stream};
use redact_render::{PageRenderer, RenderOptions, RenderedPage};

// ---------------------------------------------------------------------------
// Hilfsmittel: Messen
// ---------------------------------------------------------------------------

/// Anteil der Pixel, die nicht reinweiß sind (0.0 … 1.0).
fn non_white_ratio(page: &RenderedPage) -> f64 {
    let total = page.rgba.len() / 4;
    assert_eq!(
        total,
        page.width as usize * page.height as usize,
        "Puffergröße passt nicht zu {}x{}",
        page.width,
        page.height
    );
    if total == 0 {
        return 0.0;
    }
    let hits = page
        .rgba
        .chunks_exact(4)
        .filter(|p| p[0] != 255 || p[1] != 255 || p[2] != 255)
        .count();
    hits as f64 / total as f64
}

/// Farbe eines Pixels; panickt bei Koordinaten außerhalb des Bildes, damit ein
/// verrutschter Test nicht stillschweigend Weiß misst.
fn pixel_at(page: &RenderedPage, x: u32, y: u32) -> [u8; 4] {
    page.pixel(x, y).unwrap_or_else(|| {
        panic!(
            "Pixel {x}/{y} liegt außerhalb von {}x{}",
            page.width, page.height
        )
    })
}

/// Prüft eine Farbe mit Toleranz (Kantenglättung und bilineare Filterung
/// treffen nie exakt).
fn assert_near(actual: [u8; 4], expected: [u8; 3], tolerance: i32, what: &str) {
    let ok = (0..3).all(|i| (i32::from(actual[i]) - i32::from(expected[i])).abs() <= tolerance);
    assert!(
        ok,
        "{what}: erwartet ~{expected:?} (±{tolerance}), gemessen {actual:?}"
    );
}

fn is_white(pixel: [u8; 4]) -> bool {
    pixel[0] == 255 && pixel[1] == 255 && pixel[2] == 255
}

// ---------------------------------------------------------------------------
// Hilfsmittel: PDFs bauen
// ---------------------------------------------------------------------------

/// Hängt Seitenbaum, Katalog und Content-Stream an ein vorbereitetes Dokument.
///
/// Getrennt vom Dokument selbst, damit ein Test vorher noch XObjects & Co.
/// einfügen und in `resources` referenzieren kann.
fn assemble(
    doc: &mut Document,
    content: &str,
    resources: Dictionary,
    media_box: Vec<Object>,
    rotate: Option<i64>,
) {
    let resources_id = doc.add_object(Object::Dictionary(resources));
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let pages_id = doc.new_object_id();
    let mut page = dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => media_box,
    };
    if let Some(rotate) = rotate {
        page.set("Rotate", rotate);
    }
    let page_id = doc.add_object(page);
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
}

fn media(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Object> {
    vec![
        Object::Real(x0 as f32),
        Object::Real(y0 as f32),
        Object::Real(x1 as f32),
        Object::Real(y1 as f32),
    ]
}

/// Einseitiges Dokument, 400 x 400 Punkt, ohne Ressourcen.
fn square_page(content: &str) -> Document {
    let mut doc = Document::with_version("1.5");
    assemble(
        &mut doc,
        content,
        Dictionary::new(),
        media(0.0, 0.0, 400.0, 400.0),
        None,
    );
    doc
}

/// Optionen, unter denen ein Punkt genau einem Pixel entspricht: 400 Punkt
/// breite Seite auf 400 Pixel. Das macht die Pixelkoordinaten der Tests
/// nachrechenbar.
fn square_opts() -> RenderOptions {
    RenderOptions {
        width: 400,
        max_pixels: 4000,
        ..RenderOptions::default()
    }
}

/// Rendert Seite 0 eines 400x400-Dokuments im Maßstab 1:1.
fn render_square(content: &str) -> RenderedPage {
    PageRenderer::new().render(&square_page(content), 0, &square_opts())
}

/// User-Space-Punkt einer 400x400-Seite → Pixelkoordinate (y wird gespiegelt).
fn at(x: f64, y: f64) -> (u32, u32) {
    (x.round() as u32, (400.0 - y).round() as u32)
}

fn render_bytes(pdf: &[u8], page_index: usize, opts: &RenderOptions) -> RenderedPage {
    let doc = Document::load_mem(pdf).expect("Testdokument lesbar");
    PageRenderer::new().render(&doc, page_index, opts)
}

// ---------------------------------------------------------------------------
// 1 — Der Kontoauszug wird sichtbar gesetzt
// ---------------------------------------------------------------------------

#[test]
fn demo_statement_renders_visible_text() {
    let pdf = redact_pdf::testing::demo_statement();
    let page = render_bytes(&pdf, 0, &RenderOptions::default());

    assert!(!page.degraded, "Warnungen: {:?}", page.warnings);
    assert!(page.drawn_ops > 0, "keine einzige Operation gezeichnet");
    let ratio = non_white_ratio(&page);
    println!("demo_statement Seite 0: non-white = {:.4} %", ratio * 100.0);
    assert!(
        ratio > 0.005,
        "Text praktisch unsichtbar: nur {:.4} % nicht-weiß",
        ratio * 100.0
    );
}

// ---------------------------------------------------------------------------
// 2 — Gefülltes Rechteck
// ---------------------------------------------------------------------------

#[test]
fn filled_rectangle_covers_only_its_area() {
    let page = render_square("1 0 0 rg 100 100 200 150 re f");
    assert!(!page.degraded, "{:?}", page.warnings);
    assert_eq!(page.drawn_ops, 1);

    let (cx, cy) = at(200.0, 175.0);
    assert_near(
        pixel_at(&page, cx, cy),
        [255, 0, 0],
        2,
        "Mitte des Rechtecks",
    );
    assert!(
        is_white(pixel_at(&page, 20, 20)),
        "außerhalb des Rechtecks darf nichts stehen"
    );
    let (ox, oy) = at(350.0, 350.0);
    assert!(is_white(pixel_at(&page, ox, oy)), "rechts oben ist frei");
}

// ---------------------------------------------------------------------------
// 3 — Farbräume
// ---------------------------------------------------------------------------

#[test]
fn fill_colors_are_converted_from_every_device_space() {
    // DeviceGray 0.5, DeviceRGB Blau, DeviceCMYK Rot.
    let cases: [(&str, [u8; 3]); 3] = [
        ("0.5 g 100 100 200 200 re f", [128, 128, 128]),
        ("0 0 1 rg 100 100 200 200 re f", [0, 0, 255]),
        ("0 1 1 0 k 100 100 200 200 re f", [255, 0, 0]),
    ];
    for (content, expected) in cases {
        let page = render_square(content);
        assert!(!page.degraded, "{content}: {:?}", page.warnings);
        let (x, y) = at(200.0, 200.0);
        assert_near(pixel_at(&page, x, y), expected, 2, content);
    }
}

// ---------------------------------------------------------------------------
// 4 — Even-Odd gegen Nonzero
// ---------------------------------------------------------------------------

#[test]
fn even_odd_and_nonzero_differ_on_a_self_overlapping_path() {
    // Zwei ineinanderliegende Rechtecke, `re` erzeugt beide im selben
    // Umlaufsinn: Nonzero füllt durch, Even-Odd stanzt ein Loch.
    let path = "0 0 0 rg 50 50 300 300 re 150 150 100 100 re";
    let nonzero = render_square(&format!("{path} f"));
    let even_odd = render_square(&format!("{path} f*"));

    let (hx, hy) = at(200.0, 200.0);
    assert_near(
        pixel_at(&nonzero, hx, hy),
        [0, 0, 0],
        2,
        "Nonzero füllt das innere Rechteck mit",
    );
    assert!(
        is_white(pixel_at(&even_odd, hx, hy)),
        "Even-Odd muss ein Loch lassen, gemessen {:?}",
        pixel_at(&even_odd, hx, hy)
    );

    // Der Ring dazwischen ist in beiden Fällen schwarz.
    let (rx, ry) = at(100.0, 100.0);
    assert_near(pixel_at(&nonzero, rx, ry), [0, 0, 0], 2, "Ring/Nonzero");
    assert_near(pixel_at(&even_odd, rx, ry), [0, 0, 0], 2, "Ring/Even-Odd");
    assert!(non_white_ratio(&even_odd) < non_white_ratio(&nonzero));
}

// ---------------------------------------------------------------------------
// 5 — Striche, auch haarfein
// ---------------------------------------------------------------------------

#[test]
fn strokes_show_up_even_at_width_zero() {
    let thick = render_square("0 0 0 RG 4 w 50 200 m 350 200 l S");
    assert!(!thick.degraded, "{:?}", thick.warnings);
    let (x, y) = at(200.0, 200.0);
    assert_near(pixel_at(&thick, x, y), [0, 0, 0], 2, "4 pt breiter Strich");

    // Breite 0 heißt in PDF „dünnste darstellbare Linie“ — die Vorschau darf
    // sie nicht verschlucken, sonst fehlen alle Tabellenlinien.
    let hairline = render_square("0 0 0 RG 0 w 50 200 m 350 200 l S");
    assert!(!hairline.degraded, "{:?}", hairline.warnings);
    assert!(
        non_white_ratio(&hairline) > 0.0,
        "Haarlinie ist unsichtbar geblieben"
    );
    assert!(
        !is_white(pixel_at(&hairline, x, y)),
        "Haarlinie fehlt genau in der Mitte"
    );
}

// ---------------------------------------------------------------------------
// 6 — Beschnitt
// ---------------------------------------------------------------------------

#[test]
fn clipping_actually_clips() {
    let plain = render_square("0 0 0 rg 100 100 200 200 re f");
    let clipped = render_square("q 100 100 60 60 re W n 0 0 0 rg 100 100 200 200 re f Q");

    assert!(!clipped.degraded, "{:?}", clipped.warnings);
    assert_ne!(plain.rgba, clipped.rgba, "der Clip hat nichts bewirkt");

    // Innerhalb des Clips: beide schwarz.
    let (ix, iy) = at(130.0, 130.0);
    assert_near(pixel_at(&plain, ix, iy), [0, 0, 0], 2, "ungeclippt innen");
    assert_near(pixel_at(&clipped, ix, iy), [0, 0, 0], 2, "geclippt innen");

    // Außerhalb des Clips, aber innerhalb des Rechtecks: nur ungeclippt schwarz.
    let (ox, oy) = at(250.0, 250.0);
    assert_near(pixel_at(&plain, ox, oy), [0, 0, 0], 2, "ungeclippt außen");
    assert!(
        is_white(pixel_at(&clipped, ox, oy)),
        "weggeschnittener Bereich muss weiß bleiben, gemessen {:?}",
        pixel_at(&clipped, ox, oy)
    );
    assert!(non_white_ratio(&clipped) < non_white_ratio(&plain));
}

// ---------------------------------------------------------------------------
// 7 — Bilder: Farben und Ausrichtung
// ---------------------------------------------------------------------------

/// 2x2-Bild: oben links rot, oben rechts grün, unten links blau,
/// unten rechts gelb — jede Ecke unterscheidbar.
fn corner_image_doc() -> Document {
    let mut doc = Document::with_version("1.5");
    let samples: Vec<u8> = vec![
        255, 0, 0, // oben links
        0, 255, 0, // oben rechts
        0, 0, 255, // unten links
        255, 255, 0, // unten rechts
    ];
    let image_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 2_i64,
            "Height" => 2_i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
        },
        samples,
    ));
    let resources = dictionary! {
        "XObject" => dictionary! { "Im0" => image_id },
    };
    assemble(
        &mut doc,
        "q 200 0 0 200 100 100 cm /Im0 Do Q",
        resources,
        media(0.0, 0.0, 400.0, 400.0),
        None,
    );
    doc
}

#[test]
fn images_keep_their_colors_and_orientation() {
    let doc = corner_image_doc();
    let page = PageRenderer::new().render(&doc, 0, &square_opts());
    assert!(!page.degraded, "{:?}", page.warnings);
    assert_eq!(page.drawn_ops, 1, "Bild wurde nicht gezeichnet");

    // Das Bild belegt den User-Space-Bereich 100..300 in x und y. Die erste
    // Bildzeile gehört im PDF an die OBERE Kante des Einheitsquadrats — die
    // Verwechslung ist der klassische Fehler.
    let quadrants: [(f64, f64, [u8; 3], &str); 4] = [
        (150.0, 250.0, [255, 0, 0], "oben links = rot"),
        (250.0, 250.0, [0, 255, 0], "oben rechts = gruen"),
        (150.0, 150.0, [0, 0, 255], "unten links = blau"),
        (250.0, 150.0, [255, 255, 0], "unten rechts = gelb"),
    ];
    for (ux, uy, expected, what) in quadrants {
        let (x, y) = at(ux, uy);
        assert_near(pixel_at(&page, x, y), expected, 24, what);
    }
}

// ---------------------------------------------------------------------------
// 8 — /Rotate
// ---------------------------------------------------------------------------

fn rotated_doc(rotate: i64) -> Document {
    let mut doc = Document::with_version("1.5");
    // Querformat 400 x 200, ein schwarzer Klotz in der linken unteren Ecke.
    assemble(
        &mut doc,
        "0 0 0 rg 10 10 60 40 re f",
        Dictionary::new(),
        media(0.0, 0.0, 400.0, 200.0),
        Some(rotate),
    );
    doc
}

#[test]
fn rotate_90_swaps_the_canvas_and_moves_the_content() {
    let opts = RenderOptions {
        width: 400,
        ..RenderOptions::default()
    };
    let upright = PageRenderer::new().render(&rotated_doc(0), 0, &opts);
    let turned = PageRenderer::new().render(&rotated_doc(90), 0, &opts);

    // `RenderOptions::width` gibt die Pixelbreite vor, die Höhe folgt dem
    // Seitenverhältnis — aus dem Querformat wird deshalb ein Hochformat.
    assert_eq!((upright.width, upright.height), (400, 200));
    assert_eq!((turned.width, turned.height), (400, 800));
    let aspect = |page: &RenderedPage| f64::from(page.width) / f64::from(page.height);
    assert!(
        (aspect(&upright) - 1.0 / aspect(&turned)).abs() < 1e-9,
        "Breite und Höhe wurden nicht getauscht: {:?} vs {:?}",
        (upright.width, upright.height),
        (turned.width, turned.height)
    );
    assert_eq!(upright.rotate, 0);
    assert_eq!(turned.rotate, 90);
    assert_eq!(turned.page_box.width(), upright.page_box.height());
    assert_eq!(turned.page_box.height(), upright.page_box.width());

    // Ohne Drehung sitzt der Klotz links UNTEN, mit 90° (im Uhrzeigersinn)
    // links OBEN.
    assert_near(
        pixel_at(&upright, 40, 170),
        [0, 0, 0],
        2,
        "0 Grad: links unten",
    );
    assert!(
        is_white(pixel_at(&upright, 40, 30)),
        "0 Grad: links oben frei"
    );

    assert_near(
        pixel_at(&turned, 60, 80),
        [0, 0, 0],
        2,
        "90 Grad: links oben",
    );
    assert!(
        is_white(pixel_at(&turned, 60, 700)),
        "90 Grad: links unten frei"
    );
    assert!(
        is_white(pixel_at(&turned, 300, 80)),
        "90 Grad: rechts oben frei"
    );
}

// ---------------------------------------------------------------------------
// 9 — Entartete MediaBox
// ---------------------------------------------------------------------------

#[test]
fn degenerate_media_boxes_still_produce_an_image() {
    let cases: [(&str, Vec<Object>); 3] = [
        ("Nullgröße", media(0.0, 0.0, 0.0, 0.0)),
        ("negativ", media(0.0, 0.0, -0.5, -0.5)),
        ("absurd groß", media(0.0, 0.0, 1.0e9, 1.0e9)),
    ];
    for (label, box_objects) in cases {
        let mut doc = Document::with_version("1.5");
        assemble(
            &mut doc,
            "0 0 0 rg 100 100 200 200 re f",
            Dictionary::new(),
            box_objects,
            None,
        );
        let page = PageRenderer::new().render(&doc, 0, &RenderOptions::default());

        assert!(page.width > 0 && page.height > 0, "{label}: leeres Bild");
        assert_eq!(
            page.rgba.len(),
            page.width as usize * page.height as usize * 4,
            "{label}: Puffergröße"
        );
        assert!(
            page.degraded || !page.warnings.is_empty(),
            "{label}: hätte gemeldet werden müssen"
        );
    }
}

// ---------------------------------------------------------------------------
// 10 — Kaputte Seite
// ---------------------------------------------------------------------------

#[test]
fn a_broken_page_yields_a_white_sheet_instead_of_an_error() {
    // Der Seitenbaum verweist auf ein Objekt, das es nicht gibt: `page_ops`
    // scheitert, `render` muss das auffangen.
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference((9999, 0))],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let page = PageRenderer::new().render(&doc, 0, &RenderOptions::default());
    assert!(page.degraded, "kaputte Seite muss als degraded gelten");
    assert!(!page.warnings.is_empty(), "ohne Begründung ist es wertlos");
    assert!(page.width > 0 && page.height > 0);
    assert_eq!(page.drawn_ops, 0);
    assert_eq!(non_white_ratio(&page), 0.0, "das Blatt muss weiß sein");
}

// ---------------------------------------------------------------------------
// 11 — Seitenindex außerhalb des Bereichs
// ---------------------------------------------------------------------------

#[test]
fn an_out_of_range_page_index_is_survivable() {
    let pdf = redact_pdf::testing::demo_statement();
    let doc = Document::load_mem(&pdf).expect("Demo lesbar");
    let mut renderer = PageRenderer::new();

    for index in [2usize, 99, usize::MAX] {
        let page = renderer.render(&doc, index, &RenderOptions::default());
        assert!(
            page.width > 0 && page.height > 0,
            "Index {index}: leeres Bild"
        );
        assert!(
            !page.rgba.is_empty() && page.rgba.len().is_multiple_of(4),
            "Index {index}: Puffer"
        );
        assert!(page.degraded, "Index {index}: müsste degraded sein");
        assert!(!page.warnings.is_empty(), "Index {index}: Warnung fehlt");
    }
}

// ---------------------------------------------------------------------------
// 12 — Nicht dekodierbares Bild
// ---------------------------------------------------------------------------

#[test]
fn an_undecodable_image_still_paints_a_placeholder() {
    let mut doc = Document::with_version("1.5");
    // JPXDecode wird bewusst nicht dekodiert; `page_ops` liefert eine
    // Ersatzfläche und einen Hinweis.
    let image_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 8_i64,
            "Height" => 8_i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Filter" => "JPXDecode",
        },
        vec![0x00, 0x01, 0x02, 0x03, 0x04],
    ));
    let resources = dictionary! {
        "XObject" => dictionary! { "Im0" => image_id },
    };
    assemble(
        &mut doc,
        "q 200 0 0 200 100 100 cm /Im0 Do Q",
        resources,
        media(0.0, 0.0, 400.0, 400.0),
        None,
    );

    let page = PageRenderer::new().render(&doc, 0, &square_opts());
    assert!(!page.degraded, "{:?}", page.warnings);
    assert_eq!(page.drawn_ops, 1, "Platzhalter wurde nicht gezeichnet");
    assert!(
        !page.warnings.is_empty(),
        "das unlesbare Bild hätte gemeldet werden müssen"
    );
    let (x, y) = at(200.0, 200.0);
    assert!(
        !is_white(pixel_at(&page, x, y)),
        "die Ersatzfläche ist nicht zu sehen"
    );
    assert!(non_white_ratio(&page) > 0.2, "Ersatzfläche zu klein");
}

// ---------------------------------------------------------------------------
// 13 — Nicht eingebetteter Standard-14-Font
// ---------------------------------------------------------------------------

#[test]
fn standard_14_fonts_render_through_the_bundled_fallback() {
    // `build_pdf` setzt Helvetica/WinAnsi ohne eingebettetes Fontprogramm —
    // ohne Ersatzfont bliebe die Seite leer.
    let pdf = redact_pdf::testing::build_pdf(&[vec![redact_pdf::testing::TextItem::new(
        40.0,
        200.0,
        48.0,
        "Hamburgefonstiv",
    )]]);
    let opts = RenderOptions {
        width: 600,
        ..RenderOptions::default()
    };
    let mut renderer = PageRenderer::new();
    let page = render_with(&mut renderer, &pdf, 0, &opts);

    assert!(!page.degraded, "{:?}", page.warnings);
    assert!(page.drawn_ops >= 14, "nur {} Glyphen", page.drawn_ops);
    assert!(renderer.font_count() > 0, "kein Font geladen");
    assert!(
        non_white_ratio(&page) > 0.001,
        "Ersatzfont hat nichts gezeichnet: {:.5}",
        non_white_ratio(&page)
    );
}

fn render_with(
    renderer: &mut PageRenderer,
    pdf: &[u8],
    page_index: usize,
    opts: &RenderOptions,
) -> RenderedPage {
    let doc = Document::load_mem(pdf).expect("Testdokument lesbar");
    renderer.render(&doc, page_index, opts)
}

// ---------------------------------------------------------------------------
// 14 — max_pixels
// ---------------------------------------------------------------------------

#[test]
fn max_pixels_caps_a_gigantic_page() {
    let mut doc = Document::with_version("1.5");
    assemble(
        &mut doc,
        "0 0 0 rg 0 0 20000 20000 re f",
        Dictionary::new(),
        media(0.0, 0.0, 20_000.0, 20_000.0),
        None,
    );
    let page = PageRenderer::new().render(
        &doc,
        0,
        &RenderOptions {
            width: 5_000,
            max_pixels: 300,
            ..RenderOptions::default()
        },
    );
    assert!(
        page.width <= 300 && page.height <= 300,
        "{}x{} überschreitet max_pixels",
        page.width,
        page.height
    );
    assert!(page.width > 0 && page.height > 0);
    assert_eq!(
        page.rgba.len(),
        page.width as usize * page.height as usize * 4
    );
}

// ---------------------------------------------------------------------------
// 15 — Reproduzierbarkeit
// ---------------------------------------------------------------------------

#[test]
fn rendering_twice_is_byte_identical() {
    let pdf = redact_pdf::testing::demo_statement();
    let doc = Document::load_mem(&pdf).expect("Demo lesbar");
    let opts = RenderOptions {
        width: 500,
        ..RenderOptions::default()
    };

    // Einmal mit frischem Renderer, einmal mit warmem Font-Cache — beide Wege
    // müssen dasselbe Bild liefern.
    let first = PageRenderer::new().render(&doc, 0, &opts);
    let mut warm = PageRenderer::new();
    let _ = warm.render(&doc, 0, &opts);
    let second = warm.render(&doc, 0, &opts);

    assert_eq!(first.width, second.width);
    assert_eq!(first.height, second.height);
    assert_eq!(first.rgba, second.rgba, "Rendern ist nicht deterministisch");
    assert_eq!(first.drawn_ops, second.drawn_ops);
    assert_eq!(first.warnings, second.warnings);
}

// ---------------------------------------------------------------------------
// 16 — draw_text abschalten
// ---------------------------------------------------------------------------

#[test]
fn disabling_text_removes_ink() {
    let pdf = redact_pdf::testing::demo_statement();
    let doc = Document::load_mem(&pdf).expect("Demo lesbar");
    let mut renderer = PageRenderer::new();

    let with_text = renderer.render(
        &doc,
        0,
        &RenderOptions {
            draw_text: true,
            ..RenderOptions::default()
        },
    );
    let without_text = renderer.render(
        &doc,
        0,
        &RenderOptions {
            draw_text: false,
            ..RenderOptions::default()
        },
    );

    let inked = non_white_ratio(&with_text);
    let bare = non_white_ratio(&without_text);
    assert!(
        bare < inked,
        "ohne Text ({bare:.5}) müsste weniger Farbe auf der Seite sein als mit ({inked:.5})"
    );
    assert_eq!(without_text.drawn_ops, 0, "der Auszug enthält nur Text");
    assert_eq!(bare, 0.0, "ohne Text bleibt das Blatt weiß");
}

// ---------------------------------------------------------------------------
// 17 — Leere Seite
// ---------------------------------------------------------------------------

#[test]
fn an_empty_page_is_a_clean_white_sheet() {
    // Seite ganz ohne /Contents.
    let mut doc = Document::with_version("1.5");
    let resources_id = doc.add_object(Object::Dictionary(Dictionary::new()));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Resources" => resources_id,
        "MediaBox" => media(0.0, 0.0, 400.0, 400.0),
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let page = PageRenderer::new().render(&doc, 0, &square_opts());
    assert_eq!((page.width, page.height), (400, 400));
    assert_eq!(page.drawn_ops, 0);
    assert_eq!(non_white_ratio(&page), 0.0);

    // Und dasselbe mit einem vorhandenen, aber leeren Content-Stream.
    let empty_stream = render_square("");
    assert_eq!((empty_stream.width, empty_stream.height), (400, 400));
    assert_eq!(empty_stream.drawn_ops, 0);
    assert_eq!(non_white_ratio(&empty_stream), 0.0);
    assert!(!empty_stream.degraded, "{:?}", empty_stream.warnings);
}

// ---------------------------------------------------------------------------
// 18 — Eine Seite, auf der ausschließlich gezeichnet wird
// ---------------------------------------------------------------------------

/// Der ursprüngliche Fehlerfall dieser Datei: eine Seite ganz **ohne Text**.
///
/// Sie besteht nur aus Linien- und Kurvenbefehlen (`m`, `l`, `c`, `re`, `S`,
/// `f`) — genau wie die technischen Zeichnungen, an denen die alte, rein
/// textbasierte Vorschau als leeres weißes Blatt scheiterte. Kein `BT`, kein
/// `Tj`, keine einzige Glyphe.
///
/// # Warum das hier erzeugt und nicht gelesen wird
///
/// Bis eben belegten drei *echte* Dateien von dieser Maschine diesen Punkt:
/// `AnnotationDemo.pdf` aus dem entpackten `lopdf`-Quelltext, dazu ein PDF aus
/// `/mnt/skills` und eines aus `/usr/lib/libreoffice`. Fehlten sie, kehrte der
/// Test mit „keine echten PDFs vorhanden — Test übersprungen“ **grün** zurück,
/// ohne irgendetwas geprüft zu haben. Auf jedem CI-Runner war das der
/// Normalfall: keine der drei Dateien liegt dort. Ein Test, der bei fehlendem
/// Material stillschweigend nichts prüft, ist schlimmer als ein roter — er
/// beweist nichts und sieht aus, als täte er es.
///
/// Die Zeichnung unten enthält dasselbe, worauf es ankam: nur Pfade, keine
/// Schrift. Sie liegt im Repository und prüft deshalb auf jeder Maschine.
fn drawing_only_page() -> String {
    let mut ops = String::from("0.15 0.15 0.15 RG 1.2 w\n");
    // Ein Raster aus Linien — Striche, wie sie eine Zeichnung ausmacht.
    for i in 0..20 {
        let v = 20.0 + f64::from(i) * 18.0;
        ops.push_str(&format!("{v:.1} 20 m {v:.1} 380 l S\n"));
        ops.push_str(&format!("20 {v:.1} m 380 {v:.1} l S\n"));
    }
    // Eine Kurve und zwei Flächen darüber, damit auch `c` und `f` vorkommen.
    ops.push_str("0.85 0.25 0.1 RG 3 w 40 60 m 140 340 260 40 360 320 c S\n");
    ops.push_str("0.1 0.35 0.75 rg 60 60 120 90 re f\n");
    ops.push_str("0.95 0.7 0.1 rg 220 250 120 90 re f\n");
    ops
}

#[test]
fn a_page_without_a_single_glyph_still_shows_its_drawing() {
    let page = render_square(&drawing_only_page());

    assert!(!page.degraded, "Warnungen: {:?}", page.warnings);
    assert!(page.drawn_ops > 0, "keine einzige Operation gezeichnet");
    let ratio = non_white_ratio(&page);
    println!(
        "reine Zeichnung: {}x{}, {} Ops, non-white = {:.4} %",
        page.width,
        page.height,
        page.drawn_ops,
        ratio * 100.0
    );
    assert!(
        ratio > 0.01,
        "die Zeichnung ist praktisch unsichtbar: nur {:.4} % nicht-weiß",
        ratio * 100.0
    );

    // Gegenprobe zum Namen des Tests: es ist wirklich kein Text im Spiel.
    // Ohne sie könnte irgendwann Text dazukommen und der Test hieße nur noch so.
    let ohne_text = PageRenderer::new().render(
        &square_page(&drawing_only_page()),
        0,
        &RenderOptions {
            draw_text: false,
            ..square_opts()
        },
    );
    assert_eq!(
        ohne_text.rgba, page.rgba,
        "die Seite verliert Farbe, wenn Text abgeschaltet wird — dann steht \
         doch Text darauf und der Test prüft nicht, was er behauptet"
    );
}

// ---------------------------------------------------------------------------
// Sichtprüfung: PNGs herausschreiben
// ---------------------------------------------------------------------------

/// Legt ein paar gerenderte Seiten als PNG ab, damit ein Mensch sie ohne
/// Bildschirm begutachten kann.
///
/// Kein Prüfling, sondern ein Werkzeug — es steht hier, weil der PNG-Schreiber
/// darunter ohnehin gebraucht wird und sonst niemand ihn benutzte. Das Ziel ist
/// `<Temp>/redact-render-sichtpruefung/`; früher stand hier der absolute Pfad
/// eines fremden Arbeitsverzeichnisses, das es auf keiner anderen Maschine gab,
/// woraufhin der „Test“ wortlos zurückkehrte.
#[test]
fn writes_reference_pngs() {
    let target = std::env::temp_dir().join("redact-render-sichtpruefung");
    std::fs::create_dir_all(&target).expect("Ausgabeordner anlegbar");

    let seiten: [(&str, RenderedPage); 2] = [
        (
            "kontoauszug",
            render_bytes(
                &redact_pdf::testing::demo_statement(),
                0,
                &RenderOptions::default(),
            ),
        ),
        ("zeichnung", render_square(&drawing_only_page())),
    ];
    for (name, page) in seiten {
        let out = target.join(format!("{name}-p0.png"));
        write_png(&out, page.width, page.height, &page.rgba).expect("PNG schreibbar");
        println!("geschrieben: {}", out.display());
    }
}

/// Minimaler PNG-Schreiber (RGBA8, Deflate-„stored“-Blöcke).
///
/// Reicht für die Sichtprüfung und erspart eine zusätzliche Abhängigkeit —
/// `tiny-skia` ist ohne das Feature `png-format` eingebunden.
fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
    assert_eq!(rgba.len(), width as usize * height as usize * 4);

    // Jede Zeile bekommt das Filter-Byte 0 („None“) vorangestellt.
    let stride = width as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for row in rgba.chunks_exact(stride) {
        raw.push(0u8);
        raw.extend_from_slice(row);
    }

    let mut zlib = vec![0x78u8, 0x01];
    let mut offset = 0usize;
    loop {
        let len = (raw.len() - offset).min(0xFFFF);
        let last = offset + len == raw.len();
        zlib.push(u8::from(last));
        zlib.extend_from_slice(&(len as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(len as u16)).to_le_bytes());
        zlib.extend_from_slice(&raw[offset..offset + len]);
        offset += len;
        if last {
            break;
        }
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 Bit, RGBA, keine Verschachtelung
    png_chunk(&mut out, b"IHDR", &header);
    png_chunk(&mut out, b"IDAT", &zlib);
    png_chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out)
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut checked = Vec::with_capacity(kind.len() + data.len());
    checked.extend_from_slice(kind);
    checked.extend_from_slice(data);
    out.extend_from_slice(&checked);
    out.extend_from_slice(&crc32(&checked).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut low, mut high) = (1u32, 0u32);
    for byte in data {
        low = (low + u32::from(*byte)) % 65_521;
        high = (high + low) % 65_521;
    }
    (high << 16) | low
}

// ---------------------------------------------------------------------------
// Kleinkram, der sonst nirgends hinpasst
// ---------------------------------------------------------------------------

/// Stellt sicher, dass die Testhilfen selbst stimmen — sonst messen alle
/// anderen Tests ins Leere.
#[test]
fn helpers_agree_with_the_crate_itself() {
    let page = render_square("0 0 0 rg 0 0 400 400 re f");
    assert!((non_white_ratio(&page) - page.non_white_ratio()).abs() < f64::EPSILON);
    assert_eq!(non_white_ratio(&page), 1.0, "die Seite ist ganz schwarz");
    assert_eq!(pixel_at(&page, 0, 0), [0, 0, 0, 255]);
    assert!(page.pixel(page.width, 0).is_none());
}
