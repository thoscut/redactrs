//! Minimaler PNG-Schreiber (Graustufen oder RGB, 8 Bit).
//!
//! Zwei Sparsamkeiten, die zusammen den Unterschied zwischen „passt ins
//! Repository“ und „passt nicht“ ausmachen:
//!
//! 1. **Graustufen, wenn das Bild grau ist.** Eine geschwärzte Textseite ist
//!    es; dann trägt jedes Pixel ein Byte statt vier. Sobald ein einziges
//!    Pixel farbig ist, wird RGB geschrieben — lieber größer als falsch.
//! 2. **Zeilenfilter nach dem üblichen Verfahren** (kleinste Summe der
//!    Beträge, PNG-Spezifikation Abschnitt 12.8). Weiße Flächen und
//!    wiederholte Zeilen werden dadurch zu Nullen, die Deflate fast
//!    vollständig wegräumt.
//!
//! Was hier **nicht** hineingeschrieben wird: `tIME`, `tEXt` und jeder andere
//! Zusatzblock. Eine PNG-Datei aus diesem Schreiber enthält keinen
//! Zeitstempel und keinen Rechnernamen — zwei Läufe derselben Fassung
//! liefern dieselben Bytes.

use super::{Canvas, Fehler};

pub fn encode(image: &Canvas) -> Result<Vec<u8>, Fehler> {
    let (width, height, rgba) = (image.width, image.height, &image.rgba);
    let pixels = width as usize * height as usize;
    if rgba.len() != pixels * 4 {
        return Err(format!("Puffer passt nicht zu {width}x{height}").into());
    }

    let gray = image.is_gray();
    let (channels, color_type) = if gray { (1usize, 0u8) } else { (3usize, 2u8) };

    let mut samples = Vec::with_capacity(pixels * channels);
    for p in rgba.chunks_exact(4) {
        if gray {
            samples.push(p[0]);
        } else {
            samples.extend_from_slice(&p[..3]);
        }
    }

    let stride = width as usize * channels;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    let mut previous = vec![0u8; stride];
    for row in samples.chunks_exact(stride) {
        let (filter, filtered) = best_filter(row, &previous, channels);
        raw.push(filter);
        raw.extend_from_slice(&filtered);
        previous.copy_from_slice(row);
    }

    let mut out: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, color_type, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib(raw)?);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// Zlib-Strom für `IDAT`.
///
/// Die Kompression kommt aus `lopdf::Stream::compress` — dieselbe, die das
/// Programm für PDF-Streams benutzt. `compress` lässt den Inhalt unangetastet,
/// wenn er sich nicht lohnend verkleinern lässt (dann steht kein `/Filter` im
/// Dictionary); für diesen Fall bleibt der einfache Weg über
/// „stored“-Blöcke, damit auf jeden Fall ein gültiger Zlib-Strom entsteht.
fn zlib(raw: Vec<u8>) -> Result<Vec<u8>, Fehler> {
    let mut stream = lopdf::Stream::new(lopdf::dictionary! {}, raw);
    stream.compress()?;
    if stream.dict.get(b"Filter").is_ok() {
        return Ok(stream.content);
    }

    let raw = stream.content;
    let mut out = vec![0x78u8, 0x01];
    let mut offset = 0usize;
    loop {
        let len = (raw.len() - offset).min(0xFFFF);
        let last = offset + len == raw.len();
        out.push(u8::from(last));
        out.extend_from_slice(&(len as u16).to_le_bytes());
        out.extend_from_slice(&(!(len as u16)).to_le_bytes());
        out.extend_from_slice(&raw[offset..offset + len]);
        offset += len;
        if last {
            break;
        }
    }
    out.extend_from_slice(&adler32(&raw).to_be_bytes());
    Ok(out)
}

fn adler32(data: &[u8]) -> u32 {
    let (mut low, mut high) = (1u32, 0u32);
    for byte in data {
        low = (low + u32::from(*byte)) % 65_521;
        high = (high + low) % 65_521;
    }
    (high << 16) | low
}

/// Wählt für eine Zeile den Filter mit der kleinsten Summe der Beträge.
fn best_filter(row: &[u8], previous: &[u8], channels: usize) -> (u8, Vec<u8>) {
    let mut best: Option<(u8, Vec<u8>, u64)> = None;
    for filter in 0u8..=4 {
        let candidate = apply_filter(filter, row, previous, channels);
        let cost: u64 = candidate
            .iter()
            .map(|b| u64::from(if *b < 128 { *b } else { 255 - *b + 1 }))
            .sum();
        if best
            .as_ref()
            .is_none_or(|(_, _, best_cost)| cost < *best_cost)
        {
            best = Some((filter, candidate, cost));
        }
    }
    let (filter, bytes, _) = best.expect("mindestens ein Filter wurde geprüft");
    (filter, bytes)
}

fn apply_filter(filter: u8, row: &[u8], previous: &[u8], channels: usize) -> Vec<u8> {
    (0..row.len())
        .map(|i| {
            let a = if i >= channels { row[i - channels] } else { 0 };
            let b = previous[i];
            let c = if i >= channels {
                previous[i - channels]
            } else {
                0
            };
            let sub = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                _ => paeth(a, b, c),
            };
            row[i].wrapping_sub(sub)
        })
        .collect()
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i32::from(a) + i32::from(b) - i32::from(c);
    let (pa, pb, pc) = (
        (p - i32::from(a)).abs(),
        (p - i32::from(b)).abs(),
        (p - i32::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
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
