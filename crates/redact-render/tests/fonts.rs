//! Integrationstests für die Glyph-Umrisse.
//!
//! Die Testfixtures in `tests/data/` sind aus demselben Liberation-Subset
//! abgeleitet wie die mitgelieferten Ersatzfonts (SIL OFL 1.1):
//!
//! * `sample-cff.otf`  — OpenType mit `CFF `-Umrissen (kubisch)
//! * `sample-bare.cff` — dieselbe `CFF `-Tabelle nackt, wie sie in
//!   `/FontFile3` mit `/Subtype /Type1C` steht

use redact_render::{FontCache, GlyphFont, GlyphKey, Outline, Seg};

const SAMPLE_OTF: &[u8] = include_bytes!("data/sample-cff.otf");
const SAMPLE_BARE_CFF: &[u8] = include_bytes!("data/sample-bare.cff");

fn outline_of(font: &GlyphFont, ch: char) -> Outline {
    font.outline(GlyphKey::Char(ch))
        .unwrap_or_else(|| panic!("kein Umriss für {ch:?}"))
}

fn has_quad(outline: &Outline) -> bool {
    outline
        .segments
        .iter()
        .any(|s| matches!(s, Seg::QuadTo(..)))
}

fn has_cubic(outline: &Outline) -> bool {
    outline
        .segments
        .iter()
        .any(|s| matches!(s, Seg::CubicTo(..)))
}

/// Einfacher Xorshift — reicht als deterministische Müllquelle, dafür braucht
/// es keine zusätzliche Abhängigkeit.
struct Xorshift(u64);

impl Xorshift {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let bytes = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }
}

// ---------------------------------------------------------------------------
// Ersatzfont
// ---------------------------------------------------------------------------

#[test]
fn fallback_yields_outlines_for_letters_but_not_for_space() {
    let font = GlyphFont::fallback(false, false, false, false);
    for ch in ['A', 'g', 'Ä'] {
        let outline = outline_of(&font, ch);
        assert!(!outline.segments.is_empty(), "{ch:?} hat keine Segmente");
        assert_eq!(outline.units_per_em, font.units_per_em());
    }
    assert_eq!(font.outline(GlyphKey::Char(' ')), None);
    // Ein Zeichen, das im Subset garantiert fehlt.
    assert_eq!(font.outline(GlyphKey::Char('漢')), None);
}

#[test]
fn fallback_variants_are_really_different_faces() {
    let sans = GlyphFont::fallback(false, false, false, false);
    let serif = GlyphFont::fallback(true, false, false, false);
    let bold = GlyphFont::fallback(false, true, false, false);
    let italic = GlyphFont::fallback(false, false, true, false);
    let mono = GlyphFont::fallback(false, false, false, true);

    let base = outline_of(&sans, 'A');
    for (name, other) in [
        ("serif", &serif),
        ("bold", &bold),
        ("italic", &italic),
        ("mono", &mono),
    ] {
        let outline = outline_of(other, 'A');
        assert_ne!(outline, base, "{name} liefert denselben Umriss wie sans");
    }

    // Mono ist tatsächlich dicktengleich, die Proportionalschnitte nicht.
    let mono_i = mono.advance(GlyphKey::Char('i')).expect("Vorschub i");
    let mono_m = mono.advance(GlyphKey::Char('m')).expect("Vorschub m");
    assert_eq!(mono_i, mono_m);
    let sans_i = sans.advance(GlyphKey::Char('i')).expect("Vorschub i");
    let sans_m = sans.advance(GlyphKey::Char('m')).expect("Vorschub m");
    assert!(sans_i < sans_m);

    // Fett ist breiter als normal.
    assert!(bold.advance(GlyphKey::Char('A')) > sans.advance(GlyphKey::Char('A')));
}

#[test]
fn capital_a_bounding_box_is_plausible_in_font_units() {
    for (serif, bold, italic, mono) in [
        (false, false, false, false),
        (true, false, false, false),
        (false, true, true, false),
        (false, false, false, true),
    ] {
        let font = GlyphFont::fallback(serif, bold, italic, mono);
        let upem = font.units_per_em();
        assert!(upem >= 16.0, "unplausible Em-Größe {upem}");
        let outline = outline_of(&font, 'A');
        let (x0, y0, x1, y1) = outline.bounds().expect("Bounding-Box");

        // Grundlinie unten, Versalhöhe zwischen 0.5 und 1.0 em.
        assert!(y0.abs() < 0.05 * upem, "Grundlinie verschoben: {y0}");
        let height = (y1 - y0) / upem;
        assert!(
            (0.5..=1.0).contains(&height),
            "Versalhöhe {height} em ist unplausibel"
        );
        let width = (x1 - x0) / upem;
        assert!((0.3..=1.2).contains(&width), "Breite {width} em");
    }
}

#[test]
fn fallback_reports_glyph_count_and_units_per_em() {
    let font = GlyphFont::fallback(false, false, false, false);
    assert!(font.glyph_count() > 0);
    assert_eq!(font.units_per_em(), 2048.0);
    // Die Glyph-ID aus der cmap muss innerhalb der Glyphenzahl liegen.
    assert_eq!(font.outline(GlyphKey::Gid(font.glyph_count())), None);
}

#[test]
fn composite_glyph_yields_multiple_contours() {
    let font = GlyphFont::fallback(false, false, false, false);
    let a = outline_of(&font, 'A');
    let a_umlaut = outline_of(&font, 'Ä');
    // "Ä" ist im Font aus "A" plus Trema zusammengesetzt: mehr Konturen als "A".
    assert!(
        a_umlaut.contour_count() > a.contour_count(),
        "Ä hat {} Konturen, A hat {}",
        a_umlaut.contour_count(),
        a.contour_count()
    );
    assert!(a_umlaut.contour_count() >= 3);
    // Und es steht höher als das nackte A.
    assert!(a_umlaut.bounds().unwrap().3 > a.bounds().unwrap().3);
}

// ---------------------------------------------------------------------------
// Formate: TrueType (quadratisch) vs. CFF (kubisch)
// ---------------------------------------------------------------------------

#[test]
fn truetype_outlines_use_quadratic_segments() {
    let font = GlyphFont::fallback(false, false, false, false);
    let outline = outline_of(&font, 'o');
    assert!(has_quad(&outline), "TrueType ohne QuadTo");
    assert!(!has_cubic(&outline), "TrueType mit CubicTo");
}

#[test]
fn opentype_cff_outlines_use_cubic_segments() {
    let font = GlyphFont::from_bytes(SAMPLE_OTF).expect("OTF ladbar");
    assert!(font.glyph_count() > 0);
    let outline = outline_of(&font, 'o');
    assert!(has_cubic(&outline), "CFF ohne CubicTo");
    assert!(!has_quad(&outline), "CFF mit QuadTo");
    assert_eq!(outline.units_per_em, font.units_per_em());
}

#[test]
fn bare_cff_is_wrapped_into_an_opentype_container() {
    // /FontFile3 mit /Subtype /Type1C liefert genau diese nackte Tabelle.
    let font = GlyphFont::from_bytes(SAMPLE_BARE_CFF).expect("bare CFF ladbar");
    let wrapped = GlyphFont::from_bytes(SAMPLE_OTF).expect("OTF ladbar");
    assert_eq!(font.glyph_count(), wrapped.glyph_count());
    assert_eq!(font.units_per_em(), wrapped.units_per_em());

    // Ohne cmap im nackten CFF geht nur der Zugriff über die Glyph-ID.
    assert_eq!(font.outline(GlyphKey::Char('o')), None);
    let gid = (0..font.glyph_count())
        .find(|gid| {
            wrapped.outline(GlyphKey::Gid(*gid)) == wrapped.outline(GlyphKey::Char('o'))
                && wrapped.outline(GlyphKey::Gid(*gid)).is_some()
        })
        .expect("Glyph-ID für 'o'");
    let outline = font.outline(GlyphKey::Gid(gid)).expect("Umriss über GID");
    assert!(has_cubic(&outline));
    assert_eq!(outline, wrapped.outline(GlyphKey::Gid(gid)).unwrap());
    // Der Wrapper hat keine echten Vorschübe.
    assert_eq!(font.advance(GlyphKey::Gid(gid)), None);
}

#[test]
fn type1_fontfile_is_rejected_and_falls_back() {
    // PFB: 0x80 0x01 gefolgt von der Segmentlänge, dann PostScript-Text.
    let mut pfb = vec![0x80, 0x01, 0x40, 0x00, 0x00, 0x00];
    pfb.extend_from_slice(b"%!PS-AdobeFont-1.0: TestFont 001.000\n/FontName /Test def\n");
    assert!(
        GlyphFont::from_bytes(&pfb).is_none(),
        "Type1 gilt als ladbar"
    );

    // PFA (reiner Text) ebenfalls nicht.
    let pfa = b"%!PS-AdobeFont-1.0: TestFont\n11 dict begin\n".to_vec();
    assert!(GlyphFont::from_bytes(&pfa).is_none());

    // Der Cache weicht dann auf den Ersatzfont aus.
    let mut cache = FontCache::new();
    let font = cache.get_or_load("Type1", Some(&pfb), true, false, false, false);
    assert!(font.glyph_count() > 0);
    assert!(font.outline(GlyphKey::Char('A')).is_some());
}

// ---------------------------------------------------------------------------
// Robustheit
// ---------------------------------------------------------------------------

#[test]
fn garbage_input_never_panics() {
    let mut rng = Xorshift(0x2b7e_1516_28ae_d2a6);
    for seed in 0..64u64 {
        let mut rng2 = Xorshift(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        for len in [0usize, 1, 4, 12, 64, 333, 4096] {
            let mut buf = vec![0u8; len];
            rng2.fill(&mut buf);
            let _ = GlyphFont::from_bytes(&buf);
        }
        // Müll, der wie ein sfnt- bzw. CFF-Kopf aussieht: die interessanteren Pfade.
        for header in [
            b"OTTO".as_slice(),
            b"\x00\x01\x00\x00".as_slice(),
            b"ttcf".as_slice(),
            b"true".as_slice(),
            &[1u8, 0, 4, 2],
        ] {
            let mut buf = header.to_vec();
            buf.resize(256, 0);
            rng.fill(&mut buf[4..]);
            let font = GlyphFont::from_bytes(&buf);
            if let Some(font) = font {
                // Auch ein „zufällig gültiger“ Font darf nicht panicken.
                let _ = font.outline(GlyphKey::Char('A'));
                let _ = font.outline(GlyphKey::Gid(0));
                let _ = font.advance(GlyphKey::Gid(1));
            }
        }
    }
}

#[test]
fn truncated_and_corrupted_fonts_never_panic() {
    let mut rng = Xorshift(0x0123_4567_89ab_cdef);
    for source in [SAMPLE_OTF, SAMPLE_BARE_CFF] {
        for cut in [1usize, 3, 16, 64, 500, 2000] {
            let cut = cut.min(source.len());
            let font = GlyphFont::from_bytes(&source[..cut]);
            if let Some(font) = font {
                let _ = font.outline(GlyphKey::Gid(0));
            }
        }
        // Einzelne Bytes kippen und schauen, ob etwas explodiert.
        for _ in 0..200 {
            let mut buf = source.to_vec();
            for _ in 0..8 {
                let idx = (rng.next_u64() as usize) % buf.len();
                buf[idx] = (rng.next_u64() & 0xff) as u8;
            }
            if let Some(font) = GlyphFont::from_bytes(&buf) {
                for gid in [0u16, 1, 5, 40, 60_000] {
                    let _ = font.outline(GlyphKey::Gid(gid));
                    let _ = font.advance(GlyphKey::Gid(gid));
                }
                let _ = font.outline(GlyphKey::Char('A'));
            }
        }
    }
}

#[test]
fn empty_input_returns_none() {
    assert!(GlyphFont::from_bytes(&[]).is_none());
    assert!(GlyphFont::from_bytes(b"nope").is_none());
}

// ---------------------------------------------------------------------------
// FontCache
// ---------------------------------------------------------------------------

#[test]
fn cache_loads_each_key_only_once() {
    let mut cache = FontCache::new();
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);

    let first = cache.get_or_load("F1", Some(SAMPLE_OTF), false, false, false, false) as *const _;
    assert_eq!(cache.len(), 1);
    // Erneuter Aufruf mit anderen Daten darf nichts nachladen.
    let second = cache.get_or_load("F1", None, true, true, true, true) as *const _;
    assert_eq!(cache.len(), 1);
    assert_eq!(first, second, "Font wurde neu geladen");

    cache.get_or_load("F2", None, false, false, false, false);
    assert_eq!(cache.len(), 2);
    assert!(!cache.is_empty());
}

#[test]
fn cache_without_data_uses_the_fallback() {
    let mut cache = FontCache::new();
    let serif = cache.get_or_load("serif", None, true, false, false, false);
    assert_eq!(serif.units_per_em(), 2048.0);
    let serif_a = serif.outline(GlyphKey::Char('A')).expect("A");

    let sans = cache.get_or_load("sans", None, false, false, false, false);
    let sans_a = sans.outline(GlyphKey::Char('A')).expect("A");
    assert_ne!(serif_a, sans_a, "serif und sans sind identisch");

    // Unbrauchbare Bytes führen ebenfalls auf den Ersatzfont.
    let broken = cache.get_or_load("broken", Some(b"junk"), false, false, false, false);
    assert!(broken.outline(GlyphKey::Char('A')).is_some());
    assert_eq!(cache.len(), 3);
}

#[test]
fn cache_returns_stable_outlines() {
    let mut cache = FontCache::new();
    cache.get_or_load("F", None, false, false, false, false);

    let direct = GlyphFont::fallback(false, false, false, false)
        .outline(GlyphKey::Char('g'))
        .expect("g");
    let first = cache
        .outline("F", GlyphKey::Char('g'))
        .expect("g aus Cache");
    let second = cache.outline("F", GlyphKey::Char('g')).expect("g erneut");
    assert_eq!(first, direct);
    assert_eq!(first, second);

    // Leere Glyphen und Fehlschläge bleiben None — auch beim zweiten Mal.
    assert_eq!(cache.outline("F", GlyphKey::Char(' ')), None);
    assert_eq!(cache.outline("F", GlyphKey::Char(' ')), None);
    // Unbekannter Schlüssel: kein Font geladen, also nichts zu zeichnen.
    assert_eq!(cache.outline("unbekannt", GlyphKey::Char('A')), None);
    assert_eq!(cache.len(), 1);
}

#[test]
fn advances_are_in_font_units() {
    let font = GlyphFont::fallback(false, false, false, false);
    let upem = font.units_per_em();
    let advance = font.advance(GlyphKey::Char('A')).expect("Vorschub A");
    let ratio = advance / upem;
    assert!((0.3..=1.0).contains(&ratio), "Vorschub {ratio} em");
    // Liberation Sans ist metrisch kompatibel zu Arial: 'A' misst 667/1000 em.
    assert!((ratio - 0.667).abs() < 0.01, "{ratio}");
    assert_eq!(font.advance(GlyphKey::Char('漢')), None);
}
