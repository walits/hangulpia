//! Optimal combination benchmark: tests all improvements together.
//!
//! Combines:
//!   - Phonetic: baseline vs marker+multitoken
//!   - Semantic: word-cooc vs adjacent-sentence (cw=0.2)
//!   - N-gram: with/without bigram/trigram scoring
//!   - Ranking weights: α(phoneme), β(context), γ(freq), δ(ngram)
//!
//! Tests a focused grid of the most promising combinations.

use ime_db::generator::{generate_corpus_with_seed, GenSentence};
use ime_db::ngram::{NgramModel, build_ngram_model_chunked};
use ime_db::phonetic_decoder::*;
use ime_db::vocab::build_full_vocab;
use ime_db::{DbBuilder, DictionaryDb, TrainerConfig};
use ime_db::{cosine_similarity, weighted_average_vectors};
use std::time::Instant;

#[derive(Debug, Clone, Default)]
struct Metrics {
    total_words: usize,
    total_docs: usize,
    exact_match: usize,
    reading_match: usize,
    top3_hit: usize,
    top10_hit: usize,
    full_doc_match: usize,
    hiragana_hit: usize,
    hiragana_top1: usize,
}

impl Metrics {
    fn pct(&self, n: usize, d: usize) -> f64 {
        if d == 0 { 0.0 } else { n as f64 / d as f64 * 100.0 }
    }
    fn exact_pct(&self) -> f64 { self.pct(self.exact_match, self.total_words) }
    fn reading_pct(&self) -> f64 { self.pct(self.reading_match, self.total_words) }
    fn top3_pct(&self) -> f64 { self.pct(self.top3_hit, self.total_words) }
    fn top10_pct(&self) -> f64 { self.pct(self.top10_hit, self.total_words) }
    fn doc_pct(&self) -> f64 { self.pct(self.full_doc_match, self.total_docs) }
    fn hira_top1_pct(&self) -> f64 { self.pct(self.hiragana_top1, self.total_words) }
    fn hira_hit_pct(&self) -> f64 { self.pct(self.hiragana_hit, self.total_words) }
}

/// 4-factor ranking with n-gram
fn rank_4factor(
    candidates: &[(String, String, f64)],
    context_words: &[&str],
    db: &DictionaryDb,
    ngram: &NgramModel,
    alpha: f64, beta: f64, gamma: f64, delta: f64,
) -> Vec<(String, String, f64)> {
    let embed_store = db.embedding_store();
    let kanji_dict = db.kanji_dict();
    let max_freq_f = 10000.0;

    let context_embeddings: Vec<(Vec<f32>, f32)> = context_words.iter().enumerate()
        .filter_map(|(i, &word)| {
            embed_store.get_embedding(word).ok().flatten().map(|emb| {
                let distance = (context_words.len() - i) as f32;
                (emb, 1.0 / distance)
            })
        }).collect();

    let context_vector = if context_embeddings.is_empty() { None } else {
        let (vecs, weights): (Vec<Vec<f32>>, Vec<f32>) = context_embeddings.into_iter().unzip();
        let avg = weighted_average_vectors(&vecs, &weights);
        if avg.iter().all(|&v| v.abs() < 1e-10) { None } else { Some(avg) }
    };

    let ngram_ctx: Vec<&str> = context_words.iter().rev().take(2).rev().copied().collect();

    let mut ranked: Vec<(String, String, f64)> = Vec::new();

    for (hiragana, _, phoneme_conf) in candidates {
        let kanji_entries = kanji_dict.lookup(hiragana).unwrap_or_default();
        let mut entries: Vec<(String, i64)> = kanji_entries.into_iter()
            .map(|e| (e.surface, e.frequency)).collect();
        if !entries.iter().any(|(s, _)| s == hiragana) {
            entries.push((hiragana.clone(), 0));
        }

        for (surface, freq) in entries {
            let ctx_score = context_vector.as_ref()
                .and_then(|ctx| embed_store.get_embedding(&surface).ok().flatten()
                    .map(|emb| (cosine_similarity(&emb, ctx) + 1.0) / 2.0))
                .unwrap_or(0.5);
            let freq_score = freq as f64 / max_freq_f;
            let ng_score = ngram.normalized_score(&ngram_ctx, &surface);

            let score = alpha * phoneme_conf + beta * ctx_score + gamma * freq_score + delta * ng_score;
            ranked.push((surface, hiragana.clone(), score));
        }
    }

    ranked.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let mut seen = std::collections::HashSet::new();
    ranked.retain(|(s, _, _)| seen.insert(s.clone()));
    ranked
}

enum PhoneticMode<'a> {
    Baseline(&'a PhoneticMap),
    MarkerMultitoken(&'a PhoneticMap),
}

fn get_hira_candidates(mode: &PhoneticMode, hangul: &str, reading: &str) -> Vec<(String, String, f64)> {
    match mode {
        PhoneticMode::Baseline(map) => {
            let decoder = BeamDecoder::new(map, 8, 20);
            decoder.decode(hangul).into_iter().map(|(h, c)| (h, String::new(), c)).collect()
        }
        PhoneticMode::MarkerMultitoken(map) => {
            let marked = hiragana_to_hangul_marked(reading);
            let decoder = BeamDecoder::new(map, 8, 20);
            decoder.decode(&marked).into_iter().map(|(h, c)| (h, String::new(), c)).collect()
        }
    }
}

fn benchmark_arm(
    test_sentences: &[GenSentence],
    db: &DictionaryDb,
    ngram: &NgramModel,
    phonetic: &PhoneticMode,
    alpha: f64, beta: f64, gamma: f64, delta: f64,
) -> Metrics {
    let mut total = Metrics { total_docs: test_sentences.len(), ..Default::default() };

    for sentence in test_sentences {
        let mut confirmed: Vec<String> = Vec::new();
        let mut doc_exact = 0usize;
        let mut doc_words = 0usize;

        for word in &sentence.words {
            total.total_words += 1;
            doc_words += 1;

            let candidates = get_hira_candidates(phonetic, &word.hangul, &word.reading);
            if candidates.is_empty() { confirmed.push(word.surface.clone()); continue; }

            if candidates.iter().any(|(h, _, _)| *h == word.reading) { total.hiragana_hit += 1; }
            if candidates.first().map(|(h, _, _)| h.as_str()) == Some(word.reading.as_str()) { total.hiragana_top1 += 1; }

            let ctx: Vec<&str> = confirmed.iter().map(|s| s.as_str()).collect();
            let ranked = rank_4factor(&candidates, &ctx, db, ngram, alpha, beta, gamma, delta);

            if let Some(top) = ranked.first() {
                if top.0 == word.surface { total.exact_match += 1; doc_exact += 1; }
                if top.1 == word.reading { total.reading_match += 1; }
            }
            if ranked.iter().take(3).any(|r| r.0 == word.surface) { total.top3_hit += 1; }
            if ranked.iter().take(10).any(|r| r.0 == word.surface) { total.top10_hit += 1; }

            confirmed.push(word.surface.clone());
        }
        if doc_exact == doc_words { total.full_doc_match += 1; }
    }
    total
}

#[derive(Clone)]
struct ArmConfig {
    label: String,
    use_marker_mt: bool,
    use_adjacent: bool,
    alpha: f64, beta: f64, gamma: f64, delta: f64,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let train_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(500_000);
    let test_count: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(500);

    let vocab = build_full_vocab();

    println!("╔═══════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║    최적 조합 벤치마크 — 음소×의미×N-gram×가중치 통합 실험                                ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  어휘: {}, 학습: {}K, 테스트: {} 건", vocab.len(), train_count / 1000, test_count);
    println!();

    let global_start = Instant::now();

    // ── Build infrastructure ────────────────────────────────────────────
    println!("━━━ 인프라 구축 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // DB 1: word-level (baseline semantic)
    print!("  [1] 어휘 공출현 DB...");
    let t = Instant::now();
    let db_word = DictionaryDb::open_in_memory().expect("db");
    let cfg = TrainerConfig { dim: 64, iterations: 50, ..Default::default() };
    DbBuilder::new(db_word.conn()).with_config(cfg.clone())
        .build_large_with_vocab_chunked(&vocab, train_count).expect("build");
    println!(" {:.1}s", t.elapsed().as_secs_f64());

    // DB 2: adjacent-sentence (cw=0.2)
    print!("  [2] 인접문장 DB (cw=0.2)...");
    let t = Instant::now();
    let db_adj = DictionaryDb::open_in_memory().expect("db");
    DbBuilder::new(db_adj.conn()).with_config(cfg)
        .build_large_with_adjacent_sentences(&vocab, train_count, 0.2).expect("build");
    println!(" {:.1}s", t.elapsed().as_secs_f64());

    // Phonetic maps
    print!("  [3] 기존 음소 맵...");
    let t = Instant::now();
    let map_base = build_phonetic_map_chunked(&vocab, train_count);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), map_base.stats());

    print!("  [4] 마커+멀티토큰 맵...");
    let t = Instant::now();
    let map_combo = build_phonetic_map_marked_multitoken_chunked(&vocab, train_count);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), map_combo.stats());

    // N-gram
    print!("  [5] N-gram 모델...");
    let t = Instant::now();
    let ngram = build_ngram_model_chunked(&vocab, train_count);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), ngram.stats());

    // Empty n-gram (for no-ngram arms)
    let ngram_empty = NgramModel::new();

    // Test data
    let test_sentences = generate_corpus_with_seed(&vocab, test_count, 99);
    println!("  [6] 테스트: {} 건", test_sentences.len());
    println!();

    // ── Define arms ─────────────────────────────────────────────────────
    let arms = vec![
        // Baseline progression
        ArmConfig { label: "①초기(어휘+기존음소)".into(),
            use_marker_mt: false, use_adjacent: false,
            alpha: 0.3, beta: 0.5, gamma: 0.2, delta: 0.0 },
        ArmConfig { label: "②+인접문장".into(),
            use_marker_mt: false, use_adjacent: true,
            alpha: 0.3, beta: 0.5, gamma: 0.2, delta: 0.0 },
        ArmConfig { label: "③+인접+γ최적".into(),
            use_marker_mt: false, use_adjacent: true,
            alpha: 0.1, beta: 0.5, gamma: 0.4, delta: 0.0 },
        ArmConfig { label: "④+인접+ngram".into(),
            use_marker_mt: false, use_adjacent: true,
            alpha: 0.1, beta: 0.35, gamma: 0.25, delta: 0.3 },

        // Marker+multitoken variants
        ArmConfig { label: "⑤마커MT+인접+ng".into(),
            use_marker_mt: true, use_adjacent: true,
            alpha: 0.1, beta: 0.35, gamma: 0.25, delta: 0.3 },

        // Fine-tune δ with marker+MT
        ArmConfig { label: "⑥마커MT adj δ=0.2".into(),
            use_marker_mt: true, use_adjacent: true,
            alpha: 0.1, beta: 0.4, gamma: 0.3, delta: 0.2 },
        ArmConfig { label: "⑦마커MT adj δ=0.4".into(),
            use_marker_mt: true, use_adjacent: true,
            alpha: 0.1, beta: 0.25, gamma: 0.25, delta: 0.4 },

        // Push β higher (more context emphasis with adjacent-sentence)
        ArmConfig { label: "⑧adj β=0.4 δ=0.3".into(),
            use_marker_mt: false, use_adjacent: true,
            alpha: 0.1, beta: 0.4, gamma: 0.2, delta: 0.3 },

        // Extreme ngram
        ArmConfig { label: "⑨adj δ=0.5 극단".into(),
            use_marker_mt: false, use_adjacent: true,
            alpha: 0.05, beta: 0.2, gamma: 0.25, delta: 0.5 },

        // Low alpha (phoneme barely matters since ~90% accuracy)
        ArmConfig { label: "⑩α=0 adj+ng".into(),
            use_marker_mt: false, use_adjacent: true,
            alpha: 0.0, beta: 0.35, gamma: 0.3, delta: 0.35 },
    ];

    // ── Run ─────────────────────────────────────────────────────────────
    println!("━━━ 벤치마크 실행 ({} 설정) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
        arms.len());

    let mut results: Vec<(ArmConfig, Metrics, f64)> = Vec::new();

    for (idx, arm) in arms.iter().enumerate() {
        eprint!("  [{:>2}/{}] {:>22}...", idx + 1, arms.len(), arm.label);
        let t = Instant::now();

        let db = if arm.use_adjacent { &db_adj } else { &db_word };
        let ng = if arm.delta > 0.0 { &ngram } else { &ngram_empty };
        let phon = if arm.use_marker_mt {
            PhoneticMode::MarkerMultitoken(&map_combo)
        } else {
            PhoneticMode::Baseline(&map_base)
        };

        let m = benchmark_arm(&test_sentences, db, ng, &phon,
            arm.alpha, arm.beta, arm.gamma, arm.delta);
        let elapsed = t.elapsed().as_secs_f64();

        eprintln!(" Top1={:.1}% Top3={:.1}% Doc={:.1}% {:.1}s",
            m.exact_pct(), m.top3_pct(), m.doc_pct(), elapsed);
        results.push((arm.clone(), m, elapsed));
    }

    // ── Results ─────────────────────────────────────────────────────────
    let baseline = results[0].1.exact_pct();

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                                     최적 조합 벤치마크 결과                                                        ║");
    println!("╠════════════════════════════╤════════╤════════╤════════╤════════╤════════╤════════╤════════╤═════════╤════════════════╣");
    println!("║ 설정                       │ Hira   │ Hira   │ Top-1  │ Read   │ Top-3  │ Top-10 │ Doc    │  시간   │ Δ vs ①        ║");
    println!("║                            │ Top-1  │ Hit    │ Surf   │ Match  │ Hit    │ Hit    │ Match  │         │               ║");
    println!("╠════════════════════════════╪════════╪════════╪════════╪════════╪════════╪════════╪════════╪═════════╪════════════════╣");

    for (arm, m, secs) in &results {
        let d = m.exact_pct() - baseline;
        let ds = if d.abs() < 0.05 { "    기준".into() } else { format!("{:>+7.1}%p", d) };
        println!("  {:>26} │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}s  │ {:>14} │",
            arm.label,
            m.hira_top1_pct(), m.hira_hit_pct(),
            m.exact_pct(), m.reading_pct(),
            m.top3_pct(), m.top10_pct(),
            m.doc_pct(), secs, ds);
    }
    println!("╚════════════════════════════╧════════╧════════╧════════╧════════╧════════╧════════╧════════╧═════════╧════════════════╝");

    // Best
    let best = results.iter().max_by(|a, b|
        a.1.exact_pct().partial_cmp(&b.1.exact_pct()).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
    println!();
    println!("  ★ 최적: {} — Top-1 {:.1}% (Δ{:>+.1}%p vs ①)",
        best.0.label, best.1.exact_pct(), best.1.exact_pct() - baseline);
    println!("    α={}, β={}, γ={}, δ={}, 마커MT={}, 인접문장={}",
        best.0.alpha, best.0.beta, best.0.gamma, best.0.delta,
        best.0.use_marker_mt, best.0.use_adjacent);

    // Bar chart
    println!();
    println!("  Top-1 Surface 정확도:");
    let mut sorted: Vec<_> = results.iter().collect();
    sorted.sort_by(|a, b| b.1.exact_pct().partial_cmp(&a.1.exact_pct()).unwrap_or(std::cmp::Ordering::Equal));
    for (arm, m, _) in &sorted {
        let bar = (m.exact_pct() * 0.4) as usize;
        println!("    {:>26} │{} {:.1}% ({:>+.1}%p)",
            arm.label, "█".repeat(bar), m.exact_pct(), m.exact_pct() - baseline);
    }

    println!();
    println!("  문서 전체 일치율:");
    sorted.sort_by(|a, b| b.1.doc_pct().partial_cmp(&a.1.doc_pct()).unwrap_or(std::cmp::Ordering::Equal));
    for (arm, m, _) in &sorted {
        let bar = (m.doc_pct() * 0.8) as usize;
        println!("    {:>26} │{} {:.1}%", arm.label, "█".repeat(bar), m.doc_pct());
    }

    println!();
    println!("  총 소요시간: {:.1}s", global_start.elapsed().as_secs_f64());
}
