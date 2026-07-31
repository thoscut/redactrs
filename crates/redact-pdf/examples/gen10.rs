//! Erzeugt ein größeres Test-PDF (Standard: 10 Seiten à 45 Zeilen).
//!
//! Dient dazu, das Akzeptanzkriterium „10 Seiten in unter 2 Sekunden“
//! nachzumessen:
//!
//! ```bash
//! cargo run --release -p redact-pdf --example gen10 -- gross.pdf 10
//! time redact-rs gross.pdf -o out.pdf --patterns iban_de,amount_eur,date_de
//! ```

use redact_pdf::testing::{build_pdf, TextItem};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "gross.pdf".to_string());
    let page_count: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10)
        .clamp(1, 500);

    let pages: Vec<Vec<TextItem>> = (0..page_count)
        .map(|page| {
            (0..45)
                .map(|line| {
                    TextItem::new(
                        50.0,
                        800.0 - line as f64 * 17.0,
                        10.0,
                        format!(
                            "{page:02}/{line:02} 05.01.2026 Überweisung an Müller & Söhne GmbH  \
                             DE89 3704 0044 0532 0130 00  1.234,56 EUR"
                        ),
                    )
                })
                .collect()
        })
        .collect();

    std::fs::write(&path, build_pdf(&pages)).expect("Datei schreibbar");
    println!("{page_count} Seiten geschrieben: {path}");
}
