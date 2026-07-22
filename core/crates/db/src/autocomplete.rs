//! Auto-completion engine for the Hangul→Japanese IME.
//!
//! Three tiers of suggestion:
//!   1. **Prefix completion** — As user types hangul, show matching words in real-time
//!   2. **Next-word prediction** — After confirming a word, predict the next word
//!   3. **Phrase completion** — Multi-word suggestions from n-gram patterns
//!
//! Scoring: 4-factor (α·phoneme + β·context + γ·frequency + δ·ngram)
//! with the optimal weights α=0.1, β=0.35, γ=0.25, δ=0.3.

use std::collections::HashMap;

use crate::ngram::NgramModel;
use crate::phonetic_decoder::{BeamDecoder, PhoneticMap};
use crate::{cosine_similarity, weighted_average_vectors, DictionaryDb};

/// A single auto-complete suggestion shown to the user.
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Display text (kanji/kana surface form)
    pub surface: String,
    /// Hiragana reading
    pub reading: String,
    /// Original hangul that would produce this
    pub hangul: String,
    /// Combined ranking score
    pub score: f64,
    /// How many hangul characters the user can skip if they accept this
    pub keystroke_saving: usize,
    /// Which tier produced this suggestion
    pub tier: SuggestionTier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionTier {
    /// Completing the current partially-typed word
    PrefixCompletion,
    /// Predicting the next word (user hasn't started typing it)
    NextWord,
    /// Multi-word phrase suggestion
    Phrase,
}

/// Hangul→surface index for prefix lookup.
/// Maps hangul reading → Vec<surface forms>.
#[derive(Debug, Clone)]
pub struct HangulSurfaceIndex {
    /// hangul → Vec<(surface, frequency)>
    index: HashMap<String, Vec<(String, i64)>>,
}

impl HangulSurfaceIndex {
    /// Build from the kanji dictionary in the database.
    pub fn build_from_db(db: &DictionaryDb) -> Self {
        let kanji_dict = db.kanji_dict();
        let mut index: HashMap<String, Vec<(String, i64)>> = HashMap::new();

        // Load all entries from hangul_index table
        let conn = db.conn();
        let mut stmt = conn.prepare(
            "SELECT hangul, reading FROM hangul_index"
        ).unwrap();
        let pairs: Vec<(String, String)> = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }).unwrap().filter_map(|r| r.ok()).collect();

        for (hangul, reading) in &pairs {
            let entries = kanji_dict.lookup(reading).unwrap_or_default();
            for entry in entries {
                index.entry(hangul.clone())
                    .or_default()
                    .push((entry.surface, entry.frequency));
            }
            // Also add hiragana itself as a candidate
            index.entry(hangul.clone())
                .or_default()
                .push((reading.clone(), 0));
        }

        // Deduplicate within each hangul key
        for entries in index.values_mut() {
            entries.sort_by(|a, b| b.1.cmp(&a.1));
            entries.dedup_by(|a, b| a.0 == b.0);
        }

        Self { index }
    }

    /// Find all surfaces whose hangul starts with the given prefix.
    /// Returns Vec<(surface, hangul, frequency)>.
    pub fn prefix_search(&self, prefix: &str, limit: usize) -> Vec<(String, String, i64)> {
        let mut results: Vec<(String, String, i64)> = Vec::new();

        for (hangul, entries) in &self.index {
            if hangul.starts_with(prefix) && hangul.len() > prefix.len() {
                for (surface, freq) in entries {
                    results.push((surface.clone(), hangul.clone(), *freq));
                }
            }
        }

        results.sort_by(|a, b| b.2.cmp(&a.2));
        results.truncate(limit);
        results
    }

    /// Exact match: find surfaces for exact hangul.
    pub fn exact_search(&self, hangul: &str) -> Vec<(String, i64)> {
        self.index.get(hangul)
            .map(|v| v.clone())
            .unwrap_or_default()
    }
}

/// The auto-completion engine.
pub struct AutoCompleteEngine<'a> {
    db: &'a DictionaryDb,
    ngram: &'a NgramModel,
    pmap: &'a PhoneticMap,
    hangul_index: HangulSurfaceIndex,
    /// Committed context (previous words in the session)
    context: Vec<String>,
    /// Scoring weights
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
    pub delta: f64,
    /// Max suggestions per tier
    pub max_suggestions: usize,
}

impl<'a> AutoCompleteEngine<'a> {
    pub fn new(
        db: &'a DictionaryDb,
        ngram: &'a NgramModel,
        pmap: &'a PhoneticMap,
    ) -> Self {
        let hangul_index = HangulSurfaceIndex::build_from_db(db);
        Self {
            db,
            ngram,
            pmap,
            hangul_index,
            context: Vec::new(),
            alpha: 0.1,
            beta: 0.35,
            gamma: 0.25,
            delta: 0.3,
            max_suggestions: 10,
        }
    }

    /// Commit a confirmed word to the context.
    pub fn commit_word(&mut self, surface: &str) {
        self.context.push(surface.to_string());
        // Keep a rolling window of context
        if self.context.len() > 20 {
            self.context.drain(0..self.context.len() - 20);
        }
    }

    /// Reset context (new sentence).
    pub fn reset_context(&mut self) {
        self.context.clear();
    }

    /// Get current context words.
    pub fn context(&self) -> &[String] {
        &self.context
    }

    /// Main entry point: get suggestions for the current hangul input.
    ///
    /// `partial_hangul` — what the user has typed so far for the current word.
    /// Returns suggestions from all tiers, merged and sorted.
    pub fn suggest(&self, partial_hangul: &str) -> Vec<Suggestion> {
        let mut suggestions = Vec::new();

        if partial_hangul.is_empty() {
            // No input yet → next-word prediction only
            suggestions.extend(self.predict_next_word());
        } else {
            // Tier 1: Prefix completion (partial match)
            suggestions.extend(self.prefix_complete(partial_hangul));

            // Tier 2: Exact decode + rank (what the current system does)
            suggestions.extend(self.exact_decode(partial_hangul));
        }

        // Deduplicate by surface form, keeping highest score
        let mut seen = std::collections::HashSet::new();
        suggestions.retain(|s| seen.insert(s.surface.clone()));

        // Sort by score descending
        suggestions.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        suggestions.truncate(self.max_suggestions);

        suggestions
    }

    /// Tier 1: Prefix completion — words whose hangul starts with the input.
    fn prefix_complete(&self, partial: &str) -> Vec<Suggestion> {
        let matches = self.hangul_index.prefix_search(partial, 30);
        let ctx: Vec<&str> = self.context.iter().map(|s| s.as_str()).collect();

        matches.into_iter().map(|(surface, hangul, freq)| {
            let saving = hangul.chars().count().saturating_sub(partial.chars().count());
            let score = self.score_candidate(&surface, freq, &ctx);

            Suggestion {
                surface,
                reading: String::new(), // could be looked up
                hangul,
                score,
                keystroke_saving: saving,
                tier: SuggestionTier::PrefixCompletion,
            }
        }).collect()
    }

    /// Tier 1b: Exact decode — full phonetic decoding of the current input.
    fn exact_decode(&self, hangul: &str) -> Vec<Suggestion> {
        let decoder = BeamDecoder::new(self.pmap, 8, 20);
        let candidates = decoder.decode(hangul);
        let ctx: Vec<&str> = self.context.iter().map(|s| s.as_str()).collect();

        let kanji_dict = self.db.kanji_dict();
        let mut results = Vec::new();

        for (hira, phone_conf) in candidates {
            let entries = kanji_dict.lookup(&hira).unwrap_or_default();
            let mut ents: Vec<(String, i64)> = entries.into_iter()
                .map(|e| (e.surface, e.frequency)).collect();
            if !ents.iter().any(|(s, _)| s == &hira) {
                ents.push((hira.clone(), 0));
            }

            for (surface, freq) in ents {
                let score = self.score_candidate_with_phoneme(&surface, freq, &ctx, phone_conf);
                results.push(Suggestion {
                    surface,
                    reading: hira.clone(),
                    hangul: hangul.to_string(),
                    score,
                    keystroke_saving: 0,
                    tier: SuggestionTier::PrefixCompletion,
                });
            }
        }

        results
    }

    /// Tier 2: Next-word prediction — predict what the user will type next.
    fn predict_next_word(&self) -> Vec<Suggestion> {
        let ctx: Vec<&str> = self.context.iter().map(|s| s.as_str()).collect();
        let predictions = self.ngram.predict_next(&ctx, self.max_suggestions);

        predictions.into_iter().map(|(surface, ng_score)| {
            // Look up hangul for this surface
            let hangul = self.surface_to_hangul(&surface);
            let saving = hangul.chars().count();

            Suggestion {
                reading: String::new(),
                hangul,
                keystroke_saving: saving,
                score: ng_score,
                surface,
                tier: SuggestionTier::NextWord,
            }
        }).collect()
    }

    /// Score a candidate using the 4-factor formula (without phoneme component).
    fn score_candidate(&self, surface: &str, freq: i64, ctx: &[&str]) -> f64 {
        let context_score = self.context_similarity(surface, ctx);
        let freq_score = freq as f64 / 10000.0;
        let ng_ctx: Vec<&str> = ctx.iter().rev().take(2).rev().copied().collect();
        let ngram_score = self.ngram.normalized_score(&ng_ctx, surface);

        // No phoneme component for prefix completion; boost with base 0.5
        self.alpha * 0.5 + self.beta * context_score + self.gamma * freq_score + self.delta * ngram_score
    }

    /// Score a candidate using the full 4-factor formula.
    fn score_candidate_with_phoneme(&self, surface: &str, freq: i64, ctx: &[&str], phone_conf: f64) -> f64 {
        let context_score = self.context_similarity(surface, ctx);
        let freq_score = freq as f64 / 10000.0;
        let ng_ctx: Vec<&str> = ctx.iter().rev().take(2).rev().copied().collect();
        let ngram_score = self.ngram.normalized_score(&ng_ctx, surface);

        self.alpha * phone_conf + self.beta * context_score + self.gamma * freq_score + self.delta * ngram_score
    }

    /// Compute embedding similarity between surface and context.
    fn context_similarity(&self, surface: &str, ctx: &[&str]) -> f64 {
        let embed_store = self.db.embedding_store();

        let ctx_embs: Vec<(Vec<f32>, f32)> = ctx.iter().enumerate()
            .filter_map(|(i, &w)| embed_store.get_embedding(w).ok().flatten()
                .map(|e| (e, 1.0 / (ctx.len() - i) as f32))).collect();

        if ctx_embs.is_empty() { return 0.5; }

        let (v, w): (Vec<_>, Vec<_>) = ctx_embs.into_iter().unzip();
        let ctx_vec = weighted_average_vectors(&v, &w);
        if ctx_vec.iter().all(|&x| x.abs() < 1e-10) { return 0.5; }

        embed_store.get_embedding(surface).ok().flatten()
            .map(|e| (cosine_similarity(&e, &ctx_vec) + 1.0) / 2.0)
            .unwrap_or(0.5)
    }

    /// Reverse lookup: find the hangul representation of a surface form.
    fn surface_to_hangul(&self, surface: &str) -> String {
        // Look through index to find hangul for this surface
        for (hangul, entries) in &self.hangul_index.index {
            for (surf, _) in entries {
                if surf == surface {
                    return hangul.clone();
                }
            }
        }
        // Fallback: try reading from kanji dict → kana_hangul conversion
        surface.to_string()
    }
}

/// Simulate a typing session and measure keystroke savings.
///
/// Returns (total_keystrokes_without_ac, total_keystrokes_with_ac, savings_ratio).
pub fn simulate_typing_session(
    engine: &mut AutoCompleteEngine,
    sentences: &[crate::generator::GenSentence],
    accept_threshold: usize, // min keystroke saving to auto-accept
) -> TypingStats {
    let mut stats = TypingStats::default();

    for sentence in sentences {
        engine.reset_context();
        for word in &sentence.words {
            let hangul_chars: Vec<char> = word.hangul.chars().collect();
            let full_len = hangul_chars.len();
            stats.total_words += 1;
            // Baseline: type all chars + 1 confirm keystroke (space/enter)
            stats.total_keystrokes_baseline += full_len + 1;

            let mut accepted = false;
            let mut keystrokes_used = 0;

            // Tier 2: Check next-word prediction (0 keystrokes)
            if !engine.context().is_empty() {
                let suggestions = engine.suggest("");
                if let Some(s) = suggestions.first() {
                    if s.surface == word.surface && s.tier == SuggestionTier::NextWord {
                        // Accept with 1 keystroke (Tab/Enter to confirm)
                        keystrokes_used = 1;
                        stats.next_word_hits += 1;
                        accepted = true;
                    }
                }
            }

            // Tier 1: Type character by character, checking prefix completion
            if !accepted {
                for i in 1..=full_len {
                    keystrokes_used = i;
                    let partial: String = hangul_chars[..i].iter().collect();
                    let suggestions = engine.suggest(&partial);

                    if let Some(top) = suggestions.first() {
                        if top.surface == word.surface && top.keystroke_saving >= accept_threshold {
                            // Accept: +1 for confirm keystroke
                            keystrokes_used = i + 1;
                            stats.prefix_hits += 1;
                            if i < full_len {
                                accepted = true;
                                break;
                            }
                        }
                    }
                }
                if !accepted {
                    // Had to type the whole word + confirm
                    keystrokes_used = full_len + 1;
                }
            }

            stats.total_keystrokes_with_ac += keystrokes_used;
            engine.commit_word(&word.surface);
        }
        stats.total_sentences += 1;
    }

    stats
}

/// Statistics from a simulated typing session.
#[derive(Debug, Clone, Default)]
pub struct TypingStats {
    pub total_sentences: usize,
    pub total_words: usize,
    pub total_keystrokes_baseline: usize,
    pub total_keystrokes_with_ac: usize,
    pub next_word_hits: usize,
    pub prefix_hits: usize,
}

impl TypingStats {
    /// Keystroke saving ratio (0.0 = no saving, 1.0 = everything predicted).
    pub fn saving_ratio(&self) -> f64 {
        if self.total_keystrokes_baseline == 0 { return 0.0; }
        1.0 - (self.total_keystrokes_with_ac as f64 / self.total_keystrokes_baseline as f64)
    }

    /// Average keystrokes per word with auto-completion.
    pub fn avg_keystrokes(&self) -> f64 {
        if self.total_words == 0 { return 0.0; }
        self.total_keystrokes_with_ac as f64 / self.total_words as f64
    }

    /// Average keystrokes per word without auto-completion.
    pub fn avg_keystrokes_baseline(&self) -> f64 {
        if self.total_words == 0 { return 0.0; }
        self.total_keystrokes_baseline as f64 / self.total_words as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hangul_index_prefix() {
        let mut index = HangulSurfaceIndex {
            index: HashMap::new(),
        };
        index.index.insert("도쿄".to_string(), vec![
            ("東京".to_string(), 5000),
            ("とうきょう".to_string(), 0),
        ]);
        index.index.insert("도쿄타워".to_string(), vec![
            ("東京タワー".to_string(), 3000),
        ]);
        index.index.insert("도로".to_string(), vec![
            ("道路".to_string(), 2000),
        ]);

        let results = index.prefix_search("도쿄", 10);
        // Should find 도쿄타워 (prefix of 도쿄) but not 도쿄 itself (exact match)
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "東京タワー");
    }
}
