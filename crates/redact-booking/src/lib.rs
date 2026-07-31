//! # redact-booking
//!
//! Buchungslisten für redact-rs: Laden der CSV-Datei ([`CsvBookingLoader`]) und
//! Abgleich des extrahierten PDF-Textes gegen Positiv- und Negativliste
//! ([`BookingMatcher`]).
//!
//! ## Semantik
//!
//! * **Positivliste** — Treffer *müssen* geschwärzt werden.
//! * **Negativliste** — Treffer dürfen *nicht* geschwärzt werden. Sie wird immer
//!   zuerst geprüft und gewinnt jeden Konflikt (die Auflösung selbst passiert in
//!   [`redact_core::resolve_conflicts`]).
//! * Der Vergleich ist **ohne Beachtung von Groß-/Kleinschreibung** und
//!   **leerraum-unempfindlich**: eine IBAN `DE89 3704 0044 0532 0130 00` aus der
//!   CSV trifft auch auf `DE89  3704\n0044 …` im PDF. Details zur Rückabbildung
//!   auf exakte Byte-Offsets (und damit auf exakte Rechtecke) siehe das interne
//!   Modul `normalize`.
//! * **Kontextprüfung**: `context_before` bzw. `context_after` müssen — falls
//!   gesetzt — im Text vor bzw. nach der Fundstelle desselben Runs vorkommen.
//!   Scheitert die Prüfung, wird nur diese Fundstelle verworfen und im selben
//!   Run weitergesucht.
//!
//! ## Einschränkung: `is_regex`
//!
//! Dieses Crate ist bewusst **abhängigkeitsfrei** (keine Regex-Engine) — das
//! hält die sicherheitskritische Buchungslisten-Logik klein und auditierbar.
//! Deshalb gilt:
//!
//! * `is_regex = true` mit einem Muster **ohne** Regex-Metazeichen
//!   (`\ . * + ? ( ) [ ] { } | ^ $`) wird wie ein normales Literal behandelt.
//! * `is_regex = true` mit Metazeichen wird von [`BookingMatcher::new`] mit
//!   [`redact_core::RedactError::Booking`] abgelehnt. Echte reguläre Ausdrücke
//!   gehören in die Pattern-Konfiguration (`--patterns-config`), die von
//!   `redact-patterns` mit einer echten Regex-Engine ausgewertet wird.

#![forbid(unsafe_code)]

mod loader;
mod matcher;
mod normalize;

pub use loader::CsvBookingLoader;
pub use matcher::BookingMatcher;
