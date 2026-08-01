//! CSV-Loader für Buchungslisten.

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

use redact_core::{BookingEntry, ListType, RedactError, Result};
use serde::Deserialize;

/// Spalten, die in der Kopfzeile vorhanden sein müssen.
const REQUIRED_COLUMNS: [&str; 3] = ["id", "list_type", "pattern"];

/// UTF-8 Byte-Order-Mark, das Excel gerne an CSV-Dateien schreibt.
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Rohzeile der CSV-Datei.
///
/// Alle Felder sind `Option<String>`, damit fehlende Spalten und leere Zellen
/// nicht schon beim Deserialisieren scheitern — die eigentliche Prüfung mit
/// verständlichen Fehlermeldungen passiert in [`row_to_entry`].
#[derive(Debug, Deserialize)]
struct CsvRow {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    list_type: Option<String>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    context_before: Option<String>,
    #[serde(default)]
    context_after: Option<String>,
    #[serde(default)]
    is_regex: Option<String>,
}

/// Lädt Buchungslisten aus CSV-Dateien.
///
/// Erwartetes Format (Kopfzeile zwingend, zusätzliche Spalten werden ignoriert,
/// `is_regex` ist optional):
///
/// ```csv
/// id,list_type,pattern,context_before,context_after,is_regex
/// b001,positive,"Musterfirma GmbH","Überweisung an",,
/// b003,negative,"Max Mustermann",,"Kontoinhaber",
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct CsvBookingLoader;

impl CsvBookingLoader {
    /// Liest eine Buchungsliste aus einem beliebigen Reader.
    ///
    /// Ein führendes UTF-8-BOM wird entfernt. Vollständig leere Zeilen werden
    /// übersprungen. Jede Fehlermeldung nennt die CSV-Zeilennummer.
    pub fn from_reader<R: Read>(&self, mut reader: R) -> Result<Vec<BookingEntry>> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        let data = strip_bom(&bytes);

        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(data);

        let headers = rdr
            .headers()
            .map_err(|e| RedactError::Booking(format!("Kopfzeile nicht lesbar: {e}")))?
            .clone();
        check_headers(&headers)?;

        let mut entries: Vec<BookingEntry> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for result in rdr.records() {
            let record =
                result.map_err(|e| RedactError::Booking(format!("CSV nicht lesbar: {e}")))?;
            let line = record.position().map(|p| p.line()).unwrap_or(0);

            // Komplett leere Zeilen (z.B. ",,,,,") stillschweigend überspringen.
            if record.iter().all(|f| f.trim().is_empty()) {
                continue;
            }

            let row: CsvRow = record
                .deserialize(Some(&headers))
                .map_err(|e| RedactError::Booking(format!("Zeile {line}: {e}")))?;
            let entry = row_to_entry(row, line)?;

            if !seen.insert(entry.id.clone()) {
                return Err(RedactError::Booking(format!(
                    "Zeile {line}: doppelte id `{}`",
                    entry.id
                )));
            }
            entries.push(entry);
        }

        Ok(entries)
    }

    /// Liest eine Buchungsliste aus einer Datei.
    pub fn load(&self, path: &Path) -> Result<Vec<BookingEntry>> {
        let file = std::fs::File::open(path).map_err(|e| {
            RedactError::Booking(format!(
                "Buchungsliste `{}` kann nicht geöffnet werden: {e}",
                path.display()
            ))
        })?;
        // Dateiname in jede Fehlermeldung hineinreichen.
        self.from_reader(std::io::BufReader::new(file))
            .map_err(|e| match e {
                RedactError::Booking(msg) => {
                    RedactError::Booking(format!("{}: {msg}", path.display()))
                }
                other => other,
            })
    }
}

/// Entfernt ein führendes UTF-8-BOM.
fn strip_bom(bytes: &[u8]) -> &[u8] {
    match bytes.strip_prefix(&BOM[..]) {
        Some(rest) => rest,
        None => bytes,
    }
}

/// Prüft, ob alle Pflichtspalten in der Kopfzeile stehen.
fn check_headers(headers: &csv::StringRecord) -> Result<()> {
    let present: Vec<String> = headers
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();
    let missing: Vec<&str> = REQUIRED_COLUMNS
        .iter()
        .filter(|c| !present.iter().any(|p| p == *c))
        .copied()
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(RedactError::Booking(format!(
        "Zeile 1: Kopfzeile unvollständig, fehlende Spalte(n): {}",
        missing.join(", ")
    )))
}

/// Normalisiert eine Zelle: leere bzw. nur aus Leerraum bestehende Zellen
/// werden zu `None` (csv+serde liefert sonst teilweise `Some("")`).
fn clean(cell: Option<String>) -> Option<String> {
    let value = cell?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Wandelt eine Rohzeile in einen [`BookingEntry`] um und validiert sie.
fn row_to_entry(row: CsvRow, line: u64) -> Result<BookingEntry> {
    let id = clean(row.id)
        .ok_or_else(|| RedactError::Booking(format!("Zeile {line}: Spalte `id` ist leer")))?;
    let list_type = parse_list_type(clean(row.list_type), line)?;
    let pattern = clean(row.pattern).ok_or_else(|| {
        RedactError::Booking(format!(
            "Zeile {line}: Spalte `pattern` ist leer (id `{id}`)"
        ))
    })?;
    let is_regex = parse_bool(clean(row.is_regex), line)?;

    Ok(BookingEntry {
        id,
        list_type,
        pattern,
        context_before: clean(row.context_before),
        context_after: clean(row.context_after),
        is_regex,
    })
}

/// `positive` / `negative`, Groß-/Kleinschreibung egal.
fn parse_list_type(value: Option<String>, line: u64) -> Result<ListType> {
    let raw = value.ok_or_else(|| {
        RedactError::Booking(format!("Zeile {line}: Spalte `list_type` ist leer"))
    })?;
    match raw.to_ascii_lowercase().as_str() {
        "positive" => Ok(ListType::Positive),
        "negative" => Ok(ListType::Negative),
        _ => Err(RedactError::Booking(format!(
            "Zeile {line}: ungültiger list_type `{raw}` (erlaubt: positive, negative)"
        ))),
    }
}

/// Akzeptiert leer (= `false`), `true`/`false`, `1`/`0`, `yes`/`no`.
fn parse_bool(value: Option<String>, line: u64) -> Result<bool> {
    let Some(raw) = value else {
        return Ok(false);
    };
    match raw.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "ja" => Ok(true),
        "false" | "0" | "no" | "nein" => Ok(false),
        _ => Err(RedactError::Booking(format!(
            "Zeile {line}: ungültiger Wert `{raw}` für is_regex (erlaubt: true, false, 1, 0, yes, no)"
        ))),
    }
}
