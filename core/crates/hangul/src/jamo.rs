//! Hangul Jamo constants and decomposition utilities.
//!
//! Unicode Hangul syllables are computed as:
//!   syllable = 0xAC00 + (choseong * 21 + jungseong) * 28 + jongseong

/// Unicode offset for Hangul syllable block.
pub const SYLLABLE_BASE: u32 = 0xAC00;

/// Number of Jungseong (vowels).
pub const JUNGSEONG_COUNT: u32 = 21;

/// Number of Jongseong (final consonants), including no-jongseong.
pub const JONGSEONG_COUNT: u32 = 28;

/// Choseong (initial consonants) in Unicode order.
/// Index 0~18: ㄱ ㄲ ㄴ ㄷ ㄸ ㄹ ㅁ ㅂ ㅃ ㅅ ㅆ ㅇ ㅈ ㅉ ㅊ ㅋ ㅌ ㅍ ㅎ
pub const CHOSEONG: &[char] = &[
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ',
    'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];

/// Jungseong (vowels) in Unicode order.
/// Index 0~20: ㅏ ㅐ ㅑ ㅒ ㅓ ㅔ ㅕ ㅖ ㅗ ㅘ ㅙ ㅚ ㅛ ㅜ ㅝ ㅞ ㅟ ㅠ ㅡ ㅢ ㅣ
pub const JUNGSEONG: &[char] = &[
    'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ',
    'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ', 'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ',
    'ㅣ',
];

/// Jongseong (final consonants) in Unicode order.
/// Index 0 = no jongseong, 1~27: ㄱ ㄲ ㄳ ㄴ ㄵ ㄶ ㄷ ㄹ ㄺ ㄻ ㄼ ㄽ ㄾ ㄿ ㅀ ㅁ ㅂ ㅄ ㅅ ㅆ ㅇ ㅈ ㅊ ㅋ ㅌ ㅍ ㅎ
pub const JONGSEONG: &[char] = &[
    '\0', 'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ',
    'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ', 'ㅀ', 'ㅁ', 'ㅂ', 'ㅄ', 'ㅅ',
    'ㅆ', 'ㅇ', 'ㅈ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];

/// Decomposed Hangul syllable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jamo {
    /// Initial consonant index (0~18)
    pub choseong: u32,
    /// Vowel index (0~20)
    pub jungseong: u32,
    /// Final consonant index (0 = none, 1~27)
    pub jongseong: u32,
}

impl Jamo {
    /// Get the choseong character.
    pub fn cho_char(&self) -> char {
        CHOSEONG[self.choseong as usize]
    }

    /// Get the jungseong character.
    pub fn jung_char(&self) -> char {
        JUNGSEONG[self.jungseong as usize]
    }

    /// Get the jongseong character, or None if absent.
    pub fn jong_char(&self) -> Option<char> {
        if self.jongseong == 0 {
            None
        } else {
            Some(JONGSEONG[self.jongseong as usize])
        }
    }
}

/// Check if a character is a Hangul syllable (가~힣).
pub fn is_hangul_syllable(ch: char) -> bool {
    let code = ch as u32;
    (0xAC00..=0xD7A3).contains(&code)
}

/// Decompose a Hangul syllable into choseong, jungseong, jongseong indices.
/// Returns None if the character is not a Hangul syllable.
pub fn decompose(ch: char) -> Option<Jamo> {
    if !is_hangul_syllable(ch) {
        return None;
    }
    let code = ch as u32 - SYLLABLE_BASE;
    let jongseong = code % JONGSEONG_COUNT;
    let jungseong = (code / JONGSEONG_COUNT) % JUNGSEONG_COUNT;
    let choseong = code / (JUNGSEONG_COUNT * JONGSEONG_COUNT);
    Some(Jamo {
        choseong,
        jungseong,
        jongseong,
    })
}

/// Compose a Hangul syllable from choseong, jungseong, and optional jongseong indices.
pub fn compose_syllable(cho: u32, jung: u32, jong: u32) -> Option<char> {
    let code = SYLLABLE_BASE + (cho * JUNGSEONG_COUNT + jung) * JONGSEONG_COUNT + jong;
    char::from_u32(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_ga() {
        let syllable = compose_syllable(0, 0, 0).unwrap();
        assert_eq!(syllable, '가');
    }

    #[test]
    fn compose_han() {
        // ㅎ(18) + ㅏ(0) + ㄴ(4) = 한
        let syllable = compose_syllable(18, 0, 4).unwrap();
        assert_eq!(syllable, '한');
    }

    #[test]
    fn decompose_ga() {
        let jamo = decompose('가').unwrap();
        assert_eq!(jamo.choseong, 0);   // ㄱ
        assert_eq!(jamo.jungseong, 0);  // ㅏ
        assert_eq!(jamo.jongseong, 0);  // none
    }

    #[test]
    fn decompose_han() {
        let jamo = decompose('한').unwrap();
        assert_eq!(jamo.cho_char(), 'ㅎ');
        assert_eq!(jamo.jung_char(), 'ㅏ');
        assert_eq!(jamo.jong_char(), Some('ㄴ'));
    }

    #[test]
    fn decompose_recompose_roundtrip() {
        for ch in ['가', '나', '다', '한', '글', '일', '본', '어'] {
            let jamo = decompose(ch).unwrap();
            let recomposed = compose_syllable(jamo.choseong, jamo.jungseong, jamo.jongseong);
            assert_eq!(recomposed, Some(ch));
        }
    }

    #[test]
    fn non_hangul_returns_none() {
        assert!(decompose('A').is_none());
        assert!(decompose('あ').is_none());
    }
}
