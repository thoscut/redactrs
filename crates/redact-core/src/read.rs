//! Eine fremde Datei lesen, ohne dass die Datei bestimmt, was das kostet.
//!
//! ## Der Befund
//!
//! `std::fs::read` und `std::fs::read_to_string` legen einen Puffer in
//! Dateigröße an und füllen ihn. Wie groß der wird, stand damit in der Datei
//! und nicht in der Konfiguration: eine dünn belegte Datei
//! (`truncate -s 6G liste.csv`) belegt 4 kB auf der Platte und 6 GB im
//! Arbeitsspeicher. Gemessen am gebauten Binary:
//!
//! | Schalter | Datei | Spitzenspeicher |
//! |---|---|---|
//! | `--patterns-config` | 2 GB dünn belegt | 2 114 MB |
//! | `--booking-list` | 1 GB dünn belegt | 5 259 MB |
//! | `--booking-list` | benannte Pipe | 7 201 MB nach 20 s — und kein Ende |
//!
//! Die Buchungsliste ist der schlimmste Fall, weil der CSV-Parser
//! obendrauf kommt: eine einzige riesige Zelle wird als `String`
//! materialisiert, dazu Puffer und eine kleingeschriebene Kopie.
//!
//! Für die Eingabe-PDF und die Einstellungsdatei war das längst geregelt
//! (`redact_pipeline::read_input`, `redact_pipeline::settings`) — dieselbe
//! Datei als Eingabe-PDF wurde in 0,1 s abgelehnt. Der Schutz existierte
//! also, er war nur nicht überall.
//!
//! ## Die Reihenfolge
//!
//! [`read_limited`] fragt erst und liest dann, in genau der Reihenfolge, die
//! `read_input` seit dem Befund benutzt:
//!
//! 1. Es muss eine **gewöhnliche Datei** sein. Eine benannte Pipe hat die
//!    Länge 0 und liefert trotzdem endlos.
//! 2. Die Länge muss unter der Grenze liegen.
//! 3. Gelesen wird trotzdem über einen **begrenzten** Leser. Zwischen der
//!    Frage und dem Lesen kann die Datei wachsen, und über einen
//!    `/proc`-Pfad oder ein Netzdateisystem lügt die Länge auch ohne Zutun.

use std::io::Read;
use std::path::Path;

use crate::display::safe_path;

/// Obergrenze für Hilfsdateien mit Datensätzen: 16 MB.
///
/// Gemeint sind die von Hand gepflegten Listen — Buchungsliste, Review-Datei,
/// manuelle Regionen —, also Dateien, in denen viele gleichartige Einträge
/// stehen. Wessen Parser je Byte teurer ist, gibt [`read_limited`] eine
/// kleinere Zahl mit; die Pattern-Konfiguration tut das (siehe dort), weil
/// jedes kompilierte Muster rund 12 kB kostet und nicht 1 Byte.
///
/// ## Warum eine eigene, kleinere Grenze
///
/// Die Grenze der Eingabe-PDF (`--max-input-mb`, 512 MB) passt hier nicht: sie
/// ist für gescannte Dokumente bemessen, und eine Buchungsliste von 512 MB
/// gibt es nicht. Je kleiner die Grenze, desto weniger Speicher lässt sich
/// über eine solche Datei anfordern — und Hilfsdateien sind das, was Menschen
/// von Hand pflegen.
///
/// ## Warum ausgerechnet 16 MB
///
/// Gemessen an dem, was legitim vorkommt:
///
/// * **Buchungsliste**: `examples/booking_list.csv` braucht 134 Byte je
///   Eintrag. 16 MB fassen damit über 100 000 Einträge — mehr, als die
///   Pipeline mit `DEFAULT_MAX_CANDIDATES` (100 000) überhaupt weiterreicht.
/// * **Review-Datei**: gemessen rund 600 Byte je geprüfter Stelle. 16 MB sind
///   damit gut 25 000 Stellen; von Hand geprüft werden Dutzende bis Hunderte.
///
/// Und gemessen an dem, was es im schlimmsten Fall kostet: eine feindselige
/// CSV-Datei genau auf der Grenze belegte 454 MB Spitzenspeicher (767 648
/// Zeilen zu je 20 Byte) und brach nach 11 s kontrolliert ab. Das ist die
/// Größenordnung der Budgets, die dieses Werkzeug ohnehin veranschlagt
/// (512 MB Eingabe-PDF, 256 MB dekodierte Bilder) — und der zwölfte Teil
/// dessen, was eine 1-GB-Datei ohne Grenze gekostet hat.
///
/// ## Warum sie fest ist
///
/// Ein Schalter mehr wäre ein Schalter, dessen einziger Zweck es wäre, diesen
/// Schutz aufzuweichen. Für `--max-input-mb` gibt es einen Grund — ein
/// 700-MB-Scan ist eine echte Eingabe —, für eine 700-MB-Buchungsliste keinen.
pub const MAX_AUX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Obergrenze für die **Zahl** der Suchbegriffe einer Nachprüfung.
///
/// `--check-leaks` und die Nachprüfung der Oberfläche nach dem Export teilen
/// sich diese Decke — dieselbe Zahl, dieselbe Einheit. Sie liegt hier und
/// nicht in einem der beiden Programme aus demselben Grund wie
/// [`crate::DEFAULT_REPLACEMENT`]: `redact-core` ist der Ort, an dem beide
/// dieselbe Sprache sprechen.
///
/// Warum eine Zahl und nicht nur die Byte-Grenze [`MAX_AUX_FILE_BYTES`]:
/// 16 MB fassen rund eine Million kurze Zeilen. Die Kosten der Nachprüfung
/// sind Begriffe × **entpackte** Streambytes; die Bytes sind durch
/// `--max-decompressed-mb` gedeckelt, die Begriffe hier. Gemessen an einer
/// 898-kB-Datei mit 420 Seiten (Release): 1 Begriff 1,7 s, 100 Begriffe
/// 2,1 s, 1 000 Begriffe 5,9 s — ein Sockel (Datei lesen, Ströme auspacken,
/// jede Seite durch den Schriftdekoder) und darüber rund 4 ms je Begriff.
/// Eine Million Begriffe liefen also über eine Stunde, ohne dass etwas kaputt
/// wäre; von außen sieht das wie ein Hänger aus.
///
/// 1 000 ist großzügig für den Zweck: die Geheimnisse **eines** Dokuments,
/// von Hand aufgeschrieben. Wer mehr hat, ruft zweimal auf.
pub const MAX_CHECK_NEEDLES: usize = 1_000;

/// Liest `path` vollständig — mit der Obergrenze `max_bytes` **vor** dem
/// ersten gelesenen Byte.
///
/// `limit_hint` wird an die Größenmeldung angehängt und sagt, **woher** die
/// Grenze kommt und was zu tun ist; ohne diesen Satz wüsste der Nutzer nur,
/// dass etwas zu groß war. Der Aufrufer schreibt ihn, weil nur er die Grenze
/// kennt: `--max-input-mb` ist einstellbar, [`MAX_AUX_FILE_BYTES`] nicht.
///
/// ## Warum der Fehler ein `String` ist
///
/// Jeder der vier Aufrufer meldet in seiner eigenen Fehlerart — eine
/// Buchungsliste als [`crate::RedactError::Booking`], eine Musterkonfiguration
/// als [`crate::RedactError::Pattern`], die Eingabe-PDF als
/// [`crate::RedactError::Pdf`]. Die Meldung ist überall dieselbe, die
/// Einordnung nicht; deshalb gibt diese Funktion den fertigen Satz zurück und
/// überlässt das Einpacken dem Aufrufer:
///
/// ```no_run
/// use std::path::Path;
/// use redact_core::{read_limited, RedactError, MAX_AUX_FILE_BYTES};
///
/// let bytes = read_limited(Path::new("liste.csv"), MAX_AUX_FILE_BYTES, "Feste Grenze.")
///     .map_err(RedactError::Booking)?;
/// # Ok::<(), RedactError>(())
/// ```
///
/// Die Datei wird in jeder Meldung beim Namen genannt, und zwar durch
/// [`safe_path`]: ein Dateiname darf unter Unix Steuerzeichen enthalten, und
/// ein Terminal führt die aus.
pub fn read_limited(path: &Path, max_bytes: u64, limit_hint: &str) -> Result<Vec<u8>, String> {
    let name = safe_path(path);
    let meta = std::fs::metadata(path).map_err(|e| format!("{name}: nicht lesbar: {e}"))?;

    // 1. Gewöhnliche Datei? Eine benannte Pipe meldet Länge 0 und liefert
    //    endlos; ein Verzeichnis lässt sich gar nicht lesen.
    if !meta.is_file() {
        return Err(format!(
            "{name}: keine gewöhnliche Datei. Gelesen werden nur Dateien — eine Pipe oder \
             ein Gerät hätte keine Größe, an der sich eine Grenze festmachen ließe, und \
             lieferte weiter, bis der Arbeitsspeicher voll ist."
        ));
    }

    // 2. Größe laut Dateisystem, noch ohne ein Byte zu lesen.
    if meta.len() > max_bytes {
        return Err(format!(
            "{name}: {} groß, erlaubt sind {}. {limit_hint}",
            measured(meta.len()),
            limit(max_bytes)
        ));
    }

    // 3. Und trotzdem begrenzt lesen. `max_bytes + 1`: so ist eine Datei, die
    //    zwischen Frage und Lesen gewachsen ist, am Ergebnis zu erkennen statt
    //    am Speicherverbrauch.
    let file = std::fs::File::open(path).map_err(|e| format!("{name}: nicht lesbar: {e}"))?;
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{name}: nicht lesbar: {e}"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!(
            "{name}: beim Lesen über die Grenze von {} hinaus gewachsen — die Datei war beim \
             Nachfragen kleiner als beim Lesen. {limit_hint}",
            limit(max_bytes)
        ));
    }
    Ok(bytes)
}

/// Die **gemessene** Größe einer Datei, wie ein Mensch sie liest.
///
/// Gerundet wird auf, und das mit Absicht: die Zahl steht nur in der Meldung
/// „zu groß“, und dort ist „17 MB“ für 16 MB + 1 Byte die Aussage, auf die es
/// ankommt. Abgerundet stünde da „16 MB groß, erlaubt sind 16 MB“ — eine
/// Meldung, die sich selbst widerspricht.
///
/// Die genaue Byte-Zahl steht daneben, denn die Aufrundung macht aus 4,0 MB
/// ein „5 MB“, und wer das mit seinem `ls -l` vergleicht, soll nicht an der
/// Meldung zweifeln. Unter einem Megabyte gibt es nichts aufzurunden.
fn measured(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes < MB {
        format!("{bytes} Byte")
    } else {
        format!("{} MB ({bytes} Byte)", bytes.div_ceil(MB))
    }
}

/// Die **festgelegte** Grenze, wie ein Mensch sie liest.
///
/// Anders als [`measured`] ohne die Byte-Zahl: eine Grenze ist eine runde
/// Zahl, die im Quelltext oder hinter `--max-input-mb` steht, und niemand
/// muss sie nachrechnen.
fn limit(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes < MB {
        format!("{bytes} Byte")
    } else {
        format!("{} MB", bytes.div_ceil(MB))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const HINT: &str = "Feste Grenze für Hilfsdateien.";

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "redact-core-read-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Die Gegenprobe zuerst: eine gewöhnliche Datei wird vollständig
    /// gelesen, Byte für Byte wie mit `std::fs::read`. Ohne diesen Test wäre
    /// alles andere nur kaputt statt abgesichert.
    #[test]
    fn an_ordinary_file_is_read_completely() {
        let dir = tempdir("gewoehnlich");
        let path = dir.join("liste.csv");
        let content: Vec<u8> = (0..=255u8).cycle().take(300_000).collect();
        std::fs::write(&path, &content).unwrap();

        assert_eq!(
            read_limited(&path, MAX_AUX_FILE_BYTES, HINT).unwrap(),
            content
        );
        // Genau auf der Grenze ist noch erlaubt, ein Byte darunter nicht.
        assert!(read_limited(&path, content.len() as u64, HINT).is_ok());
        assert!(read_limited(&path, content.len() as u64 - 1, HINT).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// **Die Auflage:** die Grenze greift *vor* dem Lesen.
    ///
    /// Zu sehen ist das daran, dass die Meldung die Größe nennt — die kann sie
    /// nur aus der Angabe des Dateisystems haben — und dass die Datei dünn
    /// belegt ist: gelesen hätte sie 6 GB gekostet, abgelehnt kostet sie
    /// nichts.
    #[test]
    fn a_file_beyond_the_limit_is_refused_before_it_is_read() {
        let dir = tempdir("gross");
        let path = dir.join("riesig.csv");
        let file = std::fs::File::create(&path).unwrap();
        // Dünn belegt: 6 GB Nennlänge, 0 Byte geschrieben.
        file.set_len(6 * 1024 * 1024 * 1024).unwrap();
        drop(file);

        let error = read_limited(&path, MAX_AUX_FILE_BYTES, HINT)
            .expect_err("6 GB müssen abgelehnt werden");
        assert!(error.contains("riesig.csv"), "{error}");
        assert!(error.contains("6144 MB"), "{error}");
        assert!(error.contains("16 MB"), "{error}");
        assert!(error.contains(HINT), "der Hinweis fehlt: {error}");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Ein Verzeichnis hat keine Größe, an der sich eine Grenze festmachen
    /// ließe — und wird gar nicht erst geöffnet.
    #[test]
    fn a_directory_is_not_a_file() {
        let dir = tempdir("art");
        let error = read_limited(&dir, MAX_AUX_FILE_BYTES, HINT)
            .expect_err("ein Verzeichnis ist keine Hilfsdatei");
        assert!(error.contains("gewöhnliche Datei"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// **Die Auflage:** eine benannte Pipe wird abgelehnt, statt endlos zu
    /// liefern.
    ///
    /// Der Test kommt ohne Schreiber am anderen Ende aus, und das ist gerade
    /// der Punkt: schon das *Öffnen* einer Pipe ohne Schreiber blockiert
    /// endlos. Wenn dieser Test zurückkehrt, ist vor dem Öffnen entschieden
    /// worden.
    #[cfg(unix)]
    #[test]
    fn a_named_pipe_is_refused_without_opening_it() {
        let dir = tempdir("pipe");
        let path = dir.join("pipe.csv");
        let ok = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("mkfifo startbar");
        assert!(ok.success(), "mkfifo ist fehlgeschlagen");

        let error = read_limited(&path, MAX_AUX_FILE_BYTES, HINT)
            .expect_err("eine Pipe ist keine Hilfsdatei");
        assert!(error.contains("gewöhnliche Datei"), "{error}");
        assert!(error.contains("pipe.csv"), "{error}");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Eine fehlende Datei ist ein gewöhnlicher Fehler — mit Namen.
    #[test]
    fn a_missing_file_is_named() {
        let error = read_limited(
            Path::new("/nicht/vorhanden/liste.csv"),
            MAX_AUX_FILE_BYTES,
            HINT,
        )
        .expect_err("die Datei gibt es nicht");
        assert!(error.contains("liste.csv"), "{error}");
        assert!(error.contains("nicht lesbar"), "{error}");
    }

    /// Steuerzeichen im Dateinamen erreichen das Terminal nicht — die Meldung
    /// geht durch [`safe_path`].
    #[test]
    fn a_name_with_control_characters_cannot_steer_the_terminal() {
        let error = read_limited(
            Path::new("/nicht/da/a\u{1b}[2Kliste.csv"),
            MAX_AUX_FILE_BYTES,
            HINT,
        )
        .expect_err("die Datei gibt es nicht");
        assert!(!error.contains('\u{1b}'), "{error}");
    }

    /// Die Größenangabe widerspricht sich nicht: ein Byte über der Grenze
    /// heißt „17 MB“, nicht „16 MB groß, erlaubt sind 16 MB“ — und die genaue
    /// Byte-Zahl steht daneben, damit die Aufrundung nachvollziehbar bleibt.
    #[test]
    fn the_size_in_the_message_is_never_self_contradictory() {
        assert_eq!(measured(0), "0 Byte");
        assert_eq!(measured(941), "941 Byte");
        assert_eq!(measured(MAX_AUX_FILE_BYTES), "16 MB (16777216 Byte)");
        assert_eq!(measured(MAX_AUX_FILE_BYTES + 1), "17 MB (16777217 Byte)");
        assert_eq!(
            measured(6 * 1024 * 1024 * 1024),
            "6144 MB (6442450944 Byte)"
        );

        // Die Grenze selbst ist eine runde Zahl und braucht keine Byte-Angabe.
        assert_eq!(limit(0), "0 Byte");
        assert_eq!(limit(MAX_AUX_FILE_BYTES), "16 MB");
        assert_eq!(limit(512 * 1024 * 1024), "512 MB");
    }
}
