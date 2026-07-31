//! Abbildung von PostScript-Glyphnamen auf Unicode.
//!
//! Wird für `/Differences`-Encodings gebraucht. Es ist bewusst nur eine
//! Teilmenge der Adobe Glyph List: alles, was in deutschen Geschäfts- und
//! Bankdokumenten vorkommt (ASCII, Latin-1, typografische Zeichen, Euro).
//! Unbekannte Namen liefern `None`; der Aufrufer setzt dann ein
//! Ersatzzeichen ein, damit die Zeichen-Indizes trotzdem stimmen.

/// Löst einen Glyphnamen in seinen Text auf.
///
/// Anders als [`glyph_name_to_char`] kann das Ergebnis mehrere Zeichen
/// umfassen: `uni00660069` ist die Ligatur „fi“ und steht für *zwei*
/// Buchstaben. Wer davon nur den ersten übernimmt, verliert das „i“ — und mit
/// ihm womöglich den Treffer.
pub fn glyph_name_to_text(name: &str) -> Option<String> {
    if let Some(c) = lookup(name) {
        return Some(c.to_string());
    }

    // uniXXXX — eine oder mehrere UTF-16-Einheiten.
    if let Some(text) = uni_name_to_text(name) {
        return Some(text);
    }

    // uXXXX bis uXXXXXX — genau ein Codepoint.
    if let Some(hex) = name.strip_prefix('u') {
        if (4..=6).contains(&hex.len()) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            if let Ok(cp) = u32::from_str_radix(hex, 16) {
                return char::from_u32(cp).map(|c| c.to_string());
            }
        }
    }

    // Varianten-Suffix abschneiden: "a.sc" -> "a", "uni0066.alt" -> "f".
    if let Some((base, _)) = name.split_once('.') {
        if !base.is_empty() {
            return glyph_name_to_text(base);
        }
    }

    None
}

/// `uniXXXX`, `uniXXXXXXXX`, … — beliebig viele UTF-16-Einheiten.
///
/// Surrogatpaare werden dabei korrekt zusammengesetzt.
fn uni_name_to_text(name: &str) -> Option<String> {
    let hex = name.strip_prefix("uni")?;
    if hex.is_empty() || hex.len() % 4 != 0 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let units: Vec<u16> = hex
        .as_bytes()
        .chunks(4)
        .filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
        .collect();
    if units.len() * 4 != hex.len() {
        return None;
    }
    let text = String::from_utf16(&units).ok()?;
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Löst einen Glyphnamen in ein *einzelnes* Zeichen auf.
///
/// Unterstützt zusätzlich die algorithmischen Namensformen `uniXXXX`,
/// `uXXXX[XX]` sowie Suffixe wie `a.sc` oder `one.oldstyle`. Namen, die für
/// mehrere Zeichen stehen (Ligaturen), liefern `None` — dafür ist
/// [`glyph_name_to_text`] zuständig.
pub fn glyph_name_to_char(name: &str) -> Option<char> {
    let text = glyph_name_to_text(name)?;
    let mut chars = text.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

fn lookup(name: &str) -> Option<char> {
    // Einzelne ASCII-Zeichen sind ihr eigener Glyphname ("a", "A", "1").
    if name.len() == 1 {
        let c = name.chars().next()?;
        if c.is_ascii_alphanumeric() {
            return Some(c);
        }
    }
    let c = match name {
        "space" | "uni00A0" | "nbspace" => ' ',
        "exclam" => '!',
        "quotedbl" => '"',
        "numbersign" => '#',
        "dollar" => '$',
        "percent" => '%',
        "ampersand" => '&',
        "quotesingle" => '\'',
        "quoteright" => '\u{2019}',
        "quoteleft" => '\u{2018}',
        "parenleft" => '(',
        "parenright" => ')',
        "asterisk" => '*',
        "plus" => '+',
        "comma" => ',',
        "hyphen" | "sfthyphen" => '-',
        "period" => '.',
        "slash" => '/',
        "zero" => '0',
        "one" => '1',
        "two" => '2',
        "three" => '3',
        "four" => '4',
        "five" => '5',
        "six" => '6',
        "seven" => '7',
        "eight" => '8',
        "nine" => '9',
        "colon" => ':',
        "semicolon" => ';',
        "less" => '<',
        "equal" => '=',
        "greater" => '>',
        "question" => '?',
        "at" => '@',
        "bracketleft" => '[',
        "backslash" => '\\',
        "bracketright" => ']',
        "asciicircum" => '^',
        "underscore" => '_',
        "grave" => '`',
        "braceleft" => '{',
        "bar" => '|',
        "braceright" => '}',
        "asciitilde" => '~',

        // Latin-1 Ergänzungen
        "exclamdown" => '¡',
        "cent" => '¢',
        "sterling" => '£',
        "currency" => '¤',
        "yen" => '¥',
        "brokenbar" => '¦',
        "section" => '§',
        "dieresis" => '¨',
        "copyright" => '©',
        "ordfeminine" => 'ª',
        "guillemotleft" => '«',
        "logicalnot" => '¬',
        "registered" => '®',
        "macron" => '¯',
        "degree" => '°',
        "plusminus" => '±',
        "twosuperior" => '²',
        "threesuperior" => '³',
        "acute" => '´',
        "mu" => 'µ',
        "paragraph" => '¶',
        "periodcentered" => '·',
        "cedilla" => '¸',
        "onesuperior" => '¹',
        "ordmasculine" => 'º',
        "guillemotright" => '»',
        "onequarter" => '¼',
        "onehalf" => '½',
        "threequarters" => '¾',
        "questiondown" => '¿',
        "Agrave" => 'À',
        "Aacute" => 'Á',
        "Acircumflex" => 'Â',
        "Atilde" => 'Ã',
        "Adieresis" => 'Ä',
        "Aring" => 'Å',
        "AE" => 'Æ',
        "Ccedilla" => 'Ç',
        "Egrave" => 'È',
        "Eacute" => 'É',
        "Ecircumflex" => 'Ê',
        "Edieresis" => 'Ë',
        "Igrave" => 'Ì',
        "Iacute" => 'Í',
        "Icircumflex" => 'Î',
        "Idieresis" => 'Ï',
        "Eth" => 'Ð',
        "Ntilde" => 'Ñ',
        "Ograve" => 'Ò',
        "Oacute" => 'Ó',
        "Ocircumflex" => 'Ô',
        "Otilde" => 'Õ',
        "Odieresis" => 'Ö',
        "multiply" => '×',
        "Oslash" => 'Ø',
        "Ugrave" => 'Ù',
        "Uacute" => 'Ú',
        "Ucircumflex" => 'Û',
        "Udieresis" => 'Ü',
        "Yacute" => 'Ý',
        "Thorn" => 'Þ',
        "germandbls" => 'ß',
        "agrave" => 'à',
        "aacute" => 'á',
        "acircumflex" => 'â',
        "atilde" => 'ã',
        "adieresis" => 'ä',
        "aring" => 'å',
        "ae" => 'æ',
        "ccedilla" => 'ç',
        "egrave" => 'è',
        "eacute" => 'é',
        "ecircumflex" => 'ê',
        "edieresis" => 'ë',
        "igrave" => 'ì',
        "iacute" => 'í',
        "icircumflex" => 'î',
        "idieresis" => 'ï',
        "eth" => 'ð',
        "ntilde" => 'ñ',
        "ograve" => 'ò',
        "oacute" => 'ó',
        "ocircumflex" => 'ô',
        "otilde" => 'õ',
        "odieresis" => 'ö',
        "divide" => '÷',
        "oslash" => 'ø',
        "ugrave" => 'ù',
        "uacute" => 'ú',
        "ucircumflex" => 'û',
        "udieresis" => 'ü',
        "yacute" => 'ý',
        "thorn" => 'þ',
        "ydieresis" => 'ÿ',

        // Typografie / Sonderzeichen
        "quotedblleft" => '\u{201C}',
        "quotedblright" => '\u{201D}',
        "quotedblbase" => '\u{201E}',
        "quotesinglbase" => '\u{201A}',
        "endash" => '\u{2013}',
        "emdash" => '\u{2014}',
        "bullet" => '\u{2022}',
        "ellipsis" => '\u{2026}',
        "dagger" => '\u{2020}',
        "daggerdbl" => '\u{2021}',
        "perthousand" => '\u{2030}',
        "guilsinglleft" => '\u{2039}',
        "guilsinglright" => '\u{203A}',
        "Euro" | "euro" => '\u{20AC}',
        "trademark" => '\u{2122}',
        "fi" => '\u{FB01}',
        "fl" => '\u{FB02}',
        "OE" => 'Œ',
        "oe" => 'œ',
        "Scaron" => 'Š',
        "scaron" => 'š',
        "Zcaron" => 'Ž',
        "zcaron" => 'ž',
        "Ydieresis" => 'Ÿ',
        "florin" => 'ƒ',
        "circumflex" => 'ˆ',
        "tilde" => '˜',
        "breve" => '˘',
        "dotaccent" => '˙',
        "ring" => '˚',
        "ogonek" => '˛',
        "caron" => 'ˇ',
        "hungarumlaut" => '˝',
        "minus" => '\u{2212}',
        "fraction" => '\u{2044}',
        "notdef" | ".notdef" => return None,
        _ => return None,
    };
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_german_umlauts() {
        assert_eq!(glyph_name_to_char("adieresis"), Some('ä'));
        assert_eq!(glyph_name_to_char("Udieresis"), Some('Ü'));
        assert_eq!(glyph_name_to_char("germandbls"), Some('ß'));
        assert_eq!(glyph_name_to_char("Euro"), Some('€'));
    }

    #[test]
    fn resolves_algorithmic_names() {
        assert_eq!(glyph_name_to_char("uni0041"), Some('A'));
        assert_eq!(glyph_name_to_char("u20AC"), Some('€'));
        assert_eq!(glyph_name_to_char("a.sc"), Some('a'));
    }

    #[test]
    fn multi_unit_uni_names_keep_every_character() {
        // `uni00660069` ist die Ligatur „fi“ — das „i“ darf nicht verloren gehen.
        assert_eq!(glyph_name_to_text("uni00660069").as_deref(), Some("fi"));
        assert_eq!(glyph_name_to_text("uni0041").as_deref(), Some("A"));
        assert_eq!(glyph_name_to_text("adieresis").as_deref(), Some("ä"));
        assert_eq!(glyph_name_to_text("g42"), None);
    }

    #[test]
    fn unknown_is_none() {
        assert_eq!(glyph_name_to_char("g42"), None);
        assert_eq!(glyph_name_to_char(".notdef"), None);
    }
}
