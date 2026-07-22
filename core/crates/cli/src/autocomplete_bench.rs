//! Auto-completion benchmark: measures keystroke savings.
//!
//! Simulates a user typing sentences, with the auto-complete engine
//! offering suggestions at each keystroke. Measures:
//!   - Next-word prediction hit rate
//!   - Prefix completion hit rate
//!   - Overall keystroke saving ratio
//!   - Comparison across accept_threshold values (1, 2, 3)

use ime_db::autocomplete::{AutoCompleteEngine, simulate_typing_session, TypingStats};
use ime_db::generator::generate_corpus_with_seed;
use ime_db::ngram::build_ngram_model_chunked;
use ime_db::phonetic_decoder::*;
use ime_db::vocab::build_full_vocab;
use ime_db::{DbBuilder, DictionaryDb, TrainerConfig};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let test_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(200);
    let train_vol: usize = 500_000;

    println!("╔══════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║    자동완성 키입력 절감률 벤치마크                                                       ║");
    println!("║    최적조합 (인접문장 cw=0.2 + N-gram δ=0.3)                                            ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════════════════╝");
    println!();

    let global_start = Instant::now();
    let vocab = build_full_vocab();
    let cfg = TrainerConfig { dim: 64, iterations: 50, ..Default::default() };

    println!("  어휘: {} 개, 테스트: {} 건, 학습량: {}K", vocab.len(), test_count, train_vol / 1000);
    println!();

    // Build training infrastructure
    eprint!("  인접문장 DB 구축...");
    let t = Instant::now();
    let db = DictionaryDb::open_in_memory().expect("db");
    DbBuilder::new(db.conn()).with_config(cfg.clone())
        .build_large_with_adjacent_sentences(&vocab, train_vol, 0.2).expect("build");
    eprintln!(" {:.1}s", t.elapsed().as_secs_f64());

    eprint!("  음소 맵 구축...");
    let t = Instant::now();
    let pmap = build_phonetic_map_chunked(&vocab, train_vol);
    eprintln!(" {:.1}s", t.elapsed().as_secs_f64());

    eprint!("  N-gram 모델 구축...");
    let t = Instant::now();
    let ngram = build_ngram_model_chunked(&vocab, train_vol);
    eprintln!(" {:.1}s — {}", t.elapsed().as_secs_f64(), ngram.stats());

    // Generate test data
    let test = generate_corpus_with_seed(&vocab, test_count, 99);
    println!("  테스트 데이터: {} 건 생성", test.len());
    println!();

    // Run simulations with different accept thresholds
    let thresholds = [1, 2, 3];
    let mut all_stats: Vec<(usize, TypingStats)> = Vec::new();

    for &threshold in &thresholds {
        eprint!("  시뮬레이션 (accept_threshold={})...", threshold);
        let t = Instant::now();
        let mut engine = AutoCompleteEngine::new(&db, &ngram, &pmap);

        let stats = simulate_typing_session(&mut engine, &test, threshold);
        let elapsed = t.elapsed().as_secs_f64();

        eprintln!(" 절감률 {:.1}% ({:.1}s)", stats.saving_ratio() * 100.0, elapsed);
        all_stats.push((threshold, stats));
    }

    // Also measure baseline (no auto-completion)
    let baseline_keys: usize = test.iter()
        .flat_map(|s| s.words.iter())
        .map(|w| w.hangul.chars().count() + 1) // +1 for space/confirm
        .sum();
    let total_words: usize = test.iter().map(|s| s.words.len()).sum();

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                          자동완성 벤치마크 결과                                              ║");
    println!("╠════════════════════════╤═══════════╤══════════╤═══════════╤════════════╤═════════╤══════════╣");
    println!("║ 설정                   │ 총키입력   │ 절감키    │  절감률    │ 평균키/단어 │ 다음예측 │ 접두사   ║");
    println!("╠════════════════════════╪═══════════╪══════════╪═══════════╪════════════╪═════════╪══════════╣");

    println!("  {:>22} │ {:>9} │ {:>8} │ {:>8}  │ {:>10.2} │ {:>7} │ {:>8} │",
        "자동완성 없음",
        baseline_keys,
        "-",
        "-",
        baseline_keys as f64 / total_words as f64,
        "-",
        "-");

    for (threshold, stats) in &all_stats {
        let saved = stats.total_keystrokes_baseline.saturating_sub(stats.total_keystrokes_with_ac);
        println!("  {:>22} │ {:>9} │ {:>8} │ {:>7.1}%  │ {:>10.2} │ {:>7} │ {:>8} │",
            format!("threshold={}", threshold),
            stats.total_keystrokes_with_ac,
            saved,
            stats.saving_ratio() * 100.0,
            stats.avg_keystrokes(),
            stats.next_word_hits,
            stats.prefix_hits);
    }

    println!("╚════════════════════════╧═══════════╧══════════╧═══════════╧════════════╧═════════╧══════════╝");

    // Visual comparison
    println!();
    println!("  키입력 절감률:");
    for (threshold, stats) in &all_stats {
        let bar = (stats.saving_ratio() * 100.0 * 0.5) as usize;
        println!("    threshold={} │{} {:.1}%",
            threshold, "█".repeat(bar), stats.saving_ratio() * 100.0);
    }

    // Breakdown
    println!();
    println!("  적중 상세:");
    for (threshold, stats) in &all_stats {
        let nw_pct = if stats.total_words > 0 {
            stats.next_word_hits as f64 / stats.total_words as f64 * 100.0
        } else { 0.0 };
        let pf_pct = if stats.total_words > 0 {
            stats.prefix_hits as f64 / stats.total_words as f64 * 100.0
        } else { 0.0 };
        let miss_pct = 100.0 - nw_pct - pf_pct;
        println!("    threshold={}: 다음단어 {:.1}% | 접두사 {:.1}% | 미적중 {:.1}% (총 {} 단어)",
            threshold, nw_pct, pf_pct, miss_pct, stats.total_words);
    }

    println!();
    println!("  총 소요시간: {:.1}s", global_start.elapsed().as_secs_f64());
}
