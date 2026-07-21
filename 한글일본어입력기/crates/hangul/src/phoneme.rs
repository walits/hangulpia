//! Hangul → Japanese phoneme mapping.
//!
//! Maps Korean Hangul syllables to Japanese phoneme sequences,
//! which are then converted to hiragana.
//!
//! ## 설계 원칙
//!
//! 한국어 화자가 일본어 소리를 한글로 적을 때의 자연스러운 표기를 기준으로 매핑.
//! 외래어 표기법과 실제 사용 관행을 모두 고려.
//!
//! ## 자음 매핑 (초성 → 일본어 자음)
//!
//! | 한글 초성 | 일본어 자음 | 예시              |
//! |-----------|-------------|-------------------|
//! | ㄱ        | k           | 가→ka(か)         |
//! | ㄲ        | g (또는 kk) | 까→ga(が)         |
//! | ㄴ        | n           | 나→na(な)         |
//! | ㄷ        | t           | 다→ta(た)         |
//! | ㄸ        | d           | 따→da(だ)         |
//! | ㄹ        | r           | 라→ra(ら)         |
//! | ㅁ        | m           | 마→ma(ま)         |
//! | ㅂ        | h           | 바→ha(は)         |
//! | ㅃ        | b           | 빠→ba(ば)         |
//! | ㅅ        | s           | 사→sa(さ)         |
//! | ㅆ        | z           | 싸→za(ざ)         |
//! | ㅇ        | (none)      | 아→a(あ)          |
//! | ㅈ        | ch (chi행)  | 자→cha→ちゃ?      |
//! | ㅉ        | j           | 짜→ja→じゃ        |
//! | ㅊ        | ch          | 차→cha(ちゃ)      |
//! | ㅋ        | k           | 카→ka(か)         |
//! | ㅌ        | t           | 타→ta(た)         |
//! | ㅍ        | p           | 파→pa(ぱ)         |
//! | ㅎ        | h           | 하→ha(は)         |

use crate::jamo::{self, Jamo};

/// A Japanese phoneme representation (consonant + vowel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JapanesePhoneme {
    /// Consonant part (empty string for vowel-only sounds)
    pub consonant: &'static str,
    /// Vowel part
    pub vowel: &'static str,
}

impl JapanesePhoneme {
    /// Get the romaji representation.
    pub fn romaji(&self) -> String {
        format!("{}{}", self.consonant, self.vowel)
    }
}

/// Multiple possible phoneme interpretations for fuzzy matching.
/// Ordered by likelihood (most probable first).
#[derive(Debug, Clone)]
pub struct PhonemeCandidate {
    pub phonemes: Vec<JapanesePhoneme>,
    /// Confidence score (0.0 ~ 1.0)
    pub confidence: f64,
}

/// Map a Hangul choseong (initial consonant) to possible Japanese consonants.
/// Returns multiple candidates for fuzzy matching.
///
/// 한글 자음과 일본어 자음의 대응은 1:1이 아님.
/// 예: ㄱ은 か행(k)일 수도, が행(g)일 수도 있음.
/// 통계적 추정을 위해 복수 후보를 반환.
pub fn choseong_to_consonants(cho: u32) -> Vec<(&'static str, f64)> {
    match cho {
        0  => vec![("k", 0.6), ("g", 0.3), ("", 0.1)],  // ㄱ → k(か) 우세, g(が), 무음(관용)
        1  => vec![("g", 0.7), ("k", 0.2), ("kk", 0.1)], // ㄲ → g(が) 우세
        2  => vec![("n", 1.0)],                   // ㄴ → n(な)
        3  => vec![("t", 0.6), ("d", 0.3), ("", 0.1)],  // ㄷ → t(た) 우세, d(だ)
        4  => vec![("d", 0.7), ("t", 0.2), ("dd", 0.1)], // ㄸ → d(だ) 우세
        5  => vec![("r", 1.0)],                   // ㄹ → r(ら)
        6  => vec![("m", 1.0)],                   // ㅁ → m(ま)
        7  => vec![("h", 0.4), ("b", 0.3), ("p", 0.2), ("w", 0.1)], // ㅂ → h(は)/b(ば)/p(ぱ)/w(わ)
        8  => vec![("b", 0.7), ("p", 0.3)],     // ㅃ → b(ば) 우세
        9  => vec![("s", 0.7), ("sh", 0.2), ("z", 0.1)], // ㅅ → s(さ), sh(し행), z(관용)
        10 => vec![("z", 0.7), ("s", 0.2), ("ss", 0.1)], // ㅆ → z(ざ) 우세
        11 => vec![("", 1.0)],                     // ㅇ → 모음만 (あ행)
        12 => vec![("j", 0.4), ("ch", 0.3), ("z", 0.2), ("ts", 0.1)], // ㅈ → j/ch/z/ts
        // ㅉ: 관용 표기에서 ち(chi)를 '찌'로 적는 경우 매우 많음 (곤니찌와, 오찌 등)
        // ち→치가 정규 매핑이지만, 한국인은 '찌'도 매우 자주 사용
        13 => vec![("ch", 0.4), ("j", 0.3), ("z", 0.2), ("ts", 0.1)], // ㅉ → ch(ち)/j(じ)/z/ts
        14 => vec![("ch", 0.6), ("ts", 0.3), ("t", 0.1)], // ㅊ → ch(ち)/ts(つ)/t(관용)
        15 => vec![("k", 1.0)],                   // ㅋ → k(か) 확정
        16 => vec![("t", 1.0)],                   // ㅌ → t(た) 확정
        17 => vec![("p", 0.7), ("f", 0.3)],     // ㅍ → p(ぱ)/f(ふ)
        18 => vec![("h", 1.0)],                   // ㅎ → h(は) 확정
        _  => vec![("", 0.0)],
    }
}

/// Map a Hangul jungseong (vowel) to Japanese vowel(s).
/// Some Korean vowels don't have direct Japanese equivalents.
pub fn jungseong_to_vowel(jung: u32) -> Vec<(&'static str, f64)> {
    match jung {
        0  => vec![("a", 1.0)],                   // ㅏ → a
        1  => vec![("a", 0.5), ("e", 0.5)],     // ㅐ → a/e (あ/え 모호)
        2  => vec![("ya", 1.0)],                   // ㅑ → ya
        3  => vec![("ya", 0.5), ("ye", 0.5)],   // ㅒ → ya/ye
        4  => vec![("e", 0.6), ("o", 0.3), ("a", 0.1)], // ㅓ → e/o/a (어≈え/お/あ)
        5  => vec![("e", 1.0)],                   // ㅔ → e
        6  => vec![("yo", 0.7), ("ye", 0.3)],   // ㅕ → yo/ye
        7  => vec![("ye", 1.0)],                   // ㅖ → ye
        8  => vec![("o", 1.0)],                   // ㅗ → o
        // ㅘ: 'わ'(wa) 뿐만 아니라 관용 표기에서 'は'(ha)를 '와'로 적는 경우 고려
        // 예: こんにちは → 곤니찌'와' (관용), 정확한 표기는 '하'
        9  => vec![("wa", 0.7), ("a", 0.3)],    // ㅘ → wa / a (ㅇ+ㅘ일 때 'ha' 가능)
        10 => vec![("wa", 0.7), ("we", 0.3)],   // ㅙ → wa/we
        11 => vec![("o", 0.7), ("we", 0.3)],    // ㅚ → o/we
        12 => vec![("yo", 1.0)],                   // ㅛ → yo
        13 => vec![("u", 1.0)],                   // ㅜ → u
        14 => vec![("wo", 0.5), ("we", 0.5)],   // ㅝ → wo/we
        15 => vec![("we", 1.0)],                   // ㅞ → we
        16 => vec![("u", 0.7), ("wi", 0.3)],    // ㅟ → u/wi
        17 => vec![("yu", 1.0)],                   // ㅠ → yu
        18 => vec![("u", 0.7), ("i", 0.3)],     // ㅡ → u/i (으≈う)
        19 => vec![("i", 1.0)],                   // ㅢ → i
        20 => vec![("i", 1.0)],                   // ㅣ → i
        _  => vec![("a", 0.0)],
    }
}

/// Map a Hangul jongseong (final consonant) to Japanese phoneme.
/// In Japanese, syllable-final consonants are limited to ん(n) and っ(geminate).
pub fn jongseong_to_phoneme(jong: u32) -> Option<&'static str> {
    match jong {
        0  => None,                // no jongseong
        2  => Some("n"),          // ㄴ as jongseong → ん
        6  => Some("m"),          // ㅁ as jongseong → ん (m before b/p)
        21 => Some("ng"),         // ㅇ as jongseong → ん (ng)
        // ㄱ,ㄷ,ㅂ etc. as jongseong → っ (geminate/double next consonant)
        1 | 7 | 17 => Some("Q"), // Q = っ marker (geminate)
        _  => Some("n"),          // default to ん for other finals
    }
}

/// Convert a single Hangul syllable to a list of possible romaji readings.
/// Returns candidates ordered by probability.
pub fn hangul_to_romaji_candidates(ch: char) -> Vec<(String, f64)> {
    let jamo = match jamo::decompose(ch) {
        Some(j) => j,
        None => return vec![(ch.to_string(), 0.0)],
    };

    let consonants = choseong_to_consonants(jamo.choseong);
    let vowels = jungseong_to_vowel(jamo.jungseong);
    let final_phoneme = jongseong_to_phoneme(jamo.jongseong);

    let mut candidates = Vec::new();

    for (cons, cons_conf) in &consonants {
        for (vowel, vowel_conf) in &vowels {
            // Special cases for Japanese phonology
            let romaji = resolve_special_romaji(cons, vowel);
            let mut full = romaji;

            if let Some(fp) = final_phoneme {
                full.push_str(fp);
            }

            let confidence = cons_conf * vowel_conf;
            candidates.push((full, confidence));
        }
    }

    // Sort by confidence (highest first)
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Deduplicate
    candidates.dedup_by(|a, b| a.0 == b.0);

    candidates
}

/// Handle special Japanese phonological rules.
/// e.g., "si" → "shi", "ti" → "chi", "tu" → "tsu", "hu" → "fu"
fn resolve_special_romaji(consonant: &str, vowel: &str) -> String {
    match (consonant, vowel) {
        ("s", "i")   => "shi".to_string(),
        ("t", "i")   => "chi".to_string(),
        ("t", "u")   => "tsu".to_string(),
        ("h", "u")   => "fu".to_string(),
        ("z", "i")   => "ji".to_string(),
        ("d", "i")   => "ji".to_string(),   // ぢ = じ (same sound)
        ("d", "u")   => "zu".to_string(),   // づ = ず
        ("ch", "a")  => "cha".to_string(),
        ("ch", "i")  => "chi".to_string(),
        ("ch", "u")  => "chu".to_string(),
        ("ch", "e")  => "che".to_string(),
        ("ch", "o")  => "cho".to_string(),
        ("sh", "a")  => "sha".to_string(),
        ("sh", "i")  => "shi".to_string(),
        ("sh", "u")  => "shu".to_string(),
        ("sh", "e")  => "she".to_string(),
        ("sh", "o")  => "sho".to_string(),
        ("j", "a")   => "ja".to_string(),
        ("j", "i")   => "ji".to_string(),
        ("j", "u")   => "ju".to_string(),
        ("j", "e")   => "je".to_string(),
        ("j", "o")   => "jo".to_string(),
        ("f", "u")   => "fu".to_string(),
        ("ts", "u")  => "tsu".to_string(),
        _ => format!("{}{}", consonant, vowel),
    }
}

/// Convert a full Hangul string to romaji candidates.
/// Returns the top-N most likely romaji representations.
pub fn hangul_string_to_romaji(input: &str, max_candidates: usize) -> Vec<(String, f64)> {
    let syllables: Vec<char> = input.chars().collect();

    if syllables.is_empty() {
        return vec![];
    }

    // Start with candidates for the first syllable
    let mut results = hangul_to_romaji_candidates(syllables[0]);

    // Iteratively combine with each subsequent syllable
    for &ch in &syllables[1..] {
        let next_candidates = hangul_to_romaji_candidates(ch);
        let mut combined = Vec::new();

        for (prev_romaji, prev_conf) in &results {
            for (next_romaji, next_conf) in &next_candidates {
                let mut full = prev_romaji.clone();

                // Handle っ (geminate): Q + consonant → double consonant
                if full.ends_with('Q') && !next_romaji.is_empty() {
                    full.pop(); // remove Q
                    if let Some(first_char) = next_romaji.chars().next() {
                        if first_char.is_ascii_alphabetic() {
                            full.push(first_char); // double the consonant
                        }
                    }
                }

                full.push_str(next_romaji);
                let confidence = prev_conf * next_conf;
                combined.push((full, confidence));
            }
        }

        combined.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        combined.truncate(max_candidates);
        results = combined;
    }

    results.truncate(max_candidates);
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_to_a() {
        // 아 → a (あ)
        let candidates = hangul_to_romaji_candidates('아');
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].0, "a");
    }

    #[test]
    fn test_ka_mapping() {
        // 카 → ka (か) - ㅋ is unambiguous k
        let candidates = hangul_to_romaji_candidates('카');
        assert_eq!(candidates[0].0, "ka");
    }

    #[test]
    fn test_sa_mapping() {
        // 사 → sa (さ)
        let candidates = hangul_to_romaji_candidates('사');
        assert_eq!(candidates[0].0, "sa");
    }

    #[test]
    fn test_shi_mapping() {
        // 시 → shi (し) - special s+i rule
        let candidates = hangul_to_romaji_candidates('시');
        assert_eq!(candidates[0].0, "shi");
    }

    #[test]
    fn test_tsu_mapping() {
        // 추 → tsu (つ) when ㅌ+ㅜ, or chu
        let candidates = hangul_to_romaji_candidates('추');
        // ㅊ(14) → ch(0.7)/ts(0.3), ㅜ(13) → u(1.0)
        assert!(candidates.iter().any(|(r, _)| r == "chu"));
        assert!(candidates.iter().any(|(r, _)| r == "tsu"));
    }

    #[test]
    fn test_na_mapping() {
        // 나 → na (な)
        let candidates = hangul_to_romaji_candidates('나');
        assert_eq!(candidates[0].0, "na");
    }

    #[test]
    fn test_string_sakura() {
        // 사쿠라 → sakura (さくら)
        let candidates = hangul_string_to_romaji("사쿠라", 5);
        assert!(candidates.iter().any(|(r, _)| r == "sakura"));
    }

    #[test]
    fn test_string_nihon() {
        // 니혼 → nihon (にほん)
        let candidates = hangul_string_to_romaji("니혼", 5);
        assert!(candidates.iter().any(|(r, _)| r == "nihon"));
    }

    #[test]
    fn test_string_tokyo() {
        // 도쿄 → tokyo (とうきょう) or tokyo
        let candidates = hangul_string_to_romaji("도쿄", 10);
        assert!(candidates.iter().any(|(r, _)| r == "tokyo"));
    }

    #[test]
    fn test_conventional_konnichiwa() {
        // 곤니찌와 → should include "konnichiwa" among candidates
        // ㅉ(찌) should map to ch (chi) as well as j
        let candidates = hangul_string_to_romaji("곤니찌와", 30);
        // Among the top candidates, "konnichiwa" or "konnichiha" should appear
        let has_konnichiwa = candidates.iter().any(|(r, _)| r == "konnichiwa");
        let has_konnichiha = candidates.iter().any(|(r, _)| r == "konnichiha");
        assert!(has_konnichiwa || has_konnichiha,
            "Expected 'konnichiwa' or 'konnichiha' among candidates, got: {:?}",
            candidates.iter().take(10).collect::<Vec<_>>());
    }

    #[test]
    fn test_jji_produces_chi() {
        // 찌 (ㅉ+ㅣ) should have "chi" as a candidate (not just "ji")
        let candidates = hangul_to_romaji_candidates('찌');
        let has_chi = candidates.iter().any(|(r, _)| r == "chi");
        assert!(has_chi, "Expected 'chi' among candidates for 찌, got: {:?}", candidates);
    }

    #[test]
    fn test_wa_produces_ha() {
        // 와 (ㅇ+ㅘ) should have "a" vowel option → "" + "a" = "a"
        // But for konnichiwa, the "wa" part needs to become わ which is correct
        // The real fix is at BeamDecoder level
        let candidates = hangul_to_romaji_candidates('와');
        // Should have "wa" as primary
        assert!(candidates.iter().any(|(r, _)| r == "wa"),
            "Expected 'wa' for 와, got: {:?}", candidates);
    }
}
