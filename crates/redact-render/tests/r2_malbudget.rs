//! Runde 2: das Arbeitsbudget des Malers.
//!
//! Gefunden mit einem Mutations-Lauf (Operator-Vervielfachung): ein kleines
//! Bild-XObject, auf die ganze Seite skaliert und tausendfach mit `Do`
//! gezeichnet. Jede Wiederholung rastert die volle Seitenfläche neu, und der
//! Renderer kannte weder eine Flächen- noch eine Zeitgrenze. Gemessen
//! (opt-level 3): 9,6-kB-Datei mit 1 000 `Do` bei 1000 px Breite = 32 s; die
//! Dauer wuchs linear mit der Zahl der Operationen, 16 MB Content
//! (`--max-parsed-mb`) trugen rund 2 Mio. `Do` — Stunden. In der Oberfläche
//! lief das im einzigen Render-Worker (`redact_gui::render`): die Vorschau
//! dieser **und jeder danach geöffneten** Datei blieb aus.
//!
//! Seither zählt der Maler die bemalte Fläche in Gerätepixeln — Bilder und
//! Clip-Masken gewichtet nach ihrem Aufwand — und hört bei 512 Seitenflächen
//! auf (`PAINT_BUDGET_PAGES` in `redact_render::raster`; das sind 32
//! Vollseitenbilder). Das Bild gilt dann als `degraded`, die
//! Oberfläche legt die schematische Vorschau darüber, und eine Warnung sagt
//! warum. Diese Tests halten beide Seiten fest: der Angriff endet schnell, und
//! eine Seite mit einer Handvoll Vollseitenebenen bleibt unberührt.

use lopdf::{dictionary, Document, Object, Stream};
use redact_render::{PageRenderer, RenderOptions, RenderedPage};
use std::sync::mpsc;
use std::time::Duration;

/// Schmal, damit der Debug-Build schnell bleibt: dort kostet ein
/// Vollseitenbild bei 800 px rund eine Sekunde. Das Budget rechnet in
/// Seitenflächen, also bricht der Maler bei jeder Breite an derselben Stelle
/// ab.
const RENDER_WIDTH: u32 = 200;

/// Großzügig für einen Debug-Build auf einem langsamen Runner: mit Budget
/// dauert der Angriff dort wenige Sekunden, ohne Budget Stunden.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Eine A4-Seite, deren Content `prefix` und danach `draws`-mal `body` ist
/// (`{i}` wird durch die laufende Nummer ersetzt); `/Im0` ist ein
/// mittelgraues 32 × 32-Bild.
fn page_pdf(prefix: &str, body: &str, draws: usize) -> Document {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let img = Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 32, "Height" => 32,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
        },
        vec![128u8; 32 * 32],
    );
    let img_id = doc.add_object(img);
    let mut content = prefix.as_bytes().to_vec();
    for i in 0..draws {
        content.extend_from_slice(body.replace("{i}", &i.to_string()).as_bytes());
    }
    let content_id = doc.add_object(Stream::new(dictionary! {}, content));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => img_id } },
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    // Über die echte Ladefunktion, wie die Oberfläche es tut.
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    redact_pdf::document::load_from_bytes(&bytes).expect("gültige Datei")
}

/// Rendert in einem eigenen Thread und gibt nach [`TIMEOUT`] auf — ein
/// hängender Renderer soll den Test scheitern lassen, nicht den Lauf.
fn render_with_timeout(doc: Document, what: &str) -> RenderedPage {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut renderer = PageRenderer::new();
        let page = renderer.render(
            &doc,
            0,
            &RenderOptions {
                width: RENDER_WIDTH,
                ..RenderOptions::default()
            },
        );
        let _ = tx.send(page);
    });
    rx.recv_timeout(TIMEOUT).unwrap_or_else(|_| {
        panic!("{what}: nach {TIMEOUT:?} noch nicht gerendert — der Maler hat keine Arbeitsgrenze")
    })
}

fn budget_warning(page: &RenderedPage) -> bool {
    page.warnings
        .iter()
        .any(|w| w.starts_with("Vorschau unvollständig"))
}

/// 50 000 `Do` = 400 kB Content, weit unter jeder Grenze. Der Maler bricht nach
/// dem Budget ab, meldet `degraded` und sagt warum — und was bis dahin gemalt
/// wurde, ist zu sehen.
#[test]
fn repeated_full_page_image_draws_do_not_stall_the_preview() {
    let doc = page_pdf("595 0 0 842 0 0 cm\n", "/Im0 Do\n", 50_000);
    let page = render_with_timeout(doc, "50 000 Vollseitenbilder");
    assert!(page.degraded, "Abbruch muss als degraded gelten");
    assert!(budget_warning(&page), "Warnung fehlt: {:?}", page.warnings);
    assert!(
        page.drawn_ops > 0 && page.drawn_ops <= 32,
        "gezeichnet: {}",
        page.drawn_ops
    );
    assert!(
        page.non_white_ratio() > 0.9,
        "das Bild vor dem Abbruch fehlt"
    );
}

/// Ebenso viele Rechtecke über die ganze Seite: auch Farbflächen zählen.
#[test]
fn repeated_full_page_fills_do_not_stall_the_preview() {
    let doc = page_pdf("0.5 g\n", "0 0 595 842 re f\n", 50_000);
    let page = render_with_timeout(doc, "50 000 Vollseitenflächen");
    assert!(page.degraded);
    assert!(budget_warning(&page), "Warnung fehlt: {:?}", page.warnings);
}

/// Jede Füllung mit eigenem, winzigem Clip: kaum Fläche, aber je Clip eine
/// bildgroße Maske. Auch das muss das Budget treffen.
#[test]
fn many_distinct_clips_do_not_stall_the_preview() {
    let doc = page_pdf("0.5 g\n", "q {i} 0 1 1 re W n 0 0 595 1 re f Q\n", 50_000);
    let page = render_with_timeout(doc, "50 000 verschiedene Clips");
    assert!(page.degraded);
    assert!(budget_warning(&page), "Warnung fehlt: {:?}", page.warnings);
}

/// Die Gegenprobe: zwanzig Vollseitenebenen (Scan, Hintergrund, Overlays —
/// mehr hat keine gewöhnliche Seite) bleiben weit unter dem Budget.
#[test]
fn a_few_full_page_layers_stay_within_the_budget() {
    let doc = page_pdf("595 0 0 842 0 0 cm\n", "/Im0 Do\n", 20);
    let page = render_with_timeout(doc, "20 Vollseitenbilder");
    assert!(!page.degraded, "{:?}", page.warnings);
    assert!(!budget_warning(&page), "{:?}", page.warnings);
    assert_eq!(page.drawn_ops, 20);
}
