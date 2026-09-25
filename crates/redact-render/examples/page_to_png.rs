//! Rendert PDF-Seiten mit **dem Rasterizer dieses Programms** in PNG-Dateien.
//!
//! Das ist das Werkzeug hinter den Vorher/Nachher-Bildern in `docs/`. Es gibt
//! es, damit kein Bild im Repository von Hand entstanden ist: was in der
//! README zu sehen ist, muss jeder mit einem Befehl nachbauen können — sonst
//! ist es Werbung und kein Beweis.
//!
//! ```console
//! $ cargo run -p redact-render --example page_to_png -- \
//!       --page 1 --width 1500 --crop \
//!       kontoauszug.pdf             docs/vorher.png \
//!       kontoauszug_geschwaerzt.pdf docs/nachher.png
//! ```
//!
//! Aufrufform: nach den Schaltern folgen **Paare** aus Eingabe-PDF und
//! Ziel-PNG. Mehrere Paare in einem Aufruf sind kein Komfort, sondern der
//! Grund für `--crop`: der Ausschnitt wird über alle Bilder zusammen
//! gerechnet — warum, steht bei `common::shared_window`.
//!
//! Der PNG-Schreiber, die Ausschnitt-Rechnung und die Leerbild-Sperre stehen
//! im Modul `common`, weil die Animations-Beispiele daneben dasselbe
//! brauchen.

mod common;

use std::path::PathBuf;

use common::Fehler;
use redact_render::PageRenderer;

fn main() -> Result<(), Fehler> {
    let args = Args::parse(std::env::args().skip(1))?;

    // Erst alles rendern, dann erst schreiben: der gemeinsame Ausschnitt lässt
    // sich nur berechnen, wenn alle Seiten vorliegen.
    let mut renderer = PageRenderer::new();
    let mut rendered = Vec::new();
    for (input, output) in &args.jobs {
        let image = common::render_page(&mut renderer, input, args.page, args.width)?;
        rendered.push((output, image));
    }

    let window = if args.crop {
        let window = common::shared_window(rendered.iter().map(|(_, image)| image));
        println!(
            "Gemeinsamer Ausschnitt: x {}..{}, y {}..{} ({} Rand)",
            window.x,
            window.x + window.width,
            window.y,
            window.y + window.height,
            common::CROP_MARGIN
        );
        Some(window)
    } else {
        None
    };

    for (output, image) in &rendered {
        let cropped;
        let image = match &window {
            Some(window) => {
                cropped = image.crop(window);
                &cropped
            }
            None => image,
        };
        let bytes = common::png::encode(image)?;
        common::write_out(output, &bytes, &format!("{}x{}", image.width, image.height))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Kommandozeile
// ---------------------------------------------------------------------------

const USAGE: &str = "\
Aufruf:
  cargo run -p redact-render --example page_to_png -- [SCHALTER] EIN.pdf AUS.png [EIN.pdf AUS.png …]

Schalter:
  --page N     1-basierte Seitennummer (Vorgabe: 1)
  --width N    Bildbreite in Pixeln, die Höhe folgt dem Seitenverhältnis (Vorgabe: 700)
  --crop       auf den Inhalt zuschneiden — für alle Bilder auf denselben Ausschnitt";

struct Args {
    page: usize,
    width: u32,
    crop: bool,
    jobs: Vec<(PathBuf, PathBuf)>,
}

impl Args {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, Fehler> {
        let mut page = 1usize;
        let mut width = 700u32;
        let mut crop = false;
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

        if rest.is_empty() || !rest.len().is_multiple_of(2) {
            return Err(format!(
                "Eingabe und Ausgabe kommen paarweise, hier stehen {} Pfade.\n\n{USAGE}",
                rest.len()
            )
            .into());
        }

        let jobs = rest
            .chunks_exact(2)
            .map(|pair| (pair[0].clone(), pair[1].clone()))
            .collect();
        Ok(Self {
            page: page - 1,
            width,
            crop,
            jobs,
        })
    }
}
