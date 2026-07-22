//! Vocabulary A/B benchmark: compares original (731) vs expanded (~1,930) vocabulary
//! using the same training pipeline and test methodology.
//!
//! Both arms use 500K training sentences, 64-dim embeddings, 50 SGD iterations.
//! Test set: 1,000 documents generated beyond the training range.

use ime_db::generator::{generate_corpus, generate_corpus_with_vocab, GenSentence};
use ime_db::kana_hangul::hiragana_to_hangul;
use ime_db::vocab::{build_vocab, build_full_vocab};
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

// ── Run one arm ─────────────────────────────────────────────────────────────

fn run_arm(label: &str, test_sentences: &[GenSentence], db: &DictionaryDb) -> Metrics {
    let actual_test = test_sentences.len();
    let mut m = Metrics::default();
    m.total_docs = actual_test;

    for (i, sentence) in test_sentences.iter().enumerate() {
        let (words, exact, reading, t3, t5, t10, full) = benchmark_sentence(sentence, db);
        m.total_words += words;
        m.exact_match += exact;
        m.reading_match += reading;
        m.top3_hit += t3;
        m.top5_hit += t5;
        m.top10_hit += t10;
        if full { m.full_doc_match += 1; }

        if (i + 1) % 200 == 0 || i + 1 == actual_test {
            eprint!("\r  [{}] {}/{} ({:.0}%)", label, i + 1, actual_test, (i + 1) as f64 / actual_test as f64 * 100.0);
        }
    }
    eprintln!();
    m
}

fn print_metrics(label: &str, m: &Metrics, build_secs: f64) {
    println!("  {:>16}  │ {:>8.1}% │ {:>8.1}% │ {:>8.1}% │ {:>8.1}% │ {:>8.1}% │ {:>8.1}% │ {:>5.1}s │",
        label,
        m.exact_pct(),
        m.reading_pct(),
        m.top3_pct(),
        m.top5_pct(),
        m.top10_pct(),
        m.doc_pct(),
        build_secs,
    );
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let train_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(500_000);
    let test_count: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(1_000);

    println!("╔═══════════════════════════════════════════════════════════════════════════════╗");
    println!("║          한글일본어입력기 어휘량 A/B 벤치마크 (Vocab Size A/B Test)            ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════════╝");
    println!();

    let base_vocab = build_vocab();
    let full_vocab = build_full_vocab();
    println!("  기본 어휘: {} 항목", base_vocab.len());
    println!("  확장 어휘: {} 항목 (+{})", full_vocab.len(), full_vocab.len() - base_vocab.len());
    println!("  학습 문장: {}K", train_count / 1000);
    println!("  테스트 문서: {} 건", test_count);
    println!();

    let global_start = Instant::now();

    // ═══ ARM A: Base vocabulary (731) ═══════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  ARM A: 기본 어휘 ({} 항목)", base_vocab.len());
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let build_start_a = Instant::now();
    eprintln!("  [ARM A] 문장 생성 + DB 구축 중...");
    let total_needed_a = train_count + test_count;
    let all_sentences_a = generate_corpus(total_needed_a);
    let test_a: Vec<GenSentence> = all_sentences_a[train_count..].to_vec();

    let db_a = DictionaryDb::open_in_memory().expect("failed to open in-memory db");
    let config = TrainerConfig {
        dim: 64,
        iterations: 50,
        ..Default::default()
    };
    let builder_a = DbBuilder::new(db_a.conn()).with_config(config.clone());
    let stats_a = builder_a.build_large(train_count).expect("build_large failed");
    let build_secs_a = build_start_a.elapsed().as_secs_f64();
    println!("  빌드 완료: 임베딩 {}, 사전 {} ({:.1}s)", stats_a.embeddings, stats_a.dict_entries, build_secs_a);

    let metrics_a = run_arm("기본", &test_a, &db_a);

    // ═══ ARM B: Expanded vocabulary (~1,930) ════════════════════════════════
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  ARM B: 확장 어휘 ({} 항목)", full_vocab.len());
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let build_start_b = Instant::now();
    eprintln!("  [ARM B] 문장 생성 + DB 구축 중...");
    let total_needed_b = train_count + test_count;
    let all_sentences_b = generate_corpus_with_vocab(&full_vocab, total_needed_b);
    let test_b: Vec<GenSentence> = all_sentences_b[train_count..].to_vec();

    let db_b = DictionaryDb::open_in_memory().expect("failed to open in-memory db");
    let builder_b = DbBuilder::new(db_b.conn()).with_config(config);
    let stats_b = builder_b.build_large_with_vocab(&full_vocab, train_count).expect("build_large_with_vocab failed");
    let build_secs_b = build_start_b.elapsed().as_secs_f64();
    println!("  빌드 완료: 임베딩 {}, 사전 {} ({:.1}s)", stats_b.embeddings, stats_b.dict_entries, build_secs_b);

    let metrics_b = run_arm("확장", &test_b, &db_b);

    // ═══ Comparison table ═══════════════════════════════════════════════════
    println!();
    println!("╔═══════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                                    어휘량 A/B 비교 결과                                              ║");
    println!("╠══════════════════╤══════════╤══════════╤══════════╤══════════╤══════════╤══════════╤═══════╣");
    println!("║                  │ Top-1    │ Reading  │ Top-3    │ Top-5    │ Top-10   │ 문서전체 │ 빌드  ║");
    println!("║                  │ Surface  │ Match    │ Hit      │ Hit      │ Hit      │ Match    │       ║");
    println!("╠══════════════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪═══════╣");
    print_metrics(&format!("기본 ({})", base_vocab.len()), &metrics_a, build_secs_a);
    print_metrics(&format!("확장 ({})", full_vocab.len()), &metrics_b, build_secs_b);
    println!("╠══════════════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪═══════╣");

    // Delta row
    let d_exact = metrics_b.exact_pct() - metrics_a.exact_pct();
    let d_reading = metrics_b.reading_pct() - metrics_a.reading_pct();
    let d_top3 = metrics_b.top3_pct() - metrics_a.top3_pct();
    let d_top5 = metrics_b.top5_pct() - metrics_a.top5_pct();
    let d_top10 = metrics_b.top10_pct() - metrics_a.top10_pct();
    let d_doc = metrics_b.doc_pct() - metrics_a.doc_pct();
    println!("  {:>16}  │ {:>+8.1}%p│ {:>+8.1}%p│ {:>+8.1}%p│ {:>+8.1}%p│ {:>+8.1}%p│ {:>+8.1}%p│       │",
        "Δ (확장-기본)", d_exact, d_reading, d_top3, d_top5, d_top10, d_doc);
    println!("╚══════════════════╧══════════╧══════════╧══════════╧══════════╧══════════╧══════════╧═══════╝");

    println!();
    println!("  어휘 세부 비교:");
    println!("    ARM A: 총 단어 {}, 고유 표면형 {}, 임베딩 {}",
        metrics_a.total_words, stats_a.unique_words, stats_a.embeddings);
    println!("    ARM B: 총 단어 {}, 고유 표면형 {}, 임베딩 {}",
        metrics_b.total_words, stats_b.unique_words, stats_b.embeddings);

    // ASCII bar chart
    println!();
    println!("  Top-1 Surface 정확도 비교:");
    let bar_a = (metrics_a.exact_pct() * 0.5) as usize;
    let bar_b = (metrics_b.exact_pct() * 0.5) as usize;
    println!("    기본 ({:>4}) │{} {:.1}%", base_vocab.len(), "█".repeat(bar_a), metrics_a.exact_pct());
    println!("    확장 ({:>4}) │{} {:.1}%", full_vocab.len(), "█".repeat(bar_b), metrics_b.exact_pct());

    println!();
    println!("  Top-10 Hit 정확도 비교:");
    let bar_a10 = (metrics_a.top10_pct() * 0.5) as usize;
    let bar_b10 = (metrics_b.top10_pct() * 0.5) as usize;
    println!("    기본 ({:>4}) │{} {:.1}%", base_vocab.len(), "█".repeat(bar_a10), metrics_a.top10_pct());
    println!("    확장 ({:>4}) │{} {:.1}%", full_vocab.len(), "█".repeat(bar_b10), metrics_b.top10_pct());

    let total_secs = global_start.elapsed().as_secs_f64();
    println!();
    println!("  총 소요시간: {:.1}s", total_secs);
}
