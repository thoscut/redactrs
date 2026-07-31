//! Seitenbilder im Hintergrund rastern und als Textur vorhalten.
//!
//! ## Warum ein eigener Thread
//!
//! `redact_render::PageRenderer::render` braucht für eine dicht gesetzte A4-Seite
//! je nach Zoomstufe zweistellige Millisekunden bis über eine Sekunde. Liefe das
//! im Zeichentakt, stünde die Oberfläche bei jedem Seitenwechsel und bei jeder
//! Bewegung des Zoomreglers. Deshalb:
//!
//! * ein einziger Arbeits-Thread ([`std::thread`]), zwei Kanäle
//!   ([`std::sync::mpsc`]) — Aufträge hin, fertige Bilder zurück;
//! * der Thread ruft nach jedem Ergebnis `ctx.request_repaint()`, damit das Bild
//!   auch dann erscheint, wenn die Oberfläche gerade nichts zu tun hat;
//! * wartende Aufträge werden **zusammengefasst**: zieht jemand am Zoomregler,
//!   überholen sich die Anforderungen, und nur die letzte wird gerechnet.
//!
//! ## Zwischenspeicher
//!
//! Je Seite höchstens zwei Texturen:
//!
//! * ein **Kleinbild** ([`THUMB_WIDTH`] Pixel breit) — bleibt liegen, damit beim
//!   Zurückblättern sofort etwas zu sehen ist und die Wartezeit auf das große
//!   Bild überbrückt wird;
//! * das **Vollbild** in der aktuellen Zoomstufe — nur für die gerade
//!   angezeigte Seite; beim Seitenwechsel werden die anderen freigegeben.
//!
//! Neu gerechnet wird ausschließlich bei einem Wechsel der Seite oder der
//! Zoomstufe. Damit ein Ziehen am Regler nicht hundert Aufträge erzeugt, wird
//! die Zielbreite auf ein Raster von [`WIDTH_STEP`] Pixeln gerundet
//! ([`target_width`]).

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;

use egui::{ColorImage, TextureHandle, TextureOptions};
use redact_core::Rect;
use redact_render::{PageRenderer, RenderOptions, RenderedPage};

use crate::viewer::PageView;

/// Breite der Kleinbilder in Pixeln.
pub const THUMB_WIDTH: u32 = 240;
/// Obergrenze für die Kantenlänge eines Vollbilds.
pub const MAX_RENDER_WIDTH: u32 = 4000;
/// Raster, auf das die angeforderte Breite gerundet wird.
pub const WIDTH_STEP: u32 = 64;

/// Gewünschte Bildbreite für eine Seite bei gegebener Zoomstufe.
///
/// Reine Funktion: gerundet auf [`WIDTH_STEP`] und begrenzt durch die größte
/// Textur, die die Grafikkarte annimmt (`max_texture_side`). Ohne das Runden
/// würde jede Zwischenstellung des Zoomreglers einen eigenen Renderauftrag
/// auslösen.
pub fn target_width(view: &PageView, zoom: f32, max_texture_side: usize) -> u32 {
    let display = view.display_box();
    let wanted = (display.width() as f32 * zoom).max(1.0);
    let stepped = ((wanted / WIDTH_STEP as f32).ceil() as u32).max(1) * WIDTH_STEP;

    // Auch die Höhe muss in die Textur passen — sonst lehnt egui sie ab.
    let aspect = if display.width() > 0.0 {
        (display.height() / display.width()) as f32
    } else {
        1.0
    };
    let side = max_texture_side.clamp(64, MAX_RENDER_WIDTH as usize) as u32;
    let by_height = if aspect > 1.0 {
        ((side as f32) / aspect) as u32
    } else {
        side
    };
    stepped.clamp(WIDTH_STEP, side.min(by_height).max(WIDTH_STEP))
}

// ---------------------------------------------------------------------------
// Nachrichten
// ---------------------------------------------------------------------------

/// Ein Renderauftrag an den Arbeits-Thread.
struct Job {
    generation: u64,
    page: usize,
    width: u32,
    thumb: bool,
    ctx: egui::Context,
}

enum Message {
    /// Neues Dokument; alles Ältere ist damit ungültig.
    Load {
        generation: u64,
        doc: Arc<lopdf::Document>,
    },
    Render(Job),
}

/// Ein fertiges Seitenbild auf dem Weg zurück in die Oberfläche.
struct Done {
    generation: u64,
    page: usize,
    thumb: bool,
    width: u32,
    rendered: RenderedPage,
}

// ---------------------------------------------------------------------------
// Ergebnis je Seite
// ---------------------------------------------------------------------------

/// Was der Rasterizer über eine Seite berichtet hat.
#[derive(Debug, Clone, PartialEq)]
pub struct PageMeta {
    /// Fläche, die das Bild abdeckt (Anzeigeraum nach `/Rotate`).
    pub page_box: Rect,
    /// Angewandte Drehung.
    pub rotate: i64,
    /// Was nicht oder nur näherungsweise dargestellt werden konnte.
    pub warnings: Vec<String>,
    /// Wurde überhaupt etwas gezeichnet?
    pub drawn_ops: usize,
    /// `true`: das Bild zeigt nur den Hintergrund — die schematische Vorschau
    /// muss einspringen.
    pub degraded: bool,
}

/// Texturen und Meldungen einer Seite.
pub struct CachedPage {
    /// Kleinbild; überbrückt die Wartezeit auf das Vollbild.
    pub thumb: Option<TextureHandle>,
    /// Vollbild in der zuletzt angeforderten Zoomstufe.
    pub full: Option<TextureHandle>,
    /// Breite des Vollbilds; `0`, wenn keines vorliegt.
    pub full_width: u32,
    pub meta: PageMeta,
}

impl std::fmt::Debug for CachedPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `TextureHandle` ist nicht `Debug`; die Größe genügt als Auskunft.
        f.debug_struct("CachedPage")
            .field("thumb", &self.thumb.is_some())
            .field("full_width", &self.full_width)
            .field("meta", &self.meta)
            .finish()
    }
}

impl CachedPage {
    /// Die beste vorhandene Textur; `true` heißt „nur das Kleinbild“.
    pub fn best(&self) -> Option<(&TextureHandle, bool)> {
        match (&self.full, &self.thumb) {
            (Some(full), _) => Some((full, false)),
            (None, Some(thumb)) => Some((thumb, true)),
            (None, None) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// PageCache
// ---------------------------------------------------------------------------

/// Verwaltet den Arbeits-Thread und die Texturen.
///
/// Ablauf je Bild: erst [`PageCache::poll`] (fertige Bilder abholen), dann
/// [`PageCache::request`] (fehlende anfordern), dann zeichnen.
pub struct PageCache {
    jobs: Option<Sender<Message>>,
    results: Receiver<Done>,
    worker: Option<std::thread::JoinHandle<()>>,
    /// Zählt Dokumente; Ergebnisse älterer Dokumente werden verworfen.
    generation: u64,
    has_document: bool,
    entries: HashMap<usize, CachedPage>,
    current: Option<usize>,
    /// Laufender Vollbildauftrag (Seite, Breite).
    pending_full: Option<(usize, u32)>,
    /// Laufender Kleinbildauftrag.
    pending_thumb: Option<usize>,
}

impl std::fmt::Debug for PageCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageCache")
            .field("generation", &self.generation)
            .field("cached_pages", &self.entries.len())
            .field("pending_full", &self.pending_full)
            .field("pending_thumb", &self.pending_thumb)
            .finish()
    }
}

impl Default for PageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PageCache {
    pub fn new() -> Self {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<Message>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<Done>();
        let worker = std::thread::Builder::new()
            .name("redact-page-renderer".to_string())
            .spawn(move || worker_loop(job_rx, done_tx))
            .ok();

        Self {
            jobs: worker.is_some().then_some(job_tx),
            results: done_rx,
            worker,
            generation: 0,
            has_document: false,
            entries: HashMap::new(),
            current: None,
            pending_full: None,
            pending_thumb: None,
        }
    }

    /// Übergibt ein neues Dokument. Alles Zwischengespeicherte verfällt.
    pub fn set_document(&mut self, doc: Arc<lopdf::Document>) {
        self.reset();
        self.has_document = true;
        let message = Message::Load {
            generation: self.generation,
            doc,
        };
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send(message);
        }
    }

    /// Wirft alles weg (kein Dokument mehr).
    pub fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.has_document = false;
        self.entries.clear();
        self.current = None;
        self.pending_full = None;
        self.pending_thumb = None;
    }

    /// Holt fertige Bilder ab und legt sie als Texturen an.
    ///
    /// Muss auf dem Oberflächen-Thread laufen — nur dort darf eine Textur
    /// entstehen.
    pub fn poll(&mut self, ctx: &egui::Context) -> usize {
        let mut taken = 0;
        while let Ok(done) = self.results.try_recv() {
            if done.generation != self.generation {
                // Ergebnis zu einem längst abgelösten Dokument.
                continue;
            }
            taken += 1;
            self.accept(ctx, done);
        }
        taken
    }

    fn accept(&mut self, ctx: &egui::Context, done: Done) {
        let meta = PageMeta {
            page_box: done.rendered.page_box,
            rotate: done.rendered.rotate,
            warnings: done.rendered.warnings.clone(),
            drawn_ops: done.rendered.drawn_ops,
            degraded: done.rendered.degraded,
        };
        let name = format!("page-{}-{}", done.page, done.width);
        let texture = ctx.load_texture(name, to_color_image(&done.rendered), texture_options());

        let entry = self.entries.entry(done.page).or_insert_with(|| CachedPage {
            thumb: None,
            full: None,
            full_width: 0,
            meta: meta.clone(),
        });
        entry.meta = meta;
        if done.thumb {
            entry.thumb = Some(texture);
            if self.pending_thumb == Some(done.page) {
                self.pending_thumb = None;
            }
        } else {
            entry.full = Some(texture);
            entry.full_width = done.width;
            if self.pending_full == Some((done.page, done.width)) {
                self.pending_full = None;
            }
        }
    }

    /// Fordert an, was für die aktuelle Ansicht fehlt.
    ///
    /// Löst nur bei einem Wechsel von Seite oder Zoomstufe wirklich einen
    /// Auftrag aus — sonst passiert hier nichts.
    pub fn request(&mut self, page: usize, view: &PageView, zoom: f32, ctx: &egui::Context) {
        if !self.has_document {
            return;
        }
        let width = target_width(view, zoom, ctx.input(|i| i.max_texture_side));

        if self.current != Some(page) {
            self.current = Some(page);
            // Vollauflösung gibt es nur für die sichtbare Seite; die Kleinbilder
            // der anderen bleiben liegen.
            for (index, entry) in self.entries.iter_mut() {
                if *index != page {
                    entry.full = None;
                    entry.full_width = 0;
                }
            }
            self.pending_full = None;
        }

        let has_thumb = self
            .entries
            .get(&page)
            .map(|e| e.thumb.is_some())
            .unwrap_or(false);
        if !has_thumb && self.pending_thumb != Some(page) {
            self.pending_thumb = Some(page);
            self.send(page, THUMB_WIDTH, true, ctx);
        }

        let have = self
            .entries
            .get(&page)
            .map(|e| e.full_width)
            .unwrap_or_default();
        if have != width && self.pending_full != Some((page, width)) {
            self.pending_full = Some((page, width));
            self.send(page, width, false, ctx);
        }
    }

    fn send(&self, page: usize, width: u32, thumb: bool, ctx: &egui::Context) {
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send(Message::Render(Job {
                generation: self.generation,
                page,
                width,
                thumb,
                ctx: ctx.clone(),
            }));
        }
    }

    pub fn page(&self, page: usize) -> Option<&CachedPage> {
        self.entries.get(&page)
    }

    /// Warnungen des Rasterizers zur angegebenen Seite.
    pub fn warnings(&self, page: usize) -> &[String] {
        self.entries
            .get(&page)
            .map(|e| e.meta.warnings.as_slice())
            .unwrap_or_default()
    }

    /// Wartet die Oberfläche gerade auf ein Bild?
    pub fn is_busy(&self) -> bool {
        self.pending_full.is_some() || self.pending_thumb.is_some()
    }

    /// Konnte der Arbeits-Thread gestartet werden?
    pub fn worker_running(&self) -> bool {
        self.worker.is_some()
    }
}

impl Drop for PageCache {
    fn drop(&mut self) {
        // Kanal schließen → `recv` im Thread liefert `Err` → Thread endet.
        self.jobs = None;
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

/// Weiche Filterung: das Bild wird fast nie pixelgenau angezeigt.
fn texture_options() -> TextureOptions {
    TextureOptions::LINEAR
}

/// RGBA8 aus dem Rasterizer → egui-Bild.
fn to_color_image(page: &RenderedPage) -> ColorImage {
    let size = [page.width as usize, page.height as usize];
    let expected = size[0] * size[1] * 4;
    if page.rgba.len() < expected {
        // Kann nach heutigem Stand nicht vorkommen; ein weißes Blatt ist
        // trotzdem besser als eine Panik in der Oberfläche.
        return ColorImage::new(size, egui::Color32::WHITE);
    }
    ColorImage::from_rgba_unmultiplied(size, &page.rgba[..expected])
}

// ---------------------------------------------------------------------------
// Arbeits-Thread
// ---------------------------------------------------------------------------

fn worker_loop(jobs: Receiver<Message>, results: Sender<Done>) {
    let mut renderer = PageRenderer::new();
    let mut document: Option<(u64, Arc<lopdf::Document>)> = None;

    while let Ok(first) = jobs.recv() {
        // Alles, was schon wartet, einsammeln. Von den Renderaufträgen bleibt
        // je Art nur der jüngste übrig — beim Ziehen am Zoomregler ist alles
        // davor ohnehin überholt.
        let mut thumb_job: Option<Job> = None;
        let mut full_job: Option<Job> = None;
        let mut message = first;
        loop {
            match message {
                Message::Load { generation, doc } => {
                    document = Some((generation, doc));
                    thumb_job = None;
                    full_job = None;
                }
                Message::Render(job) => {
                    if job.thumb {
                        thumb_job = Some(job);
                    } else {
                        full_job = Some(job);
                    }
                }
            }
            match jobs.try_recv() {
                Ok(next) => message = next,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        // Kleinbild zuerst: es ist schnell fertig und füllt die Fläche, während
        // das Vollbild noch gerechnet wird.
        for job in [thumb_job, full_job].into_iter().flatten() {
            let Some((generation, doc)) = &document else {
                continue;
            };
            if job.generation != *generation {
                continue;
            }
            let opts = RenderOptions {
                width: job.width,
                max_pixels: MAX_RENDER_WIDTH,
                ..RenderOptions::default()
            };
            let rendered = renderer.render(doc, job.page, &opts);
            let done = Done {
                generation: job.generation,
                page: job.page,
                thumb: job.thumb,
                width: job.width,
                rendered,
            };
            if results.send(done).is_err() {
                return;
            }
            job.ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a4() -> PageView {
        PageView::upright(Rect::new(0.0, 0.0, 600.0, 800.0))
    }

    #[test]
    fn target_width_is_rounded_to_a_grid() {
        let view = a4();
        // 600 pt bei Zoom 1 → 600 px, aufgerundet auf das 64er-Raster = 640.
        assert_eq!(target_width(&view, 1.0, 8192), 640);
        // Kleine Änderungen am Regler ergeben dieselbe Breite …
        assert_eq!(target_width(&view, 1.01, 8192), 640);
        assert_eq!(target_width(&view, 1.05, 8192), 640);
        // … eine deutliche Änderung dagegen eine andere.
        assert_ne!(target_width(&view, 2.0, 8192), 640);
        // Nie 0.
        assert!(target_width(&view, 0.0001, 8192) >= WIDTH_STEP);
    }

    #[test]
    fn target_width_respects_the_texture_limit_in_both_directions() {
        let view = a4(); // hoch: 600 × 800
                         // Die Höhe ist der Engpass: 800/600 · Breite ≤ 2048.
        let width = target_width(&view, 10.0, 2048);
        assert!(width <= 2048, "{width}");
        let height = (width as f64 * 800.0 / 600.0) as u32;
        assert!(height <= 2048, "Höhe {height} passt nicht in die Textur");

        // Quer ist die Breite der Engpass.
        let wide = PageView::upright(Rect::new(0.0, 0.0, 800.0, 600.0));
        assert!(target_width(&wide, 10.0, 2048) <= 2048);
    }

    #[test]
    fn target_width_follows_the_rotated_display_box() {
        let portrait = PageView::new(Rect::new(0.0, 0.0, 600.0, 800.0), 0);
        let turned = PageView::new(Rect::new(0.0, 0.0, 600.0, 800.0), 90);
        // Gedreht ist die Seite 800 pt breit, das Bild also breiter.
        assert!(target_width(&turned, 1.0, 8192) > target_width(&portrait, 1.0, 8192));
    }

    /// Demo-PDF mit `/Rotate` auf jeder Seite.
    fn rotated_demo(rotate: i64) -> lopdf::Document {
        let mut doc = redact_pdf::load_from_bytes(&redact_pdf::testing::demo_statement()).unwrap();
        let ids: Vec<lopdf::ObjectId> = doc.get_pages().values().copied().collect();
        for id in ids {
            doc.get_object_mut(id)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Rotate", rotate);
        }
        doc
    }

    /// **Der Test, an dem alles hängt.**
    ///
    /// Er prüft nicht, dass die Umrechnung in sich stimmig ist (das tun die
    /// Tests in [`crate::viewer`]), sondern dass sie zu dem passt, was der
    /// Rasterizer wirklich malt: jedes Pixel Farbe im Seitenbild muss innerhalb
    /// eines Rechtecks liegen, das [`crate::viewer::pdf_to_screen`] aus den
    /// extrahierten Text-Runs errechnet. Läge die Umrechnung auf gedrehten
    /// Seiten daneben, schwärzte der Export die falsche Stelle.
    #[test]
    fn every_pixel_of_ink_lands_inside_the_mapped_text_runs() {
        use redact_core::Extractor;

        for rotate in [0_i64, 90, 180, 270] {
            let doc = rotated_demo(rotate);
            let runs = redact_pdf::PdfExtractor::new().extract(&doc).unwrap();
            let view = PageView::new(redact_pdf::page_boxes(&doc)[0], rotate);

            let mut renderer = PageRenderer::new();
            let rendered = renderer.render(
                &doc,
                0,
                &RenderOptions {
                    width: 900,
                    ..RenderOptions::default()
                },
            );
            assert!(!rendered.degraded, "rot={rotate}: Seite nicht rasterbar");

            // Die Oberfläche und der Rasterizer müssen dieselbe Fläche meinen.
            assert_eq!(rendered.rotate, view.rotate, "rot={rotate}");
            assert_eq!(rendered.page_box, view.display_box(), "rot={rotate}");

            // Maßstab und Ursprung so wählen, dass ein Bildschirmpunkt genau
            // einem Pixel entspricht.
            let zoom = rendered.width as f32 / view.display_box().width() as f32;
            let origin = egui::Pos2::ZERO;

            // Alle Text-Runs der ersten Seite auf Pixel abbilden.
            let boxes: Vec<egui::Rect> = runs
                .iter()
                .filter(|r| r.page == 0)
                .map(|r| {
                    crate::viewer::pdf_to_screen(&r.rect, &view, zoom, origin)
                        // Rundung, Antialiasing und Unterlängen der Glyphen —
                        // zwei Pixel Luft, mehr nicht.
                        .expand(2.0)
                })
                .collect();
            assert!(!boxes.is_empty());

            let (mut inside, mut outside) = (0_u32, 0_u32);
            for y in 0..rendered.height {
                for x in 0..rendered.width {
                    let Some(px) = rendered.pixel(x, y) else {
                        continue;
                    };
                    // Reines Weiß ist Papier, alles andere ist Farbe.
                    if px[0] == 255 && px[1] == 255 && px[2] == 255 {
                        continue;
                    }
                    let point = egui::Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                    if boxes.iter().any(|b| b.contains(point)) {
                        inside += 1;
                    } else {
                        outside += 1;
                    }
                }
            }

            assert!(inside > 500, "rot={rotate}: kaum Farbe gefunden ({inside})");
            let stray = outside as f64 / (inside + outside) as f64;
            assert!(
                stray < 0.01,
                "rot={rotate}: {outside} von {} farbigen Pixeln liegen außerhalb \
                 der umgerechneten Textkästen ({:.1} %) — die Koordinaten passen nicht \
                 zum Seitenbild",
                inside + outside,
                stray * 100.0
            );
        }
    }

    /// Ein Durchlauf durch den echten Thread: Dokument setzen, anfordern,
    /// warten, Textur einsammeln.
    #[test]
    fn the_worker_thread_delivers_a_texture_for_the_demo_pdf() {
        let ctx = egui::Context::default();
        let doc = redact_pdf::load_from_bytes(&redact_pdf::testing::demo_statement()).unwrap();
        let view = PageView::upright(redact_pdf::page_boxes(&doc)[0]);

        let mut cache = PageCache::new();
        assert!(cache.worker_running());
        // Ohne Dokument wird nichts angefordert.
        cache.request(0, &view, 1.0, &ctx);
        assert!(!cache.is_busy());

        cache.set_document(Arc::new(doc));
        cache.request(0, &view, 1.0, &ctx);
        assert!(cache.is_busy(), "es müssen Aufträge unterwegs sein");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while cache.page(0).and_then(|p| p.full.as_ref()).is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "Rendern dauert zu lang"
            );
            cache.poll(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let page = cache.page(0).unwrap();
        assert!(page.best().is_some());
        assert!(!page.meta.degraded, "Demo-PDF muss rasterbar sein");
        assert_eq!(page.meta.rotate, 0);
        assert!(page.meta.drawn_ops > 0);
        assert_eq!(page.full_width, target_width(&view, 1.0, 2048));

        // Erneutes Anfordern bei gleicher Zoomstufe erzeugt keinen neuen Auftrag.
        cache.request(0, &view, 1.0, &ctx);
        assert!(!cache.is_busy());

        // Seitenwechsel gibt das Vollbild der alten Seite frei.
        cache.request(1, &view, 1.0, &ctx);
        assert!(cache.page(0).unwrap().full.is_none());
        assert!(
            cache.page(0).unwrap().thumb.is_some(),
            "das Kleinbild bleibt liegen"
        );

        // Ein neues Dokument macht alles ungültig.
        cache.reset();
        assert!(cache.page(0).is_none());
        assert!(!cache.is_busy());
    }
}
