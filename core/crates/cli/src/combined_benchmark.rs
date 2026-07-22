//! Combined approach benchmark: marker+multi-token phonetic + adjacent-sentence semantic.
//!
//! Compares 4 configurations:
//!   A: 기존 (baseline phonetic + word-level co-occurrence)
//!   B: 마커+멀티토큰 음소 (combined marker+multitoken phonetic + word-level co-occurrence)
//!   C: 기존 음소 + 인접문장 의미 (baseline phonetic + adjacent-sentence co-occurrence)
//!   D: 마커+멀티토큰 + 인접문장 (combined phonetic + adjacent-sentence co-occurrence)

use ime_db::generator::generate_corpus_with_seed;
use ime_db::phonetic_decoder::*;
use ime_db::vocab::build_full_vocab;
use ime_db::{DbBuilder, DictionaryDb, TrainerConfig};
use std::time::Instant;

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
    fn top5_pct(&self) -> f64 { self.pct(self.top5_hit, self.total_words) }
    fn top10_pct(&self) -> f64 { self.pct(self.top10_hit, self.total_words) }
    fn doc_pct(&self) -> f64 { self.pct(self.full_doc_match, self.total_docs) }
    fn hira_hit_pct(&self) -> f64 { self.pct(self.hiragana_hit, self.total_words) }
    fn hira_top1_pct(&self) -> f64 { self.pct(self.hiragana_top1, self.total_words) }
}

use ime_db::generator::GenSentence;

fn benchmark_arm<F>(
    label: &str,
    test_sentences: &[GenSentence],
    db: &DictionaryDb,
    mut get_candidates: F,
) -> Metrics
where
    F: FnMut(&str, &str) -> Vec<(String, String, f64)>,
{
    let ranker = db.context_ranker();
    let mut total = Metrics { total_docs: test_sentences.len(), ..Default::default() };

    for (si, sentence) in test_sentences.iter().enumerate() {
        let mut confirmed: Vec<String> = Vec::new();
        let mut doc_exact = 0usize;
        let mut doc_words = 0usize;

        for word in &sentence.words {
            total.total_words += 1;
            doc_words += 1;

            let candidates = get_candidates(&word.hangul, &word.reading);

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

        if (si + 1) % 200 == 0 || si + 1 == test_sentences.len() {
            eprint!("\r  [{}] {}/{}", label, si + 1, test_sentences.len());
        }
    }
    eprintln!();
    total
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let train_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(500_000);
    let test_count: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(1_000);
    let cross_weight: f64 = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(0.4);

    let vocab = build_full_vocab();

    println!("╔═══════════════════════════════════════════════════════════════════════════════════╗");
    println!("║    마커+멀티토큰 음소 × 인접문장 의미 조합 벤치마크                                  ║");
    println!("║    A: 기존  B: 마커+멀티토큰  C: 인접문장  D: 마커+멀티토큰+인접문장                  ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  어휘: {}, 학습: {}K, 테스트: {} 건, cross_weight: {}",
        vocab.len(), train_count / 1000, test_count, cross_weight);
    println!();

    let global_start = Instant::now();

    // ── Step 1: Build two context DBs ───────────────────────────────────
    println!("━━━ 컨텍스트 DB 구축 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // DB A/B: word-level co-occurrence (baseline semantic)
    println!("  [A/B] 어휘 단위 공출현 (기존)...");
    let t = Instant::now();
    let db_word = DictionaryDb::open_in_memory().expect("open_in_memory");
    let config = TrainerConfig { dim: 64, iterations: 50, ..Default::default() };
    let builder = DbBuilder::new(db_word.conn()).with_config(config.clone());
    let stats_word = builder.build_large_with_vocab_chunked(&vocab, train_count).expect("build failed");
    println!("    임베딩: {}, 사전: {}, {:.1}s",
        stats_word.embeddings, stats_word.dict_entries, t.elapsed().as_secs_f64());

    // DB C/D: adjacent-sentence co-occurrence
    println!("  [C/D] 인접문장 공출현 (확장)...");
    let t = Instant::now();
    let db_adj = DictionaryDb::open_in_memory().expect("open_in_memory");
    let builder_adj = DbBuilder::new(db_adj.conn()).with_config(config);
    let stats_adj = builder_adj.build_large_with_adjacent_sentences(&vocab, train_count, cross_weight)
        .expect("build failed");
    println!("    임베딩: {}, 사전: {}, {:.1}s",
        stats_adj.embeddings, stats_adj.dict_entries, t.elapsed().as_secs_f64());
    println!();

    // ── Step 2: Build phonetic maps ─────────────────────────────────────
    println!("━━━ 음소 맵 구축 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    print!("  기존 음소 맵...");
    let t = Instant::now();
    let map_base = build_phonetic_map_chunked(&vocab, train_count);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), map_base.stats());

    print!("  마커+멀티토큰 음소 맵...");
    let t = Instant::now();
    let map_combo = build_phonetic_map_marked_multitoken_chunked(&vocab, train_count);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), map_combo.stats());

    // Show key mappings for combo map
    println!();
    println!("  마커+멀티토큰 맵 주요 매핑:");
    let examples = ["ⓝ", "ⓧ", "ⓜ", "츠", "치"];
    for ex in &examples {
        if let Some(candidates) = map_combo.get_candidates(ex) {
            let top: Vec<String> = candidates.iter().take(3)
                .map(|m| format!("{}({:.0}%)", m.hiragana, m.probability * 100.0))
                .collect();
            println!("    {} → {}", ex, top.join(", "));
        }
    }
    let multi_examples = ["시ⓝ", "ⓝ파", "케ⓧ", "ⓧ카", "코ⓜ"];
    for ex in &multi_examples {
        if let Some(candidates) = map_combo.get_candidates(ex) {
            let top: Vec<String> = candidates.iter().take(3)
                .map(|m| format!("{}({:.0}%)", m.hiragana, m.probability * 100.0))
                .collect();
            println!("    {} → {}", ex, top.join(", "));
        }
    }
    println!();

    // ── Step 3: Generate test data ──────────────────────────────────────
    println!("━━━ 테스트 데이터 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let test_sentences = generate_corpus_with_seed(&vocab, test_count, 99);
    println!("  {} 건 생성 완료", test_sentences.len());
    println!();

    // ── Step 4: Run all 4 arms ──────────────────────────────────────────
    println!("━━━ 벤치마크 실행 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // ARM A: baseline phonetic + word co-occurrence
    let t = Instant::now();
    let m_a = benchmark_arm("A", &test_sentences, &db_word, |hangul, _reading| {
        let decoder = BeamDecoder::new(&map_base, 8, 20);
        decoder.decode(hangul).into_iter()
            .map(|(h, c)| (h, String::new(), c))
            .collect()
    });
    let t_a = t.elapsed().as_secs_f64();

    // ARM B: marker+multi-token phonetic + word co-occurrence
    let t = Instant::now();
    let m_b = benchmark_arm("B", &test_sentences, &db_word, |_hangul, reading| {
        let marked = hiragana_to_hangul_marked(reading);
        let decoder = BeamDecoder::new(&map_combo, 8, 20);
        decoder.decode(&marked).into_iter()
            .map(|(h, c)| (h, String::new(), c))
            .collect()
    });
    let t_b = t.elapsed().as_secs_f64();

    // ARM C: baseline phonetic + adjacent-sentence co-occurrence
    let t = Instant::now();
    let m_c = benchmark_arm("C", &test_sentences, &db_adj, |hangul, _reading| {
        let decoder = BeamDecoder::new(&map_base, 8, 20);
        decoder.decode(hangul).into_iter()
            .map(|(h, c)| (h, String::new(), c))
            .collect()
    });
    let t_c = t.elapsed().as_secs_f64();

    // ARM D: marker+multi-token + adjacent-sentence
    let t = Instant::now();
    let m_d = benchmark_arm("D", &test_sentences, &db_adj, |_hangul, reading| {
        let marked = hiragana_to_hangul_marked(reading);
        let decoder = BeamDecoder::new(&map_combo, 8, 20);
        decoder.decode(&marked).into_iter()
            .map(|(h, c)| (h, String::new(), c))
            .collect()
    });
    let t_d = t.elapsed().as_secs_f64();

    // ── Results table ───────────────────────────────────────────────────
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                         마커+멀티토큰 × 인접문장 조합 결과                                                      ║");
    println!("╠════════════════════════════╤══════════╤══════════╤══════════╤══════════╤══════════╤══════════╤══════════╤════════╣");
    println!("║                            │ Hira     │ Hira     │ Top-1    │ Reading  │ Top-3    │ Top-10   │ 문서전체 │  시간  ║");
    println!("║                            │ Top-1    │ Any-Hit  │ Surface  │ Match    │ Hit      │ Hit      │ Match    │        ║");
    println!("╠════════════════════════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪════════╣");

    let arms: Vec<(&str, &Metrics, f64)> = vec![
        ("A 기존음소+어휘공출현", &m_a, t_a),
        ("B 마커멀티+어휘공출현", &m_b, t_b),
        ("C 기존음소+인접문장", &m_c, t_c),
        ("D 마커멀티+인접문장", &m_d, t_d),
    ];

    for (label, m, secs) in &arms {
        println!("  {:>26} │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>4.1}s │",
            label,
            m.hira_top1_pct(), m.hira_hit_pct(),
            m.exact_pct(), m.reading_pct(),
            m.top3_pct(), m.top10_pct(),
            m.doc_pct(), secs);
    }

    println!("╠════════════════════════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪════════╣");

    // Delta rows vs baseline (A)
    for (label, m, _) in arms.iter().skip(1) {
        println!("  {:>26} │ {:>+7.1}%p│ {:>+7.1}%p│ {:>+7.1}%p│ {:>+7.1}%p│ {:>+7.1}%p│ {:>+7.1}%p│ {:>+7.1}%p│        │",
            format!("Δ{}", label),
            m.hira_top1_pct() - m_a.hira_top1_pct(),
            m.hira_hit_pct() - m_a.hira_hit_pct(),
            m.exact_pct() - m_a.exact_pct(),
            m.reading_pct() - m_a.reading_pct(),
            m.top3_pct() - m_a.top3_pct(),
            m.top10_pct() - m_a.top10_pct(),
            m.doc_pct() - m_a.doc_pct());
    }
    println!("╚════════════════════════════╧══════════╧══════════╧══════════╧══════════╧══════════╧══════════╧══════════╧════════╝");

    // Bar charts
    println!();
    println!("  Hira Top-1 (히라가나 변환 정확도):");
    for (label, m, _) in &arms {
        let bar = (m.hira_top1_pct() * 0.5) as usize;
        println!("    {:>26} │{} {:.1}%", label, "█".repeat(bar), m.hira_top1_pct());
    }

    println!();
    println!("  Top-1 Surface (최종 정확도):");
    for (label, m, _) in &arms {
        let bar = (m.exact_pct() * 0.5) as usize;
        println!("    {:>26} │{} {:.1}%", label, "█".repeat(bar), m.exact_pct());
    }

    println!();
    println!("  Hira Any-Hit (후보 포함율):");
    for (label, m, _) in &arms {
        let bar = (m.hira_hit_pct() * 0.5) as usize;
        println!("    {:>26} │{} {:.1}%", label, "█".repeat(bar), m.hira_hit_pct());
    }

    println!();
    println!("  총 소요시간: {:.1}s", global_start.elapsed().as_secs_f64());
}
