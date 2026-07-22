//! Hangul composition engine for Windows.
//!
//! Handles QWERTY → Jamo → Syllable block composition (두벌식),
//! then delegates to the Rust engine for Hangul → Hiragana conversion.

/// QWERTY key → Hangul Jamo mapping (두벌식 layout)
pub fn qwerty_to_jamo(ch: char) -> Option<char> {
    match ch {
        // Consonants (초성/종성)
        'r' => Some('ㄱ'), 'R' => Some('ㄲ'),
        's' => Some('ㄴ'),
        'e' => Some('ㄷ'), 'E' => Some('ㄸ'),
        'f' => Some('ㄹ'),
        'a' => Some('ㅁ'),
        'q' => Some('ㅂ'), 'Q' => Some('ㅃ'),
        't' => Some('ㅅ'), 'T' => Some('ㅆ'),
        'd' => Some('ㅇ'),
        'w' => Some('ㅈ'), 'W' => Some('ㅉ'),
        'c' => Some('ㅊ'),
        'z' => Some('ㅋ'),
        'x' => Some('ㅌ'),
        'v' => Some('ㅍ'),
        'g' => Some('ㅎ'),
        // Vowels (중성)
        'k' => Some('ㅏ'),
        'i' => Some('ㅑ'),
        'j' => Some('ㅓ'),
        'u' => Some('ㅕ'),
        'h' => Some('ㅗ'),
        'y' => Some('ㅛ'),
        'n' => Some('ㅜ'),
        'b' => Some('ㅠ'),
        'm' => Some('ㅡ'),
        'l' => Some('ㅣ'),
        'o' => Some('ㅐ'), 'O' => Some('ㅒ'),
        'p' => Some('ㅔ'), 'P' => Some('ㅖ'),
        _ => None,
    }
}

// ── Jamo classification ─────────────────────────────────

const CHOSEONG: [char; 19] = [
    'ㄱ','ㄲ','ㄴ','ㄷ','ㄸ','ㄹ','ㅁ','ㅂ','ㅃ','ㅅ',
    'ㅆ','ㅇ','ㅈ','ㅉ','ㅊ','ㅋ','ㅌ','ㅍ','ㅎ',
];

const JUNGSEONG: [char; 21] = [
    'ㅏ','ㅐ','ㅑ','ㅒ','ㅓ','ㅔ','ㅕ','ㅖ','ㅗ','ㅘ',
    'ㅙ','ㅚ','ㅛ','ㅜ','ㅝ','ㅞ','ㅟ','ㅠ','ㅡ','ㅢ','ㅣ',
];

const JONGSEONG: [char; 27] = [
    'ㄱ','ㄲ','ㄳ','ㄴ','ㄵ','ㄶ','ㄷ','ㄹ','ㄺ','ㄻ',
    'ㄼ','ㄽ','ㄾ','ㄿ','ㅀ','ㅁ','ㅂ','ㅄ','ㅅ','ㅆ',
    'ㅇ','ㅈ','ㅊ','ㅋ','ㅌ','ㅍ','ㅎ',
];

pub fn cho_index(c: char) -> Option<u32> {
    CHOSEONG.iter().position(|&x| x == c).map(|i| i as u32)
}

pub fn jung_index(c: char) -> Option<u32> {
    JUNGSEONG.iter().position(|&x| x == c).map(|i| i as u32)
}

pub fn jong_index(c: char) -> Option<u32> {
    JONGSEONG.iter().position(|&x| x == c).map(|i| (i + 1) as u32) // +1 because 0 = no jongseong
}

pub fn is_consonant(c: char) -> bool {
    cho_index(c).is_some()
}

pub fn is_vowel(c: char) -> bool {
    jung_index(c).is_some()
}

/// Compose a Hangul syllable from cho + jung + optional jong indices.
pub fn compose_syllable(cho: u32, jung: u32, jong: u32) -> Option<char> {
    let code = 0xAC00 + cho * 21 * 28 + jung * 28 + jong;
    char::from_u32(code)
}

/// Compound vowel (ㅗ+ㅏ→ㅘ, etc.)
pub fn compound_jung(a: u32, b: u32) -> Option<u32> {
    match (a, b) {
        (8, 0) => Some(9),    // ㅗ+ㅏ→ㅘ
        (8, 1) => Some(10),   // ㅗ+ㅐ→ㅙ
        (8, 20) => Some(11),  // ㅗ+ㅣ→ㅚ
        (13, 4) => Some(14),  // ㅜ+ㅓ→ㅝ
        (13, 5) => Some(15),  // ㅜ+ㅔ→ㅞ
        (13, 20) => Some(16), // ㅜ+ㅣ→ㅟ
        (18, 20) => Some(19), // ㅡ+ㅣ→ㅢ
        _ => None,
    }
}

// ── Jamo buffer → Syllable string ───────────────────────

/// Compose a sequence of jamo characters into Hangul syllable blocks.
pub fn compose_jamo_buffer(jamos: &[char]) -> String {
    let mut result = String::new();
    let mut i = 0;

    while i < jamos.len() {
        let ch = jamos[i];

        if let Some(cho_idx) = cho_index(ch) {
            // Need a vowel next
            if i + 1 < jamos.len() {
                if let Some(jung_idx) = jung_index(jamos[i + 1]) {
                    // Check compound vowel
                    let mut final_jung = jung_idx;
                    let mut vowel_len = 1;
                    if i + 2 < jamos.len() {
                        if let Some(next_jung) = jung_index(jamos[i + 2]) {
                            if let Some(compound) = compound_jung(jung_idx, next_jung) {
                                final_jung = compound;
                                vowel_len = 2;
                            }
                        }
                    }

                    // Check jongseong
                    let after_vowel = i + 1 + vowel_len;
                    if after_vowel < jamos.len() {
                        if let Some(jong_idx) = jong_index(jamos[after_vowel]) {
                            // Is next char a vowel? → this consonant starts new syllable
                            if after_vowel + 1 < jamos.len() && is_vowel(jamos[after_vowel + 1]) {
                                if let Some(s) = compose_syllable(cho_idx, final_jung, 0) {
                                    result.push(s);
                                }
                                i += 1 + vowel_len;
                                continue;
                            }

                            if let Some(s) = compose_syllable(cho_idx, final_jung, jong_idx) {
                                result.push(s);
                            }
                            i += 1 + vowel_len + 1;
                            continue;
                        }
                    }

                    // No jongseong
                    if let Some(s) = compose_syllable(cho_idx, final_jung, 0) {
                        result.push(s);
                    }
                    i += 1 + vowel_len;
                    continue;
                }
            }
            // Consonant alone
            result.push(ch);
            i += 1;
            continue;
        }

        // Vowel alone or unknown
        result.push(ch);
        i += 1;
    }

    result
}
