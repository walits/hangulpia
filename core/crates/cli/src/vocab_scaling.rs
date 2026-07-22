//! Vocabulary scaling benchmark: base vocab (~1.4K) vs large vocab (~20K)
//!
//! Both tested at 500K training with optimal config:
//!   base phonetic + adj-sentence(cw=0.2) + ngram(δ=0.3)
//!   weights: α=0.1, β=0.35, γ=0.25, δ=0.3

use ime_db::generator::generate_corpus_with_seed;
use ime_db::ngram::build_ngram_model_chunked;
use ime_db::phonetic_decoder::*;
use ime_db::vocab::build_full_vocab;
use ime_db::vocab_large::build_vocab_large;
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
    fn hira_hit_pct(&self) -> f64 { self.pct(self.hiragana_hit, self.total_words) }
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
    let test_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(300);
    let train_vol: usize = 500_000;

    println!("╔══════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║    어휘 확장 벤치마크: 기존(~1.4K) vs 확장(~20K)                                         ║");
    println!("║    학습량 500K, 최적 조합: α=0.1 β=0.35 γ=0.25 δ=0.3                                   ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════════════════╝");
    println!();

    let global_start = Instant::now();

    // ── Build vocabs ──
    let vocab_base = build_full_vocab();
    let vocab_large = build_vocab_large();
    println!("  기존 어휘: {} 개", vocab_base.len());
    println!("  확장 어휘: {} 개", vocab_large.len());
    println!("  테스트: {} 건, 학습량: {}K", test_count, train_vol / 1000);
    println!();

    let cfg = TrainerConfig { dim: 64, iterations: 50, ..Default::default() };

    // ── ARM A: Base vocab, baseline (old weights) ──
    println!("━━━ [A] 기존어휘 + 기존가중치 (α=0.3 β=0.5 γ=0.2 δ=0) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let test_base = generate_corpus_with_seed(&vocab_base, test_count, 99);
    eprint!("  DB...");
    let t = Instant::now();
    let db_a = DictionaryDb::open_in_memory().expect("db");
    DbBuilder::new(db_a.conn()).with_config(cfg.clone())
        .build_large_with_vocab_chunked(&vocab_base, train_vol).expect("build");
    eprintln!(" {:.1}s", t.elapsed().as_secs_f64());
    eprint!("  음소맵...");
    let t = Instant::now();
    let pmap_a = build_phonetic_map_chunked(&vocab_base, train_vol);
    eprintln!(" {:.1}s", t.elapsed().as_secs_f64());
    let ng_empty = NgramModel::new();
    eprint!("  평가...");
    let t = Instant::now();
    let m_a = bench(&test_base, &db_a, &ng_empty, &pmap_a, 0.3, 0.5, 0.2, 0.0);
    eprintln!(" {:.1}% ({:.1}s)", m_a.exact_pct(), t.elapsed().as_secs_f64());
    println!();

    // ── ARM B: Base vocab, optimal ──
    println!("━━━ [B] 기존어휘 + 최적조합 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    eprint!("  인접문장DB...");
    let t = Instant::now();
    let db_b = DictionaryDb::open_in_memory().expect("db");
    DbBuilder::new(db_b.conn()).with_config(cfg.clone())
        .build_large_with_adjacent_sentences(&vocab_base, train_vol, 0.2).expect("build");
    eprintln!(" {:.1}s", t.elapsed().as_secs_f64());
    eprint!("  N-gram...");
    let t = Instant::now();
    let ng_b = build_ngram_model_chunked(&vocab_base, train_vol);
    eprintln!(" {:.1}s — {}", t.elapsed().as_secs_f64(), ng_b.stats());
    eprint!("  평가...");
    let t = Instant::now();
    let m_b = bench(&test_base, &db_b, &ng_b, &pmap_a, 0.1, 0.35, 0.25, 0.3);
    eprintln!(" {:.1}% ({:.1}s)", m_b.exact_pct(), t.elapsed().as_secs_f64());
    println!();

    // ── ARM C: Large vocab, baseline ──
    println!("━━━ [C] 확장어휘 + 기존가중치 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let test_large = generate_corpus_with_seed(&vocab_large, test_count, 99);
    eprint!("  DB...");
    let t = Instant::now();
    let db_c = DictionaryDb::open_in_memory().expect("db");
    DbBuilder::new(db_c.conn()).with_config(cfg.clone())
        .build_large_with_vocab_chunked(&vocab_large, train_vol).expect("build");
    eprintln!(" {:.1}s", t.elapsed().as_secs_f64());
    eprint!("  음소맵...");
    let t = Instant::now();
    let pmap_c = build_phonetic_map_chunked(&vocab_large, train_vol);
    eprintln!(" {:.1}s", t.elapsed().as_secs_f64());
    eprint!("  평가...");
    let t = Instant::now();
    let m_c = bench(&test_large, &db_c, &ng_empty, &pmap_c, 0.3, 0.5, 0.2, 0.0);
    eprintln!(" {:.1}% ({:.1}s)", m_c.exact_pct(), t.elapsed().as_secs_f64());
    println!();

    // ── ARM D: Large vocab, optimal ──
    println!("━━━ [D] 확장어휘 + 최적조합 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    eprint!("  인접문장DB...");
    let t = Instant::now();
    let db_d = DictionaryDb::open_in_memory().expect("db");
    DbBuilder::new(db_d.conn()).with_config(cfg.clone())
        .build_large_with_adjacent_sentences(&vocab_large, train_vol, 0.2).expect("build");
    eprintln!(" {:.1}s", t.elapsed().as_secs_f64());
    eprint!("  N-gram...");
    let t = Instant::now();
    let ng_d = build_ngram_model_chunked(&vocab_large, train_vol);
    eprintln!(" {:.1}s — {}", t.elapsed().as_secs_f64(), ng_d.stats());
    eprint!("  평가...");
    let t = Instant::now();
    let m_d = bench(&test_large, &db_d, &ng_d, &pmap_c, 0.1, 0.35, 0.25, 0.3);
    eprintln!(" {:.1}% ({:.1}s)", m_d.exact_pct(), t.elapsed().as_secs_f64());
    println!();

    // ── Summary ──
    let results: Vec<(&str, &Metrics)> = vec![
        ("기존어휘+기존가중치", &m_a),
        ("기존어휘+최적조합", &m_b),
        ("확장어휘+기존가중치", &m_c),
        ("확장어휘+최적조합", &m_d),
    ];

    println!("╔═══════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                          어휘 확장 벤치마크 결과 (500K)                                       ║");
    println!("╠════════════════════════╤════════╤════════╤════════╤════════╤════════╤════════╤════════╤══════╣");
    println!("║ 설정                   │ Hira   │ Hira   │ Top-1  │ Read   │ Top-3  │ Top-10 │  Doc   │ 단어 ║");
    println!("║                        │ Hit    │ Top-1  │ Surf   │ Match  │ Hit    │ Hit    │ Match  │ 수   ║");
    println!("╠════════════════════════╪════════╪════════╪════════╪════════╪════════╪════════╪════════╪══════╣");

    for (label, m) in &results {
        println!("  {:>22} │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │{:>5} │",
            label, m.hira_hit_pct(), m.hira_top1_pct(), m.exact_pct(),
            m.reading_pct(), m.top3_pct(), m.top10_pct(), m.doc_pct(), m.total_words);
    }

    println!("╚════════════════════════╧════════╧════════╧════════╧════════╧════════╧════════╧════════╧══════╝");

    // Progression chart
    println!();
    println!("  Top-1 Surface 비교:");
    for (label, m) in &results {
        let bar = (m.exact_pct() * 0.4) as usize;
        println!("    {:>22} │{} {:.1}%", label, "█".repeat(bar), m.exact_pct());
    }

    println!();
    println!("  히라가나 적중률 비교:");
    for (label, m) in &results {
        let bar = (m.hira_hit_pct() * 0.4) as usize;
        println!("    {:>22} │{} {:.1}%", label, "█".repeat(bar), m.hira_hit_pct());
    }

    println!();
    println!("  총 소요시간: {:.1}s", global_start.elapsed().as_secs_f64());
}
