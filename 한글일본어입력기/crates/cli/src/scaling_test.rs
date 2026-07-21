//! Scaling accuracy test: measures how training data volume affects round-trip accuracy.
//!
//! Builds DBs at 100K, 200K, 300K, 400K, 500K sentence increments,
//! then benchmarks each with 1,000 test documents.
//!
//! Test documents are generated beyond the training range to avoid data leakage.

use ime_db::generator::{generate_corpus, GenSentence};
use ime_db::kana_hangul::hiragana_to_hangul;
use ime_db::{ContextRanker, DbBuilder, DictionaryDb, RankedCandidate, TrainerConfig};
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
    exact_match: usize,    // top-1 surface exact
    reading_match: usize,  // top-1 reading exact
    top3_hit: usize,
    top5_hit: usize,
    top10_hit: usize,
    full_doc_match: usize, // all words in doc exact
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
    // returns (words, exact, reading, top3, top5, top10, full_match)
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
    let max_train: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(500_000);
    let step: usize = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(100_000);

    let train_sizes: Vec<usize> = (1..=(max_train / step)).map(|i| i * step).collect();

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║     한글일본어입력기 학습량-정확도 스케일링 테스트              ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();
    println!("  테스트 문서: {} 건", test_doc_count);
    println!("  학습 단계:   {:?}", train_sizes.iter().map(|n| format!("{}K", n / 1000)).collect::<Vec<_>>());
    println!();

    let global_start = Instant::now();

    // ── Pre-generate ALL sentences at once (max_train + test) ───────────
    // The generator is deterministic (seed=42), so generate_corpus(N) always
    // yields the same first N sentences. Test sentences = [max_train .. max_train+test_doc_count].
    let total_needed = max_train + test_doc_count;
    println!("  [준비] 전체 문장 {} 건 생성 중...", total_needed);
    let gen_start = Instant::now();
    let all_sentences = generate_corpus(total_needed);
    println!("         완료 ({:.1}s)", gen_start.elapsed().as_secs_f64());
    println!();

    let test_sentences: Vec<&GenSentence> = all_sentences[max_train..].iter().collect();
    let actual_test = test_sentences.len();
    println!("  테스트 셋: {} 건 (index {}..{})", actual_test, max_train, max_train + actual_test);
    println!();

    // ── Results storage ─────────────────────────────────────────────────
    let mut all_metrics: Vec<(usize, Metrics, f64, f64)> = Vec::new(); // (train_size, metrics, build_secs, bench_secs)

    for &train_size in &train_sizes {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("  학습량: {}K 문장", train_size / 1000);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        // Build in-memory DB (avoids disk I/O issues in sandbox)
        let build_start = Instant::now();
        print!("  [빌드] DB 구축 중...");
        use std::io::Write;
        std::io::stdout().flush().ok();

        let db = DictionaryDb::open_in_memory().expect("failed to open in-memory db");
        let config = TrainerConfig {
            dim: 64,
            iterations: 50,
            ..Default::default()
        };
        let builder = DbBuilder::new(db.conn()).with_config(config);
        let stats = builder.build_large(train_size).expect("build_large failed");
        let build_secs = build_start.elapsed().as_secs_f64();
        println!(" 완료 ({:.1}s)", build_secs);
        println!("         임베딩: {}, 사전: {}", stats.embeddings, stats.dict_entries);

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
        println!("  [결과] Top-1 Surface: {:.1}% | Reading: {:.1}% | Top-5: {:.1}% | Top-10: {:.1}% | Doc: {:.1}% ({:.1}s)",
            m.exact_pct(), m.reading_pct(), m.top5_pct(), m.top10_pct(), m.doc_pct(), bench_secs);
        println!();

        all_metrics.push((train_size, m, build_secs, bench_secs));
    }

    // ── Summary table ───────────────────────────────────────────────────
    println!();
    println!("╔═══════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                         학습량-정확도 스케일링 결과 요약                                ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  테스트 문서: {} 건  |  테스트 단어: ~{} 개", test_doc_count,
        all_metrics.first().map(|m| m.1.total_words).unwrap_or(0));
    println!();
    println!("  ┌────────┬───────────┬───────────┬──────────┬──────────┬──────────┬──────────┬──────────┐");
    println!("  │ 학습량 │ Top1표기  │ Top1읽기  │  Top-3   │  Top-5   │  Top-10  │  문서일치│ 빌드(s)  │");
    println!("  ├────────┼───────────┼───────────┼──────────┼──────────┼──────────┼──────────┼──────────┤");

    for (train_size, m, build_s, _bench_s) in &all_metrics {
        println!(
            "  │ {:>4}K  │  {:>5.1}%   │  {:>5.1}%   │ {:>5.1}%  │ {:>5.1}%  │ {:>5.1}%  │  {:>5.1}%  │ {:>6.1}   │",
            train_size / 1000,
            m.exact_pct(),
            m.reading_pct(),
            m.top3_pct(),
            m.top5_pct(),
            m.top10_pct(),
            m.doc_pct(),
            build_s,
        );
    }
    println!("  └────────┴───────────┴───────────┴──────────┴──────────┴──────────┴──────────┴──────────┘");

    // ── Delta table (improvement per step) ──────────────────────────────
    if all_metrics.len() >= 2 {
        println!();
        println!("  ■ 단계별 개선폭 (Δ)");
        println!("  ┌────────────────┬───────────┬───────────┬──────────┬──────────┐");
        println!("  │ 구간           │ ΔTop1표기 │ ΔTop1읽기 │ ΔTop-5   │ ΔTop-10  │");
        println!("  ├────────────────┼───────────┼───────────┼──────────┼──────────┤");
        for i in 1..all_metrics.len() {
            let (prev_size, prev_m, _, _) = &all_metrics[i - 1];
            let (cur_size, cur_m, _, _) = &all_metrics[i];
            let d_exact = cur_m.exact_pct() - prev_m.exact_pct();
            let d_reading = cur_m.reading_pct() - prev_m.reading_pct();
            let d_top5 = cur_m.top5_pct() - prev_m.top5_pct();
            let d_top10 = cur_m.top10_pct() - prev_m.top10_pct();
            println!(
                "  │ {:>4}K → {:>4}K │  {:>+5.1}%  │  {:>+5.1}%  │ {:>+5.1}% │ {:>+5.1}% │",
                prev_size / 1000, cur_size / 1000,
                d_exact, d_reading, d_top5, d_top10,
            );
        }
        println!("  └────────────────┴───────────┴───────────┴──────────┴──────────┘");
    }

    // ── ASCII chart ─────────────────────────────────────────────────────
    println!();
    println!("  ■ Top-1 Surface 정확도 추이");
    println!();
    let max_pct = all_metrics.iter().map(|m| m.1.exact_pct()).fold(0.0f64, f64::max);
    let chart_width = 50;
    for (train_size, m, _, _) in &all_metrics {
        let pct = m.exact_pct();
        let bar_len = if max_pct > 0.0 { (pct / max_pct * chart_width as f64) as usize } else { 0 };
        let bar: String = "█".repeat(bar_len);
        println!("  {:>4}K │{:<50} {:.1}%", train_size / 1000, bar, pct);
    }
    println!("       └──────────────────────────────────────────────");
    println!();

    println!("  ■ Top-1 Reading 정확도 추이");
    println!();
    let max_rpct = all_metrics.iter().map(|m| m.1.reading_pct()).fold(0.0f64, f64::max);
    for (train_size, m, _, _) in &all_metrics {
        let pct = m.reading_pct();
        let bar_len = if max_rpct > 0.0 { (pct / max_rpct * chart_width as f64) as usize } else { 0 };
        let bar: String = "█".repeat(bar_len);
        println!("  {:>4}K │{:<50} {:.1}%", train_size / 1000, bar, pct);
    }
    println!("       └──────────────────────────────────────────────");
    println!();

    println!("  ■ Top-10 포함률 추이");
    println!();
    let max_t10 = all_metrics.iter().map(|m| m.1.top10_pct()).fold(0.0f64, f64::max);
    for (train_size, m, _, _) in &all_metrics {
        let pct = m.top10_pct();
        let bar_len = if max_t10 > 0.0 { (pct / max_t10 * chart_width as f64) as usize } else { 0 };
        let bar: String = "█".repeat(bar_len);
        println!("  {:>4}K │{:<50} {:.1}%", train_size / 1000, bar, pct);
    }
    println!("       └──────────────────────────────────────────────");

    println!();
    println!("  총 소요 시간: {:.1}s", global_start.elapsed().as_secs_f64());
    println!();
}
