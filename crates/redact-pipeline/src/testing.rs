//! Prüfmaterial, das mehr als ein Crate braucht.
//!
//! Hier liegt genau eine Sache: ein **wirklich verschlüsseltes** PDF. Ohne das
//! ließe sich `--password` nur behaupten, nicht messen — und ein Test, der die
//! Entschlüsselung nie ausführt, ist keiner.
//!
//! Die Datei ist von Hand gebaut (Standard-Sicherheitshandler, `/V 1 /R 2`,
//! RC4 mit 40 Bit) und enthält eine einzige Textzeile mit einer IBAN. Genau
//! diese Fassung ist die kleinste, die `lopdf` entschlüsseln kann; sie ist
//! bewusst *ohne* Objekt-Streams gebaut, damit die Entschlüsselung nicht von
//! deren Behandlung abhängt.
//!
//! Warum eine Datei und kein Erzeuger im Code: verschlüsseln kann `lopdf` in
//! der benutzten Fassung nicht, und MD5 und RC4 dafür selbst mitzubringen wäre
//! eine Abhängigkeit (oder eigener Kryptocode) allein für einen Test.

/// Ein RC4-verschlüsseltes PDF; das Passwort ist [`ENCRYPTED_PDF_PASSWORD`].
///
/// Inhalt: `IBAN DE89 3704 0044 0532 0130 00` auf einer Seite.
pub const ENCRYPTED_PDF: &[u8] = include_bytes!("testdata/verschluesselt.pdf");

/// Passwort zu [`ENCRYPTED_PDF`].
pub const ENCRYPTED_PDF_PASSWORD: &str = "geheim123";

/// Die IBAN, die in [`ENCRYPTED_PDF`] steht.
pub const ENCRYPTED_PDF_IBAN: &str = "DE89 3704 0044 0532 0130 00";
