//! Prüfrunde 9 — **NaN macht aus einem Rechteck die ganze Seite.**
//!
//! Kindmodul von [`crate::state`], damit `page_boxes`, `slide_onto_page` und
//! die Bilanz unmittelbar erreichbar sind.
//!
//! ## Worum es geht
//!
//! [`AppState::clamp_to_page`] schneidet ein Rechteck auf das Blatt, indem es
//! `max` gegen die untere und `min` gegen die obere Blattkante rechnet. Beide
//! **schlucken** einen NaN-Operanden und geben den anderen zurück. Aus einem
//! Rechteck ohne brauchbare Koordinaten wurde damit der Schnitt „ganzes
//! Blatt“ — gemessen `Some(Rect(0, 0, 595, 842))` für
//! `Rect::new(NaN, NaN, 100, 100)` auf der Demo-Seite.
//!
//! Das ist die gefährliche Richtung der NaN-Familie: nicht „es passiert
//! nichts“, sondern „es wird alles geschwärzt“, ohne dass der Nutzer je ein
//! solches Rechteck gezogen hätte. `add_manual_region` legte es an,
//! quittierte mit „Manuelle Region auf Seite 1 angelegt“, und die Kopfzeile
//! zählte es als „1 werden geschwärzt“.
//!
//! Die Regel steht seit dem vorigen Commit als [`Rect::is_usable`] in
//! `redact-core` und wird hier benutzt, nicht wiederholt.
//!
//! **Sie deckt nur das eine Ende ab.** `is_usable` fragt nach Endlichkeit;
//! `Rect::new(-3,4e38, -3,4e38, 3,4e38, 3,4e38)` ist endlich und damit
//! „brauchbar“. Was daraus folgt und was nicht, steht bei
//! [`ein_riesiges_rechteck_aus_einer_datei_bringt_die_anzeige_nicht_um`].
//!
//! ## Was noch in derselben Bauart steckte
//!
//! * [`AppState::resize_selected`] hielt die Mindestgröße mit `f64::max` —
//!   ein Schritt der Weite NaN schrumpfte einen 100 pt breiten Balken auf
//!   2 pt, still und ohne Meldung.
//! * [`AppState::set_zoom`] klemmte mit `f32::clamp`, und `clamp` reicht NaN
//!   **durch**. Danach waren Vergrößern und Verkleinern gleichzeitig
//!   abgeschaltet.
//!
//! ## Was ausdrücklich unverändert bleibt
//!
//! Das gewöhnliche Beschneiden, die Absage für ein Rechteck neben dem Blatt
//! und vor allem [`AppState::slide_onto_page`]: wer mit den Pfeiltasten gegen
//! den Blattrand schiebt, dessen Balken **schrumpft nicht**. Siehe
//! [`der_balken_am_seitenrand_schrumpft_beim_schieben_nicht`].

use super::*;

// --------------------------------------------------------------- Hilfsmittel

fn iban_only() -> Config {
    Config {
        patterns: vec!["iban_de".to_string()],
        ..Config::default()
    }
}

fn loaded_state() -> AppState {
    let mut state = AppState::with_config(iban_only());
    state
        .load_bytes(
            &redact_pdf::testing::demo_statement(),
            Some(PathBuf::from("demo.pdf")),
        )
        .expect("Demo-PDF ladbar");
    state
}

/// Alle Rechtecke, die aus **einer** unbrauchbaren Koordinate entstehen
/// können — je Ecke einmal NaN, einmal ±∞.
///
/// Eine einzelne reicht: `Rect::from_corners` zieht sie über beide Ecken
/// derselben Achse (siehe `Rect::from_corners` in `redact-core`), und genau
/// deshalb genügte eine, um das ganze Blatt zu erzeugen.
fn unbrauchbare_rechtecke() -> Vec<(&'static str, Rect)> {
    let mut out = Vec::new();
    for (name, wert) in [
        ("NaN", f64::NAN),
        ("+inf", f64::INFINITY),
        ("-inf", f64::NEG_INFINITY),
    ] {
        out.push((name, Rect::new(wert, 100.0, 200.0, 140.0)));
        out.push((name, Rect::new(100.0, wert, 200.0, 140.0)));
        out.push((name, Rect::new(100.0, 100.0, wert, 140.0)));
        out.push((name, Rect::new(100.0, 100.0, 200.0, wert)));
    }
    out
}

// ---------------------------------------------------------------------------
// (1) Der Befund selbst
// ---------------------------------------------------------------------------

/// **Der Befund.** Ein Rechteck mit unbrauchbaren Koordinaten ergab beim
/// Beschneiden das ganze Blatt.
///
/// Festgehalten wird beides: was `clamp_to_page` liefert, und dass es nicht
/// zufällig deshalb `None` ist, weil das Blatt selbst leer wäre — die
/// Gegenprobe mit einem gewöhnlichen Rechteck steht daneben.
#[test]
fn ein_rechteck_ohne_brauchbare_koordinaten_wird_nicht_zum_ganzen_blatt() {
    let state = loaded_state();
    let blatt = state.page_box(0).expect("Demo-PDF hat eine erste Seite");
    let ganzes_blatt = Rect::new(blatt.ll.x, blatt.ll.y, blatt.ur.x, blatt.ur.y);

    for (name, rect) in unbrauchbare_rechtecke() {
        let ergebnis = state.clamp_to_page(0, rect);
        assert_ne!(
            ergebnis,
            Some(ganzes_blatt),
            "{name} in {rect:?} wurde zum ganzen Blatt"
        );
        assert_eq!(
            ergebnis, None,
            "{name} in {rect:?} bezeichnet keinen Bereich der Seite"
        );
    }

    // Die Gegenprobe: das Blatt ist in Ordnung, ein gewöhnliches Rechteck
    // darauf kommt weiterhin durch.
    assert_eq!(
        state.clamp_to_page(0, Rect::new(100.0, 100.0, 200.0, 140.0)),
        Some(Rect::new(100.0, 100.0, 200.0, 140.0))
    );
}

/// Und was der Nutzer davon gesehen hätte: eine manuelle Region über dem
/// ganzen Blatt, die er nie gezogen hat — angelegt, ausgewählt, als „wird
/// geschwärzt“ gezählt und mit „angelegt“ quittiert.
#[test]
fn aus_nan_entsteht_keine_manuelle_region_ueber_dem_ganzen_blatt() {
    let mut state = loaded_state();
    state.regions.clear();

    let index = state.add_manual_region(0, Rect::new(f64::NAN, f64::NAN, 100.0, 100.0), "aus NaN");

    assert_eq!(index, None, "es entstand eine Region: {:?}", state.regions);
    assert!(state.regions.is_empty(), "{:?}", state.regions);
    assert_eq!(state.selected_region, None);
    assert!(
        state.status.contains("außerhalb"),
        "und die Absage steht sofort in der Zeile, nicht erst nach dem Export: {}",
        state.status
    );
    assert_eq!(state.hit_summary().redacted, 0);
    assert_eq!(
        state.hit_summary().headline(),
        "0 Treffer · 0 werden geschwärzt"
    );
}

/// Auch der Zug am Eckgriff kommt nicht daran vorbei: `set_region_rect` lehnt
/// ab und lässt das bisherige Rechteck stehen.
///
/// Der Weg dorthin ist echt — `crate::app::apply_pointer` baut das Rechteck
/// aus `viewer::screen_to_pdf_point`, und was da hereinkommt, hängt an
/// Zoomfaktor und Seitengeometrie.
#[test]
fn ein_zug_auf_unbrauchbare_koordinaten_laesst_das_rechteck_stehen() {
    let mut state = loaded_state();
    state.regions.clear();
    let index = state
        .add_manual_region(0, Rect::new(100.0, 100.0, 200.0, 140.0), "Anschrift")
        .expect("gewöhnliches Rechteck");
    let vorher = state.regions[index].region.rect;

    for (name, rect) in unbrauchbare_rechtecke() {
        assert!(
            !state.set_region_rect(index, rect),
            "{name} in {rect:?} wurde angenommen"
        );
        assert_eq!(
            state.regions[index].region.rect, vorher,
            "{name} in {rect:?} hat das Rechteck verändert"
        );
    }
}

/// Ein Eintrag mit unbrauchbaren Koordinaten wird in der Kopfzeile **nicht**
/// als „wird geschwärzt“ versprochen.
///
/// Der Weg dorthin geht nicht über die Maus, sondern über die Liste: eine
/// Review-Datei oder ein Musterfund aus einer Datei, deren `cm`-Matrix beim
/// Multiplizieren **auf ∞ überläuft**. `redact-pdf` zeichnet ein solches
/// Rechteck nicht, die Zahl in der Kopfzeile darf es also auch nicht
/// versprechen.
///
/// Die Einschränkung „auf ∞“ steht hier ausdrücklich: bleibt dieselbe Matrix
/// knapp darunter (3,4e38), ist das Rechteck endlich, `is_usable` sagt ja, und
/// die Kopfzeile verspricht zu Recht eine Schwärzung — sie überdeckt das Blatt
/// wirklich. Siehe
/// [`ein_riesiges_rechteck_aus_einer_datei_bringt_die_anzeige_nicht_um`].
#[test]
fn ein_unbrauchbarer_eintrag_zaehlt_nicht_als_geschwaerzt() {
    let mut state = loaded_state();
    state.regions.clear();
    let region = Region::new(
        0,
        Rect::new(f64::NAN, 700.0, f64::NAN, 715.0),
        Some("DE89 3704 0044 0532 0130 00".into()),
        Source::Pattern {
            pattern_id: "iban_de".into(),
            confidence: 0.99,
        },
    );
    let annotiert = state.annotate(region.clone());
    state.regions.push(annotiert);

    assert!(
        state.is_off_page(&region),
        "ein Rechteck ohne brauchbare Koordinaten liegt auf keiner Seite"
    );
    let summary = state.hit_summary();
    assert_eq!(summary.redacted, 0, "{summary:?}");
    assert_eq!(summary.outcomes, vec![HitOutcome::OffPage], "{summary:?}");
    assert!(
        summary.headline().contains("0 werden geschwärzt"),
        "die Kopfzeile verspricht etwas: {}",
        summary.headline()
    );
}

// ---------------------------------------------------------------------------
// (2) Dieselbe Bauart an zwei weiteren Stellen in `state.rs`
// ---------------------------------------------------------------------------

/// **Die Mindestgröße war der zweite Einstieg.**
///
/// `(rect.ur.x + dx).max(rect.ll.x + MIN_REGION_EXTENT)` — mit `dx = NaN`
/// schluckte `max` die linke Seite und gab die rechte zurück. Aus einem 100 pt
/// breiten Balken wurden 2 pt, ohne Fehler und ohne Meldung; darunter käme der
/// überdeckte Text wieder hervor.
#[test]
fn ein_schritt_ohne_endliche_weite_schrumpft_den_balken_nicht() {
    let mut state = loaded_state();
    state.regions.clear();
    let index = state
        .add_manual_region(0, Rect::new(100.0, 100.0, 200.0, 140.0), "IBAN")
        .expect("gewöhnliches Rechteck");
    state.selected_region = Some(index);
    let vorher = state.regions[index].region.rect;

    for weite in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(!state.resize_selected(weite, 0.0), "{weite} als dx");
        assert!(!state.resize_selected(0.0, weite), "{weite} als dy");
        assert_eq!(
            state.regions[index].region.rect, vorher,
            "{weite} hat das Rechteck verändert"
        );
    }

    // Gegenprobe: ein gewöhnlicher Schritt ändert es weiterhin.
    assert!(state.resize_selected(10.0, 0.0));
    assert_eq!(state.regions[index].region.rect.ur.x, vorher.ur.x + 10.0);
}

/// **`f32::clamp` klemmt NaN nicht weg, es reicht ihn durch.**
///
/// Danach war der Zoom nicht bloß falsch, sondern verriegelt: `can_zoom_in`
/// und `can_zoom_out` waren beide `false`, weil jeder Vergleich mit NaN falsch
/// ist. Vergrößern und Verkleinern gleichzeitig aus, und `NaN * ZOOM_STEP`
/// bleibt NaN.
#[test]
fn ein_unbrauchbarer_zoomwert_verriegelt_die_anzeige_nicht() {
    let mut state = loaded_state();
    let vorher = state.zoom;

    for wert in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        state.set_zoom(wert);
        assert_eq!(state.zoom, vorher, "{wert} kam durch");
        assert!(state.zoom.is_finite());
        assert!(
            state.can_zoom_in() || state.can_zoom_out(),
            "der Zoom ist verriegelt: {}",
            state.zoom
        );
    }

    // Gegenprobe: die Grenzen und die gewöhnlichen Schritte gelten weiter.
    state.set_zoom(100.0);
    assert_eq!(state.zoom, MAX_ZOOM);
    state.set_zoom(0.0);
    assert_eq!(state.zoom, MIN_ZOOM);
    state.set_zoom(1.5);
    assert_eq!(state.zoom, 1.5);
}

// ---------------------------------------------------------------------------
// (3) Gegenprobe: was unverändert bleiben muss
// ---------------------------------------------------------------------------

/// Das gewöhnliche Beschneiden — halb daneben wird abgeschnitten, ganz daneben
/// ergibt `None`, ganz drauf bleibt unangetastet.
#[test]
fn gewoehnliche_rechtecke_werden_weiterhin_genauso_beschnitten() {
    let state = loaded_state();
    let blatt = state.page_box(0).expect("erste Seite");

    // Ganz auf dem Blatt: unverändert.
    let drauf = Rect::new(70.0, 700.0, 250.0, 715.0);
    assert_eq!(state.clamp_to_page(0, drauf), Some(drauf));

    // Halb daneben: am Blattrand ist Schluss.
    let halb = Rect::new(blatt.ur.x - 40.0, 400.0, blatt.ur.x + 100.0, 420.0);
    assert_eq!(
        state.clamp_to_page(0, halb),
        Some(Rect::new(blatt.ur.x - 40.0, 400.0, blatt.ur.x, 420.0))
    );

    // Ganz daneben: nichts.
    assert_eq!(
        state.clamp_to_page(0, Rect::new(700.0, 400.0, 760.0, 420.0)),
        None
    );

    // Ohne geladenes Dokument wird weiterhin nichts beschnitten.
    let leer = AppState::new();
    let weit_weg = Rect::new(5000.0, 5000.0, 5100.0, 5100.0);
    assert_eq!(leer.clamp_to_page(0, weit_weg), Some(weit_weg));
}

/// **Die wichtigste Gegenprobe.** Wer mit den Pfeiltasten gegen den Blattrand
/// schiebt, dessen Balken schrumpft nicht — er stößt an und bleibt ganz.
///
/// Das ist die Zusage aus [`AppState::slide_onto_page`], und sie ist genau
/// das, was eine zu eifrige Prüfung in `clamp_to_page` kaputtmachen würde.
#[test]
fn der_balken_am_seitenrand_schrumpft_beim_schieben_nicht() {
    let mut state = loaded_state();
    state.regions.clear();
    let breite = 180.0;
    let index = state
        .add_manual_region(0, Rect::new(60.0, 700.0, 60.0 + breite, 715.0), "IBAN")
        .expect("gewöhnliches Rechteck");
    state.selected_region = Some(index);

    // Hundert Anschläge nach links — weit über die linke Blattkante hinaus.
    for _ in 0..100 {
        assert!(state.move_selected(-10.0, 0.0));
    }
    let rect = state.regions[index].region.rect;
    assert_eq!(rect.width(), breite, "der Balken ist geschrumpft: {rect:?}");
    assert_eq!(rect.ll.x, 0.0, "er steht nicht an der Blattkante: {rect:?}");

    // Und dasselbe nach rechts.
    let blatt = state.page_box(0).expect("erste Seite");
    for _ in 0..100 {
        assert!(state.move_selected(10.0, 0.0));
    }
    let rect = state.regions[index].region.rect;
    assert_eq!(rect.width(), breite, "der Balken ist geschrumpft: {rect:?}");
    assert_eq!(
        rect.ur.x, blatt.ur.x,
        "er steht nicht an der Kante: {rect:?}"
    );
}

/// Dieselbe Datei, nur mit einer selbst gesetzten MediaBox.
///
/// Über `lopdf` und nicht über einen neuen Helfer in `redact-pdf`: dort ist
/// diese Runde ein anderer Zuständigkeitsbereich, und drei Zeilen rechtfertigen
/// keine fremde Datei.
fn pdf_mit_mediabox(werte: Vec<lopdf::Object>) -> Vec<u8> {
    let bytes = redact_pdf::testing::minimal_pdf("DE89 3704 0044 0532 0130 00");
    let mut doc = lopdf::Document::load_mem(&bytes).expect("Testdatei ladbar");
    let seiten: Vec<lopdf::ObjectId> = doc.get_pages().values().copied().collect();
    for id in seiten {
        doc.get_object_mut(id)
            .and_then(|o| o.as_dict_mut())
            .expect("Seitenwörterbuch")
            .set("MediaBox", lopdf::Object::Array(werte.clone()));
    }
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    out
}

/// Die Begründung dafür, dass [`AppState::clamp_to_page`] das **Blatt** nicht
/// prüft: es kann gar nicht unbrauchbar sein.
///
/// `load_bytes` füllt `page_boxes` aus [`redact_pdf::document::sane_page_boxes`],
/// und das ersetzt jede nicht endliche oder absurd bemessene MediaBox durch
/// A4. Geprüft wird das an einer Datei, die die Grenze wirklich reißt — auf der
/// gewöhnlichen Demo-Seite wäre dieser Test wertlos, weil deren MediaBox
/// ohnehin in Ordnung ist.
///
/// Fällt die Zusage, fällt dieser Test, und nicht still das Beschneiden.
///
/// # Was jeder der drei Fälle festhält
///
/// Hier stand, alle drei rissen [`redact_pdf::document::sane_box`], „Nullgröße
/// und negative Kanten unter `MIN_PAGE_EXTENT`“. Das stimmt für den dritten
/// nicht, und der Test hielt ihn auch nicht fest: gemessen wird `[0 0 -595
/// -842]` **nicht** geheilt, weil `sane_box` zuerst `normalized()` rechnet und
/// danach eine gewöhnliche 595 x 842 pt große Seite dasteht — sie liegt bloß
/// im negativen Viertel. Geprüft wird deshalb je Fall das, was wirklich
/// herauskommt:
///
/// | Fall | `healed_pages` | `page_boxes[0]` |
/// |---|---|---|
/// | absurd groß (10 Mio. pt > `MAX_PAGE_EXTENT`) | `[0]` | A4 |
/// | Nullgröße (Kante 0 < `MIN_PAGE_EXTENT`) | `[0]` | A4 |
/// | negativ (nach `normalized()` 595 x 842) | `[]` | `Rect(-595, -842, 0, 0)` |
///
/// Der dritte Fall ist damit die **Gegenprobe** und nicht bloß ein dritter
/// Durchlauf: er hält fest, dass die Heilung nicht zu weit greift. Ein noch
/// größerer Wert (1e30) wäre der schärfere erste Fall, kommt aber durch
/// `lopdf`s eigenen Schreib-/Lesegang nicht unbeschadet zurück (die Datei hat
/// danach null Seiten) — das ist eine Grenze des Testwegs, nicht der geprüften
/// Regel.
#[test]
fn jedes_geladene_blatt_ist_brauchbar() {
    // Die gewöhnliche Datei zuerst — sie muss unverändert durchkommen.
    let state = loaded_state();
    assert_eq!(state.page_count(), 2);
    assert_eq!(state.page_boxes[0], Rect::new(0.0, 0.0, 595.0, 842.0));
    assert!(state.healed_pages.is_empty(), "{:?}", state.healed_pages);

    let a4 = redact_pdf::document::A4;
    for (name, werte, geheilt, erwartetes_blatt) in [
        (
            "absurd groß",
            vec![0.into(), 0.into(), 10_000_000.into(), 10_000_000.into()],
            true,
            a4,
        ),
        (
            "Nullgröße",
            vec![0.into(), 0.into(), 0.into(), 0.into()],
            true,
            a4,
        ),
        (
            "negativ",
            vec![0.into(), 0.into(), (-595).into(), (-842).into()],
            false,
            Rect::new(-595.0, -842.0, 0.0, 0.0),
        ),
    ] {
        let mut state = AppState::with_config(iban_only());
        state
            .load_bytes(&pdf_mit_mediabox(werte), Some(PathBuf::from("krumm.pdf")))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(state.page_count(), 1, "{name}");

        // 1. Geheilt oder nicht — und zwar genau bei den beiden, die die
        //    Grenzen von `sane_box` wirklich reißen.
        assert_eq!(
            state.healed_pages,
            if geheilt { vec![0] } else { Vec::new() },
            "{name}: healed_pages"
        );
        assert_eq!(state.page_boxes[0], erwartetes_blatt, "{name}: Blatt");

        // 2. Die Heilung wird **angesagt**. Ohne die Warnung sähe die Seite
        //    aus wie jede andere; das ist der wichtigere Teil der Heilung
        //    (siehe `healed_page_warning`).
        let angesagt = state.warnings.iter().any(|w| w == &healed_page_warning(0));
        assert_eq!(angesagt, geheilt, "{name}: Warnung {:?}", state.warnings);

        // 3. Egal wie: das Blatt, mit dem weitergerechnet wird, ist brauchbar.
        let blatt = state.page_box(0).expect("erste Seite");
        assert!(blatt.is_usable(), "{name}: {blatt:?}");
        assert!(
            blatt.width() > 0.0 && blatt.height() > 0.0,
            "{name}: {blatt:?}"
        );

        // 4. Und die Folge davon: auf so einem Blatt lässt sich weiterhin
        //    gewöhnlich arbeiten — die Heilung darf nicht alles ablehnen.
        //
        //    Gemessen **relativ zum Blatt**, nicht an festen Koordinaten: auf
        //    der negativen MediaBox läge ein Rechteck bei (70, 700) wirklich
        //    daneben.
        let drauf = Rect::new(
            blatt.ll.x + 10.0,
            blatt.ll.y + 10.0,
            blatt.ll.x + 190.0,
            blatt.ll.y + 25.0,
        );
        assert_eq!(state.clamp_to_page(0, drauf), Some(drauf), "{name}");
    }
}

// ---------------------------------------------------------------------------
// (4) Die Zusicherung, auf die sich das Zurückrechnen stützt
// ---------------------------------------------------------------------------

/// **`set_zoom` ist kein Wächter, sondern eine Bitte.**
///
/// [`AppState::zoom`] ist ein öffentliches Feld. Wer es unmittelbar
/// beschreibt, kommt an jeder Prüfung in `set_zoom` vorbei — und die Folge
/// stand nicht im Zoom, sondern in den **Regionskoordinaten**:
/// `viewer::screen_to_pdf_point` klemmte den Zoom mit
/// `(zoom as f64).max(f64::EPSILON)`, und `f64::max` schluckt NaN. NaN, 0 und
/// jeder negative Wert wurden damit gleichermaßen zu 2,2e-16; ein Zug über
/// 120 x 60 Bildschirmpunkte von der linken oberen Blattecke aus ergab
/// gemessen `Rect(0, -2,7e17, 5,4e17, 842)`, `is_usable` sagte ja,
/// `clamp_to_page` schnitt es auf das ganze Blatt, und heraus kam eine
/// manuelle Region über der ganzen Seite mit „1 Treffer · 1 werden
/// geschwärzt“ — genau die Wirkung, die die NaN-Runde abgestellt hatte.
#[test]
fn ein_zug_mit_unbrauchbarem_zoom_legt_keine_region_ueber_das_ganze_blatt() {
    use crate::viewer::screen_to_pdf;
    use egui::Pos2;

    let mut state = loaded_state();
    let view = state.page_view(0);
    let origin = Pos2::new(8.0, 8.0);

    for wert in [f32::NAN, 0.0, -1.0, f32::INFINITY, MIN_ZOOM / 2.0] {
        state.regions.clear();
        // Am Feld vorbei — genau das, was `set_zoom` nicht verhindern kann.
        state.zoom = wert;
        let gezogen = screen_to_pdf(
            origin,
            Pos2::new(origin.x + 120.0, origin.y + 60.0),
            &view,
            state.zoom,
            origin,
        );
        assert!(
            !gezogen.is_usable(),
            "{wert} ergab ein brauchbar aussehendes Rechteck: {gezogen:?}"
        );
        assert_eq!(
            state.add_manual_region(0, gezogen, "gezogen"),
            None,
            "{wert} legte eine Region an: {:?}",
            state.regions
        );
        assert!(state.regions.is_empty(), "{wert}: {:?}", state.regions);
        assert!(
            state.status.contains("außerhalb"),
            "{wert}: {}",
            state.status
        );
    }
}

/// Die Gegenprobe dazu, und sie ist die wichtigere: **jeder Zoom, den das
/// Programm einstellen kann, rechnet weiterhin genau zurück.**
///
/// Eine Grenze, die den gewöhnlichen Fall abweist, wäre derselbe Fehler in der
/// anderen Richtung. Geprüft werden die beiden Enden des erlaubten Bereichs,
/// die Originalgröße und das, was [`crate::viewer::fit_zoom`] liefert — also
/// alles, was `set_zoom` je in das Feld schreibt.
#[test]
fn jeder_einstellbare_zoom_rechnet_weiterhin_genau_zurueck() {
    use crate::viewer::{fit_zoom, pdf_to_screen, screen_to_pdf, usable_zoom};
    use egui::{Pos2, Vec2};

    let state = loaded_state();
    let view = state.page_view(0);
    let origin = Pos2::new(8.0, 8.0);
    let eingepasst = fit_zoom(Vec2::new(900.0, 600.0), &view);

    for wert in [MIN_ZOOM, 0.5, 1.0, 2.5, MAX_ZOOM, eingepasst] {
        assert_eq!(usable_zoom(wert), Some(wert as f64), "{wert}");
        let rect = Rect::new(70.0, 700.0, 250.0, 715.0);
        let auf_dem_schirm = pdf_to_screen(&rect, &view, wert, origin);
        let zurueck = screen_to_pdf(auf_dem_schirm.min, auf_dem_schirm.max, &view, wert, origin);
        assert!(zurueck.is_usable(), "{wert}: {zurueck:?}");
        for (a, b) in [
            (zurueck.ll.x, rect.ll.x),
            (zurueck.ll.y, rect.ll.y),
            (zurueck.ur.x, rect.ur.x),
            (zurueck.ur.y, rect.ur.y),
        ] {
            assert!((a - b).abs() < 0.5, "Zoom {wert}: {a} != {b}");
        }
    }

    // Und ein gewöhnlicher Zug legt weiterhin eine Region an.
    let mut state = state;
    state.set_zoom(MIN_ZOOM);
    let gezogen = screen_to_pdf(
        Pos2::new(origin.x + 20.0, origin.y + 20.0),
        Pos2::new(origin.x + 60.0, origin.y + 40.0),
        &view,
        state.zoom,
        origin,
    );
    assert!(state.add_manual_region(0, gezogen, "gezogen").is_some());
}

// ---------------------------------------------------------------------------
// (5) Endlich, aber riesig — das andere Ende derselben Familie
// ---------------------------------------------------------------------------

/// **`is_usable` deckt nur das ∞-Ende ab, und das genügt hier auch.**
///
/// `Rect::new(-3,4e38, -3,4e38, 3,4e38, 3,4e38)` ist endlich. `clamp_to_page`
/// liefert dafür `Some(Rect(0, 0, 595, 842))`, die Kopfzeile sagt „1 Treffer ·
/// 1 werden geschwärzt“, und das ist **richtig**: ein so großes Rechteck
/// überdeckt das Blatt wirklich, und die Datei hat genau das verlangt.
///
/// # Woher so ein Rechteck kommt — und woher nicht
///
/// Nicht von der Maus: mit einem Zoom aus dem erlaubten Bereich und
/// Bildschirmkoordinaten bis ±1e6 liegt die größte erreichbare Koordinate bei
/// 4,0e6 (gemessen; siehe
/// [`jeder_einstellbare_zoom_rechnet_weiterhin_genau_zurueck`] für die
/// Zusicherung, an der diese Schranke hängt). Nicht von der Tastatur:
/// `move_selected` stößt am Blattrand an und `set_region_rect` beschneidet —
/// nach 20 000 Anschlägen mit je 1000 pt stand das Rechteck gemessen bei
/// `Rect(100, 100, 595, 842)`, also auf dem Blatt. Es kommt aus einer
/// **Datei**: eine Review-Datei oder `--manual-regions` ist eine Liste von
/// Koordinaten, und [`AppState::apply_review_file`] legt sie so ab, wie sie
/// dort stehen.
///
/// # Wo es weh tat
///
/// Nicht in der Rechnung, sondern beim **Zeichnen**. Der Rahmen einer Region
/// wird aus dem ungeschnittenen Rechteck gezeichnet, und
/// `epaint::Shape::dashed_line` legt je Strich einen eigenen `Shape` an.
/// Gemessen forderte `viewer::paint_padding` dafür 5 368 709 120 Byte an und
/// der Prozess brach mit SIGABRT ab. Die Decke dagegen steht in
/// [`crate::viewer::MAX_DASHES_PER_EDGE`] — sie zählt Striche, nicht Punkte.
#[test]
fn ein_riesiges_rechteck_aus_einer_datei_bringt_die_anzeige_nicht_um() {
    use crate::viewer::{paint_padding, paint_region, pdf_to_screen, RegionStyle};
    use egui::Pos2;

    let mut state = loaded_state();
    state.regions.clear();
    state
        .add_manual_region(0, Rect::new(100.0, 100.0, 200.0, 140.0), "Anschrift")
        .expect("gewöhnliches Rechteck");

    // Der echte Weg: eine Review-Datei zu genau diesem Dokument, in der jemand
    // die Koordinaten durch 3,4e38 ersetzt hat.
    let mut datei = state.to_review_file();
    let riesig = Rect::new(-3.4e38, -3.4e38, 3.4e38, 3.4e38);
    datei.items[0].region.rect = riesig;
    state.apply_review_file(datei).expect("Prüfsumme stimmt");

    // Die Bilanz sagt die Wahrheit: das Rechteck überdeckt das Blatt.
    assert!(riesig.is_usable());
    assert_eq!(
        state.clamp_to_page(0, riesig),
        Some(Rect::new(0.0, 0.0, 595.0, 842.0))
    );
    assert_eq!(state.hit_summary().redacted, 1);

    // Und die Anzeige übersteht es. Gezählt werden die erzeugten Shapes: ohne
    // Decke wächst diese Liste, bis der Speicher ausgeht.
    let view = state.page_view(0);
    let origin = Pos2::new(8.0, 8.0);
    let ctx = egui::Context::default();
    let ausgabe = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            for eintrag in &state.regions {
                let schirm = pdf_to_screen(&eintrag.region.rect, &view, 1.0, origin);
                paint_padding(ui.painter(), schirm, (0, 0, 0), 1.0);
                paint_region(ui.painter(), schirm, (0, 0, 0), RegionStyle::Redacted, true);
                paint_region(
                    ui.painter(),
                    schirm,
                    (0, 0, 0),
                    RegionStyle::Discarded,
                    false,
                );
            }
        });
    });
    assert!(
        ausgabe.shapes.len() < 10_000,
        "die Strichliste läuft davon: {} Shapes",
        ausgabe.shapes.len()
    );
}

/// Die Rechnung hinter der Decke, ohne Fenster — und die Gegenprobe, dass sie
/// für alles Gewöhnliche **nichts** ändert.
#[test]
fn die_strichlaenge_waechst_erst_jenseits_jedes_bildschirms() {
    use crate::viewer::{dash_lengths_for, MAX_DASHES_PER_EDGE, PADDING_DASH, PADDING_GAP};
    use egui::{Pos2, Rect as ERect};

    let von_bis = |breite: f32| {
        ERect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(breite, breite.min(2000.0)))
    };

    // A4 bei Originalgröße, dieselbe Seite bei vierfacher Vergrößerung und
    // eine Kante über die volle Breite der breitesten gebräuchlichen Anzeige:
    // alles unverändert.
    for breite in [595.0_f32, 842.0 * 4.0, 7680.0] {
        assert_eq!(
            dash_lengths_for(von_bis(breite), PADDING_DASH, PADDING_GAP),
            Some((PADDING_DASH, PADDING_GAP)),
            "{breite} pt wurden angetastet"
        );
    }

    // Darüber wachsen Strich und Lücke im gleichen Verhältnis, und die Zahl
    // der Striche bleibt unter der Decke.
    for breite in [1.0e5_f32, 1.0e7, 1.0e30] {
        let (strich, luecke) = dash_lengths_for(von_bis(breite), PADDING_DASH, PADDING_GAP)
            .unwrap_or_else(|| panic!("{breite} wurde abgelehnt"));
        assert!(
            (strich / luecke - PADDING_DASH / PADDING_GAP).abs() < 1e-3,
            "{breite}: Verhältnis verschoben ({strich} : {luecke})"
        );
        let striche = breite / (strich + luecke);
        assert!(
            striche <= MAX_DASHES_PER_EDGE + 1.0,
            "{breite}: {striche} Striche je Kante"
        );
    }

    // Und was sich nicht ausrechnen lässt, wird nicht gezeichnet: 3,4e38 minus
    // -3,4e38 läuft in `f32` auf ∞ über, NaN ohnehin.
    for kaputt in [
        ERect::from_min_max(Pos2::new(-3.4e38, -3.4e38), Pos2::new(3.4e38, 3.4e38)),
        ERect::from_min_max(Pos2::new(f32::NAN, 0.0), Pos2::new(10.0, 10.0)),
        ERect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(f32::INFINITY, 10.0)),
    ] {
        assert_eq!(
            dash_lengths_for(kaputt, PADDING_DASH, PADDING_GAP),
            None,
            "{kaputt:?} kam durch"
        );
    }
}
