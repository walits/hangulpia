//! Romaji to Hiragana conversion.
//!
//! Converts romaji strings to hiragana using a comprehensive lookup table.
//! Supports all standard hiragana including voiced (dakuten), semi-voiced (handakuten),
//! youon (contracted sounds), and sokuon (geminate consonants).

use std::collections::HashMap;

/// Build the complete romaji-to-hiragana conversion table.
pub fn build_romaji_table() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();

    // ── あ行 (vowels) ──
    m.insert("a", "あ");
    m.insert("i", "い");
    m.insert("u", "う");
    m.insert("e", "え");
    m.insert("o", "お");

    // ── か行 (k) ──
    m.insert("ka", "か");
    m.insert("ki", "き");
    m.insert("ku", "く");
    m.insert("ke", "け");
    m.insert("ko", "こ");

    // ── さ行 (s) ──
    m.insert("sa", "さ");
    m.insert("shi", "し");
    m.insert("si", "し");
    m.insert("su", "す");
    m.insert("se", "せ");
    m.insert("so", "そ");

    // ── た行 (t) ──
    m.insert("ta", "た");
    m.insert("chi", "ち");
    m.insert("ti", "ち");
    m.insert("tsu", "つ");
    m.insert("tu", "つ");
    m.insert("te", "て");
    m.insert("to", "と");

    // ── な行 (n) ──
    m.insert("na", "な");
    m.insert("ni", "に");
    m.insert("nu", "ぬ");
    m.insert("ne", "ね");
    m.insert("no", "の");

    // ── は行 (h) ──
    m.insert("ha", "は");
    m.insert("hi", "ひ");
    m.insert("fu", "ふ");
    m.insert("hu", "ふ");
    m.insert("he", "へ");
    m.insert("ho", "ほ");

    // ── ま行 (m) ──
    m.insert("ma", "ま");
    m.insert("mi", "み");
    m.insert("mu", "む");
    m.insert("me", "め");
    m.insert("mo", "も");

    // ── や行 (y) ──
    m.insert("ya", "や");
    m.insert("yu", "ゆ");
    m.insert("yo", "よ");

    // ── ら行 (r) ──
    m.insert("ra", "ら");
    m.insert("ri", "り");
    m.insert("ru", "る");
    m.insert("re", "れ");
    m.insert("ro", "ろ");

    // ── わ行 (w) ──
    m.insert("wa", "わ");
    m.insert("wi", "ゐ");
    m.insert("we", "ゑ");
    m.insert("wo", "を");

    // ── ん ──
    m.insert("n", "ん");
    m.insert("nn", "ん");
    m.insert("n'", "ん");

    // ── が行 (g) - 濁音 ──
    m.insert("ga", "が");
    m.insert("gi", "ぎ");
    m.insert("gu", "ぐ");
    m.insert("ge", "げ");
    m.insert("go", "ご");

    // ── ざ行 (z) - 濁音 ──
    m.insert("za", "ざ");
    m.insert("ji", "じ");
    m.insert("zi", "じ");
    m.insert("zu", "ず");
    m.insert("ze", "ぜ");
    m.insert("zo", "ぞ");

    // ── だ行 (d) - 濁音 ──
    m.insert("da", "だ");
    m.insert("di", "ぢ");
    m.insert("du", "づ");
    m.insert("de", "で");
    m.insert("do", "ど");

    // ── ば行 (b) - 濁音 ──
    m.insert("ba", "ば");
    m.insert("bi", "び");
    m.insert("bu", "ぶ");
    m.insert("be", "べ");
    m.insert("bo", "ぼ");

    // ── ぱ行 (p) - 半濁音 ──
    m.insert("pa", "ぱ");
    m.insert("pi", "ぴ");
    m.insert("pu", "ぷ");
    m.insert("pe", "ぺ");
    m.insert("po", "ぽ");

    // ── 拗音 (contracted sounds) ──
    // きゃ行
    m.insert("kya", "きゃ");
    m.insert("kyu", "きゅ");
    m.insert("kyo", "きょ");
    // しゃ行
    m.insert("sha", "しゃ");
    m.insert("shu", "しゅ");
    m.insert("sho", "しょ");
    m.insert("sya", "しゃ");
    m.insert("syu", "しゅ");
    m.insert("syo", "しょ");
    // ちゃ行
    m.insert("cha", "ちゃ");
    m.insert("chu", "ちゅ");
    m.insert("cho", "ちょ");
    m.insert("tya", "ちゃ");
    m.insert("tyu", "ちゅ");
    m.insert("tyo", "ちょ");
    // にゃ行
    m.insert("nya", "にゃ");
    m.insert("nyu", "にゅ");
    m.insert("nyo", "にょ");
    // ひゃ行
    m.insert("hya", "ひゃ");
    m.insert("hyu", "ひゅ");
    m.insert("hyo", "ひょ");
    // みゃ行
    m.insert("mya", "みゃ");
    m.insert("myu", "みゅ");
    m.insert("myo", "みょ");
    // りゃ行
    m.insert("rya", "りゃ");
    m.insert("ryu", "りゅ");
    m.insert("ryo", "りょ");
    // ぎゃ行
    m.insert("gya", "ぎゃ");
    m.insert("gyu", "ぎゅ");
    m.insert("gyo", "ぎょ");
    // じゃ行
    m.insert("ja", "じゃ");
    m.insert("ju", "じゅ");
    m.insert("jo", "じょ");
    m.insert("jya", "じゃ");
    m.insert("jyu", "じゅ");
    m.insert("jyo", "じょ");
    // びゃ行
    m.insert("bya", "びゃ");
    m.insert("byu", "びゅ");
    m.insert("byo", "びょ");
    // ぴゃ行
    m.insert("pya", "ぴゃ");
    m.insert("pyu", "ぴゅ");
    m.insert("pyo", "ぴょ");

    m
}

/// Convert a romaji string to hiragana.
///
/// Uses greedy longest-match from left to right.
/// Handles sokuon (っ) via double consonants (e.g., "kk" → "っk").
pub fn romaji_to_hiragana(romaji: &str) -> String {
    let table = build_romaji_table();
    let chars: Vec<char> = romaji.chars().collect();
    let len = chars.len();
    let mut result = String::new();
    let mut i = 0;

    while i < len {
        // Handle sokuon: double consonant → っ + single consonant
        if i + 1 < len
            && chars[i] == chars[i + 1]
            && chars[i].is_ascii_alphabetic()
            && chars[i] != 'a'
            && chars[i] != 'i'
            && chars[i] != 'u'
            && chars[i] != 'e'
            && chars[i] != 'o'
            && chars[i] != 'n'
        {
            result.push('っ');
            i += 1; // skip one, keep the second for next iteration
            continue;
        }

        // Handle 'nn' → ん (explicit double-n for ん)
        // BUT if followed by a vowel or 'y', treat as ん + n+vowel
        // e.g., "konnichiwa" → こ + ん + に + ち + わ (not こ + ん + い + ち + わ)
        if chars[i] == 'n' && i + 1 < len && chars[i + 1] == 'n' {
            // Check what follows the double-n
            if i + 2 < len {
                let after_nn = chars[i + 2];
                if after_nn == 'a' || after_nn == 'i' || after_nn == 'u'
                    || after_nn == 'e' || after_nn == 'o' || after_nn == 'y' {
                    // nn + vowel: treat first n as ん, second n starts next syllable
                    // e.g., "nni" → ん + に
                    result.push_str("ん");
                    i += 1; // consume only first n; second n + vowel parsed next
                    continue;
                }
            }
            // nn at end or before consonant: just ん
            result.push_str("ん");
            i += 2;
            continue;
        }

        // Handle 'n' before consonant or end → ん
        if chars[i] == 'n' && i + 1 < len {
            let next = chars[i + 1];
            if next != 'a'
                && next != 'i'
                && next != 'u'
                && next != 'e'
                && next != 'o'
                && next != 'y'
                && next != 'n'
            {
                result.push_str("ん");
                i += 1;
                continue;
            }
        }

        // Try longest match first (up to 4 chars)
        let mut matched = false;
        for match_len in (1..=4.min(len - i)).rev() {
            let slice: String = chars[i..i + match_len].iter().collect();
            if let Some(hiragana) = table.get(slice.as_str()) {
                result.push_str(hiragana);
                i += match_len;
                matched = true;
                break;
            }
        }

        if !matched {
            // Pass through unmatched characters
            result.push(chars[i]);
            i += 1;
        }
    }

    // Handle trailing 'n' → ん
    if result.ends_with('n') {
        let mut chars: Vec<char> = result.chars().collect();
        chars.pop();
        result = chars.into_iter().collect();
        result.push_str("ん");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_vowels() {
        assert_eq!(romaji_to_hiragana("a"), "あ");
        assert_eq!(romaji_to_hiragana("i"), "い");
        assert_eq!(romaji_to_hiragana("u"), "う");
        assert_eq!(romaji_to_hiragana("e"), "え");
        assert_eq!(romaji_to_hiragana("o"), "お");
    }

    #[test]
    fn ka_row() {
        assert_eq!(romaji_to_hiragana("ka"), "か");
        assert_eq!(romaji_to_hiragana("ki"), "き");
        assert_eq!(romaji_to_hiragana("ku"), "く");
        assert_eq!(romaji_to_hiragana("ke"), "け");
        assert_eq!(romaji_to_hiragana("ko"), "こ");
    }

    #[test]
    fn special_sounds() {
        assert_eq!(romaji_to_hiragana("shi"), "し");
        assert_eq!(romaji_to_hiragana("chi"), "ち");
        assert_eq!(romaji_to_hiragana("tsu"), "つ");
        assert_eq!(romaji_to_hiragana("fu"), "ふ");
    }

    #[test]
    fn dakuten() {
        assert_eq!(romaji_to_hiragana("ga"), "が");
        assert_eq!(romaji_to_hiragana("za"), "ざ");
        assert_eq!(romaji_to_hiragana("da"), "だ");
        assert_eq!(romaji_to_hiragana("ba"), "ば");
    }

    #[test]
    fn handakuten() {
        assert_eq!(romaji_to_hiragana("pa"), "ぱ");
        assert_eq!(romaji_to_hiragana("pi"), "ぴ");
    }

    #[test]
    fn youon() {
        assert_eq!(romaji_to_hiragana("sha"), "しゃ");
        assert_eq!(romaji_to_hiragana("cha"), "ちゃ");
        assert_eq!(romaji_to_hiragana("kyo"), "きょ");
    }

    #[test]
    fn sokuon() {
        assert_eq!(romaji_to_hiragana("kka"), "っか");
        assert_eq!(romaji_to_hiragana("tta"), "った");
    }

    #[test]
    fn words() {
        assert_eq!(romaji_to_hiragana("sakura"), "さくら");
        assert_eq!(romaji_to_hiragana("nihon"), "にほん");
        assert_eq!(romaji_to_hiragana("tokyo"), "ときょ");
    }

    #[test]
    fn n_before_consonant() {
        assert_eq!(romaji_to_hiragana("kanda"), "かんだ");
        assert_eq!(romaji_to_hiragana("shinbun"), "しんぶん");
    }

    #[test]
    fn nn_before_vowel() {
        // "konnichiwa" → こんにちわ (nn + i → ん + に, not ん + い)
        assert_eq!(romaji_to_hiragana("konnichiwa"), "こんにちわ");
        // "onna" → おんな
        assert_eq!(romaji_to_hiragana("onna"), "おんな");
        // "sennin" → せんにん
        assert_eq!(romaji_to_hiragana("sennin"), "せんにん");
        // "annai" → あんない
        assert_eq!(romaji_to_hiragana("annai"), "あんない");
    }
}
