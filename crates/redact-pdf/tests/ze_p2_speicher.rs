//! Gegenprüfung P2 (Fix-Runde 4): was der aufgeschobene Schreibvorgang
//! **hält**.
//!
//! `redact::PendingPage` sammelt je Seite die Pläne der getroffenen
//! Textoperationen, die schon entschiedenen Spiegel und die offenen
//! Abschnitte über Formularen — und gibt sie erst nach der Formularschleife
//! frei. Vorher fiel das je Seite weg. Die Frage ist also, ob der Speicher
//! jetzt mit der Seitenzahl wächst.
//!
//! Gemessen wird am **Kindprozess** (`VmHWM`), nicht hier: dieser Test baut
//! nur das Material. `ZE_P2_OUT` sagt wohin; ohne die Variable tut er nichts.

use redact_pdf::testing::{build_pdf, TextItem};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

/// Eine Seite mit `hits` Treffern und ebenso vielen harmlosen Zeilen.
fn sheet(page: usize, hits: usize) -> Vec<TextItem> {
    let mut items = vec![TextItem::new(
        72.0,
        800.0,
        10.0,
        format!("Seite {} Kontoauszug Musterbank AG", page + 1),
    )];
    for i in 0..hits {
        let y = 780.0 - (i as f64) * 18.0;
        items.push(TextItem::new(
            72.0,
            y,
            10.0,
            format!("Buchung {i}: IBAN {SECRET} Betrag 1.234,56 EUR"),
        ));
    }
    items
}

fn document(pages: usize, hits: usize) -> Vec<u8> {
    let sheets: Vec<Vec<TextItem>> = (0..pages).map(|p| sheet(p, hits)).collect();
    build_pdf(&sheets)
}

/// Baut die Messreihe: gleiche Trefferdichte, verschiedene Seitenzahlen —
/// und dieselben Seitenzahlen ganz **ohne** Treffer als Nulllinie. Aus dem
/// Abstand der beiden Reihen liest sich ab, was die aufgeschobenen Pläne
/// kosten.
#[test]
fn schreibt_die_messreihe() {
    let Ok(dir) = std::env::var("ZE_P2_OUT") else {
        return;
    };
    for pages in [50usize, 100, 200, 300] {
        std::fs::write(format!("{dir}/treffer_{pages}.pdf"), document(pages, 40))
            .expect("schreibbar");
        std::fs::write(format!("{dir}/leer_{pages}.pdf"), document(pages, 0)).expect("schreibbar");
    }
}
