//! Kommandozeilen-Definition.

use std::path::{Path, PathBuf};

use clap::{ArgAction, Parser, ValueEnum};
use redact_pipeline::{Config, Secret, Settings};

/// Umgebungsvariable für das Passwort verschlüsselter PDFs.
///
/// Der Weg an der Prozessliste vorbei — siehe [`Cli::password`].
pub const PASSWORD_ENV: &str = "REDACT_RS_PASSWORD";

/// Schalter, die mit `--check-leaks` nicht zusammengehen.
///
/// ## Warum eine Liste und kein Übergehen
///
/// `--check-leaks` **liest** eine fertige Datei und schreibt nichts. Jeder
/// Schalter hier gehört zum Schwärzen — er bestimmt, was gefunden (`--patterns`,
/// `--min-confidence`, `--booking-list`), was daraus gemacht (`--action`,
/// `--padding`) und wohin es geschrieben wird (`-o`, `--audit-log`,
/// `--review`). Bei einer Nachprüfung hat keiner davon eine Wirkung.
///
/// Sie stillschweigend zu übergehen wäre die schlechtere Wahl: wer
/// `--check-leaks "DE89 …" --patterns iban_de` schreibt, meint erkennbar „such
/// nur nach IBANs“ — und bekäme eine Prüfung, die etwas anderes tut, als er
/// gelesen hat. Dasselbe bei `-o`: die Erwartung wäre „schwärzen **und**
/// nachprüfen“, und das Ergebnis wäre eine Datei, die nie geschrieben wurde.
/// Das Werkzeug lehnt deshalb ab und sagt es (Rückgabewert 2), wie schon bei
/// einem unbekannten Namen hinter `--disable-pattern`.
///
/// Zwei Schritte statt eines: erst `redact-rs auszug.pdf -o out.pdf`, dann
/// `redact-rs out.pdf --check-leaks "…"`. Der zweite Aufruf prüft damit
/// nachweislich *die geschriebene Datei* und nicht einen Zwischenstand im
/// Speicher — was eine Kontrolle erst zu einer macht.
///
/// **Nicht in dieser Liste** und deshalb erlaubt: `--quiet` (weniger Ausgabe,
/// Funde bleiben) und die drei Grenzen `--max-input-mb`,
/// `--max-decompressed-mb` und `--max-parsed-mb`. Die greifen beim Lesen und
/// Vorprüfen jeder fremden Datei, also auch hier. `--max-image-mb` steht
/// dagegen in der Liste: es begrenzt die *dekodierten* Bildbytes, und beim
/// Nachprüfen wird kein Bild dekodiert.
///
/// ## Auch die Schalter mit Vorgabewert
///
/// `--action`, `--replace-with`, `--padding` und `--max-image-mb`
/// tragen ein `default_value`; clap zählt eine Vorgabe nicht als „angegeben“
/// und schlägt deshalb nur an, wenn der Schalter wirklich auf der
/// Kommandozeile stand. Der Test
/// `check_leaks_alone_survives_the_defaults` hält das fest — liefe es anders,
/// wäre `--check-leaks` allein nicht mehr aufrufbar.
const CHECK_LEAKS_CONFLICTS: [&str; 26] = [
    // Es wird nichts geschrieben.
    "output",
    "output_suffix",
    "force",
    "audit_log",
    // Es wird nichts analysiert und nichts geschwärzt.
    "review",
    "review_out",
    "apply_review",
    "allow_unverified_review",
    "patterns",
    "no_patterns",
    "disable_pattern",
    "patterns_config",
    "min_confidence",
    "booking_list",
    "manual_regions",
    "allow_undecodable_images",
    "max_candidates",
    // Begrenzt die *dekodierten* Bildbytes — hier wird kein Bild dekodiert.
    "max_image_mb",
    "action",
    "replace_with",
    "padding",
    // Verschlüsselte Dateien lehnt die Prüfung ab (eine Bytesuche fände darin
    // nichts) — ein Passwort hilft ihr also nicht, und stillschweigend
    // ignoriert sähe es aus, als täte es das.
    "password",
    // `--json` gibt die Zusammenfassung eines Schwärzungslaufs aus; die gibt
    // es hier nicht. Ein zweites Ausgabeformat für die Prüfung wäre eine
    // zweite Wahrheit neben der ersten — die Schnittstelle für ein Skript ist
    // der Rückgabewert (0 sauber, 3 Fund, sonst Fehler).
    "json",
    // Andere Betriebsarten.
    "gui",
    "write_demo",
    "list_patterns",
];

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
    ///
    /// Geprüft wird an der Grenze: `f64` allein nimmt `nan`, `inf` und `1e400`
    /// an, und jeder dieser Werte macht **jede** Schwärzung wirkungslos. Die
    /// Regel steht in [`redact_pipeline::check_padding`] — dieselbe, die für
    /// den Schlüssel `padding` der Einstellungsdatei gilt.
    #[arg(long, value_parser = parse_padding)]
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

    /// Davon: Obergrenze für alles, woraus PDF-**Syntax** wird — die
    /// geparsten Streams (Seiteninhalt, Objekt-Streams) **und** der Rumpf der
    /// Datei selbst (Objektköpfe, Dictionaries, Arrays, Querverweistabelle,
    /// Trailer). Beide Klassen zusammen gegen dieselbe Zahl; eine Datei ganz
    /// ohne Streams kann sie allein ausschöpfen.
    ///
    /// Ob ein Stream hierher zählt, entscheidet sein **ausgepackter Inhalt**,
    /// nicht sein Dictionary: sieht er wie PDF-Syntax aus statt wie Nutzlast,
    /// gilt dieses engere Budget. Auch ein Bild kann darunterfallen, wenn
    /// seine Bildpunkte wie druckbarer Text aussehen — ein dunkler
    /// Graustufen-Scan tut das.
    ///
    /// Das ist **nicht** der Spitzenbedarf, und der Aufblähfaktor ist keine
    /// Konstante: er hängt an der *Form* der Syntax, nicht an ihrer Länge.
    /// Gemessen an einer Seite mit 20 000 Textzeilen: 1,31 MB Seiteninhalt,
    /// 477 MB Spitzenspeicher — 363 Byte je Byte. Für eine Textseite zieht
    /// ohnehin meist nicht diese Grenze, sondern die Deckelung auf eine
    /// Million Zeichen je Seite. Die Messreihe steht in `SECURITY.md`.
    ///
    /// Genau weil sich aus Dateibytes kein Speicher ablesen lässt, deckelt
    /// derselbe Schalter noch eine zweite Größe: den *gerechneten* Speicher
    /// der Objekte, die daraus entstehen. Beide Decken hängen an dieser einen
    /// Zahl und bewegen sich zusammen — wer eine wirklich so große Datei
    /// durchlassen will, hebt nur sie.
    #[arg(long, value_name = "MB", default_value_t = 16)]
    pub max_parsed_mb: u64,

    /// Obergrenze für die gleichzeitig gehaltenen **dekodierten** Bildbytes.
    ///
    /// Eine Schwärzung auf einem Bild überschreibt dessen Bildpunkte, dafür
    /// muss das Bild nach RGBA8 ausgepackt werden: 4 Byte je Bildpunkt. Ein
    /// gewöhnlicher Schwarzweiß-Scan (`/BitsPerComponent 1`) wächst dabei um
    /// den Faktor 32 — deshalb genügen `--max-decompressed-mb` und
    /// `--max-parsed-mb` hier nicht: die zählen die **ausgepackten**
    /// Streambytes, also die Bildpunkte in der Form, in der sie in der Datei
    /// stehen, nicht das RGBA8 danach. (Roh gezählt wird nirgends: eine
    /// 82 679 Byte kleine Datei mit einem FlateDecode-Bild von 19 998 784 Byte
    /// entpackt scheitert an `--max-decompressed-mb 8`.)
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
    /// Die Treffer müssen gegeneinander aufgelöst werden, und dieser Aufwand
    /// wächst schneller als ihre Zahl — es ist eine Frage über Paare. Ohne
    /// Grenze genügt eine kleine Datei mit sehr vielen Treffern, um die
    /// Maschine stundenlang zu beschäftigen.
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

    /// **Nachprüfen statt schwärzen:** steht dieser Text noch in der Datei?
    ///
    /// Mehrfach angebbar — je Angabe ein Suchbegriff. Gesucht wird mit
    /// `redact_pdf::leaks` auf allen Ebenen, auf denen ein Geheimnis
    /// überleben kann: rohe Dateibytes, jeder `stream … endstream`-Block (auch
    /// Flate-dekomprimiert, also inklusive Altrevisionen), jedes Stream-Objekt
    /// dekodiert, die Objekte in `/ObjStm`-Containern und jedes
    /// Zeichenketten-Objekt unter jedem Schlüssel — jeweils in UTF-8,
    /// Latin-1/PDFDoc, UTF-16BE und als Hex-String. `pdftotext … | grep …`
    /// sieht davon einen Bruchteil und gibt an der eigenen Demo-Ausgabe
    /// falsche Entwarnung.
    ///
    /// **`-` liest die Begriffe zeilenweise von der Standardeingabe.** Ein
    /// Suchbegriff ist ein Geheimnis; auf der Kommandozeile steht er in der
    /// Prozessliste (`ps`) und in der Shell-Historie — genau wie ein Passwort.
    /// `redact-rs geschwaerzt.pdf --check-leaks - < begriffe.txt` nimmt keinen
    /// der beiden Wege.
    ///
    /// **Kein Komma-Trenner:** eine Angabe ist ein Begriff, ganz.
    /// „Mustermann, Max“ ist ein Name und nicht zwei.
    ///
    /// Rückgabewert: `0`, wenn keiner der Begriffe gefunden wurde, `3`, wenn
    /// mindestens einer noch dasteht. Ein Fund ist kein Verarbeitungsfehler —
    /// der Lauf ist gelungen, das *Ergebnis* ist es nicht.
    ///
    /// **Nichts gefunden ist kein Freibrief:** geprüft ist damit genau diese
    /// Liste und sonst nichts.
    #[arg(
        long = "check-leaks",
        value_name = "TEXT",
        action = ArgAction::Append,
        conflicts_with_all = CHECK_LEAKS_CONFLICTS,
    )]
    pub check_leaks: Vec<String>,

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

/// Wertet `--padding` aus und lehnt ab, was keine Länge ist.
///
/// **Warum ein `value_parser` und nicht eine Prüfung in [`Cli::config`]:** clap
/// beendet den Aufruf bei einem abgelehnten Wert selbst, mit Rückgabewert 2
/// (Benutzungsfehler) und der Meldung unter dem beanstandeten Schalter — also
/// bevor eine Datei gelesen wird. Eine Prüfung in [`Cli::config`] müsste
/// dagegen deren Signatur auf `Result` umstellen, und die ruft auch die
/// Oberfläche.
///
/// Die Regel selbst steht **nicht hier**, sondern in
/// [`redact_pipeline::check_padding`]: den Wert kann auch die
/// Einstellungsdatei liefern, und zwei Fassungen derselben Grenze wären zwei
/// Gelegenheiten, dass sie auseinanderlaufen.
fn parse_padding(raw: &str) -> Result<f64, String> {
    let value: f64 = raw
        .trim()
        .parse()
        .map_err(|_| format!("„{raw}“ ist keine Zahl"))?;
    redact_pipeline::check_padding(value).map_err(|e| e.to_string())?;
    Ok(value)
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

  # Nachprüfen: steht das Geheimnis noch in der fertigen Datei?
  redact-rs geschwaerzt.pdf --check-leaks \"DE89 3704 0044 0532 0130 00\" \\
      --check-leaks \"Max Mustermann\"

  # Dasselbe, ohne die Begriffe in Prozessliste und Shell-Historie zu schreiben
  redact-rs geschwaerzt.pdf --check-leaks - < begriffe.txt

Einstellungsdatei — Namenszusatz, Muster, Mindestvertrauen, Polsterung, Thema:
  ~/.config/redact-rs/settings.yaml   bzw.   %APPDATA%\\redact-rs\\settings.yaml
  {} zeigt auf eine andere Datei.
  Rangfolge: Kommandozeile schlägt Datei schlägt Vorgabe.

Rückgabewerte:
  {EXIT_OK}  Fertig. Das Dokument wurde vollständig durchsucht.
     Bei --check-leaks: keiner der angegebenen Begriffe steht noch in der
     Datei. Das ist kein Freibrief — geprüft wurde genau diese Liste.
  {EXIT_ERROR}  Fehlgeschlagen — keine (oder keine brauchbare) Ausgabe. Im Stapel:
     mindestens eine Datei ist gescheitert; die übrigen wurden bearbeitet.
  {EXIT_USAGE}  Bedienfehler: ein Schalter, die Einstellungsdatei oder eine mitgegebene
     Datei passt nicht (z.B. eine Review-Datei zu einem anderen Dokument).
  {EXIT_INCOMPLETE}  Der Lauf ist gelungen, das Ergebnis ist es nicht — sieh hin.
     Zwei Fälle:
     • Verarbeitet, aber nicht vollständig geprüft. Die Ausgabe ist
       geschrieben und was gefunden wurde, ist geschwärzt — für einen Teil des
       Dokuments konnte die Analyse aber nicht einstehen: ein Font ohne
       /ToUnicode, ein zu tief verschachteltes Form-XObject, ein Kachelmuster
       mit Text, eine Annotation ohne Erscheinungsstrom, ein Bild, das sich
       nicht dekodieren ließ. Dort kann etwas stehen geblieben sein.
       Diese Ausgabe gehört von Hand geprüft. Die betroffenen Stellen stehen
       auf stderr und im Audit-Log; im Stapel weist die Zusammenfassung solche
       Dateien getrennt aus, sie zählen nicht als „verarbeitet“.
     • --check-leaks hat mindestens einen Begriff in der Datei gefunden.
       Ein Fund ist kein Verarbeitungsfehler (das wäre 1) und kein
       Bedienfehler (das wäre 2): die Suche lief vollständig, die Antwort
       lautet „ja, es steht noch drin“.
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

    // ---------------------------------------------------------- Nachprüfung

    /// **Die Falle, die diese Liste stellen könnte.** `--action`,
    /// `--replace-with` und `--padding` stehen in
    /// [`CHECK_LEAKS_CONFLICTS`] und tragen zugleich einen `default_value`.
    /// Zählte clap eine Vorgabe als „angegeben“, wäre `--check-leaks` allein
    /// nicht mehr aufrufbar — der Schalter wäre tot, und zwar sofort und für
    /// jeden Aufruf.
    #[test]
    fn check_leaks_alone_survives_the_defaults() {
        let cli = Cli::try_parse_from(["redact-rs", "out.pdf", "--check-leaks", "DE89"])
            .expect("--check-leaks muss allein aufrufbar sein");
        assert_eq!(cli.check_leaks, vec!["DE89".to_string()]);
    }

    /// Mehrfach angeben sammelt; ein Komma trennt nicht.
    #[test]
    fn every_occurrence_is_one_needle() {
        let cli = Cli::parse_from([
            "redact-rs",
            "out.pdf",
            "--check-leaks",
            "Mustermann, Max",
            "--check-leaks",
            "DE89 3704",
        ]);
        assert_eq!(cli.check_leaks, vec!["Mustermann, Max", "DE89 3704"]);
    }

    /// Und die Gegenprobe zur Liste: ein Schalter des Schwärzens wird
    /// abgelehnt, statt wirkungslos mitzulaufen.
    #[test]
    fn a_redaction_switch_next_to_check_leaks_is_refused() {
        for extra in [
            vec!["-o", "egal.pdf"],
            vec!["--review"],
            vec!["--patterns", "iban_de"],
            vec!["--json"],
        ] {
            let mut args = vec!["redact-rs", "out.pdf", "--check-leaks", "DE89"];
            args.extend(extra.iter().copied());
            assert!(
                Cli::try_parse_from(&args).is_err(),
                "{extra:?} müsste mit --check-leaks kollidieren"
            );
        }
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

    // ------------------------------------- Der Hilfetext und was wirklich zählt
    //
    // Beide Tests hier prüfen **eine Aussage des Hilfetextes**, und zwar an
    // der Tat statt am Wortlaut: der Text sagt, *welche Menge* ein Budget
    // zählt, und genau daran hing schon zweimal eine falsche Zusicherung.
    // Geprüft wird deshalb immer dieselbe Datei mit zwei Werten desselben
    // Schalters — zählte das Programm die andere Menge, liefe sie beide Male
    // durch, und der Test fiele auf.

    /// Ein PDF-Rumpf **ohne einen einzigen Stream**: `ballast` Dictionaries,
    /// die niemand referenziert.
    fn rumpf_ohne_stream(ballast: usize) -> Vec<u8> {
        use lopdf::xref::XrefType;
        use lopdf::{dictionary, Document, Object};

        let mut doc = Document::with_version("1.4");
        // Klassische Querverweistabelle statt Querverweis-*Stream*: `lopdf`
        // schreibt sonst einen, und dann hätte diese Datei doch einen Stream —
        // was den Versuch gerade um seinen Kern brächte.
        doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1_i64,
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);

        for _ in 0..ballast {
            let mut d = lopdf::Dictionary::new();
            for k in 0..500 {
                d.set(format!("k{k}"), Object::Integer(0));
            }
            doc.add_object(Object::Dictionary(d));
        }

        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("speicherbar");
        bytes
    }

    /// Ein PDF mit einem komprimierten Bildstrom: auf der Platte klein,
    /// ausgepackt `megabytes` MB **Nutzlast** (Bytes über 126, damit der
    /// Inhalt nicht wie PDF-Syntax aussieht).
    fn bild_das_sich_aufblaeht(megabytes: usize) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object, Stream};

        let punkte: Vec<u8> = (0..megabytes * 1024 * 1024)
            .map(|i| 128u8.wrapping_add((i % 128) as u8))
            .collect();
        let mut bild = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 1024_i64,
                "Height" => (megabytes * 1024) as i64,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8_i64,
            },
            punkte,
        );
        bild.compress().expect("komprimierbar");

        let mut doc = Document::with_version("1.5");
        let bild_id = doc.add_object(bild);
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => bild_id } },
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1_i64,
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);

        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("speicherbar");
        bytes
    }

    /// `--max-parsed-mb` zählt **auch den Rumpf** der Datei, nicht nur die
    /// geparsten Streams — so steht es im Hilfetext, seit dort der halbe Satz
    /// stand. Diese Datei hat keinen einzigen Stream; zählte nur, was in
    /// Streams steht, käme sie durch beide Werte.
    #[test]
    fn das_parse_budget_zaehlt_auch_den_rumpf_ohne_jeden_stream() {
        let pdf = rumpf_ohne_stream(700);
        assert!(
            pdf.len() > 2 * 1024 * 1024,
            "Rumpf zu klein für den Versuch: {} Byte",
            pdf.len()
        );
        let stelle = pdf.windows(6).position(|w| w == b"stream");
        let umfeld = stelle
            .map(|i| String::from_utf8_lossy(&pdf[i.saturating_sub(80)..i]).into_owned())
            .unwrap_or_default();
        assert!(
            stelle.is_none(),
            "diese Datei darf keinen Stream enthalten, hat aber einen: {umfeld}"
        );

        let eng = Cli::parse_from(["redact-rs", "in.pdf", "--max-parsed-mb", "1"]).limits();
        let weit = Cli::parse_from(["redact-rs", "in.pdf", "--max-parsed-mb", "8"]).limits();
        assert!(
            redact_pdf::document::prescan(&pdf, &eng).is_err(),
            "ein Rumpf über dem Budget muss abgelehnt werden"
        );
        assert!(
            redact_pdf::document::prescan(&pdf, &weit).is_ok(),
            "derselbe Rumpf unter dem Budget muss durchlaufen"
        );
    }

    /// `--max-decompressed-mb` zählt die **ausgepackten** Streambytes, nicht
    /// die rohen — so steht es beim Hilfetext von `--max-image-mb`, seit dort
    /// „Rohbytes“ stand. Die Datei hier ist auf der Platte weit unter jeder
    /// der beiden Grenzen; nur ausgepackt reißt sie die engere.
    #[test]
    fn das_dekompressionsbudget_zaehlt_die_ausgepackten_streambytes() {
        let pdf = bild_das_sich_aufblaeht(4);
        assert!(
            pdf.len() < 1024 * 1024,
            "roh gezählt müsste die Datei unter 1 MB bleiben, ist aber {} Byte",
            pdf.len()
        );

        let eng = Cli::parse_from(["redact-rs", "in.pdf", "--max-decompressed-mb", "1"]).limits();
        let weit = Cli::parse_from(["redact-rs", "in.pdf", "--max-decompressed-mb", "8"]).limits();
        assert!(
            redact_pdf::document::prescan(&pdf, &eng).is_err(),
            "4 MB ausgepackt müssen an 1 MB scheitern"
        );
        assert!(
            redact_pdf::document::prescan(&pdf, &weit).is_ok(),
            "dieselben 4 MB müssen unter 8 MB durchlaufen"
        );
    }
}
