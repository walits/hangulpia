//! Wikipedia corpus benchmark: compares synthetic training vs Wikipedia-based training.
//!
//! Reads pre-processed Wikipedia JSONL corpus (from process_wiki.py) containing
//! GenSentence-compatible data, builds DBs from both synthetic and Wikipedia corpora,
//! and runs identical benchmarks to measure the impact of real-world training data.
//!
//! Usage:
//!   hj-wiki-benchmark <wiki_corpus.jsonl> [test_count]

use ime_db::generator::{generate_corpus_with_seed, GenSentence, GenWord};
use ime_db::ngram::NgramModel;
use ime_db::phonetic_decoder::*;
use ime_db::vocab::build_full_vocab;
use ime_db::{DbBuilder, DictionaryDb, TrainerConfig};
use ime_db::{cosine_similarity, weighted_average_vectors};
use std::collections::HashSet;
use std::io::BufRead;
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
    fn top10_pct(&self) -> f64 { self.pct(self.top10_hit, self.total_words) }
    fn doc_pct(&self) -> f64 { self.pct(self.full_doc_match, self.total_docs) }
    fn hira_top1_pct(&self) -> f64 { self.pct(self.hiragana_top1, self.total_words) }
    fn hira_hit_pct(&self) -> f64 { self.pct(self.hiragana_hit, self.total_words) }
}

/// Load Wikipedia JSONL corpus into GenSentence structs.
fn load_wiki_corpus(path: &str, max_sentences: usize) -> Vec<GenSentence> {
    let file = std::fs::File::open(path).expect(&format!("Cannot open: {}", path));
    let reader = std::io::BufReader::new(file);
    let mut corpus = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.is_empty() { continue; }

        let obj: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let category = obj["category"].as_str().unwrap_or("wikipedia").to_string();
        let words_arr = match obj["words"].as_array() {
            Some(a) => a,
            None => continue,
        };

        let words: Vec<GenWord> = words_arr.iter().filter_map(|w| {
            let surface = w["surface"].as_str()?.to_string();
            let reading = w["reading"].as_str()?.to_string();
            let hangul = w["hangul"].as_str()?.to_string();
            Some(GenWord { surface, reading, hangul })
        }).collect();

        if words.len() >= 2 {
            corpus.push(GenSentence { category, words });
        }

        if corpus.len() >= max_sentences {
            break;
        }
    }
    corpus
}

/// 4-factor ranking with n-gram
fn rank_4factor(
    candidates: &[(String, String, f64)],
    context_words: &[&str],
    db: &DictionaryDb,
    ngram: &NgramModel,
    alpha: f64, beta: f64, gamma: f64, delta: f64,
) -> Vec<(String, String, f64)> {
    let embed_store = db.embedding_store();
    let kanji_dict = db.kanji_dict();
    let max_freq_f = 10000.0;

    let context_embeddings: Vec<(Vec<f32>, f32)> = context_words.iter().enumerate()
        .filter_map(|(i, &word)| {
            embed_store.get_embedding(word).ok().flatten().map(|emb| {
                let distance = (context_words.len() - i) as f32;
                (emb, 1.0 / distance)
            })
        }).collect();

    let context_vector = if context_embeddings.is_empty() { None } else {
        let (vecs, weights): (Vec<Vec<f32>>, Vec<f32>) = context_embeddings.into_iter().unzip();
        let avg = weighted_average_vectors(&vecs, &weights);
        if avg.iter().all(|&v| v.abs() < 1e-10) { None } else { Some(avg) }
    };

    let ngram_ctx: Vec<&str> = context_words.iter().rev().take(2).rev().copied().collect();

    let mut ranked: Vec<(String, String, f64)> = Vec::new();

    for (hiragana, _, phoneme_conf) in candidates {
        let kanji_entries = kanji_dict.lookup(hiragana).unwrap_or_default();
        let mut entries: Vec<(String, i64)> = kanji_entries.into_iter()
            .map(|e| (e.surface, e.frequency)).collect();
        if !entries.iter().any(|(s, _)| s == hiragana) {
            entries.push((hiragana.clone(), 0));
        }

        for (surface, freq) in entries {
            let ctx_score = context_vector.as_ref()
                .and_then(|ctx| embed_store.get_embedding(&surface).ok().flatten()
                    .map(|emb| (cosine_similarity(&emb, ctx) + 1.0) / 2.0))
                .unwrap_or(0.5);
            let freq_score = freq as f64 / max_freq_f;
            let ng_score = ngram.normalized_score(&ngram_ctx, &surface);

            let score = alpha * phoneme_conf + beta * ctx_score + gamma * freq_score + delta * ng_score;
            ranked.push((surface, hiragana.clone(), score));
        }
    }

    ranked.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let mut seen = HashSet::new();
    ranked.retain(|(s, _, _)| seen.insert(s.clone()));
    ranked
}

fn benchmark_arm(
    test_sentences: &[GenSentence],
    db: &DictionaryDb,
    ngram: &NgramModel,
    phonetic_map: &PhoneticMap,
    alpha: f64, beta: f64, gamma: f64, delta: f64,
) -> Metrics {
    let mut total = Metrics { total_docs: test_sentences.len(), ..Default::default() };

    for sentence in test_sentences {
        let mut confirmed: Vec<String> = Vec::new();
        let mut doc_exact = 0usize;
        let mut doc_words = 0usize;

        for word in &sentence.words {
            total.total_words += 1;
            doc_words += 1;

            let marked = hiragana_to_hangul_marked(&word.reading);
            let decoder = BeamDecoder::new(phonetic_map, 8, 20);
            let candidates: Vec<(String, String, f64)> = decoder.decode(&marked)
                .into_iter()
                .map(|(h, c)| (h, String::new(), c))
                .collect();

            if candidates.is_empty() { confirmed.push(word.surface.clone()); continue; }

            if candidates.iter().any(|(h, _, _)| *h == word.reading) { total.hiragana_hit += 1; }
            if candidates.first().map(|(h, _, _)| h.as_str()) == Some(word.reading.as_str()) {
                total.hiragana_top1 += 1;
            }

            let ctx: Vec<&str> = confirmed.iter().map(|s| s.as_str()).collect();
            let ranked = rank_4factor(&candidates, &ctx, db, ngram, alpha, beta, gamma, delta);

            if let Some(top) = ranked.first() {
                if top.0 == word.surface { total.exact_match += 1; doc_exact += 1; }
                if top.1 == word.reading { total.reading_match += 1; }
            }
            if ranked.iter().take(3).any(|r| r.0 == word.surface) { total.top3_hit += 1; }
            if ranked.iter().take(10).any(|r| r.0 == word.surface) { total.top10_hit += 1; }

            confirmed.push(word.surface.clone());
        }
        if doc_exact == doc_words && doc_words > 0 { total.full_doc_match += 1; }
    }
    total
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: hj-wiki-benchmark <wiki_corpus.jsonl> [test_count] [wiki_train_count]");
        std::process::exit(1);
    }

    let wiki_path = &args[1];
    let test_count: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(500);
    let wiki_train_count: usize = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(100_000);
    let synthetic_train_count: usize = wiki_train_count; // Match the synthetic count

    let vocab = build_full_vocab();

    // Optimal weights from previous benchmarks
    let alpha = 0.1;
    let beta = 0.35;
    let gamma = 0.25;
    let delta = 0.3;

    println!("╔══════════════════════════════════════════════════════════════════════════╗");
    println!("║     위키피디아 코퍼스 벤치마크 — 합성 vs 위키 학습 데이터 비교            ║");
    println!("╚══════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  위키 코퍼스: {}", wiki_path);
    println!("  위키 학습: {}K 문장", wiki_train_count / 1000);
    println!("  합성 학습: {}K 문장 (비교 기준)", synthetic_train_count / 1000);
    println!("  테스트: {} 건", test_count);
    println!("  가중치: α={}, β={}, γ={}, δ={}", alpha, beta, gamma, delta);
    println!();

    let global_start = Instant::now();

    // ── Load Wikipedia corpus ───────────────────────────────────────
    print!("  [1/8] 위키피디아 코퍼스 로딩...");
    let t = Instant::now();
    let wiki_corpus = load_wiki_corpus(wiki_path, wiki_train_count);
    println!(" {} 문장 ({:.1}s)", wiki_corpus.len(), t.elapsed().as_secs_f64());

    // Show corpus stats
    let mut total_words = 0usize;
    let mut unique_surfaces: HashSet<String> = HashSet::new();
    let mut unique_readings: HashSet<String> = HashSet::new();
    for s in &wiki_corpus {
        for w in &s.words {
            total_words += 1;
            unique_surfaces.insert(w.surface.clone());
            unique_readings.insert(w.reading.clone());
        }
    }
    println!("    → 총 {} 단어, {} 고유 표면형, {} 고유 읽기",
        total_words, unique_surfaces.len(), unique_readings.len());
    println!();

    // ── Build synthetic DB (baseline) ───────────────────────────────
    println!("━━━ 합성 코퍼스 인프라 (기준) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    print!("  [2/8] 합성 DB (인접문장, cw=0.2)...");
    let t = Instant::now();
    let db_synth = DictionaryDb::open_in_memory().expect("db");
    let cfg = TrainerConfig { dim: 64, iterations: 50, ..Default::default() };
    DbBuilder::new(db_synth.conn()).with_config(cfg.clone())
        .build_large_with_adjacent_sentences(&vocab, synthetic_train_count, 0.2).expect("build");
    println!(" {:.1}s", t.elapsed().as_secs_f64());

    print!("  [3/8] 합성 음소맵 (마커+멀티토큰)...");
    let t = Instant::now();
    let map_synth = build_phonetic_map_marked_multitoken_chunked(&vocab, synthetic_train_count);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), map_synth.stats());

    print!("  [4/8] 합성 N-gram...");
    let t = Instant::now();
    let mut ngram_synth = NgramModel::new();
    let synth_corpus = generate_corpus_with_seed(&vocab, synthetic_train_count, 42);
    ngram_synth.build_from_generated(&synth_corpus);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), ngram_synth.stats());
    drop(synth_corpus); // Free memory
    println!();

    // ── Build Wikipedia DB ──────────────────────────────────────────
    println!("━━━ 위키피디아 코퍼스 인프라 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    print!("  [5/8] 위키 DB (인접문장, cw=0.2)...");
    let t = Instant::now();
    let db_wiki = DictionaryDb::open_in_memory().expect("db");
    DbBuilder::new(db_wiki.conn()).with_config(cfg.clone())
        .build_from_external_corpus(&wiki_corpus, 0.2, 1).expect("build");
    println!(" {:.1}s", t.elapsed().as_secs_f64());

    print!("  [6/8] 위키 음소맵 (마커+멀티토큰)...");
    let t = Instant::now();
    let map_wiki = build_phonetic_map_marked_multitoken_from_generated(&wiki_corpus);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), map_wiki.stats());

    print!("  [7/8] 위키 N-gram...");
    let t = Instant::now();
    let mut ngram_wiki = NgramModel::new();
    ngram_wiki.build_from_generated(&wiki_corpus);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), ngram_wiki.stats());

    // ── Hybrid: Wiki embeddings + Synthetic phonetic map ────────────
    // Build combined n-gram (wiki + synthetic)
    println!();
    println!("━━━ 하이브리드 모드: 위키 임베딩 + 합성 음소맵 ━━━━━━━━━━━━━━━━━━━━━");
    let mut ngram_hybrid = NgramModel::new();
    ngram_hybrid.build_from_generated(&wiki_corpus);
    // Also add synthetic n-grams by regenerating
    let synth_for_ngram = generate_corpus_with_seed(&vocab, synthetic_train_count, 42);
    ngram_hybrid.build_from_generated(&synth_for_ngram);
    drop(synth_for_ngram);
    println!("  하이브리드 N-gram: {}", ngram_hybrid.stats());

    println!();

    // ── Test data (from synthetic, as ground truth) ─────────────────
    let test_sentences = generate_corpus_with_seed(&vocab, test_count, 99);
    println!("  [8/8] 테스트: {} 건 (합성 생성, seed=99)", test_sentences.len());
    println!();

    // ── Benchmark ───────────────────────────────────────────────────
    println!("━━━ 벤치마크 실행 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    struct ArmResult {
        label: String,
        metrics: Metrics,
        elapsed: f64,
    }

    let mut results: Vec<ArmResult> = Vec::new();

    // Arm 1: Pure synthetic (baseline)
    {
        eprint!("  [1/5] 합성 코퍼스 (기준)...");
        let t = Instant::now();
        let m = benchmark_arm(&test_sentences, &db_synth, &ngram_synth, &map_synth,
            alpha, beta, gamma, delta);
        let elapsed = t.elapsed().as_secs_f64();
        eprintln!(" Top1={:.1}% {:.1}s", m.exact_pct(), elapsed);
        results.push(ArmResult { label: "①합성(기준)".into(), metrics: m, elapsed });
    }

    // Arm 2: Pure Wikipedia
    {
        eprint!("  [2/5] 위키 코퍼스...");
        let t = Instant::now();
        let m = benchmark_arm(&test_sentences, &db_wiki, &ngram_wiki, &map_wiki,
            alpha, beta, gamma, delta);
        let elapsed = t.elapsed().as_secs_f64();
        eprintln!(" Top1={:.1}% {:.1}s", m.exact_pct(), elapsed);
        results.push(ArmResult { label: "②위키피디아".into(), metrics: m, elapsed });
    }

    // Arm 3: Wiki embeddings + Synthetic phonetic
    {
        eprint!("  [3/5] 하이브리드(위키DB+합성음소)...");
        let t = Instant::now();
        let m = benchmark_arm(&test_sentences, &db_wiki, &ngram_wiki, &map_synth,
            alpha, beta, gamma, delta);
        let elapsed = t.elapsed().as_secs_f64();
        eprintln!(" Top1={:.1}% {:.1}s", m.exact_pct(), elapsed);
        results.push(ArmResult { label: "③위키DB+합성음소".into(), metrics: m, elapsed });
    }

    // Arm 4: Synthetic DB + Wiki phonetic
    {
        eprint!("  [4/5] 하이브리드(합성DB+위키음소)...");
        let t = Instant::now();
        let m = benchmark_arm(&test_sentences, &db_synth, &ngram_synth, &map_wiki,
            alpha, beta, gamma, delta);
        let elapsed = t.elapsed().as_secs_f64();
        eprintln!(" Top1={:.1}% {:.1}s", m.exact_pct(), elapsed);
        results.push(ArmResult { label: "④합성DB+위키음소".into(), metrics: m, elapsed });
    }

    // Arm 5: Wiki DB + Synthetic phonetic + Hybrid n-gram
    {
        eprint!("  [5/5] 하이브리드(위키DB+합성음소+통합ng)...");
        let t = Instant::now();
        let m = benchmark_arm(&test_sentences, &db_wiki, &ngram_hybrid, &map_synth,
            alpha, beta, gamma, delta);
        let elapsed = t.elapsed().as_secs_f64();
        eprintln!(" Top1={:.1}% {:.1}s", m.exact_pct(), elapsed);
        results.push(ArmResult { label: "⑤위키DB+합성음소+통합ng".into(), metrics: m, elapsed });
    }

    // Also test with wiki corpus as test data
    println!();
    println!("━━━ 추가: 위키피디아 문장으로 테스트 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Use last 500 wiki sentences (not used in training) as test
    let wiki_test_start = wiki_corpus.len().saturating_sub(500);
    let wiki_test = &wiki_corpus[wiki_test_start..];
    println!("  위키 테스트: {} 건 (코퍼스 마지막 부분)", wiki_test.len());

    let mut wiki_test_results: Vec<ArmResult> = Vec::new();

    {
        eprint!("  [W1] 합성 DB로 위키 테스트...");
        let t = Instant::now();
        let m = benchmark_arm(wiki_test, &db_synth, &ngram_synth, &map_synth,
            alpha, beta, gamma, delta);
        let elapsed = t.elapsed().as_secs_f64();
        eprintln!(" Top1={:.1}% {:.1}s", m.exact_pct(), elapsed);
        wiki_test_results.push(ArmResult { label: "W①합성→위키테스트".into(), metrics: m, elapsed });
    }

    {
        eprint!("  [W2] 위키 DB로 위키 테스트...");
        let t = Instant::now();
        let m = benchmark_arm(wiki_test, &db_wiki, &ngram_wiki, &map_wiki,
            alpha, beta, gamma, delta);
        let elapsed = t.elapsed().as_secs_f64();
        eprintln!(" Top1={:.1}% {:.1}s", m.exact_pct(), elapsed);
        wiki_test_results.push(ArmResult { label: "W②위키→위키테스트".into(), metrics: m, elapsed });
    }

    {
        eprint!("  [W3] 하이브리드→위키 테스트...");
        let t = Instant::now();
        let m = benchmark_arm(wiki_test, &db_wiki, &ngram_hybrid, &map_synth,
            alpha, beta, gamma, delta);
        let elapsed = t.elapsed().as_secs_f64();
        eprintln!(" Top1={:.1}% {:.1}s", m.exact_pct(), elapsed);
        wiki_test_results.push(ArmResult { label: "W③하이브리드→위키테스트".into(), metrics: m, elapsed });
    }

    // ── Results table ───────────────────────────────────────────────
    let baseline = results[0].metrics.exact_pct();

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                          합성 테스트 데이터 기준 결과                                                    ║");
    println!("╠═══════════════════════════════╤════════╤════════╤════════╤════════╤════════╤════════╤═════════╤═════════╣");
    println!("║ 설정                          │ Hira   │ Hira   │ Top-1  │ Top-3  │ Top-10 │ Doc    │  시간   │ Δ기준   ║");
    println!("║                               │ Top-1  │ Hit    │ Surf   │ Hit    │ Hit    │ Match  │         │         ║");
    println!("╠═══════════════════════════════╪════════╪════════╪════════╪════════╪════════╪════════╪═════════╪═════════╣");

    for r in &results {
        let d = r.metrics.exact_pct() - baseline;
        let ds = if d.abs() < 0.05 { "  기준".into() } else { format!("{:>+.1}%p", d) };
        println!("║ {:>29} │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}s  │ {:>7} ║",
            r.label,
            r.metrics.hira_top1_pct(), r.metrics.hira_hit_pct(),
            r.metrics.exact_pct(), r.metrics.top3_pct(),
            r.metrics.top10_pct(), r.metrics.doc_pct(),
            r.elapsed, ds);
    }
    println!("╚═══════════════════════════════╧════════╧════════╧════════╧════════╧════════╧════════╧═════════╧═════════╝");

    // Wiki test results
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                          위키피디아 테스트 데이터 기준 결과                                               ║");
    println!("╠═══════════════════════════════╤════════╤════════╤════════╤════════╤════════╤════════╤═════════╤═════════╣");
    println!("║ 설정                          │ Hira   │ Hira   │ Top-1  │ Top-3  │ Top-10 │ Doc    │  시간   │ Δ기준   ║");
    println!("║                               │ Top-1  │ Hit    │ Surf   │ Hit    │ Hit    │ Match  │         │         ║");
    println!("╠═══════════════════════════════╪════════╪════════╪════════╪════════╪════════╪════════╪═════════╪═════════╣");

    let wiki_baseline = wiki_test_results[0].metrics.exact_pct();
    for r in &wiki_test_results {
        let d = r.metrics.exact_pct() - wiki_baseline;
        let ds = if d.abs() < 0.05 { "  기준".into() } else { format!("{:>+.1}%p", d) };
        println!("║ {:>29} │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}s  │ {:>7} ║",
            r.label,
            r.metrics.hira_top1_pct(), r.metrics.hira_hit_pct(),
            r.metrics.exact_pct(), r.metrics.top3_pct(),
            r.metrics.top10_pct(), r.metrics.doc_pct(),
            r.elapsed, ds);
    }
    println!("╚═══════════════════════════════╧════════╧════════╧════════╧════════╧════════╧════════╧═════════╧═════════╝");

    // ── Phase 2: Combined (Synthetic + Wiki) ──────────────────────
    println!();
    println!("━━━ 혼합 학습: 합성 + 위키피디아 코퍼스 결합 ━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Build combined corpus (use smaller wiki subset to fit in memory)
    let wiki_subset_size = 20_000.min(wiki_corpus.len());
    print!("  [C1] 합성 코퍼스 생성...");
    let t = Instant::now();
    let synth_corpus_for_combine = generate_corpus_with_seed(&vocab, synthetic_train_count, 42);
    println!(" {:.1}s ({} 문장)", t.elapsed().as_secs_f64(), synth_corpus_for_combine.len());

    // Combine: synthetic + wiki subset
    print!("  [C2] 혼합 코퍼스 구성 (위키 {}K 사용)...", wiki_subset_size / 1000);
    let t = Instant::now();
    let mut combined_corpus: Vec<GenSentence> = Vec::with_capacity(
        synth_corpus_for_combine.len() + wiki_subset_size
    );
    combined_corpus.extend(synth_corpus_for_combine.iter().cloned());
    combined_corpus.extend(wiki_corpus[..wiki_subset_size].iter().cloned());
    println!(" {:.1}s ({} 문장 = 합성 {} + 위키 {})",
        t.elapsed().as_secs_f64(), combined_corpus.len(),
        synth_corpus_for_combine.len(), wiki_subset_size);
    drop(synth_corpus_for_combine);

    // Build combined DB
    print!("  [C3] 혼합 DB (인접문장, cw=0.2)...");
    let t = Instant::now();
    let db_combined = DictionaryDb::open_in_memory().expect("db");
    DbBuilder::new(db_combined.conn()).with_config(cfg.clone())
        .build_from_external_corpus(&combined_corpus, 0.2, 1).expect("build");
    println!(" {:.1}s", t.elapsed().as_secs_f64());

    // Combined phonetic map (from combined corpus)
    print!("  [C4] 혼합 음소맵...");
    let t = Instant::now();
    let map_combined = build_phonetic_map_marked_multitoken_from_generated(&combined_corpus);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), map_combined.stats());

    // Combined n-gram
    print!("  [C5] 혼합 N-gram...");
    let t = Instant::now();
    let mut ngram_combined = NgramModel::new();
    ngram_combined.build_from_generated(&combined_corpus);
    println!(" {:.1}s — {}", t.elapsed().as_secs_f64(), ngram_combined.stats());
    drop(combined_corpus);

    // Test combined on both test sets
    println!();
    println!("  혼합 모델 벤치마크:");

    {
        eprint!("  [C-S] 혼합→합성 테스트...");
        let t = Instant::now();
        let m = benchmark_arm(&test_sentences, &db_combined, &ngram_combined, &map_combined,
            alpha, beta, gamma, delta);
        let elapsed = t.elapsed().as_secs_f64();
        let d = m.exact_pct() - baseline;
        eprintln!(" Top1={:.1}% (Δ{:>+.1}%p vs 합성기준) {:.1}s", m.exact_pct(), d, elapsed);
        results.push(ArmResult { label: "⑥혼합(합성+위키)".into(), metrics: m, elapsed });
    }

    {
        eprint!("  [C-W] 혼합→위키 테스트...");
        let t = Instant::now();
        let m = benchmark_arm(wiki_test, &db_combined, &ngram_combined, &map_combined,
            alpha, beta, gamma, delta);
        let elapsed = t.elapsed().as_secs_f64();
        let d = m.exact_pct() - wiki_baseline;
        eprintln!(" Top1={:.1}% (Δ{:>+.1}%p vs 합성→위키기준) {:.1}s", m.exact_pct(), d, elapsed);
        wiki_test_results.push(ArmResult { label: "W④혼합→위키테스트".into(), metrics: m, elapsed });
    }

    // Also test: combined DB + synthetic phonetic (best of both)
    {
        eprint!("  [C-H] 혼합DB+합성음소→합성 테스트...");
        let t = Instant::now();
        let m = benchmark_arm(&test_sentences, &db_combined, &ngram_combined, &map_synth,
            alpha, beta, gamma, delta);
        let elapsed = t.elapsed().as_secs_f64();
        let d = m.exact_pct() - baseline;
        eprintln!(" Top1={:.1}% (Δ{:>+.1}%p) {:.1}s", m.exact_pct(), d, elapsed);
        results.push(ArmResult { label: "⑦혼합DB+합성음소".into(), metrics: m, elapsed });
    }

    {
        eprint!("  [C-H2] 혼합DB+합성음소→위키 테스트...");
        let t = Instant::now();
        let m = benchmark_arm(wiki_test, &db_combined, &ngram_combined, &map_synth,
            alpha, beta, gamma, delta);
        let elapsed = t.elapsed().as_secs_f64();
        let d = m.exact_pct() - wiki_baseline;
        eprintln!(" Top1={:.1}% (Δ{:>+.1}%p) {:.1}s", m.exact_pct(), d, elapsed);
        wiki_test_results.push(ArmResult { label: "W⑤혼합DB+합성음소".into(), metrics: m, elapsed });
    }

    // ── Updated results tables ──────────────────────────────────────
    let baseline = results[0].metrics.exact_pct();

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                          합성 테스트 데이터 기준 최종 결과                                                ║");
    println!("╠═══════════════════════════════╤════════╤════════╤════════╤════════╤════════╤════════╤═════════╤═════════╣");
    println!("║ 설정                          │ Hira   │ Hira   │ Top-1  │ Top-3  │ Top-10 │ Doc    │  시간   │ Δ기준   ║");
    println!("║                               │ Top-1  │ Hit    │ Surf   │ Hit    │ Hit    │ Match  │         │         ║");
    println!("╠═══════════════════════════════╪════════╪════════╪════════╪════════╪════════╪════════╪═════════╪═════════╣");

    for r in &results {
        let d = r.metrics.exact_pct() - baseline;
        let ds = if d.abs() < 0.05 { "  기준".into() } else { format!("{:>+.1}%p", d) };
        println!("║ {:>29} │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}s  │ {:>7} ║",
            r.label,
            r.metrics.hira_top1_pct(), r.metrics.hira_hit_pct(),
            r.metrics.exact_pct(), r.metrics.top3_pct(),
            r.metrics.top10_pct(), r.metrics.doc_pct(),
            r.elapsed, ds);
    }
    println!("╚═══════════════════════════════╧════════╧════════╧════════╧════════╧════════╧════════╧═════════╧═════════╝");

    let wiki_baseline = wiki_test_results[0].metrics.exact_pct();

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                          위키피디아 테스트 데이터 기준 최종 결과                                           ║");
    println!("╠═══════════════════════════════╤════════╤════════╤════════╤════════╤════════╤════════╤═════════╤═════════╣");
    println!("║ 설정                          │ Hira   │ Hira   │ Top-1  │ Top-3  │ Top-10 │ Doc    │  시간   │ Δ기준   ║");
    println!("║                               │ Top-1  │ Hit    │ Surf   │ Hit    │ Hit    │ Match  │         │         ║");
    println!("╠═══════════════════════════════╪════════╪════════╪════════╪════════╪════════╪════════╪═════════╪═════════╣");

    for r in &wiki_test_results {
        let d = r.metrics.exact_pct() - wiki_baseline;
        let ds = if d.abs() < 0.05 { "  기준".into() } else { format!("{:>+.1}%p", d) };
        println!("║ {:>29} │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}% │ {:>5.1}s  │ {:>7} ║",
            r.label,
            r.metrics.hira_top1_pct(), r.metrics.hira_hit_pct(),
            r.metrics.exact_pct(), r.metrics.top3_pct(),
            r.metrics.top10_pct(), r.metrics.doc_pct(),
            r.elapsed, ds);
    }
    println!("╚═══════════════════════════════╧════════╧════════╧════════╧════════╧════════╧════════╧═════════╧═════════╝");

    // ── Analysis ────────────────────────────────────────────────────
    println!();
    println!("━━━ 분석 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Best configuration
    let best = results.iter().max_by(|a, b|
        a.metrics.exact_pct().partial_cmp(&b.metrics.exact_pct()).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
    println!("  ★ 합성 테스트 최적: {} — Top-1 {:.1}% (Δ{:>+.1}%p)",
        best.label, best.metrics.exact_pct(), best.metrics.exact_pct() - baseline);

    let best_wiki = wiki_test_results.iter().max_by(|a, b|
        a.metrics.exact_pct().partial_cmp(&b.metrics.exact_pct()).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
    println!("  ★ 위키 테스트 최적: {} — Top-1 {:.1}% (Δ{:>+.1}%p)",
        best_wiki.label, best_wiki.metrics.exact_pct(), best_wiki.metrics.exact_pct() - wiki_baseline);

    // Bar chart
    println!();
    println!("  합성 테스트 Top-1 정확도:");
    let mut sorted: Vec<_> = results.iter().collect();
    sorted.sort_by(|a, b| b.metrics.exact_pct().partial_cmp(&a.metrics.exact_pct()).unwrap_or(std::cmp::Ordering::Equal));
    for r in &sorted {
        let bar_len = (r.metrics.exact_pct() * 0.5) as usize;
        println!("    {:>29} │{} {:.1}% ({:>+.1}%p)",
            r.label, "█".repeat(bar_len), r.metrics.exact_pct(), r.metrics.exact_pct() - baseline);
    }

    println!();
    println!("  위키 테스트 Top-1 정확도:");
    let mut sorted_w: Vec<_> = wiki_test_results.iter().collect();
    sorted_w.sort_by(|a, b| b.metrics.exact_pct().partial_cmp(&a.metrics.exact_pct()).unwrap_or(std::cmp::Ordering::Equal));
    for r in &sorted_w {
        let bar_len = (r.metrics.exact_pct() * 0.5) as usize;
        println!("    {:>29} │{} {:.1}% ({:>+.1}%p)",
            r.label, "█".repeat(bar_len), r.metrics.exact_pct(), r.metrics.exact_pct() - wiki_baseline);
    }

    println!();
    println!("  총 소요시간: {:.1}s", global_start.elapsed().as_secs_f64());
}
