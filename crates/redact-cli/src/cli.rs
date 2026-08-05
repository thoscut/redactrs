//! Kommandozeilen-Definition.

use std::path::{Path, PathBuf};

use clap::{ArgAction, Parser, ValueEnum};
use redact_pipeline::{Config, Secret, Settings};

/// Umgebungsvariable für das Passwort verschlüsselter PDFs.
///
/// Der Weg an der Prozessliste vorbei — siehe [`Cli::password`].
pub const PASSWORD_ENV: &str = "REDACT_RS_PASSWORD";

/// Lokales Schwärzen sensibler Daten in PDF-Dokumenten.
///
/// Ohne Eingabedatei (oder mit `--gui`) startet die grafische Oberfläche.
#[derive(Debug, Parser)]
#[command(
    name = "redact-rs",
    version,
    about = "Schwärzt sensible Daten in PDFs – lokal, ohne Cloud.",
    long_about = None,
    after_help = examples(),
)]
pub struct Cli {
    /// Eingabe-PDFs oder Verzeichnisse.
    ///
    /// Mehrere Angaben und Verzeichnisse werden als Stapel abgearbeitet: je
    /// Datei ein Ergebnis neben der Eingabe, am Ende eine Zusammenfassung.
    /// Eine Datei, die scheitert, beendet den Stapel nicht — sie wird gemeldet,
    /// der Rest läuft weiter, und der Rückgabewert sagt, ob alles gut ging.
    /// Aus einem Verzeichnis werden die `*.pdf` der obersten Ebene genommen,
    /// ohne die bereits geschwärzten (Namenszusatz).
    #[arg(value_name = "PDF")]
    pub inputs: Vec<PathBuf>,

    /// Ausgabe-PDF. Ohne Angabe wird neben der Eingabedatei gespeichert,
    /// mit dem Zusatz aus `--output-suffix` im Dateinamen.
    ///
    /// Im Stapelbetrieb nicht erlaubt — ein Name für viele Dateien hieße,
    /// dass jedes Ergebnis das vorige überschreibt.
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Namenszusatz für die Ausgabedatei, wenn `-o` fehlt.
    ///
    /// Ohne Angabe gilt der Wert aus der Einstellungsdatei, sonst
    /// `_geschwaerzt`.
    #[arg(long, value_name = "TEXT")]
    pub output_suffix: Option<String>,

    /// Passwort eines verschlüsselten PDFs.
    ///
    /// **Vorsicht:** ein Passwort auf der Kommandozeile steht in der
    /// Prozessliste (`ps`) und in der Shell-Historie und ist damit für andere
    /// Konten auf derselben Maschine lesbar. Ohne diesen Schalter geht es
    /// über die Umgebungsvariable `REDACT_RS_PASSWORD`, die keinen der beiden
    /// Wege nimmt; in der Oberfläche fragt ein Fenster danach. Ohne jedes
    /// Passwort bleibt es bei der Ablehnung verschlüsselter Dateien.
    #[arg(long, value_name = "PW")]
    pub password: Option<String>,

    /// Vorhandene Ausgabedateien überschreiben.
    #[arg(short, long)]
    pub force: bool,

    /// Zu verwendende Patterns (kommagetrennt, z.B. `iban_de,bic`).
    /// Ohne Angabe werden die standardmäßig aktiven Patterns benutzt.
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub patterns: Vec<String>,

    /// Keine Patterns anwenden — kein einziger automatischer Treffer.
    ///
    /// Geschwärzt wird dann nur, was `--manual-regions`, `--apply-review` oder
    /// die Buchungsliste nennen. Der Lauf sagt es in der Zusammenfassung und
    /// schreibt es ins Audit-Log: eine Datei, die so entstanden ist, sieht in
    /// jeder Zahl aus wie eine vollständig geprüfte.
    #[arg(long, conflicts_with_all = ["patterns", "patterns_config"])]
    pub no_patterns: bool,

    /// Einzelnes Muster abschalten (mehrfach oder kommagetrennt).
    ///
    /// Die übrigen bleiben an: `--disable-pattern date_de` nimmt genau das
    /// Datumsmuster aus dem Lauf. Ein unbekannter Name beendet den Lauf mit
    /// Rückgabewert 2 und nennt die gültigen — stillschweigend übergangen sähe
    /// er aus wie eine Abschaltung und wäre keine.
    ///
    /// Ohne Angabe gilt die Liste aus der Einstellungsdatei.
    #[arg(long = "disable-pattern", value_name = "ID", value_delimiter = ',', num_args = 1..)]
    pub disable_pattern: Vec<String>,

    /// Eigene Pattern-Konfiguration (YAML oder JSON).
    #[arg(long, value_name = "DATEI")]
    pub patterns_config: Option<PathBuf>,

    /// Mindestvertrauen eines Treffers (0.0 … 1.0). Ohne Angabe gilt der Wert
    /// aus der Einstellungsdatei, sonst 0.5.
    ///
    /// Ein Pattern mit einer Gruppe `context` bewertet denselben Treffer je
    /// nach Umfeld unterschiedlich: „Kto. 532013000“ ist eine Kontonummer,
    /// eine nackte Ziffernkette bestenfalls ein Verdacht. Wer auch die
    /// Verdachtsfälle sehen will, senkt die Schwelle (`--min-confidence 0.25`);
    /// `--list-patterns` zeigt beide Werte je Pattern.
    #[arg(long, value_name = "WERT")]
    pub min_confidence: Option<f32>,

    /// Buchungsliste (CSV) mit Positiv- und Negativeinträgen.
    #[arg(long, value_name = "CSV")]
    pub booking_list: Option<PathBuf>,

    /// Manuell festgelegte Regionen (JSON).
    #[arg(long, value_name = "JSON")]
    pub manual_regions: Option<PathBuf>,

    /// Nur analysieren: gefundene Regionen exportieren, nichts schwärzen.
    #[arg(long)]
    pub review: bool,

    /// Zieldatei für den Review-Export (Standard: `<input>_review.json`).
    #[arg(long, value_name = "JSON")]
    pub review_out: Option<PathBuf>,

    /// Geprüfte Review-Datei anwenden (überspringt die Analyse).
    #[arg(long, value_name = "JSON", conflicts_with = "review")]
    pub apply_review: Option<PathBuf>,

    /// Eine Review-Datei ohne Prüfsumme trotzdem anwenden.
    ///
    /// Eine Review-Datei nennt nur Koordinaten. Ohne die Prüfsumme ihres
    /// Dokuments lässt sich nicht feststellen, ob sie zu dieser Eingabe
    /// gehört — auf ein fremdes PDF angewendet lägen die Schwärzungen an
    /// beliebigen Stellen, das Ergebnis sähe geschwärzt aus und wäre es nicht.
    /// Deshalb wird sie sonst abgelehnt.
    #[arg(long)]
    pub allow_unverified_review: bool,

    /// Audit-Log schreiben.
    #[arg(long, value_name = "JSON")]
    pub audit_log: Option<PathBuf>,

    /// Art der Schwärzung.
    #[arg(long, value_enum, default_value_t = ActionArg::Blackout)]
    pub action: ActionArg,

    /// Ersatztext für `--action replace`.
    ///
    /// Der Wert gilt auch dann, wenn `--action` etwas anderes sagt: in der
    /// Oberfläche lässt sich die Art je Treffer umstellen, und dann soll der
    /// hier genannte Text erscheinen und nicht die Vorgabe. Die Vorgabe selbst
    /// steht in [`redact_core::DEFAULT_REPLACEMENT`] — **nicht** als Literal an
    /// dieser Stelle, denn sonst hätte die Oberfläche wieder eine zweite.
    #[arg(long, value_name = "TEXT", default_value = redact_core::DEFAULT_REPLACEMENT)]
    pub replace_with: String,

    /// Zusätzlicher Rand um jede Schwärzung, in Punkt.
    ///
    /// Ohne Angabe gilt der Wert aus der Einstellungsdatei, sonst 1.0.
    #[arg(long)]
    pub padding: Option<f64>,

    /// Bilder, die sich nicht dekodieren lassen, durchgehen lassen.
    ///
    /// **Unsicher.** JPEG-2000, CCITT-Fax und defekte Bildstreams kann
    /// redact-rs nicht öffnen; ein Schwärzungsbereich darauf lässt sich dann
    /// nur *überdecken*, die Bildpunkte bleiben in der Datei. Ohne diesen
    /// Schalter bricht der Lauf in dem Fall ab — lieber ein Fehler als eine
    /// Datei, in der die Schwärzung nur obenauf liegt. Mit ihm entsteht eine
    /// Ausgabe, deren Bilder ungeschwärzt sind; die Warnung dazu steht in der
    /// Zusammenfassung und im Audit-Log.
    #[arg(long)]
    pub allow_undecodable_images: bool,

    /// Obergrenze für die Summe aller entpackten Streams einer Eingabedatei.
    ///
    /// Schutz gegen Dekompressionsbomben: eine kleine Datei, die sich beim
    /// Öffnen vervielfacht.
    #[arg(long, value_name = "MB", default_value_t = 1024)]
    pub max_decompressed_mb: u64,

    /// Davon: Obergrenze für die Streams, die geparst werden
    /// (Seiteninhalt und Objekt-Streams).
    ///
    /// Aus einem Byte Seiteninhalt werden beim Parsen rund 60 Byte
    /// Arbeitsspeicher — deshalb ist diese Grenze deutlich enger.
    #[arg(long, value_name = "MB", default_value_t = 16)]
    pub max_parsed_mb: u64,

    /// Obergrenze für die gleichzeitig gehaltenen **dekodierten** Bildbytes.
    ///
    /// Eine Schwärzung auf einem Bild überschreibt dessen Bildpunkte, dafür
    /// muss das Bild nach RGBA8 ausgepackt werden: 4 Byte je Bildpunkt. Ein
    /// gewöhnlicher Schwarzweiß-Scan (`/BitsPerComponent 1`) wächst dabei um
    /// den Faktor 32 — deshalb greifen `--max-decompressed-mb` und
    /// `--max-parsed-mb` hier nicht, die zählen die Rohbytes des Streams.
    ///
    /// Reicht die Grenze nicht, bricht der Lauf mit einer Meldung ab, statt
    /// die Speicheranforderung scheitern zu lassen. Sie deckt nicht den
    /// gesamten Prozessbedarf ab: beim Umkodieren *eines* Bildes entstehen
    /// vorübergehend rund anderthalb weitere Kopien.
    #[arg(long, value_name = "MB", default_value_t = 256)]
    pub max_image_mb: u64,

    /// Obergrenze für die Eingabedatei selbst.
    ///
    /// Die Datei wird in einem Stück gelesen — die Prüfsumme im Audit-Log soll
    /// die der verarbeiteten Bytes sein. Ohne diese Grenze bestimmte damit die
    /// Datei, wie viel Arbeitsspeicher der Lauf belegt: eine dünn belegte
    /// Datei mit 6 GB Nennlänge belegt 4 kB auf der Platte und 6 GB im
    /// Speicher. Im Stapelbetrieb genügt dafür eine Datei im Verzeichnis.
    #[arg(long, value_name = "MB", default_value_t = redact_pipeline::DEFAULT_MAX_INPUT_BYTES / (1024 * 1024))]
    pub max_input_mb: u64,

    /// Obergrenze für die Zahl der Trefferkandidaten in einer Datei.
    ///
    /// Die Konfliktauflösung wächst quadratisch mit dieser Zahl; ohne Grenze
    /// genügt eine kleine Datei mit sehr vielen Treffern, um die Maschine
    /// stundenlang zu beschäftigen.
    #[arg(long, value_name = "N", default_value_t = redact_pipeline::DEFAULT_MAX_CANDIDATES)]
    pub max_candidates: usize,

    /// Grafische Oberfläche starten.
    ///
    /// `hide` in einer Fassung ohne das Feature `gui` (Aufgabe #61): der
    /// musl-Build entsteht mit `--no-default-features` und *hat* keine
    /// Oberfläche — ein Schalter im Hilfetext, der nur mit einer
    /// Fehlermeldung enden kann, ist eine Falle. Der Schalter selbst bleibt
    /// erhalten, damit `--gui` dort weiterhin mit Rückgabewert 2 und einem
    /// erklärenden Satz endet statt mit „unknown argument“.
    #[arg(long, hide = !cfg!(feature = "gui"))]
    pub gui: bool,

    /// Verfügbare Patterns auflisten und beenden.
    #[arg(long)]
    pub list_patterns: bool,

    /// Beispiel-PDF (Kontoauszug) schreiben und beenden.
    #[arg(long, value_name = "PDF")]
    pub write_demo: Option<PathBuf>,

    /// Zusammenfassung als JSON auf stdout ausgeben.
    #[arg(long)]
    pub json: bool,

    /// Weniger Ausgabe.
    #[arg(short, long, action = ArgAction::SetTrue)]
    pub quiet: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ActionArg {
    /// Schwarzes Rechteck.
    Blackout,
    /// Weißes Rechteck.
    Whiteout,
    /// Weißes Rechteck mit Ersatztext.
    Replace,
}

impl Cli {
    /// Grenzen, mit denen fremde PDFs gelesen werden.
    pub fn limits(&self) -> redact_pdf::document::Limits {
        let mb = |n: u64| n.saturating_mul(1024 * 1024);
        redact_pdf::document::Limits {
            max_decompressed_bytes: mb(self.max_decompressed_mb),
            max_parsed_bytes: mb(self.max_parsed_mb),
            ..redact_pdf::document::Limits::default()
        }
    }

    /// Das Passwort dieses Aufrufs.
    ///
    /// Rangfolge: `--password` schlägt `REDACT_RS_PASSWORD`. Der Schalter ist
    /// der bequeme, die Variable der unauffällige Weg — beide sind hier, weil
    /// die Kommandozeile in der Prozessliste steht und die Umgebung nicht.
    fn password(&self) -> Option<Secret> {
        self.password
            .clone()
            .or_else(|| std::env::var(PASSWORD_ENV).ok())
            .filter(|pw| !pw.is_empty())
            .map(Secret::new)
    }

    /// Die Einstellungen dieses Aufrufs als [`Config`] der Verarbeitungskette.
    ///
    /// **Ein** Bauplatz für beide Programme: `redact-rs auszug.pdf …` und
    /// `redact-rs --gui auszug.pdf …` bekommen dieselbe Konfiguration, also
    /// gelten `--patterns-config`, `--no-patterns`, `--min-confidence`,
    /// `--manual-regions` und `--padding` auch in der Oberfläche. Vorher kannte
    /// sie nichts davon und polsterte fest mit 1,0.
    ///
    /// **Und der einzige Ort, an dem die Rangfolge gilt**: Kommandozeile
    /// schlägt Einstellungsdatei schlägt Vorgabe. Die betroffenen Schalter
    /// sind deshalb `Option` bzw. eine leere Liste — `None` heißt „nicht
    /// angegeben“, und nur dann kommt der Wert aus `settings`. Ein
    /// `default_value` in der clap-Definition würde genau diese Unterscheidung
    /// zerstören.
    ///
    /// Ohne Eingabedatei (nur die Oberfläche kommt so weit) bleibt
    /// [`Config::input`] leer und wird beim Öffnen eines Dokuments gesetzt.
    pub fn config(&self, settings: &Settings) -> Config {
        Config {
            input: self.inputs.first().cloned().unwrap_or_default(),
            output: self.output.clone(),
            output_suffix: self
                .output_suffix
                .clone()
                .unwrap_or_else(|| settings.output_suffix.clone()),
            force: self.force,
            patterns: if self.patterns.is_empty() {
                settings.patterns.clone()
            } else {
                self.patterns.clone()
            },
            no_patterns: self.no_patterns,
            // Wie bei `patterns`: eine Angabe auf der Kommandozeile **ersetzt**
            // die Liste aus der Datei. Nur so bleibt eine dort eingetragene
            // Abschaltung widerrufbar (siehe `Settings::disabled_patterns`).
            disabled_patterns: if self.disable_pattern.is_empty() {
                settings.disabled_patterns.clone()
            } else {
                self.disable_pattern.clone()
            },
            patterns_config: self.patterns_config.clone(),
            min_confidence: self.min_confidence.or(settings.min_confidence),
            booking_list: self.booking_list.clone(),
            manual_regions: self.manual_regions.clone(),
            review: self.review,
            review_out: self.review_out.clone(),
            apply_review: self.apply_review.clone(),
            allow_unverified_review: self.allow_unverified_review,
            audit_log: self.audit_log.clone(),
            action: self.action.to_action(&self.replace_with),
            // Zusätzlich zu `action`, nicht statt: `Action::Blackout` hat kein
            // Textfeld, `--action blackout --replace-with X` verlor X sonst
            // schon hier. Siehe [`Config::replace_with`].
            replace_with: self.replace_with.clone(),
            padding: self.padding.unwrap_or(settings.padding),
            allow_undecodable_images: self.allow_undecodable_images,
            max_decoded_image_bytes: self.max_image_mb.saturating_mul(1024 * 1024),
            limits: self.limits(),
            max_input_bytes: self.max_input_mb.saturating_mul(1024 * 1024),
            max_candidates: self.max_candidates,
            password: self.password(),
            theme: settings.theme.clone(),
        }
    }

    /// Wie [`Cli::config`], aber für eine bestimmte Datei des Stapels.
    pub fn config_for(&self, settings: &Settings, input: &Path) -> Config {
        Config {
            input: input.to_path_buf(),
            ..self.config(settings)
        }
    }
}

impl ActionArg {
    pub fn to_action(self, replacement: &str) -> redact_core::Action {
        match self {
            ActionArg::Blackout => redact_core::Action::Blackout,
            ActionArg::Whiteout => redact_core::Action::Whiteout,
            ActionArg::Replace => redact_core::Action::Replace(replacement.to_string()),
        }
    }
}

/// Der Abschnitt zur Oberfläche — nur in einer Fassung, die eine hat.
///
/// Aufgabe #61: der musl-Build entsteht mit `--no-default-features`; dort darf
/// die Oberfläche weder im Beispielteil noch bei den Schaltern auftauchen.
#[cfg(feature = "gui")]
const GUI_EXAMPLE: &str = "\
  # Grafische Oberfläche
  redact-rs --gui kontoauszug.pdf

";
#[cfg(not(feature = "gui"))]
const GUI_EXAMPLE: &str = "\
  # (Diese Fassung wurde ohne grafische Oberfläche gebaut.)

";

/// Der Text unter dem Hilfetext.
pub fn examples() -> String {
    format!(
        "\
Beispiele:
  # Automatisch schwärzen (Standard-Patterns)
  redact-rs kontoauszug.pdf -o geschwaerzt.pdf

  # Nur bestimmte Patterns
  redact-rs kontoauszug.pdf -o geschwaerzt.pdf --patterns iban_de,bic

  # Mit Buchungsliste (Positiv-/Negativliste)
  redact-rs kontoauszug.pdf -o geschwaerzt.pdf --booking-list buchungen.csv

  # Ganzes Verzeichnis als Stapel — je Datei ein Ergebnis daneben
  redact-rs auszuege/

  # Verschlüsseltes PDF; das Passwort über die Umgebung statt über die
  # Kommandozeile, denn die steht in der Prozessliste und in der Historie
  REDACT_RS_PASSWORD=geheim redact-rs kontoauszug.pdf

  # Zwei Schritte: erst prüfen, dann anwenden
  redact-rs kontoauszug.pdf --review --review-out review.json
  redact-rs kontoauszug.pdf -o geschwaerzt.pdf --apply-review review.json \\
      --audit-log audit.json

{GUI_EXAMPLE}\
  # Beispieldatei zum Ausprobieren erzeugen
  redact-rs --write-demo beispiel.pdf

Einstellungsdatei — Namenszusatz, Muster, Mindestvertrauen, Polsterung, Thema:
  ~/.config/redact-rs/settings.yaml   bzw.   %APPDATA%\\redact-rs\\settings.yaml
  {} zeigt auf eine andere Datei.
  Rangfolge: Kommandozeile schlägt Datei schlägt Vorgabe.

Rückgabewerte:
  {EXIT_OK}  Fertig. Das Dokument wurde vollständig durchsucht.
  {EXIT_ERROR}  Fehlgeschlagen — keine (oder keine brauchbare) Ausgabe. Im Stapel:
     mindestens eine Datei ist gescheitert; die übrigen wurden bearbeitet.
  {EXIT_USAGE}  Bedienfehler: ein Schalter, die Einstellungsdatei oder eine mitgegebene
     Datei passt nicht (z.B. eine Review-Datei zu einem anderen Dokument).
  {EXIT_INCOMPLETE}  Verarbeitet, aber nicht vollständig geprüft. Die Ausgabe ist
     geschrieben und was gefunden wurde, ist geschwärzt — für einen Teil des
     Dokuments konnte die Analyse aber nicht einstehen: ein Font ohne
     /ToUnicode, ein zu tief verschachteltes Form-XObject, ein Kachelmuster
     mit Text, eine Annotation ohne Erscheinungsstrom, ein Bild, das sich
     nicht dekodieren ließ. Dort kann etwas stehen geblieben sein.
     Diese Ausgabe gehört von Hand geprüft. Die betroffenen Stellen stehen
     auf stderr und im Audit-Log; im Stapel weist die Zusammenfassung solche
     Dateien getrennt aus, sie zählen nicht als „verarbeitet“.
",
        redact_pipeline::settings::SETTINGS_ENV,
        EXIT_OK = crate::EXIT_OK,
        EXIT_ERROR = crate::EXIT_ERROR,
        EXIT_USAGE = crate::EXIT_USAGE,
        EXIT_INCOMPLETE = crate::EXIT_INCOMPLETE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_comma_separated_patterns() {
        let cli = Cli::parse_from([
            "redact-rs",
            "in.pdf",
            "-o",
            "out.pdf",
            "--patterns",
            "iban_de,bic",
        ]);
        assert_eq!(cli.patterns, vec!["iban_de", "bic"]);
        assert_eq!(cli.output.unwrap().to_str().unwrap(), "out.pdf");
    }

    #[test]
    fn several_input_files_are_accepted() {
        let cli = Cli::parse_from(["redact-rs", "a.pdf", "b.pdf", "auszuege"]);
        assert_eq!(
            cli.inputs,
            vec![
                PathBuf::from("a.pdf"),
                PathBuf::from("b.pdf"),
                PathBuf::from("auszuege")
            ]
        );
    }

    // ------------------------------------------------------------ Rangfolge

    /// Die Einstellungen, die eine Datei setzen könnte.
    fn file_settings() -> Settings {
        Settings {
            output_suffix: "_ausDatei".into(),
            patterns: vec!["bic".into()],
            disabled_patterns: vec!["date_de".into()],
            min_confidence: Some(0.25),
            padding: 7.5,
            theme: "dunkel".into(),
        }
    }

    /// Ohne Datei und ohne Schalter gilt die eingebaute Vorgabe.
    #[test]
    fn the_built_in_default_applies_without_a_file_and_without_switches() {
        let config = Cli::parse_from(["redact-rs", "in.pdf"]).config(&Settings::default());
        assert_eq!(config.output_suffix, redact_core::DEFAULT_OUTPUT_SUFFIX);
        assert_eq!(config.padding, redact_pipeline::DEFAULT_PADDING);
        assert_eq!(config.min_confidence, None);
        assert!(config.patterns.is_empty());
        assert!(config.disabled_patterns.is_empty());
        assert!(!config.no_patterns);
        assert_eq!(config.theme, "hell");
    }

    /// Die Datei schlägt die Vorgabe.
    #[test]
    fn the_settings_file_beats_the_built_in_default() {
        let config = Cli::parse_from(["redact-rs", "in.pdf"]).config(&file_settings());
        assert_eq!(config.output_suffix, "_ausDatei");
        assert_eq!(config.padding, 7.5);
        assert_eq!(config.min_confidence, Some(0.25));
        assert_eq!(config.patterns, vec!["bic"]);
        assert_eq!(config.disabled_patterns, vec!["date_de"]);
        assert_eq!(config.theme, "dunkel");
    }

    /// Und die Kommandozeile schlägt die Datei — jeder Wert einzeln.
    #[test]
    fn the_command_line_beats_the_settings_file() {
        let config = Cli::parse_from([
            "redact-rs",
            "in.pdf",
            "--output-suffix",
            "_vonHand",
            "--padding",
            "0.5",
            "--min-confidence",
            "0.9",
            "--patterns",
            "iban_de",
            "--disable-pattern",
            "bic,email",
        ])
        .config(&file_settings());
        assert_eq!(config.output_suffix, "_vonHand");
        assert_eq!(config.padding, 0.5);
        assert_eq!(config.min_confidence, Some(0.9));
        assert_eq!(config.patterns, vec!["iban_de"]);
        assert_eq!(
            config.disabled_patterns,
            vec!["bic", "email"],
            "die Abschaltung der Datei muss widerrufbar sein"
        );
    }

    /// `--disable-pattern` nimmt Kommata **und** mehrere Angaben, wie
    /// `--patterns` auch.
    #[test]
    fn disabled_patterns_come_as_a_list_or_one_by_one() {
        let cli = Cli::parse_from([
            "redact-rs",
            "in.pdf",
            "--disable-pattern",
            "date_de,bic",
            "--disable-pattern",
            "email",
        ]);
        assert_eq!(cli.disable_pattern, vec!["date_de", "bic", "email"]);
    }

    /// Gegenprobe: ein Schalter, der *nicht* angegeben wurde, darf den Wert
    /// aus der Datei nicht überschreiben. Genau das täte ein `default_value`
    /// in der clap-Definition — und niemand würde es merken.
    #[test]
    fn an_unused_switch_does_not_overwrite_the_file() {
        let cli = Cli::parse_from(["redact-rs", "in.pdf", "--padding", "0.5"]);
        assert_eq!(
            cli.output_suffix, None,
            "der Schalter braucht keine Vorgabe"
        );
        let config = cli.config(&file_settings());
        assert_eq!(config.padding, 0.5, "die Kommandozeile gilt");
        assert_eq!(config.output_suffix, "_ausDatei", "die Datei gilt weiter");
    }

    /// Der Stapel baut je Datei dieselbe Konfiguration, nur mit anderer Eingabe.
    #[test]
    fn config_for_only_changes_the_input() {
        let cli = Cli::parse_from(["redact-rs", "a.pdf", "b.pdf", "--padding", "2"]);
        let settings = Settings::default();
        let a = cli.config_for(&settings, Path::new("a.pdf"));
        let b = cli.config_for(&settings, Path::new("b.pdf"));
        assert_eq!(a.input, PathBuf::from("a.pdf"));
        assert_eq!(b.input, PathBuf::from("b.pdf"));
        assert_eq!(a.padding, b.padding);
        assert_eq!(a.output_suffix, b.output_suffix);
    }

    // ------------------------------------------------------------- Passwort

    #[test]
    fn the_password_switch_reaches_the_configuration_without_becoming_readable() {
        let config = Cli::parse_from(["redact-rs", "in.pdf", "--password", "geheim"])
            .config(&Settings::default());
        assert_eq!(config.password.as_ref().map(|s| s.reveal()), Some("geheim"));
        // Und es steht in keinem Text, den irgendjemand ausgeben könnte.
        assert!(!format!("{config:?}").contains("geheim"));
    }

    #[test]
    fn without_a_password_nothing_is_set() {
        // Die Umgebungsvariable ist in dieser Prüfung nicht gesetzt (sie wird
        // nirgends im Prozess gesetzt); ohne beides bleibt es bei `None`.
        if std::env::var_os(PASSWORD_ENV).is_none() {
            let config = Cli::parse_from(["redact-rs", "in.pdf"]).config(&Settings::default());
            assert!(config.password.is_none());
        }
    }

    #[test]
    fn review_and_apply_review_are_mutually_exclusive() {
        let r = Cli::try_parse_from([
            "redact-rs",
            "in.pdf",
            "--review",
            "--apply-review",
            "review.json",
        ]);
        assert!(r.is_err());
    }

    #[test]
    fn action_maps_to_core_action() {
        assert_eq!(
            ActionArg::Replace.to_action("[IBAN]"),
            redact_core::Action::Replace("[IBAN]".into())
        );
    }

    // ---------------------------------------------------------- Ersatztext

    /// **#76 (a): der Vorgabe-Ersatztext hat genau eine Quelle.**
    ///
    /// Die Zeichenkette stand als clap-Literal hier *und* als eigene
    /// Konstante in `redact-gui/src/state.rs`. Ein Test, der beide Literale
    /// vergleicht, meldet das Auseinanderlaufen erst hinterher; eine
    /// gemeinsame Konstante lässt es nicht zu. Der Vergleich hier ist deshalb
    /// kein Gleichheitstest zweier Wahrheiten, sondern die Zusicherung, dass
    /// dieser Schalter überhaupt keine eigene mehr hat.
    #[test]
    fn the_replacement_default_comes_from_redact_core() {
        assert_eq!(
            Cli::parse_from(["redact-rs", "in.pdf"]).replace_with,
            redact_core::DEFAULT_REPLACEMENT
        );
        // Und was im Hilfetext steht, ist derselbe Wert — clap druckt den
        // `default_value`, nicht einen zweiten.
        let help = Cli::command().render_long_help().to_string();
        assert!(
            help.contains(redact_core::DEFAULT_REPLACEMENT),
            "der Hilfetext nennt eine andere Vorgabe:\n{help}"
        );
    }

    /// **#76 (b): `--replace-with` überlebt jede `--action`.**
    ///
    /// `Action::Blackout` hat kein Textfeld — mit
    /// `--action blackout --replace-with "[IBAN]"` war `[IBAN]` schon vor dem
    /// ersten Fenster weg, und wer in der Trefferliste der Oberfläche auf
    /// „Ersetzen“ umstellte, bekam die Vorgabe statt seines Textes.
    #[test]
    fn replace_with_survives_every_action() {
        for art in ["blackout", "whiteout", "replace"] {
            let config = Cli::parse_from([
                "redact-rs",
                "in.pdf",
                "--action",
                art,
                "--replace-with",
                "[IBAN]",
            ])
            .config(&Settings::default());
            assert_eq!(
                config.replacement(),
                "[IBAN]",
                "--action {art} frisst --replace-with"
            );
            // Auch das rohe Feld trägt ihn — die Oberfläche liest es.
            assert_eq!(config.replace_with, "[IBAN]");
        }

        // Ohne den Schalter bleibt es bei der einen Vorgabe.
        let config = Cli::parse_from(["redact-rs", "in.pdf"]).config(&Settings::default());
        assert_eq!(config.replacement(), redact_core::DEFAULT_REPLACEMENT);

        // Und `--action replace` trägt den Text weiterhin selbst, denn nur so
        // kommt er in die Review-Datei.
        let config = Cli::parse_from([
            "redact-rs",
            "in.pdf",
            "--action",
            "replace",
            "--replace-with",
            "[IBAN]",
        ])
        .config(&Settings::default());
        assert_eq!(config.action, redact_core::Action::Replace("[IBAN]".into()));
    }

    #[test]
    fn no_arguments_is_allowed_gui_mode() {
        let cli = Cli::parse_from(["redact-rs"]);
        assert!(cli.inputs.is_empty());
    }

    /// Aufgabe #61: der Schalter für die Oberfläche steht nur im Hilfetext
    /// einer Fassung, die eine Oberfläche hat.
    #[test]
    fn the_gui_switch_appears_in_the_help_only_when_there_is_a_gui() {
        let help = Cli::command().render_long_help().to_string();
        assert_eq!(
            help.contains("--gui"),
            cfg!(feature = "gui"),
            "Hilfetext und Fassung passen nicht zusammen:\n{help}"
        );
        // Aufrufbar bleibt er in beiden Fassungen — sonst käme statt der
        // erklärenden Meldung ein „unexpected argument“.
        assert!(Cli::try_parse_from(["redact-rs", "--gui"]).is_ok());
    }
}
