//! `--check-leaks` — die Nachprüfung im ausgelieferten Binary.
//!
//! ## Warum es diesen Schalter gibt
//!
//! Die README erklärt den Abschnitt „Prüfen, ob die Schwärzung gewirkt hat“
//! selbst zur wichtigsten Stelle: `pdftotext … | grep …` gibt an der eigenen
//! Demo-Ausgabe falsche Entwarnung, während Kontoinhaber, Kontonummer,
//! Telefonnummer und Steuer-ID noch in der Datei stehen. Der Ersatz dafür war
//! bis 0.4.0 eine **Bibliotheksfunktion**: `redact_pdf::leaks`. Wer nur das
//! Release heruntergeladen hatte, konnte sie nicht aufrufen — im Archiv liegt
//! kein Quelltext, und die Anleitung verlangte eine Rust-Toolchain, Netzzugang
//! zu crates.io und einen Klon des Repositories. Für ein Werkzeug, dessen
//! erstes Versprechen „keine Cloud, keine Netzverbindung“ lautet, war die
//! Kontrolle damit genau für die Gruppe unerreichbar, für die sie gedacht ist.
//!
//! ## Was hier passiert — und was nicht
//!
//! Dieses Modul **reicht durch**. Gesucht wird nichts eigenes: die Arbeit
//! macht [`redact_pdf::leaks`], dieselbe Funktion, die die Tests von
//! `redact-pdf` als Messgerät benutzen. Hier steht nur, was drumherum gehört —
//! die Datei sicher lesen, das Ergebnis lesbar ausgeben, den richtigen
//! Rückgabewert setzen.
//!
//! ## Die drei Entscheidungen
//!
//! **1. Ein Schalter, kein Unterkommando.** Das Werkzeug hat heute keine
//! Unterkommandos; eines einzuführen hieße, die gewohnte Form
//! (`redact-rs <pdf> …`) für alle anderen Aufrufe zu ändern oder zwei Formen
//! nebeneinander zu haben. Die zu prüfende Datei ist deshalb das gewohnte
//! Positionsargument, der Schalter nimmt die Suchbegriffe:
//! `redact-rs geschwaerzt.pdf --check-leaks "DE89 …" --check-leaks "Max
//! Mustermann"`.
//!
//! **2. Rückgabewert 3 für einen Fund.** Siehe [`crate::EXIT_INCOMPLETE`].
//!
//! **3. Die Suchbegriffe sind Geheimnisse.** Auf der Kommandozeile stehen sie
//! in der Prozessliste (`ps`) und in der Shell-Historie — dasselbe Problem wie
//! beim Passwort, und dieselbe Antwort: es gibt einen Weg daran vorbei.
//! `--check-leaks -` liest die Begriffe zeilenweise von der Standardeingabe:
//!
//! ```console
//! $ redact-rs geschwaerzt.pdf --check-leaks - < begriffe.txt
//! $ printf '%s\n' "$IBAN" "$NAME" | redact-rs geschwaerzt.pdf --check-leaks -
//! ```
//!
//! Ein zweiter Schalter (`--check-leaks-from DATEI`) wäre die andere Form
//! gewesen; `-` für die Standardeingabe ist die übliche Schreibweise, kommt
//! ohne einen weiteren Schalter aus und deckt den Dateifall über die
//! Umlenkung `< datei` mit ab.
//!
//! Was dagegen **nicht** geht: die Ausgabe geheimnisfrei machen. Ein Fund
//! zeigt seine Umgebung aus der Datei, sonst wäre er nicht nachvollziehbar,
//! und die geprüfte Liste steht im Bericht, sonst ließe sich das Ergebnis
//! nicht nachprüfen. Diese Ausgabe gehört behandelt wie das Original.

use std::io::Read;
use std::process::ExitCode;

use redact_core::{safe_path, safe_text, RedactError, Result, MAX_CHECK_NEEDLES};

use crate::cli::Cli;

/// Der Satz, der aus dem Ergebnis keinen Freibrief werden lässt.
///
/// **Die wichtigste Zeile dieses Moduls.** Eine Prüfung, die „nichts
/// gefunden“ meldet, hat nicht bewiesen, dass die Datei sauber ist — sie hat
/// bewiesen, dass *diese* Begriffe nicht vorkommen. Wird dieser Unterschied
/// nicht ausgesprochen, ersetzt `--check-leaks` bloß die alte falsche
/// Entwarnung (`pdftotext | grep`) durch eine neue, die genauer aussieht.
///
/// Der Satz steht auf **stdout**, nicht auf stderr, und zwar aus demselben
/// Grund wie die Ansage über abgeschaltete Erkennung in `crate::report`:
/// `redact-rs … > bericht.txt` behielte sonst genau die harmlose Hälfte.
const NO_CLEAN_BILL: &str = "\
Das heißt NICHT, dass in der Datei nichts mehr steht. Geprüft wurde genau diese
Liste. Was nicht darin steht — ein zweiter Name, eine weitere Kontonummer, eine
Schreibweise mit anderen Leerzeichen, Text in einem Rasterbild, Text in einer
Schrift ohne brauchbare Zeichenzuordnung —, ist damit nicht geprüft. Die Liste
zu schreiben bleibt Handarbeit, und die Sichtprüfung des Ergebnisses ersetzt
sie nicht.";

/// Obergrenze für die Begriffsliste auf der Standardeingabe.
///
/// Dieselbe Grenze wie für jede andere von Hand gepflegte Hilfsdatei
/// ([`redact_core::MAX_AUX_FILE_BYTES`], 16 MB) — eine Liste von
/// Suchbegriffen ist genau das. Ohne Grenze bestimmte die Gegenseite einer
/// Pipe, wie viel Arbeitsspeicher der Lauf belegt.
const MAX_NEEDLE_INPUT: u64 = redact_core::MAX_AUX_FILE_BYTES;

/// Führt die Prüfung aus. Rückgabe: [`crate::EXIT_OK`] oder
/// [`crate::EXIT_INCOMPLETE`]; jeder Fehler wandert als [`RedactError`] nach
/// oben und wird dort nach Art unterschieden (Konfiguration ⇒ 2, sonst 1).
pub fn run(cli: &Cli) -> Result<ExitCode> {
    let path = single_input(cli)?;
    let needles = needles(cli)?;

    // Erst fragen, dann lesen: Typ (gewöhnliche Datei — eine benannte Pipe
    // liefert endlos) und Größe vor dem ersten gelesenen Byte. Dieselbe Tür,
    // durch die auch jede zu schwärzende Datei geht.
    let bytes = redact_pipeline::read_input(path, cli.max_input_mb.saturating_mul(1024 * 1024))?;

    // Und dieselbe Prüfung des Inhalts: %PDF-Header, Vorprüfung gegen
    // Dekompressionsbomben, Ablehnung verschlüsselter Dateien. Das ist hier
    // kein Selbstzweck:
    //
    // * `leaks` packt jeden Stream aus, den es findet, und zwar **ohne
    //   Budget** — die Vorprüfung ist die Stelle, an der eine kleine Datei
    //   mit riesigem Inhalt abgelehnt wird, bevor sie ausgepackt wird.
    // * In einer verschlüsselten Datei stehen die Zeichenketten verschlüsselt.
    //   Eine Bytesuche fände darin nichts — und „nichts gefunden“ wäre die
    //   falscheste aller Antworten. Lieber gar keine Auskunft als eine
    //   erfundene.
    redact_pdf::document::load_from_bytes_with_limits(&bytes, &cli.limits()).map_err(
        |e| match e {
            RedactError::Pdf(msg) if msg.contains("verschlüsselt") => RedactError::Pdf(format!(
                "{}: {msg} In einer verschlüsselten Datei stehen die Zeichenketten \
                 verschlüsselt; eine Suche darin fände auch dann nichts, wenn das \
                 Geheimnis noch darin steht.",
                safe_path(path)
            )),
            RedactError::Pdf(msg) => RedactError::Pdf(format!("{}: {msg}", safe_path(path))),
            other => other,
        },
    )?;

    report(cli, path, &bytes, &needles)
}

/// Genau eine Datei — kein Stapel, kein Verzeichnis.
///
/// Der Stapelbetrieb schreibt je Datei ein Ergebnis daneben; hier gibt es kein
/// Ergebnis zum Danebenlegen, sondern eine Frage und eine Antwort. Zwei
/// Dateien in einem Aufruf hätten zwei Antworten und einen Rückgabewert —
/// welche der beiden er meint, ließe sich nicht sagen. Zwei Aufrufe können es.
fn single_input(cli: &Cli) -> Result<&std::path::Path> {
    match cli.inputs.as_slice() {
        [one] => Ok(one.as_path()),
        [] => Err(RedactError::Config(
            "--check-leaks braucht die zu prüfende PDF-Datei: \
             `redact-rs geschwaerzt.pdf --check-leaks \"DE89 …\"`"
                .into(),
        )),
        many => Err(RedactError::Config(format!(
            "--check-leaks prüft genau eine Datei, hier stehen {}. Der Rückgabewert \
             könnte sonst nur für eine davon gelten. Bitte je Datei ein Aufruf.",
            many.len()
        ))),
    }
}

/// Die Suchbegriffe dieses Aufrufs, in der Reihenfolge der Angabe.
///
/// `-` steht für die Standardeingabe: eine Zeile, ein Begriff. Leere Zeilen
/// werden übergangen — eine Datei endet üblicherweise mit einem Zeilenumbruch,
/// und ein leerer Begriff passte auf jede Datei, meldete also „nicht
/// gefunden“ und wäre eine Entwarnung ohne Prüfung.
fn needles(cli: &Cli) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut from_stdin = false;
    for arg in &cli.check_leaks {
        if arg == "-" {
            // Zweimal `-` liest nicht zweimal: die Standardeingabe ist beim
            // zweiten Mal leer, und „0 Begriffe“ wäre die Folge.
            if from_stdin {
                continue;
            }
            from_stdin = true;
            out.extend(read_stdin_needles()?);
        } else if arg.trim().is_empty() {
            return Err(RedactError::Config(
                "--check-leaks mit leerem Text: ein leerer Suchbegriff steht in jeder \
                 Datei und in keiner, die Prüfung hätte keine Aussage."
                    .into(),
            ));
        } else {
            out.push(arg.clone());
        }
    }
    if out.is_empty() {
        return Err(RedactError::Config(
            "--check-leaks ohne Suchbegriff. Zu prüfen ist, ob ein *bestimmter* Text \
             noch in der Datei steht — welcher, kann nur der Mensch sagen, der das \
             Original kennt."
                .into(),
        ));
    }
    // Die Decke selbst liegt in `redact-core` ([`MAX_CHECK_NEEDLES`], mit
    // Messung und Begründung) — dieselbe Zahl, mit der die Oberfläche nach
    // dem Export nachprüft. Hier steht nur, was passiert, wenn sie greift.
    if out.len() > MAX_CHECK_NEEDLES {
        return Err(RedactError::Config(format!(
            "--check-leaks mit {} Suchbegriffen; mehr als {MAX_CHECK_NEEDLES} nimmt der Lauf \
             nicht an. Die Prüfung kostet je Begriff einen Vergleich über die ganze \
             Datei — bei dieser Zahl liefe sie so lange, dass sie wie ein Hänger \
             aussieht. Teilen Sie die Liste auf und rufen Sie mehrmals auf; jeder Lauf \
             meldet für sich.",
            out.len()
        )));
    }
    Ok(out)
}

/// Liest die Begriffsliste von der Standardeingabe — begrenzt.
fn read_stdin_needles() -> Result<Vec<String>> {
    let mut text = String::new();
    std::io::stdin()
        .lock()
        .take(MAX_NEEDLE_INPUT.saturating_add(1))
        .read_to_string(&mut text)
        .map_err(|e| {
            RedactError::Config(format!(
                "--check-leaks -: Standardeingabe nicht lesbar: {e}"
            ))
        })?;
    if text.len() as u64 > MAX_NEEDLE_INPUT {
        return Err(RedactError::Config(format!(
            "--check-leaks -: die Standardeingabe liefert mehr als {} MB. Erwartet wird \
             eine von Hand geschriebene Liste von Suchbegriffen, eine je Zeile.",
            MAX_NEEDLE_INPUT / (1024 * 1024)
        )));
    }
    // `str::lines` trennt an `\n` **und** an `\r\n` und lässt den
    // Zeilenumbruch weg — eine unter Windows geschriebene Liste braucht
    // dafür also nichts Eigenes.
    Ok(text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect())
}

/// Sucht und berichtet.
///
/// Jede Zeile geht durch [`safe_text`]: die Fundstellen tragen Text **aus der
/// geprüften Datei** (Objektnamen, Umgebung des Fundes), und der kommt von
/// außen. Roh ausgegeben könnte er das Terminal steuern statt dazustehen —
/// ausgerechnet in der Ausgabe, an der jemand ablesen will, ob eine Datei
/// sauber ist.
fn report(cli: &Cli, path: &std::path::Path, bytes: &[u8], needles: &[String]) -> Result<ExitCode> {
    let name = safe_path(path);
    if !cli.quiet {
        println!("Geprüft: {name} ({} Byte)", bytes.len());
    }

    // Ein Durchgang durch die Datei für alle Begriffe: das Entpacken der
    // Streams und das Parsen des Objektgraphen hängt an der Datei, nicht am
    // Suchbegriff. Zehn Begriffe kosten sonst zehnmal dieselbe Arbeit.
    let by_needle: Vec<&str> = needles.iter().map(String::as_str).collect();
    let mut leaking = 0usize;
    for (needle, hits) in needles
        .iter()
        .zip(redact_pdf::leaks_many(bytes, &by_needle))
    {
        if hits.is_empty() {
            if !cli.quiet {
                println!("  nicht gefunden: {}", safe_text(needle));
            }
        } else {
            leaking += 1;
            // Auch bei `--quiet`: ein Fund ist die Nachricht, wegen der dieser
            // Schalter existiert. Stumm bliebe nur der Rückgabewert.
            println!(
                "  GEFUNDEN ({} Fundstelle(n)): {}",
                hits.len(),
                safe_text(needle)
            );
            for hit in &hits {
                println!("      {}", safe_text(hit));
            }
        }
    }

    if leaking > 0 {
        println!();
        // Ein einzelner Begriff bekommt einen eigenen Satz: „1 von 1
        // Suchbegriffen stehen“ ist weder Zahl noch Deutsch.
        let befund = match needles.len() {
            1 => "der Suchbegriff steht".to_string(),
            n if leaking == 1 => format!("1 von {n} Suchbegriffen steht"),
            n => format!("{leaking} von {n} Suchbegriffen stehen"),
        };
        println!(
            "Ergebnis: {befund} noch in der Datei. Diese Datei ist nicht geschwärzt — sie darf \
             so nicht weitergegeben werden. (Rückgabewert {}.)",
            crate::EXIT_INCOMPLETE
        );
        return Ok(ExitCode::from(crate::EXIT_INCOMPLETE));
    }

    if !cli.quiet {
        println!();
        match needles.len() {
            1 => println!("Ergebnis: der Suchbegriff steht nicht mehr in der Datei."),
            n => println!("Ergebnis: keiner der {n} Suchbegriffe steht noch in der Datei."),
        }
        println!("{NO_CLEAN_BILL}");
    }
    Ok(ExitCode::from(crate::EXIT_OK))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn cli(args: &[&str]) -> Cli {
        Cli::parse_from(args)
    }

    #[test]
    fn one_file_is_required() {
        assert!(single_input(&cli(&["redact-rs", "a.pdf"])).is_ok());
        assert!(single_input(&cli(&["redact-rs"])).is_err());
        let err = single_input(&cli(&["redact-rs", "a.pdf", "b.pdf"]))
            .expect_err("zwei Dateien, ein Rückgabewert");
        assert!(err.to_string().contains("genau eine Datei"), "{err}");
    }

    #[test]
    fn needles_keep_their_order_and_their_commas() {
        let cli = cli(&[
            "redact-rs",
            "a.pdf",
            "--check-leaks",
            "Mustermann, Max",
            "--check-leaks",
            "DE89 3704",
        ]);
        assert_eq!(
            needles(&cli).unwrap(),
            vec!["Mustermann, Max".to_string(), "DE89 3704".to_string()],
            "ein Komma gehört zum Begriff und trennt nicht"
        );
    }

    #[test]
    fn an_empty_needle_is_refused() {
        let err = needles(&cli(&["redact-rs", "a.pdf", "--check-leaks", "  "]))
            .expect_err("ein leerer Begriff ist keine Prüfung");
        assert!(err.to_string().contains("leerem Text"), "{err}");
    }

    // `the_caveat_names_what_was_not_checked` stand hier und prüfte
    // `NO_CLEAN_BILL` gegen sich selbst — eine Konstante enthält ihre eigenen
    // Teilzeichenketten, das gilt immer. Die Sache selbst ist am gebauten
    // Binary abgedeckt: `check_leaks.rs::a_clean_run_says_what_it_does_not_prove`
    // prüft, dass der Vorbehalt in der Ausgabe steht, dass er den Grund nennt
    // und dass er auf stdout und nicht auf stderr landet.

    /// Eine Begriffsliste, die der Lauf nicht in vertretbarer Zeit prüfen
    /// kann, wird abgelehnt statt still stundenlang gerechnet.
    #[test]
    fn too_many_needles_are_refused_with_a_reason() {
        let mut args = vec!["redact-rs".to_string(), "a.pdf".to_string()];
        for i in 0..=MAX_CHECK_NEEDLES {
            args.push("--check-leaks".to_string());
            args.push(format!("Begriff{i}"));
        }
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let err = needles(&cli(&borrowed)).expect_err("einer zu viel");
        let text = err.to_string();
        assert!(text.contains(&MAX_CHECK_NEEDLES.to_string()), "{text}");
        assert!(text.contains("Hänger"), "der Grund fehlt: {text}");

        // Gegenprobe: genau an der Grenze geht es durch. Eine Decke, die
        // schon den erlaubten Fall ablehnt, wäre keine Härtung.
        let borrowed: Vec<&str> = borrowed[..borrowed.len() - 2].to_vec();
        assert_eq!(needles(&cli(&borrowed)).unwrap().len(), MAX_CHECK_NEEDLES);
    }
    /// Die Decke ist **eine** Zahl — im Code, im Hilfetext und in der Doku.
    ///
    /// Bis hierher war der Test zur Decke aus der Konstante abgeleitet und
    /// hätte bei jedem Wert bestanden; die Literale „1 000“ in `--help`,
    /// README, SECURITY.md und CHANGELOG band nichts. Dieser Test liest die
    /// vier Stellen und verlangt an jeder die formatierte Zahl aus
    /// [`MAX_CHECK_NEEDLES`] — mit dem Satz drumherum, damit ein zufälliges
    /// Vorkommen der Zahl an anderer Stelle nicht als Treffer zählt.
    ///
    /// Zwei Fassungen der Zahl sind erlaubt und beide werden geprüft: die
    /// mit Tausendertrennzeichen im Fließtext („1 000“) und die nackte in
    /// der zitierten Fehlermeldung („mehr als 1000 nimmt der Lauf nicht an“)
    /// — so druckt sie `needles` wirklich.
    #[test]
    fn the_needle_ceiling_is_one_number_in_code_help_and_docs() {
        use clap::CommandFactory;

        /// „1 000“ statt „1000“ — Leerzeichen als Tausendertrenner, wie die
        /// Doku es schreibt.
        fn mit_tausendertrenner(n: usize) -> String {
            let ziffern = n.to_string();
            let mut aus = String::new();
            for (i, z) in ziffern.chars().enumerate() {
                if i > 0 && (ziffern.len() - i).is_multiple_of(3) {
                    aus.push(' ');
                }
                aus.push(z);
            }
            aus
        }
        assert_eq!(mit_tausendertrenner(1_000), "1 000");
        assert_eq!(mit_tausendertrenner(500), "500");
        assert_eq!(mit_tausendertrenner(1_234_567), "1 234 567");

        /// Zeilenumbrüche der Markdown-Quellen glätten, damit ein Umbruch
        /// mitten im Satz nicht zählt.
        fn glatt(text: &str) -> String {
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        }

        let formatiert = mit_tausendertrenner(MAX_CHECK_NEEDLES);
        let nackt = MAX_CHECK_NEEDLES.to_string();
        let wurzel = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let lies = |name: &str| {
            glatt(
                &std::fs::read_to_string(wurzel.join(name)).unwrap_or_else(|e| {
                    panic!("{name} lesbar: {e}");
                }),
            )
        };

        // Der Hilfetext, so wie clap ihn druckt.
        let help = glatt(&Cli::command().render_long_help().to_string());
        assert!(
            help.contains(&format!("Höchstens {formatiert} Begriffe je Aufruf")),
            "--help nennt die Decke nicht als {formatiert}"
        );

        // README: der Absatz zur Decke, samt der zitierten Fehlermeldung und
        // dem ersten abgelehnten Begriff.
        let readme = lies("README.md");
        for satz in [
            format!("**Höchstens {formatiert} Begriffe je Aufruf.**"),
            format!(
                "Der {}. Begriff",
                mit_tausendertrenner(MAX_CHECK_NEEDLES + 1)
            ),
            format!(
                "mit {} Suchbegriffen; mehr als {nackt} nimmt der Lauf nicht an",
                MAX_CHECK_NEEDLES + 1
            ),
            format!("mit {formatiert} Zeilen läuft derselbe Aufruf durch"),
        ] {
            assert!(readme.contains(&satz), "README.md ohne „{satz}“");
        }

        // SECURITY.md: die Zeile in der Grenzentabelle.
        let security = lies("SECURITY.md");
        let zeile =
            format!("| **Suchbegriffe je `--check-leaks`-Lauf** | **{formatiert}** | **fest** |");
        assert!(security.contains(&zeile), "SECURITY.md ohne „{zeile}“");

        // CHANGELOG: die Einführung der Decke und ihre Erwähnung im
        // Hilfetext-Eintrag.
        let changelog = lies("CHANGELOG.md");
        for satz in [
            format!("Obergrenze von {formatiert} Begriffen"),
            format!("Decke von {formatiert} Begriffen je `--check-leaks`-Lauf"),
        ] {
            assert!(changelog.contains(&satz), "CHANGELOG.md ohne „{satz}“");
        }

        // Und die Fehlermeldung selbst druckt dieselbe Zahl — sonst zitierte
        // die README einen Text, den das Programm nie ausgibt.
        let mut args = vec!["redact-rs".to_string(), "a.pdf".to_string()];
        for i in 0..=MAX_CHECK_NEEDLES {
            args.push("--check-leaks".to_string());
            args.push(format!("Begriff{i}"));
        }
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let meldung = needles(&cli(&borrowed))
            .expect_err("einer zu viel")
            .to_string();
        assert!(
            meldung.contains(&format!(
                "mit {} Suchbegriffen; mehr als {nackt} nimmt der Lauf nicht an",
                MAX_CHECK_NEEDLES + 1
            )),
            "die Fehlermeldung lautet anders als in der README zitiert: {meldung}"
        );
    }
}
