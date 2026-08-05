//! Korpus-Test: die Vorschau darf bei keiner Art von PDF versagen.
//!
//! Anforderung des Auftraggebers: „die darstellung sollte immer funktionieren,
//! dafür sollte es tests mit unterschiedlichen arten von pdfs geben, um
//! auszuschließen, dass die darstellung versagt“.
//!
//! Diese Datei ist der Beweis. Ein breites Korpus (siehe [`corpus_gen`]:
//! Text-, Vektor-, Bild-, Struktur- und bewusst kaputte Dateien) wird durch
//! [`PageRenderer::render`] geschickt.
//! Geprüft wird für **jede Seite jedes Beispiels**:
//!
//! * der Aufruf kehrt zurück — keine Panik, kein Hänger,
//! * `width > 0`, `height > 0`, `rgba.len() == width * height * 4`,
//! * bei `expect_content`: mehr als 0,1 % der Pixel sind nicht reinweiß
//!   (**das** ist die eigentliche Prüfung — eine leere Vorschau ist der Fehler,
//!   den diese Datei ausschließen soll; von der Schwelle darf nur abweichen,
//!   wer in `Sample::min_non_white` begründet, warum — derzeit **niemand**,
//!   siehe [`only_documented_samples_lower_the_threshold`]),
//! * zwei Läufe liefern Byte für Byte dasselbe Bild.
//!
//! Zusätzlich läuft das ganze Korpus durch Extraktion und Schwärzung und wird
//! danach erneut gerendert — direkt und nach einem Speicher-/Ladezyklus: auch
//! ein geschwärztes Dokument muss darstellbar bleiben.
//!
//! # Was hier *nicht* geprüft wird: die Maschine
//!
//! Das Korpus enthält keine Datei aus der Umgebung mehr. Sechs Beispiele
//! stammten früher aus `~/.cargo/registry`, `/usr/lib` und `/mnt` — jeweils mit
//! „fehlt die Datei, fällt das Beispiel weg“. Damit war das Ergebnis dieser
//! Suite eine Aussage über den Rechner statt über den Renderer: derselbe
//! Testbinary war mit gefülltem Cargo-Cache grün und mit leerem rot. Die
//! Begründung und die Ersatzbeispiele stehen in der Modulbeschreibung von
//! [`corpus_gen`]; hier hält [`the_corpus_is_complete`] fest, dass keines
//! stillschweigend verschwinden kann.

mod corpus_gen;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use corpus_gen::{corpus, Category, Sample, DEFAULT_MIN_NON_WHITE};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_render::{PageRenderer, RenderOptions, RenderedPage};

// ---------------------------------------------------------------------------
// Schwellwerte
// ---------------------------------------------------------------------------

/// Zeitbudget je Beispiel (alle Seiten zusammen). Wird es gerissen, ist das
/// ebenso ein Ausfall der Darstellung wie ein weißes Blatt: eine Vorschau, die
/// nach zehn Sekunden noch rechnet, ist für den Benutzer ausgefallen.
///
/// Damit die Messung etwas über den Renderer aussagt und nicht über die
/// Auslastung der Maschine, wird das Korpus **einmal** gerendert (siehe
/// [`rendered_corpus`]) und die dort gemessene Zeit von allen Tests geteilt —
/// und zwar unter [`RASTERBANK`], damit nicht der zweite schwere Test derselben
/// Datei danebenläuft.
const MAX_SAMPLE_TIME: Duration = Duration::from_secs(10);

/// Wer das ganze Korpus rastert, nimmt diese Sperre.
///
/// # Warum das nötig ist
///
/// Der Kommentar über [`MAX_SAMPLE_TIME`] versprach schon immer, die Messung
/// hänge nicht an der Auslastung der Maschine — und hielt es nur zur Hälfte.
/// [`rendered_corpus`] rendert zwar nur einmal, aber
/// [`extraction_and_redaction_survive_the_corpus`] rendert das Korpus daneben
/// noch zweimal, völlig unabhängig. Auf einer Vierkernmaschine fochten damit
/// die beiden schwersten Tests der Datei genau während der Zeitmessung
/// gegeneinander. Nachgemessen an `hostile_huge_content` (2 MB
/// Content-Stream): allein 5,7–6,1 s, nebeneinander 11,0–14,8 s — der
/// Laufzeitwächter schlug also nicht bei einem langsamen Renderer an, sondern
/// bei einem beschäftigten Rechner.
///
/// Die Sperre ist der Gegenentwurf zum bequemen Weg, das Budget anzuheben: das
/// Budget bleibt bei zehn Sekunden und meint jetzt auch zehn Sekunden Rendern.
/// Kosten tut es nichts, im Gegenteil — beide Tests lasten die Maschine ohnehin
/// voll aus, und ohne das gegenseitige Ausbremsen wird die Datei sogar
/// schneller fertig: dreimal 74–76 s ohne Sperre (alle drei Läufe rot), dreimal
/// 49–68 s mit (alle drei grün), auf derselben Vierkernmaschine.
static RASTERBANK: Mutex<()> = Mutex::new(());

/// Nimmt [`RASTERBANK`] und übersteht dabei eine vergiftete Sperre.
///
/// Panikt ein Test, während er die Sperre hält, ist sie danach vergiftet. Das
/// darf nicht *alle* übrigen Tests mit in den Fehlschlag reißen — die eine
/// echte Fehlermeldung wäre sonst zwischen zehn Folgefehlern nicht mehr zu
/// finden.
fn rasterbank() -> MutexGuard<'static, ()> {
    RASTERBANK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Renderoptionen des Korpus.
///
/// Bewusst kleiner als die GUI-Vorgabe (1000 px). Alle Prüfungen hier sind
/// auflösungsunabhängig — der Anteil nicht-weißer Pixel ändert sich mit der
/// Bildgröße kaum, die Rechenzeit dagegen quadratisch. Mit 520 px bleibt das
/// grösste Beispiel (2 MB Content-Stream) bei 5,7–6,1 s (nachgemessen, ohne
/// Nebenläufigkeit) und damit unter dem Zeitbudget. Dass „ohne Nebenläufigkeit“
/// dabei keine Ausrede ist, sondern eine Zusage, hält [`RASTERBANK`] fest.
fn opts() -> RenderOptions {
    RenderOptions {
        width: 520,
        max_pixels: 1040,
        ..RenderOptions::default()
    }
}

// ---------------------------------------------------------------------------
// Hilfen
// ---------------------------------------------------------------------------

/// Prüft die Invarianten, die für **jedes** Bild gelten — auch für das
/// Notnagel-Bild eines kaputten Dokuments.
fn assert_valid_image(page: &RenderedPage, sample: &str, index: usize) {
    assert!(
        page.width > 0 && page.height > 0,
        "{sample} Seite {index}: Bildgröße {}x{}",
        page.width,
        page.height
    );
    assert_eq!(
        page.rgba.len(),
        page.width as usize * page.height as usize * 4,
        "{sample} Seite {index}: RGBA-Puffer passt nicht zur Bildgröße",
    );
}

/// Ein Beispiel samt seinen gerasterten Seiten.
struct Rendered {
    sample: Sample,
    /// Leer, wenn sich die (bewusst kaputte) Datei nicht laden ließ.
    pages: Vec<RenderedPage>,
    /// Zeit für alle Seiten dieses Beispiels zusammen.
    elapsed: Duration,
}

/// Das einmal gerasterte Korpus.
///
/// Mehrere Testfunktionen brauchen dieselben Pixel. Würde jede das Korpus
/// selbst rendern, liefen auf einer Vierkernmaschine ein halbes Dutzend
/// Rasterläufe gegeneinander — die Wanduhrzeit eines Beispiels sagte dann mehr
/// über den Testrunner als über den Renderer, und der Laufzeitwächter würde
/// zufällig anschlagen. Also: einmal rendern, alle lesen mit.
fn rendered_corpus() -> &'static [Rendered] {
    static CACHE: OnceLock<Vec<Rendered>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let _bank = rasterbank();
        corpus().into_iter().map(render_sample).collect()
    })
}

/// Rendert alle Seiten eines Beispiels und misst dabei die Zeit.
fn render_sample(sample: Sample) -> Rendered {
    let doc = match redact_pdf::load_from_bytes(&sample.bytes) {
        Ok(doc) => doc,
        Err(err) => {
            // Nur bösartige Beispiele dürfen hier landen; für sie ist ein
            // nicht ladbares Dokument ein zulässiger Ausgang.
            assert!(sample.is_hostile(), "{} laedt nicht: {err}", sample.name);
            return Rendered {
                sample,
                pages: Vec::new(),
                elapsed: Duration::ZERO,
            };
        }
    };
    let count = redact_pdf::page_count(&doc).max(1);
    let mut renderer = PageRenderer::new();
    let options = opts();

    let start = Instant::now();
    let pages: Vec<RenderedPage> = (0..count)
        .map(|i| renderer.render(&doc, i, &options))
        .collect();
    let elapsed = start.elapsed();
    Rendered {
        sample,
        pages,
        elapsed,
    }
}

// ---------------------------------------------------------------------------
// 1. Das Korpus selbst
// ---------------------------------------------------------------------------

/// Anzahl der Beispiele im Korpus.
///
/// Eine feste Zahl und keine Untergrenze — das ist der Kern der Sache. Solange
/// hier `>= 40` stand, durfte das Korpus je nach Rechner zwischen 48 (frischer
/// Runner: nur die erzeugten Beispiele) und 54 Beispielen haben (Rechner mit
/// den sechs zusätzlich eingelesenen Dateien), und der Test sagte zu jeder Zahl
/// ja. Als der Baum von `lopdf` 0.34 auf 0.42 wechselte, fielen vier davon weg
/// — kein Test hat es bemerkt, vier Commits lang. Wer ein Beispiel hinzufügt,
/// hebt die Zahl hier um eins an: eine Zeile Arbeit, dafür kann keines mehr
/// unbemerkt verschwinden.
const CORPUS_SIZE: usize = 50;

/// Das Korpus ist vollständig — auf jeder Maschine dasselbe.
///
/// Siehe [`CORPUS_SIZE`]. Zusätzlich müssen die Namen eindeutig sein: doppelte
/// Namen machen jede Fehlermeldung dieser Suite mehrdeutig, und ein Beispiel,
/// das versehentlich zweimal antritt, verdeckt eines, das fehlt.
#[test]
fn the_corpus_is_complete() {
    let names: Vec<&str> = corpus().iter().map(|s| s.name).collect();
    assert_eq!(
        names.len(),
        CORPUS_SIZE,
        "das Korpus hat {} statt {CORPUS_SIZE} Beispiele. Vorhanden: {names:?}",
        names.len()
    );

    let mut sorted = names.clone();
    sorted.sort_unstable();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(sorted.len(), before, "doppelte Beispielnamen im Korpus");
}

/// Ein Beispiel, das versehentlich kaputt ist, beweist gar nichts. Also wird
/// zuerst geprüft, dass jedes nicht-bösartige Beispiel wirklich das PDF ist,
/// als das es antritt: ladbar, mit der angekündigten Seitenzahl, und bei
/// `expect_content` mit echten Zeichenoperationen.
#[test]
fn generator_samples_are_the_pdfs_they_claim_to_be() {
    let samples = corpus();
    assert_eq!(samples.len(), CORPUS_SIZE, "siehe the_corpus_is_complete");

    for sample in &samples {
        if sample.is_hostile() {
            continue;
        }
        let doc = redact_pdf::load_from_bytes(&sample.bytes)
            .unwrap_or_else(|e| panic!("{} laedt nicht: {e}", sample.name));
        assert_eq!(
            redact_pdf::page_count(&doc),
            sample.pages,
            "{} hat unerwartete Seitenzahl",
            sample.name
        );
        redact_pdf::validate(&doc)
            .unwrap_or_else(|e| panic!("{} ist strukturell defekt: {e}", sample.name));
        for page in 0..sample.pages {
            let ops = redact_pdf::page_ops(&doc, page).unwrap_or_else(|e| {
                panic!("{} Seite {page}: page_ops schlug fehl: {e}", sample.name)
            });
            if sample.expect_content {
                assert!(
                    !ops.ops.is_empty(),
                    "{} Seite {page}: keine Zeichenoperationen",
                    sample.name
                );
            }
        }
    }
}

/// Jede in der Aufgabenstellung genannte Kategorie ist tatsächlich besetzt.
#[test]
fn every_category_is_populated() {
    let samples = corpus();
    for category in [
        Category::Text,
        Category::Vector,
        Category::Image,
        Category::Structure,
        Category::Hostile,
    ] {
        let count = samples.iter().filter(|s| s.category == category).count();
        assert!(
            count >= 7,
            "Kategorie {} hat nur {count} Beispiele",
            category.as_str()
        );
    }
}

// ---------------------------------------------------------------------------
// 2. Rendern: es gibt immer ein Bild
// ---------------------------------------------------------------------------

/// Kernprüfung. Für jede Seite jedes Beispiels muss ein gültiges Bild
/// entstehen — und Seiten mit Inhalt dürfen nicht weiß sein.
#[test]
fn every_page_renders_to_a_sane_image() {
    let mut lowest: Option<(String, f64)> = None;

    for entry in rendered_corpus() {
        let sample = &entry.sample;
        for (index, page) in entry.pages.iter().enumerate() {
            assert_valid_image(page, sample.name, index);

            if sample.expect_content {
                let ratio = page.non_white_ratio();
                assert!(
                    ratio > sample.min_non_white,
                    "{} Seite {index}: nur {:.4} % nicht-weiße Pixel (Schwelle {:.4} %) — \
                     die Vorschau ist praktisch leer (degraded={}, gezeichnete \
                     Operationen={}, Warnungen={:?})",
                    sample.name,
                    ratio * 100.0,
                    sample.min_non_white * 100.0,
                    page.degraded,
                    page.drawn_ops,
                    page.warnings,
                );
                let label = format!("{} Seite {index}", sample.name);
                if lowest.as_ref().is_none_or(|(_, low)| ratio < *low) {
                    lowest = Some((label, ratio));
                }
            }
        }
    }

    // Die eigentliche Prüfung steckt in der Schleife; hier steht nur noch, wie
    // knapp es im schlimmsten Fall war.
    let (name, ratio) = lowest.expect("kein einziges Beispiel mit erwartetem Inhalt");
    println!(
        "niedrigster Nicht-Weiß-Anteil: {name} = {:.4} %",
        ratio * 100.0
    );
}

/// Laufzeitwächter: kein Beispiel darf die Vorschau blockieren.
///
/// Gemessen wird an den Zeiten aus [`rendered_corpus`] — dem einen Lauf, den
/// sich alle Tests teilen.
#[test]
fn no_sample_exceeds_the_time_budget() {
    let mut slowest = ("", Duration::ZERO);
    for entry in rendered_corpus() {
        assert!(
            entry.elapsed < MAX_SAMPLE_TIME,
            "{} brauchte {:?} fuer {} Seite(n) — Zeitbudget {MAX_SAMPLE_TIME:?}",
            entry.sample.name,
            entry.elapsed,
            entry.pages.len()
        );
        if entry.elapsed > slowest.1 {
            slowest = (entry.sample.name, entry.elapsed);
        }
    }
    println!(
        "langsamstes Beispiel: {} mit {} ms (Budget {} ms)",
        slowest.0,
        slowest.1.as_millis(),
        MAX_SAMPLE_TIME.as_millis()
    );
}

/// Die 0,1-%-Regel ist die eigentliche Zusage dieser Suite. Sie darf nicht
/// stillschweigend aufgeweicht werden, deshalb steht hier schwarz auf weiß,
/// **welche** Beispiele eine niedrigere Schwelle führen — und warum.
///
/// Die Liste ist inzwischen leer. Die einzige Ausnahme war `unicode.pdf` aus
/// dem `lopdf`-Quelltext: eine einzige kurze Textzeile auf einer Letter-Seite,
/// gemessen 0,04 % Farbe. Sie ist mit dem Beispiel entfallen — jedes erzeugte
/// Beispiel ist bewusst so reich bedruckt, dass die Regelschwelle eine Aussage
/// über die Vorschau bleibt und nicht über den Inhalt der Datei. Der Weg,
/// abzuweichen, steht in `Sample::min_non_white` weiterhin offen; wer ihn geht,
/// muss ihn hier eintragen und begründen.
#[test]
fn only_documented_samples_lower_the_threshold() {
    let exceptions: Vec<&str> = corpus()
        .iter()
        .filter(|s| s.expect_content && s.min_non_white < DEFAULT_MIN_NON_WHITE)
        .map(|s| s.name)
        .collect();
    assert_eq!(
        exceptions,
        Vec::<&str>::new(),
        "unerwartete Ausnahme von der 0,1-%-Regel"
    );
}

/// Die beiden Beispiele, die für die entfallenen echten Dateien eingesprungen
/// sind, prüfen wirklich das, wofür sie eingesprungen sind.
///
/// Ein erzeugtes Beispiel hat eine Schwäche, die eine echte Datei nicht hat:
/// man kann es versehentlich so bauen, dass es auch dann bunt ist, wenn der
/// geprüfte Weg gar nicht gegangen wird — dann steht es im Korpus und sagt
/// nichts. Also die Gegenprobe: einmal dasselbe Dokument ohne das eingebettete
/// Fontprogramm, einmal ohne die angehängte Aktualisierung. Beide **müssen**
/// weiß bleiben. Bleiben sie es nicht, kommt die Farbe im Korpusbeispiel
/// woanders her, und das Beispiel ist seinen Platz nicht wert.
#[test]
fn the_new_samples_would_notice_if_the_renderer_stopped() {
    let mut renderer = PageRenderer::new();
    let options = opts();

    for (was, bytes) in [
        (
            "ohne eingebettetes Fontprogramm",
            corpus_gen::text_embedded_truetype_without_the_font(),
        ),
        (
            "ohne die angehängte Aktualisierung",
            corpus_gen::struct_incremental_update_without_the_update(),
        ),
    ] {
        let doc = redact_pdf::load_from_bytes(&bytes)
            .unwrap_or_else(|e| panic!("die Gegenprobe „{was}“ laedt nicht: {e}"));
        let page = renderer.render(&doc, 0, &options);
        assert_valid_image(&page, was, 0);
        let ratio = page.non_white_ratio();
        assert!(
            ratio <= DEFAULT_MIN_NON_WHITE,
            "die Gegenprobe „{was}“ ist mit {:.4} % nicht weiß — das Korpusbeispiel \
             bekommt seine Farbe also nicht von dem Weg, den es prüfen soll",
            ratio * 100.0
        );
    }
}

/// Bösartige Beispiele dürfen `degraded` setzen und Warnungen sammeln — ein
/// gültiges Bild müssen sie trotzdem liefern, und panicken dürfen sie nie.
#[test]
fn hostile_samples_still_produce_an_image() {
    let mut renderer = PageRenderer::new();
    let options = opts();

    for entry in rendered_corpus()
        .iter()
        .filter(|e| e.sample.is_hostile() && !e.pages.is_empty())
    {
        let sample = &entry.sample;
        for (index, page) in entry.pages.iter().enumerate() {
            assert_valid_image(page, sample.name, index);
        }

        // Auch Seitenindizes jenseits des Dokuments müssen ein Bild liefern.
        let Ok(doc) = redact_pdf::load_from_bytes(&sample.bytes) else {
            continue;
        };
        let beyond = entry.pages.len() + 5;
        let page = renderer.render(&doc, beyond, &options);
        assert_valid_image(&page, sample.name, beyond);
    }
}

/// Zweimal rendern muss byteweise dasselbe ergeben — sonst flackert die
/// Vorschau oder es steht uninitialisierter Speicher im Bild.
///
/// Verglichen wird ein frischer Lauf mit dem Ergebnis aus [`rendered_corpus`]:
/// anderer [`PageRenderer`], anderer Font-Cache, anderes geladenes Dokument —
/// gleiche Pixel.
#[test]
fn rendering_is_deterministic() {
    let options = opts();
    for entry in rendered_corpus() {
        let sample = &entry.sample;
        if entry.pages.is_empty() {
            continue;
        }
        let Ok(doc) = redact_pdf::load_from_bytes(&sample.bytes) else {
            continue;
        };
        let mut renderer = PageRenderer::new();
        for (index, first) in entry.pages.iter().enumerate() {
            let again = renderer.render(&doc, index, &options);
            assert_eq!(
                (first.width, first.height, first.rotate, first.degraded),
                (again.width, again.height, again.rotate, again.degraded),
                "{} Seite {index}: Kopfdaten unterscheiden sich zwischen zwei Laeufen",
                sample.name
            );
            assert!(
                first.rgba == again.rgba,
                "{} Seite {index}: Pixel unterscheiden sich zwischen zwei Laeufen",
                sample.name
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Schwärzen und danach wieder rendern
// ---------------------------------------------------------------------------

/// Das ganze Korpus durch Extraktion und Schwärzung — und danach wieder durch
/// den Rasterizer. Beides zusammen ist der Weg, den die GUI geht; keiner der
/// Schritte darf an irgendeinem PDF zerbrechen.
#[test]
fn extraction_and_redaction_survive_the_corpus() {
    // Der zweite schwere Test dieser Datei — und der, der die Zeitmessung in
    // [`rendered_corpus`] verdorben hat. Siehe [`RASTERBANK`].
    let _bank = rasterbank();
    let options = opts();
    let mut roundtrip_failures: Vec<&str> = Vec::new();

    for sample in corpus() {
        let Ok(doc) = redact_pdf::load_from_bytes(&sample.bytes) else {
            continue;
        };
        let pages = redact_pdf::page_count(&doc);
        let boxes = redact_pdf::page_boxes(&doc);

        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut doc = doc;
            let extractor = redact_pdf::PdfExtractor::new();
            for index in 0..pages {
                // Fehler sind erlaubt (kaputte Seite), Paniken nicht.
                let _ = extractor.extract_page(&doc, index);
            }

            // Je Seite ein Balken quer durch die Mitte.
            let redactions: Vec<Redaction> = (0..pages)
                .map(|index| {
                    let page_box = boxes
                        .get(index)
                        .copied()
                        .unwrap_or(Rect::new(0.0, 0.0, 595.0, 842.0));
                    let w = page_box.ur.x - page_box.ll.x;
                    let h = page_box.ur.y - page_box.ll.y;
                    let rect = Rect::new(
                        page_box.ll.x + w * 0.25,
                        page_box.ll.y + h * 0.40,
                        page_box.ll.x + w * 0.75,
                        page_box.ll.y + h * 0.60,
                    );
                    Redaction::new(
                        Region::new(
                            index,
                            rect,
                            None,
                            Source::Manual {
                                reason: "Korpustest".to_string(),
                            },
                        ),
                        Action::Blackout,
                    )
                })
                .collect();

            let redactor = redact_pdf::PdfRedactor::new();
            let _ = redactor.apply_with_report(&mut doc, &redactions);
            doc
        }));

        let doc = match result {
            Ok(doc) => doc,
            Err(_) => panic!("{}: Panik in Extraktion/Schwaerzung", sample.name),
        };

        // Schritt zwei: das geschwärzte Dokument muss weiterhin darstellbar
        // sein — direkt und nach einem Speicher-/Ladezyklus.
        let mut renderer = PageRenderer::new();
        for index in 0..pages.max(1) {
            let page = renderer.render(&doc, index, &options);
            assert_valid_image(&page, sample.name, index);
            // Eine Schwärzung nimmt Text weg und legt ein schwarzes Rechteck
            // darüber. Sie darf eine Seite mit Inhalt niemals leerräumen.
            if sample.expect_content {
                let ratio = page.non_white_ratio();
                assert!(
                    ratio > sample.min_non_white,
                    "{} Seite {index}: nach der Schwaerzung nur noch {:.4} % \
                     nicht-weiße Pixel",
                    sample.name,
                    ratio * 100.0
                );
            }
        }

        let Ok(bytes) = redact_pdf::save_to_bytes(&doc) else {
            roundtrip_failures.push(sample.name);
            continue;
        };
        let Ok(reloaded) = redact_pdf::load_from_bytes(&bytes) else {
            roundtrip_failures.push(sample.name);
            continue;
        };
        let mut renderer = PageRenderer::new();
        for index in 0..redact_pdf::page_count(&reloaded).max(1) {
            let page = renderer.render(&reloaded, index, &options);
            assert_valid_image(&page, sample.name, index);
        }
    }

    // Bösartige Beispiele dürfen den Schreib-/Lesezyklus scheitern lassen —
    // sie sind ja absichtlich defekt. Alles andere muss ihn überstehen.
    let unexpected: Vec<&str> = roundtrip_failures
        .iter()
        .copied()
        .filter(|name| !hostile_names().contains(name))
        .collect();
    assert!(
        unexpected.is_empty(),
        "geschwaerzte Dokumente lassen sich nicht mehr laden: {unexpected:?}"
    );
}

/// Namen aller bewusst kaputten Beispiele.
fn hostile_names() -> Vec<&'static str> {
    corpus()
        .iter()
        .filter(|s| s.is_hostile())
        .map(|s| s.name)
        .collect()
}

// ---------------------------------------------------------------------------
// 4. Übersicht
// ---------------------------------------------------------------------------

/// Druckt die Korpustabelle und prüft dabei die Gesamtbilanz.
///
/// `cargo test -p redact-render -- --nocapture corpus_overview` liefert damit
/// einen lesbaren Bericht über den Zustand der Vorschau.
#[test]
fn corpus_overview() {
    let entries = rendered_corpus();
    let mut total_pages = 0usize;
    let mut degraded_pages = 0usize;
    let mut warned = 0usize;
    let mut lowest: Option<(String, f64)> = None;
    let mut slowest = (String::new(), Duration::ZERO);

    println!();
    println!(
        "{:<34} {:<10} {:>9} {:>6} {:>10} {:>9} {:>8}  Warnungen",
        "Beispiel", "Kategorie", "Bytes", "Seiten", "nicht-weiß", "degraded", "Zeit"
    );
    println!("{}", "-".repeat(114));

    for entry in entries {
        let sample = &entry.sample;
        let pages = &entry.pages;
        let elapsed = entry.elapsed;
        if elapsed > slowest.1 {
            slowest = (sample.name.to_string(), elapsed);
        }
        if pages.is_empty() {
            println!(
                "{:<34} {:<10} {:>9} {:>6} {:>10} {:>9} {:>8}  laedt nicht (bewusst kaputt)",
                sample.name,
                sample.category.as_str(),
                sample.bytes.len(),
                "-",
                "-",
                "-",
                "-"
            );
            continue;
        }

        total_pages += pages.len();
        // Für die Tabelle zählt die schlechteste Seite des Beispiels.
        let ratio = pages
            .iter()
            .map(RenderedPage::non_white_ratio)
            .fold(f64::INFINITY, f64::min);
        let degraded = pages.iter().filter(|p| p.degraded).count();
        degraded_pages += degraded;
        let mut warnings: Vec<&str> = Vec::new();
        for page in pages {
            for w in &page.warnings {
                if !warnings.contains(&w.as_str()) {
                    warnings.push(w.as_str());
                }
            }
        }
        if !warnings.is_empty() {
            warned += 1;
        }

        println!(
            "{:<34} {:<10} {:>9} {:>6} {:>9.3}% {:>9} {:>7} ms  {}",
            sample.name,
            sample.category.as_str(),
            sample.bytes.len(),
            pages.len(),
            ratio * 100.0,
            if degraded > 0 {
                format!("{degraded}/{}", pages.len())
            } else {
                "nein".to_string()
            },
            elapsed.as_millis(),
            summarize(&warnings),
        );

        for (index, page) in pages.iter().enumerate() {
            // Gesamtbilanz 1: nie ein Bild der Größe null.
            assert!(
                page.width > 0 && page.height > 0 && !page.rgba.is_empty(),
                "{} Seite {index}: leeres Bild",
                sample.name
            );
            // Gesamtbilanz 2: Seiten mit Inhalt sind nicht weiß.
            if sample.expect_content {
                let r = page.non_white_ratio();
                assert!(
                    r > sample.min_non_white,
                    "{} Seite {index}: {:.4} % nicht-weiße Pixel",
                    sample.name,
                    r * 100.0
                );
                if lowest.as_ref().is_none_or(|(_, low)| r < *low) {
                    lowest = Some((format!("{} Seite {index}", sample.name), r));
                }
            }
        }
    }

    let (worst, worst_ratio) = lowest.expect("kein Beispiel mit erwartetem Inhalt");
    println!("{}", "-".repeat(114));
    println!(
        "{} Beispiele, {total_pages} Seiten gerendert, 0 Paniken, 0 Bilder der Groesse null.",
        entries.len()
    );
    println!("{degraded_pages} Seite(n) im Notnagel-Modus, {warned} Beispiel(e) mit Warnungen.");
    println!(
        "Schwaechste Seite mit Inhalt: {worst} bei {:.4} % (Regelschwelle {:.1} %).",
        worst_ratio * 100.0,
        DEFAULT_MIN_NON_WHITE * 100.0
    );
    println!(
        "Langsamstes Beispiel: {} mit {} ms (Budget {} ms).",
        slowest.0,
        slowest.1.as_millis(),
        MAX_SAMPLE_TIME.as_millis()
    );

    assert!(total_pages >= entries.len(), "es wurden Seiten verschluckt");
}

/// Kürzt die Warnungsliste auf etwas, das in eine Tabellenzeile passt.
fn summarize(warnings: &[&str]) -> String {
    match warnings.len() {
        0 => "-".to_string(),
        1 => warnings[0].to_string(),
        n => format!("{} (+{} weitere)", warnings[0], n - 1),
    }
}
