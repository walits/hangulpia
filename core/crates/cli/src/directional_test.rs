//! A/B comparison: non-directional vs directional embeddings.
//!
//! Builds two in-memory DBs (same 500K training data):
//!   A) build_large        → symmetric co-occurrence → ContextRanker
//!   B) build_large_directional → fwd/bwd co-occurrence → DirectionalContextRanker
//!
//! Benchmarks both with 1,000 test documents and compares accuracy.

use ime_db::generator::{generate_corpus, GenSentence};
use ime_db::{DbBuilder, DictionaryDb, DirectionalContextRanker, TrainerConfig};
use ime_hangul::phoneme;
use ime_japanese::romaji;
use std::time::Instant;

fn hangul_to_hiragana_candidates(hangul: &str) -> Vec<(String, String, f64)> {
    let romaji_candidates = phoneme::hangul_string_to_romaji(hangul, 10);
    let mut results: Vec<(String, String, f64)> = romaji_candidates
        .into_iter()
        .map(|(rom, conf)| {
            let hiragana = romaji::romaji_to_hiragana(&rom);
            (hiragana, rom, conf)
        })
        .collect();
    results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    results.dedup_by(|a, b| a.0 == b.0);
    results
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
}

impl Metrics {
    fn pct(&self, n: usize, d: usize) -> f64 {
        if d == 0 { 0.0 } else { n as f64 / d as f64 * 100.0 }
    }
    fn exact_pct(&self) -> f64 { self.pct(self.exact_match, self.total_words) }
    fn reading_pct(&self) -> f64 { self.pct(self.reading_match, self.total_words) }
    fn top3_pct(&self) -> f64 { self.pct(self.top3_hit, self.total_words) }
    fn top5_pct(&self) -> f64 { self.pct(self.top5_hit, self.total_words) }
    fn top10_pct(&self) -> f64 { self.pct(self.top10_hit, self.total_words) }
    fn doc_pct(&self) -> f64 { self.pct(self.full_doc_match, self.total_docs) }
}

/// Benchmark with non-directional (symmetric) ranker.
fn bench_symmetric(sentence: &GenSentence, db: &DictionaryDb) -> (usize, usize, usize, usize, usize, usize, bool) {
    let ranker = db.context_ranker();
    let mut confirmed: Vec<String> = Vec::new();
    let (mut total, mut exact, mut reading, mut t3, mut t5, mut t10) = (0,0,0,0,0,0);

    for word in &sentence.words {
        total += 1;
        let candidates = hangul_to_hiragana_candidates(&word.hangul);
        if candidates.is_empty() {
            confirmed.push(word.surface.clone());
            continue;
        }
        let ctx: Vec<&str> = confirmed.iter().map(|s| s.as_str()).collect();
        let ranked = ranker.rank_candidates(&candidates, &ctx, 10000);

        if let Some(top) = ranked.first() {
            if top.surface == word.surface { exact += 1; }
            if top.reading == word.reading { reading += 1; }
        }
        if ranked.iter().take(3).any(|r| r.surface == word.surface) { t3 += 1; }
        if ranked.iter().take(5).any(|r| r.surface == word.surface) { t5 += 1; }
        if ranked.iter().take(10).any(|r| r.surface == word.surface) { t10 += 1; }
        confirmed.push(word.surface.clone());
    }
    (total, exact, reading, t3, t5, t10, exact == total)
}

/// Benchmark with directional ranker.
fn bench_directional(sentence: &GenSentence, db: &DictionaryDb) -> (usize, usize, usize, usize, usize, usize, bool) {
    let ranker = db.directional_ranker();
    let mut confirmed: Vec<String> = Vec::new();
    let (mut total, mut exact, mut reading, mut t3, mut t5, mut t10) = (0,0,0,0,0,0);

    for word in &sentence.words {
        total += 1;
        let candidates = hangul_to_hiragana_candidates(&word.hangul);
        if candidates.is_empty() {
            confirmed.push(word.surface.clone());
            continue;
        }
        // Left context = words confirmed so far; right context = empty (progressive input)
        let left_ctx: Vec<&str> = confirmed.iter().map(|s| s.as_str()).collect();
        let ranked = ranker.rank_candidates(&candidates, &left_ctx, &[], 10000);

        if let Some(top) = ranked.first() {
            if top.surface == word.surface { exact += 1; }
            if top.reading == word.reading { reading += 1; }
        }
        if ranked.iter().take(3).any(|r| r.surface == word.surface) { t3 += 1; }
        if ranked.iter().take(5).any(|r| r.surface == word.surface) { t5 += 1; }
        if ranked.iter().take(10).any(|r| r.surface == word.surface) { t10 += 1; }
        confirmed.push(word.surface.clone());
    }
    (total, exact, reading, t3, t5, t10, exact == total)
}

fn run_benchmark<F>(
    label: &str,
    test_sentences: &[&GenSentence],
    bench_fn: F,
) -> Metrics
where
    F: Fn(&GenSentence) -> (usize, usize, usize, usize, usize, usize, bool),
{
    let n = test_sentences.len();
    let mut m = Metrics { total_docs: n, ..Default::default() };

    for (i, sentence) in test_sentences.iter().enumerate() {
        let (words, exact, reading, t3, t5, t10, full) = bench_fn(sentence);
        m.total_words += words;
        m.exact_match += exact;
        m.reading_match += reading;
        m.top3_hit += t3;
        m.top5_hit += t5;
        m.top10_hit += t10;
        if full { m.full_doc_match += 1; }

        if (i + 1) % 200 == 0 || i + 1 == n {
            print!("\r  [{}] {}/{} ({:.0}%)", label, i + 1, n, (i + 1) as f64 / n as f64 * 100.0);
            use std::io::Write;
            std::io::stdout().flush().ok();
        }
    }
    println!();
    m
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let test_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(1000);
    let train_size: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(500_000);

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║  A/B 비교: 무방향 vs 방향성 임베딩 정확도 테스트               ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();
    println!("  학습 문장: {}K  |  테스트 문서: {} 건", train_size / 1000, test_count);
    println!();

    let global_start = Instant::now();

    // Generate all sentences
    let total_needed = train_size + test_count;
    println!("  [준비] 전체 문장 {} 건 생성 중...", total_needed);
    let gen_start = Instant::now();
    let all_sentences = generate_corpus(total_needed);
    println!("         완료 ({:.1}s)", gen_start.elapsed().as_secs_f64());

    let test_sentences: Vec<&GenSentence> = all_sentences[train_size..].iter().collect();
    let actual_test = test_sentences.len();
    println!("  테스트 셋: {} 건", actual_test);
    println!();

    let config = TrainerConfig {
        dim: 64,
        iterations: 50,
        window_size: 5,
        ..Default::default()
    };

    // ── Build A: Non-directional (symmetric) ────────────────────────────
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  [A] 무방향 (Symmetric) DB 구축 중...");
    let build_a_start = Instant::now();
    let db_a = DictionaryDb::open_in_memory().unwrap();
    let builder_a = DbBuilder::new(db_a.conn()).with_config(config.clone());
    let stats_a = builder_a.build_large(train_size).unwrap();
    let build_a_secs = build_a_start.elapsed().as_secs_f64();
    println!("      완료 ({:.1}s) — 임베딩: {}, 사전: {}", build_a_secs, stats_a.embeddings, stats_a.dict_entries);
    println!();

    // ── Build B: Directional (fwd/bwd) ──────────────────────────────────
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  [B] 방향성 (Directional) DB 구축 중...");
    let build_b_start = Instant::now();
    let db_b = DictionaryDb::open_in_memory().unwrap();
    let builder_b = DbBuilder::new(db_b.conn()).with_config(config.clone());
    let stats_b = builder_b.build_large_directional(train_size).unwrap();
    let build_b_secs = build_b_start.elapsed().as_secs_f64();
    println!("      완료 ({:.1}s) — 임베딩: {}, 사전: {}", build_b_secs, stats_b.embeddings, stats_b.dict_entries);
    println!();

    // ── Benchmark A ─────────────────────────────────────────────────────
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  테스트 A: 무방향 (Symmetric)");
    let bench_a_start = Instant::now();
    let m_a = run_benchmark("A-Sym", &test_sentences, |s| bench_symmetric(s, &db_a));
    let bench_a_secs = bench_a_start.elapsed().as_secs_f64();
    println!("  → Top-1: {:.1}% | Reading: {:.1}% | Top-5: {:.1}% | Top-10: {:.1}% ({:.1}s)",
        m_a.exact_pct(), m_a.reading_pct(), m_a.top5_pct(), m_a.top10_pct(), bench_a_secs);
    println!();

    // ── Benchmark B ─────────────────────────────────────────────────────
    println!("  테스트 B: 방향성 (Directional)");
    let bench_b_start = Instant::now();
    let m_b = run_benchmark("B-Dir", &test_sentences, |s| bench_directional(s, &db_b));
    let bench_b_secs = bench_b_start.elapsed().as_secs_f64();
    println!("  → Top-1: {:.1}% | Reading: {:.1}% | Top-5: {:.1}% | Top-10: {:.1}% ({:.1}s)",
        m_b.exact_pct(), m_b.reading_pct(), m_b.top5_pct(), m_b.top10_pct(), bench_b_secs);
    println!();

    // ── Comparison ──────────────────────────────────────────────────────
    println!("╔═══════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                    A/B 비교 결과 (학습: {}K, 테스트: {} 문서)                         ║", train_size / 1000, actual_test);
    println!("╚═══════════════════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  ┌───────────────┬───────────┬───────────┬──────────┬──────────┬──────────┬──────────┐");
    println!("  │ 모델          │ Top1표기  │ Top1읽기  │  Top-3   │  Top-5   │  Top-10  │ 문서일치 │");
    println!("  ├───────────────┼───────────┼───────────┼──────────┼──────────┼──────────┼──────────┤");
    println!("  │ A) 무방향     │  {:>5.1}%   │  {:>5.1}%   │ {:>5.1}%  │ {:>5.1}%  │ {:>5.1}%  │  {:>5.1}%  │",
        m_a.exact_pct(), m_a.reading_pct(), m_a.top3_pct(), m_a.top5_pct(), m_a.top10_pct(), m_a.doc_pct());
    println!("  │ B) 방향성     │  {:>5.1}%   │  {:>5.1}%   │ {:>5.1}%  │ {:>5.1}%  │ {:>5.1}%  │  {:>5.1}%  │",
        m_b.exact_pct(), m_b.reading_pct(), m_b.top3_pct(), m_b.top5_pct(), m_b.top10_pct(), m_b.doc_pct());
    println!("  ├───────────────┼───────────┼───────────┼──────────┼──────────┼──────────┼──────────┤");

    let d_exact = m_b.exact_pct() - m_a.exact_pct();
    let d_reading = m_b.reading_pct() - m_a.reading_pct();
    let d_top3 = m_b.top3_pct() - m_a.top3_pct();
    let d_top5 = m_b.top5_pct() - m_a.top5_pct();
    let d_top10 = m_b.top10_pct() - m_a.top10_pct();
    let d_doc = m_b.doc_pct() - m_a.doc_pct();
    println!("  │ Δ (B-A)       │ {:>+5.1}%   │ {:>+5.1}%   │{:>+5.1}%  │{:>+5.1}%  │{:>+5.1}%  │ {:>+5.1}%  │",
        d_exact, d_reading, d_top3, d_top5, d_top10, d_doc);
    println!("  └───────────────┴───────────┴───────────┴──────────┴──────────┴──────────┴──────────┘");

    // Visual comparison
    let chart_width = 50;
    println!();
    println!("  ■ Top-1 Surface 정확도");
    let max_e = m_a.exact_pct().max(m_b.exact_pct());
    let bar_a = if max_e > 0.0 { (m_a.exact_pct() / max_e * chart_width as f64) as usize } else { 0 };
    let bar_b = if max_e > 0.0 { (m_b.exact_pct() / max_e * chart_width as f64) as usize } else { 0 };
    println!("  무방향 │{:<50} {:.1}%", "█".repeat(bar_a), m_a.exact_pct());
    println!("  방향성 │{:<50} {:.1}%", "█".repeat(bar_b), m_b.exact_pct());
    println!("         └──────────────────────────────────────────────");

    println!();
    println!("  ■ Top-1 Reading 정확도");
    let max_r = m_a.reading_pct().max(m_b.reading_pct());
    let bar_a = if max_r > 0.0 { (m_a.reading_pct() / max_r * chart_width as f64) as usize } else { 0 };
    let bar_b = if max_r > 0.0 { (m_b.reading_pct() / max_r * chart_width as f64) as usize } else { 0 };
    println!("  무방향 │{:<50} {:.1}%", "█".repeat(bar_a), m_a.reading_pct());
    println!("  방향성 │{:<50} {:.1}%", "█".repeat(bar_b), m_b.reading_pct());
    println!("         └──────────────────────────────────────────────");

    println!();
    println!("  ■ Top-10 포함률");
    let max_t = m_a.top10_pct().max(m_b.top10_pct());
    let bar_a = if max_t > 0.0 { (m_a.top10_pct() / max_t * chart_width as f64) as usize } else { 0 };
    let bar_b = if max_t > 0.0 { (m_b.top10_pct() / max_t * chart_width as f64) as usize } else { 0 };
    println!("  무방향 │{:<50} {:.1}%", "█".repeat(bar_a), m_a.top10_pct());
    println!("  방향성 │{:<50} {:.1}%", "█".repeat(bar_b), m_b.top10_pct());
    println!("         └──────────────────────────────────────────────");

    println!();
    println!("  ■ DB 빌드 시간");
    let max_build = build_a_secs.max(build_b_secs);
    let bar_a = (build_a_secs / max_build * chart_width as f64) as usize;
    let bar_b = (build_b_secs / max_build * chart_width as f64) as usize;
    println!("  무방향 │{:<50} {:.1}s", "▓".repeat(bar_a), build_a_secs);
    println!("  방향성 │{:<50} {:.1}s", "▓".repeat(bar_b), build_b_secs);
    println!("         └──────────────────────────────────────────────");

    println!();
    println!("  총 소요 시간: {:.1}s", global_start.elapsed().as_secs_f64());
    println!();
}
