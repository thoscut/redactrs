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

/// Prüfsummenrechnung nach ISO 7064 (mod 97 == 1) über eine bereits
/// verdichtete Zeichenkette.
///
/// Erwartet wird der Aufbau „zwei Buchstaben, zwei Prüfziffern, Rest
/// alphanumerisch"; die ersten vier Zeichen wandern für die Rechnung ans Ende
/// und Buchstaben zählen als zwei Ziffern (A = 10 … Z = 35). Die Modulo-Rechnung
/// läuft iterativ über die Ziffernfolge, es wird also keine Großzahl-Arithmetik
/// benötigt.
///
/// Das ist die gemeinsame Grundlage von [`validate_iban`] und
/// [`validate_creditor_id`]: die SEPA-Gläubiger-ID rechnet nach exakt demselben
/// Verfahren, nur über einer um die Geschäftsbereichskennung gekürzten
/// Zeichenkette. Die zulässige *Länge* prüft jeder Aufrufer selbst — sie ist
/// bei IBAN und Gläubiger-ID verschieden.
fn passes_mod97(s: &str) -> bool {
    let bytes = s.as_bytes();
    // Vier Kopfzeichen plus mindestens eine weitere Stelle.
    if bytes.len() < 5 {
        return false;
    }
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

/// IBAN-Prüfung nach ISO 13616 / ISO 7064 (mod 97 == 1).
pub fn validate_iban(s: &str) -> bool {
    let iban = compact_upper(s);
    if !(15..=34).contains(&iban.chars().count()) {
        return false;
    }
    passes_mod97(&iban)
}

/// SEPA-Gläubiger-Identifikationsnummer (Creditor Identifier, EPC260-08).
///
/// Aufbau: Ländercode (2 Buchstaben) + 2 Prüfziffern + 3 Zeichen
/// Geschäftsbereichskennung (Creditor Business Code, üblich `ZZZ`) + nationale
/// Kennung. In Deutschland ist die nationale Kennung elf Zeichen lang, die ID
/// also insgesamt 18 Zeichen (`DE98ZZZ09999999999`); andere Länder benutzen
/// kürzere oder längere Kennungen, in der Summe höchstens 35 Zeichen.
///
/// Für die Prüfziffern gilt **dasselbe** Verfahren wie bei der IBAN — nur wird
/// die Geschäftsbereichskennung vorher entfernt: aus `DE98ZZZ09999999999` wird
/// `DE9809999999999`. Gerechnet wird deshalb mit [`passes_mod97`], derselben
/// Routine wie bei der IBAN; eine zweite Prüfsummen-Implementierung gibt es
/// bewusst nicht.
///
/// Zusätzlich muss der Ländercode aus [`ISO_COUNTRIES`] stammen — dieselbe
/// Liste wie bei der BIC-Prüfung. Sie deckt den SEPA-Raum bis auf die
/// Kleinstaaten (AD, MC, SM, VA) ab; deren Gläubiger-IDs werden verworfen.
/// Ohne diese Prüfung würde jede hinreichend lange Großbuchstaben-/Ziffernfolge
/// mit passendem Kopf zu einem 1-zu-97-Glücksspiel.
pub fn validate_creditor_id(s: &str) -> bool {
    let id = compact_upper(s);
    // Die Prüfsumme kennt nur ASCII; alles andere fällt hier heraus und macht
    // zugleich das byteweise Zerschneiden weiter unten sicher.
    if !id.is_ascii() {
        return false;
    }
    // 2 Ländercode + 2 Prüfziffern + 3 Geschäftsbereichskennung + mindestens
    // eine Stelle nationale Kennung; nach oben begrenzt EPC260-08 auf 35.
    if !(8..=35).contains(&id.len()) {
        return false;
    }
    if !is_known_country(&id[..2]) {
        return false;
    }
    // Stellen 5–7 (die Geschäftsbereichskennung) zählen nicht mit.
    let ohne_kennung = format!("{}{}", &id[..4], &id[7..]);
    passes_mod97(&ohne_kennung)
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
    sum.is_multiple_of(10)
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
    fn creditor_id_accepts_real_identifiers() {
        // Das Beispiel der Deutschen Bundesbank.
        assert!(validate_creditor_id("DE98ZZZ09999999999"));
        assert!(validate_creditor_id("de98zzz09999999999"));
        assert!(validate_creditor_id("DE24ZZZ00000561652"));
        assert!(validate_creditor_id("AT61ZZZ01234567890"));
        // Die Geschäftsbereichskennung geht nicht in die Prüfsumme ein — mit
        // einer anderen als `ZZZ` bleibt dieselbe ID gültig.
        assert!(validate_creditor_id("DE24ABC00000561652"));
        // Kürzere und längere nationale Kennungen als die deutschen elf Stellen.
        assert!(validate_creditor_id("FR72ZZZ123456"));
        assert!(validate_creditor_id("LU96ZZZ0000000000000000058"));
    }

    #[test]
    fn creditor_id_rejects_broken_check_digits_and_ibans() {
        assert!(!validate_creditor_id("DE99ZZZ09999999999"));
        assert!(!validate_creditor_id("DE98ZZZ09999999998"));
        // Eine gültige IBAN ist keine gültige Gläubiger-ID: nach dem Entfernen
        // der „Geschäftsbereichskennung" (hier drei IBAN-Stellen) geht die
        // Prüfsumme nicht mehr auf.
        assert!(!validate_creditor_id("DE89370400440532013000"));
        assert!(!validate_creditor_id("DE02120300000000202051"));
        // Zu kurz, unbekannter Ländercode, falscher Kopf, Sonderzeichen.
        assert!(!validate_creditor_id("DE98ZZZ"));
        assert!(!validate_creditor_id("XX98ZZZ09999999999"));
        assert!(!validate_creditor_id("DEZZZZZ09999999999"));
        assert!(!validate_creditor_id("DE98-ZZZ-09999999999"));
    }

    /// Die Gegenrichtung: eine Gläubiger-ID darf die IBAN-Prüfung nicht
    /// bestehen — sonst wären beide Muster nicht auseinanderzuhalten.
    #[test]
    fn creditor_id_is_not_a_valid_iban() {
        assert!(!validate_iban("DE98ZZZ09999999999"));
        assert!(!validate_iban("DE24ZZZ00000561652"));
        assert!(!validate_iban("LU96ZZZ0000000000000000058"));
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
