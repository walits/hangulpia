//! Extended dictionary with kanji lookup and context-aware ranking.
//!
//! Supports hiragana→kanji conversion with frequency data,
//! and uses token embedding vectors for sentence-level disambiguation.

use rusqlite::{params, Connection, Result as SqlResult};

use crate::embedding::{
    cosine_similarity, weighted_average_vectors, EmbeddingStore,
};

/// A dictionary entry with reading, surface form, and frequency.
#[derive(Debug, Clone)]
pub struct DictEntry {
    pub reading: String,
    pub surface: String,
    pub frequency: i64,
}

/// A candidate ranked by both phoneme confidence and context similarity.
#[derive(Debug, Clone)]
pub struct RankedCandidate {
    /// The display form (kanji or hiragana).
    pub surface: String,
    /// The reading in hiragana.
    pub reading: String,
    /// Phoneme-level confidence from Hangul→romaji mapping (0.0–1.0).
    pub phoneme_score: f64,
    /// Context similarity score from embedding vectors (0.0–1.0).
    pub context_score: f64,
    /// Combined final score.
    pub final_score: f64,
}

/// Manages the kanji dictionary in SQLite.
pub struct KanjiDict<'a> {
    conn: &'a Connection,
}

impl<'a> KanjiDict<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Create the kanji dictionary tables.
    pub fn init_tables(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS kanji_dict (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                reading TEXT NOT NULL,
                surface TEXT NOT NULL,
                frequency INTEGER DEFAULT 0,
                UNIQUE(reading, surface)
            );
            CREATE INDEX IF NOT EXISTS idx_kanji_reading ON kanji_dict(reading);
            CREATE INDEX IF NOT EXISTS idx_kanji_freq ON kanji_dict(reading, frequency DESC);
            ",
        )?;
        Ok(())
    }

    /// Insert a dictionary entry.
    pub fn insert(&self, reading: &str, surface: &str, frequency: i64) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO kanji_dict (reading, surface, frequency) VALUES (?1, ?2, ?3)",
            params![reading, surface, frequency],
        )?;
        Ok(())
    }

    /// Batch insert dictionary entries.
    pub fn insert_batch(&self, entries: &[(&str, &str, i64)]) -> SqlResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO kanji_dict (reading, surface, frequency) VALUES (?1, ?2, ?3)",
            )?;
            for &(reading, surface, freq) in entries {
                stmt.execute(params![reading, surface, freq])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Look up kanji candidates for a hiragana reading, ordered by frequency.
    pub fn lookup(&self, reading: &str) -> SqlResult<Vec<DictEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT reading, surface, frequency FROM kanji_dict WHERE reading = ?1 ORDER BY frequency DESC",
        )?;
        let results = stmt
            .query_map(params![reading], |row| {
                Ok(DictEntry {
                    reading: row.get(0)?,
                    surface: row.get(1)?,
                    frequency: row.get(2)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(results)
    }

    /// Look up with prefix match (for incremental input).
    pub fn lookup_prefix(&self, prefix: &str) -> SqlResult<Vec<DictEntry>> {
        let pattern = format!("{}%", prefix);
        let mut stmt = self.conn.prepare(
            "SELECT reading, surface, frequency FROM kanji_dict WHERE reading LIKE ?1 ORDER BY frequency DESC LIMIT 20",
        )?;
        let results = stmt
            .query_map(params![pattern], |row| {
                Ok(DictEntry {
                    reading: row.get(0)?,
                    surface: row.get(1)?,
                    frequency: row.get(2)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(results)
    }
}

/// Context-aware candidate ranker using token embedding vectors.
///
/// Given a list of phoneme candidates and the surrounding sentence context,
/// this ranker uses embedding cosine similarity to disambiguate.
///
/// The scoring formula is:
///   final_score = α * phoneme_score + β * context_score + γ * freq_score
///
/// Where:
///   - phoneme_score: confidence from Hangul→romaji mapping
///   - context_score: cosine similarity between candidate embedding and context centroid
///   - freq_score: normalized word frequency from dictionary
pub struct ContextRanker<'a> {
    conn: &'a Connection,
    /// Weight for phoneme confidence.
    pub alpha: f64,
    /// Weight for context embedding similarity.
    pub beta: f64,
    /// Weight for word frequency.
    pub gamma: f64,
}

impl<'a> ContextRanker<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            alpha: 0.3,
            beta: 0.5,
            gamma: 0.2,
        }
    }

    /// Set custom weights (must sum to ~1.0 for interpretability).
    pub fn with_weights(mut self, alpha: f64, beta: f64, gamma: f64) -> Self {
        self.alpha = alpha;
        self.beta = beta;
        self.gamma = gamma;
        self
    }

    /// Rank candidates using context embedding vectors.
    ///
    /// # Arguments
    /// * `candidates` - List of (surface, reading, phoneme_confidence)
    /// * `context_words` - Surrounding words in the sentence (already committed)
    /// * `max_freq` - Maximum frequency in dictionary (for normalization)
    ///
    /// # Returns
    /// Ranked candidates sorted by final_score descending.
    pub fn rank_candidates(
        &self,
        candidates: &[(String, String, f64)],
        context_words: &[&str],
        max_freq: i64,
    ) -> Vec<RankedCandidate> {
        let embed_store = EmbeddingStore::new(self.conn);

        // Build context vector: weighted average of context word embeddings.
        // Words closer to the current position get higher weight.
        let context_embeddings: Vec<(Vec<f32>, f32)> = context_words
            .iter()
            .enumerate()
            .filter_map(|(i, &word)| {
                embed_store.get_embedding(word).ok().flatten().map(|emb| {
                    // Distance-based weight: closer words get higher weight.
                    // Last word in context is closest → highest weight.
                    let distance = (context_words.len() - i) as f32;
                    let weight = 1.0 / distance;
                    (emb, weight)
                })
            })
            .collect();

        let context_vector = if context_embeddings.is_empty() {
            None
        } else {
            let (vecs, weights): (Vec<Vec<f32>>, Vec<f32>) =
                context_embeddings.into_iter().unzip();
            let avg = weighted_average_vectors(&vecs, &weights);
            if avg.iter().all(|&v| v.abs() < 1e-10) {
                None
            } else {
                Some(avg)
            }
        };

        let kanji_dict = KanjiDict::new(self.conn);
        let max_freq_f = if max_freq > 0 {
            max_freq as f64
        } else {
            1.0
        };

        let mut ranked: Vec<RankedCandidate> = candidates
            .iter()
            .flat_map(|(hiragana, _romaji, phoneme_conf)| {
                // Look up kanji candidates for this hiragana reading.
                let kanji_entries = kanji_dict
                    .lookup(hiragana)
                    .unwrap_or_default();

                // Always include the hiragana itself as a candidate.
                let mut entries: Vec<(String, i64)> = kanji_entries
                    .into_iter()
                    .map(|e| (e.surface, e.frequency))
                    .collect();

                // Add hiragana form with lower frequency.
                if !entries.iter().any(|(s, _)| s == hiragana) {
                    entries.push((hiragana.clone(), 0));
                }

                entries
                    .into_iter()
                    .map(|(surface, freq)| {
                        let context_score = context_vector
                            .as_ref()
                            .and_then(|ctx| {
                                embed_store
                                    .get_embedding(&surface)
                                    .ok()
                                    .flatten()
                                    .map(|emb| {
                                        // Map cosine similarity from [-1,1] to [0,1].
                                        (cosine_similarity(&emb, ctx) + 1.0) / 2.0
                                    })
                            })
                            .unwrap_or(0.5); // neutral score if no embedding

                        let freq_score = freq as f64 / max_freq_f;

                        let final_score = self.alpha * phoneme_conf
                            + self.beta * context_score
                            + self.gamma * freq_score;

                        RankedCandidate {
                            surface,
                            reading: hiragana.clone(),
                            phoneme_score: *phoneme_conf,
                            context_score,
                            final_score,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        // Sort by final score descending.
        ranked.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Deduplicate by surface form (keep highest scored).
        ranked.dedup_by(|a, b| a.surface == b.surface);

        ranked
    }

    /// Simplified ranking without kanji lookup — just re-ranks hiragana candidates
    /// using context similarity.
    pub fn rank_hiragana_candidates(
        &self,
        candidates: &[(String, String, f64)],
        context_words: &[&str],
    ) -> Vec<RankedCandidate> {
        let embed_store = EmbeddingStore::new(self.conn);

        // Build context vector.
        let context_embeddings: Vec<(Vec<f32>, f32)> = context_words
            .iter()
            .enumerate()
            .filter_map(|(i, &word)| {
                embed_store.get_embedding(word).ok().flatten().map(|emb| {
                    let distance = (context_words.len() - i) as f32;
                    let weight = 1.0 / distance;
                    (emb, weight)
                })
            })
            .collect();

        let context_vector = if context_embeddings.is_empty() {
            None
        } else {
            let (vecs, weights): (Vec<Vec<f32>>, Vec<f32>) =
                context_embeddings.into_iter().unzip();
            let avg = weighted_average_vectors(&vecs, &weights);
            if avg.iter().all(|&v| v.abs() < 1e-10) {
                None
            } else {
                Some(avg)
            }
        };

        let mut ranked: Vec<RankedCandidate> = candidates
            .iter()
            .map(|(hiragana, _romaji, phoneme_conf)| {
                let context_score = context_vector
                    .as_ref()
                    .and_then(|ctx| {
                        embed_store
                            .get_embedding(hiragana)
                            .ok()
                            .flatten()
                            .map(|emb| (cosine_similarity(&emb, ctx) + 1.0) / 2.0)
                    })
                    .unwrap_or(0.5);

                // Without kanji, use higher weight for phoneme confidence.
                let final_score = 0.5 * phoneme_conf + 0.5 * context_score;

                RankedCandidate {
                    surface: hiragana.clone(),
                    reading: hiragana.clone(),
                    phoneme_score: *phoneme_conf,
                    context_score,
                    final_score,
                }
            })
            .collect();

        ranked.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        ranked.dedup_by(|a, b| a.surface == b.surface);

        ranked
    }
}

/// Directional context ranker: uses forward/backward embedding vectors.
///
/// For a candidate word at position k in the sentence:
///   - Words BEFORE k (left context): their fwd_vec ("F:word") predicts "what comes next"
///   - The candidate's bwd_vec ("B:word") says "what should be before me"
///   - Score = cos(candidate.bwd_vec, weighted_avg(left_context.fwd_vec))
///
/// This captures word ORDER, not just co-occurrence.
pub struct DirectionalContextRanker<'a> {
    conn: &'a Connection,
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
}

impl<'a> DirectionalContextRanker<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            alpha: 0.3,
            beta: 0.5,
            gamma: 0.2,
        }
    }

    /// Rank candidates using directional embedding vectors.
    ///
    /// `left_context` = words already confirmed BEFORE this position
    /// `right_context` = words already confirmed AFTER this position (if any)
    pub fn rank_candidates(
        &self,
        candidates: &[(String, String, f64)],
        left_context: &[&str],
        right_context: &[&str],
        max_freq: i64,
    ) -> Vec<RankedCandidate> {
        let embed_store = EmbeddingStore::new(self.conn);

        // Left context → use their fwd_vec (they predict what comes to their right)
        let left_ctx_vec = Self::build_context_vector(&embed_store, left_context, "F:");

        // Right context → use their bwd_vec (they predict what was to their left)
        let right_ctx_vec = Self::build_context_vector(&embed_store, right_context, "B:");

        let kanji_dict = KanjiDict::new(self.conn);
        let max_freq_f = if max_freq > 0 { max_freq as f64 } else { 1.0 };

        let mut ranked: Vec<RankedCandidate> = candidates
            .iter()
            .flat_map(|(hiragana, _romaji, phoneme_conf)| {
                let kanji_entries = kanji_dict.lookup(hiragana).unwrap_or_default();
                let mut entries: Vec<(String, i64)> = kanji_entries
                    .into_iter()
                    .map(|e| (e.surface, e.frequency))
                    .collect();
                if !entries.iter().any(|(s, _)| s == hiragana) {
                    entries.push((hiragana.clone(), 0));
                }

                entries
                    .into_iter()
                    .map(|(surface, freq)| {
                        // Candidate's bwd_vec vs left_context's fwd_vec
                        let left_score = left_ctx_vec.as_ref().and_then(|ctx| {
                            let bwd_key = format!("B:{}", surface);
                            embed_store.get_embedding(&bwd_key).ok().flatten()
                                .map(|emb| (cosine_similarity(&emb, ctx) + 1.0) / 2.0)
                        }).unwrap_or(0.5);

                        // Candidate's fwd_vec vs right_context's bwd_vec
                        let right_score = right_ctx_vec.as_ref().and_then(|ctx| {
                            let fwd_key = format!("F:{}", surface);
                            embed_store.get_embedding(&fwd_key).ok().flatten()
                                .map(|emb| (cosine_similarity(&emb, ctx) + 1.0) / 2.0)
                        }).unwrap_or(0.5);

                        let has_left = left_ctx_vec.is_some();
                        let has_right = right_ctx_vec.is_some();
                        let context_score = match (has_left, has_right) {
                            (true, true) => left_score * 0.6 + right_score * 0.4,
                            (true, false) => left_score,
                            (false, true) => right_score,
                            (false, false) => 0.5,
                        };

                        let freq_score = freq as f64 / max_freq_f;
                        let final_score = self.alpha * phoneme_conf
                            + self.beta * context_score
                            + self.gamma * freq_score;

                        RankedCandidate {
                            surface,
                            reading: hiragana.clone(),
                            phoneme_score: *phoneme_conf,
                            context_score,
                            final_score,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        ranked.sort_by(|a, b| {
            b.final_score.partial_cmp(&a.final_score).unwrap_or(std::cmp::Ordering::Equal)
        });
        ranked.dedup_by(|a, b| a.surface == b.surface);
        ranked
    }

    fn build_context_vector(
        embed_store: &EmbeddingStore,
        words: &[&str],
        prefix: &str,
    ) -> Option<Vec<f32>> {
        let embeddings: Vec<(Vec<f32>, f32)> = words
            .iter()
            .enumerate()
            .filter_map(|(i, &word)| {
                let key = format!("{}{}", prefix, word);
                embed_store.get_embedding(&key).ok().flatten().map(|emb| {
                    let distance = (words.len() - i) as f32;
                    let weight = 1.0 / distance;
                    (emb, weight)
                })
            })
            .collect();

        if embeddings.is_empty() {
            return None;
        }

        let (vecs, weights): (Vec<Vec<f32>>, Vec<f32>) = embeddings.into_iter().unzip();
        let avg = weighted_average_vectors(&vecs, &weights);
        if avg.iter().all(|&v| v.abs() < 1e-10) { None } else { Some(avg) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::EmbeddingStore;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        let embed = EmbeddingStore::new(&conn);
        embed.init_tables().unwrap();
        let dict = KanjiDict::new(&conn);
        dict.init_tables().unwrap();
        conn
    }

    #[test]
    fn test_kanji_dict_insert_and_lookup() {
        let conn = setup_db();
        let dict = KanjiDict::new(&conn);

        dict.insert("さくら", "桜", 5000).unwrap();
        dict.insert("さくら", "櫻", 100).unwrap();

        let results = dict.lookup("さくら").unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].surface, "桜"); // higher freq first
        assert_eq!(results[1].surface, "櫻");
    }

    #[test]
    fn test_kanji_dict_prefix() {
        let conn = setup_db();
        let dict = KanjiDict::new(&conn);

        dict.insert("さくら", "桜", 5000).unwrap();
        dict.insert("さくらんぼ", "桜桃", 200).unwrap();
        dict.insert("さけ", "酒", 3000).unwrap();

        let results = dict.lookup_prefix("さく").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_context_ranking_with_embeddings() {
        let conn = setup_db();
        let embed = EmbeddingStore::new(&conn);
        let dict = KanjiDict::new(&conn);

        // Set up dictionary entries.
        dict.insert("はな", "花", 5000).unwrap();
        dict.insert("はな", "鼻", 3000).unwrap();

        // Set up embeddings:
        // "桜" is close to "花" (flower) but far from "鼻" (nose).
        // Context: user already typed "桜" (cherry blossom).
        embed
            .store_embedding("桜", &[0.9, 0.1, 0.0, 0.0])
            .unwrap();
        embed
            .store_embedding("花", &[0.8, 0.2, 0.0, 0.0])
            .unwrap();
        embed
            .store_embedding("鼻", &[0.0, 0.0, 0.8, 0.2])
            .unwrap();

        let ranker = ContextRanker::new(&conn);

        let candidates = vec![("はな".to_string(), "hana".to_string(), 0.9)];
        let context = vec!["桜"];

        let ranked = ranker.rank_candidates(&candidates, &context, 5000);

        // "花" should rank higher than "鼻" because "桜" context is closer to "花".
        assert!(ranked.len() >= 2);
        let flower_idx = ranked.iter().position(|r| r.surface == "花").unwrap();
        let nose_idx = ranked.iter().position(|r| r.surface == "鼻").unwrap();
        assert!(
            flower_idx < nose_idx,
            "花 should rank before 鼻 with 桜 context"
        );
    }

    #[test]
    fn test_context_ranking_no_context() {
        let conn = setup_db();
        let dict = KanjiDict::new(&conn);

        dict.insert("はな", "花", 5000).unwrap();
        dict.insert("はな", "鼻", 3000).unwrap();

        let ranker = ContextRanker::new(&conn);
        let candidates = vec![("はな".to_string(), "hana".to_string(), 0.9)];

        // No context → falls back to frequency-based ranking.
        let ranked = ranker.rank_candidates(&candidates, &[], 5000);
        assert!(ranked.len() >= 2);
        // With no context, 花 should still rank first due to higher frequency.
        assert_eq!(ranked[0].surface, "花");
    }

    #[test]
    fn test_context_ranking_changes_result_with_different_context() {
        let conn = setup_db();
        let embed = EmbeddingStore::new(&conn);
        let dict = KanjiDict::new(&conn);

        dict.insert("はな", "花", 5000).unwrap();
        dict.insert("はな", "鼻", 3000).unwrap();

        // "花見" (hanami/flower viewing) is semantically close to "花" (flower)
        // "風邪" (kaze/cold) is semantically close to "鼻" (nose)
        embed
            .store_embedding("花見", &[0.9, 0.1, 0.0, 0.0])
            .unwrap();
        embed
            .store_embedding("風邪", &[0.0, 0.0, 0.9, 0.1])
            .unwrap();
        embed
            .store_embedding("花", &[0.8, 0.2, 0.0, 0.0])
            .unwrap();
        embed
            .store_embedding("鼻", &[0.0, 0.0, 0.8, 0.2])
            .unwrap();

        let ranker = ContextRanker::new(&conn);
        let candidates = vec![("はな".to_string(), "hana".to_string(), 0.9)];

        // Context: "花見" → 花 should rank first.
        let ranked_flower = ranker.rank_candidates(&candidates, &["花見"], 5000);
        assert_eq!(ranked_flower[0].surface, "花");

        // Context: "風邪" → 鼻 should rank first.
        let ranked_nose = ranker.rank_candidates(&candidates, &["風邪"], 5000);
        assert_eq!(ranked_nose[0].surface, "鼻");
    }
}
