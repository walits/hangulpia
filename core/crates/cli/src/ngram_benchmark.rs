//! N-gram language model benchmark.
//!
//! Tests adding bigram/trigram scoring to the ranking pipeline.
//! Compares multiple configurations:
//!   A: 기존 (α=0.1, β=0.5, γ=0.4, δ=0.0) — previous best from sweep
//!   B~F: δ weight sweep for n-gram contribution
//!
//! Scoring formula:
//!   final = α*phoneme + β*context + γ*freq + δ*ngram
//!
//! The n-gram model captures sequential patterns (e.g., "今日の天気" is
//! more natural than "今日の店記"), which embedding similarity can miss.

use ime_db::generator::{generate_corpus_with_seed, generate_corpus_chunked, GenSentence};
use ime_db::ngram::{NgramModel, build_ngram_model_chunked};
use ime_db::phonetic_decoder::*;
use ime_db::vocab::build_full_vocab;
use ime_db::{DbBuilder, DictionaryDb, TrainerConfig};
use ime_db::dictionary::KanjiDict;
use ime_db::embedding::EmbeddingStore;
use ime_db::{cosine_similarity, weighted_average_vectors};
use std::time::Instant;

#[derive(Debug, Clone, Default)]
struct Metrics {
    total_words: usize,
    total_docs: usize,
    exact_match: usize,
    reading_match: usize,
    top3_hit: usize,
    top5_hit: usize,
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

/// Custom 4-factor ranker: phoneme + context + freq + ngram
fn rank_with_ngram(
    candidates: &[(String, String, f64)],
    context_words: &[&str],
    db: &DictionaryDb,
    ngram: &NgramModel,
    alpha: f64,
    beta: f64,
    gamma: f64,
    delta: f64,
    max_freq: i64,
) -> Vec<(String, String, f64)> {
    let embed_store = db.embedding_store();
    let kanji_dict = db.kanji_dict();
    let max_freq_f = if max_freq > 0 { max_freq as f64 } else { 1.0 };

    // Build context vector
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
        if avg.iter().all(|&v| v.abs() < 1e-10) { None } else { Some(avg) }
    };

    // N-gram context: last 2 confirmed surface forms
    let ngram_ctx: Vec<&str> = if context_words.len() >= 2 {
        vec![context_words[context_words.len() - 2], context_words[context_words.len() - 1]]
    } else if context_words.len() == 1 {
        vec![context_words[0]]
    } else {
        vec![]
    };

    let mut ranked: Vec<(String, String, f64)> = Vec::new();

    for (hiragana, _romaji, phoneme_conf) in candidates {
        let kanji_entries = kanji_dict.lookup(hiragana).unwrap_or_default();
        let mut entries: Vec<(String, i64)> = kanji_entries
            .into_iter()
            .map(|e| (e.surface, e.frequency))
            .collect();
        if !entries.iter().any(|(s, _)| s == hiragana) {
            entries.push((hiragana.clone(), 0));
        }

        for (surface, freq) in entries {
            let context_score = context_vector
                .as_ref()
                .and_then(|ctx| {
                    embed_store.get_embedding(&surface).ok().flatten().map(|emb| {
                        (cosine_similarity(&emb, ctx) + 1.0) / 2.0
                    })
                })
                .unwrap_or(0.5);

            let freq_score = freq as f64 / max_freq_f;

            let ngram_score = ngram.normalized_score(&ngram_ctx, &surface);

            let final_score = alpha * phoneme_conf
                + beta * context_score
                + gamma * freq_score
                + delta * ngram_score;

            ranked.push((surface, hiragana.clone(), final_score));
        }
    }

    ranked.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    // Dedup by surface
    let mut seen = std::collections::HashSet::new();
    ranked.retain(|(s, _, _)| seen.insert(s.clone()));
    ranked
}

fn benchmark_ngram_arm(
    label: &str,
    test_sentences: &[GenSentence],
    db: &DictionaryDb,
    ngram: &NgramModel,
    phonetic_map: &PhoneticMap,
    alpha: f64,
    beta: f64,
    gamma: f64,
    delta: f64,
) -> Metrics {
    let mut total = Metrics { total_docs: test_sentences.len(), ..Default::default() };

    for sentence in test_sentences {
        let mut confirmed: Vec<String> = Vec::new();
        let mut doc_exact = 0usize;
        let mut doc_words = 0usize;

        for word in &sentence.words {
            total.total_words += 1;
            doc_words += 1;

            let decoder = BeamDecoder::new(phonetic_map, 8, 20);
            let hira_candidates: Vec<(String, String, f64)> = decoder.decode(&word.hangul)
                .into_iter()
                .map(|(h, c)| (h, String::new(), c))
                .collect();

            if hira_candidates.is_empty() {
                confirmed.push(word.surface.clone());
                continue;
            }

            if hira_candidates.iter().any(|(h, _, _)| *h == word.reading) {
                total.hiragana_hit += 1;
            }
            if hira_candidates.first().map(|(h, _, _)| h.as_str()) == Some(&word.reading as &str) {
                total.hiragana_top1 += 1;
            }

            let ctx: Vec<&str> = confirmed.iter().map(|s| s.as_str()).collect();
            let ranked = rank_with_ngram(
                &hira_candidates, &ctx, db, ngram,
                alpha, beta, gamma, delta, 10000,
            );

            if let Some(top) = ranked.first() {
                if top.0 == word.surface { total.exact_match += 1; doc_exact += 1; }
                if top.1 == word.reading { total.reading_match += 1; }
            }
            if ranked.iter().take(3).any(|r| r.0 == word.surface) { total.top3_hit += 1; }
            if ranked.iter().take(5).any(|r| r.0 == word.surface) { total.top5_hit += 1; }
            if ranked.iter().take(10).any(|r| r.0 == word.surface) { total.top10_hit += 1; }

            confirmed.push(word.surface.clone());
        }

        if doc_exact == doc_words { total.full_doc_match += 1; }
    }
    total
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let train_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(200_000);
    let test_count: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(300);

    let vocab = build_full_vocab();

    println!("╔═══════════════════════════════════════════════════════════════════════════════════╗");
    println!("║    N-gram 언어모델 도입 벤치마크                                                  ║");
    println!("║    score = α·phoneme + β·context + γ·freq + δ·ngram                              ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  어휘: {}, 학습: {}K, 테스트: {} 건", vocab.len(), train_count / 1000, test_count);
    println!();

    let global_start = Instant::now();

    // ── Step 1: Build context DB (with adjacent-sentence, cw=0.2) ───────
    println!("━━━ 컨텍스트 DB 구축 (인접문장 cw=0.2) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let t = Instant::now();
    let db = DictionaryDb::open_in_memory().expect("open_in_memory");
    let config = TrainerConfig { dim: 64, iterations: 50, ..Default::default() };
    let builder = DbBuilder::new(db.conn()).with_config(config);
    builder.build_large_with_adjacent_sentences(&vocab, train_count, 0.2)
        .expect("build failed");
    println!("  구축 완료: {:.1}s", t.elapsed().as_secs_f64());
    println!();

    // ── Step 2: Build phonetic map ──────────────────────────────────────
    print!("  음소 맵 구축...");
    let t = Instant::now();
    let phonetic_map = build_phonetic_map_chunked(&vocab, train_count);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), phonetic_map.stats());

    // ── Step 3: Build n-gram model ──────────────────────────────────────
    print!("  N-gram 모델 구축...");
    let t = Instant::now();
    let ngram = build_ngram_model_chunked(&vocab, train_count);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), ngram.stats());
    println!();

    // Show some n-gram examples
    println!("  N-gram 예시:");
    let examples = [
        (&["今日"][..], "の"),
        (&["今日", "の"][..], "天気"),
        (&["東京"][..], "の"),
        (&["<BOS>"][..], "今日"),
    ];
    for (ctx, word) in &examples {
        let p = ngram.score(ctx, word);
        let ns = ngram.normalized_score(ctx, word);
        println!("    P({} | {}) = {:.6} (norm={:.3})", word, ctx.join(","), p, ns);
    }
    println!();

    // ── Step 4: Generate test data ──────────────────────────────────────
    let test_sentences = generate_corpus_with_seed(&vocab, test_count, 99);
    println!("  테스트 데이터: {} 건", test_sentences.len());
    println!();

    // ── Step 5: Sweep δ weights ─────────────────────────────────────────
    println!("━━━ δ weight 스위프 실행 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Base weights from previous sweep: α=0.1, β=0.5, γ=0.4
    // When adding δ, we redistribute: sum should ≈ 1.0
    struct ArmConfig {
        label: &'static str,
        alpha: f64,
        beta: f64,
        gamma: f64,
        delta: f64,
    }

    let arms = vec![
        ArmConfig { label: "δ=0.0 (기존)", alpha: 0.1, beta: 0.5, gamma: 0.4, delta: 0.0 },
        ArmConfig { label: "δ=0.1 (-γ)", alpha: 0.1, beta: 0.5, gamma: 0.3, delta: 0.1 },
        ArmConfig { label: "δ=0.2 (-γ)", alpha: 0.1, beta: 0.5, gamma: 0.2, delta: 0.2 },
        ArmConfig { label: "δ=0.2 (-β)", alpha: 0.1, beta: 0.3, gamma: 0.4, delta: 0.2 },
        ArmConfig { label: "δ=0.3 (-βγ)", alpha: 0.1, beta: 0.35, gamma: 0.25, delta: 0.3 },
        ArmConfig { label: "δ=0.4 (-βγ)", alpha: 0.1, beta: 0.25, gamma: 0.25, delta: 0.4 },
        ArmConfig { label: "δ=0.1 add", alpha: 0.1, beta: 0.5, gamma: 0.4, delta: 0.1 },
        ArmConfig { label: "δ=0.2 add", alpha: 0.1, beta: 0.5, gamma: 0.4, delta: 0.2 },
    ];

    let mut results: Vec<(&str, Metrics, f64)> = Vec::new();

    for (idx, arm) in arms.iter().enumerate() {
        eprint!("  [{}/{}] {:>14}...", idx + 1, arms.len(), arm.label);
        let t = Instant::now();
        let m = benchmark_ngram_arm(
            arm.label, &test_sentences, &db, &ngram, &phonetic_map,
            arm.alpha, arm.beta, arm.gamma, arm.delta,
        );
        let elapsed = t.elapsed().as_secs_f64();
        eprintln!(" Top-1={:.1}%, Top-3={:.1}%, Doc={:.1}%, {:.1}s",
            m.exact_pct(), m.top3_pct(), m.doc_pct(), elapsed);
        results.push((arm.label, m, elapsed));
    }

    // ── Results table ───────────────────────────────────────────────────
    println!();
    println!("╔════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                              N-gram 언어모델 도입 결과                                            ║");
    println!("╠════════════════════╤════════╤════════╤════════╤════════╤════════╤════════╤════════╤═══════════════╣");
    println!("║ 설정               │ Hira   │ Hira   │ Top-1  │ Read   │ Top-3  │ Top-10 │  Doc   │ Δ vs 기존    ║");
    println!("║                    │ Top-1  │ Hit    │ Surf   │ Match  │ Hit    │ Hit    │ Match  │               ║");
    println!("╠════════════════════╪════════╪════════╪════════╪════════╪════════╪════════╪════════╪═══════════════╣");

    let baseline_exact = results[0].1.exact_pct();

    for (label, m, _) in &results {
        let delta = m.exact_pct() - baseline_exact;
        let delta_str = if delta.abs() < 0.05 { "   기준".to_string() } else { format!("{:>+6.1}%p", delta) };
        println!("  {:>18} │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>13} │",
            label,
            m.hira_top1_pct(), m.hira_hit_pct(),
            m.exact_pct(), m.reading_pct(),
            m.top3_pct(), m.top10_pct(),
            m.doc_pct(), delta_str);
    }
    println!("╚════════════════════╧════════╧════════╧════════╧════════╧════════╧════════╧════════╧═══════════════╝");

    // Bar chart
    println!();
    println!("  Top-1 Surface 정확도:");
    let mut sorted: Vec<_> = results.iter().collect();
    sorted.sort_by(|a, b| b.1.exact_pct().partial_cmp(&a.1.exact_pct()).unwrap_or(std::cmp::Ordering::Equal));
    for (label, m, _) in &sorted {
        let bar_len = (m.exact_pct() * 0.5) as usize;
        let delta = m.exact_pct() - baseline_exact;
        println!("    {:>18} │{} {:.1}% ({:>+.1}%p)",
            label, "█".repeat(bar_len), m.exact_pct(), delta);
    }

    println!();
    println!("  Top-3 Hit:");
    sorted.sort_by(|a, b| b.1.top3_pct().partial_cmp(&a.1.top3_pct()).unwrap_or(std::cmp::Ordering::Equal));
    for (label, m, _) in &sorted {
        let bar_len = (m.top3_pct() * 0.5) as usize;
        println!("    {:>18} │{} {:.1}%", label, "█".repeat(bar_len), m.top3_pct());
    }

    println!();
    println!("  총 소요시간: {:.1}s", global_start.elapsed().as_secs_f64());
}
