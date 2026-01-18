//! String transformation utilities.

use crate::core::{CaseType, NumberFormat};
use regex::Regex;
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;

/// Transform case of a string.
pub fn transform_case(input: &str, case_type: CaseType) -> String {
    match case_type {
        CaseType::Lower => input.to_lowercase(),
        CaseType::Upper => input.to_uppercase(),
        CaseType::Title => to_title_case(input),
        CaseType::Sentence => to_sentence_case(input),
        CaseType::Toggle => toggle_case(input),
        CaseType::Camel => to_camel_case(input),
        CaseType::Pascal => to_pascal_case(input),
        CaseType::Snake => to_snake_case(input),
        CaseType::Kebab => to_kebab_case(input),
        CaseType::Constant => to_constant_case(input),
        CaseType::Capitalize => capitalize_first(input),
        CaseType::Alternating => to_alternating_case(input),
        CaseType::Random => to_random_case(input),
    }
}

/// Convert to Title Case.
fn to_title_case(input: &str) -> String {
    // Use the titlecase crate for proper handling
    titlecase::titlecase(input)
}

/// Convert to Sentence case.
fn to_sentence_case(input: &str) -> String {
    let lower = input.to_lowercase();
    let mut chars: Vec<char> = lower.chars().collect();
    let mut capitalize_next = true;

    for ch in chars.iter_mut() {
        if capitalize_next && ch.is_alphabetic() {
            *ch = ch.to_uppercase().next().unwrap_or(*ch);
            capitalize_next = false;
        }
        if *ch == '.' || *ch == '!' || *ch == '?' {
            capitalize_next = true;
        }
    }

    chars.into_iter().collect()
}

/// Toggle case (swap uppercase and lowercase).
fn toggle_case(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_uppercase() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                c.to_uppercase().next().unwrap_or(c)
            }
        })
        .collect()
}

/// Convert to camelCase.
fn to_camel_case(input: &str) -> String {
    let pascal = to_pascal_case(input);
    let mut chars: Vec<char> = pascal.chars().collect();
    if let Some(first) = chars.first_mut() {
        *first = first.to_lowercase().next().unwrap_or(*first);
    }
    chars.into_iter().collect()
}

/// Convert to PascalCase.
fn to_pascal_case(input: &str) -> String {
    split_into_words(input)
        .into_iter()
        .map(|w| capitalize_first(&w.to_lowercase()))
        .collect()
}

/// Convert to snake_case.
fn to_snake_case(input: &str) -> String {
    split_into_words(input)
        .into_iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// Convert to kebab-case.
fn to_kebab_case(input: &str) -> String {
    split_into_words(input)
        .into_iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join("-")
}

/// Convert to CONSTANT_CASE.
fn to_constant_case(input: &str) -> String {
    split_into_words(input)
        .into_iter()
        .map(|w| w.to_uppercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// Capitalize first letter.
fn capitalize_first(input: &str) -> String {
    let mut chars: Vec<char> = input.chars().collect();
    if let Some(first) = chars.first_mut() {
        *first = first.to_uppercase().next().unwrap_or(*first);
    }
    chars.into_iter().collect()
}

/// Convert to aLtErNaTiNg CaSe.
fn to_alternating_case(input: &str) -> String {
    input
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if i % 2 == 0 {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                c.to_uppercase().next().unwrap_or(c)
            }
        })
        .collect()
}

/// Convert to RaNdOm CaSe.
fn to_random_case(input: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    input
        .chars()
        .enumerate()
        .map(|(i, c)| {
            let mut hasher = DefaultHasher::new();
            (input, i).hash(&mut hasher);
            if hasher.finish() % 2 == 0 {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                c.to_uppercase().next().unwrap_or(c)
            }
        })
        .collect()
}

/// Split a string into words for case conversion.
fn split_into_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current_word = String::new();
    let mut prev_is_lower = false;

    for grapheme in input.graphemes(true) {
        let c = grapheme.chars().next().unwrap_or(' ');

        if c.is_whitespace() || c == '_' || c == '-' || c == '.' {
            if !current_word.is_empty() {
                words.push(std::mem::take(&mut current_word));
            }
            prev_is_lower = false;
        } else if c.is_uppercase() && prev_is_lower {
            // camelCase boundary
            if !current_word.is_empty() {
                words.push(std::mem::take(&mut current_word));
            }
            current_word.push(c);
            prev_is_lower = false;
        } else {
            current_word.push(c);
            prev_is_lower = c.is_lowercase();
        }
    }

    if !current_word.is_empty() {
        words.push(current_word);
    }

    words
}

/// Format a number with the specified format and padding.
pub fn format_number(number: i64, format: NumberFormat, padding: usize) -> String {
    match format {
        NumberFormat::Decimal => format!("{:0>width$}", number, width = padding),
        NumberFormat::Hex => format!("{:0>width$x}", number, width = padding),
        NumberFormat::Octal => format!("{:0>width$o}", number, width = padding),
        NumberFormat::Binary => format!("{:0>width$b}", number, width = padding),
        NumberFormat::Roman => to_roman(number as u32),
        NumberFormat::Letter => to_letter(number),
    }
}

/// Convert number to Roman numerals.
fn to_roman(mut num: u32) -> String {
    let numerals = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];

    let mut result = String::new();
    for (value, symbol) in numerals {
        while num >= value {
            result.push_str(symbol);
            num -= value;
        }
    }
    result
}

/// Convert number to letter (A, B, C, ..., Z, AA, AB, ...).
fn to_letter(num: i64) -> String {
    if num <= 0 {
        return String::from("A");
    }

    let mut n = num - 1;
    let mut result = String::new();

    loop {
        result.insert(0, (b'A' + (n % 26) as u8) as char);
        n = n / 26 - 1;
        if n < 0 {
            break;
        }
    }

    result
}

/// Remove diacritics from a string.
pub fn remove_diacritics(input: &str) -> String {
    let mut result = String::with_capacity(input.len());

    for c in input.chars() {
        // Common diacritics to ASCII mapping
        let replacement = match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => 'a',
            'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Ā' => 'A',
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' => 'e',
            'É' | 'È' | 'Ê' | 'Ë' | 'Ē' | 'Ė' | 'Ę' => 'E',
            'í' | 'ì' | 'î' | 'ï' | 'ī' | 'į' => 'i',
            'Í' | 'Ì' | 'Î' | 'Ï' | 'Ī' | 'Į' => 'I',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ō' | 'ø' => 'o',
            'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ō' | 'Ø' => 'O',
            'ú' | 'ù' | 'û' | 'ü' | 'ū' => 'u',
            'Ú' | 'Ù' | 'Û' | 'Ü' | 'Ū' => 'U',
            'ý' | 'ÿ' => 'y',
            'Ý' | 'Ÿ' => 'Y',
            'ñ' => 'n',
            'Ñ' => 'N',
            'ç' | 'ć' | 'č' => 'c',
            'Ç' | 'Ć' | 'Č' => 'C',
            'ß' => 's',
            'ł' => 'l',
            'Ł' => 'L',
            'ś' | 'š' => 's',
            'Ś' | 'Š' => 'S',
            'ź' | 'ż' | 'ž' => 'z',
            'Ź' | 'Ż' | 'Ž' => 'Z',
            'ą' => 'a',
            'Ą' => 'A',
            'ğ' => 'g',
            'Ğ' => 'G',
            'ş' => 's',
            'Ş' => 'S',
            'ı' => 'i',
            'İ' => 'I',
            _ => c,
        };
        result.push(replacement);
    }

    result
}

/// Transliterate Cyrillic to Latin.
pub fn cyrillic_to_latin(input: &str) -> String {
    let mapping: HashMap<char, &str> = [
        ('а', "a"), ('б', "b"), ('в', "v"), ('г', "g"), ('д', "d"),
        ('е', "e"), ('ё', "yo"), ('ж', "zh"), ('з', "z"), ('и', "i"),
        ('й', "y"), ('к', "k"), ('л', "l"), ('м', "m"), ('н', "n"),
        ('о', "o"), ('п', "p"), ('р', "r"), ('с', "s"), ('т', "t"),
        ('у', "u"), ('ф', "f"), ('х', "kh"), ('ц', "ts"), ('ч', "ch"),
        ('ш', "sh"), ('щ', "shch"), ('ъ', ""), ('ы', "y"), ('ь', ""),
        ('э', "e"), ('ю', "yu"), ('я', "ya"),
        ('А', "A"), ('Б', "B"), ('В', "V"), ('Г', "G"), ('Д', "D"),
        ('Е', "E"), ('Ё', "Yo"), ('Ж', "Zh"), ('З', "Z"), ('И', "I"),
        ('Й', "Y"), ('К', "K"), ('Л', "L"), ('М', "M"), ('Н', "N"),
        ('О', "O"), ('П', "P"), ('Р', "R"), ('С', "S"), ('Т', "T"),
        ('У', "U"), ('Ф', "F"), ('Х', "Kh"), ('Ц', "Ts"), ('Ч', "Ch"),
        ('Ш', "Sh"), ('Щ', "Shch"), ('Ъ', ""), ('Ы', "Y"), ('Ь', ""),
        ('Э', "E"), ('Ю', "Yu"), ('Я', "Ya"),
    ].iter().cloned().collect();

    input.chars().map(|c| {
        mapping.get(&c).map(|s| s.to_string()).unwrap_or_else(|| c.to_string())
    }).collect()
}

/// Transliterate Greek to Latin.
pub fn greek_to_latin(input: &str) -> String {
    let mapping: HashMap<char, &str> = [
        ('α', "a"), ('β', "b"), ('γ', "g"), ('δ', "d"), ('ε', "e"),
        ('ζ', "z"), ('η', "i"), ('θ', "th"), ('ι', "i"), ('κ', "k"),
        ('λ', "l"), ('μ', "m"), ('ν', "n"), ('ξ', "x"), ('ο', "o"),
        ('π', "p"), ('ρ', "r"), ('σ', "s"), ('ς', "s"), ('τ', "t"),
        ('υ', "y"), ('φ', "f"), ('χ', "ch"), ('ψ', "ps"), ('ω', "o"),
        ('Α', "A"), ('Β', "B"), ('Γ', "G"), ('Δ', "D"), ('Ε', "E"),
        ('Ζ', "Z"), ('Η', "I"), ('Θ', "Th"), ('Ι', "I"), ('Κ', "K"),
        ('Λ', "L"), ('Μ', "M"), ('Ν', "N"), ('Ξ', "X"), ('Ο', "O"),
        ('Π', "P"), ('Ρ', "R"), ('Σ', "S"), ('Τ', "T"), ('Υ', "Y"),
        ('Φ', "F"), ('Χ', "Ch"), ('Ψ', "Ps"), ('Ω', "O"),
    ].iter().cloned().collect();

    input.chars().map(|c| {
        mapping.get(&c).map(|s| s.to_string()).unwrap_or_else(|| c.to_string())
    }).collect()
}

/// Remove characters from a string by type.
pub fn remove_by_type(input: &str, remove_digits: bool, remove_letters: bool, remove_symbols: bool) -> String {
    input.chars().filter(|c| {
        if remove_digits && c.is_ascii_digit() {
            return false;
        }
        if remove_letters && c.is_alphabetic() {
            return false;
        }
        if remove_symbols && !c.is_alphanumeric() && !c.is_whitespace() {
            return false;
        }
        true
    }).collect()
}

/// Collapse multiple spaces into one.
pub fn collapse_spaces(input: &str) -> String {
    use std::sync::OnceLock;
    static WHITESPACE_RE: OnceLock<Regex> = OnceLock::new();
    let re = WHITESPACE_RE.get_or_init(|| {
        Regex::new(r"\s+").expect("valid whitespace regex")
    });
    re.replace_all(input, " ").trim().to_string()
}

/// Remove bracketed content.
pub fn remove_bracketed(input: &str, open: char, close: char) -> String {
    let mut result = String::new();
    let mut depth = 0;

    for c in input.chars() {
        if c == open {
            depth += 1;
        } else if c == close && depth > 0 {
            depth -= 1;
        } else if depth == 0 {
            result.push(c);
        }
    }

    collapse_spaces(&result)
}

/// Insert text at a specific position.
pub fn insert_at_position(input: &str, text: &str, position: i32) -> String {
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len() as i32;

    let pos = if position < 0 {
        (len + position + 1).max(0) as usize
    } else {
        position.min(len) as usize
    };

    let mut result: String = chars[..pos].iter().collect();
    result.push_str(text);
    result.push_str(&chars[pos..].iter().collect::<String>());

    result
}

/// Remove characters at a range.
pub fn remove_range(input: &str, start: i32, end: i32) -> String {
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len() as i32;

    let start_pos = if start < 0 {
        (len + start).max(0) as usize
    } else {
        start.min(len) as usize
    };

    let end_pos = if end < 0 {
        (len + end + 1).max(0) as usize
    } else {
        end.min(len) as usize
    };

    if start_pos >= end_pos {
        return input.to_string();
    }

    let mut result: String = chars[..start_pos].iter().collect();
    result.push_str(&chars[end_pos..].iter().collect::<String>());

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_title_case() {
        assert_eq!(to_title_case("hello world"), "Hello World");
    }

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("HelloWorld"), "hello_world");
        assert_eq!(to_snake_case("hello world"), "hello_world");
    }

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("hello world"), "helloWorld");
        assert_eq!(to_camel_case("Hello World"), "helloWorld");
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(5, NumberFormat::Decimal, 3), "005");
        assert_eq!(format_number(15, NumberFormat::Hex, 2), "0f");
        assert_eq!(format_number(4, NumberFormat::Roman, 0), "IV");
        assert_eq!(format_number(1, NumberFormat::Letter, 0), "A");
        assert_eq!(format_number(27, NumberFormat::Letter, 0), "AA");
    }

    #[test]
    fn test_remove_diacritics() {
        assert_eq!(remove_diacritics("café"), "cafe");
        assert_eq!(remove_diacritics("naïve"), "naive");
    }

    #[test]
    fn test_insert_at_position() {
        assert_eq!(insert_at_position("hello", "_", 0), "_hello");
        assert_eq!(insert_at_position("hello", "_", 5), "hello_");
        assert_eq!(insert_at_position("hello", "_", -1), "hello_");
        assert_eq!(insert_at_position("hello", "_", 2), "he_llo");
    }
}
