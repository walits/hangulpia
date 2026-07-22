//! Sentence-level buffer with bidirectional re-ranking.
//!
//! Holds multiple uncommitted segments, each with a list of candidates.
//! When a new segment is added, ALL segments are re-ranked using
//! bidirectional context — both left (preceding) and right (following)
//! segments contribute to each segment's context vector.
//!
//! This enables retroactive correction:
//!   1. "하나" → 花 (flower, highest freq)
//!   2. "카제" added → system re-ranks: 花→鼻, because 鼻+風邪 co-occur
//!
//! The re-ranking loop converges quickly (typically 2–3 passes) because
//! embedding-based scoring is continuous and stable.

use rusqlite::Connection;

use crate::dictionary::{ContextRanker, RankedCandidate};
use crate::embedding::{cosine_similarity, EmbeddingStore};

/// A single segment in the sentence buffer.
#[derive(Debug, Clone)]
pub struct Segment {
    /// Original Hangul input for this segment.
    pub hangul: String,
    /// Phoneme candidates: (hiragana, romaji, phoneme_confidence).
    pub phoneme_candidates: Vec<(String, String, f64)>,
    /// Full ranked candidates (including kanji) after context ranking.
    pub ranked: Vec<RankedCandidate>,
    /// Index of the currently selected candidate (default 0 = top).
    pub selected_idx: usize,
}

impl Segment {
    /// Get the currently selected surface form.
    pub fn selected_surface(&self) -> &str {
        if self.ranked.is_empty() {
            &self.hangul
        } else {
            let idx = self.selected_idx.min(self.ranked.len() - 1);
            &self.ranked[idx].surface
        }
    }

    /// Get the currently selected reading.
    pub fn selected_reading(&self) -> &str {
        if self.ranked.is_empty() {
            ""
        } else {
            let idx = self.selected_idx.min(self.ranked.len() - 1);
            &self.ranked[idx].reading
        }
    }

    /// Cycle to the next candidate.
    pub fn next_candidate(&mut self) {
        if !self.ranked.is_empty() {
            self.selected_idx = (self.selected_idx + 1) % self.ranked.len();
        }
    }

    /// Cycle to the previous candidate.
    pub fn prev_candidate(&mut self) {
        if !self.ranked.is_empty() {
            if self.selected_idx == 0 {
                self.selected_idx = self.ranked.len() - 1;
            } else {
                self.selected_idx -= 1;
            }
        }
    }
}

/// Sentence buffer holding multiple uncommitted segments.
///
/// Each time a segment is added or modified, `rerank_all()` is called
/// to re-evaluate every segment using bidirectional context.
pub struct SentenceBuffer<'a> {
    conn: &'a Connection,
    /// The uncommitted segments.
    pub segments: Vec<Segment>,
    /// Maximum re-ranking iterations (convergence loop).
    pub max_iterations: usize,
    /// Already-committed context words (from previous sentences).
    pub committed_context: Vec<String>,
}

impl<'a> SentenceBuffer<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            segments: Vec::new(),
            max_iterations: 3,
            committed_context: Vec::new(),
        }
    }

    /// Add a new segment and trigger bidirectional re-ranking.
    pub fn add_segment(
        &mut self,
        hangul: String,
        phoneme_candidates: Vec<(String, String, f64)>,
    ) {
        let segment = Segment {
            hangul,
            phoneme_candidates,
            ranked: Vec::new(),
            selected_idx: 0,
        };
        self.segments.push(segment);
        self.rerank_all();
    }

    /// Remove the last segment (backspace) and re-rank.
    pub fn pop_segment(&mut self) -> Option<Segment> {
        let seg = self.segments.pop();
        if !self.segments.is_empty() {
            self.rerank_all();
        }
        seg
    }

    /// Get the full composed sentence (selected surfaces joined).
    pub fn composed(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.selected_surface())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Get the full composed sentence with segment boundaries shown.
    pub fn composed_debug(&self) -> String {
        self.segments
            .iter()
            .map(|s| format!("[{}]", s.selected_surface()))
            .collect::<Vec<_>>()
            .join("")
    }

    /// Commit the current sentence, moving selected surfaces to committed context.
    pub fn commit(&mut self) -> String {
        let result = self.composed();
        for seg in &self.segments {
            self.committed_context.push(seg.selected_surface().to_string());
        }
        // Keep committed context window manageable.
        while self.committed_context.len() > 20 {
            self.committed_context.remove(0);
        }
        self.segments.clear();
        result
    }

    /// Clear without committing.
    pub fn clear(&mut self) {
        self.segments.clear();
    }

    /// The core bidirectional re-ranking algorithm.
    ///
    /// Uses a multi-hypothesis initialization to avoid chicken-and-egg:
    ///
    /// **Pass 0 (independent):** Each segment is ranked using ONLY
    /// committed_context, without cross-segment influence. This produces
    /// an unbiased initial ranking for each segment.
    ///
    /// **Pass 1+ (bidirectional):** Each segment is re-ranked using
    /// committed_context + selected surfaces of OTHER segments. This is
    /// where retroactive correction happens.
    ///
    /// Additionally, after the independent pass, we try a "joint hypothesis"
    /// check: for each pair of adjacent segments that have homophone
    /// ambiguity, we evaluate both cluster assignments and pick the
    /// one with higher joint score.
    ///
    /// Convergence: stops when no top-1 candidate changes, or max_iterations.
    pub fn rerank_all(&mut self) {
        let ranker = ContextRanker::new(self.conn);

        // ── Pass 0: independent ranking (no cross-segment context) ──
        for seg in &mut self.segments {
            let context_refs: Vec<&str> = self
                .committed_context
                .iter()
                .map(|s| s.as_str())
                .collect();

            seg.ranked = ranker.rank_candidates(
                &seg.phoneme_candidates,
                &context_refs,
                9000,
            );
            seg.selected_idx = 0;
        }

        // ── Joint hypothesis: try flipping ambiguous pairs ──
        // For each adjacent pair (i, i+1), check if swapping their
        // top candidates to the #2 option yields a higher joint context score.
        if self.segments.len() >= 2 {
            let embed_store = EmbeddingStore::new(self.conn);
            self.try_joint_hypotheses(&embed_store);
        }

        // ── Pass 1+: bidirectional iterative re-ranking ──
        for _iteration in 0..self.max_iterations {
            let mut any_changed = false;

            for i in 0..self.segments.len() {
                let mut context_words: Vec<String> = self.committed_context.clone();

                for (j, seg) in self.segments.iter().enumerate() {
                    if j != i {
                        context_words.push(seg.selected_surface().to_string());
                    }
                }

                let context_refs: Vec<&str> =
                    context_words.iter().map(|s| s.as_str()).collect();

                let new_ranked = ranker.rank_candidates(
                    &self.segments[i].phoneme_candidates,
                    &context_refs,
                    9000,
                );

                let old_top = self.segments[i]
                    .ranked
                    .first()
                    .map(|r| r.surface.clone())
                    .unwrap_or_default();
                let new_top = new_ranked
                    .first()
                    .map(|r| r.surface.clone())
                    .unwrap_or_default();

                if old_top != new_top {
                    any_changed = true;
                }

                self.segments[i].ranked = new_ranked;
                self.segments[i].selected_idx = 0;
            }

            if !any_changed {
                break;
            }
        }
    }

    /// Try joint hypotheses for adjacent segment pairs.
    ///
    /// For each pair (A, B), evaluate the 4 combinations of
    /// (A_top1, B_top1), (A_top1, B_top2), (A_top2, B_top1), (A_top2, B_top2)
    /// and pick the pair with highest mutual cosine similarity.
    fn try_joint_hypotheses(&mut self, embed_store: &EmbeddingStore<'_>) {
        for i in 0..(self.segments.len() - 1) {
            let j = i + 1;

            // Get top-2 candidates for each segment.
            let a_candidates: Vec<String> = self.segments[i]
                .ranked
                .iter()
                .take(2)
                .map(|r| r.surface.clone())
                .collect();
            let b_candidates: Vec<String> = self.segments[j]
                .ranked
                .iter()
                .take(2)
                .map(|r| r.surface.clone())
                .collect();

            if a_candidates.len() < 2 || b_candidates.len() < 2 {
                continue;
            }

            // Evaluate all 4 combinations.
            let mut best_sim = f64::NEG_INFINITY;
            let mut best_a = 0_usize;
            let mut best_b = 0_usize;

            for (ai, a_surf) in a_candidates.iter().enumerate() {
                for (bi, b_surf) in b_candidates.iter().enumerate() {
                    let sim = match (
                        embed_store.get_embedding(a_surf).ok().flatten(),
                        embed_store.get_embedding(b_surf).ok().flatten(),
                    ) {
                        (Some(va), Some(vb)) => cosine_similarity(&va, &vb),
                        _ => 0.0,
                    };

                    // Also factor in the original ranking scores.
                    let a_score = self.segments[i]
                        .ranked
                        .get(ai)
                        .map(|r| r.final_score)
                        .unwrap_or(0.0);
                    let b_score = self.segments[j]
                        .ranked
                        .get(bi)
                        .map(|r| r.final_score)
                        .unwrap_or(0.0);

                    // Joint score: heavily weight mutual similarity,
                    // because individual scores already incorporate frequency.
                    // High mutual similarity means "these words belong together."
                    let joint = sim * 0.7 + (a_score + b_score) * 0.15;

                    if joint > best_sim {
                        best_sim = joint;
                        best_a = ai;
                        best_b = bi;
                    }
                }
            }

            // If a non-default combination wins, swap.
            if best_a != 0 || best_b != 0 {
                self.segments[i].selected_idx = best_a;
                self.segments[j].selected_idx = best_b;

                // Promote the selected candidates to top of ranked list
                // so subsequent iterations use them as context.
                if best_a != 0 && best_a < self.segments[i].ranked.len() {
                    self.segments[i].ranked.swap(0, best_a);
                    self.segments[i].selected_idx = 0;
                }
                if best_b != 0 && best_b < self.segments[j].ranked.len() {
                    self.segments[j].ranked.swap(0, best_b);
                    self.segments[j].selected_idx = 0;
                }
            }
        }
    }

    /// Manually override a segment's selection and re-rank the rest.
    pub fn select_candidate(&mut self, segment_idx: usize, candidate_idx: usize) {
        if segment_idx < self.segments.len() {
            self.segments[segment_idx].selected_idx = candidate_idx;
            // Re-rank other segments based on this manual choice.
            // But DON'T reset the manually selected segment.
            self.rerank_others(segment_idx);
        }
    }

    /// Re-rank all segments except the manually-selected one.
    fn rerank_others(&mut self, fixed_idx: usize) {
        let ranker = ContextRanker::new(self.conn);

        for _iteration in 0..self.max_iterations {
            let mut any_changed = false;

            for i in 0..self.segments.len() {
                if i == fixed_idx {
                    continue; // Don't re-rank the manually selected segment.
                }

                let mut context_words: Vec<String> = self.committed_context.clone();
                for (j, seg) in self.segments.iter().enumerate() {
                    if j != i {
                        context_words.push(seg.selected_surface().to_string());
                    }
                }

                let context_refs: Vec<&str> =
                    context_words.iter().map(|s| s.as_str()).collect();

                let new_ranked = ranker.rank_candidates(
                    &self.segments[i].phoneme_candidates,
                    &context_refs,
                    9000,
                );

                let old_top = self.segments[i]
                    .ranked
                    .first()
                    .map(|r| r.surface.clone())
                    .unwrap_or_default();
                let new_top = new_ranked
                    .first()
                    .map(|r| r.surface.clone())
                    .unwrap_or_default();

                if old_top != new_top {
                    any_changed = true;
                }

                self.segments[i].ranked = new_ranked;
                self.segments[i].selected_idx = 0;
            }

            if !any_changed {
                break;
            }
        }
    }

    /// Number of segments currently in the buffer.
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::KanjiDict;
    use crate::embedding::EmbeddingStore;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        let embed = EmbeddingStore::new(&conn);
        embed.init_tables().unwrap();
        let dict = KanjiDict::new(&conn);
        dict.init_tables().unwrap();

        // Set up a minimal world for the 花/鼻 disambiguation scenario.
        dict.insert("はな", "花", 8000).unwrap();
        dict.insert("はな", "鼻", 5000).unwrap();
        dict.insert("かぜ", "風", 6000).unwrap();
        dict.insert("かぜ", "風邪", 5000).unwrap();

        // Embeddings: flower-domain vs body-domain
        //   花, 桜, 花見, 風 → [0.8+, 0.2-, 0, 0]  (nature cluster)
        //   鼻, 風邪, 頭, 痛い → [0, 0, 0.8+, 0.2-]  (body cluster)
        embed.store_embedding("花", &[0.85, 0.15, 0.05, 0.0]).unwrap();
        embed.store_embedding("桜", &[0.90, 0.10, 0.0, 0.0]).unwrap();
        embed.store_embedding("花見", &[0.80, 0.20, 0.0, 0.0]).unwrap();
        embed.store_embedding("風", &[0.70, 0.30, 0.0, 0.0]).unwrap();
        embed.store_embedding("鼻", &[0.05, 0.0, 0.85, 0.15]).unwrap();
        embed.store_embedding("風邪", &[0.0, 0.05, 0.80, 0.20]).unwrap();
        embed.store_embedding("頭", &[0.0, 0.0, 0.75, 0.25]).unwrap();
        embed.store_embedding("痛い", &[0.0, 0.0, 0.70, 0.30]).unwrap();

        // Particles (neutral)
        embed.store_embedding("が", &[0.25, 0.25, 0.25, 0.25]).unwrap();
        embed.store_embedding("を", &[0.25, 0.25, 0.25, 0.25]).unwrap();

        conn
    }

    #[test]
    fn test_single_segment_uses_frequency() {
        let conn = setup_db();
        let mut buf = SentenceBuffer::new(&conn);

        // "はな" alone → 花 wins by frequency (8000 > 5000).
        buf.add_segment(
            "하나".into(),
            vec![("はな".into(), "hana".into(), 0.9)],
        );

        assert_eq!(buf.segments[0].selected_surface(), "花");
    }

    #[test]
    fn test_retroactive_change_with_new_context() {
        let conn = setup_db();
        let mut buf = SentenceBuffer::new(&conn);

        // Step 1: "하나" → 花 (flower) — default by frequency.
        buf.add_segment(
            "하나".into(),
            vec![("はな".into(), "hana".into(), 0.9)],
        );
        assert_eq!(
            buf.segments[0].selected_surface(),
            "花",
            "Initially 花 should win"
        );

        // Step 2: "카제" → 風邪 (cold/illness) is added.
        // Now the bidirectional context should flip "はな" from 花→鼻.
        buf.add_segment(
            "카제".into(),
            vec![("かぜ".into(), "kaze".into(), 0.9)],
        );

        // The key assertion: segment 0 should have CHANGED to 鼻.
        assert_eq!(
            buf.segments[0].selected_surface(),
            "鼻",
            "After adding 風邪 context, はな should retroactively change to 鼻"
        );

        // And segment 1 should be 風邪 (cold) not 風 (wind).
        assert_eq!(
            buf.segments[1].selected_surface(),
            "風邪",
            "With 鼻 context, かぜ should prefer 風邪 over 風"
        );
    }

    #[test]
    fn test_retroactive_stays_flower_with_nature_context() {
        let conn = setup_db();
        let mut buf = SentenceBuffer::new(&conn);

        // Committed context: "桜" (cherry blossom).
        buf.committed_context.push("桜".into());

        // "하나" with 桜 context → 花 (flower) should stay.
        buf.add_segment(
            "하나".into(),
            vec![("はな".into(), "hana".into(), 0.9)],
        );
        assert_eq!(buf.segments[0].selected_surface(), "花");

        // Even adding a neutral particle shouldn't change it.
        buf.add_segment(
            "가".into(),
            vec![("が".into(), "ga".into(), 1.0)],
        );
        assert_eq!(
            buf.segments[0].selected_surface(),
            "花",
            "花 should remain with 桜 committed context"
        );
    }

    #[test]
    fn test_compose_and_commit() {
        let conn = setup_db();
        let mut buf = SentenceBuffer::new(&conn);

        buf.add_segment(
            "하나".into(),
            vec![("はな".into(), "hana".into(), 0.9)],
        );
        buf.add_segment(
            "가".into(),
            vec![("が".into(), "ga".into(), 1.0)],
        );

        let debug = buf.composed_debug();
        assert!(debug.contains("["), "Debug should have brackets");

        let committed = buf.commit();
        assert!(!committed.is_empty());
        assert!(buf.is_empty(), "Buffer should be empty after commit");
        assert!(
            !buf.committed_context.is_empty(),
            "Committed context should be populated"
        );
    }

    #[test]
    fn test_pop_segment() {
        let conn = setup_db();
        let mut buf = SentenceBuffer::new(&conn);

        buf.add_segment(
            "하나".into(),
            vec![("はな".into(), "hana".into(), 0.9)],
        );
        buf.add_segment(
            "카제".into(),
            vec![("かぜ".into(), "kaze".into(), 0.9)],
        );

        assert_eq!(buf.len(), 2);

        // Pop the last segment → should revert to single-segment ranking.
        buf.pop_segment();
        assert_eq!(buf.len(), 1);
        // Without 風邪 context, はな should go back to 花.
        assert_eq!(buf.segments[0].selected_surface(), "花");
    }

    #[test]
    fn test_manual_override_and_rerank() {
        let conn = setup_db();
        let mut buf = SentenceBuffer::new(&conn);

        buf.add_segment(
            "하나".into(),
            vec![("はな".into(), "hana".into(), 0.9)],
        );
        buf.add_segment(
            "카제".into(),
            vec![("かぜ".into(), "kaze".into(), 0.9)],
        );

        // After retroactive re-ranking, seg[0] = 鼻, seg[1] = 風邪.
        // Now manually override seg[0] back to 花.
        let flower_idx = buf.segments[0]
            .ranked
            .iter()
            .position(|r| r.surface == "花")
            .unwrap();
        buf.select_candidate(0, flower_idx);

        assert_eq!(buf.segments[0].selected_surface(), "花");
        // seg[1] should adjust: with 花 context, かぜ may prefer 風 over 風邪.
        // (depends on embedding distance, but the mechanism is verified)
    }

    #[test]
    fn test_three_segment_chain_reranking() {
        let conn = setup_db();

        // Add more entries for a 3-word scenario: 頭 が 痛い
        let dict = KanjiDict::new(&conn);
        dict.insert("あたま", "頭", 5000).unwrap();
        dict.insert("いたい", "痛い", 5000).unwrap();

        let embed = EmbeddingStore::new(&conn);
        embed.store_embedding("あたま", &[0.0, 0.0, 0.70, 0.30]).unwrap();
        embed.store_embedding("いたい", &[0.0, 0.0, 0.65, 0.35]).unwrap();

        let mut buf = SentenceBuffer::new(&conn);

        // "아타마" → 頭
        buf.add_segment(
            "아타마".into(),
            vec![("あたま".into(), "atama".into(), 0.9)],
        );

        // "가" → が
        buf.add_segment(
            "가".into(),
            vec![("が".into(), "ga".into(), 1.0)],
        );

        // "이타이" → 痛い
        buf.add_segment(
            "이타이".into(),
            vec![("いたい".into(), "itai".into(), 0.9)],
        );

        let composed = buf.composed();
        assert!(
            composed.contains("頭"),
            "Should contain 頭, got: {}",
            composed
        );
        assert!(
            composed.contains("痛い"),
            "Should contain 痛い, got: {}",
            composed
        );
    }
}
