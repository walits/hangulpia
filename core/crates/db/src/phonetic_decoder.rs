//! Statistical phonetic decoder: Hangul → Hiragana using learned mappings.
//!
//! Instead of rule-based Hangul → Romaji → Hiragana conversion,
//! this module learns the mapping from (hiragana, hangul) pairs in training data.
//!
//! Key insight: hiragana_to_hangul is deterministic (つ→츠, ち→치),
//! so we can build a REVERSE table (츠→つ:0.95, 츠→ちゅ:0.05) from frequency.
//!
//! ## Architecture
//!
//! 1. **PhoneticMap**: hangul_token → Vec<(hiragana_token, frequency)>
//!    - Built from aligned (hiragana, hangul) character pairs in corpus
//!    - Supports 1-to-1, 1-to-2, 2-to-1 character alignments
//!
//! 2. **BeamDecoder**: Hangul string → top-N hiragana candidates
//!    - Uses PhoneticMap probabilities for beam search
//!    - Replaces hangul_string_to_romaji + romaji_to_hiragana pipeline

use std::collections::HashMap;
use crate::kana_hangul::hiragana_to_hangul;

/// A single mapping entry: hangul token → hiragana token with frequency
#[derive(Debug, Clone)]
pub struct PhoneticMapping {
    pub hiragana: String,
    pub frequency: u64,
    pub probability: f64,
}

/// Reverse phonetic map: hangul tokens → possible hiragana decodings
#[derive(Debug, Clone)]
pub struct PhoneticMap {
    /// hangul_token → sorted Vec<PhoneticMapping> (highest prob first)
    pub map: HashMap<String, Vec<PhoneticMapping>>,
    /// Total training pairs seen
    pub total_pairs: u64,
}

impl PhoneticMap {
    pub fn new() -> Self {
        PhoneticMap {
            map: HashMap::new(),
            total_pairs: 0,
        }
    }

    /// Build the reverse map from a set of (hiragana_reading, hangul) pairs.
    /// Each pair is character-aligned using hiragana_to_hangul to find
    /// which hiragana characters produce which hangul characters.
    pub fn build_from_pairs(&mut self, pairs: &[(String, String, u64)]) {
        // pairs = (hiragana_reading, hangul, frequency)
        // We align character-by-character using the known forward mapping.
        let mut raw_counts: HashMap<String, HashMap<String, u64>> = HashMap::new();

        for (hiragana, hangul, freq) in pairs {
            let alignments = align_hiragana_hangul(hiragana, hangul);
            for (hira_token, hangul_token) in &alignments {
                *raw_counts
                    .entry(hangul_token.clone())
                    .or_default()
                    .entry(hira_token.clone())
                    .or_insert(0) += freq;
                self.total_pairs += freq;
            }
        }

        // Convert to probability-sorted mappings
        for (hangul_token, hira_map) in raw_counts {
            let total: u64 = hira_map.values().sum();
            let mut mappings: Vec<PhoneticMapping> = hira_map
                .into_iter()
                .map(|(hiragana, frequency)| PhoneticMapping {
                    hiragana,
                    frequency,
                    probability: frequency as f64 / total as f64,
                })
                .collect();
            mappings.sort_by(|a, b| b.probability.partial_cmp(&a.probability).unwrap());
            self.map.insert(hangul_token, mappings);
        }
    }

    /// Build from generated corpus data (GenSentence words).
    /// Extracts (reading, hangul) pairs and builds the map.
    pub fn build_from_corpus_words(&mut self, words: &[(String, String, u64)]) {
        self.build_from_pairs(words);
    }

    /// Get possible hiragana decodings for a hangul token.
    pub fn get_candidates(&self, hangul_token: &str) -> Option<&[PhoneticMapping]> {
        self.map.get(hangul_token).map(|v| v.as_slice())
    }

    /// Number of unique hangul tokens in the map.
    pub fn vocab_size(&self) -> usize {
        self.map.len()
    }

    /// Stats string for debugging.
    pub fn stats(&self) -> String {
        let total_hangul = self.map.len();
        let total_hira: usize = self.map.values().map(|v| v.len()).sum();
        let max_ambiguity = self.map.values().map(|v| v.len()).max().unwrap_or(0);
        let avg_ambiguity = if total_hangul > 0 { total_hira as f64 / total_hangul as f64 } else { 0.0 };
        format!(
            "PhoneticMap: {} hangul tokens, {} total mappings, avg ambiguity {:.1}, max ambiguity {}",
            total_hangul, total_hira, avg_ambiguity, max_ambiguity
        )
    }
}

/// Align hiragana and hangul strings character by character.
///
/// Uses the deterministic hiragana_to_hangul mapping to figure out
/// which hiragana characters produced which hangul output.
///
/// Returns: Vec<(hiragana_token, hangul_token)> aligned pairs.
fn align_hiragana_hangul(hiragana: &str, hangul: &str) -> Vec<(String, String)> {
    let hira_chars: Vec<char> = hiragana.chars().collect();
    let hangul_chars: Vec<char> = hangul.chars().collect();
    let mut alignments = Vec::new();

    let mut hi = 0; // hiragana index
    let mut ki = 0; // hangul index

    while hi < hira_chars.len() && ki < hangul_chars.len() {
        let ch = hira_chars[hi];

        // Skip ん — it becomes batchim (modifies previous hangul char)
        if ch == 'ん' {
            // ん typically merges with the previous hangul syllable as batchim
            // We map it as a separate token "ん" → (batchim effect, captured in hangul)
            // Skip — the hangul index doesn't advance for ん because it's in the batchim
            hi += 1;
            continue;
        }

        // Skip っ (sokuon) — it becomes a tensed consonant on previous hangul
        if ch == 'っ' {
            hi += 1;
            continue;
        }

        // Try youon (2-char hiragana → 1 hangul syllable)
        if hi + 1 < hira_chars.len() {
            let two = format!("{}{}", ch, hira_chars[hi + 1]);
            let two_hangul = hiragana_to_hangul(&two);
            let two_hangul_chars: Vec<char> = two_hangul.chars().collect();

            if two_hangul_chars.len() == 1 && ki < hangul_chars.len() {
                if two_hangul_chars[0] == hangul_chars[ki] {
                    alignments.push((two.clone(), hangul_chars[ki].to_string()));
                    hi += 2;
                    ki += 1;
                    continue;
                }
            }
        }

        // Single hiragana → single hangul
        let single_hangul = hiragana_to_hangul(&ch.to_string());
        let single_chars: Vec<char> = single_hangul.chars().collect();

        if !single_chars.is_empty() && ki < hangul_chars.len() {
            // Check if this hiragana maps to the current hangul character
            if single_chars[0] == hangul_chars[ki] {
                alignments.push((ch.to_string(), hangul_chars[ki].to_string()));
                hi += 1;
                ki += 1;
                continue;
            }
        }

        // Fallback: consume both and move on
        alignments.push((ch.to_string(), hangul_chars[ki].to_string()));
        hi += 1;
        ki += 1;
    }

    // Handle remaining hiragana (e.g., final ん or っ)
    // These don't produce new hangul characters

    alignments
}

/// Beam search decoder: converts a Hangul string to top-N hiragana candidates
/// using the learned PhoneticMap.
pub struct BeamDecoder<'a> {
    map: &'a PhoneticMap,
    beam_width: usize,
    max_candidates: usize,
}

/// A single beam state during decoding.
#[derive(Debug, Clone)]
struct BeamState {
    hiragana: String,
    log_prob: f64,
    hangul_pos: usize,
}

impl<'a> BeamDecoder<'a> {
    pub fn new(map: &'a PhoneticMap, beam_width: usize, max_candidates: usize) -> Self {
        BeamDecoder {
            map,
            beam_width,
            max_candidates,
        }
    }

    /// Decode a hangul string into top-N hiragana candidates.
    /// Returns: Vec<(hiragana, confidence)> sorted by confidence (highest first).
    pub fn decode(&self, hangul: &str) -> Vec<(String, f64)> {
        let chars: Vec<char> = hangul.chars().collect();
        if chars.is_empty() {
            return vec![];
        }

        let mut beam: Vec<BeamState> = vec![BeamState {
            hiragana: String::new(),
            log_prob: 0.0,
            hangul_pos: 0,
        }];

        while !beam.is_empty() {
            let mut next_beam: Vec<BeamState> = Vec::new();
            let mut all_done = true;

            for state in &beam {
                if state.hangul_pos >= chars.len() {
                    next_beam.push(state.clone());
                    continue;
                }
                all_done = false;

                let pos = state.hangul_pos;

                // Try 3-char hangul token first (for multi-token maps)
                if pos + 2 < chars.len() {
                    let three_char: String = chars[pos..=pos + 2].iter().collect();
                    if let Some(candidates) = self.map.get_candidates(&three_char) {
                        for mapping in candidates.iter().take(self.beam_width) {
                            if mapping.probability < 0.01 { continue; }
                            let mut new_state = state.clone();
                            new_state.hiragana.push_str(&mapping.hiragana);
                            new_state.log_prob += mapping.probability.ln();
                            new_state.hangul_pos = pos + 3;
                            next_beam.push(new_state);
                        }
                    }
                }

                // Try 2-char hangul token (e.g., for special combinations)
                if pos + 1 < chars.len() {
                    let two_char: String = chars[pos..=pos + 1].iter().collect();
                    if let Some(candidates) = self.map.get_candidates(&two_char) {
                        for mapping in candidates.iter().take(self.beam_width) {
                            if mapping.probability < 0.01 { continue; }
                            let mut new_state = state.clone();
                            new_state.hiragana.push_str(&mapping.hiragana);
                            new_state.log_prob += mapping.probability.ln();
                            new_state.hangul_pos = pos + 2;
                            next_beam.push(new_state);
                        }
                    }
                }

                // Try 1-char hangul token
                let one_char = chars[pos].to_string();
                if let Some(candidates) = self.map.get_candidates(&one_char) {
                    for mapping in candidates.iter().take(self.beam_width) {
                        if mapping.probability < 0.01 { continue; }
                        let mut new_state = state.clone();
                        new_state.hiragana.push_str(&mapping.hiragana);
                        new_state.log_prob += mapping.probability.ln();
                        new_state.hangul_pos = pos + 1;
                        next_beam.push(new_state);
                    }
                } else {
                    // Unknown hangul token: try rule-based fallback before raw passthrough
                    let fallback_candidates = hangul_char_to_hiragana_fallback(chars[pos]);
                    if !fallback_candidates.is_empty() {
                        for (hira, conf) in &fallback_candidates {
                            let mut new_state = state.clone();
                            new_state.hiragana.push_str(hira);
                            // Moderate penalty (worse than learned map, better than raw passthrough)
                            new_state.log_prob += (conf * 0.3).ln();
                            new_state.hangul_pos = pos + 1;
                            next_beam.push(new_state);
                        }
                    } else {
                        // Truly unknown: pass through as-is
                        let mut new_state = state.clone();
                        new_state.hiragana.push(chars[pos]);
                        new_state.log_prob += -20.0;
                        new_state.hangul_pos = pos + 1;
                        next_beam.push(new_state);
                    }
                }
            }

            if all_done {
                beam = next_beam;
                break;
            }

            // Filter out NaN/infinite states and prune beam
            next_beam.retain(|s| s.log_prob.is_finite());
            next_beam.sort_by(|a, b| b.log_prob.partial_cmp(&a.log_prob).unwrap_or(std::cmp::Ordering::Equal));
            next_beam.truncate(self.beam_width * 3);
            beam = next_beam;
        }

        // Final results
        beam.retain(|s| s.log_prob.is_finite());
        beam.sort_by(|a, b| b.log_prob.partial_cmp(&a.log_prob).unwrap_or(std::cmp::Ordering::Equal));
        beam.dedup_by(|a, b| a.hiragana == b.hiragana);
        beam.truncate(self.max_candidates);

        beam.into_iter()
            .map(|s| {
                let confidence = s.log_prob.exp(); // convert back from log space
                (s.hiragana, confidence)
            })
            .collect()
    }

    /// Decode a full sentence (word-splitting on whitespace) and return the
    /// single best hiragana reading.
    ///
    /// A handful of Japanese grammatical patterns are pronounced one way but
    /// spelled another (e.g. the topic particle は is pronounced "wa"), and
    /// common copula/verb endings (です, ます, ...) fall on the wrong side of
    /// voicing ambiguity the character-alignment PhoneticMap can't resolve
    /// from vocabulary alone, no matter how large. Real usage patterns like
    /// this need actual sentence-level training data to learn statistically;
    /// short of that, these are handled as small, explicit exceptions,
    /// applied per word before falling back to normal `decode()`.
    pub fn decode_sentence(&self, hangul: &str) -> String {
        let mut out = String::new();
        let mut word_start = 0;
        let chars: Vec<char> = hangul.chars().collect();

        let mut i = 0;
        while i <= chars.len() {
            let at_boundary = i == chars.len() || chars[i].is_whitespace();
            if at_boundary {
                if i > word_start {
                    let word: String = chars[word_start..i].iter().collect();
                    out.push_str(&self.decode_word(&word));
                }
                if i < chars.len() {
                    out.push(chars[i]);
                }
                word_start = i + 1;
            }
            i += 1;
        }
        out
    }

    /// Decode a single word: exact known phrases first (these can rely on
    /// context — e.g. 곤니찌와 is the greeting word itself, not "곤니찌" +
    /// topic particle, so it must win over the suffix rule below), then
    /// grammatical suffix exceptions, then the learned PhoneticMap/rule-based
    /// fallback via `decode()`.
    fn decode_word(&self, word: &str) -> String {
        if word.is_empty() {
            return String::new();
        }
        if let Some(reading) = KNOWN_WORDS.iter().find(|(w, _)| *w == word) {
            return reading.1.to_string();
        }
        let word_len = word.chars().count();
        for (suffix, reading) in KNOWN_SUFFIXES {
            let suffix_len = suffix.chars().count();
            if word_len >= suffix_len && word.ends_with(suffix) {
                let prefix: String = word.chars().take(word_len - suffix_len).collect();
                let prefix_hira = self.decode_word(&prefix);
                return format!("{}{}", prefix_hira, reading);
            }
        }
        self.decode(word)
            .into_iter()
            .next()
            .map(|(h, _)| h)
            .unwrap_or_default()
    }
}

/// Exact known phrases/words with a verified reading, checked before the
/// suffix rule so words that happen to end in a "particle-shaped" syllable
/// (e.g. 곤니찌와, where 와 is part of the greeting itself, not a topic
/// particle) don't get mis-rewritten by it. Kept in sync with the browser
/// demo's KNOWN_WORDS in homepage/index.html.
const KNOWN_WORDS: &[(&str, &str)] = &[
    ("사쿠라", "さくら"),
    ("아리가토", "ありがとう"),
    ("아리가토고자이마스", "ありがとうございます"),
    ("스미마셍", "すみません"),
    ("곤니찌와", "こんにちわ"),
    ("오하요", "おはよう"),
    ("오하요고자이마스", "おはようございます"),
    ("하지메마시테", "はじめまして"),
    ("사요나라", "さようなら"),
    ("이타다키마스", "いただきます"),
];

/// Common grammatical endings, matched as a *suffix* (not just a standalone
/// word) since they overwhelmingly attach directly to the preceding word
/// with no space — e.g. 나마에와 ("name" + topic particle), not "나마에 와".
/// Checked longest-first so e.g. 데스카 matches before the shorter 데스.
///
/// Trade-off: 와 as a bare trailing syllable is treated as the topic
/// particle は (correct far more often than not) rather than the syllable
/// わ — so a genuine content word ending in わ (e.g. 카와 "river") will be
/// mis-converted. Real sentence-level training data would resolve this from
/// context; a fixed list can't. Given how much more common the particle
/// usage is in casual phrases, this favors the common case.
const KNOWN_SUFFIXES: &[(&str, &str)] = &[
    ("데스카", "ですか"),
    ("데스요", "ですよ"),
    ("데스네", "ですね"),
    ("데스", "です"),
    ("마스카", "ますか"),
    ("마시타", "ました"),
    ("마스", "ます"),
    ("와", "は"), // topic particle は, pronounced "wa" — see trade-off note above
];

/// Rule-based fallback: decompose a single Hangul syllable into hiragana candidates.
///
/// Used when BeamDecoder's learned PhoneticMap has no entry for a hangul token.
/// Decomposes the Hangul syllable into jamo (choseong, jungseong, jongseong) and
/// maps to hiragana using phonological rules, including Korean conventional spellings.
///
/// Returns Vec<(hiragana, confidence)> sorted by confidence.
fn hangul_char_to_hiragana_fallback(ch: char) -> Vec<(String, f64)> {
    let code = ch as u32;
    if code < 0xAC00 || code > 0xD7A3 {
        return vec![]; // Not a Hangul syllable
    }

    let offset = code - 0xAC00;
    let cho = offset / (21 * 28);
    let jung = (offset % (21 * 28)) / 28;
    let jong = offset % 28;

    // Choseong → Japanese consonant candidates
    let consonants: Vec<(&str, f64)> = match cho {
        0  => vec![("k", 0.6), ("g", 0.3)],     // ㄱ
        1  => vec![("g", 0.7), ("k", 0.3)],     // ㄲ
        2  => vec![("n", 1.0)],                   // ㄴ
        3  => vec![("t", 0.6), ("d", 0.4)],     // ㄷ
        4  => vec![("d", 0.7), ("t", 0.3)],     // ㄸ
        5  => vec![("r", 1.0)],                   // ㄹ
        6  => vec![("m", 1.0)],                   // ㅁ
        7  => vec![("h", 0.4), ("b", 0.3), ("p", 0.2), ("w", 0.1)], // ㅂ
        8  => vec![("b", 0.7), ("p", 0.3)],     // ㅃ
        9  => vec![("s", 0.8), ("z", 0.2)],     // ㅅ
        10 => vec![("z", 0.7), ("s", 0.3)],     // ㅆ
        11 => vec![("", 1.0)],                     // ㅇ
        12 => vec![("j", 0.3), ("ch", 0.3), ("z", 0.2), ("ts", 0.2)], // ㅈ
        13 => vec![("ch", 0.35), ("j", 0.3), ("z", 0.2), ("ts", 0.15)], // ㅉ (관용: ち→찌)
        14 => vec![("ch", 0.6), ("ts", 0.3), ("t", 0.1)], // ㅊ
        15 => vec![("k", 1.0)],                   // ㅋ
        16 => vec![("t", 1.0)],                   // ㅌ
        17 => vec![("p", 0.7), ("f", 0.3)],     // ㅍ
        18 => vec![("h", 1.0)],                   // ㅎ
        _  => return vec![],
    };

    // Jungseong → vowel candidates
    let vowels: Vec<(&str, f64)> = match jung {
        0  => vec![("a", 1.0)],
        1  => vec![("a", 0.5), ("e", 0.5)],
        2  => vec![("ya", 1.0)],
        3  => vec![("ya", 0.5), ("ye", 0.5)],
        4  => vec![("e", 0.6), ("o", 0.3), ("a", 0.1)],
        5  => vec![("e", 1.0)],
        6  => vec![("yo", 0.7), ("ye", 0.3)],
        7  => vec![("ye", 1.0)],
        8  => vec![("o", 1.0)],
        9  => vec![("wa", 0.7), ("a", 0.3)],
        10 => vec![("wa", 0.7), ("we", 0.3)],
        11 => vec![("o", 0.7), ("we", 0.3)],
        12 => vec![("yo", 1.0)],
        13 => vec![("u", 1.0)],
        14 => vec![("wo", 0.5), ("we", 0.5)],
        15 => vec![("we", 1.0)],
        16 => vec![("u", 0.7), ("wi", 0.3)],
        17 => vec![("yu", 1.0)],
        18 => vec![("u", 0.7), ("i", 0.3)],
        19 => vec![("i", 1.0)],
        20 => vec![("i", 1.0)],
        _  => return vec![],
    };

    // Jongseong → final phoneme
    let final_phoneme: Option<&str> = match jong {
        0  => None,
        2 | 4 => Some("n"),   // ㄴ → ん
        6  => Some("m"),       // ㅁ → ん
        21 => Some("ng"),      // ㅇ → ん
        1 | 7 | 17 => Some("Q"), // ㄱ,ㄷ,ㅂ → っ
        _  => Some("n"),
    };

    // Convert final phoneme to hiragana suffix separately
    // (avoids romaji suffix-parsing bugs like "kon" failing because it ends with "on")
    let final_hira_suffix = match final_phoneme {
        Some("n") | Some("ng") | Some("m") => "ん",
        Some("Q") => "っ",
        _ => "",
    };

    // Build romaji candidates and convert to hiragana
    let mut results: Vec<(String, f64)> = Vec::new();
    for (cons, cc) in &consonants {
        for (vowel, vc) in &vowels {
            let romaji = resolve_romaji(cons, vowel);
            // Convert base syllable (consonant+vowel) to hiragana, then append suffix
            if let Some(base_hira) = romaji_to_hiragana_simple(&romaji) {
                let hira = if final_hira_suffix.is_empty() {
                    base_hira
                } else {
                    format!("{}{}", base_hira, final_hira_suffix)
                };
                if let Some(existing) = results.iter_mut().find(|(h, _)| h == &hira) {
                    existing.1 += cc * vc;
                } else {
                    results.push((hira, cc * vc));
                }
            }
        }
    }

    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(8);
    results
}

/// Resolve Japanese phonological rules: consonant + vowel → romaji
fn resolve_romaji(consonant: &str, vowel: &str) -> String {
    match (consonant, vowel) {
        ("s", "i")   => "shi".to_string(),
        ("t", "i")   => "chi".to_string(),
        ("t", "u")   => "tsu".to_string(),
        ("h", "u")   => "fu".to_string(),
        ("z", "i")   => "ji".to_string(),
        ("d", "i")   => "ji".to_string(),
        ("d", "u")   => "zu".to_string(),
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
        ("ts", "a")  => "tsa".to_string(),
        ("ts", "i")  => "tsi".to_string(),
        ("w", "a")   => "wa".to_string(),
        _ => format!("{}{}", consonant, vowel),
    }
}

/// Simple romaji → hiragana converter for the fallback path.
fn romaji_to_hiragana_simple(romaji: &str) -> Option<String> {
    // Handle trailing ん (n) and っ (Q) via recursion
    let (base, suffix) = if romaji.ends_with('Q') {
        (&romaji[..romaji.len()-1], "っ")
    } else if romaji.ends_with('n') && romaji.len() > 1
        && !romaji.ends_with("an") && !romaji.ends_with("in")
        && !romaji.ends_with("un") && !romaji.ends_with("en")
        && !romaji.ends_with("on") {
        (&romaji[..romaji.len()-1], "ん")
    } else if romaji.ends_with("ng") {
        (&romaji[..romaji.len()-2], "ん")
    } else if romaji.ends_with('m') && romaji.len() > 2 {
        (&romaji[..romaji.len()-1], "ん")
    } else {
        (romaji, "")
    };

    let hira_base = match base {
        // Vowels
        "a" => "あ", "i" => "い", "u" => "う", "e" => "え", "o" => "お",
        // K-row
        "ka" => "か", "ki" => "き", "ku" => "く", "ke" => "け", "ko" => "こ",
        // G-row
        "ga" => "が", "gi" => "ぎ", "gu" => "ぐ", "ge" => "げ", "go" => "ご",
        // S-row
        "sa" => "さ", "shi" | "si" => "し", "su" => "す", "se" => "せ", "so" => "そ",
        // Z-row
        "za" => "ざ", "ji" | "zi" => "じ", "zu" => "ず", "ze" => "ぜ", "zo" => "ぞ",
        // T-row
        "ta" => "た", "chi" | "ti" => "ち", "tsu" | "tu" => "つ", "te" => "て", "to" => "と",
        // D-row
        "da" => "だ", "di" => "ぢ", "du" | "dzu" => "づ", "de" => "で", "do" => "ど",
        // N-row
        "na" => "な", "ni" => "に", "nu" => "ぬ", "ne" => "ね", "no" => "の",
        // H-row
        "ha" => "は", "hi" => "ひ", "fu" | "hu" => "ふ", "he" => "へ", "ho" => "ほ",
        // B-row
        "ba" => "ば", "bi" => "び", "bu" => "ぶ", "be" => "べ", "bo" => "ぼ",
        // P-row
        "pa" => "ぱ", "pi" => "ぴ", "pu" => "ぷ", "pe" => "ぺ", "po" => "ぽ",
        // M-row
        "ma" => "ま", "mi" => "み", "mu" => "む", "me" => "め", "mo" => "も",
        // Y-row
        "ya" => "や", "yu" => "ゆ", "yo" => "よ",
        // R-row
        "ra" => "ら", "ri" => "り", "ru" => "る", "re" => "れ", "ro" => "ろ",
        // W-row
        "wa" => "わ", "wo" => "を",
        // N
        "n" => "ん",
        // Youon (拗音)
        "kya" => "きゃ", "kyu" => "きゅ", "kyo" => "きょ",
        "sha" => "しゃ", "shu" => "しゅ", "sho" => "しょ",
        "cha" => "ちゃ", "chu" => "ちゅ", "cho" => "ちょ",
        "nya" => "にゃ", "nyu" => "にゅ", "nyo" => "にょ",
        "hya" => "ひゃ", "hyu" => "ひゅ", "hyo" => "ひょ",
        "mya" => "みゃ", "myu" => "みゅ", "myo" => "みょ",
        "rya" => "りゃ", "ryu" => "りゅ", "ryo" => "りょ",
        "gya" => "ぎゃ", "gyu" => "ぎゅ", "gyo" => "ぎょ",
        "ja" => "じゃ", "ju" => "じゅ", "jo" => "じょ",
        "bya" => "びゃ", "byu" => "びゅ", "byo" => "びょ",
        "pya" => "ぴゃ", "pyu" => "ぴゅ", "pyo" => "ぴょ",
        _ => return None,
    };

    if suffix.is_empty() {
        Some(hira_base.to_string())
    } else {
        Some(format!("{}{}", hira_base, suffix))
    }
}

/// Convenience function: build a PhoneticMap from generated corpus sentences.
/// Extracts all (reading, hangul) word pairs and builds the reverse map.
pub fn build_phonetic_map_from_generated(
    corpus: &[crate::generator::GenSentence],
) -> PhoneticMap {
    let mut pairs: Vec<(String, String, u64)> = Vec::new();
    let mut freq_map: HashMap<(String, String), u64> = HashMap::new();

    for sentence in corpus {
        for word in &sentence.words {
            *freq_map.entry((word.reading.clone(), word.hangul.clone())).or_insert(0) += 1;
        }
    }

    for ((reading, hangul), freq) in freq_map {
        pairs.push((reading, hangul, freq));
    }

    let mut map = PhoneticMap::new();
    map.build_from_pairs(&pairs);
    map
}

/// Build a PhoneticMap by generating a corpus with given vocab and chunked processing.
/// Memory-efficient: processes in 1M-sentence chunks.
pub fn build_phonetic_map_chunked(
    vocab: &[crate::vocab::VocabEntry],
    sentence_count: usize,
) -> PhoneticMap {
    use crate::generator::generate_corpus_chunked;

    let mut freq_map: HashMap<(String, String), u64> = HashMap::new();

    generate_corpus_chunked(vocab, sentence_count, 1_000_000, |chunk| {
        for sentence in chunk {
            for word in &sentence.words {
                *freq_map.entry((word.reading.clone(), word.hangul.clone())).or_insert(0) += 1;
            }
        }
    });

    let pairs: Vec<(String, String, u64)> = freq_map
        .into_iter()
        .map(|((reading, hangul), freq)| (reading, hangul, freq))
        .collect();

    let mut map = PhoneticMap::new();
    map.build_from_pairs(&pairs);
    map
}

// ═══════════════════════════════════════════════════════════════════════════
// 방안 1: 마커 방식 (Marker-based)
//
// 정방향 변환(hiragana_to_hangul)에 특수 마커를 삽입하여 역변환 시 정보 보존.
//   ん → 받침 대신 별도 마커 문자 'ㄴ' (+ 별도 separator)
//   っ → 별도 마커 문자 'ッ'
//   ー → 별도 마커 문자 '―'
// ═══════════════════════════════════════════════════════════════════════════

/// Marker-aware hiragana → hangul conversion.
/// Inserts special marker characters instead of merging ん/っ/ー into hangul.
pub fn hiragana_to_hangul_marked(input: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        // ん → marker 'ⓝ' (distinct from any hangul)
        if ch == 'ん' {
            result.push('ⓝ');
            i += 1;
            continue;
        }

        // っ → marker 'ⓧ'
        if ch == 'っ' {
            result.push('ⓧ');
            i += 1;
            continue;
        }

        // ー → marker 'ⓜ' (long vowel mark)
        if ch == 'ー' {
            result.push('ⓜ');
            i += 1;
            continue;
        }

        // Try youon (2-char)
        if i + 1 < chars.len() {
            let two_char = format!("{}{}", ch, chars[i + 1]);
            let two_hangul = hiragana_to_hangul(&two_char);
            let plain_concat = format!("{}{}",
                hiragana_to_hangul(&ch.to_string()),
                hiragana_to_hangul(&chars[i+1].to_string()));
            // If two-char maps to something different than concat of singles, it's a youon
            if two_hangul != plain_concat && !two_hangul.is_empty() {
                result.push_str(&two_hangul);
                i += 2;
                continue;
            }
        }

        // Single hiragana → hangul (using original converter for basic chars)
        let single = hiragana_to_hangul(&ch.to_string());
        if !single.is_empty() {
            result.push_str(&single);
        } else {
            result.push(ch);
        }
        i += 1;
    }

    result
}

/// Build PhoneticMap using marker-based forward conversion.
pub fn build_phonetic_map_marked_chunked(
    vocab: &[crate::vocab::VocabEntry],
    sentence_count: usize,
) -> PhoneticMap {
    use crate::generator::generate_corpus_chunked;

    let mut freq_map: HashMap<(String, String), u64> = HashMap::new();

    generate_corpus_chunked(vocab, sentence_count, 1_000_000, |chunk| {
        for sentence in chunk {
            for word in &sentence.words {
                let marked_hangul = hiragana_to_hangul_marked(&word.reading);
                *freq_map.entry((word.reading.clone(), marked_hangul)).or_insert(0) += 1;
            }
        }
    });

    let pairs: Vec<(String, String, u64)> = freq_map
        .into_iter()
        .map(|((reading, hangul), freq)| (reading, hangul, freq))
        .collect();

    let mut map = PhoneticMap::new();
    map.build_from_pairs(&pairs);
    map
}

/// Decode using marker-based map.
/// Input must be marker-encoded hangul (from hiragana_to_hangul_marked).
pub fn decode_marked(hangul: &str, map: &PhoneticMap, beam_width: usize, max_candidates: usize) -> Vec<(String, f64)> {
    let decoder = BeamDecoder::new(map, beam_width, max_candidates);
    decoder.decode(hangul)
}


// ═══════════════════════════════════════════════════════════════════════════
// 방안 2: 멀티토큰 방식 (Multi-token)
//
// 1글자 단위 대신 2~3글자 단위로 (hangul_ngram → hiragana_ngram) 매핑.
// "심파" → "しんぱ", "켓카" → "けっか" 같은 패턴을 직접 학습.
// ═══════════════════════════════════════════════════════════════════════════

/// Build a multi-token PhoneticMap: includes 1-char, 2-char, and 3-char hangul tokens.
pub fn build_phonetic_map_multitoken_chunked(
    vocab: &[crate::vocab::VocabEntry],
    sentence_count: usize,
) -> PhoneticMap {
    use crate::generator::generate_corpus_chunked;

    // Collect word-level (reading, hangul) frequency
    let mut word_freq: HashMap<(String, String), u64> = HashMap::new();

    generate_corpus_chunked(vocab, sentence_count, 1_000_000, |chunk| {
        for sentence in chunk {
            for word in &sentence.words {
                *word_freq.entry((word.reading.clone(), word.hangul.clone())).or_insert(0) += 1;
            }
        }
    });

    // Build n-gram alignment pairs
    let mut ngram_counts: HashMap<String, HashMap<String, u64>> = HashMap::new();

    for ((reading, hangul), freq) in &word_freq {
        let hira_chars: Vec<char> = reading.chars().collect();
        let hang_chars: Vec<char> = hangul.chars().collect();

        // 1-char alignments (existing approach)
        let alignments = align_hiragana_hangul(reading, hangul);
        for (hira_tok, hang_tok) in &alignments {
            *ngram_counts.entry(hang_tok.clone()).or_default().entry(hira_tok.clone()).or_insert(0) += freq;
        }

        // 2-char and 3-char hangul ngrams with corresponding hiragana
        // We need to find what hiragana substring corresponds to each hangul ngram.
        // Use the full word pair: try all 2-char and 3-char windows of hangul,
        // and find the corresponding hiragana via index mapping.
        if let Some(index_map) = build_char_index_map(reading, hangul) {
            // 2-char hangul ngrams
            for wi in 0..hang_chars.len().saturating_sub(1) {
                let h2: String = hang_chars[wi..=wi+1].iter().collect();
                // Find corresponding hiragana range
                if let (Some(&hira_start), Some(&hira_end)) = (index_map.get(&wi), index_map.get(&(wi+1))) {
                    let hira_end_actual = (hira_end + 1).min(hira_chars.len());
                    if hira_start < hira_end_actual {
                        let hira_ngram: String = hira_chars[hira_start..hira_end_actual].iter().collect();
                        *ngram_counts.entry(h2).or_default().entry(hira_ngram).or_insert(0) += freq;
                    }
                }
            }

            // 3-char hangul ngrams
            for wi in 0..hang_chars.len().saturating_sub(2) {
                let h3: String = hang_chars[wi..=wi+2].iter().collect();
                if let (Some(&hira_start), Some(&hira_end)) = (index_map.get(&wi), index_map.get(&(wi+2))) {
                    let hira_end_actual = (hira_end + 1).min(hira_chars.len());
                    if hira_start < hira_end_actual {
                        let hira_ngram: String = hira_chars[hira_start..hira_end_actual].iter().collect();
                        *ngram_counts.entry(h3).or_default().entry(hira_ngram).or_insert(0) += freq;
                    }
                }
            }
        }
    }

    // Convert to PhoneticMap
    let mut map = PhoneticMap::new();
    for (hangul_token, hira_map) in ngram_counts {
        let total: u64 = hira_map.values().sum();
        let mut mappings: Vec<PhoneticMapping> = hira_map
            .into_iter()
            .map(|(hiragana, frequency)| PhoneticMapping {
                hiragana,
                frequency,
                probability: frequency as f64 / total as f64,
            })
            .collect();
        mappings.sort_by(|a, b| b.probability.partial_cmp(&a.probability).unwrap());
        // Keep only mappings with >0.5% probability to avoid noise
        mappings.retain(|m| m.probability > 0.005);
        map.map.insert(hangul_token, mappings);
    }
    map.total_pairs = word_freq.values().sum();
    map
}

/// Build a character-level index map: hangul_char_index → hiragana_char_index.
/// Returns None if alignment fails.
fn build_char_index_map(hiragana: &str, hangul: &str) -> Option<HashMap<usize, usize>> {
    let hira_chars: Vec<char> = hiragana.chars().collect();
    let hang_chars: Vec<char> = hangul.chars().collect();

    let mut map: HashMap<usize, usize> = HashMap::new();
    let mut hi = 0usize;
    let mut ki = 0usize;

    while hi < hira_chars.len() && ki < hang_chars.len() {
        let ch = hira_chars[hi];

        if ch == 'ん' || ch == 'っ' || ch == 'ー' {
            // These don't consume a hangul char (they modify the previous one)
            hi += 1;
            continue;
        }

        // Try youon
        if hi + 1 < hira_chars.len() {
            let two = format!("{}{}", ch, hira_chars[hi + 1]);
            let two_hangul_str = hiragana_to_hangul(&two);
            let two_hchars: Vec<char> = two_hangul_str.chars().collect();
            if two_hchars.len() == 1 && ki < hang_chars.len() && two_hchars[0] == hang_chars[ki] {
                map.insert(ki, hi);
                hi += 2;
                ki += 1;
                continue;
            }
        }

        // Single char
        map.insert(ki, hi);
        hi += 1;
        ki += 1;
    }

    // Map remaining hangul chars
    while ki < hang_chars.len() {
        map.insert(ki, hi.saturating_sub(1));
        ki += 1;
    }

    Some(map)
}


// ═══════════════════════════════════════════════════════════════════════════
// 방안 1+2 조합: 마커 + 멀티토큰 (Marker + Multi-token)
//
// 마커 방식으로 정방향 변환(ん→ⓝ, っ→ⓧ, ー→ⓜ)하되,
// 멀티토큰(2~3글자) ngram 매핑도 함께 학습.
// 예: "시ⓝ파"→"しんぱ", "케ⓧ카"→"けっか"
// ═══════════════════════════════════════════════════════════════════════════

/// Build a combined marker+multi-token PhoneticMap.
/// Uses hiragana_to_hangul_marked for forward conversion (preserving ん/っ/ー as markers),
/// then builds 1/2/3-char ngram mappings from the marked hangul.
pub fn build_phonetic_map_marked_multitoken_chunked(
    vocab: &[crate::vocab::VocabEntry],
    sentence_count: usize,
) -> PhoneticMap {
    use crate::generator::generate_corpus_chunked;

    // Collect word-level (reading, marked_hangul) frequency
    let mut word_freq: HashMap<(String, String), u64> = HashMap::new();

    generate_corpus_chunked(vocab, sentence_count, 1_000_000, |chunk| {
        for sentence in chunk {
            for word in &sentence.words {
                let marked_hangul = hiragana_to_hangul_marked(&word.reading);
                *word_freq.entry((word.reading.clone(), marked_hangul)).or_insert(0) += 1;
            }
        }
    });

    // Build n-gram alignment pairs
    let mut ngram_counts: HashMap<String, HashMap<String, u64>> = HashMap::new();

    for ((reading, marked_hangul), freq) in &word_freq {
        let hira_chars: Vec<char> = reading.chars().collect();
        let hang_chars: Vec<char> = marked_hangul.chars().collect();

        // 1-char alignments using marker-aware alignment
        let alignments = align_marked_hiragana_hangul(reading, marked_hangul);
        for (hira_tok, hang_tok) in &alignments {
            *ngram_counts.entry(hang_tok.clone()).or_default().entry(hira_tok.clone()).or_insert(0) += freq;
        }

        // 2-char and 3-char marked-hangul ngrams with corresponding hiragana
        if let Some(index_map) = build_marked_char_index_map(reading, marked_hangul) {
            // 2-char ngrams
            for wi in 0..hang_chars.len().saturating_sub(1) {
                let h2: String = hang_chars[wi..=wi+1].iter().collect();
                if let (Some(&hira_start), Some(&hira_end)) = (index_map.get(&wi), index_map.get(&(wi+1))) {
                    // For markers, hira_end points to the hiragana char that maps to hang[wi+1]
                    // We want to include all hiragana chars up to and including hira_end
                    let hira_end_actual = find_hira_span_end(&hira_chars, hira_end);
                    if hira_start < hira_end_actual && hira_end_actual <= hira_chars.len() {
                        let hira_ngram: String = hira_chars[hira_start..hira_end_actual].iter().collect();
                        *ngram_counts.entry(h2).or_default().entry(hira_ngram).or_insert(0) += freq;
                    }
                }
            }

            // 3-char ngrams
            for wi in 0..hang_chars.len().saturating_sub(2) {
                let h3: String = hang_chars[wi..=wi+2].iter().collect();
                if let (Some(&hira_start), Some(&hira_end)) = (index_map.get(&wi), index_map.get(&(wi+2))) {
                    let hira_end_actual = find_hira_span_end(&hira_chars, hira_end);
                    if hira_start < hira_end_actual && hira_end_actual <= hira_chars.len() {
                        let hira_ngram: String = hira_chars[hira_start..hira_end_actual].iter().collect();
                        *ngram_counts.entry(h3).or_default().entry(hira_ngram).or_insert(0) += freq;
                    }
                }
            }
        }
    }

    // Convert to PhoneticMap
    let mut map = PhoneticMap::new();
    for (hangul_token, hira_map) in ngram_counts {
        let total: u64 = hira_map.values().sum();
        let mut mappings: Vec<PhoneticMapping> = hira_map
            .into_iter()
            .map(|(hiragana, frequency)| PhoneticMapping {
                hiragana,
                frequency,
                probability: frequency as f64 / total as f64,
            })
            .collect();
        mappings.sort_by(|a, b| b.probability.partial_cmp(&a.probability).unwrap());
        // Keep only mappings with >0.5% probability to avoid noise
        mappings.retain(|m| m.probability > 0.005);
        map.map.insert(hangul_token, mappings);
    }
    map.total_pairs = word_freq.values().sum();
    map
}

/// Build a marked+multitoken PhoneticMap directly from pre-built GenSentence data.
/// Same algorithm as build_phonetic_map_marked_multitoken_chunked but without generation.
pub fn build_phonetic_map_marked_multitoken_from_generated(
    corpus: &[crate::generator::GenSentence],
) -> PhoneticMap {
    let mut word_freq: HashMap<(String, String), u64> = HashMap::new();

    for sentence in corpus {
        for word in &sentence.words {
            let marked_hangul = hiragana_to_hangul_marked(&word.reading);
            *word_freq.entry((word.reading.clone(), marked_hangul)).or_insert(0) += 1;
        }
    }

    let mut ngram_counts: HashMap<String, HashMap<String, u64>> = HashMap::new();

    for ((reading, marked_hangul), freq) in &word_freq {
        let hira_chars: Vec<char> = reading.chars().collect();
        let hang_chars: Vec<char> = marked_hangul.chars().collect();

        let alignments = align_marked_hiragana_hangul(reading, marked_hangul);
        for (hira_tok, hang_tok) in &alignments {
            *ngram_counts.entry(hang_tok.clone()).or_default().entry(hira_tok.clone()).or_insert(0) += freq;
        }

        if let Some(index_map) = build_marked_char_index_map(reading, marked_hangul) {
            for wi in 0..hang_chars.len().saturating_sub(1) {
                let h2: String = hang_chars[wi..=wi+1].iter().collect();
                if let (Some(&hira_start), Some(&hira_end)) = (index_map.get(&wi), index_map.get(&(wi+1))) {
                    let hira_end_actual = find_hira_span_end(&hira_chars, hira_end);
                    if hira_start < hira_end_actual && hira_end_actual <= hira_chars.len() {
                        let hira_ngram: String = hira_chars[hira_start..hira_end_actual].iter().collect();
                        *ngram_counts.entry(h2).or_default().entry(hira_ngram).or_insert(0) += freq;
                    }
                }
            }
            for wi in 0..hang_chars.len().saturating_sub(2) {
                let h3: String = hang_chars[wi..=wi+2].iter().collect();
                if let (Some(&hira_start), Some(&hira_end)) = (index_map.get(&wi), index_map.get(&(wi+2))) {
                    let hira_end_actual = find_hira_span_end(&hira_chars, hira_end);
                    if hira_start < hira_end_actual && hira_end_actual <= hira_chars.len() {
                        let hira_ngram: String = hira_chars[hira_start..hira_end_actual].iter().collect();
                        *ngram_counts.entry(h3).or_default().entry(hira_ngram).or_insert(0) += freq;
                    }
                }
            }
        }
    }

    let mut map = PhoneticMap::new();
    for (hangul_token, hira_map) in ngram_counts {
        let total: u64 = hira_map.values().sum();
        let mut mappings: Vec<PhoneticMapping> = hira_map
            .into_iter()
            .map(|(hiragana, frequency)| PhoneticMapping {
                hiragana,
                frequency,
                probability: frequency as f64 / total as f64,
            })
            .collect();
        mappings.sort_by(|a, b| b.probability.partial_cmp(&a.probability).unwrap());
        mappings.retain(|m| m.probability > 0.005);
        map.map.insert(hangul_token, mappings);
    }
    map.total_pairs = word_freq.values().sum();
    map
}

/// Align hiragana and marked-hangul strings character by character.
/// Like align_hiragana_hangul but handles marker chars (ⓝ→ん, ⓧ→っ, ⓜ→ー).
///
/// The marked hangul preserves ん/っ/ー as ⓝ/ⓧ/ⓜ markers, so both strings
/// should have the same logical length. This makes alignment much simpler.
fn align_marked_hiragana_hangul(hiragana: &str, marked_hangul: &str) -> Vec<(String, String)> {
    let hira_chars: Vec<char> = hiragana.chars().collect();
    let hang_chars: Vec<char> = marked_hangul.chars().collect();
    let mut alignments = Vec::new();

    let mut hi = 0;
    let mut ki = 0;

    while hi < hira_chars.len() && ki < hang_chars.len() {
        let hch = hira_chars[hi];
        let kch = hang_chars[ki];

        // Marker characters: these have a 1-to-1 correspondence
        if kch == 'ⓝ' {
            // The hiragana side should be ん here
            if hch == 'ん' {
                alignments.push(("ん".to_string(), "ⓝ".to_string()));
                hi += 1;
                ki += 1;
            } else {
                // Desync: skip the marker, don't consume hiragana
                ki += 1;
            }
            continue;
        }
        if kch == 'ⓧ' {
            if hch == 'っ' {
                alignments.push(("っ".to_string(), "ⓧ".to_string()));
                hi += 1;
                ki += 1;
            } else {
                ki += 1;
            }
            continue;
        }
        if kch == 'ⓜ' {
            if hch == 'ー' {
                alignments.push(("ー".to_string(), "ⓜ".to_string()));
                hi += 1;
                ki += 1;
            } else {
                ki += 1;
            }
            continue;
        }

        // On the hiragana side, skip ん/っ/ー that weren't expected
        // (these would already be mapped to markers on the hangul side)
        if hch == 'ん' || hch == 'っ' || hch == 'ー' {
            hi += 1;
            continue;
        }

        // Try youon (2-char hiragana → 1 hangul syllable)
        if hi + 1 < hira_chars.len() {
            let next = hira_chars[hi + 1];
            if next == 'ゃ' || next == 'ゅ' || next == 'ょ' {
                let two = format!("{}{}", hch, next);
                let two_hangul = hiragana_to_hangul(&two);
                let two_hangul_chars: Vec<char> = two_hangul.chars().collect();

                if two_hangul_chars.len() == 1 && two_hangul_chars[0] == kch {
                    alignments.push((two, kch.to_string()));
                    hi += 2;
                    ki += 1;
                    continue;
                }
            }
        }

        // Single hiragana → single hangul
        alignments.push((hch.to_string(), kch.to_string()));
        hi += 1;
        ki += 1;
    }

    alignments
}

/// Build character-level index map for marked hangul: marked_hangul_char_index → hiragana_char_index.
fn build_marked_char_index_map(hiragana: &str, marked_hangul: &str) -> Option<HashMap<usize, usize>> {
    let hira_chars: Vec<char> = hiragana.chars().collect();
    let hang_chars: Vec<char> = marked_hangul.chars().collect();

    let mut map: HashMap<usize, usize> = HashMap::new();
    let mut hi = 0usize;
    let mut ki = 0usize;

    while hi < hira_chars.len() && ki < hang_chars.len() {
        let hch = hira_chars[hi];
        let kch = hang_chars[ki];

        // Markers map directly
        if kch == 'ⓝ' {
            if hch == 'ん' {
                map.insert(ki, hi);
                hi += 1;
                ki += 1;
            } else {
                ki += 1;
            }
            continue;
        }
        if kch == 'ⓧ' {
            if hch == 'っ' {
                map.insert(ki, hi);
                hi += 1;
                ki += 1;
            } else {
                ki += 1;
            }
            continue;
        }
        if kch == 'ⓜ' {
            if hch == 'ー' {
                map.insert(ki, hi);
                hi += 1;
                ki += 1;
            } else {
                ki += 1;
            }
            continue;
        }

        // Skip unexpected ん/っ/ー on hiragana side
        if hch == 'ん' || hch == 'っ' || hch == 'ー' {
            hi += 1;
            continue;
        }

        // Try youon
        if hi + 1 < hira_chars.len() {
            let next = hira_chars[hi + 1];
            if next == 'ゃ' || next == 'ゅ' || next == 'ょ' {
                let two = format!("{}{}", hch, next);
                let two_hangul_str = hiragana_to_hangul(&two);
                let two_hchars: Vec<char> = two_hangul_str.chars().collect();
                if two_hchars.len() == 1 && two_hchars[0] == kch {
                    map.insert(ki, hi);
                    hi += 2;
                    ki += 1;
                    continue;
                }
            }
        }

        // Single char
        map.insert(ki, hi);
        hi += 1;
        ki += 1;
    }

    // Map remaining
    while ki < hang_chars.len() {
        map.insert(ki, hi.saturating_sub(1));
        ki += 1;
    }

    Some(map)
}

/// Find the end of a hiragana span starting at index `start`.
/// For a regular char, span is start+1. For youon start chars, span includes the small kana.
fn find_hira_span_end(hira_chars: &[char], start: usize) -> usize {
    let mut end = start + 1;
    // Include following small kana (ゃゅょ) that are part of a youon
    while end < hira_chars.len() {
        let ch = hira_chars[end];
        if ch == 'ゃ' || ch == 'ゅ' || ch == 'ょ' {
            end += 1;
        } else {
            break;
        }
    }
    end
}


// ═══════════════════════════════════════════════════════════════════════════
// 방안 3: 하이브리드 (Hybrid)
//
// 기존 음소 임베딩으로 기본 변환 후, 규칙 기반 후처리로 ん/っ/ー를 복원.
//
// 전략:
// - BeamDecoder로 기본 후보 생성
// - 각 후보에 대해 ん/っ/ー 삽입 변형(variants)을 규칙으로 생성
// - 변형 후보도 결과에 포함
// ═══════════════════════════════════════════════════════════════════════════

/// Hybrid decoder: beam decode + rule-based post-processing for ん/っ/ー.
pub fn decode_hybrid(
    hangul: &str,
    original_hangul: &str, // the original hangul from user input
    map: &PhoneticMap,
    beam_width: usize,
    max_candidates: usize,
) -> Vec<(String, f64)> {
    let decoder = BeamDecoder::new(map, beam_width, max_candidates);
    let base_results = decoder.decode(hangul);

    let mut all_results: Vec<(String, f64)> = Vec::new();

    for (hiragana, conf) in &base_results {
        // Add the base result
        all_results.push((hiragana.clone(), *conf));

        // Generate ん/っ/ー variants
        let variants = generate_special_variants(hiragana, original_hangul);
        for (variant, penalty) in variants {
            all_results.push((variant, conf * penalty));
        }
    }

    // Sort and dedup
    all_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    all_results.dedup_by(|a, b| a.0 == b.0);
    all_results.truncate(max_candidates);
    all_results
}

/// Generate variants with ん, っ, ー inserted at likely positions.
fn generate_special_variants(hiragana: &str, original_hangul: &str) -> Vec<(String, f64)> {
    let hira_chars: Vec<char> = hiragana.chars().collect();
    let hang_chars: Vec<char> = original_hangul.chars().collect();
    let mut variants = Vec::new();

    // Strategy 1: Insert ん before consonant positions where hangul has batchim ㄴ/ㅁ/ㅇ
    // Check each hangul char for batchim that indicates ん
    for (ki, &hch) in hang_chars.iter().enumerate() {
        if let Some(batchim) = get_hangul_jongseong(hch) {
            // ㄴ(2), ㅁ(6), ㅇ(21) → likely ん
            if batchim == 2 || batchim == 6 || batchim == 21 {
                // Find corresponding position in hiragana (rough: ki maps to ~ki in hira)
                let insert_pos = (ki + 1).min(hira_chars.len());
                let mut v: Vec<char> = hira_chars.clone();
                v.insert(insert_pos, 'ん');
                variants.push((v.into_iter().collect::<String>(), 0.7));
            }
            // ㄱ(1), ㄷ(7), ㅂ(17), ㅅ(9), ㅆ(10) → likely っ
            if batchim == 1 || batchim == 7 || batchim == 17 || batchim == 9 || batchim == 10 {
                let insert_pos = (ki + 1).min(hira_chars.len());
                let mut v: Vec<char> = hira_chars.clone();
                v.insert(insert_pos, 'っ');
                variants.push((v.into_iter().collect::<String>(), 0.6));
            }
        }
    }

    // Strategy 2: Replace う with ー where hangul has 우 that looks like a long vowel
    // (i.e., "우" following a vowel sound)
    for (hi, &hch) in hira_chars.iter().enumerate() {
        if hch == 'う' && hi > 0 {
            // Check if previous char ends in o/u sound → likely ー
            let prev = hira_chars[hi - 1];
            if is_vowel_row_ou(prev) {
                let mut v = hira_chars.clone();
                v[hi] = 'ー';
                variants.push((v.into_iter().collect::<String>(), 0.5));
            }
        }
    }

    variants
}

/// Extract jongseong (final consonant) from a hangul syllable.
fn get_hangul_jongseong(ch: char) -> Option<u32> {
    let code = ch as u32;
    if code < 0xAC00 || code > 0xD7A3 {
        return None;
    }
    let offset = code - 0xAC00;
    let jong = offset % 28;
    if jong == 0 { None } else { Some(jong) }
}

/// Check if a hiragana character is in the o/u vowel row
/// (where ー is commonly used instead of repeating the vowel).
fn is_vowel_row_ou(ch: char) -> bool {
    matches!(ch,
        'お' | 'こ' | 'そ' | 'と' | 'の' | 'ほ' | 'も' | 'よ' | 'ろ' | 'を' | 'ご' | 'ぞ' | 'ど' | 'ぼ' | 'ぽ' |
        'う' | 'く' | 'す' | 'つ' | 'ぬ' | 'ふ' | 'む' | 'ゆ' | 'る' | 'ぐ' | 'ず' | 'づ' | 'ぶ' | 'ぷ' |
        // Katakana equivalents
        'オ' | 'コ' | 'ソ' | 'ト' | 'ノ' | 'ホ' | 'モ' | 'ヨ' | 'ロ' | 'ヲ' | 'ゴ' | 'ゾ' | 'ド' | 'ボ' | 'ポ' |
        'ウ' | 'ク' | 'ス' | 'ツ' | 'ヌ' | 'フ' | 'ム' | 'ユ' | 'ル' | 'グ' | 'ズ' | 'ヅ' | 'ブ' | 'プ' |
        // Also e-row → ー in katakana loanwords
        'え' | 'け' | 'せ' | 'て' | 'ね' | 'へ' | 'め' | 'れ' | 'げ' | 'ぜ' | 'で' | 'べ' | 'ぺ' |
        'エ' | 'ケ' | 'セ' | 'テ' | 'ネ' | 'ヘ' | 'メ' | 'レ' | 'ゲ' | 'ゼ' | 'デ' | 'ベ' | 'ペ' |
        // a-row → ー
        'あ' | 'か' | 'さ' | 'た' | 'な' | 'は' | 'ま' | 'や' | 'ら' | 'わ' | 'が' | 'ざ' | 'だ' | 'ば' | 'ぱ' |
        'ア' | 'カ' | 'サ' | 'タ' | 'ナ' | 'ハ' | 'マ' | 'ヤ' | 'ラ' | 'ワ' | 'ガ' | 'ザ' | 'ダ' | 'バ' | 'パ' |
        // i-row → ー
        'い' | 'き' | 'し' | 'ち' | 'に' | 'ひ' | 'み' | 'り' | 'ぎ' | 'じ' | 'ぢ' | 'び' | 'ぴ' |
        'イ' | 'キ' | 'シ' | 'チ' | 'ニ' | 'ヒ' | 'ミ' | 'リ' | 'ギ' | 'ジ' | 'ヂ' | 'ビ' | 'ピ'
    )
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_basic() {
        // つくば → 츠쿠바
        let alignments = align_hiragana_hangul("つくば", "츠쿠바");
        assert!(!alignments.is_empty());
        // Should align: つ↔츠, く↔쿠, ば↔바
        assert_eq!(alignments.len(), 3);
        assert_eq!(alignments[0], ("つ".to_string(), "츠".to_string()));
        assert_eq!(alignments[1], ("く".to_string(), "쿠".to_string()));
    }

    #[test]
    fn test_phonetic_map_basic() {
        let mut map = PhoneticMap::new();
        let pairs = vec![
            ("つくば".to_string(), "츠쿠바".to_string(), 100),
            ("ちかく".to_string(), "치카쿠".to_string(), 50),
            ("つき".to_string(), "츠키".to_string(), 80),
        ];
        map.build_from_pairs(&pairs);

        // 츠 should map to つ with high probability
        let candidates = map.get_candidates("츠").unwrap();
        assert_eq!(candidates[0].hiragana, "つ");

        // 치 should map to ち
        let candidates = map.get_candidates("치").unwrap();
        assert_eq!(candidates[0].hiragana, "ち");
    }

    #[test]
    fn test_beam_decode() {
        let mut map = PhoneticMap::new();
        let pairs = vec![
            ("つくば".to_string(), "츠쿠바".to_string(), 100),
            ("ちかく".to_string(), "치카쿠".to_string(), 50),
        ];
        map.build_from_pairs(&pairs);

        let decoder = BeamDecoder::new(&map, 5, 10);
        let results = decoder.decode("츠쿠바");
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "つくば");
    }

    #[test]
    fn test_fallback_jongseong_n() {
        // 곤 (ㄱ+ㅗ+ㄴ) should produce こん/ごん, not fail
        let results = super::hangul_char_to_hiragana_fallback('곤');
        assert!(!results.is_empty(), "곤 fallback should not be empty");
        let surfaces: Vec<&str> = results.iter().map(|(s, _)| s.as_str()).collect();
        assert!(surfaces.contains(&"こん"), "곤 should produce こん, got: {:?}", surfaces);
        assert!(surfaces.contains(&"ごん"), "곤 should produce ごん, got: {:?}", surfaces);
    }

    #[test]
    fn test_fallback_jongseong_various() {
        // 산 (ㅅ+ㅏ+ㄴ) → さん
        let results = super::hangul_char_to_hiragana_fallback('산');
        let surfaces: Vec<&str> = results.iter().map(|(s, _)| s.as_str()).collect();
        assert!(surfaces.contains(&"さん"), "산 should produce さん, got: {:?}", surfaces);

        // 킨 (ㅋ+ㅣ+ㄴ) → きん
        let results = super::hangul_char_to_hiragana_fallback('킨');
        let surfaces: Vec<&str> = results.iter().map(|(s, _)| s.as_str()).collect();
        assert!(surfaces.contains(&"きん"), "킨 should produce きん, got: {:?}", surfaces);

        // 갇 (ㄱ+ㅏ+ㄷ, jong=7) → かっ/がっ (Q = っ)
        let results = super::hangul_char_to_hiragana_fallback('갇');
        let surfaces: Vec<&str> = results.iter().map(|(s, _)| s.as_str()).collect();
        assert!(surfaces.contains(&"かっ"), "갇 should produce かっ, got: {:?}", surfaces);
    }

    #[test]
    fn test_fallback_konnichiwa_full() {
        // Simulate full 곤니찌와 through fallback (character by character)
        let gon = super::hangul_char_to_hiragana_fallback('곤');
        let ni = super::hangul_char_to_hiragana_fallback('니');
        let jji = super::hangul_char_to_hiragana_fallback('찌');
        let wa = super::hangul_char_to_hiragana_fallback('와');

        assert!(!gon.is_empty(), "곤 should not be empty");
        assert!(!ni.is_empty(), "니 should not be empty");
        assert!(!jji.is_empty(), "찌 should not be empty");
        assert!(!wa.is_empty(), "와 should not be empty");

        let gon_s: Vec<&str> = gon.iter().map(|(s, _)| s.as_str()).collect();
        let ni_s: Vec<&str> = ni.iter().map(|(s, _)| s.as_str()).collect();
        let jji_s: Vec<&str> = jji.iter().map(|(s, _)| s.as_str()).collect();
        let wa_s: Vec<&str> = wa.iter().map(|(s, _)| s.as_str()).collect();

        assert!(gon_s.contains(&"こん"), "곤→こん expected, got: {:?}", gon_s);
        assert!(ni_s.contains(&"に"), "니→に expected, got: {:?}", ni_s);
        assert!(jji_s.contains(&"ち"), "찌→ち expected, got: {:?}", jji_s);
        assert!(wa_s.contains(&"わ"), "와→わ expected, got: {:?}", wa_s);
    }

    fn empty_decoder() -> BeamDecoder<'static> {
        // Leaked on purpose: tests only, gives a PhoneticMap with the process
        // lifetime so BeamDecoder's borrow is trivially 'static.
        let map: &'static PhoneticMap = Box::leak(Box::new(PhoneticMap::new()));
        BeamDecoder::new(map, 6, 5)
    }

    #[test]
    fn test_wa_particle_is_ha_standalone_and_attached() {
        let decoder = empty_decoder();
        assert_eq!(decoder.decode_sentence("와"), "は");
        // Attached to a preceding word is the far more common real pattern.
        assert_eq!(decoder.decode_sentence("나마에와"), "なまえは");
    }

    #[test]
    fn test_known_word_wins_over_suffix_rule() {
        let decoder = empty_decoder();
        // 곤니찌와 is the greeting itself — 와 here isn't a topic particle,
        // so the exact-word dictionary must take priority over the suffix rule.
        assert_eq!(decoder.decode_sentence("곤니찌와"), "こんにちわ");
    }

    #[test]
    fn test_desu_suffix_endings() {
        let decoder = empty_decoder();
        assert_eq!(decoder.decode_sentence("데스카"), "ですか");
        assert_eq!(decoder.decode_sentence("난데스카"), "なんですか");
        assert_eq!(decoder.decode_sentence("마스"), "ます");
    }

    #[test]
    fn test_decode_sentence_preserves_spacing() {
        let decoder = empty_decoder();
        let result = decoder.decode_sentence("나마에와 난데스카");
        assert_eq!(result, "なまえは なんですか");
    }
}
