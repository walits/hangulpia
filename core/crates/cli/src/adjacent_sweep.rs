//! Adjacent-sentence embedding parameter sweep benchmark.
//!
//! Sweeps across multiple tuning axes to find optimal configuration:
//!   1. cross_weight: [0.2, 0.4, 0.6, 0.8, 1.0]
//!   2. sentence_range: [1, 2, 3]
//!   3. embedding dim: [64, 128]
//!   4. window_size: [3, 5, 8]
//!   5. ranking weights (alpha, beta, gamma)
//!   6. training iterations: [50, 100]

use ime_db::generator::generate_corpus_with_seed;
use ime_db::phonetic_decoder::*;
use ime_db::vocab::build_full_vocab;
use ime_db::{DbBuilder, DictionaryDb, TrainerConfig};
use std::time::Instant;

#[derive(Debug, Clone)]
struct SweepConfig {
    label: String,
    cross_weight: f64,
    sentence_range: usize,
    dim: usize,
    window_size: usize,
    iterations: usize,
    alpha: f64,  // phoneme weight
    beta: f64,   // context weight
    gamma: f64,  // freq weight
}

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
    fn hira_hit_pct(&self) -> f64 { self.pct(self.hiragana_hit, self.total_words) }
    fn hira_top1_pct(&self) -> f64 { self.pct(self.hiragana_top1, self.total_words) }
}

use ime_db::generator::GenSentence;
use ime_db::dictionary::ContextRanker;

fn benchmark_with_ranker(
    label: &str,
    test_sentences: &[GenSentence],
    ranker: &ContextRanker,
    map: &PhoneticMap,
) -> Metrics {
    let mut total = Metrics { total_docs: test_sentences.len(), ..Default::default() };

    for sentence in test_sentences {
        let mut confirmed: Vec<String> = Vec::new();
        let mut doc_exact = 0usize;
        let mut doc_words = 0usize;

        for word in &sentence.words {
            total.total_words += 1;
            doc_words += 1;

            let decoder = BeamDecoder::new(map, 8, 20);
            let candidates: Vec<(String, String, f64)> = decoder.decode(&word.hangul)
                .into_iter()
                .map(|(h, c)| (h, String::new(), c))
                .collect();

            if candidates.is_empty() {
                confirmed.push(word.surface.clone());
                continue;
            }

            if candidates.iter().any(|(h, _, _)| *h == word.reading) {
                total.hiragana_hit += 1;
            }
            if candidates.first().map(|(h, _, _)| h.as_str()) == Some(&word.reading as &str) {
                total.hiragana_top1 += 1;
            }

            let ctx: Vec<&str> = confirmed.iter().map(|s| s.as_str()).collect();
            let ranked = ranker.rank_candidates(&candidates, &ctx, 10000);

            if let Some(top) = ranked.first() {
                if top.surface == word.surface { total.exact_match += 1; doc_exact += 1; }
                if top.reading == word.reading { total.reading_match += 1; }
            }
            if ranked.iter().take(3).any(|r| r.surface == word.surface) { total.top3_hit += 1; }
            if ranked.iter().take(5).any(|r| r.surface == word.surface) { total.top5_hit += 1; }
            if ranked.iter().take(10).any(|r| r.surface == word.surface) { total.top10_hit += 1; }

            confirmed.push(word.surface.clone());
        }

        if doc_exact == doc_words { total.full_doc_match += 1; }
    }
    total
}

fn run_sweep_arm(
    config: &SweepConfig,
    vocab: &[ime_db::vocab::VocabEntry],
    train_count: usize,
    test_sentences: &[GenSentence],
    phonetic_map: &PhoneticMap,
) -> (Metrics, f64) {
    let t = Instant::now();

    // Build DB with this config
    let db = DictionaryDb::open_in_memory().expect("open_in_memory");
    let trainer_config = TrainerConfig {
        dim: config.dim,
        window_size: config.window_size,
        iterations: config.iterations,
        ..Default::default()
    };
    let builder = DbBuilder::new(db.conn()).with_config(trainer_config);

    if config.cross_weight == 0.0 {
        // Baseline: no adjacent-sentence
        builder.build_large_with_vocab_chunked(vocab, train_count).expect("build failed");
    } else {
        builder.build_large_with_adjacent_sentences_range(
            vocab, train_count, config.cross_weight, config.sentence_range
        ).expect("build failed");
    }

    // Create ranker with custom weights
    let ranker = ContextRanker::new(db.conn())
        .with_weights(config.alpha, config.beta, config.gamma);

    let metrics = benchmark_with_ranker(&config.label, test_sentences, &ranker, phonetic_map);
    let elapsed = t.elapsed().as_secs_f64();

    (metrics, elapsed)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let train_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(200_000);
    let test_count: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(300);
    let phase: usize = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(0); // 0=all, 1=phase1, 2=phase2

    let vocab = build_full_vocab();

    println!("╔═══════════════════════════════════════════════════════════════════════════════════╗");
    println!("║    인접문장 임베딩 파라미터 스위프 벤치마크                                         ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  어휘: {}, 학습: {}K, 테스트: {} 건", vocab.len(), train_count / 1000, test_count);
    println!();

    let global_start = Instant::now();

    // Build shared phonetic map (same for all arms since we're testing semantic side)
    print!("  음소 맵 구축...");
    let t = Instant::now();
    let phonetic_map = build_phonetic_map_chunked(&vocab, train_count);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), phonetic_map.stats());
    println!();

    // Generate test data
    let test_sentences = generate_corpus_with_seed(&vocab, test_count, 99);
    println!("  테스트 데이터: {} 건", test_sentences.len());
    println!();

    // ── Define sweep configurations ─────────────────────────────────────
    // Split into phases to avoid timeout
    let mut configs: Vec<SweepConfig> = Vec::new();

    if phase == 0 || phase == 1 {
        // Phase 1: cross_weight + baseline + range + window
        configs.push(SweepConfig {
            label: "baseline".into(),
            cross_weight: 0.0, sentence_range: 0,
            dim: 64, window_size: 5, iterations: 50,
            alpha: 0.3, beta: 0.5, gamma: 0.2,
        });
        for &cw in &[0.2, 0.4, 0.8] {
            configs.push(SweepConfig {
                label: format!("cw={:.1}", cw),
                cross_weight: cw, sentence_range: 1,
                dim: 64, window_size: 5, iterations: 50,
                alpha: 0.3, beta: 0.5, gamma: 0.2,
            });
        }
        configs.push(SweepConfig {
            label: "range=2".into(),
            cross_weight: 0.4, sentence_range: 2,
            dim: 64, window_size: 5, iterations: 50,
            alpha: 0.3, beta: 0.5, gamma: 0.2,
        });
        configs.push(SweepConfig {
            label: "win=8".into(),
            cross_weight: 0.4, sentence_range: 1,
            dim: 64, window_size: 8, iterations: 50,
            alpha: 0.3, beta: 0.5, gamma: 0.2,
        });
    }

    if phase == 0 || phase == 2 {
        // Phase 2: dim + iter + ranking weights
        if phase == 2 {
            // Need baseline for delta calc
            configs.push(SweepConfig {
                label: "baseline".into(),
                cross_weight: 0.0, sentence_range: 0,
                dim: 64, window_size: 5, iterations: 50,
                alpha: 0.3, beta: 0.5, gamma: 0.2,
            });
            configs.push(SweepConfig {
                label: "cw=0.2(ref)".into(),
                cross_weight: 0.2, sentence_range: 1,
                dim: 64, window_size: 5, iterations: 50,
                alpha: 0.3, beta: 0.5, gamma: 0.2,
            });
        }
        configs.push(SweepConfig {
            label: "dim=128".into(),
            cross_weight: 0.2, sentence_range: 1,
            dim: 128, window_size: 5, iterations: 50,
            alpha: 0.3, beta: 0.5, gamma: 0.2,
        });
        configs.push(SweepConfig {
            label: "iter=100".into(),
            cross_weight: 0.2, sentence_range: 1,
            dim: 64, window_size: 5, iterations: 100,
            alpha: 0.3, beta: 0.5, gamma: 0.2,
        });
        configs.push(SweepConfig {
            label: "β=0.7".into(),
            cross_weight: 0.2, sentence_range: 1,
            dim: 64, window_size: 5, iterations: 50,
            alpha: 0.1, beta: 0.7, gamma: 0.2,
        });
        configs.push(SweepConfig {
            label: "γ=0.4".into(),
            cross_weight: 0.2, sentence_range: 1,
            dim: 64, window_size: 5, iterations: 50,
            alpha: 0.1, beta: 0.5, gamma: 0.4,
        });
    }

    // ── Run all configurations ──────────────────────────────────────────
    println!("━━━ 스위프 실행 ({} 설정) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
        configs.len());
    println!();

    let mut results: Vec<(SweepConfig, Metrics, f64)> = Vec::new();

    for (idx, config) in configs.iter().enumerate() {
        eprint!("  [{}/{}] {:>20}...", idx + 1, configs.len(), config.label);
        let (metrics, elapsed) = run_sweep_arm(config, &vocab, train_count, &test_sentences, &phonetic_map);
        eprintln!(" Top-1={:.1}%, Top-3={:.1}%, Doc={:.1}%, {:.1}s",
            metrics.exact_pct(), metrics.top3_pct(), metrics.doc_pct(), elapsed);
        results.push((config.clone(), metrics, elapsed));
    }

    // ── Results table ───────────────────────────────────────────────────
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                            인접문장 파라미터 스위프 결과                                                 ║");
    println!("╠══════════════════════╤════════╤════════╤════════╤════════╤════════╤════════╤════════╤═══════╤═══════════╣");
    println!("║ 설정                 │ Hira   │ Hira   │ Top-1  │ Read   │ Top-3  │ Top-10 │  Doc   │ 시간  │ 파라미터  ║");
    println!("║                      │ Top-1  │ Hit    │ Surf   │ Match  │ Hit    │ Hit    │ Match  │       │           ║");
    println!("╠══════════════════════╪════════╪════════╪════════╪════════╪════════╪════════╪════════╪═══════╪═══════════╣");

    let baseline_exact = results[0].1.exact_pct();

    for (config, m, secs) in &results {
        let delta = m.exact_pct() - baseline_exact;
        let delta_str = if delta.abs() < 0.05 { "  base".to_string() } else { format!("{:>+5.1}%p", delta) };
        let params = format!("cw={:.1} r={} d={} w={} i={}",
            config.cross_weight, config.sentence_range, config.dim, config.window_size, config.iterations);
        println!("  {:>20} │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>4.0}s │ {} │",
            config.label,
            m.hira_top1_pct(), m.hira_hit_pct(),
            m.exact_pct(), m.reading_pct(),
            m.top3_pct(), m.top10_pct(),
            m.doc_pct(), secs, delta_str);
    }
    println!("╚══════════════════════╧════════╧════════╧════════╧════════╧════════╧════════╧════════╧═══════╧═══════════╝");

    // ── Find best configuration ─────────────────────────────────────────
    println!();
    let best = results.iter().max_by(|a, b|
        a.1.exact_pct().partial_cmp(&b.1.exact_pct()).unwrap_or(std::cmp::Ordering::Equal)
    ).unwrap();
    println!("  최적 설정: {} (Top-1 Surface: {:.1}%, Δ{:>+.1}%p vs baseline)",
        best.0.label, best.1.exact_pct(), best.1.exact_pct() - baseline_exact);
    println!("    cross_weight={}, sentence_range={}, dim={}, window={}, iter={}, α={}, β={}, γ={}",
        best.0.cross_weight, best.0.sentence_range, best.0.dim, best.0.window_size,
        best.0.iterations, best.0.alpha, best.0.beta, best.0.gamma);

    // Bar chart for top-1 surface
    println!();
    println!("  Top-1 Surface 정확도 순위:");
    let mut sorted: Vec<_> = results.iter().collect();
    sorted.sort_by(|a, b| b.1.exact_pct().partial_cmp(&a.1.exact_pct()).unwrap_or(std::cmp::Ordering::Equal));
    for (config, m, _) in sorted.iter().take(10) {
        let bar_len = (m.exact_pct() * 0.5) as usize;
        let delta = m.exact_pct() - baseline_exact;
        println!("    {:>20} │{} {:.1}% ({:>+.1}%p)",
            config.label, "█".repeat(bar_len), m.exact_pct(), delta);
    }

    println!();
    println!("  총 소요시간: {:.1}s", global_start.elapsed().as_secs_f64());
}
