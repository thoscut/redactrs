//! Zusatzprüfungen für Regex-Treffer (Prüfsummen und Struktur-Checks).
//!
//! Alle Prüfungen kommen ohne zusätzliche Abhängigkeiten aus und arbeiten
//! direkt auf dem gefundenen Text (inklusive Leerzeichen und Trennzeichen).

/// Erlaubte Länderkennungen für BIC-Treffer (ISO-3166 alpha-2).
///
/// Bewusst klein gehalten: EU-27 + EWR (IS, LI, NO) + CH, GB, US. Damit werden
/// zufällige Großbuchstaben-Wörter wie `RECHNUNG` (Länderkennung `NU`)
/// zuverlässig verworfen, ohne echte deutsche/europäische BICs zu verlieren.
const ISO_COUNTRIES: &[&str] = &[
    // EU-27
    "AT", "BE", "BG", "CY", "CZ", "DE", "DK", "EE", "ES", "FI", "FR", "GR", "HR", "HU", "IE", "IT",
    "LT", "LU", "LV", "MT", "NL", "PL", "PT", "RO", "SE", "SI", "SK", // EWR ohne EU
    "IS", "LI", "NO", // Schweiz, Vereinigtes Königreich, USA
    "CH", "GB", "US",
];

/// Prüft, ob `code` eine bekannte Länderkennung ist.
pub fn is_known_country(code: &str) -> bool {
    ISO_COUNTRIES.contains(&code)
}

/// Entfernt Leerzeichen und wandelt in Großbuchstaben um.
fn compact_upper(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_uppercase())
        .collect()
}

/// IBAN-Prüfung nach ISO 13616 / ISO 7064 (mod 97 == 1).
///
/// Die Modulo-Rechnung läuft iterativ über die Ziffernfolge, es wird also
/// keine Großzahl-Arithmetik benötigt.
pub fn validate_iban(s: &str) -> bool {
    let iban = compact_upper(s);
    if !(15..=34).contains(&iban.chars().count()) {
        return false;
    }
    let bytes = iban.as_bytes();
    // Erste zwei Zeichen Buchstaben, danach zwei Prüfziffern.
    if !bytes[0].is_ascii_alphabetic()
        || !bytes[1].is_ascii_alphabetic()
        || !bytes[2].is_ascii_digit()
        || !bytes[3].is_ascii_digit()
    {
        return false;
    }
    if !bytes.iter().all(|b| b.is_ascii_alphanumeric()) {
        return false;
    }
    // Die ersten vier Zeichen wandern ans Ende.
    let rearranged = bytes[4..].iter().chain(bytes[..4].iter());
    let mut remainder: u32 = 0;
    for &b in rearranged {
        if b.is_ascii_digit() {
            remainder = (remainder * 10 + u32::from(b - b'0')) % 97;
        } else {
            // Buchstaben werden zu zwei Ziffern (A = 10 … Z = 35).
            let value = u32::from(b - b'A') + 10;
            remainder = (remainder * 100 + value) % 97;
        }
    }
    remainder == 1
}

/// BIC/SWIFT-Prüfung.
///
/// Gewählte Regel (bewusst einfach und dokumentiert):
/// 1. Länge nach Entfernen der Leerzeichen genau 8 oder 11 Zeichen.
/// 2. Zeichen 1–4 (Bankcode) müssen Buchstaben sein.
/// 3. Zeichen 5–6 müssen Buchstaben *und* eine bekannte Länderkennung aus
///    [`ISO_COUNTRIES`] sein — daran scheitern zufällige Großbuchstaben-Wörter
///    wie `RECHNUNG` (Länderkennung `NU`).
/// 4. Die restlichen Zeichen müssen alphanumerisch sein.
///
/// Bewusst *nicht* gefordert wird eine Ziffer im Ortscode: echte BICs wie
/// `DEUTDEFF` bestehen komplett aus Buchstaben und sollen gültig bleiben.
pub fn validate_bic(s: &str) -> bool {
    let bic = compact_upper(s);
    let bytes = bic.as_bytes();
    if bytes.len() != 8 && bytes.len() != 11 {
        return false;
    }
    if !bytes[..4].iter().all(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    if !bytes[4..6].iter().all(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    if !bytes[6..].iter().all(|b| b.is_ascii_alphanumeric()) {
        return false;
    }
    is_known_country(&bic[4..6])
}

/// Luhn-Prüfsumme über alle Ziffern des Treffers (Trennzeichen werden ignoriert).
pub fn validate_luhn(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 2 {
        return false;
    }
    let mut sum = 0u32;
    for (i, d) in digits.iter().rev().enumerate() {
        let mut v = *d;
        if i % 2 == 1 {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
    }
    sum % 10 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iban_valid_with_and_without_spaces() {
        assert!(validate_iban("DE89370400440532013000"));
        assert!(validate_iban("DE89 3704 0044 0532 0130 00"));
        assert!(validate_iban("de89 3704 0044 0532 0130 00"));
        assert!(validate_iban("GB82 WEST 1234 5698 7654 32"));
    }

    #[test]
    fn iban_rejects_broken_check_digits() {
        assert!(!validate_iban("DE88370400440532013000"));
        assert!(!validate_iban("DE00370400440532013000"));
        // zu kurz
        assert!(!validate_iban("DE89370400"));
        // Sonderzeichen
        assert!(!validate_iban("DE89-3704-0044-0532-0130-00"));
    }

    #[test]
    fn bic_accepts_real_codes() {
        assert!(validate_bic("DEUTDEFF"));
        assert!(validate_bic("COBADEFFXXX"));
        assert!(validate_bic("GENODEF1S01"));
        assert!(validate_bic("MARKDEF1100"));
    }

    #[test]
    fn bic_rejects_uppercase_words_and_wrong_lengths() {
        // Länderkennung "NU" steht nicht auf der Liste.
        assert!(!validate_bic("RECHNUNG"));
        assert!(!validate_bic("KUNDENNUMMER"));
        assert!(!validate_bic("DEUTDEF"));
        assert!(!validate_bic("DEUT1EFF"));
    }

    #[test]
    fn luhn_checks_card_numbers() {
        assert!(validate_luhn("4539 1488 0343 6467"));
        assert!(validate_luhn("79927398713"));
        assert!(!validate_luhn("4539 1488 0343 6468"));
        assert!(!validate_luhn("1234567890123"));
    }
}
