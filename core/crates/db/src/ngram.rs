//! N-gram language model for word sequence prediction.
//!
//! Provides bigram and trigram probabilities to score candidate sequences.
//! Uses simple add-k smoothing with backoff:
//!
//!   P(w3 | w1, w2) = λ_tri * P_tri(w3|w1,w2) + λ_bi * P_bi(w3|w2) + λ_uni * P_uni(w3)
//!
//! The model operates on surface forms (漢字), so it captures which kanji
//! sequences are natural in Japanese.

use std::collections::HashMap;
use rusqlite::Connection;

/// Bigram/trigram language model with backoff smoothing.
#[derive(Debug, Clone)]
pub struct NgramModel {
    /// Unigram counts: surface → count
    pub unigram: HashMap<String, u64>,
    /// Bigram counts: (prev, current) → count
    pub bigram: HashMap<(String, String), u64>,
    /// Trigram counts: (w1, w2, w3) → count
    pub trigram: HashMap<(String, String, String), u64>,
    /// Total unigram count (sum of all unigram counts)
    pub total_unigrams: u64,
    /// Smoothing parameter (add-k)
    pub smoothing_k: f64,
    /// Interpolation weights: (trigram, bigram, unigram)
    pub lambda: (f64, f64, f64),
}

impl NgramModel {
    pub fn new() -> Self {
        NgramModel {
            unigram: HashMap::new(),
            bigram: HashMap::new(),
            trigram: HashMap::new(),
            total_unigrams: 0,
            smoothing_k: 0.1,
            lambda: (0.4, 0.4, 0.2), // default interpolation weights
        }
    }

    /// Add counts from a sequence of surface forms (one sentence).
    pub fn add_sentence(&mut self, surfaces: &[&str]) {
        // Add BOS (beginning of sentence) marker
        let mut words: Vec<&str> = vec!["<BOS>"];
        words.extend_from_slice(surfaces);

        // Unigrams
        for &w in &words {
            *self.unigram.entry(w.to_string()).or_insert(0) += 1;
            self.total_unigrams += 1;
        }

        // Bigrams
        for window in words.windows(2) {
            let key = (window[0].to_string(), window[1].to_string());
            *self.bigram.entry(key).or_insert(0) += 1;
        }

        // Trigrams
        if words.len() >= 3 {
            for window in words.windows(3) {
                let key = (
                    window[0].to_string(),
                    window[1].to_string(),
                    window[2].to_string(),
                );
                *self.trigram.entry(key).or_insert(0) += 1;
            }
        }
    }

    /// Build from generated corpus.
    pub fn build_from_generated(&mut self, corpus: &[crate::generator::GenSentence]) {
        for sentence in corpus {
            let surfaces: Vec<&str> = sentence.words.iter()
                .map(|w| w.surface.as_str())
                .collect();
            self.add_sentence(&surfaces);
        }
    }

    /// Smoothed unigram probability: P(w)
    pub fn p_unigram(&self, word: &str) -> f64 {
        let count = self.unigram.get(word).copied().unwrap_or(0) as f64;
        let vocab_size = self.unigram.len() as f64;
        (count + self.smoothing_k) / (self.total_unigrams as f64 + self.smoothing_k * vocab_size)
    }

    /// Smoothed bigram probability: P(w | prev)
    pub fn p_bigram(&self, prev: &str, word: &str) -> f64 {
        let bigram_count = self.bigram
            .get(&(prev.to_string(), word.to_string()))
            .copied()
            .unwrap_or(0) as f64;
        let prev_count = self.unigram.get(prev).copied().unwrap_or(0) as f64;
        let vocab_size = self.unigram.len() as f64;

        if prev_count == 0.0 {
            return self.p_unigram(word);
        }
        (bigram_count + self.smoothing_k) / (prev_count + self.smoothing_k * vocab_size)
    }

    /// Smoothed trigram probability: P(w | w1, w2)
    pub fn p_trigram(&self, w1: &str, w2: &str, word: &str) -> f64 {
        let tri_count = self.trigram
            .get(&(w1.to_string(), w2.to_string(), word.to_string()))
            .copied()
            .unwrap_or(0) as f64;
        let bi_count = self.bigram
            .get(&(w1.to_string(), w2.to_string()))
            .copied()
            .unwrap_or(0) as f64;
        let vocab_size = self.unigram.len() as f64;

        if bi_count == 0.0 {
            return self.p_bigram(w2, word);
        }
        (tri_count + self.smoothing_k) / (bi_count + self.smoothing_k * vocab_size)
    }

    /// Interpolated probability with backoff.
    /// context = previous words (last 1-2 words used).
    ///
    /// Returns a score in [0, 1] range suitable for combining with other scores.
    pub fn score(&self, context: &[&str], word: &str) -> f64 {
        let (lam_tri, lam_bi, lam_uni) = self.lambda;

        match context.len() {
            0 => self.p_unigram(word),
            1 => {
                let p_bi = self.p_bigram(context[0], word);
                let p_uni = self.p_unigram(word);
                (lam_bi + lam_tri) * p_bi + lam_uni * p_uni
            }
            _ => {
                let n = context.len();
                let w1 = context[n - 2];
                let w2 = context[n - 1];
                let p_tri = self.p_trigram(w1, w2, word);
                let p_bi = self.p_bigram(w2, word);
                let p_uni = self.p_unigram(word);
                lam_tri * p_tri + lam_bi * p_bi + lam_uni * p_uni
            }
        }
    }

    /// Normalized score: maps raw probability to [0, 1] scale.
    ///
    /// Uses log-scale normalization since raw probabilities are tiny.
    /// The score is: clamp((log(p) + log_floor) / log_floor, 0, 1)
    /// where log_floor typically = 15 (so p=exp(-15)≈3e-7 maps to 0).
    pub fn normalized_score(&self, context: &[&str], word: &str) -> f64 {
        let p = self.score(context, word);
        if p <= 0.0 {
            return 0.0;
        }
        let log_p = p.ln();
        let log_floor = 15.0; // probabilities below exp(-15) → 0
        let normalized = (log_p + log_floor) / log_floor;
        normalized.clamp(0.0, 1.0)
    }

    /// Predict top-K next words given context (previous words).
    /// Returns Vec<(word, score)> sorted by score descending.
    pub fn predict_next(&self, context: &[&str], top_k: usize) -> Vec<(String, f64)> {
        let mut scored: Vec<(String, f64)> = Vec::new();

        match context.len() {
            0 => {
                // No context: return most frequent unigrams
                for (word, _) in &self.unigram {
                    if word == "<BOS>" { continue; }
                    scored.push((word.clone(), self.p_unigram(word)));
                }
            }
            1 => {
                // Bigram context: find all words that follow context[0]
                let prev = context[0];
                // First collect bigram continuations
                let mut seen = std::collections::HashSet::new();
                for ((p, w), _) in &self.bigram {
                    if p == prev && w != "<BOS>" {
                        let s = self.score(context, w);
                        scored.push((w.clone(), s));
                        seen.insert(w.clone());
                    }
                }
                // Add high-frequency unigrams not yet seen (backoff)
                if scored.len() < top_k * 2 {
                    let mut uni_sorted: Vec<_> = self.unigram.iter()
                        .filter(|(w, _)| !seen.contains(w.as_str()) && w.as_str() != "<BOS>")
                        .collect();
                    uni_sorted.sort_by(|a, b| b.1.cmp(a.1));
                    for (w, _) in uni_sorted.into_iter().take(top_k) {
                        scored.push((w.clone(), self.score(context, w)));
                    }
                }
            }
            _ => {
                // Trigram context: find all words that follow (w1, w2)
                let n = context.len();
                let (w1, w2) = (context[n - 2], context[n - 1]);
                let mut seen = std::collections::HashSet::new();
                // Trigram continuations
                for ((t1, t2, w), _) in &self.trigram {
                    if t1 == w1 && t2 == w2 && w != "<BOS>" {
                        let s = self.score(context, w);
                        scored.push((w.clone(), s));
                        seen.insert(w.clone());
                    }
                }
                // Bigram backoff
                for ((p, w), _) in &self.bigram {
                    if p == w2 && !seen.contains(w.as_str()) && w != "<BOS>" {
                        let s = self.score(context, w);
                        scored.push((w.clone(), s));
                        seen.insert(w.clone());
                    }
                }
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }

    /// Predict next words filtered by a hangul prefix.
    /// Only returns words whose hangul reading starts with the given prefix.
    pub fn predict_next_with_prefix(
        &self,
        context: &[&str],
        hangul_prefix: &str,
        hangul_index: &std::collections::HashMap<String, Vec<String>>,
        top_k: usize,
    ) -> Vec<(String, String, f64)> {
        // Gather candidate surfaces that match the hangul prefix
        let mut candidates: Vec<(String, String)> = Vec::new(); // (surface, hangul)
        for (hangul, surfaces) in hangul_index {
            if hangul.starts_with(hangul_prefix) {
                for surf in surfaces {
                    candidates.push((surf.clone(), hangul.clone()));
                }
            }
        }

        // Score each candidate
        let mut scored: Vec<(String, String, f64)> = candidates.into_iter()
            .map(|(surf, hangul)| {
                let s = self.score(context, &surf);
                (surf, hangul, s)
            })
            .collect();

        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }

    /// Stats string for debugging.
    pub fn stats(&self) -> String {
        format!(
            "NgramModel: {} unigrams, {} bigrams, {} trigrams, total={}",
            self.unigram.len(),
            self.bigram.len(),
            self.trigram.len(),
            self.total_unigrams,
        )
    }
}

/// Store n-gram model in SQLite for production use.
pub struct NgramStore<'a> {
    conn: &'a Connection,
}

impl<'a> NgramStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn init_tables(&self) -> Result<(), rusqlite::Error> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ngram_unigram (
                word TEXT PRIMARY KEY,
                count INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ngram_bigram (
                prev TEXT NOT NULL,
                word TEXT NOT NULL,
                count INTEGER NOT NULL,
                PRIMARY KEY (prev, word)
            );
            CREATE INDEX IF NOT EXISTS idx_bigram_prev ON ngram_bigram(prev);
            CREATE TABLE IF NOT EXISTS ngram_trigram (
                w1 TEXT NOT NULL,
                w2 TEXT NOT NULL,
                word TEXT NOT NULL,
                count INTEGER NOT NULL,
                PRIMARY KEY (w1, w2, word)
            );
            CREATE INDEX IF NOT EXISTS idx_trigram_w1w2 ON ngram_trigram(w1, w2);
            CREATE TABLE IF NOT EXISTS ngram_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    /// Save an in-memory NgramModel to SQLite.
    pub fn save(&self, model: &NgramModel) -> Result<(), rusqlite::Error> {
        self.init_tables()?;

        // Unigrams
        {
            let tx = self.conn.unchecked_transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR REPLACE INTO ngram_unigram (word, count) VALUES (?1, ?2)",
                )?;
                for (word, count) in &model.unigram {
                    stmt.execute(rusqlite::params![word, count])?;
                }
            }
            tx.commit()?;
        }

        // Bigrams
        {
            let tx = self.conn.unchecked_transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR REPLACE INTO ngram_bigram (prev, word, count) VALUES (?1, ?2, ?3)",
                )?;
                for ((prev, word), count) in &model.bigram {
                    stmt.execute(rusqlite::params![prev, word, count])?;
                }
            }
            tx.commit()?;
        }

        // Trigrams
        {
            let tx = self.conn.unchecked_transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR REPLACE INTO ngram_trigram (w1, w2, word, count) VALUES (?1, ?2, ?3, ?4)",
                )?;
                for ((w1, w2, word), count) in &model.trigram {
                    stmt.execute(rusqlite::params![w1, w2, word, count])?;
                }
            }
            tx.commit()?;
        }

        // Meta
        self.conn.execute(
            "INSERT OR REPLACE INTO ngram_meta (key, value) VALUES ('total_unigrams', ?1)",
            rusqlite::params![model.total_unigrams.to_string()],
        )?;
        self.conn.execute(
            "INSERT OR REPLACE INTO ngram_meta (key, value) VALUES ('vocab_size', ?1)",
            rusqlite::params![model.unigram.len().to_string()],
        )?;

        Ok(())
    }

    /// Load an NgramModel from SQLite.
    pub fn load(&self) -> Result<NgramModel, rusqlite::Error> {
        let mut model = NgramModel::new();

        // Unigrams
        {
            let mut stmt = self.conn.prepare("SELECT word, count FROM ngram_unigram")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })?;
            for row in rows {
                let (word, count) = row?;
                model.unigram.insert(word, count);
            }
        }

        // Bigrams
        {
            let mut stmt = self.conn.prepare("SELECT prev, word, count FROM ngram_bigram")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                ))
            })?;
            for row in rows {
                let (prev, word, count) = row?;
                model.bigram.insert((prev, word), count);
            }
        }

        // Trigrams
        {
            let mut stmt = self.conn.prepare("SELECT w1, w2, word, count FROM ngram_trigram")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            })?;
            for row in rows {
                let (w1, w2, word, count) = row?;
                model.trigram.insert((w1, w2, word), count);
            }
        }

        // Meta
        let total: String = self.conn.query_row(
            "SELECT value FROM ngram_meta WHERE key='total_unigrams'",
            [],
            |row| row.get(0),
        ).unwrap_or_else(|_| "0".to_string());
        model.total_unigrams = total.parse().unwrap_or(0);

        Ok(model)
    }
}

/// Build an NgramModel from generated corpus using chunked processing.
pub fn build_ngram_model_chunked(
    vocab: &[crate::vocab::VocabEntry],
    sentence_count: usize,
) -> NgramModel {
    use crate::generator::generate_corpus_chunked;

    let mut model = NgramModel::new();

    generate_corpus_chunked(vocab, sentence_count, 1_000_000, |chunk| {
        model.build_from_generated(chunk);
    });

    model
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unigram() {
        let mut model = NgramModel::new();
        model.add_sentence(&["東京", "は", "大きい"]);
        model.add_sentence(&["東京", "の", "天気"]);

        // 東京 appears twice, others once
        assert_eq!(model.unigram["東京"], 2);
        assert_eq!(model.unigram["天気"], 1);

        // Unigram probabilities should be valid
        let p = model.p_unigram("東京");
        assert!(p > 0.0 && p < 1.0);
    }

    #[test]
    fn test_bigram() {
        let mut model = NgramModel::new();
        model.add_sentence(&["東京", "は", "大きい"]);
        model.add_sentence(&["東京", "の", "天気"]);

        // P(は | 東京) should be around 0.5 (1 out of 2 times after 東京)
        let p_ha = model.p_bigram("東京", "は");
        let p_no = model.p_bigram("東京", "の");
        // Both should be similar due to equal counts
        assert!((p_ha - p_no).abs() < 0.01);
    }

    #[test]
    fn test_score_with_context() {
        let mut model = NgramModel::new();
        for _ in 0..100 {
            model.add_sentence(&["今日", "の", "天気"]);
        }
        model.add_sentence(&["今日", "の", "料理"]);

        // P(天気 | 今日, の) should be much higher than P(料理 | 今日, の)
        let s_tenki = model.score(&["今日", "の"], "天気");
        let s_ryouri = model.score(&["今日", "の"], "料理");
        assert!(s_tenki > s_ryouri * 5.0);
    }

    #[test]
    fn test_normalized_score() {
        let mut model = NgramModel::new();
        for _ in 0..100 {
            model.add_sentence(&["東京", "は", "大きい"]);
        }

        let score = model.normalized_score(&["東京"], "は");
        assert!(score > 0.0 && score <= 1.0);
    }
}
