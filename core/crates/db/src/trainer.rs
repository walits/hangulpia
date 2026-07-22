//! Token embedding trainer using sentence-level word co-occurrence.
//!
//! Implements a simplified GloVe-style approach:
//! 1. Build co-occurrence matrix from sentence corpus
//! 2. Use SVD-like iterative factorization to produce dense vectors
//! 3. Store resulting embeddings in the database
//!
//! The co-occurrence is weighted by distance: words closer together
//! in a sentence receive higher co-occurrence weight (1/distance).

use std::collections::HashMap;

use crate::corpus::{build_corpus, unique_words, CorpusSentence};
use crate::dictionary::KanjiDict;
use crate::embedding::EmbeddingStore;
use crate::generator::{generate_corpus, generate_corpus_with_vocab, generate_corpus_chunked, GenSentence};
use crate::vocab::VocabEntry;
use rusqlite::Connection;

/// Configuration for the embedding trainer.
#[derive(Debug, Clone)]
pub struct TrainerConfig {
    /// Dimension of the output embedding vectors.
    pub dim: usize,
    /// Window size for co-occurrence counting (words on each side).
    pub window_size: usize,
    /// Number of training iterations for vector optimization.
    pub iterations: usize,
    /// Learning rate for gradient descent.
    pub learning_rate: f64,
    /// Minimum co-occurrence count to consider.
    pub min_count: f64,
}

impl Default for TrainerConfig {
    fn default() -> Self {
        Self {
            dim: 64,
            window_size: 5,
            iterations: 50,
            learning_rate: 0.05,
            min_count: 0.01,
        }
    }
}

/// Co-occurrence matrix (sparse representation).
#[derive(Debug)]
pub struct CooccurrenceMatrix {
    /// word_id → word_id → co-occurrence weight
    pub matrix: HashMap<usize, HashMap<usize, f64>>,
    /// word → word_id mapping
    pub word_to_id: HashMap<String, usize>,
    /// word_id → word mapping
    pub id_to_word: Vec<String>,
}

impl CooccurrenceMatrix {
    pub fn new() -> Self {
        Self {
            matrix: HashMap::new(),
            word_to_id: HashMap::new(),
            id_to_word: Vec::new(),
        }
    }

    /// Get or create an ID for a word.
    pub fn get_or_create_id(&mut self, word: &str) -> usize {
        if let Some(&id) = self.word_to_id.get(word) {
            return id;
        }
        let id = self.id_to_word.len();
        self.id_to_word.push(word.to_string());
        self.word_to_id.insert(word.to_string(), id);
        id
    }

    /// Add co-occurrence count between two words.
    pub fn add_cooccurrence(&mut self, id_a: usize, id_b: usize, weight: f64) {
        *self
            .matrix
            .entry(id_a)
            .or_insert_with(HashMap::new)
            .entry(id_b)
            .or_insert(0.0) += weight;
        *self
            .matrix
            .entry(id_b)
            .or_insert_with(HashMap::new)
            .entry(id_a)
            .or_insert(0.0) += weight;
    }

    /// Build co-occurrence matrix from a corpus of sentences.
    pub fn build_from_corpus(
        &mut self,
        corpus: &[CorpusSentence],
        window_size: usize,
    ) {
        for sentence in corpus {
            let word_ids: Vec<usize> = sentence
                .words
                .iter()
                .map(|w| self.get_or_create_id(w.surface))
                .collect();

            // Also register readings and hangul as aliases pointing to same concept.
            // We add co-occurrence for surface forms (primary) and reading forms.
            let reading_ids: Vec<usize> = sentence
                .words
                .iter()
                .map(|w| {
                    if w.reading != w.surface {
                        self.get_or_create_id(w.reading)
                    } else {
                        self.get_or_create_id(w.surface)
                    }
                })
                .collect();

            let n = word_ids.len();
            for i in 0..n {
                for j in (i + 1)..n {
                    let distance = (j - i) as f64;
                    if distance <= window_size as f64 {
                        let weight = 1.0 / distance;

                        // Surface-surface co-occurrence
                        self.add_cooccurrence(word_ids[i], word_ids[j], weight);

                        // Reading-reading co-occurrence
                        self.add_cooccurrence(reading_ids[i], reading_ids[j], weight);

                        // Surface-reading cross co-occurrence (weaker)
                        self.add_cooccurrence(word_ids[i], reading_ids[j], weight * 0.5);
                        self.add_cooccurrence(reading_ids[i], word_ids[j], weight * 0.5);
                    }
                }
            }

            // Strong self-link between surface and its reading.
            for (i, w) in sentence.words.iter().enumerate() {
                if w.surface != w.reading {
                    self.add_cooccurrence(word_ids[i], reading_ids[i], 2.0);
                }
            }
        }
    }

    /// Build co-occurrence matrix from generated sentences.
    pub fn build_from_generated(
        &mut self,
        corpus: &[GenSentence],
        window_size: usize,
    ) {
        for sentence in corpus {
            let word_ids: Vec<usize> = sentence
                .words
                .iter()
                .map(|w| self.get_or_create_id(&w.surface))
                .collect();

            let reading_ids: Vec<usize> = sentence
                .words
                .iter()
                .map(|w| {
                    if w.reading != w.surface {
                        self.get_or_create_id(&w.reading)
                    } else {
                        self.get_or_create_id(&w.surface)
                    }
                })
                .collect();

            let n = word_ids.len();
            for i in 0..n {
                for j in (i + 1)..n {
                    let distance = (j - i) as f64;
                    if distance <= window_size as f64 {
                        let weight = 1.0 / distance;
                        self.add_cooccurrence(word_ids[i], word_ids[j], weight);
                        self.add_cooccurrence(reading_ids[i], reading_ids[j], weight);
                        self.add_cooccurrence(word_ids[i], reading_ids[j], weight * 0.5);
                        self.add_cooccurrence(reading_ids[i], word_ids[j], weight * 0.5);
                    }
                }
            }

            // Surface↔reading self-link.
            for w in &sentence.words {
                if w.surface != w.reading {
                    let sid = *self.word_to_id.get(w.surface.as_str()).unwrap();
                    let rid = *self.word_to_id.get(w.reading.as_str()).unwrap();
                    self.add_cooccurrence(sid, rid, 2.0);
                }
            }
        }
    }

    /// Build co-occurrence matrix from generated sentences WITH adjacent-sentence context.
    ///
    /// In addition to intra-sentence co-occurrence (same as build_from_generated),
    /// words in adjacent sentences (N-1, N, N+1) also co-occur with reduced weight.
    ///
    /// `cross_weight` controls how strongly cross-sentence pairs are weighted
    /// relative to intra-sentence pairs (recommended: 0.3~0.5).
    pub fn build_from_generated_with_adjacent(
        &mut self,
        corpus: &[GenSentence],
        window_size: usize,
        cross_weight: f64,
    ) {
        self.build_from_generated_with_adjacent_range(corpus, window_size, cross_weight, 1);
    }

    /// Build co-occurrence with adjacent-sentence context, with configurable sentence range.
    ///
    /// `sentence_range`: how many sentences forward/backward to include.
    ///   1 = only immediately adjacent (N-1, N+1)
    ///   2 = N-2, N-1, N+1, N+2
    ///   3 = N-3..N+3
    ///
    /// Weight decays by `1/sentence_distance` in addition to positional distance,
    /// so farther sentences contribute less.
    pub fn build_from_generated_with_adjacent_range(
        &mut self,
        corpus: &[GenSentence],
        window_size: usize,
        cross_weight: f64,
        sentence_range: usize,
    ) {
        // First, build intra-sentence co-occurrence (same as before)
        self.build_from_generated(corpus, window_size);

        // Then, build cross-sentence co-occurrence
        if corpus.len() < 2 { return; }

        for i in 0..corpus.len() {
            // Look at sentences within range
            for offset in 1..=sentence_range {
                let j = i + offset;
                if j >= corpus.len() { break; }

                let sent_a = &corpus[i];
                let sent_b = &corpus[j];
                let sentence_decay = 1.0 / offset as f64;

                // Get IDs for all words in both sentences
                let ids_a: Vec<usize> = sent_a.words.iter()
                    .map(|w| self.get_or_create_id(&w.surface))
                    .collect();
                let reading_ids_a: Vec<usize> = sent_a.words.iter()
                    .map(|w| if w.reading != w.surface {
                        self.get_or_create_id(&w.reading)
                    } else {
                        self.get_or_create_id(&w.surface)
                    })
                    .collect();

                let ids_b: Vec<usize> = sent_b.words.iter()
                    .map(|w| self.get_or_create_id(&w.surface))
                    .collect();
                let reading_ids_b: Vec<usize> = sent_b.words.iter()
                    .map(|w| if w.reading != w.surface {
                        self.get_or_create_id(&w.reading)
                    } else {
                        self.get_or_create_id(&w.surface)
                    })
                    .collect();

                // Cross-sentence co-occurrence
                for (ai, &aid) in ids_a.iter().enumerate() {
                    let a_dist_to_end = (ids_a.len() - ai) as f64;
                    for (bi, &bid) in ids_b.iter().enumerate() {
                        let b_dist_from_start = (bi + 1) as f64;
                        let total_dist = a_dist_to_end + b_dist_from_start;
                        let weight = cross_weight * sentence_decay / total_dist;
                        if weight < 0.01 { continue; }

                        self.add_cooccurrence(aid, bid, weight);
                        self.add_cooccurrence(reading_ids_a[ai], reading_ids_b[bi], weight);
                        self.add_cooccurrence(aid, reading_ids_b[bi], weight * 0.5);
                        self.add_cooccurrence(reading_ids_a[ai], bid, weight * 0.5);
                    }
                }
            }
        }
    }

    /// Vocabulary size.
    pub fn vocab_size(&self) -> usize {
        self.id_to_word.len()
    }
}

// ── Directional (Forward/Backward) Co-occurrence ────────────────────────────

/// Directional co-occurrence: tracks separately what appears to the LEFT vs RIGHT.
///
/// For sentence [A, B, C] with window=5:
///   forward_cooc(A, B) += 1/1   (B is 1 step to the RIGHT of A)
///   forward_cooc(A, C) += 1/2   (C is 2 steps to the RIGHT of A)
///   backward_cooc(B, A) += 1/1  (A is 1 step to the LEFT of B)
///   backward_cooc(C, A) += 1/2  (A is 2 steps to the LEFT of C)
///   backward_cooc(C, B) += 1/1  (B is 1 step to the LEFT of C)
///
/// This produces TWO embedding vectors per word:
///   fwd_vec[word] = "what typically appears to my RIGHT"
///   bwd_vec[word] = "what typically appears to my LEFT"
#[derive(Debug)]
pub struct DirectionalCooccurrence {
    /// forward: entry (i, j) = "word j appears to the RIGHT of word i"
    pub forward: CooccurrenceMatrix,
    /// backward: entry (i, j) = "word j appears to the LEFT of word i"
    pub backward: CooccurrenceMatrix,
}

impl DirectionalCooccurrence {
    pub fn new() -> Self {
        Self {
            forward: CooccurrenceMatrix::new(),
            backward: CooccurrenceMatrix::new(),
        }
    }

    /// Build directional co-occurrence from generated sentences.
    pub fn build_from_generated(&mut self, corpus: &[GenSentence], window_size: usize) {
        for sentence in corpus {
            let n = sentence.words.len();

            // Collect surface IDs (both matrices share the same vocab)
            let fwd_surface_ids: Vec<usize> = sentence.words.iter()
                .map(|w| self.forward.get_or_create_id(&w.surface))
                .collect();
            let bwd_surface_ids: Vec<usize> = sentence.words.iter()
                .map(|w| self.backward.get_or_create_id(&w.surface))
                .collect();

            // Reading IDs
            let fwd_reading_ids: Vec<usize> = sentence.words.iter()
                .map(|w| if w.reading != w.surface {
                    self.forward.get_or_create_id(&w.reading)
                } else {
                    self.forward.get_or_create_id(&w.surface)
                })
                .collect();
            let bwd_reading_ids: Vec<usize> = sentence.words.iter()
                .map(|w| if w.reading != w.surface {
                    self.backward.get_or_create_id(&w.reading)
                } else {
                    self.backward.get_or_create_id(&w.surface)
                })
                .collect();

            for i in 0..n {
                for j in (i + 1)..n {
                    let distance = (j - i) as f64;
                    if distance > window_size as f64 { break; }
                    let weight = 1.0 / distance;

                    // FORWARD: j is to the RIGHT of i
                    self.forward.add_cooccurrence(fwd_surface_ids[i], fwd_surface_ids[j], weight);
                    self.forward.add_cooccurrence(fwd_reading_ids[i], fwd_reading_ids[j], weight);
                    self.forward.add_cooccurrence(fwd_surface_ids[i], fwd_reading_ids[j], weight * 0.5);
                    self.forward.add_cooccurrence(fwd_reading_ids[i], fwd_surface_ids[j], weight * 0.5);

                    // BACKWARD: i is to the LEFT of j
                    self.backward.add_cooccurrence(bwd_surface_ids[j], bwd_surface_ids[i], weight);
                    self.backward.add_cooccurrence(bwd_reading_ids[j], bwd_reading_ids[i], weight);
                    self.backward.add_cooccurrence(bwd_surface_ids[j], bwd_reading_ids[i], weight * 0.5);
                    self.backward.add_cooccurrence(bwd_reading_ids[j], bwd_surface_ids[i], weight * 0.5);
                }
            }

            // Self-links: surface ↔ reading (both directions)
            for w in &sentence.words {
                if w.surface != w.reading {
                    let fs = *self.forward.word_to_id.get(w.surface.as_str()).unwrap();
                    let fr = *self.forward.word_to_id.get(w.reading.as_str()).unwrap();
                    self.forward.add_cooccurrence(fs, fr, 2.0);
                    let bs = *self.backward.word_to_id.get(w.surface.as_str()).unwrap();
                    let br = *self.backward.word_to_id.get(w.reading.as_str()).unwrap();
                    self.backward.add_cooccurrence(bs, br, 2.0);
                }
            }
        }
    }

    /// Build directional co-occurrence from hand-crafted corpus.
    pub fn build_from_corpus(&mut self, corpus: &[CorpusSentence], window_size: usize) {
        for sentence in corpus {
            let n = sentence.words.len();

            let fwd_surface_ids: Vec<usize> = sentence.words.iter()
                .map(|w| self.forward.get_or_create_id(w.surface))
                .collect();
            let bwd_surface_ids: Vec<usize> = sentence.words.iter()
                .map(|w| self.backward.get_or_create_id(w.surface))
                .collect();
            let fwd_reading_ids: Vec<usize> = sentence.words.iter()
                .map(|w| if w.reading != w.surface {
                    self.forward.get_or_create_id(w.reading)
                } else {
                    self.forward.get_or_create_id(w.surface)
                })
                .collect();
            let bwd_reading_ids: Vec<usize> = sentence.words.iter()
                .map(|w| if w.reading != w.surface {
                    self.backward.get_or_create_id(w.reading)
                } else {
                    self.backward.get_or_create_id(w.surface)
                })
                .collect();

            for i in 0..n {
                for j in (i + 1)..n {
                    let distance = (j - i) as f64;
                    if distance > window_size as f64 { break; }
                    let weight = 1.0 / distance;

                    self.forward.add_cooccurrence(fwd_surface_ids[i], fwd_surface_ids[j], weight);
                    self.forward.add_cooccurrence(fwd_reading_ids[i], fwd_reading_ids[j], weight);
                    self.forward.add_cooccurrence(fwd_surface_ids[i], fwd_reading_ids[j], weight * 0.5);
                    self.forward.add_cooccurrence(fwd_reading_ids[i], fwd_surface_ids[j], weight * 0.5);

                    self.backward.add_cooccurrence(bwd_surface_ids[j], bwd_surface_ids[i], weight);
                    self.backward.add_cooccurrence(bwd_reading_ids[j], bwd_reading_ids[i], weight);
                    self.backward.add_cooccurrence(bwd_surface_ids[j], bwd_reading_ids[i], weight * 0.5);
                    self.backward.add_cooccurrence(bwd_reading_ids[j], bwd_surface_ids[i], weight * 0.5);
                }
            }

            for w in &sentence.words {
                if w.surface != w.reading {
                    let fs = *self.forward.word_to_id.get(w.surface).unwrap();
                    let fr = *self.forward.word_to_id.get(w.reading).unwrap();
                    self.forward.add_cooccurrence(fs, fr, 2.0);
                    let bs = *self.backward.word_to_id.get(w.surface).unwrap();
                    let br = *self.backward.word_to_id.get(w.reading).unwrap();
                    self.backward.add_cooccurrence(bs, br, 2.0);
                }
            }
        }
    }
}

/// Train embedding vectors from a co-occurrence matrix.
///
/// Uses a simplified GloVe-like factorization:
///   For each (i, j) pair with co-occurrence X_ij > 0:
///     loss = (dot(w_i, w_j) - log(X_ij))^2
///   Optimize via stochastic gradient descent.
pub fn train_embeddings(
    cooc: &CooccurrenceMatrix,
    config: &TrainerConfig,
) -> Vec<Vec<f32>> {
    let vocab_size = cooc.vocab_size();
    let dim = config.dim;

    // Initialize vectors with small random-like values (deterministic for reproducibility).
    let mut vectors: Vec<Vec<f32>> = (0..vocab_size)
        .map(|i| {
            (0..dim)
                .map(|d| {
                    // Deterministic pseudo-random initialization using word_id and dimension.
                    let seed = (i * 7919 + d * 104729 + 31) as f64;
                    let val = ((seed * 0.6180339887).fract() - 0.5) * 0.1;
                    val as f32
                })
                .collect()
        })
        .collect();

    // Bias terms.
    let mut bias: Vec<f32> = vec![0.0; vocab_size];

    // Collect all non-zero pairs for training.
    let mut pairs: Vec<(usize, usize, f64)> = Vec::new();
    for (&i, neighbors) in &cooc.matrix {
        for (&j, &weight) in neighbors {
            if i < j && weight >= config.min_count {
                pairs.push((i, j, weight));
            }
        }
    }

    let lr = config.learning_rate as f32;

    for _iter in 0..config.iterations {
        let mut total_loss = 0.0_f64;

        for &(i, j, x_ij) in &pairs {
            // GloVe-style weighting: f(x) = min(1, (x/x_max)^0.75)
            let x_max = 10.0_f64;
            let f_x = if x_ij < x_max {
                (x_ij / x_max).powf(0.75)
            } else {
                1.0
            } as f32;

            // Compute dot product.
            let dot: f32 = vectors[i]
                .iter()
                .zip(vectors[j].iter())
                .map(|(a, b)| a * b)
                .sum::<f32>()
                + bias[i]
                + bias[j];

            let target = (x_ij as f32).ln();
            let diff = dot - target;
            let fdiff = f_x * diff;

            total_loss += (f_x * diff * diff) as f64;

            // Gradient update.
            let grad_scale = 2.0 * lr * fdiff;
            for d in 0..dim {
                let gi = grad_scale * vectors[j][d];
                let gj = grad_scale * vectors[i][d];
                vectors[i][d] -= gi;
                vectors[j][d] -= gj;
            }
            bias[i] -= 2.0 * lr * fdiff;
            bias[j] -= 2.0 * lr * fdiff;
        }

        // Log progress periodically.
        if (_iter + 1) % 10 == 0 || _iter == 0 {
            let avg_loss = if pairs.is_empty() {
                0.0
            } else {
                total_loss / pairs.len() as f64
            };
            log::info!(
                "Iteration {}/{}: avg_loss = {:.6}",
                _iter + 1,
                config.iterations,
                avg_loss
            );
        }
    }

    // Normalize vectors to unit length.
    for v in &mut vectors {
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-8 {
            for x in v.iter_mut() {
                *x /= norm;
            }
        }
    }

    vectors
}

/// Full training pipeline: corpus → co-occurrence → embeddings → DB.
pub struct DbBuilder<'a> {
    conn: &'a Connection,
    config: TrainerConfig,
}

impl<'a> DbBuilder<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            config: TrainerConfig::default(),
        }
    }

    pub fn with_config(mut self, config: TrainerConfig) -> Self {
        self.config = config;
        self
    }

    /// Run the full build pipeline.
    pub fn build(&self) -> Result<BuildStats, rusqlite::Error> {
        let corpus = build_corpus();
        let words = unique_words(&corpus);

        // Step 1: Build co-occurrence matrix.
        let mut cooc = CooccurrenceMatrix::new();
        cooc.build_from_corpus(&corpus, self.config.window_size);

        // Step 2: Train embedding vectors.
        let vectors = train_embeddings(&cooc, &self.config);

        // Step 3: Store embeddings in DB.
        let embed_store = EmbeddingStore::new(self.conn);
        embed_store.init_tables()?;

        let mut embedding_count = 0;
        let tx = self.conn.unchecked_transaction()?;
        {
            let store = EmbeddingStore::new(&tx);
            for (id, word) in cooc.id_to_word.iter().enumerate() {
                store.store_embedding(word, &vectors[id])?;
                embedding_count += 1;
            }
            store.set_meta("dim", &self.config.dim.to_string())?;
            store.set_meta("window_size", &self.config.window_size.to_string())?;
            store.set_meta("iterations", &self.config.iterations.to_string())?;
            store.set_meta("corpus_sentences", &corpus.len().to_string())?;
            store.set_meta("vocab_size", &cooc.vocab_size().to_string())?;
        }
        tx.commit()?;

        // Step 4: Populate kanji dictionary from corpus.
        let kanji_dict = KanjiDict::new(self.conn);
        kanji_dict.init_tables()?;

        let mut dict_count = 0;
        let tx2 = self.conn.unchecked_transaction()?;
        {
            let dict = KanjiDict::new(&tx2);
            // Track frequency from corpus usage.
            let mut freq_map: HashMap<(&str, &str), i64> = HashMap::new();
            for sentence in &corpus {
                for w in &sentence.words {
                    *freq_map.entry((w.reading, w.surface)).or_insert(0) += 1;
                }
            }

            for (&(reading, surface), &freq) in &freq_map {
                // Scale frequency (multiply by 100 for granularity).
                dict.insert(reading, surface, freq * 100)?;
                dict_count += 1;
            }

            // Also insert common homophone disambiguation entries
            // that may not be in the corpus but are important.
            let extra_entries = extra_kanji_entries();
            for (reading, surface, freq) in &extra_entries {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }
        }
        tx2.commit()?;

        // Step 5: Populate hangul → reading mapping table.
        self.build_hangul_index(&corpus)?;

        Ok(BuildStats {
            sentences: corpus.len(),
            unique_words: words.len(),
            vocab_size: cooc.vocab_size(),
            embeddings: embedding_count,
            dict_entries: dict_count,
            dim: self.config.dim,
        })
    }

    /// Run a large-scale build pipeline using the generator.
    ///
    /// Generates `sentence_count` sentences (default 100,000), trains
    /// embeddings on the co-occurrence data, and populates all tables.
    pub fn build_large(&self, sentence_count: usize) -> Result<BuildStats, rusqlite::Error> {
        eprintln!("  [1/5] Generating {} sentences...", sentence_count);
        let gen_corpus = generate_corpus(sentence_count);

        // Count unique words.
        let mut unique_surfaces = std::collections::HashSet::new();
        for s in &gen_corpus {
            for w in &s.words {
                unique_surfaces.insert(w.surface.clone());
            }
        }
        let unique_count = unique_surfaces.len();

        eprintln!("  [2/5] Building co-occurrence matrix...");
        let mut cooc = CooccurrenceMatrix::new();
        // Also include the hand-crafted corpus for high-quality base data.
        let base_corpus = build_corpus();
        cooc.build_from_corpus(&base_corpus, self.config.window_size);
        cooc.build_from_generated(&gen_corpus, self.config.window_size);
        eprintln!("         Vocabulary: {} words, co-occurrence pairs computed", cooc.vocab_size());

        eprintln!("  [3/5] Training embeddings (dim={}, iter={})...", self.config.dim, self.config.iterations);
        let vectors = train_embeddings(&cooc, &self.config);

        eprintln!("  [4/5] Storing embeddings and dictionary...");
        let embed_store = EmbeddingStore::new(self.conn);
        embed_store.init_tables()?;

        let mut embedding_count = 0;
        {
            let tx = self.conn.unchecked_transaction()?;
            let store = EmbeddingStore::new(&tx);
            for (id, word) in cooc.id_to_word.iter().enumerate() {
                store.store_embedding(word, &vectors[id])?;
                embedding_count += 1;
            }
            store.set_meta("dim", &self.config.dim.to_string())?;
            store.set_meta("window_size", &self.config.window_size.to_string())?;
            store.set_meta("iterations", &self.config.iterations.to_string())?;
            store.set_meta("corpus_sentences", &(base_corpus.len() + gen_corpus.len()).to_string())?;
            store.set_meta("generated_sentences", &gen_corpus.len().to_string())?;
            store.set_meta("vocab_size", &cooc.vocab_size().to_string())?;
            tx.commit()?;
        }

        // Populate kanji dictionary from both corpora.
        let kanji_dict = KanjiDict::new(self.conn);
        kanji_dict.init_tables()?;

        let mut dict_count = 0;
        {
            let tx = self.conn.unchecked_transaction()?;
            let dict = KanjiDict::new(&tx);

            // From hand-crafted corpus.
            let mut freq_map: HashMap<(String, String), i64> = HashMap::new();
            for sentence in &base_corpus {
                for w in &sentence.words {
                    *freq_map.entry((w.reading.to_string(), w.surface.to_string())).or_insert(0) += 1;
                }
            }
            // From generated corpus.
            for sentence in &gen_corpus {
                for w in &sentence.words {
                    *freq_map.entry((w.reading.clone(), w.surface.clone())).or_insert(0) += 1;
                }
            }

            for ((reading, surface), freq) in &freq_map {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }

            // Extra homophones.
            for (reading, surface, freq) in &extra_kanji_entries() {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }
            tx.commit()?;
        }

        // Hangul index from both corpora.
        eprintln!("  [5/5] Building hangul index...");
        self.build_hangul_index(&base_corpus)?;
        self.build_hangul_index_generated(&gen_corpus)?;

        eprintln!("  Done!");

        Ok(BuildStats {
            sentences: base_corpus.len() + gen_corpus.len(),
            unique_words: unique_count,
            vocab_size: cooc.vocab_size(),
            embeddings: embedding_count,
            dict_entries: dict_count,
            dim: self.config.dim,
        })
    }

    /// Run a large-scale build pipeline with a CUSTOM vocabulary.
    ///
    /// Same as build_large but uses the provided vocab instead of the default.
    pub fn build_large_with_vocab(&self, vocab: &[VocabEntry], sentence_count: usize) -> Result<BuildStats, rusqlite::Error> {
        eprintln!("  [1/5] Generating {} sentences with {} vocab entries...", sentence_count, vocab.len());
        let gen_corpus = generate_corpus_with_vocab(vocab, sentence_count);

        let mut unique_surfaces = std::collections::HashSet::new();
        for s in &gen_corpus {
            for w in &s.words {
                unique_surfaces.insert(w.surface.clone());
            }
        }
        let unique_count = unique_surfaces.len();

        eprintln!("  [2/5] Building co-occurrence matrix...");
        let mut cooc = CooccurrenceMatrix::new();
        let base_corpus = build_corpus();
        cooc.build_from_corpus(&base_corpus, self.config.window_size);
        cooc.build_from_generated(&gen_corpus, self.config.window_size);
        eprintln!("         Vocabulary: {} words, co-occurrence pairs computed", cooc.vocab_size());

        eprintln!("  [3/5] Training embeddings (dim={}, iter={})...", self.config.dim, self.config.iterations);
        let vectors = train_embeddings(&cooc, &self.config);

        eprintln!("  [4/5] Storing embeddings and dictionary...");
        let embed_store = EmbeddingStore::new(self.conn);
        embed_store.init_tables()?;

        let mut embedding_count = 0;
        {
            let tx = self.conn.unchecked_transaction()?;
            let store = EmbeddingStore::new(&tx);
            for (id, word) in cooc.id_to_word.iter().enumerate() {
                store.store_embedding(word, &vectors[id])?;
                embedding_count += 1;
            }
            store.set_meta("dim", &self.config.dim.to_string())?;
            store.set_meta("window_size", &self.config.window_size.to_string())?;
            store.set_meta("iterations", &self.config.iterations.to_string())?;
            store.set_meta("corpus_sentences", &(base_corpus.len() + gen_corpus.len()).to_string())?;
            store.set_meta("generated_sentences", &gen_corpus.len().to_string())?;
            store.set_meta("vocab_size", &cooc.vocab_size().to_string())?;
            tx.commit()?;
        }

        let kanji_dict = KanjiDict::new(self.conn);
        kanji_dict.init_tables()?;

        let mut dict_count = 0;
        {
            let tx = self.conn.unchecked_transaction()?;
            let dict = KanjiDict::new(&tx);
            let mut freq_map: HashMap<(String, String), i64> = HashMap::new();
            for sentence in &base_corpus {
                for w in &sentence.words {
                    *freq_map.entry((w.reading.to_string(), w.surface.to_string())).or_insert(0) += 1;
                }
            }
            for sentence in &gen_corpus {
                for w in &sentence.words {
                    *freq_map.entry((w.reading.clone(), w.surface.clone())).or_insert(0) += 1;
                }
            }
            for ((reading, surface), freq) in &freq_map {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }
            for (reading, surface, freq) in &extra_kanji_entries() {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }
            tx.commit()?;
        }

        eprintln!("  [5/5] Building hangul index...");
        self.build_hangul_index(&base_corpus)?;
        self.build_hangul_index_generated(&gen_corpus)?;

        eprintln!("  Done!");

        Ok(BuildStats {
            sentences: base_corpus.len() + gen_corpus.len(),
            unique_words: unique_count,
            vocab_size: cooc.vocab_size(),
            embeddings: embedding_count,
            dict_entries: dict_count,
            dim: self.config.dim,
        })
    }

    /// Run a large-scale build with custom vocab using CHUNKED generation.
    ///
    /// Generates sentences in 1M-sentence chunks to avoid OOM on large corpora (4M+).
    /// Co-occurrence, frequency, and hangul index are built incrementally.
    pub fn build_large_with_vocab_chunked(&self, vocab: &[VocabEntry], sentence_count: usize) -> Result<BuildStats, rusqlite::Error> {
        let chunk_size = 1_000_000usize.min(sentence_count);
        eprintln!("  [1/5] Generating {} sentences in {}K chunks (vocab {})...",
            sentence_count, chunk_size / 1000, vocab.len());

        let mut cooc = CooccurrenceMatrix::new();
        let mut unique_surfaces = std::collections::HashSet::new();
        let mut freq_map: HashMap<(String, String), i64> = HashMap::new();
        let mut hangul_map: HashMap<(String, String, String), i64> = HashMap::new();
        let mut gen_count = 0usize;

        let window_size = self.config.window_size;

        generate_corpus_chunked(vocab, sentence_count, chunk_size, |chunk| {
            gen_count += chunk.len();
            eprint!("\r         Generated {}K / {}K", gen_count / 1000, sentence_count / 1000);

            for s in chunk {
                for w in &s.words {
                    unique_surfaces.insert(w.surface.clone());
                    *freq_map.entry((w.reading.clone(), w.surface.clone())).or_insert(0) += 1;
                    *hangul_map.entry((w.hangul.clone(), w.reading.clone(), w.surface.clone())).or_insert(0) += 1;
                }
            }
            cooc.build_from_generated(chunk, window_size);
        });
        eprintln!();
        let unique_count = unique_surfaces.len();

        // Also include hand-crafted base corpus
        let base_corpus = build_corpus();
        cooc.build_from_corpus(&base_corpus, self.config.window_size);
        for sentence in &base_corpus {
            for w in &sentence.words {
                *freq_map.entry((w.reading.to_string(), w.surface.to_string())).or_insert(0) += 1;
            }
        }

        eprintln!("  [2/5] Vocabulary: {} words", cooc.vocab_size());

        eprintln!("  [3/5] Training embeddings (dim={}, iter={})...", self.config.dim, self.config.iterations);
        let vectors = train_embeddings(&cooc, &self.config);

        eprintln!("  [4/5] Storing embeddings and dictionary...");
        let embed_store = EmbeddingStore::new(self.conn);
        embed_store.init_tables()?;

        let mut embedding_count = 0;
        {
            let tx = self.conn.unchecked_transaction()?;
            let store = EmbeddingStore::new(&tx);
            for (id, word) in cooc.id_to_word.iter().enumerate() {
                store.store_embedding(word, &vectors[id])?;
                embedding_count += 1;
            }
            store.set_meta("dim", &self.config.dim.to_string())?;
            store.set_meta("window_size", &self.config.window_size.to_string())?;
            store.set_meta("iterations", &self.config.iterations.to_string())?;
            store.set_meta("generated_sentences", &gen_count.to_string())?;
            store.set_meta("vocab_size", &cooc.vocab_size().to_string())?;
            tx.commit()?;
        }

        let kanji_dict = KanjiDict::new(self.conn);
        kanji_dict.init_tables()?;

        let mut dict_count = 0;
        {
            let tx = self.conn.unchecked_transaction()?;
            let dict = KanjiDict::new(&tx);
            for ((reading, surface), freq) in &freq_map {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }
            for (reading, surface, freq) in &extra_kanji_entries() {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }
            tx.commit()?;
        }

        // Hangul index from accumulated map
        eprintln!("  [5/5] Building hangul index...");
        self.build_hangul_index(&base_corpus)?;
        {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS hangul_index (
                    hangul TEXT NOT NULL,
                    reading TEXT NOT NULL,
                    surface TEXT NOT NULL,
                    frequency INTEGER NOT NULL DEFAULT 1,
                    UNIQUE(hangul, reading, surface)
                );
                CREATE INDEX IF NOT EXISTS idx_hangul ON hangul_index(hangul);",
            )?;
            let tx = self.conn.unchecked_transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR REPLACE INTO hangul_index (hangul, reading, surface, frequency) VALUES (?1, ?2, ?3, ?4)",
                )?;
                for ((hangul, reading, surface), freq) in &hangul_map {
                    stmt.execute(rusqlite::params![hangul, reading, surface, freq])?;
                }
            }
            tx.commit()?;
        }

        eprintln!("  Done!");

        Ok(BuildStats {
            sentences: base_corpus.len() + gen_count,
            unique_words: unique_count,
            vocab_size: cooc.vocab_size(),
            embeddings: embedding_count,
            dict_entries: dict_count,
            dim: self.config.dim,
        })
    }

    /// Build from an externally-provided corpus of GenSentence data.
    ///
    /// Unlike the generate-based methods, this accepts pre-built GenSentence structs
    /// (e.g. from Wikipedia morphological analysis). Processes in chunks to control memory.
    /// Optionally adds adjacent-sentence co-occurrence if cross_weight > 0.
    pub fn build_from_external_corpus(
        &self,
        corpus: &[GenSentence],
        cross_weight: f64,
        sentence_range: usize,
    ) -> Result<BuildStats, rusqlite::Error> {
        let chunk_size = 50_000usize;
        eprintln!("  [1/5] Processing {} external sentences (cross_weight={}, sent_range={})...",
            corpus.len(), cross_weight, sentence_range);

        let mut cooc = CooccurrenceMatrix::new();
        let mut unique_surfaces = std::collections::HashSet::new();
        let mut freq_map: HashMap<(String, String), i64> = HashMap::new();
        let mut hangul_map: HashMap<(String, String, String), i64> = HashMap::new();

        let window_size = self.config.window_size;

        // Process in chunks
        for (chunk_idx, chunk) in corpus.chunks(chunk_size).enumerate() {
            eprint!("\r         Chunk {} / {} ({} sentences)",
                chunk_idx + 1, (corpus.len() + chunk_size - 1) / chunk_size, corpus.len());

            for s in chunk {
                for w in &s.words {
                    unique_surfaces.insert(w.surface.clone());
                    *freq_map.entry((w.reading.clone(), w.surface.clone())).or_insert(0) += 1;
                    *hangul_map.entry((w.hangul.clone(), w.reading.clone(), w.surface.clone())).or_insert(0) += 1;
                }
            }

            if cross_weight > 0.0 {
                cooc.build_from_generated_with_adjacent_range(chunk, window_size, cross_weight, sentence_range);
            } else {
                cooc.build_from_generated(chunk, window_size);
            }
        }
        eprintln!();
        let unique_count = unique_surfaces.len();

        // Also include hand-crafted base corpus
        let base_corpus = build_corpus();
        cooc.build_from_corpus(&base_corpus, self.config.window_size);
        for sentence in &base_corpus {
            for w in &sentence.words {
                *freq_map.entry((w.reading.to_string(), w.surface.to_string())).or_insert(0) += 1;
            }
        }

        eprintln!("  [2/5] Vocabulary: {} words", cooc.vocab_size());

        eprintln!("  [3/5] Training embeddings (dim={}, iter={})...", self.config.dim, self.config.iterations);
        let vectors = train_embeddings(&cooc, &self.config);

        eprintln!("  [4/5] Storing embeddings and dictionary...");
        let embed_store = EmbeddingStore::new(self.conn);
        embed_store.init_tables()?;

        let mut embedding_count = 0;
        {
            let tx = self.conn.unchecked_transaction()?;
            let store = EmbeddingStore::new(&tx);
            for (id, word) in cooc.id_to_word.iter().enumerate() {
                store.store_embedding(word, &vectors[id])?;
                embedding_count += 1;
            }
            store.set_meta("dim", &self.config.dim.to_string())?;
            store.set_meta("window_size", &self.config.window_size.to_string())?;
            store.set_meta("iterations", &self.config.iterations.to_string())?;
            store.set_meta("generated_sentences", &corpus.len().to_string())?;
            store.set_meta("vocab_size", &cooc.vocab_size().to_string())?;
            store.set_meta("source", "external_corpus")?;
            tx.commit()?;
        }

        let kanji_dict = KanjiDict::new(self.conn);
        kanji_dict.init_tables()?;

        let mut dict_count = 0;
        {
            let tx = self.conn.unchecked_transaction()?;
            let dict = KanjiDict::new(&tx);
            for ((reading, surface), freq) in &freq_map {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }
            for (reading, surface, freq) in &extra_kanji_entries() {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }
            tx.commit()?;
        }

        // Hangul index from accumulated map
        eprintln!("  [5/5] Building hangul index...");
        self.build_hangul_index(&base_corpus)?;
        {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS hangul_index (
                    hangul TEXT NOT NULL,
                    reading TEXT NOT NULL,
                    surface TEXT NOT NULL,
                    frequency INTEGER NOT NULL DEFAULT 1,
                    UNIQUE(hangul, reading, surface)
                );
                CREATE INDEX IF NOT EXISTS idx_hangul ON hangul_index(hangul);",
            )?;
            let tx = self.conn.unchecked_transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR REPLACE INTO hangul_index (hangul, reading, surface, frequency) VALUES (?1, ?2, ?3, ?4)",
                )?;
                for ((hangul, reading, surface), freq) in &hangul_map {
                    stmt.execute(rusqlite::params![hangul, reading, surface, freq])?;
                }
            }
            tx.commit()?;
        }

        eprintln!("  Done!");

        Ok(BuildStats {
            sentences: base_corpus.len() + corpus.len(),
            unique_words: unique_count,
            vocab_size: cooc.vocab_size(),
            embeddings: embedding_count,
            dict_entries: dict_count,
            dim: self.config.dim,
        })
    }

    /// Run a large-scale build with adjacent-sentence co-occurrence.
    ///
    /// Like build_large_with_vocab_chunked but also adds cross-sentence co-occurrence
    /// between words in consecutive sentences (with reduced weight).
    /// This captures discourse-level context beyond individual sentence boundaries.
    ///
    /// Parameters:
    ///   - `cross_weight`: strength of cross-sentence co-occurrence (0.0~1.0)
    ///   - `sentence_range`: how many sentences forward/backward (1=adjacent only, 2=±2, etc.)
    pub fn build_large_with_adjacent_sentences(
        &self,
        vocab: &[VocabEntry],
        sentence_count: usize,
        cross_weight: f64,
    ) -> Result<BuildStats, rusqlite::Error> {
        self.build_large_with_adjacent_sentences_range(vocab, sentence_count, cross_weight, 1)
    }

    /// Run a large-scale build with configurable adjacent-sentence range.
    pub fn build_large_with_adjacent_sentences_range(
        &self,
        vocab: &[VocabEntry],
        sentence_count: usize,
        cross_weight: f64,
        sentence_range: usize,
    ) -> Result<BuildStats, rusqlite::Error> {
        let chunk_size = 1_000_000usize.min(sentence_count);
        eprintln!("  [1/5] Generating {} sentences in {}K chunks (vocab {}, cross_weight={}, sent_range={})...",
            sentence_count, chunk_size / 1000, vocab.len(), cross_weight, sentence_range);

        let mut cooc = CooccurrenceMatrix::new();
        let mut unique_surfaces = std::collections::HashSet::new();
        let mut freq_map: HashMap<(String, String), i64> = HashMap::new();
        let mut hangul_map: HashMap<(String, String, String), i64> = HashMap::new();
        let mut gen_count = 0usize;

        let window_size = self.config.window_size;

        generate_corpus_chunked(vocab, sentence_count, chunk_size, |chunk| {
            gen_count += chunk.len();
            eprint!("\r         Generated {}K / {}K", gen_count / 1000, sentence_count / 1000);

            for s in chunk {
                for w in &s.words {
                    unique_surfaces.insert(w.surface.clone());
                    *freq_map.entry((w.reading.clone(), w.surface.clone())).or_insert(0) += 1;
                    *hangul_map.entry((w.hangul.clone(), w.reading.clone(), w.surface.clone())).or_insert(0) += 1;
                }
            }
            // Use adjacent-sentence co-occurrence
            cooc.build_from_generated_with_adjacent_range(chunk, window_size, cross_weight, sentence_range);
        });
        eprintln!();
        let unique_count = unique_surfaces.len();

        // Also include hand-crafted base corpus
        let base_corpus = build_corpus();
        cooc.build_from_corpus(&base_corpus, self.config.window_size);
        for sentence in &base_corpus {
            for w in &sentence.words {
                *freq_map.entry((w.reading.to_string(), w.surface.to_string())).or_insert(0) += 1;
            }
        }

        eprintln!("  [2/5] Vocabulary: {} words", cooc.vocab_size());

        eprintln!("  [3/5] Training embeddings (dim={}, iter={})...", self.config.dim, self.config.iterations);
        let vectors = train_embeddings(&cooc, &self.config);

        eprintln!("  [4/5] Storing embeddings and dictionary...");
        let embed_store = EmbeddingStore::new(self.conn);
        embed_store.init_tables()?;

        let mut embedding_count = 0;
        {
            let tx = self.conn.unchecked_transaction()?;
            let store = EmbeddingStore::new(&tx);
            for (id, word) in cooc.id_to_word.iter().enumerate() {
                store.store_embedding(word, &vectors[id])?;
                embedding_count += 1;
            }
            store.set_meta("dim", &self.config.dim.to_string())?;
            store.set_meta("window_size", &self.config.window_size.to_string())?;
            store.set_meta("iterations", &self.config.iterations.to_string())?;
            store.set_meta("generated_sentences", &gen_count.to_string())?;
            store.set_meta("vocab_size", &cooc.vocab_size().to_string())?;
            store.set_meta("cross_sentence_weight", &cross_weight.to_string())?;
            tx.commit()?;
        }

        let kanji_dict = KanjiDict::new(self.conn);
        kanji_dict.init_tables()?;

        let mut dict_count = 0;
        {
            let tx = self.conn.unchecked_transaction()?;
            let dict = KanjiDict::new(&tx);
            for ((reading, surface), freq) in &freq_map {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }
            for (reading, surface, freq) in &extra_kanji_entries() {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }
            tx.commit()?;
        }

        // Hangul index from accumulated map
        eprintln!("  [5/5] Building hangul index...");
        self.build_hangul_index(&base_corpus)?;
        {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS hangul_index (
                    hangul TEXT NOT NULL,
                    reading TEXT NOT NULL,
                    surface TEXT NOT NULL,
                    frequency INTEGER NOT NULL DEFAULT 1,
                    UNIQUE(hangul, reading, surface)
                );
                CREATE INDEX IF NOT EXISTS idx_hangul ON hangul_index(hangul);",
            )?;
            let tx = self.conn.unchecked_transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR REPLACE INTO hangul_index (hangul, reading, surface, frequency) VALUES (?1, ?2, ?3, ?4)",
                )?;
                for ((hangul, reading, surface), freq) in &hangul_map {
                    stmt.execute(rusqlite::params![hangul, reading, surface, freq])?;
                }
            }
            tx.commit()?;
        }

        eprintln!("  Done!");

        Ok(BuildStats {
            sentences: base_corpus.len() + gen_count,
            unique_words: unique_count,
            vocab_size: cooc.vocab_size(),
            embeddings: embedding_count,
            dict_entries: dict_count,
            dim: self.config.dim,
        })
    }

    /// Run a large-scale DIRECTIONAL build pipeline.
    ///
    /// Same as build_large but stores TWO vectors per word:
    ///   "F:{word}" = forward vector (what appears to the right)
    ///   "B:{word}" = backward vector (what appears to the left)
    pub fn build_large_directional(&self, sentence_count: usize) -> Result<BuildStats, rusqlite::Error> {
        eprintln!("  [1/6] Generating {} sentences...", sentence_count);
        let gen_corpus = generate_corpus(sentence_count);

        let mut unique_surfaces = std::collections::HashSet::new();
        for s in &gen_corpus {
            for w in &s.words {
                unique_surfaces.insert(w.surface.clone());
            }
        }
        let unique_count = unique_surfaces.len();

        eprintln!("  [2/6] Building DIRECTIONAL co-occurrence matrices...");
        let mut dir_cooc = DirectionalCooccurrence::new();
        let base_corpus = build_corpus();
        dir_cooc.build_from_corpus(&base_corpus, self.config.window_size);
        dir_cooc.build_from_generated(&gen_corpus, self.config.window_size);
        eprintln!("         Forward vocab: {}, Backward vocab: {}",
            dir_cooc.forward.vocab_size(), dir_cooc.backward.vocab_size());

        eprintln!("  [3/6] Training FORWARD embeddings (dim={}, iter={})...", self.config.dim, self.config.iterations);
        let fwd_vectors = train_embeddings(&dir_cooc.forward, &self.config);

        eprintln!("  [4/6] Training BACKWARD embeddings (dim={}, iter={})...", self.config.dim, self.config.iterations);
        let bwd_vectors = train_embeddings(&dir_cooc.backward, &self.config);

        eprintln!("  [5/6] Storing directional embeddings and dictionary...");
        let embed_store = EmbeddingStore::new(self.conn);
        embed_store.init_tables()?;

        let mut embedding_count = 0;
        {
            let tx = self.conn.unchecked_transaction()?;
            let store = EmbeddingStore::new(&tx);

            // Store forward vectors with "F:" prefix
            for (id, word) in dir_cooc.forward.id_to_word.iter().enumerate() {
                let key = format!("F:{}", word);
                store.store_embedding(&key, &fwd_vectors[id])?;
                embedding_count += 1;
            }

            // Store backward vectors with "B:" prefix
            for (id, word) in dir_cooc.backward.id_to_word.iter().enumerate() {
                let key = format!("B:{}", word);
                store.store_embedding(&key, &bwd_vectors[id])?;
                embedding_count += 1;
            }

            // Also store non-directional (for fallback/compatibility)
            // Use forward vocab as base, store average of fwd+bwd where both exist
            for (id, word) in dir_cooc.forward.id_to_word.iter().enumerate() {
                if let Some(&bwd_id) = dir_cooc.backward.word_to_id.get(word) {
                    let avg: Vec<f32> = fwd_vectors[id].iter().zip(bwd_vectors[bwd_id].iter())
                        .map(|(f, b)| (f + b) / 2.0)
                        .collect();
                    store.store_embedding(word, &avg)?;
                } else {
                    store.store_embedding(word, &fwd_vectors[id])?;
                }
                embedding_count += 1;
            }
            // Backward-only words
            for (id, word) in dir_cooc.backward.id_to_word.iter().enumerate() {
                if !dir_cooc.forward.word_to_id.contains_key(word) {
                    store.store_embedding(word, &bwd_vectors[id])?;
                    embedding_count += 1;
                }
            }

            store.set_meta("dim", &self.config.dim.to_string())?;
            store.set_meta("mode", "directional")?;
            store.set_meta("window_size", &self.config.window_size.to_string())?;
            store.set_meta("iterations", &self.config.iterations.to_string())?;
            store.set_meta("corpus_sentences", &(base_corpus.len() + gen_corpus.len()).to_string())?;
            store.set_meta("generated_sentences", &gen_corpus.len().to_string())?;
            tx.commit()?;
        }

        // Populate kanji dictionary (same as build_large)
        let kanji_dict = KanjiDict::new(self.conn);
        kanji_dict.init_tables()?;
        let mut dict_count = 0;
        {
            let tx = self.conn.unchecked_transaction()?;
            let dict = KanjiDict::new(&tx);
            let mut freq_map: HashMap<(String, String), i64> = HashMap::new();
            for sentence in &base_corpus {
                for w in &sentence.words {
                    *freq_map.entry((w.reading.to_string(), w.surface.to_string())).or_insert(0) += 1;
                }
            }
            for sentence in &gen_corpus {
                for w in &sentence.words {
                    *freq_map.entry((w.reading.clone(), w.surface.clone())).or_insert(0) += 1;
                }
            }
            for ((reading, surface), freq) in &freq_map {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }
            for (reading, surface, freq) in &extra_kanji_entries() {
                dict.insert(reading, surface, *freq)?;
                dict_count += 1;
            }
            tx.commit()?;
        }

        eprintln!("  [6/6] Building hangul index...");
        self.build_hangul_index(&base_corpus)?;
        self.build_hangul_index_generated(&gen_corpus)?;

        let total_vocab = {
            let mut all_words = std::collections::HashSet::new();
            for w in &dir_cooc.forward.id_to_word { all_words.insert(w.clone()); }
            for w in &dir_cooc.backward.id_to_word { all_words.insert(w.clone()); }
            all_words.len()
        };

        eprintln!("  Done!");

        Ok(BuildStats {
            sentences: base_corpus.len() + gen_corpus.len(),
            unique_words: unique_count,
            vocab_size: total_vocab,
            embeddings: embedding_count,
            dict_entries: dict_count,
            dim: self.config.dim,
        })
    }

    /// Build hangul index from generated sentences.
    fn build_hangul_index_generated(&self, corpus: &[GenSentence]) -> Result<(), rusqlite::Error> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS hangul_index (
                hangul TEXT NOT NULL,
                reading TEXT NOT NULL,
                surface TEXT NOT NULL,
                frequency INTEGER DEFAULT 0,
                UNIQUE(hangul, reading, surface)
            );
            CREATE INDEX IF NOT EXISTS idx_hangul ON hangul_index(hangul);",
        )?;

        let mut freq_map: HashMap<(String, String, String), i64> = HashMap::new();
        for sentence in corpus {
            for w in &sentence.words {
                *freq_map
                    .entry((w.hangul.clone(), w.reading.clone(), w.surface.clone()))
                    .or_insert(0) += 1;
            }
        }

        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO hangul_index (hangul, reading, surface, frequency) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for ((hangul, reading, surface), freq) in &freq_map {
                stmt.execute(rusqlite::params![hangul, reading, surface, freq])?;
            }
        }
        tx.commit()?;

        Ok(())
    }

    /// Build an index mapping hangul input to possible readings.
    fn build_hangul_index(&self, corpus: &[CorpusSentence]) -> Result<(), rusqlite::Error> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS hangul_index (
                hangul TEXT NOT NULL,
                reading TEXT NOT NULL,
                surface TEXT NOT NULL,
                frequency INTEGER DEFAULT 0,
                UNIQUE(hangul, reading, surface)
            );
            CREATE INDEX IF NOT EXISTS idx_hangul ON hangul_index(hangul);",
        )?;

        let mut freq_map: HashMap<(&str, &str, &str), i64> = HashMap::new();
        for sentence in corpus {
            for w in &sentence.words {
                *freq_map
                    .entry((w.hangul, w.reading, w.surface))
                    .or_insert(0) += 1;
            }
        }

        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO hangul_index (hangul, reading, surface, frequency) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (&(hangul, reading, surface), &freq) in &freq_map {
                stmt.execute(rusqlite::params![hangul, reading, surface, freq * 100])?;
            }
        }
        tx.commit()?;

        Ok(())
    }
}

/// Statistics from the build process.
#[derive(Debug)]
pub struct BuildStats {
    pub sentences: usize,
    pub unique_words: usize,
    pub vocab_size: usize,
    pub embeddings: usize,
    pub dict_entries: usize,
    pub dim: usize,
}

impl std::fmt::Display for BuildStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== DB Build Statistics ===")?;
        writeln!(f, "  Corpus sentences:  {}", self.sentences)?;
        writeln!(f, "  Unique words:      {}", self.unique_words)?;
        writeln!(f, "  Vocabulary size:   {}", self.vocab_size)?;
        writeln!(f, "  Embeddings stored: {}", self.embeddings)?;
        writeln!(f, "  Dict entries:      {}", self.dict_entries)?;
        writeln!(f, "  Embedding dim:     {}", self.dim)?;
        Ok(())
    }
}

/// Extra kanji dictionary entries for common homophones.
/// These supplement the corpus-derived entries.
fn extra_kanji_entries() -> Vec<(&'static str, &'static str, i64)> {
    vec![
        // はな - 花(flower) vs 鼻(nose)
        ("はな", "花", 8000),
        ("はな", "鼻", 5000),
        // かみ - 紙(paper) vs 髪(hair) vs 神(god)
        ("かみ", "紙", 6000),
        ("かみ", "髪", 5000),
        ("かみ", "神", 4000),
        // はし - 箸(chopsticks) vs 橋(bridge) vs 端(edge)
        ("はし", "箸", 5000),
        ("はし", "橋", 4500),
        ("はし", "端", 3000),
        // あめ - 雨(rain) vs 飴(candy)
        ("あめ", "雨", 7000),
        ("あめ", "飴", 3000),
        // かわ - 川(river) vs 皮(skin/leather)
        ("かわ", "川", 6000),
        ("かわ", "皮", 3000),
        // き - 木(tree) vs 気(spirit) vs 機(machine)
        ("き", "木", 5000),
        ("き", "気", 7000),
        ("き", "機", 3000),
        // くも - 雲(cloud) vs 蜘蛛(spider)
        ("くも", "雲", 6000),
        ("くも", "蜘蛛", 2000),
        // かぜ - 風(wind) vs 風邪(cold/illness)
        ("かぜ", "風", 6000),
        ("かぜ", "風邪", 5000),
        // め - 目(eye) vs 芽(bud)
        ("め", "目", 7000),
        ("め", "芽", 2000),
        // まち - 町(town) vs 街(street)
        ("まち", "町", 5000),
        ("まち", "街", 4000),
        // おと - 音(sound) vs 夫(husband, archaic)
        ("おと", "音", 5000),
        // ひ - 日(day/sun) vs 火(fire) vs 灯(light)
        ("ひ", "日", 8000),
        ("ひ", "火", 5000),
        // こ - 子(child) vs 個(counter)
        ("こ", "子", 7000),
        ("こ", "個", 3000),
        // にわ - 庭(garden) vs 鶏(chicken)
        ("にわ", "庭", 5000),
        ("にわ", "鶏", 2000),
        // さけ - 酒(sake) vs 鮭(salmon)
        ("さけ", "酒", 5000),
        ("さけ", "鮭", 4000),
        // Common words from the corpus that should always be in dict
        ("さくら", "桜", 7000),
        ("とうきょう", "東京", 9000),
        ("きょうと", "京都", 8000),
        ("おおさか", "大阪", 8000),
        ("にほん", "日本", 9000),
        ("すし", "寿司", 7000),
        ("てんぷら", "天ぷら", 5000),
        ("しんかんせん", "新幹線", 6000),
        ("ありがとう", "ありがとう", 9000),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cooccurrence_matrix_build() {
        let corpus = build_corpus();
        let mut cooc = CooccurrenceMatrix::new();
        cooc.build_from_corpus(&corpus, 5);
        assert!(cooc.vocab_size() > 50);
        // "桜" and "花" should co-occur (both in nature sentences).
        let sakura_id = cooc.word_to_id.get("桜");
        let hana_id = cooc.word_to_id.get("花");
        assert!(sakura_id.is_some() && hana_id.is_some());
        let weight = cooc
            .matrix
            .get(sakura_id.unwrap())
            .and_then(|m| m.get(hana_id.unwrap()));
        assert!(weight.is_some(), "桜 and 花 should co-occur");
        assert!(*weight.unwrap() > 0.0);
    }

    #[test]
    fn test_train_embeddings() {
        let corpus = build_corpus();
        let mut cooc = CooccurrenceMatrix::new();
        cooc.build_from_corpus(&corpus, 5);

        let config = TrainerConfig {
            dim: 16,
            iterations: 20,
            ..Default::default()
        };
        let vectors = train_embeddings(&cooc, &config);

        assert_eq!(vectors.len(), cooc.vocab_size());
        for v in &vectors {
            assert_eq!(v.len(), 16);
            // Should be unit normalized.
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!(
                (norm - 1.0).abs() < 0.01,
                "Vector should be unit normalized, got norm={}",
                norm
            );
        }
    }

    #[test]
    fn test_semantic_similarity_after_training() {
        let corpus = build_corpus();
        let mut cooc = CooccurrenceMatrix::new();
        cooc.build_from_corpus(&corpus, 5);

        let config = TrainerConfig {
            dim: 32,
            iterations: 100,
            learning_rate: 0.03,
            ..Default::default()
        };
        let vectors = train_embeddings(&cooc, &config);

        // Words that appear in the same sentences should have
        // non-trivial similarity (not exactly 0).
        let sakura_id = cooc.word_to_id["桜"];
        let hana_id = cooc.word_to_id["花"];
        let sim = crate::embedding::cosine_similarity(
            &vectors[sakura_id],
            &vectors[hana_id],
        );
        // With a small corpus, we only verify that co-occurring words
        // produce a distinguishable similarity (not near-zero noise).
        assert!(
            sim.abs() > 0.05,
            "桜-花 similarity should be non-trivial, got {:.4}",
            sim
        );

        // Verify that the full DB build pipeline produces meaningful
        // disambiguation: this is the integration test that matters.
        let conn = Connection::open_in_memory().unwrap();
        EmbeddingStore::new(&conn).init_tables().unwrap();
        KanjiDict::new(&conn).init_tables().unwrap();

        let builder = DbBuilder::new(&conn).with_config(config);
        builder.build().unwrap();

        // After building, 花見 context should help disambiguate はな→花 vs 鼻.
        let ranker = crate::dictionary::ContextRanker::new(&conn);
        let candidates = vec![("はな".to_string(), "hana".to_string(), 0.9)];
        let ranked = ranker.rank_candidates(&candidates, &["花見"], 8000);
        assert!(
            ranked.iter().any(|r| r.surface == "花"),
            "Should find 花 in ranked results"
        );
    }

    #[test]
    fn test_full_db_build() {
        let conn = Connection::open_in_memory().unwrap();

        // Init base tables.
        let embed_store = EmbeddingStore::new(&conn);
        embed_store.init_tables().unwrap();
        let dict = KanjiDict::new(&conn);
        dict.init_tables().unwrap();

        let builder = DbBuilder::new(&conn).with_config(TrainerConfig {
            dim: 16,
            iterations: 10,
            ..Default::default()
        });

        let stats = builder.build().unwrap();
        assert!(stats.sentences > 50);
        assert!(stats.embeddings > 50);
        assert!(stats.dict_entries > 20);

        // Verify we can look up kanji.
        let results = dict.lookup("さくら").unwrap();
        assert!(
            results.iter().any(|e| e.surface == "桜"),
            "Should find 桜 for reading さくら"
        );

        // Verify embeddings exist.
        let emb = embed_store.get_embedding("桜").unwrap();
        assert!(emb.is_some());
        assert_eq!(emb.unwrap().len(), 16);
    }
}
