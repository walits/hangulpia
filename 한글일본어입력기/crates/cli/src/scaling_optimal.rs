//! Training volume scaling with optimal configuration.
//!
//! Tests the best config (adj-sentence cw=0.2 + ngram δ=0.3) at different training volumes.
//! Also includes baseline (no adj, no ngram) at each scale for comparison.

use ime_db::generator::generate_corpus_with_seed;
use ime_db::ngram::build_ngram_model_chunked;
use ime_db::phonetic_decoder::*;
use ime_db::vocab::build_full_vocab;
use ime_db::{DbBuilder, DictionaryDb, TrainerConfig};
use ime_db::{cosine_similarity, weighted_average_vectors};
use ime_db::ngram::NgramModel;
use ime_db::generator::GenSentence;
use std::time::Instant;

#[derive(Debug, Clone, Default)]
struct Metrics {
    total_words: usize,
    total_docs: usize,
    exact_match: usize,
    reading_match: usize,
    top3_hit: usize,
    top10_hit: usize,
    full_doc_match: usize,
    hiragana_top1: usize,
    hiragana_hit: usize,
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
}

fn rank_4factor(
    candidates: &[(String, String, f64)],
    context_words: &[&str],
    db: &DictionaryDb,
    ngram: &NgramModel,
    alpha: f64, beta: f64, gamma: f64, delta: f64,
) -> Vec<(String, String, f64)> {
    let embed_store = db.embedding_store();
    let kanji_dict = db.kanji_dict();

    let ctx_embs: Vec<(Vec<f32>, f32)> = context_words.iter().enumerate()
        .filter_map(|(i, &w)| embed_store.get_embedding(w).ok().flatten()
            .map(|e| (e, 1.0 / (context_words.len() - i) as f32))).collect();
    let ctx_vec = if ctx_embs.is_empty() { None } else {
        let (v, w): (Vec<_>, Vec<_>) = ctx_embs.into_iter().unzip();
        let a = weighted_average_vectors(&v, &w);
        if a.iter().all(|&x| x.abs() < 1e-10) { None } else { Some(a) }
    };
    let ng_ctx: Vec<&str> = context_words.iter().rev().take(2).rev().copied().collect();

    let mut ranked = Vec::new();
    for (hira, _, pc) in candidates {
        let entries = kanji_dict.lookup(hira).unwrap_or_default();
        let mut ents: Vec<(String, i64)> = entries.into_iter().map(|e| (e.surface, e.frequency)).collect();
        if !ents.iter().any(|(s, _)| s == hira) { ents.push((hira.clone(), 0)); }
        for (surf, freq) in ents {
            let cs = ctx_vec.as_ref().and_then(|c| embed_store.get_embedding(&surf).ok().flatten()
                .map(|e| (cosine_similarity(&e, c) + 1.0) / 2.0)).unwrap_or(0.5);
            let fs = freq as f64 / 10000.0;
            let ns = ngram.normalized_score(&ng_ctx, &surf);
            ranked.push((surf, hira.clone(), alpha * pc + beta * cs + gamma * fs + delta * ns));
        }
    }
    ranked.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let mut seen = std::collections::HashSet::new();
    ranked.retain(|(s, _, _)| seen.insert(s.clone()));
    ranked
}

fn bench(
    test: &[GenSentence], db: &DictionaryDb, ngram: &NgramModel, pmap: &PhoneticMap,
    a: f64, b: f64, g: f64, d: f64,
) -> Metrics {
    let mut m = Metrics { total_docs: test.len(), ..Default::default() };
    for s in test {
        let mut ctx: Vec<String> = Vec::new();
        let (mut de, mut dw) = (0, 0);
        for w in &s.words {
            m.total_words += 1; dw += 1;
            let dec = BeamDecoder::new(pmap, 8, 20);
            let cands: Vec<(String, String, f64)> = dec.decode(&w.hangul).into_iter()
                .map(|(h, c)| (h, String::new(), c)).collect();
            if cands.is_empty() { ctx.push(w.surface.clone()); continue; }
            if cands.iter().any(|(h, _, _)| *h == w.reading) { m.hiragana_hit += 1; }
            if cands.first().map(|(h, _, _)| h.as_str()) == Some(w.reading.as_str()) { m.hiragana_top1 += 1; }
            let cv: Vec<&str> = ctx.iter().map(|s| s.as_str()).collect();
            let r = rank_4factor(&cands, &cv, db, ngram, a, b, g, d);
            if let Some(top) = r.first() {
                if top.0 == w.surface { m.exact_match += 1; de += 1; }
                if top.1 == w.reading { m.reading_match += 1; }
            }
            if r.iter().take(3).any(|x| x.0 == w.surface) { m.top3_hit += 1; }
            if r.iter().take(10).any(|x| x.0 == w.surface) { m.top10_hit += 1; }
            ctx.push(w.surface.clone());
        }
        if de == dw { m.full_doc_match += 1; }
    }
    m
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let test_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(500);

    let vocab = build_full_vocab();
    let volumes: Vec<usize> = vec![200_000, 500_000, 2_000_000];

    println!("╔══════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║    학습량 스케일링 × 최적 조합 벤치마크                                                   ║");
    println!("║    최적: 기존음소 + 인접문장(cw=0.2) + N-gram(δ=0.3)                                     ║");
    println!("║    가중치: α=0.1, β=0.35, γ=0.25, δ=0.3                                                ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  어휘: {}, 테스트: {} 건, 학습량: {:?}", vocab.len(), test_count,
        volumes.iter().map(|v| format!("{}K", v/1000)).collect::<Vec<_>>());
    println!();

    let global_start = Instant::now();

    // Test data (same for all)
    let test = generate_corpus_with_seed(&vocab, test_count, 99);
    println!("  테스트 데이터: {} 건 생성", test.len());
    println!();

    let cfg = TrainerConfig { dim: 64, iterations: 50, ..Default::default() };

    let mut all_results: Vec<(String, Metrics)> = Vec::new();

    for &vol in &volumes {
        println!("━━━ 학습량 {}K ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
            vol / 1000);

        // Build baseline DB
        eprint!("  기존 DB...");
        let t = Instant::now();
        let db_base = DictionaryDb::open_in_memory().expect("db");
        DbBuilder::new(db_base.conn()).with_config(cfg.clone())
            .build_large_with_vocab_chunked(&vocab, vol).expect("build");
        eprintln!(" {:.1}s", t.elapsed().as_secs_f64());

        // Build adjacent DB
        eprint!("  인접문장 DB...");
        let t = Instant::now();
        let db_adj = DictionaryDb::open_in_memory().expect("db");
        DbBuilder::new(db_adj.conn()).with_config(cfg.clone())
            .build_large_with_adjacent_sentences(&vocab, vol, 0.2).expect("build");
        eprintln!(" {:.1}s", t.elapsed().as_secs_f64());

        // Phonetic map
        eprint!("  음소 맵...");
        let t = Instant::now();
        let pmap = build_phonetic_map_chunked(&vocab, vol);
        eprintln!(" {:.1}s", t.elapsed().as_secs_f64());

        // N-gram
        eprint!("  N-gram...");
        let t = Instant::now();
        let ngram = build_ngram_model_chunked(&vocab, vol);
        eprintln!(" {:.1}s — {}", t.elapsed().as_secs_f64(), ngram.stats());

        let ng_empty = NgramModel::new();

        // Run arms
        eprint!("  [A] 기존...");
        let t = Instant::now();
        let m_a = bench(&test, &db_base, &ng_empty, &pmap, 0.3, 0.5, 0.2, 0.0);
        eprintln!(" {:.1}% ({:.1}s)", m_a.exact_pct(), t.elapsed().as_secs_f64());
        all_results.push((format!("{}K 기존", vol/1000), m_a));

        eprint!("  [B] 최적...");
        let t = Instant::now();
        let m_b = bench(&test, &db_adj, &ngram, &pmap, 0.1, 0.35, 0.25, 0.3);
        eprintln!(" {:.1}% ({:.1}s)", m_b.exact_pct(), t.elapsed().as_secs_f64());
        all_results.push((format!("{}K 최적", vol/1000), m_b));

        println!();
    }

    // ── Summary ─────────────────────────────────────────────────────────
    println!("╔════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                          학습량 스케일링 × 최적 조합 결과                                      ║");
    println!("╠══════════════════════╤════════╤════════╤════════╤════════╤════════╤════════╤═════════════════╣");
    println!("║ 설정                 │ Hira   │ Top-1  │ Read   │ Top-3  │ Top-10 │  Doc   │ 개선폭          ║");
    println!("║                      │ Top-1  │ Surf   │ Match  │ Hit    │ Hit    │ Match  │                 ║");
    println!("╠══════════════════════╪════════╪════════╪════════╪════════╪════════╪════════╪═════════════════╣");

    for i in 0..volumes.len() {
        let (ref la, ref ma) = all_results[i * 2];
        let (ref lb, ref mb) = all_results[i * 2 + 1];
        let delta = mb.exact_pct() - ma.exact_pct();
        println!("  {:>20} │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │                 │",
            la, ma.hira_top1_pct(), ma.exact_pct(), ma.reading_pct(), ma.top3_pct(), ma.top10_pct(), ma.doc_pct());
        println!("  {:>20} │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>+14.1}%p  │",
            lb, mb.hira_top1_pct(), mb.exact_pct(), mb.reading_pct(), mb.top3_pct(), mb.top10_pct(), mb.doc_pct(), delta);
        if i < volumes.len() - 1 {
            println!("  ─────────────────────┼────────┼────────┼────────┼────────┼────────┼────────┼─────────────────┤");
        }
    }
    println!("╚══════════════════════╧════════╧════════╧════════╧════════╧════════╧════════╧═════════════════╝");

    // Progression chart
    println!();
    println!("  Top-1 Surface 학습량별 추이:");
    for (label, m) in &all_results {
        let bar = (m.exact_pct() * 0.4) as usize;
        println!("    {:>20} │{} {:.1}%", label, "█".repeat(bar), m.exact_pct());
    }

    println!();
    println!("  문서 전체 일치율:");
    for (label, m) in &all_results {
        let bar = (m.doc_pct() * 0.6) as usize;
        println!("    {:>20} │{} {:.1}%", label, "█".repeat(bar), m.doc_pct());
    }

    println!();
    println!("  총 소요시간: {:.1}s", global_start.elapsed().as_secs_f64());
}
