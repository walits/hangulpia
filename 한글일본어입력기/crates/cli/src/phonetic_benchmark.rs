//! A/B Benchmark: Rule-based vs Phonetic-embedding Hangul→Hiragana decoding.
//!
//! ARM A: 기존 규칙 기반 (hangul_string_to_romaji → romaji_to_hiragana)
//! ARM B: 학습 기반 음소 임베딩 (PhoneticMap + BeamDecoder)
//!
//! Both arms use the same context ranker (ContextRanker) for kanji disambiguation.
//! The ONLY difference is how Hangul is converted to Hiragana candidates.

use ime_db::generator::{generate_corpus_with_seed, generate_corpus_with_vocab, GenSentence};
use ime_db::phonetic_decoder::{BeamDecoder, PhoneticMap, build_phonetic_map_from_generated};
use ime_db::vocab::{build_vocab, build_full_vocab};
use ime_db::{DbBuilder, DictionaryDb, TrainerConfig};
use ime_hangul::phoneme;
use ime_japanese::romaji;
use std::time::Instant;

// ── ARM A: Rule-based Hangul → Hiragana ─────────────────────────────────────

fn rule_based_candidates(hangul: &str) -> Vec<(String, String, f64)> {
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

// ── ARM B: Phonetic embedding Hangul → Hiragana ────────────────────────────

fn phonetic_candidates(hangul: &str, decoder: &BeamDecoder) -> Vec<(String, String, f64)> {
    let decoded = decoder.decode(hangul);
    decoded
        .into_iter()
        .map(|(hiragana, conf)| {
            // romaji is not used in this path, but we keep the same tuple format
            (hiragana.clone(), String::new(), conf)
        })
        .collect()
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
    // Phoneme-level metrics (how often hiragana candidates include the correct reading)
    hiragana_hit: usize, // correct hiragana in ANY candidate
    hiragana_top1: usize, // correct hiragana as TOP-1 candidate
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
    fn hira_hit_pct(&self) -> f64 { self.pct(self.hiragana_hit, self.total_words) }
    fn hira_top1_pct(&self) -> f64 { self.pct(self.hiragana_top1, self.total_words) }
}

// ── Benchmark engine ────────────────────────────────────────────────────────

enum CandidateMode<'a> {
    RuleBased,
    Phonetic(&'a BeamDecoder<'a>),
}

fn benchmark_sentence(
    sentence: &GenSentence,
    db: &DictionaryDb,
    mode: &CandidateMode,
) -> Metrics {
    let ranker = db.context_ranker();
    let mut confirmed: Vec<String> = Vec::new();
    let mut m = Metrics::default();

    for word in &sentence.words {
        m.total_words += 1;

        let candidates: Vec<(String, String, f64)> = match mode {
            CandidateMode::RuleBased => rule_based_candidates(&word.hangul),
            CandidateMode::Phonetic(decoder) => phonetic_candidates(&word.hangul, decoder),
        };

        if candidates.is_empty() {
            confirmed.push(word.surface.clone());
            continue;
        }

        // Check if correct hiragana (reading) is in candidates at all
        if candidates.iter().any(|(h, _, _)| *h == word.reading) {
            m.hiragana_hit += 1;
        }
        if candidates.first().map(|(h, _, _)| h.as_str()) == Some(&word.reading) {
            m.hiragana_top1 += 1;
        }

        let ctx: Vec<&str> = confirmed.iter().map(|s| s.as_str()).collect();
        let ranked = ranker.rank_candidates(&candidates, &ctx, 10000);

        if let Some(top) = ranked.first() {
            if top.surface == word.surface { m.exact_match += 1; }
            if top.reading == word.reading { m.reading_match += 1; }
        }
        if ranked.iter().take(3).any(|r| r.surface == word.surface) { m.top3_hit += 1; }
        if ranked.iter().take(5).any(|r| r.surface == word.surface) { m.top5_hit += 1; }
        if ranked.iter().take(10).any(|r| r.surface == word.surface) { m.top10_hit += 1; }

        confirmed.push(word.surface.clone());
    }

    m.total_docs = 1;
    if m.exact_match == m.total_words { m.full_doc_match = 1; }
    m
}

fn run_benchmark(
    label: &str,
    test_sentences: &[GenSentence],
    db: &DictionaryDb,
    mode: &CandidateMode,
) -> Metrics {
    let mut total = Metrics::default();
    total.total_docs = test_sentences.len();

    for (i, sentence) in test_sentences.iter().enumerate() {
        let sm = benchmark_sentence(sentence, db, mode);
        total.total_words += sm.total_words;
        total.exact_match += sm.exact_match;
        total.reading_match += sm.reading_match;
        total.top3_hit += sm.top3_hit;
        total.top5_hit += sm.top5_hit;
        total.top10_hit += sm.top10_hit;
        total.hiragana_hit += sm.hiragana_hit;
        total.hiragana_top1 += sm.hiragana_top1;
        if sm.full_doc_match > 0 { total.full_doc_match += 1; }

        if (i + 1) % 200 == 0 || i + 1 == test_sentences.len() {
            eprint!("\r  [{}] {}/{}", label, i + 1, test_sentences.len());
        }
    }
    eprintln!();
    total
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let train_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(500_000);
    let test_count: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(1_000);

    let vocab = build_full_vocab();

    println!("╔═══════════════════════════════════════════════════════════════════════════════╗");
    println!("║    한글→히라가나 변환 A/B 벤치마크: 규칙 기반 vs 음소 임베딩                    ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  어휘: {} 항목", vocab.len());
    println!("  학습 문장: {}K", train_count / 1000);
    println!("  테스트 문서: {} 건", test_count);
    println!();

    let global_start = Instant::now();

    // ── Step 1: Build context DB (same for both arms) ───────────────────
    println!("━━━ Step 1: 컨텍스트 DB 구축 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let db_start = Instant::now();
    let db = DictionaryDb::open_in_memory().expect("open_in_memory");
    let config = TrainerConfig { dim: 64, iterations: 50, ..Default::default() };
    let builder = DbBuilder::new(db.conn()).with_config(config);
    let stats = builder.build_large_with_vocab_chunked(&vocab, train_count)
        .expect("build_large_with_vocab_chunked");
    let db_secs = db_start.elapsed().as_secs_f64();
    println!("  완료: 임베딩 {}, 사전 {} ({:.1}s)", stats.embeddings, stats.dict_entries, db_secs);
    println!();

    // ── Step 2: Build PhoneticMap (for ARM B) ────────────────────────────
    println!("━━━ Step 2: 음소 임베딩 맵 구축 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let map_start = Instant::now();
    use ime_db::phonetic_decoder::build_phonetic_map_chunked;
    let phonetic_map = build_phonetic_map_chunked(&vocab, train_count);
    let map_secs = map_start.elapsed().as_secs_f64();
    println!("  {}", phonetic_map.stats());
    println!("  구축 시간: {:.1}s", map_secs);

    // Show some example mappings
    println!();
    println!("  주요 매핑 예시:");
    let examples = ["츠", "치", "하", "카", "쿠", "시", "스", "후"];
    for ex in &examples {
        if let Some(candidates) = phonetic_map.get_candidates(ex) {
            let top3: Vec<String> = candidates.iter().take(3)
                .map(|m| format!("{}({:.0}%)", m.hiragana, m.probability * 100.0))
                .collect();
            println!("    {} → {}", ex, top3.join(", "));
        }
    }
    println!();

    // ── Step 3: Generate test data ───────────────────────────────────────
    println!("━━━ Step 3: 테스트 데이터 생성 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let test_sentences = generate_corpus_with_seed(&vocab, test_count, 99);
    println!("  테스트 문서: {} 건 생성 (seed=99)", test_sentences.len());
    println!();

    // ── Step 4: Run benchmarks ───────────────────────────────────────────
    println!("━━━ Step 4: 벤치마크 실행 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    println!("  ARM A: 규칙 기반 (hangul_string_to_romaji → romaji_to_hiragana)");
    let a_start = Instant::now();
    let metrics_a = run_benchmark("규칙", &test_sentences, &db, &CandidateMode::RuleBased);
    let a_secs = a_start.elapsed().as_secs_f64();

    println!("  ARM B: 음소 임베딩 (PhoneticMap + BeamDecoder)");
    let decoder = BeamDecoder::new(&phonetic_map, 8, 20);
    let b_start = Instant::now();
    let metrics_b = run_benchmark("음소", &test_sentences, &db, &CandidateMode::Phonetic(&decoder));
    let b_secs = b_start.elapsed().as_secs_f64();

    // ── Results ──────────────────────────────────────────────────────────
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                        한글→히라가나 변환 A/B 비교 결과                                                          ║");
    println!("╠══════════════╤══════════╤══════════╤══════════╤══════════╤══════════╤══════════╤══════════╤══════════╤══════════╣");
    println!("║              │ Hira     │ Hira     │ Top-1    │ Reading  │ Top-3    │ Top-5    │ Top-10   │ 문서전체 │ 소요시간 ║");
    println!("║              │ Top-1    │ Any-Hit  │ Surface  │ Match    │ Hit      │ Hit      │ Hit      │ Match    │          ║");
    println!("╠══════════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╣");

    println!("  {:>12} │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>5.1}s  │",
        "규칙 기반",
        metrics_a.hira_top1_pct(), metrics_a.hira_hit_pct(),
        metrics_a.exact_pct(), metrics_a.reading_pct(),
        metrics_a.top3_pct(), metrics_a.top5_pct(), metrics_a.top10_pct(),
        metrics_a.doc_pct(), a_secs);

    println!("  {:>12} │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>5.1}s  │",
        "음소 임베딩",
        metrics_b.hira_top1_pct(), metrics_b.hira_hit_pct(),
        metrics_b.exact_pct(), metrics_b.reading_pct(),
        metrics_b.top3_pct(), metrics_b.top5_pct(), metrics_b.top10_pct(),
        metrics_b.doc_pct(), b_secs);

    println!("╠══════════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╣");

    let d = |a: f64, b: f64| -> String { format!("{:>+7.1}%p", b - a) };
    println!("  {:>12} │ {} │ {} │ {} │ {} │ {} │ {} │ {} │ {} │          │",
        "Δ (B-A)",
        d(metrics_a.hira_top1_pct(), metrics_b.hira_top1_pct()),
        d(metrics_a.hira_hit_pct(), metrics_b.hira_hit_pct()),
        d(metrics_a.exact_pct(), metrics_b.exact_pct()),
        d(metrics_a.reading_pct(), metrics_b.reading_pct()),
        d(metrics_a.top3_pct(), metrics_b.top3_pct()),
        d(metrics_a.top5_pct(), metrics_b.top5_pct()),
        d(metrics_a.top10_pct(), metrics_b.top10_pct()),
        d(metrics_a.doc_pct(), metrics_b.doc_pct()));

    println!("╚══════════════╧══════════╧══════════╧══════════╧══════════╧══════════╧══════════╧══════════╧══════════╧══════════╝");

    // Bar charts
    println!();
    println!("  히라가나 변환 정확도 (Hira Top-1: 올바른 히라가나가 1순위):");
    let bar_a = (metrics_a.hira_top1_pct() * 0.5) as usize;
    let bar_b = (metrics_b.hira_top1_pct() * 0.5) as usize;
    println!("    규칙 기반  │{} {:.1}%", "█".repeat(bar_a), metrics_a.hira_top1_pct());
    println!("    음소 임베딩│{} {:.1}%", "█".repeat(bar_b), metrics_b.hira_top1_pct());

    println!();
    println!("  최종 Top-1 Surface 정확도:");
    let bar_a2 = (metrics_a.exact_pct() * 0.5) as usize;
    let bar_b2 = (metrics_b.exact_pct() * 0.5) as usize;
    println!("    규칙 기반  │{} {:.1}%", "█".repeat(bar_a2), metrics_a.exact_pct());
    println!("    음소 임베딩│{} {:.1}%", "█".repeat(bar_b2), metrics_b.exact_pct());

    println!();
    println!("  Top-10 Hit 정확도:");
    let bar_a10 = (metrics_a.top10_pct() * 0.5) as usize;
    let bar_b10 = (metrics_b.top10_pct() * 0.5) as usize;
    println!("    규칙 기반  │{} {:.1}%", "█".repeat(bar_a10), metrics_a.top10_pct());
    println!("    음소 임베딩│{} {:.1}%", "█".repeat(bar_b10), metrics_b.top10_pct());

    println!();
    println!("  총 소요시간: {:.1}s", global_start.elapsed().as_secs_f64());
}
