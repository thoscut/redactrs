//! Prüfmaterial, das mehr als ein Crate braucht.
//!
//! Hier liegen **wirklich verschlüsselte** PDFs. Ohne die ließe sich
//! `--password` nur behaupten, nicht messen — und ein Test, der die
//! Entschlüsselung nie ausführt, ist keiner. Dasselbe gilt für die Frage, ob
//! die Grenzen aus `SECURITY.md` auch hinter der Entschlüsselung greifen:
//! solange die Prüfdatei unverschlüsselt ist, misst der Test den Fall nicht,
//! um den es geht.
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

/// Eine **verschlüsselte Dekompressionsbombe**; Passwort wie oben.
///
/// 33 kB auf der Platte, ein Content-Stream von 32 MB nach dem Auspacken —
/// das Doppelte des Budgets `--max-parsed-mb`. Genau dieselbe Datei
/// unverschlüsselt wird von der Vorprüfung abgelehnt; verschlüsselt lief sie
/// bis zur Prüfung nach der Entschlüsselung durch, weil sich der Stream vorher
/// nicht auspacken lässt.
///
/// Gemessen mit der Fassung aus dem Befund (64 MB statt 32): Spitzenspeicher
/// über 15 GB und Abbruch durch den OOM-Killer; mit `ulimit -v 4 GB` stattdessen
/// SIGABRT nach 10,6 s bei 3 876 MB. Hier steht die kleinere Fassung, weil ein
/// Prüfstück, das erst bei mehreren Gigabyte anschlägt, kein Prüfstück ist:
/// entscheidend ist die Ablehnung, nicht die Höhe der Zahl.
pub const ENCRYPTED_BOMB_PDF: &[u8] = include_bytes!("testdata/bombe_verschluesselt.pdf");

/// Entpackte Größe des Content-Streams in [`ENCRYPTED_BOMB_PDF`], in MB.
pub const ENCRYPTED_BOMB_MB: u64 = 32;

/// Eine **verschlüsselte Verschachtelungsbombe**; Passwort wie oben.
///
/// 1,3 kB, im Content-Stream 200 000 offene `[` — dieselbe Zahl wie in
/// RUSTSEC-2026-0187. Unverschlüsselt lehnt die Vorprüfung sie ab.
/// Verschlüsselt lief sie mit Rückgabewert 0 durch und schrieb eine Ausgabe,
/// in der [`ENCRYPTED_PDF_IBAN`] unverändert stand: `lopdf` bekommt den Stream
/// nicht in Operationen zerlegt, die Analyse sieht keinen Text, und „0
/// Schwärzungen“ liest sich wie „nichts zu schwärzen“.
pub const ENCRYPTED_NESTING_BOMB_PDF: &[u8] = include_bytes!("testdata/bombe_verschachtelt.pdf");
