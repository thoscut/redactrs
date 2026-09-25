//! Animation A: die Schwärzung, wie sie entsteht — aus echten Ausgabedateien.
//!
//! ```console
//! $ cargo run -p redact-render --example redaction_gif -- \
//!       --page 1 --width 1500 --crop \
//!       schritt0.pdf schritt1.pdf schritt2.pdf schritt3.pdf docs/schwaerzung.gif
//! ```
//!
//! Alle Pfade bis auf den letzten sind Eingabe-PDFs in der Reihenfolge, in der
//! sie gezeigt werden; der letzte ist die Ziel-GIF-Datei.
//!
//! # Was diese Animation zeigt — und warum das nicht dasselbe ist wie eine
//! # Überblendung
//!
//! Jedes Einzelbild ist **eine wirklich geschriebene PDF-Datei**, gerendert
//! wie jede andere Seite. Es wird nichts eingeblendet, nichts überlagert,
//! nichts interpoliert. Wenn im dritten Einzelbild ein Balken über der
//! Kontonummer liegt, dann liegt er in der Datei `schritt2.pdf`, die auf der
//! Platte steht und die man mit `--check-leaks` nachprüfen kann.
//!
//! Der Preis dafür steht in der Doku: die vier Dateien entstehen aus **vier
//! Läufen mit unterschiedlichem `--patterns`**, nicht aus einem. Der
//! gewöhnliche Aufruf macht alle Schwärzungen auf einmal; die Reihenfolge ist
//! hier zum Zeigen gemacht und wird auch so benannt.
//!
//! # Prüfungen, die diese Datei erzwingt
//!
//! * Kein Einzelbild darf leer sein (`common::render_page`).
//! * Zwei aufeinanderfolgende Einzelbilder dürfen nicht gleich sein — sonst
//!   hat ein Schritt nichts bewirkt, und die Animation zeigte eine Wirkung,
//!   die es nicht gab.
//! * Alle Einzelbilder haben denselben Ausschnitt (`--crop` rechnet ihn über
//!   alle zusammen).

mod common;

use std::path::PathBuf;

use common::gif::{Frame, Loop};
use common::Fehler;
use redact_render::PageRenderer;

fn main() -> Result<(), Fehler> {
    let args = Args::parse(std::env::args().skip(1))?;

    let mut renderer = PageRenderer::new();
    let mut images = Vec::new();
    for input in &args.inputs {
        images.push(common::render_page(
            &mut renderer,
            input,
            args.page,
            args.width,
        )?);
    }

    if args.crop {
        let window = common::shared_window(images.iter());
        println!(
            "Gemeinsamer Ausschnitt: x {}..{}, y {}..{} ({} Rand)",
            window.x,
            window.x + window.width,
            window.y,
            window.y + window.height,
            common::CROP_MARGIN
        );
        images = images.iter().map(|image| image.crop(&window)).collect();
    }

    for (index, pair) in images.windows(2).enumerate() {
        if pair[0] == pair[1] {
            return Err(format!(
                "Schritt {} ({}) sieht genau aus wie Schritt {} ({}). Ein Einzelbild, \
                 das nichts ändert, behauptet eine Wirkung, die es nicht gab — \
                 entweder greift das Muster dieses Schritts auf dieser Seite nicht, \
                 oder die Reihenfolge stimmt nicht.",
                index + 1,
                args.inputs[index + 1].display(),
                index,
                args.inputs[index].display()
            )
            .into());
        }
    }

    let last = images.len() - 1;
    let frames: Vec<Frame> = images
        .into_iter()
        .enumerate()
        .map(|(index, image)| {
            let delay = if index == last {
                args.hold
            } else if index == 0 {
                args.lead
            } else {
                args.delay
            };
            Frame::new(image, delay)
        })
        .collect();

    println!(
        "{} Einzelbilder, Standzeiten {} / {} / {} cs (erstes / Mitte / letztes)",
        frames.len(),
        args.lead,
        args.delay,
        args.hold
    );

    let bytes = common::gif::encode(&frames, Loop::Forever)?;
    common::write_out(
        &args.output,
        &bytes,
        &format!(
            "{}x{}, {} Einzelbilder",
            frames[0].image.width,
            frames[0].image.height,
            frames.len()
        ),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Kommandozeile
// ---------------------------------------------------------------------------

const USAGE: &str = "\
Aufruf:
  cargo run -p redact-render --example redaction_gif -- [SCHALTER] SCHRITT.pdf … ZIEL.gif

Schalter:
  --page N     1-basierte Seitennummer (Vorgabe: 1)
  --width N    Renderbreite in Pixeln (Vorgabe: 1100)
  --crop       auf den Inhalt zuschneiden — für alle Einzelbilder derselbe Ausschnitt
  --lead N     Standzeit des ersten Einzelbildes in Hundertstelsekunden (Vorgabe: 160)
  --delay N    Standzeit der mittleren Einzelbilder (Vorgabe: 110)
  --hold N     Standzeit des letzten Einzelbildes (Vorgabe: 450)";

struct Args {
    page: usize,
    width: u32,
    crop: bool,
    lead: u16,
    delay: u16,
    hold: u16,
    inputs: Vec<PathBuf>,
    output: PathBuf,
}

impl Args {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, Fehler> {
        let mut page = 1usize;
        let mut width = 1100u32;
        let mut crop = false;
        let mut lead = 160u16;
        let mut delay = 110u16;
        let mut hold = 450u16;
        let mut rest: Vec<PathBuf> = Vec::new();

        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--page" => {
                    page = common::next_value(&mut args, "--page")?.parse()?;
                    if page == 0 {
                        return Err("--page zählt ab 1".into());
                    }
                }
                "--width" => width = common::next_value(&mut args, "--width")?.parse()?,
                "--crop" => crop = true,
                "--lead" => lead = common::next_value(&mut args, "--lead")?.parse()?,
                "--delay" => delay = common::next_value(&mut args, "--delay")?.parse()?,
                "--hold" => hold = common::next_value(&mut args, "--hold")?.parse()?,
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

        let Some(output) = rest.pop() else {
            return Err(format!("kein Ziel angegeben.\n\n{USAGE}").into());
        };
        if rest.len() < 2 {
            return Err(format!(
                "eine Animation braucht mindestens zwei Schritte, hier steht {}.\n\n{USAGE}",
                rest.len()
            )
            .into());
        }

        Ok(Self {
            page: page - 1,
            width,
            crop,
            lead,
            delay,
            hold,
            inputs: rest,
            output,
        })
    }
}
