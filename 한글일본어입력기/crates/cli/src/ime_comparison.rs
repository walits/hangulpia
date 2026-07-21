//! IME comparison benchmark: our system vs commercial IME baselines.
//!
//! Evaluates on metrics comparable to industry standards:
//!   - Sentence-level conversion accuracy (全文一致率)
//!   - Word-level top-1 / top-3 accuracy (変換精度)
//!   - Keystroke efficiency (KSPC: keystrokes per character)
//!   - Auto-complete keystroke saving
//!   - Breakdown by word type (noun, verb, particle, adjective)

use ime_db::autocomplete::{AutoCompleteEngine, SuggestionTier};
use ime_db::generator::{generate_corpus_with_seed, GenSentence};
use ime_db::ngram::{build_ngram_model_chunked, NgramModel};
use ime_db::phonetic_decoder::*;
use ime_db::vocab::build_full_vocab;
use ime_db::{cosine_similarity, weighted_average_vectors};
use ime_db::{DbBuilder, DictionaryDb, TrainerConfig};
use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Clone, Default)]
struct DetailedMetrics {
    // Per-word metrics
    total_words: usize,
    exact_match: usize,      // surface exact match
    top3_hit: usize,
    top5_hit: usize,
    hiragana_hit: usize,     // correct reading in beam
    // Per-sentence metrics
    total_sentences: usize,
    full_sentence_match: usize,
    // Per-type breakdown
    type_total: HashMap<String, usize>,
    type_match: HashMap<String, usize>,
    // Keystroke metrics
    total_hangul_chars: usize,   // total hangul characters typed
    total_output_chars: usize,   // total output characters (Japanese)
    keystrokes_no_ac: usize,     // without autocomplete
    keystrokes_with_ac: usize,   // with autocomplete (threshold=1)
    // Autocomplete breakdown
    ac_next_word_hits: usize,
    ac_prefix_hits: usize,
}

impl DetailedMetrics {
    fn pct(&self, n: usize, d: usize) -> f64 {
        if d == 0 { 0.0 } else { n as f64 / d as f64 * 100.0 }
    }
}

fn rank_4factor(
    candidates: &[(String, String, f64)],
    context_words: &[&str],
    db: &DictionaryDb,
    ngram: &NgramModel,
) -> Vec<(String, String, f64)> {
    let (alpha, beta, gamma, delta) = (0.1, 0.35, 0.25, 0.3);
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

/// Classify word type from the template category
fn classify_word(surface: &str, reading: &str) -> &'static str {
    // Particles
    let particles = ["は", "が", "を", "に", "で", "と", "の", "も",
                     "から", "まで", "より", "へ"];
    if particles.contains(&surface) {
        return "조사";
    }
    // Verb forms (ends with ます, て, た, ない, etc.)
    if surface.ends_with("ます") || surface.ends_with("した")
        || surface.ends_with("して") || surface.ends_with("ない")
        || surface.ends_with("った") || surface.ends_with("いた")
        || surface.ends_with("んだ") || surface.ends_with("いで")
    {
        return "동사";
    }
    // Adjectives (ends with い + です, or な-adj pattern)
    if surface.ends_with("です") {
        if reading.ends_with("いです") {
            return "형용사";
        }
        return "서술";
    }
    if surface.ends_with("い") && reading.ends_with("い") && surface.chars().count() > 1 {
        return "형용사";
    }
    "명사"
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let test_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(300);
    let train_vol: usize = 500_000;

    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║   일본어 키보드 대비 효율 비교 벤치마크                                      ║");
    println!("║   한글→일본어 IME (최적조합) vs 상용 IME 업계 기준                            ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════╝");
    println!();

    let global_start = Instant::now();
    let vocab = build_full_vocab();
    let cfg = TrainerConfig { dim: 64, iterations: 50, ..Default::default() };

    // Build infrastructure
    eprint!("  인접문장 DB...");
    let t = Instant::now();
    let db = DictionaryDb::open_in_memory().expect("db");
    DbBuilder::new(db.conn()).with_config(cfg)
        .build_large_with_adjacent_sentences(&vocab, train_vol, 0.2).expect("build");
    eprintln!(" {:.1}s", t.elapsed().as_secs_f64());

    eprint!("  음소 맵...");
    let t = Instant::now();
    let pmap = build_phonetic_map_chunked(&vocab, train_vol);
    eprintln!(" {:.1}s", t.elapsed().as_secs_f64());

    eprint!("  N-gram...");
    let t = Instant::now();
    let ngram = build_ngram_model_chunked(&vocab, train_vol);
    eprintln!(" {:.1}s — {}", t.elapsed().as_secs_f64(), ngram.stats());

    let test = generate_corpus_with_seed(&vocab, test_count, 99);
    println!("  어휘: {} | 테스트: {} 문장 | 학습: {}K", vocab.len(), test.len(), train_vol / 1000);
    println!();

    // ══════════════════════════════════════════════════════
    //  Part 1: Conversion accuracy (comparable to かな漢字変換)
    // ══════════════════════════════════════════════════════
    let mut m = DetailedMetrics::default();
    m.total_sentences = test.len();

    // Also run autocomplete simulation
    let mut ac_engine = AutoCompleteEngine::new(&db, &ngram, &pmap);
    let ng_empty = NgramModel::new();

    for sentence in &test {
        ac_engine.reset_context();
        let mut ctx: Vec<String> = Vec::new();
        let mut sent_all_match = true;

        for word in &sentence.words {
            m.total_words += 1;
            let hangul_len = word.hangul.chars().count();
            let surface_len = word.surface.chars().count();
            m.total_hangul_chars += hangul_len;
            m.total_output_chars += surface_len;
            m.keystrokes_no_ac += hangul_len + 1; // type + confirm

            // Word type classification
            let wtype = classify_word(&word.surface, &word.reading);
            *m.type_total.entry(wtype.to_string()).or_insert(0) += 1;

            // Phonetic decode
            let dec = BeamDecoder::new(&pmap, 8, 20);
            let cands: Vec<(String, String, f64)> = dec.decode(&word.hangul).into_iter()
                .map(|(h, c)| (h, String::new(), c)).collect();

            if cands.is_empty() {
                sent_all_match = false;
                m.keystrokes_with_ac += hangul_len + 1;
                ctx.push(word.surface.clone());
                ac_engine.commit_word(&word.surface);
                continue;
            }

            if cands.iter().any(|(h, _, _)| *h == word.reading) { m.hiragana_hit += 1; }

            // Rank with context
            let cv: Vec<&str> = ctx.iter().map(|s| s.as_str()).collect();
            let ranked = rank_4factor(&cands, &cv, &db, &ngram);

            if let Some(top) = ranked.first() {
                if top.0 == word.surface {
                    m.exact_match += 1;
                    *m.type_match.entry(wtype.to_string()).or_insert(0) += 1;
                } else {
                    sent_all_match = false;
                }
            } else {
                sent_all_match = false;
            }

            if ranked.iter().take(3).any(|x| x.0 == word.surface) { m.top3_hit += 1; }
            if ranked.iter().take(5).any(|x| x.0 == word.surface) { m.top5_hit += 1; }

            // Autocomplete simulation
            let hangul_chars: Vec<char> = word.hangul.chars().collect();
            let mut ac_used = false;

            // Check next-word prediction
            if !ac_engine.context().is_empty() {
                let suggestions = ac_engine.suggest("");
                if let Some(s) = suggestions.first() {
                    if s.surface == word.surface && s.tier == SuggestionTier::NextWord {
                        m.keystrokes_with_ac += 1; // just Tab
                        m.ac_next_word_hits += 1;
                        ac_used = true;
                    }
                }
            }

            // Check prefix completion
            if !ac_used {
                for i in 1..=hangul_len {
                    let partial: String = hangul_chars[..i].iter().collect();
                    let suggestions = ac_engine.suggest(&partial);
                    if let Some(top) = suggestions.first() {
                        if top.surface == word.surface && top.keystroke_saving >= 1 && i < hangul_len {
                            m.keystrokes_with_ac += i + 1; // typed chars + confirm
                            m.ac_prefix_hits += 1;
                            ac_used = true;
                            break;
                        }
                    }
                }
                if !ac_used {
                    m.keystrokes_with_ac += hangul_len + 1;
                }
            }

            ctx.push(word.surface.clone());
            ac_engine.commit_word(&word.surface);
        }

        if sent_all_match { m.full_sentence_match += 1; }
    }

    // ══════════════════════════════════════════════════════
    //  Part 2: Industry comparison table
    // ══════════════════════════════════════════════════════
    let kspc_ours = m.total_hangul_chars as f64 / m.total_output_chars as f64;
    let kspc_ours_ac = m.keystrokes_with_ac as f64 / m.total_output_chars as f64;
    let saving = 1.0 - (m.keystrokes_with_ac as f64 / m.keystrokes_no_ac as f64);

    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!("  ① 변환 정확도 (상용 IME 대비)");
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!();
    println!("  ┌────────────────────────┬──────────┬──────────┬──────────────────────────┐");
    println!("  │ 메트릭                  │ 우리 IME  │ 상용 IME  │ 비고                     │");
    println!("  │                         │ (한→일)   │ (참고치)  │                          │");
    println!("  ├────────────────────────┼──────────┼──────────┼──────────────────────────┤");
    println!("  │ Top-1 변환 정확도       │ {:>6.1}%  │  ~95%    │ かな→漢字 단일 단어       │",
        m.pct(m.exact_match, m.total_words));
    println!("  │ Top-3 적중률            │ {:>6.1}%  │  ~99%    │ 후보 리스트 내 정답 포함  │",
        m.pct(m.top3_hit, m.total_words));
    println!("  │ Top-5 적중률            │ {:>6.1}%  │  ~99%+   │                          │",
        m.pct(m.top5_hit, m.total_words));
    println!("  │ 문장 전체 일치          │ {:>6.1}%  │  ~80%    │ 문절 변환 기준            │",
        m.pct(m.full_sentence_match, m.total_sentences));
    println!("  │ 히라가나 디코딩         │ {:>6.1}%  │   100%   │ 일반 IME는 직접 입력      │",
        m.pct(m.hiragana_hit, m.total_words));
    println!("  └────────────────────────┴──────────┴──────────┴──────────────────────────┘");

    println!();
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!("  ② 키입력 효율 (KSPC: Keystrokes Per Character)");
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!();
    println!("  ┌────────────────────────┬──────────┬──────────────────────────────────────┐");
    println!("  │ 입력 방식               │  KSPC    │ 설명                                 │");
    println!("  ├────────────────────────┼──────────┼──────────────────────────────────────┤");
    println!("  │ 로마자 입력 (일반)      │  ~2.0    │ 2타=1かな (ka→か, shi→し)            │");
    println!("  │ かな 직접 입력          │  ~1.0    │ 1타=1かな (JIS かな배열)              │");
    println!("  │ 플릭 입력 (스마트폰)    │  ~1.15   │ KSPC 1.15 (フリック방식)             │");
    println!("  │ 로마자+예측변환         │  ~1.2    │ 예측으로 ~40% 절감                   │");
    println!("  │ 한글 입력 (우리 IME)    │ {:>7.2}  │ 한글 음소 대응                       │",
        kspc_ours);
    println!("  │ 한글+자동완성 (우리)    │ {:>7.2}  │ 접두사완성+다음단어 예측              │",
        kspc_ours_ac);
    println!("  └────────────────────────┴──────────┴──────────────────────────────────────┘");

    println!();
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!("  ③ 자동완성 효과");
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!();
    println!("  총 단어: {} | 키입력(기본): {} | 키입력(AC): {} | 절감률: {:.1}%",
        m.total_words, m.keystrokes_no_ac, m.keystrokes_with_ac, saving * 100.0);
    println!("  다음단어 예측 적중: {} ({:.1}%) | 접두사 완성: {} ({:.1}%)",
        m.ac_next_word_hits, m.pct(m.ac_next_word_hits, m.total_words),
        m.ac_prefix_hits, m.pct(m.ac_prefix_hits, m.total_words));
    println!();

    // KSPC comparison
    println!("  ┌─ 업계 예측변환 절감률 비교 ─────────────────────────────────────────────┐");
    println!("  │ AI 예측변환 (IO사 2018)        │  ~67%  │ 24타→8타 (플릭, 일본어)        │");
    println!("  │ 상용 IME 예측 (Google/ATOK)    │ ~30-40%│ 로마자 입력 기준               │");
    println!("  │ 우리 IME 자동완성              │ {:>5.1}% │ 한글 입력 기준                 │",
        saving * 100.0);
    println!("  └────────────────────────────────┴────────┴────────────────────────────────┘");

    println!();
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!("  ④ 품사별 정확도");
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!();

    let mut type_list: Vec<_> = m.type_total.iter().collect();
    type_list.sort_by(|a, b| b.1.cmp(a.1));

    println!("  ┌──────────┬────────┬────────┬────────┐");
    println!("  │ 품사      │  전체   │  적중   │ 정확도 │");
    println!("  ├──────────┼────────┼────────┼────────┤");
    for (wtype, total) in &type_list {
        let matched = m.type_match.get(wtype.as_str()).copied().unwrap_or(0);
        let bar_len = (m.pct(matched, **total) * 0.3) as usize;
        println!("  │ {:>8} │ {:>6} │ {:>6} │ {:>5.1}% │ {}",
            wtype, total, matched, m.pct(matched, **total), "█".repeat(bar_len));
    }
    println!("  └──────────┴────────┴────────┴────────┘");

    // Summary comparison
    println!();
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!("  ⑤ 종합 비교 요약");
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!();
    println!("  ┌───────────────────────────┬──────────────────┬──────────────────────────┐");
    println!("  │ 항목                       │ 일반 일본어 IME   │ 한글→일본어 IME (우리)   │");
    println!("  ├───────────────────────────┼──────────────────┼──────────────────────────┤");
    println!("  │ 입력 소스                  │ 로마자/かな       │ 한글                     │");
    println!("  │ 입력→ひらがな              │ 확정적 (100%)     │ 확률적 ({:.1}%)          │",
        m.pct(m.hiragana_hit, m.total_words));
    println!("  │ ひらがな→漢字              │ ~95% Top-1       │ 4-factor ranking         │");
    println!("  │ 최종 Top-1 정확도          │ ~95%             │ {:.1}%                   │",
        m.pct(m.exact_match, m.total_words));
    println!("  │ 예측변환 절감률            │ 30~40%           │ {:.1}%                   │",
        saving * 100.0);
    println!("  │ KSPC                       │ ~1.2 (예측 포함)  │ {:.2}                    │",
        kspc_ours_ac);

    let our_gap = 95.0 - m.pct(m.exact_match, m.total_words);
    println!("  │ 상용 대비 격차             │ (기준)            │ -{:.1}%p                  │",
        our_gap);
    println!("  │ 격차의 주요 원인           │                  │ 한글→ひらがな 변환 손실   │");
    println!("  └───────────────────────────┴──────────────────┴──────────────────────────┘");

    println!();
    println!("  총 소요시간: {:.1}s", global_start.elapsed().as_secs_f64());
}
