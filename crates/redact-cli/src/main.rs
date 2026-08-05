//! redact-rs — lokales Schwärzen sensibler Daten in PDF-Dokumenten.

#![forbid(unsafe_code)]

mod batch;
mod check;
mod cli;

use std::process::ExitCode;

use clap::Parser;
use redact_core::{safe_text, RedactError, Result};
use redact_pipeline::{Outcome, Settings};

use crate::cli::Cli;

/// Alles gelaufen, alles gesehen.
pub const EXIT_OK: u8 = 0;

/// Der Lauf ist gescheitert: keine (oder keine vollständige) Ausgabe.
pub const EXIT_ERROR: u8 = 1;

/// Bedienfehler — die Kommandozeile, die Einstellungsdatei oder eine
/// mitgegebene Datei passt nicht ([`RedactError::Config`]).
pub const EXIT_USAGE: u8 = 2;

/// **Verarbeitet, aber nicht vollständig geprüft.**
///
/// Die Ausgabe ist geschrieben, und was gefunden wurde, ist geschwärzt. Für
/// einen Teil des Dokuments konnte die Analyse aber nicht einstehen: ein Font
/// ohne `/ToUnicode`, ein Form-XObject unterhalb der Verschachtelungsgrenze,
/// ein Kachelmuster mit Text, eine Annotation ohne Erscheinungsstrom. Dort kann
/// etwas stehen geblieben sein, ohne dass es jemand gemerkt hätte.
///
/// ## Warum ein eigener Wert und nicht die 2
///
/// Die 2 heißt in diesem Programm seit jeher **Bedienfehler** — „der Schalter
/// passt nicht“, „diese Review-Datei gehört woandershin“. Sie auf einen
/// Dokumentbefund zu legen, hieße zwei sehr verschiedene Nachrichten unter
/// einer Zahl zu senden: die eine ist an den Aufrufenden gerichtet und heißt
/// „mach es anders“, die andere an die Prüfung des Ergebnisses und heißt
/// „schau selbst nach“.
///
/// ## Warum nicht bei jeder Warnung
///
/// Ein Rückgabewert, der bei harmlosen Warnungen anspringt, wird nach der
/// zweiten Datei weggedrückt. Er gilt deshalb **nur** für Deckungslücken;
/// welche Warnung das ist und welche nicht, entscheidet
/// [`redact_pipeline::coverage`] — mit Begründung je Ausnahme.
///
/// ## Der zweite Fall: `--check-leaks` hat etwas gefunden
///
/// Dieselbe Zahl, dieselbe Nachricht: **der Lauf ist gelungen, das Ergebnis
/// ist nicht in Ordnung — sieh hin.** Die drei anderen Werte passen nicht:
///
/// * `0` wäre falsch und gefährlich. `redact-rs out.pdf --check-leaks "$IBAN"
///   && versenden out.pdf` verschickte die Datei mit der IBAN darin.
/// * `1` heißt „fehlgeschlagen, keine brauchbare Ausgabe“. Ein Fund ist kein
///   Verarbeitungsfehler: die Datei wurde gelesen, die Suche lief vollständig,
///   die Antwort steht fest. Sie lautet nur „ja, es steht noch drin“.
/// * `2` heißt „mach es anders“ und ist an den Aufrufenden gerichtet. An dem
///   Aufruf war nichts falsch.
///
/// Ein Skript unterscheidet damit drei Fälle, ohne die Ausgabe zu lesen:
/// `0` sauber (im Rahmen der geprüften Liste), `3` Fund, alles andere Fehler.
pub const EXIT_INCOMPLETE: u8 = 3;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(&cli) {
        Ok(code) => code,
        Err(e) => {
            // Die eine Stelle, an der jeder Fehler herauskommt — und damit die
            // eine Stelle, an der er entschärft werden muss. In fast jeder
            // Meldung steckt ein Dateiname, und der kommt von außen: mit
            // `ESC [ 2 K` darin löschte er beim Ausgeben die Zeile darüber.
            eprintln!("Fehler: {}", safe_text(&e.to_string()));
            if let RedactError::Config(_) = e {
                return ExitCode::from(EXIT_USAGE);
            }
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn dispatch(cli: &Cli) -> Result<ExitCode> {
    if cli.list_patterns {
        list_patterns()?;
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(path) = &cli.write_demo {
        // Auch die Beispieldatei geht durch den zentralen Schreibpfad. Vorher
        // hat sie `--force` ignoriert und wäre über einen Symlink an eine
        // beliebige Stelle geschrieben worden.
        redact_pdf::document::write_file(
            path,
            &redact_pdf::testing::demo_statement(),
            &redact_pdf::document::WriteOptions::new().force(cli.force),
        )?;
        println!("Beispiel-PDF geschrieben: {}", path.display());
        return Ok(ExitCode::SUCCESS);
    }

    // Die Nachprüfung — vor der Einstellungsdatei und vor der Oberfläche.
    //
    // Vor der Einstellungsdatei, weil sie nichts daraus braucht: kein
    // Namenszusatz, keine Muster, keine Polsterung. Eine kaputte
    // `settings.yaml` soll nicht ausgerechnet die Kontrolle verhindern, mit
    // der jemand nachsieht, ob seine Datei sauber ist.
    //
    // Vor der Oberfläche, weil `redact-rs --check-leaks …` ohne Eingabedatei
    // sonst in den GUI-Zweig liefe (`inputs.is_empty() && output.is_none()`)
    // und ein Fenster öffnete, statt den Bedienfehler zu nennen.
    if !cli.check_leaks.is_empty() {
        return check::run(cli);
    }

    // Die Einstellungsdatei — die Schicht zwischen Vorgabe und Kommandozeile.
    // Eine fehlerhafte Datei ist ein Fehler und wird nicht übergangen.
    let settings = Settings::load()?;

    // Ohne Eingabedatei oder mit --gui: grafische Oberfläche.
    if cli.gui || (cli.inputs.is_empty() && cli.output.is_none()) {
        return start_gui(cli, &settings).map(|()| ExitCode::SUCCESS);
    }

    if cli.inputs.is_empty() {
        return Err(RedactError::Config(
            "keine Eingabedatei angegeben (`redact-rs --help` zeigt Beispiele)".into(),
        ));
    }

    let inputs = batch::gather_inputs(&cli.inputs, &cli.config(&settings).output_suffix)?;

    // Eine Datei bleibt eine Datei: derselbe Ablauf und dieselben
    // Rückgabewerte wie bisher — ein Fehler wandert nach oben und wird dort
    // nach Art unterschieden (Konfiguration ⇒ 2, sonst 1).
    if let [input] = inputs.as_slice() {
        let outcome = redact_pipeline::run(&cli.config_for(&settings, input))?;
        if cli.json {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        } else if !cli.quiet {
            report(&outcome);
        }
        // Eine Datei, die nur teilweise durchsucht werden konnte, ist kein
        // Fehler — sie ist aber auch kein „alles gut“. Siehe [`EXIT_INCOMPLETE`].
        return Ok(exit_code_for(&outcome));
    }

    batch::run(cli, &settings, &inputs)
}

/// Der Rückgabewert eines gelungenen Laufs: 0 oder [`EXIT_INCOMPLETE`].
pub fn exit_code_for(outcome: &Outcome) -> ExitCode {
    if outcome.fully_inspected() {
        ExitCode::from(EXIT_OK)
    } else {
        ExitCode::from(EXIT_INCOMPLETE)
    }
}

/// Die Zusammenfassung eines einzelnen Laufs.
///
/// Jeder Text, der aus der Datei stammt — Pfade, blockierte Treffer,
/// Warnungen —, geht durch [`safe_text`]: ein Dateiname darf Steuerzeichen
/// enthalten, und roh ausgegeben steuern die das Terminal statt dazustehen.
fn report(outcome: &Outcome) {
    println!("Eingabe:            {}", safe_text(&outcome.input));
    println!("Seiten:             {}", outcome.pages);
    if outcome.text_runs > 0 {
        println!("Textzeilen:         {}", outcome.text_runs);
        println!("Treffer gesamt:     {}", outcome.candidates);
    }
    // Die Ansage über abgeschaltete Erkennung steht unten bei den Warnungen —
    // sie gehört aber **neben die Trefferzahl**: „Treffer gesamt: 0“ liest sich
    // sonst als „nichts gefunden“ statt als „nicht gesucht“. Und stdout und
    // stderr landen nicht zwangsläufig an derselben Stelle: `redact-rs … >
    // bericht.txt` behielte ohne diese Zeile nur die harmlose Hälfte.
    //
    // Gelesen wird die Warnung, nicht die Konfiguration: der Satz hat genau
    // eine Quelle (`redact_pipeline::detection_notice`), und die Textmarke
    // davor ist dafür da, ihn wiederzuerkennen.
    if let Some(notice) = outcome
        .warnings
        .iter()
        .find(|w| w.starts_with(redact_pipeline::DETECTION_NOTICE))
    {
        println!("{}", safe_text(notice));
    }
    if outcome.blocked > 0 {
        println!("Durch Negativliste blockiert: {}", outcome.blocked);
        for detail in &outcome.blocked_details {
            println!("  - {}", safe_text(detail));
        }
    }
    match &outcome.review_out {
        Some(path) => {
            println!("Review geschrieben: {}", safe_text(path));
            println!();
            println!("Datei prüfen, `enabled` anpassen und dann anwenden mit:");
            println!(
                "  redact-rs {} --apply-review {}",
                safe_text(&outcome.input),
                safe_text(path)
            );
        }
        None => {
            // Absicht und Ergebnis werden getrennt ausgewiesen. „Schwärzungen“
            // ist die Zahl der *geplanten* Regionen; was davon nachweislich
            // gewirkt hat, steht darunter. Eine Region auf einer nicht
            // vorhandenen Seite, eine, von der `--padding` nichts übrig lässt,
            // und eine, die kein Zeichen getroffen hat, sind drei verschiedene
            // Befunde — und keiner davon ist eine ausgeführte Schwärzung.
            println!("Schwärzungen:       {}", outcome.redactions);
            let unproven = outcome.covered_redactions
                + outcome.degenerate_redactions
                + outcome.missing_page_redactions
                + outcome.off_page_redactions;
            if unproven > 0 {
                println!(
                    "  davon wirksam:      {} (Zeichen entfernt)",
                    outcome.effective_redactions
                );
            }
            if outcome.covered_redactions > 0 {
                println!(
                    "  davon ohne Textfund: {} (Deck-Rechteck gezeichnet, kein Zeichen \
                     entfernt — richtig über Grafik, falsch bei danebenliegenden \
                     Koordinaten)",
                    outcome.covered_redactions
                );
            }
            if outcome.degenerate_redactions > 0 {
                println!(
                    "  davon wirkungslos:  {} (leeres Rechteck nach --padding)",
                    outcome.degenerate_redactions
                );
            }
            if outcome.missing_page_redactions > 0 {
                println!(
                    "  davon wirkungslos:  {} (Seite gibt es in diesem Dokument nicht)",
                    outcome.missing_page_redactions
                );
            }
            // Dieselben Worte wie die Kopfzeile der Oberfläche („n neben der
            // Seite“), und an derselben Stelle wie ihr Nachbar darüber: die
            // Seite gibt es, die Koordinaten treffen sie nur nicht. Bis zu
            // dieser Runde lief der Fall in der Zeile „davon ohne Textfund“
            // mit — also unter „Deck-Rechteck gezeichnet“, was neben dem Blatt
            // nicht stimmt.
            if outcome.off_page_redactions > 0 {
                println!(
                    "  davon wirkungslos:  {} (Rechteck liegt neben der Seite)",
                    outcome.off_page_redactions
                );
            }
            println!("Entfernte Zeichen:  {}", outcome.removed_glyphs);
            println!("Deck-Rechtecke:     {}", outcome.drawn_rects);
            if outcome.removed_annotations > 0 {
                println!("Entfernte Annotationen: {}", outcome.removed_annotations);
                // Die Zahl allein verschweigt, was verlorengegangen ist: mit
                // der Annotation verschwindet ein Formularfeld samt seinem
                // sichtbaren Text. Der Name geht durch `safe_text`, weil er
                // aus der fremden Datei stammt.
                for detail in &outcome.removed_annotation_details {
                    println!(
                        "  - {} (samt allen Erscheinungszustaenden)",
                        redact_core::safe_text(detail)
                    );
                }
            }
            // Ein überschriebenes Bild gehört gemeldet: die Bildpunkte
            // außerhalb der Schwärzung bleiben zwar unverändert (neu kodiert
            // wird verlustfrei mit Flate), die *Datei* ist danach aber eine
            // andere — aus einem JPEG-Stream wird ein Flate-Stream, und die
            // Ausgabe wächst dadurch spürbar.
            //
            // „meist“ und nicht „immer“: gemessen ging ein Flate-Bild von
            // 57 516 auf 49 031 Byte zurück, weil eine große schwarze Fläche
            // sich besser packt als das, was vorher dort stand. Die längere
            // Warnung daneben sagt es genauso; eine Zusammenfassung, die mehr
            // behauptet als die Warnung, ist die falsche von beiden.
            if outcome.redacted_images > 0 {
                println!(
                    "Überschriebene Bilder: {} (neu kodiert: außerhalb der \
                     Schwärzung verlustfrei, Datei meist größer)",
                    outcome.redacted_images
                );
            }
            if outcome.metadata_removed.is_empty() {
                println!("Metadaten:          nichts zu entfernen");
            } else {
                println!(
                    "Metadaten entfernt: {}",
                    safe_text(&outcome.metadata_removed.join(", "))
                );
            }
            if let Some(path) = &outcome.output {
                println!("Ausgabe:            {}", safe_text(path));
            }
            if let Some(path) = &outcome.audit_log {
                println!("Audit-Log:          {}", safe_text(path));
            }
        }
    }
    // Warnungen bleiben Warnungen — aber die, für die der Rückgabewert
    // anspringt, werden auch als solche ausgewiesen. Sonst stünde die
    // wichtigste Zeile des Laufs zwischen Mitteilungen über Bildkodierung.
    for warning in &outcome.warnings {
        let marke = if redact_pipeline::is_coverage_gap(warning) {
            "NICHT GEPRÜFT"
        } else {
            "Warnung"
        };
        eprintln!("{marke}: {}", safe_text(warning));
    }
    let gaps = outcome.coverage_gaps().len();
    if gaps > 0 {
        eprintln!(
            "\n{gaps} Stelle(n) in diesem Dokument wurden nicht durchsucht. Was dort \
             steht, kann nicht geschwärzt worden sein — bitte das Ergebnis dort von \
             Hand prüfen. (Rückgabewert {EXIT_INCOMPLETE}: verarbeitet, aber nicht \
             vollständig geprüft.)"
        );
    }
}

fn list_patterns() -> Result<()> {
    let min = redact_patterns::DEFAULT_MIN_CONFIDENCE;
    println!("Eingebaute Patterns:\n");
    println!(
        "  [an/aus] {:<14} {:<8} {:<8} Beschreibung",
        "id", "mit Kt.", "ohne"
    );
    for def in redact_patterns::builtin_patterns() {
        let state = if def.enabled { "an " } else { "aus" };
        // Zwei Spalten, weil dasselbe Pattern je nach Umfeld unterschiedlich
        // bewertet wird: „mit Kt." gilt, wenn die Gruppe `context` gegriffen
        // hat, „ohne" sonst. Ein `-` heißt: das Pattern kennt keinen Kontext,
        // der eine Wert gilt immer.
        let weak = match def.confidence_without_context {
            Some(value) => format!("{value:.2}"),
            None => "-".to_string(),
        };
        println!(
            "  [{state}]    {:<14} {:<8} {:<8} {}",
            def.id,
            format!("{:.2}", def.confidence),
            weak,
            def.description
        );
    }
    println!(
        "\nAuswahl mit --patterns id1,id2 — standardmäßig ausgeschaltete Patterns\n\
         lassen sich so gezielt einschalten. Umgekehrt nimmt --disable-pattern id\n\
         ein einzelnes Muster aus dem Lauf und lässt die übrigen laufen;\n\
         --no-patterns schaltet die automatische Erkennung ganz ab. Beides steht\n\
         danach in der Zusammenfassung und im Audit-Log — eine Datei ohne\n\
         automatische Suche sieht sonst aus wie eine vollständig geprüfte.\n\n\
         Treffer unterhalb des Mindestvertrauens werden verworfen; die Vorgabe\n\
         ist {min:.2}. Ein Pattern, dessen Spalte „ohne“ darunter liegt, findet\n\
         also nichts, solange kein Schlüsselwort danebensteht — mit\n\
         `--min-confidence 0.25` kommen auch diese Verdachtsfälle durch."
    );
    Ok(())
}

#[cfg(feature = "gui")]
fn start_gui(cli: &Cli, settings: &Settings) -> Result<()> {
    if cli.inputs.len() > 1 {
        return Err(RedactError::Config(
            "Die Oberfläche zeigt ein Dokument. Für mehrere Dateien den Stapel \
             ohne --gui benutzen."
                .into(),
        ));
    }
    // Dieselbe Konfiguration wie ein Lauf auf der Kommandozeile — die
    // Oberfläche analysiert und exportiert damit über dieselbe Kette.
    redact_gui::run(cli.config(settings))
}

#[cfg(not(feature = "gui"))]
fn start_gui(_cli: &Cli, _settings: &Settings) -> Result<()> {
    Err(RedactError::Config(
        "Diese Fassung wurde ohne grafische Oberfläche gebaut \
         (Feature `gui` deaktiviert). Bitte Eingabe- und Ausgabedatei angeben."
            .into(),
    ))
}
