//! 4-arm benchmark: compares approaches to fix the 9.5% phonetic embedding ceiling.
//!
//! ARM 0: 기존 음소 임베딩 (baseline, Hira Top-1 ~90.5%)
//! ARM 1: 마커 방식 (ん→ⓝ, っ→ⓧ, ー→ⓜ markers in forward conversion)
//! ARM 2: 멀티토큰 (2~3글자 hangul ngram → hiragana ngram mappings)
//! ARM 3: 하이브리드 (기존 임베딩 + ん/っ/ー 규칙 후처리)

use ime_db::generator::generate_corpus_with_seed;
use ime_db::kana_hangul::hiragana_to_hangul;
use ime_db::phonetic_decoder::*;
use ime_db::vocab::build_full_vocab;
use ime_db::{DbBuilder, DictionaryDb, TrainerConfig};
use ime_hangul::phoneme;
use ime_japanese::romaji;
use std::time::Instant;

// ── Candidate generators per ARM ────────────────────────────────────────────

fn arm0_candidates(hangul: &str, map: &PhoneticMap) -> Vec<(String, String, f64)> {
    let decoder = BeamDecoder::new(map, 8, 20);
    decoder.decode(hangul).into_iter()
        .map(|(h, c)| (h, String::new(), c))
        .collect()
}

fn arm1_candidates(reading: &str, map: &PhoneticMap) -> Vec<(String, String, f64)> {
    // Forward: reading → marked hangul → decode with marked map
    let marked_hangul = hiragana_to_hangul_marked(reading);
    let decoder = BeamDecoder::new(map, 8, 20);
    decoder.decode(&marked_hangul).into_iter()
        .map(|(h, c)| (h, String::new(), c))
        .collect()
}

// For arm1 we need to pass the marked hangul, not the regular hangul
fn arm1_candidates_from_hangul(marked_hangul: &str, map: &PhoneticMap) -> Vec<(String, String, f64)> {
    let decoder = BeamDecoder::new(map, 8, 20);
    decoder.decode(marked_hangul).into_iter()
        .map(|(h, c)| (h, String::new(), c))
        .collect()
}

fn arm2_candidates(hangul: &str, map: &PhoneticMap) -> Vec<(String, String, f64)> {
    // Multi-token map supports 1/2/3 char lookups in BeamDecoder
    let decoder = BeamDecoder::new(map, 8, 20);
    decoder.decode(hangul).into_iter()
        .map(|(h, c)| (h, String::new(), c))
        .collect()
}

fn arm3_candidates(hangul: &str, original_hangul: &str, map: &PhoneticMap) -> Vec<(String, String, f64)> {
    decode_hybrid(hangul, original_hangul, map, 8, 20).into_iter()
        .map(|(h, c)| (h, String::new(), c))
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
    // get_candidates(hangul, reading) → candidates
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

    let vocab = build_full_vocab();

    println!("╔═══════════════════════════════════════════════════════════════════════════════╗");
    println!("║    음소 임베딩 개선 4-arm 비교 벤치마크                                        ║");
    println!("║    ARM 0: 기존 음소 임베딩 │ ARM 1: 마커 │ ARM 2: 멀티토큰 │ ARM 3: 하이브리드  ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  어휘: {}, 학습: {}K, 테스트: {} 건", vocab.len(), train_count / 1000, test_count);
    println!();

    let global_start = Instant::now();

    // ── Step 1: Build shared context DB ──────────────────────────────────
    println!("━━━ 컨텍스트 DB 구축 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let db = DictionaryDb::open_in_memory().expect("open_in_memory");
    let config = TrainerConfig { dim: 64, iterations: 50, ..Default::default() };
    let builder = DbBuilder::new(db.conn()).with_config(config);
    let stats = builder.build_large_with_vocab_chunked(&vocab, train_count).expect("build failed");
    println!("  임베딩: {}, 사전: {}", stats.embeddings, stats.dict_entries);
    println!();

    // ── Step 2: Build all 4 phonetic maps ────────────────────────────────
    println!("━━━ 음소 맵 구축 (4종) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    print!("  ARM 0 (기존)...");
    let t = Instant::now();
    let map0 = build_phonetic_map_chunked(&vocab, train_count);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), map0.stats());

    print!("  ARM 1 (마커)...");
    let t = Instant::now();
    let map1 = build_phonetic_map_marked_chunked(&vocab, train_count);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), map1.stats());

    print!("  ARM 2 (멀티토큰)...");
    let t = Instant::now();
    let map2 = build_phonetic_map_multitoken_chunked(&vocab, train_count);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), map2.stats());

    println!("  ARM 3 (하이브리드): ARM 0 맵 재사용 + 규칙 후처리");
    println!();

    // Show key mappings for each map
    let examples = ["츠", "치", "심", "켓", "ⓝ", "ⓧ", "ⓜ"];
    println!("  주요 매핑 비교:");
    println!("  ┌────────┬────────────────────────┬────────────────────────┬────────────────────────┐");
    println!("  │ 토큰   │ ARM 0 (기존)           │ ARM 1 (마커)           │ ARM 2 (멀티토큰)       │");
    println!("  ├────────┼────────────────────────┼────────────────────────┼────────────────────────┤");
    for ex in &examples {
        let m0 = map0.get_candidates(ex).map(|c| c.iter().take(2)
            .map(|m| format!("{}({:.0}%)", m.hiragana, m.probability * 100.0)).collect::<Vec<_>>().join(","))
            .unwrap_or_else(|| "—".to_string());
        let m1 = map1.get_candidates(ex).map(|c| c.iter().take(2)
            .map(|m| format!("{}({:.0}%)", m.hiragana, m.probability * 100.0)).collect::<Vec<_>>().join(","))
            .unwrap_or_else(|| "—".to_string());
        let m2 = map2.get_candidates(ex).map(|c| c.iter().take(2)
            .map(|m| format!("{}({:.0}%)", m.hiragana, m.probability * 100.0)).collect::<Vec<_>>().join(","))
            .unwrap_or_else(|| "—".to_string());
        println!("  │ {:>6} │ {:>22} │ {:>22} │ {:>22} │", ex, m0, m1, m2);
    }
    // Also show multi-char tokens for ARM 2
    let multi_examples = ["심파", "켓카", "코우", "칸조"];
    for ex in &multi_examples {
        let m2 = map2.get_candidates(ex).map(|c| c.iter().take(2)
            .map(|m| format!("{}({:.0}%)", m.hiragana, m.probability * 100.0)).collect::<Vec<_>>().join(","))
            .unwrap_or_else(|| "—".to_string());
        println!("  │ {:>6} │ {:>22} │ {:>22} │ {:>22} │", ex, "—", "—", m2);
    }
    println!("  └────────┴────────────────────────┴────────────────────────┴────────────────────────┘");
    println!();

    // ── Step 3: Generate test data ───────────────────────────────────────
    println!("━━━ 테스트 데이터 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let test_sentences = generate_corpus_with_seed(&vocab, test_count, 99);
    println!("  {} 건 생성 완료", test_sentences.len());
    println!();

    // ── Step 4: Run all 4 arms ───────────────────────────────────────────
    println!("━━━ 벤치마크 실행 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let t = Instant::now();
    let m0 = benchmark_arm("ARM0", &test_sentences, &db, |hangul, _reading| {
        arm0_candidates(hangul, &map0)
    });
    let t0 = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let m1 = benchmark_arm("ARM1", &test_sentences, &db, |_hangul, reading| {
        // ARM1 needs marked hangul from the reading
        let marked = hiragana_to_hangul_marked(reading);
        arm1_candidates_from_hangul(&marked, &map1)
    });
    let t1 = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let m2 = benchmark_arm("ARM2", &test_sentences, &db, |hangul, _reading| {
        arm2_candidates(hangul, &map2)
    });
    let t2 = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let m3 = benchmark_arm("ARM3", &test_sentences, &db, |hangul, _reading| {
        arm3_candidates(hangul, hangul, &map0)
    });
    let t3 = t.elapsed().as_secs_f64();

    // ── Results table ────────────────────────────────────────────────────
    println!();
    println!("╔═════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                               음소 임베딩 개선 4-arm 비교 결과                                                     ║");
    println!("╠═══════════════════╤══════════╤══════════╤══════════╤══════════╤══════════╤══════════╤══════════╤══════════╤═════════╣");
    println!("║                   │ Hira     │ Hira     │ Top-1    │ Reading  │ Top-3    │ Top-5    │ Top-10   │ 문서전체 │  시간   ║");
    println!("║                   │ Top-1    │ Any-Hit  │ Surface  │ Match    │ Hit      │ Hit      │ Hit      │ Match    │         ║");
    println!("╠═══════════════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪═════════╣");

    let arms: Vec<(&str, &Metrics, f64)> = vec![
        ("기존 음소", &m0, t0),
        ("①마커", &m1, t1),
        ("②멀티토큰", &m2, t2),
        ("③하이브리드", &m3, t3),
    ];

    for (label, m, secs) in &arms {
        println!("  {:>17} │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>7.1}% │ {:>5.1}s │",
            label,
            m.hira_top1_pct(), m.hira_hit_pct(),
            m.exact_pct(), m.reading_pct(),
            m.top3_pct(), m.top5_pct(), m.top10_pct(),
            m.doc_pct(), secs);
    }

    println!("╠═══════════════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪══════════╪═════════╣");

    // Delta rows vs baseline (ARM 0)
    for (label, m, _) in arms.iter().skip(1) {
        println!("  {:>17} │ {:>+7.1}%p│ {:>+7.1}%p│ {:>+7.1}%p│ {:>+7.1}%p│ {:>+7.1}%p│ {:>+7.1}%p│ {:>+7.1}%p│ {:>+7.1}%p│         │",
            format!("Δ{}", label),
            m.hira_top1_pct() - m0.hira_top1_pct(),
            m.hira_hit_pct() - m0.hira_hit_pct(),
            m.exact_pct() - m0.exact_pct(),
            m.reading_pct() - m0.reading_pct(),
            m.top3_pct() - m0.top3_pct(),
            m.top5_pct() - m0.top5_pct(),
            m.top10_pct() - m0.top10_pct(),
            m.doc_pct() - m0.doc_pct());
    }
    println!("╚═══════════════════╧══════════╧══════════╧══════════╧══════════╧══════════╧══════════╧══════════╧══════════╧═════════╝");

    // Bar charts
    println!();
    println!("  Hira Top-1 (히라가나 변환 정확도):");
    for (label, m, _) in &arms {
        let bar = (m.hira_top1_pct() * 0.5) as usize;
        println!("    {:>13} │{} {:.1}%", label, "█".repeat(bar), m.hira_top1_pct());
    }

    println!();
    println!("  Top-1 Surface (최종 정확도):");
    for (label, m, _) in &arms {
        let bar = (m.exact_pct() * 0.5) as usize;
        println!("    {:>13} │{} {:.1}%", label, "█".repeat(bar), m.exact_pct());
    }

    println!();
    println!("  Top-10 Hit:");
    for (label, m, _) in &arms {
        let bar = (m.top10_pct() * 0.5) as usize;
        println!("    {:>13} │{} {:.1}%", label, "█".repeat(bar), m.top10_pct());
    }

    println!();
    println!("  총 소요시간: {:.1}s", global_start.elapsed().as_secs_f64());
}
