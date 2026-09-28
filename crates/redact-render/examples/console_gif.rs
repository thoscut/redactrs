//! Animation B: der Lauf in der Konsole — aus einem Mitschnitt.
//!
//! ```console
//! $ cargo run -p redact-render --example console_gif -- \
//!       mitschnitt.txt docs/konsole.gif
//! ```
//!
//! # Die Regel, an der diese Animation hängt
//!
//! **Die Antwort wird nicht abgetippt.** Was als Ausgabe im Bild steht, hat
//! ein echter Lauf gedruckt und `scripts/make-preview.sh` mitgeschnitten.
//! Stünde im Bild ein Zeichen, das kein Lauf gedruckt hat, wäre die ganze
//! Animation wertlos — und zwar auf die schlimmste Art, weil sie echt
//! aussieht.
//!
//! Der **Befehl** darf getippt aussehen: er wird Zeichen für Zeichen
//! aufgebaut. Das ist keine Erfindung, sondern eine Darstellung dessen, was
//! jemand eintippt; die Zeichen stehen so in der Mitschnittdatei.
//!
//! # Das Mitschnittformat
//!
//! Eine Zeile, ein Ereignis — absichtlich so schlicht, dass man die Datei
//! neben das Bild legen und Zeichen für Zeichen vergleichen kann:
//!
//! | Anfang | Bedeutung |
//! |--------|-----------|
//! | `$ `   | Befehlszeile; wird getippt |
//! | `\|`   | eine Zeile Ausgabe; erscheint auf einmal |
//! | `~ N`  | N Hundertstelsekunden länger stehen lassen |
//! | `#`    | Bemerkung, wird übergangen |
//!
//! Die Ausgabezeilen entstehen im Skript mechanisch (`sed 's/^/| /'`) aus dem,
//! was der Lauf gedruckt hat. Es gibt keinen Weg, dabei etwas umzuschreiben,
//! ohne dass es in der Mitschnittdatei sichtbar wäre.
//!
//! # Höhe und Breite
//!
//! Die Zeilenzahl wird **aus dem Mitschnitt gerechnet**, nicht festgelegt.
//! Wird die Ausgabe des Programms einmal länger, wächst das Bild mit, statt
//! den Rest abzuschneiden.

mod common;

use std::path::PathBuf;

use common::console::Console;
use common::gif::{Frame, Loop};
use common::Fehler;

fn main() -> Result<(), Fehler> {
    let args = Args::parse(std::env::args().skip(1))?;
    let text = std::fs::read_to_string(&args.script)?;
    let events = parse_events(&text)?;

    let rows = needed_rows(&events, args.cols);
    println!(
        "Mitschnitt: {} Ereignisse → {} Zeilen à {} Zeichen",
        events.len(),
        rows,
        args.cols
    );

    let mut console = Console::new(args.cols, rows, args.size, args.background, args.foreground)?;
    let mut frames: Vec<Frame> = Vec::new();

    for event in &events {
        match event {
            Event::Command(line) => {
                console.write("$ ")?;
                let chars: Vec<char> = line.chars().collect();
                for chunk in chars.chunks(args.chunk) {
                    for ch in chunk {
                        console.write(&ch.to_string())?;
                    }
                    frames.push(Frame::new(console.snapshot(), args.type_cs));
                }
                console.write("\n")?;
                extend_last(&mut frames, args.think_cs);
            }
            Event::Output(line) => {
                console.write(line)?;
                console.write("\n")?;
                frames.push(Frame::new(console.snapshot(), args.line_cs));
            }
            Event::Hold(cs) => extend_last(&mut frames, *cs),
        }
    }

    if frames.is_empty() {
        return Err("der Mitschnitt hat kein einziges Ereignis ergeben".into());
    }
    // Der letzte Zustand ist die Aussage — er bleibt am längsten stehen.
    let last = frames.len() - 1;
    frames[last].delay_cs = frames[last].delay_cs.saturating_add(args.hold_cs);

    // Kein Einzelbild darf leer sein. Beim ersten Bild ist das keine
    // Formalität: begänne die Animation mit der leeren Konsole, wäre genau
    // das erste, was jemand sieht, ein Bild ohne Aussage.
    for (index, frame) in frames.iter().enumerate() {
        if frame.image.ink_ratio(args.background) == 0.0 {
            return Err(format!("Einzelbild {index} ist leer — so wird das kein Beleg").into());
        }
    }

    let total: u32 = frames.iter().map(|f| u32::from(f.delay_cs)).sum();
    println!(
        "{} Einzelbilder, {} x {} px, Gesamtdauer {:.1} s",
        frames.len(),
        console.width(),
        console.height(),
        f64::from(total) / 100.0
    );

    let bytes = common::gif::encode(&frames, Loop::Forever)?;
    common::write_out(
        &args.output,
        &bytes,
        &format!(
            "{}x{}, {} Einzelbilder",
            console.width(),
            console.height(),
            frames.len()
        ),
    )?;
    Ok(())
}

fn extend_last(frames: &mut [Frame], cs: u16) {
    if let Some(frame) = frames.last_mut() {
        frame.delay_cs = frame.delay_cs.saturating_add(cs);
    }
}

// ---------------------------------------------------------------------------
// Mitschnitt
// ---------------------------------------------------------------------------

enum Event {
    Command(String),
    Output(String),
    Hold(u16),
}

fn parse_events(text: &str) -> Result<Vec<Event>, Fehler> {
    let mut out = Vec::new();
    for (number, raw) in text.lines().enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("$ ") {
            out.push(Event::Command(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix("| ") {
            out.push(Event::Output(rest.to_string()));
        } else if line == "|" {
            out.push(Event::Output(String::new()));
        } else if let Some(rest) = line.strip_prefix("~ ") {
            out.push(Event::Hold(rest.trim().parse()?));
        } else {
            return Err(format!(
                "Mitschnitt Zeile {}: kein bekanntes Ereignis ({line:?}). \
                 Erlaubt sind „$ “, „| “, „~ “ und „#“.",
                number + 1
            )
            .into());
        }
    }
    Ok(out)
}

/// Wie viele Zeilen der Mitschnitt belegt, wenn bei `cols` umgebrochen wird.
fn needed_rows(events: &[Event], cols: usize) -> usize {
    let wrapped = |len: usize| len.div_ceil(cols).max(1);
    events
        .iter()
        .map(|event| match event {
            Event::Command(line) => wrapped(line.chars().count() + 2),
            Event::Output(line) => wrapped(line.chars().count()),
            Event::Hold(_) => 0,
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Kommandozeile
// ---------------------------------------------------------------------------

const USAGE: &str = "\
Aufruf:
  cargo run -p redact-render --example console_gif -- [SCHALTER] MITSCHNITT.txt ZIEL.gif

Schalter:
  --cols N     Zeichen je Zeile (Vorgabe: 86)
  --size N     Schriftgröße in Pixeln (Vorgabe: 14)
  --chunk N    Zeichen je Tipp-Einzelbild (Vorgabe: 3)
  --type N     Standzeit eines Tipp-Einzelbildes in Hundertstelsekunden (Vorgabe: 4)
  --think N    Pause nach einem getippten Befehl (Vorgabe: 40)
  --line N     Standzeit je Ausgabezeile (Vorgabe: 8)
  --hold N     zusätzliche Standzeit auf dem letzten Bild (Vorgabe: 500)";

struct Args {
    cols: usize,
    size: f64,
    chunk: usize,
    type_cs: u16,
    think_cs: u16,
    line_cs: u16,
    hold_cs: u16,
    background: [u8; 4],
    foreground: [u8; 4],
    script: PathBuf,
    output: PathBuf,
}

impl Args {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, Fehler> {
        let mut cols = 86usize;
        let mut size = 14.0f64;
        let mut chunk = 3usize;
        let mut type_cs = 4u16;
        let mut think_cs = 40u16;
        let mut line_cs = 8u16;
        let mut hold_cs = 500u16;
        let mut rest: Vec<PathBuf> = Vec::new();

        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--cols" => cols = common::next_value(&mut args, "--cols")?.parse()?,
                "--size" => size = common::next_value(&mut args, "--size")?.parse()?,
                "--chunk" => chunk = common::next_value(&mut args, "--chunk")?.parse()?,
                "--type" => type_cs = common::next_value(&mut args, "--type")?.parse()?,
                "--think" => think_cs = common::next_value(&mut args, "--think")?.parse()?,
                "--line" => line_cs = common::next_value(&mut args, "--line")?.parse()?,
                "--hold" => hold_cs = common::next_value(&mut args, "--hold")?.parse()?,
                "-h" | "--help" => {
                    println!("{USAGE}");
                    std::process::exit(0);
                }
                other if other.starts_with("--") => {
                    return Err(format!("unbekannter Schalter {other}\n\n{USAGE}").into());
                }
                other => rest.push(PathBuf::from(other)),
            }
        }

        if cols == 0 || chunk == 0 {
            return Err("--cols und --chunk müssen größer als null sein".into());
        }
        let [script, output] = <[PathBuf; 2]>::try_from(rest).map_err(|rest| {
            format!(
                "erwartet werden genau zwei Pfade (Mitschnitt, Ziel), hier stehen {}.\n\n{USAGE}",
                rest.len()
            )
        })?;

        Ok(Self {
            cols,
            size,
            chunk,
            type_cs,
            think_cs,
            line_cs,
            hold_cs,
            // Ein ruhiges Dunkelgrau statt Reinschwarz und ein gedämpftes Weiß
            // statt Reinweiß: der Wechsel zwischen Bild und Seite soll niemanden
            // blenden, und harte Helligkeitssprünge sind ausdrücklich unerwünscht.
            // Beide Farben sind echte Graustufen — damit bleibt die
            // Kantenglättung zwischen ihnen ebenfalls grau, und die GIF-Palette
            // kommt mit höchstens 200 Einträgen exakt aus.
            background: [26, 26, 26, 255],
            foreground: [225, 225, 225, 255],
            script,
            output,
        })
    }
}
