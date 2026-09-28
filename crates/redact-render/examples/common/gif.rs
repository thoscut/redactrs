//! GIF89a-Schreiber mit LZW — ohne neue Abhängigkeit.
//!
//! # Die Palette wird nicht geraten
//!
//! GIF kann höchstens 256 Farben. Der übliche Weg dorthin ist Quantisierung:
//! man wirft Farben zusammen, bis es passt, und streut den Fehler per Dithering
//! über das Bild. Das passiert hier **nicht**. Stattdessen zählt
//! [`palette`] die tatsächlich vorkommenden Farben. Passen sie in 256
//! Einträge, ist die Palette exakt und das GIF pixelgenau dasselbe Bild;
//! passen sie nicht, bricht der Schreiber ab.
//!
//! Für die Belege dieses Projekts geht das immer auf: die gerenderten Seiten
//! sind grau, und Grau hat höchstens 256 Werte. Ein Bild, das nicht hineinpasst,
//! soll auch nicht als GIF entstehen — ein gedithertes Belegbild wäre ein Bild,
//! das der Rasterizer so nie gezeichnet hat.
//!
//! # Einzelbilder sind Differenzen
//!
//! Nur das erste Einzelbild ist vollständig. Jedes weitere wird auf das
//! kleinste Rechteck beschnitten, in dem es sich vom vorigen unterscheidet
//! (GIF kann das von Haus aus: der Image Descriptor trägt Position und Größe,
//! die Entsorgungsart „nicht entsorgen“ lässt den Rest stehen). Das ist
//! verlustfrei und der Grund, warum diese Dateien klein bleiben: bei der
//! Schwärzungs-Animation ändert sich je Schritt ein Balken, bei der Konsole
//! ein Zeichen.
//!
//! # Kein Zeitstempel
//!
//! Das GIF-Format hat kein Feld für Erzeugungszeit, Rechnernamen oder
//! Programmversion, und dieser Schreiber erfindet keines. Der einzige
//! Zusatzblock ist die Netscape-Erweiterung für die Wiederholung. Zwei Läufe
//! derselben Fassung liefern damit dieselben Bytes.

use std::collections::HashMap;

use super::{Canvas, Fehler};

/// Ein Einzelbild samt Standzeit.
pub struct Frame {
    pub image: Canvas,
    /// Standzeit in Hundertstelsekunden (GIF rechnet so).
    pub delay_cs: u16,
}

impl Frame {
    pub fn new(image: Canvas, delay_cs: u16) -> Self {
        Self { image, delay_cs }
    }
}

/// Wie oft die Animation läuft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loop {
    /// Einmal und dann stehenbleiben.
    Once,
    /// Endlos — nur sinnvoll mit einer spürbaren Standzeit auf dem letzten
    /// Einzelbild, sonst kommt das Bild nie zur Ruhe.
    Forever,
}

/// Baut die GIF-Datei.
///
/// Prüft dabei zweierlei, weil ein Beleg, den niemand ansieht, sonst still
/// kaputtgehen kann: alle Einzelbilder müssen gleich groß sein, und kein
/// Einzelbild darf leer sein (siehe `ink_ratio`-Prüfung beim Aufrufer).
pub fn encode(frames: &[Frame], repeat: Loop) -> Result<Vec<u8>, Fehler> {
    let first = frames
        .first()
        .ok_or("GIF ohne ein einziges Einzelbild ist kein Beleg")?;
    let (width, height) = (first.image.width, first.image.height);
    for (index, frame) in frames.iter().enumerate() {
        if frame.image.width != width || frame.image.height != height {
            return Err(format!(
                "Einzelbild {index} ist {}x{}, das erste war {width}x{height} — \
                 eine Animation mit springender Größe zeigt einen Unterschied, \
                 den nichts gemacht hat",
                frame.image.width, frame.image.height
            )
            .into());
        }
    }

    let palette = palette(frames)?;
    let bits = palette_bits(palette.len());
    let table_len = 1usize << bits;

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"GIF89a");
    out.extend_from_slice(&(width as u16).to_le_bytes());
    out.extend_from_slice(&(height as u16).to_le_bytes());
    // Globale Farbtabelle vorhanden (0x80), Farbtiefe 8 Bit (0x70),
    // unsortiert, Tabellengröße 2^(bits).
    out.push(0x80 | 0x70 | (bits - 1));
    out.push(0); // Hintergrundfarbe: Index 0
    out.push(0); // Seitenverhältnis: nicht angegeben
    for index in 0..table_len {
        let color = palette.get(index).copied().unwrap_or([0, 0, 0]);
        out.extend_from_slice(&color);
    }

    if repeat == Loop::Forever {
        // Netscape Application Extension — der einzige Weg, „endlos“ zu sagen.
        out.extend_from_slice(&[0x21, 0xFF, 0x0B]);
        out.extend_from_slice(b"NETSCAPE2.0");
        out.extend_from_slice(&[0x03, 0x01, 0x00, 0x00, 0x00]);
    }

    let lookup: HashMap<[u8; 3], u8> = palette
        .iter()
        .enumerate()
        .map(|(index, color)| (*color, index as u8))
        .collect();

    let mut previous: Option<&Canvas> = None;
    for frame in frames {
        let window = match previous {
            None => super::Window {
                x: 0,
                y: 0,
                width,
                height,
            },
            Some(before) => match frame.image.diff_window(before) {
                Some(window) => window,
                // Nichts geändert: es reicht, das vorige Einzelbild länger
                // stehen zu lassen. Ein 1x1-Block genügt dafür.
                None => super::Window {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            },
        };

        // Graphic Control Extension: Entsorgungsart 1 („nicht entsorgen“ —
        // das vorige Bild bleibt stehen), keine Transparenz.
        out.extend_from_slice(&[0x21, 0xF9, 0x04, 0x04]);
        out.extend_from_slice(&frame.delay_cs.to_le_bytes());
        out.extend_from_slice(&[0x00, 0x00]);

        // Image Descriptor
        out.push(0x2C);
        out.extend_from_slice(&(window.x as u16).to_le_bytes());
        out.extend_from_slice(&(window.y as u16).to_le_bytes());
        out.extend_from_slice(&(window.width as u16).to_le_bytes());
        out.extend_from_slice(&(window.height as u16).to_le_bytes());
        out.push(0x00); // keine lokale Farbtabelle, nicht verschachtelt

        let mut indices = Vec::with_capacity(window.width as usize * window.height as usize);
        for y in 0..window.height {
            for x in 0..window.width {
                let pixel = frame.image.pixel(window.x + x, window.y + y);
                let key = [pixel[0], pixel[1], pixel[2]];
                indices.push(*lookup.get(&key).expect("Palette deckt jede Farbe ab"));
            }
        }

        let min_code_size = bits.max(2);
        out.push(min_code_size);
        write_blocks(&mut out, &lzw(&indices, min_code_size));

        previous = Some(&frame.image);
    }

    out.push(0x3B); // Trailer
    Ok(out)
}

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// Alle im Film vorkommenden Farben, aufsteigend sortiert.
///
/// Sortiert, damit die Palette nicht davon abhängt, in welcher Reihenfolge die
/// Pixel durchlaufen wurden — dieselben Einzelbilder ergeben dieselbe Datei.
fn palette(frames: &[Frame]) -> Result<Vec<[u8; 3]>, Fehler> {
    // Ein Bitfeld über alle 2^24 Farben (2 MB) statt einer Menge: die
    // Animationen haben Millionen Pixel, und ein Bitfeld ist dabei sowohl
    // schnell als auch von der Durchlaufreihenfolge unabhängig.
    let mut seen = vec![0u64; (1usize << 24) / 64];
    let mut count = 0usize;
    for frame in frames {
        for pixel in frame.image.rgba.chunks_exact(4) {
            let key = (usize::from(pixel[0]) << 16)
                | (usize::from(pixel[1]) << 8)
                | usize::from(pixel[2]);
            let (word, bit) = (key / 64, key % 64);
            if seen[word] & (1u64 << bit) != 0 {
                continue;
            }
            seen[word] |= 1u64 << bit;
            count += 1;
            if count > 256 {
                return Err(
                    "mehr als 256 verschiedene Farben in der Animation. GIF kann \
                     das nur mit Quantisierung, und die wird hier nicht gemacht: ein \
                     gedithertes Belegbild zeigte etwas, das der Rasterizer nie \
                     gezeichnet hat. Für graue Seiten tritt der Fall nicht ein \
                     (Grau hat 256 Werte); ein farbiges Dokument gehört als PNG belegt."
                        .into(),
                );
            }
        }
    }

    let mut out = Vec::with_capacity(count);
    for (word, bits) in seen.iter().enumerate() {
        let mut bits = *bits;
        while bits != 0 {
            let bit = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            let key = word * 64 + bit;
            out.push([(key >> 16) as u8, (key >> 8) as u8, key as u8]);
        }
    }
    Ok(out)
}

/// Wie viele Bits die Farbtabelle braucht (GIF erlaubt nur Zweierpotenzen,
/// mindestens 2 Einträge).
fn palette_bits(colors: usize) -> u8 {
    let mut bits = 1u8;
    while (1usize << bits) < colors.max(2) {
        bits += 1;
    }
    bits.min(8)
}

// ---------------------------------------------------------------------------
// LZW
// ---------------------------------------------------------------------------

/// GIF-LZW: variable Codebreite, Clear- und End-of-Information-Code,
/// Bits von der niederwertigen Seite her gepackt.
fn lzw(indices: &[u8], min_code_size: u8) -> Vec<u8> {
    let clear = 1u16 << min_code_size;
    let eoi = clear + 1;

    let mut bits = BitWriter::default();
    let mut width = min_code_size + 1;
    let mut table: HashMap<(u16, u8), u16> = HashMap::new();
    let mut next = eoi + 1;

    bits.push(clear, width);

    let mut current: Option<u16> = None;
    for &byte in indices {
        let Some(prefix) = current else {
            current = Some(u16::from(byte));
            continue;
        };
        if let Some(&code) = table.get(&(prefix, byte)) {
            current = Some(code);
            continue;
        }
        bits.push(prefix, width);
        if next < 4096 {
            table.insert((prefix, byte), next);
            next += 1;
            // Die Breite wächst, sobald der nächste zu vergebende Code nicht
            // mehr hineinpasst.
            if next > (1u16 << width) && width < 12 {
                width += 1;
            }
        } else {
            bits.push(clear, width);
            table.clear();
            next = eoi + 1;
            width = min_code_size + 1;
        }
        current = Some(u16::from(byte));
    }
    if let Some(prefix) = current {
        bits.push(prefix, width);
    }
    bits.push(eoi, width);
    bits.finish()
}

#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    accumulator: u32,
    used: u8,
}

impl BitWriter {
    fn push(&mut self, code: u16, width: u8) {
        self.accumulator |= u32::from(code) << self.used;
        self.used += width;
        while self.used >= 8 {
            self.out.push((self.accumulator & 0xFF) as u8);
            self.accumulator >>= 8;
            self.used -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.out.push((self.accumulator & 0xFF) as u8);
        }
        self.out
    }
}

/// Verpackt den LZW-Strom in GIF-Unterblöcke (je höchstens 255 Byte).
fn write_blocks(out: &mut Vec<u8>, data: &[u8]) {
    for block in data.chunks(255) {
        out.push(block.len() as u8);
        out.extend_from_slice(block);
    }
    out.push(0x00);
}
