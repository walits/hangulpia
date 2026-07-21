//! Window-size accuracy test: measures how co-occurrence window size affects accuracy.
//!
//! Fixes training data at 500K sentences, varies window_size = [3, 5, 10].
//! Benchmarks each with 1,000 test documents.

use ime_db::generator::{generate_corpus, GenSentence};
use ime_db::{DbBuilder, DictionaryDb, TrainerConfig};
use ime_hangul::phoneme;
use ime_japanese::romaji;
use std::time::Instant;

// ── Hangul → Hiragana pipeline ──────────────────────────────────────────────

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

// ── Metrics ─────────────────────────────────────────────────────────────────

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

// ── Benchmark one sentence ──────────────────────────────────────────────────

fn benchmark_sentence(sentence: &GenSentence, db: &DictionaryDb) -> (usize, usize, usize, usize, usize, usize, bool) {
    let ranker = db.context_ranker();
    let mut confirmed: Vec<String> = Vec::new();
    let mut total = 0usize;
    let mut exact = 0usize;
    let mut reading = 0usize;
    let mut t3 = 0usize;
    let mut t5 = 0usize;
    let mut t10 = 0usize;

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

    let full = exact == total;
    (total, exact, reading, t3, t5, t10, full)
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let test_doc_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(1000);
    let train_size: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(500_000);

    let window_sizes: Vec<usize> = vec![3, 5, 10];

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║   한글일본어입력기 윈도우 크기-정확도 테스트                    ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();
    println!("  학습 문장:   {}K", train_size / 1000);
    println!("  테스트 문서: {} 건", test_doc_count);
    println!("  윈도우 크기: {:?}", window_sizes);
    println!();

    let global_start = Instant::now();

    // ── Pre-generate ALL sentences at once ──────────────────────────────
    let total_needed = train_size + test_doc_count;
    println!("  [준비] 전체 문장 {} 건 생성 중...", total_needed);
    let gen_start = Instant::now();
    let all_sentences = generate_corpus(total_needed);
    println!("         완료 ({:.1}s)", gen_start.elapsed().as_secs_f64());
    println!();

    let test_sentences: Vec<&GenSentence> = all_sentences[train_size..].iter().collect();
    let actual_test = test_sentences.len();
    println!("  테스트 셋: {} 건 (index {}..{})", actual_test, train_size, train_size + actual_test);
    println!();

    // ── Results storage ─────────────────────────────────────────────────
    let mut all_metrics: Vec<(usize, Metrics, f64, f64)> = Vec::new();

    for &win in &window_sizes {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("  윈도우 크기: {} (앞뒤 {}단어 범위 공기 벡터화)", win, win);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        // Build in-memory DB with this window size
        let build_start = Instant::now();
        print!("  [빌드] DB 구축 중 (window={})...", win);
        use std::io::Write;
        std::io::stdout().flush().ok();

        let db = DictionaryDb::open_in_memory().expect("failed to open in-memory db");
        let config = TrainerConfig {
            dim: 64,
            iterations: 50,
            window_size: win,
            ..Default::default()
        };
        let builder = DbBuilder::new(db.conn()).with_config(config);
        let stats = builder.build_large(train_size).expect("build_large failed");
        let build_secs = build_start.elapsed().as_secs_f64();
        println!(" 완료 ({:.1}s)", build_secs);
        println!("         임베딩: {}, 사전: {}, vocab: {}", stats.embeddings, stats.dict_entries, stats.vocab_size);

        // Benchmark
        let bench_start = Instant::now();
        let mut m = Metrics::default();
        m.total_docs = actual_test;

        for (i, sentence) in test_sentences.iter().enumerate() {
            let (words, exact, reading, t3, t5, t10, full) = benchmark_sentence(sentence, &db);
            m.total_words += words;
            m.exact_match += exact;
            m.reading_match += reading;
            m.top3_hit += t3;
            m.top5_hit += t5;
            m.top10_hit += t10;
            if full { m.full_doc_match += 1; }

            if (i + 1) % 200 == 0 || i + 1 == actual_test {
                print!("\r  [테스트] {}/{} ({:.0}%)", i + 1, actual_test, (i + 1) as f64 / actual_test as f64 * 100.0);
                std::io::stdout().flush().ok();
            }
        }
        let bench_secs = bench_start.elapsed().as_secs_f64();
        println!();
        println!("  [결과] Top-1 Surface: {:.1}% | Reading: {:.1}% | Top-3: {:.1}% | Top-5: {:.1}% | Top-10: {:.1}% | Doc: {:.1}% ({:.1}s)",
            m.exact_pct(), m.reading_pct(), m.top3_pct(), m.top5_pct(), m.top10_pct(), m.doc_pct(), bench_secs);
        println!();

        all_metrics.push((win, m, build_secs, bench_secs));
    }

    // ── Summary table ───────────────────────────────────────────────────
    println!();
    println!("╔═══════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║              윈도우 크기별 정확도 비교 (학습: {}K 문장, 테스트: {} 문서)              ║", train_size / 1000, test_doc_count);
    println!("╚═══════════════════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  ┌──────────┬───────────┬───────────┬──────────┬──────────┬──────────┬──────────┬──────────┐");
    println!("  │ Window   │ Top1표기  │ Top1읽기  │  Top-3   │  Top-5   │  Top-10  │  문서일치│ 빌드(s)  │");
    println!("  ├──────────┼───────────┼───────────┼──────────┼──────────┼──────────┼──────────┼──────────┤");

    for (win, m, build_s, _) in &all_metrics {
        println!(
            "  │ w={:<5}  │  {:>5.1}%   │  {:>5.1}%   │ {:>5.1}%  │ {:>5.1}%  │ {:>5.1}%  │  {:>5.1}%  │ {:>6.1}   │",
            win,
            m.exact_pct(),
            m.reading_pct(),
            m.top3_pct(),
            m.top5_pct(),
            m.top10_pct(),
            m.doc_pct(),
            build_s,
        );
    }
    println!("  └──────────┴───────────┴───────────┴──────────┴──────────┴──────────┴──────────┴──────────┘");

    // ── Delta from baseline (w=5) ───────────────────────────────────────
    let baseline = all_metrics.iter().find(|(w, _, _, _)| *w == 5);
    if let Some((_, base_m, _, _)) = baseline {
        println!();
        println!("  ■ w=5 대비 변화량 (Δ)");
        println!("  ┌──────────┬───────────┬───────────┬──────────┬──────────┐");
        println!("  │ Window   │ ΔTop1표기 │ ΔTop1읽기 │ ΔTop-5   │ ΔTop-10  │");
        println!("  ├──────────┼───────────┼───────────┼──────────┼──────────┤");
        for (win, m, _, _) in &all_metrics {
            let d_exact = m.exact_pct() - base_m.exact_pct();
            let d_reading = m.reading_pct() - base_m.reading_pct();
            let d_top5 = m.top5_pct() - base_m.top5_pct();
            let d_top10 = m.top10_pct() - base_m.top10_pct();
            let marker = if *win == 5 { " (기준)" } else { "" };
            println!(
                "  │ w={:<5}  │  {:>+5.1}%  │  {:>+5.1}%  │ {:>+5.1}% │ {:>+5.1}% │{}",
                win, d_exact, d_reading, d_top5, d_top10, marker,
            );
        }
        println!("  └──────────┴───────────┴───────────┴──────────┴──────────┘");
    }

    // ── ASCII charts ────────────────────────────────────────────────────
    let chart_width = 50;

    println!();
    println!("  ■ Top-1 Surface 정확도 비교");
    println!();
    let max_exact = all_metrics.iter().map(|m| m.1.exact_pct()).fold(0.0f64, f64::max);
    for (win, m, _, _) in &all_metrics {
        let pct = m.exact_pct();
        let bar_len = if max_exact > 0.0 { (pct / max_exact * chart_width as f64) as usize } else { 0 };
        println!("  w={:<2} │{:<50} {:.1}%", win, "█".repeat(bar_len), pct);
    }
    println!("       └──────────────────────────────────────────────");

    println!();
    println!("  ■ Top-1 Reading 정확도 비교");
    println!();
    let max_read = all_metrics.iter().map(|m| m.1.reading_pct()).fold(0.0f64, f64::max);
    for (win, m, _, _) in &all_metrics {
        let pct = m.reading_pct();
        let bar_len = if max_read > 0.0 { (pct / max_read * chart_width as f64) as usize } else { 0 };
        println!("  w={:<2} │{:<50} {:.1}%", win, "█".repeat(bar_len), pct);
    }
    println!("       └──────────────────────────────────────────────");

    println!();
    println!("  ■ Top-10 포함률 비교");
    println!();
    let max_t10 = all_metrics.iter().map(|m| m.1.top10_pct()).fold(0.0f64, f64::max);
    for (win, m, _, _) in &all_metrics {
        let pct = m.top10_pct();
        let bar_len = if max_t10 > 0.0 { (pct / max_t10 * chart_width as f64) as usize } else { 0 };
        println!("  w={:<2} │{:<50} {:.1}%", win, "█".repeat(bar_len), pct);
    }
    println!("       └──────────────────────────────────────────────");

    println!();
    println!("  ■ 빌드 시간 비교");
    println!();
    let max_build = all_metrics.iter().map(|m| m.2).fold(0.0f64, f64::max);
    for (win, _, build_s, _) in &all_metrics {
        let bar_len = if max_build > 0.0 { (*build_s / max_build * chart_width as f64) as usize } else { 0 };
        println!("  w={:<2} │{:<50} {:.1}s", win, "▓".repeat(bar_len), build_s);
    }
    println!("       └──────────────────────────────────────────────");

    println!();
    println!("  총 소요 시간: {:.1}s", global_start.elapsed().as_secs_f64());
    println!();
}
