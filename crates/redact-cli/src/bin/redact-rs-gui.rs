//! Fensterfassung von redact-rs — startet ausschließlich die Oberfläche.
//!
//! Unter Windows hängt an jeder Konsolenanwendung ein schwarzes
//! Konsolenfenster; beim veröffentlichten `redact-rs.exe` ließ sich das
//! nachweisen (`PE32+ executable (console)`). `#![windows_subsystem =
//! "windows"]` schaltet das ab, kostet aber jede Ausgabe auf stdout/stderr —
//! deshalb eine zweite Datei statt eines Schalters: `redact-rs.exe` bleibt
//! die Konsolenfassung mit allen Optionen, `redact-rs-gui.exe` ist die zum
//! Doppelklicken. Auf anderen Systemen ist das Attribut wirkungslos.
//!
//! Bewusst ohne `clap`: die Oberfläche kennt nur einen Fall, nämlich „öffne
//! diese Datei". Genau das kommt an, wenn man ein PDF auf das Symbol zieht.
//! Alles Weitere — Buchungsliste, Musterauswahl, Stapelbetrieb — ist ein
//! Fall für die Konsolenfassung.

// `deny` statt `forbid` aus demselben Grund wie in `main.rs`: das geteilte
// Modul unten enthält die eine `unsafe`-Stelle des Projekts.
#![deny(unsafe_code)]
#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::process::ExitCode;

// Jedes Binärziel ist ein eigenes Crate; dieselbe Datei wird deshalb
// eingebunden statt importiert. Diese Fassung hält dieselben Geheimnisse wie
// die Konsolenfassung und braucht dieselbe Absicherung — auch wenn sie unter
// Windows, wo dieses Binärziel hingehört, wirkungslos bleibt.
#[path = "../dumpable.rs"]
mod dumpable;

fn main() -> ExitCode {
    // Das Abschalten gehört an dieselbe Stelle wie in der Konsolenfassung:
    // vor allem, was den Klartext in den Speicher holt.
    //
    // Ausdrücklich abgehandelt statt weggeworfen — `let _ =` hätte hier
    // gereicht, aber dann stünde nirgends, warum diese Fassung schweigt, wo
    // die Konsolenfassung warnt.
    match dumpable::deny_core_dumps() {
        // Abgeschaltet, oder das System kennt nichts dergleichen (Windows —
        // wohin dieses Binärziel gehört). Beides ist keine Nachricht an den
        // Benutzer: das eine ist der Normalfall, das andere unabänderlich
        // und steht in SECURITY.md.
        dumpable::CoreDumps::Disabled | dumpable::CoreDumps::Unavailable => {}
        // Ein Fenster aufzumachen, bevor das Programm überhaupt sichtbar
        // ist, hiesse den Start mit einer Meldung zu beginnen, auf die
        // niemand reagieren kann. Wer den Hinweis braucht, startet die
        // Konsolenfassung — die sagt es.
        dumpable::CoreDumps::Failed => {}
    }

    // Ein Argument, das nicht mit `-` beginnt: die zu öffnende Datei.
    let pdf: Option<PathBuf> = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|p| !p.to_string_lossy().starts_with('-'));

    let config = redact_pipeline::Config {
        input: pdf.unwrap_or_default(),
        ..redact_pipeline::Config::default()
    };

    match redact_gui::run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Ohne Konsole sieht das niemand — die Meldung gehört deshalb in
            // ein Fenster. Schlägt selbst das fehl, bleibt nur der Exit-Code.
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("redact-rs")
                .set_description(format!("Fehler: {e}"))
                .show();
            ExitCode::FAILURE
        }
    }
}
