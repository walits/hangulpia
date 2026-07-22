//! Round-trip accuracy benchmark for the Hangul↔Japanese IME.
//!
//! Pipeline per document (sentence):
//!   1. Generate a Japanese sentence (surface + reading)
//!   2. Convert each word's reading → hangul  (hiragana_to_hangul)
//!   3. Convert hangul → romaji → hiragana    (phoneme pipeline)
//!   4. Look up kanji candidates with ContextRanker (using sentence context)
//!   5. Compare the top-1 restored surface with the original surface
//!
//! Metrics:
//!   - Word-level exact match rate  (surface 일치)
//!   - Reading-level match rate     (reading/히라가나 일치)
//!   - Sentence-level full match    (문장 전체 일치)
//!   - Top-3 / Top-5 hit rate       (상위 후보 안에 정답 포함)

use ime_db::generator::{generate_corpus, GenSentence};
use ime_db::kana_hangul::hiragana_to_hangul;
use ime_db::{ContextRanker, DictionaryDb, KanjiDict};
use ime_hangul::phoneme;
use ime_japanese::romaji;
use std::collections::HashMap;
use std::time::Instant;

/// Convert a single hangul chunk back to hiragana candidates.
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

/// Result for a single word restoration attempt.
#[derive(Debug)]
struct WordResult {
    original_surface: String,
    original_reading: String,
    hangul: String,
    restored_top1: String,
    restored_reading: String,
    exact_match: bool,
    reading_match: bool,
    in_top3: bool,
    in_top5: bool,
    in_top10: bool,
    candidate_count: usize,
}

/// Result for a single sentence/document.
#[derive(Debug)]
struct DocResult {
    word_results: Vec<WordResult>,
    full_match: bool,
    category: String,
}

fn benchmark_sentence(
    sentence: &GenSentence,
    db: &DictionaryDb,
) -> DocResult {
    let ranker = db.context_ranker();

    // Phase 1: Convert all words to hangul and collect basic info.
    let mut word_infos: Vec<(String, String, String)> = Vec::new(); // (surface, reading, hangul)
    for word in &sentence.words {
        let hangul = word.hangul.clone();
        word_infos.push((word.surface.clone(), word.reading.clone(), hangul));
    }

    // Phase 2: Progressive restoration with growing context.
    // Simulate real typing: each word is converted with all previously confirmed words as context.
    let mut confirmed_surfaces: Vec<String> = Vec::new();
    let mut word_results: Vec<WordResult> = Vec::new();

    for (surface, reading, hangul) in &word_infos {
        // Step A: hangul → hiragana candidates
        let candidates = hangul_to_hiragana_candidates(hangul);

        if candidates.is_empty() {
            word_results.push(WordResult {
                original_surface: surface.clone(),
                original_reading: reading.clone(),
                hangul: hangul.clone(),
                restored_top1: String::new(),
                restored_reading: String::new(),
                exact_match: false,
                reading_match: false,
                in_top3: false,
                in_top5: false,
                in_top10: false,
                candidate_count: 0,
            });
            confirmed_surfaces.push(surface.clone());
            continue;
        }

        // Step B: rank with context from previously confirmed words
        let context_refs: Vec<&str> = confirmed_surfaces.iter().map(|s| s.as_str()).collect();
        let ranked = ranker.rank_candidates(&candidates, &context_refs, 10000);

        let top1_surface = ranked.first().map(|r| r.surface.clone()).unwrap_or_default();
        let top1_reading = ranked.first().map(|r| r.reading.clone()).unwrap_or_default();

        let exact = top1_surface == *surface;
        let reading_ok = top1_reading == *reading;
        let in_top3 = ranked.iter().take(3).any(|r| r.surface == *surface);
        let in_top5 = ranked.iter().take(5).any(|r| r.surface == *surface);
        let in_top10 = ranked.iter().take(10).any(|r| r.surface == *surface);

        word_results.push(WordResult {
            original_surface: surface.clone(),
            original_reading: reading.clone(),
            hangul: hangul.clone(),
            restored_top1: top1_surface,
            restored_reading: top1_reading,
            exact_match: exact,
            reading_match: reading_ok,
            in_top3,
            in_top5,
            in_top10,
            candidate_count: ranked.len(),
        });

        // Use original surface as context (simulating correct user selection)
        confirmed_surfaces.push(surface.clone());
    }

    let full_match = word_results.iter().all(|w| w.exact_match);

    DocResult {
        word_results,
        full_match,
        category: sentence.category.clone(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let db_path = if args.len() > 1 { &args[1] } else { "hj-ime-large.db" };
    let doc_count: usize = if args.len() > 2 {
        args[2].parse().unwrap_or(1000)
    } else {
        1000
    };

    println!("═══════════════════════════════════════════════════════════");
    println!("  한글일본어입력기 라운드트립 정확도 벤치마크");
    println!("═══════════════════════════════════════════════════════════");
    println!("  DB: {}", db_path);
    println!("  문서 수: {}", doc_count);
    println!();

    // Open database
    let db = match DictionaryDb::open(db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("DB 열기 실패: {}", e);
            std::process::exit(1);
        }
    };

    // Generate test sentences (use a different seed from training to avoid overfitting)
    println!("  [1/3] 테스트 문서 {} 건 생성 중...", doc_count);
    let start = Instant::now();

    // Generate more than needed, then skip the first 100K (training data)
    // to avoid testing on training data. Use tail of a larger generation.
    let total_gen = doc_count + 100_000;
    let all_sentences = generate_corpus(total_gen);
    let test_sentences: Vec<&GenSentence> = all_sentences.iter().skip(100_000).take(doc_count).collect();
    let actual_count = test_sentences.len();

    println!("    생성 완료: {}건 ({}ms)", actual_count, start.elapsed().as_millis());
    println!();

    // Run benchmark
    println!("  [2/3] 라운드트립 테스트 진행 중...");
    let bench_start = Instant::now();

    let mut doc_results: Vec<DocResult> = Vec::with_capacity(actual_count);
    let mut progress_interval = (actual_count / 20).max(1);

    for (i, sentence) in test_sentences.iter().enumerate() {
        let result = benchmark_sentence(sentence, &db);
        doc_results.push(result);

        if (i + 1) % progress_interval == 0 || i + 1 == actual_count {
            let pct = (i + 1) as f64 / actual_count as f64 * 100.0;
            print!("\r    진행: {:>6.1}% ({}/{})", pct, i + 1, actual_count);
            use std::io::Write;
            std::io::stdout().flush().ok();
        }
    }
    println!();
    println!("    완료 ({}ms)", bench_start.elapsed().as_millis());
    println!();

    // Compute metrics
    println!("  [3/3] 결과 분석 중...");
    println!();

    let total_words: usize = doc_results.iter().map(|d| d.word_results.len()).sum();
    let exact_match_words: usize = doc_results.iter()
        .flat_map(|d| &d.word_results)
        .filter(|w| w.exact_match)
        .count();
    let reading_match_words: usize = doc_results.iter()
        .flat_map(|d| &d.word_results)
        .filter(|w| w.reading_match)
        .count();
    let top3_words: usize = doc_results.iter()
        .flat_map(|d| &d.word_results)
        .filter(|w| w.in_top3)
        .count();
    let top5_words: usize = doc_results.iter()
        .flat_map(|d| &d.word_results)
        .filter(|w| w.in_top5)
        .count();
    let top10_words: usize = doc_results.iter()
        .flat_map(|d| &d.word_results)
        .filter(|w| w.in_top10)
        .count();
    let full_match_docs: usize = doc_results.iter()
        .filter(|d| d.full_match)
        .count();
    let no_candidate_words: usize = doc_results.iter()
        .flat_map(|d| &d.word_results)
        .filter(|w| w.candidate_count == 0)
        .count();

    // Category breakdown
    let mut cat_stats: HashMap<String, (usize, usize, usize)> = HashMap::new(); // (total_words, exact, full_doc)
    for doc in &doc_results {
        let entry = cat_stats.entry(doc.category.clone()).or_insert((0, 0, 0));
        let word_count = doc.word_results.len();
        let exact_count = doc.word_results.iter().filter(|w| w.exact_match).count();
        entry.0 += word_count;
        entry.1 += exact_count;
        entry.2 += if doc.full_match { 1 } else { 0 };
    }

    // Error analysis
    let mut error_patterns: HashMap<String, usize> = HashMap::new();
    for doc in &doc_results {
        for w in &doc.word_results {
            if !w.exact_match && !w.restored_top1.is_empty() {
                let pattern = format!("{} → {} (원본: {})", w.hangul, w.restored_top1, w.original_surface);
                *error_patterns.entry(pattern).or_insert(0) += 1;
            }
        }
    }

    // Print results
    println!("═══════════════════════════════════════════════════════════");
    println!("                    전체 결과 요약");
    println!("═══════════════════════════════════════════════════════════");
    println!();
    println!("  ■ 문서 수준 (Sentence-level)");
    println!("    전체 문서:     {:>6} 건", actual_count);
    println!("    완전 일치:     {:>6} 건 ({:.1}%)", full_match_docs, full_match_docs as f64 / actual_count as f64 * 100.0);
    println!();
    println!("  ■ 단어 수준 (Word-level)");
    println!("    전체 단어:     {:>6} 개", total_words);
    println!("    후보 없음:     {:>6} 개 ({:.1}%)", no_candidate_words, no_candidate_words as f64 / total_words as f64 * 100.0);
    println!();
    println!("    ┌──────────────────────┬──────────┬─────────┐");
    println!("    │ 지표                 │ 정답 수  │ 정확률  │");
    println!("    ├──────────────────────┼──────────┼─────────┤");
    println!("    │ Top-1 Surface 일치   │ {:>6}   │ {:>5.1}%  │", exact_match_words, exact_match_words as f64 / total_words as f64 * 100.0);
    println!("    │ Top-1 Reading 일치   │ {:>6}   │ {:>5.1}%  │", reading_match_words, reading_match_words as f64 / total_words as f64 * 100.0);
    println!("    │ Top-3 Surface 포함   │ {:>6}   │ {:>5.1}%  │", top3_words, top3_words as f64 / total_words as f64 * 100.0);
    println!("    │ Top-5 Surface 포함   │ {:>6}   │ {:>5.1}%  │", top5_words, top5_words as f64 / total_words as f64 * 100.0);
    println!("    │ Top-10 Surface 포함  │ {:>6}   │ {:>5.1}%  │", top10_words, top10_words as f64 / total_words as f64 * 100.0);
    println!("    └──────────────────────┴──────────┴─────────┘");
    println!();

    // Category breakdown
    println!("  ■ 카테고리별 정확도");
    println!("    ┌──────────────────┬────────┬──────────┬──────────┐");
    println!("    │ 카테고리         │ 단어수 │ 단어정확 │ 문서정확 │");
    println!("    ├──────────────────┼────────┼──────────┼──────────┤");
    let mut sorted_cats: Vec<_> = cat_stats.iter().collect();
    sorted_cats.sort_by_key(|(_, (total, _, _))| std::cmp::Reverse(*total));
    for (cat, (total, exact, full)) in &sorted_cats {
        let cat_docs: usize = doc_results.iter().filter(|d| &d.category == *cat).count();
        println!(
            "    │ {:<16} │ {:>5}  │ {:>6.1}%  │ {:>6.1}%  │",
            cat,
            total,
            *exact as f64 / *total as f64 * 100.0,
            *full as f64 / cat_docs as f64 * 100.0,
        );
    }
    println!("    └──────────────────┴────────┴──────────┴──────────┘");
    println!();

    // Top error patterns
    let mut sorted_errors: Vec<_> = error_patterns.iter().collect();
    sorted_errors.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
    println!("  ■ 주요 오류 패턴 (상위 20)");
    println!("    ┌────┬──────────────────────────────────────────────────────┐");
    println!("    │ 횟수│ 패턴                                               │");
    println!("    ├────┼──────────────────────────────────────────────────────┤");
    for (pattern, count) in sorted_errors.iter().take(20) {
        println!("    │ {:>3} │ {}│", count, format!("{:<52}", pattern));
    }
    println!("    └────┴──────────────────────────────────────────────────────┘");
    println!();

    // Sample failures
    println!("  ■ 실패 문서 샘플 (최대 10건)");
    let failures: Vec<_> = doc_results.iter()
        .filter(|d| !d.full_match)
        .take(10)
        .collect();
    for (i, doc) in failures.iter().enumerate() {
        let original: String = doc.word_results.iter().map(|w| w.original_surface.as_str()).collect::<Vec<_>>().join("");
        let restored: String = doc.word_results.iter().map(|w| w.restored_top1.as_str()).collect::<Vec<_>>().join("");
        let hangul: String = doc.word_results.iter().map(|w| w.hangul.as_str()).collect::<Vec<_>>().join(" ");
        println!("    [{:>2}] 원본:  {}", i + 1, original);
        println!("         한글:  {}", hangul);
        println!("         복원:  {}", restored);
        // Mark mismatched words
        let mismatches: Vec<String> = doc.word_results.iter()
            .filter(|w| !w.exact_match)
            .map(|w| format!("{}→{} (원:{}) ", w.hangul, w.restored_top1, w.original_surface))
            .collect();
        println!("         오류:  {}", mismatches.join(""));
        println!();
    }

    println!("═══════════════════════════════════════════════════════════");
    println!("  총 소요 시간: {}ms", start.elapsed().as_millis());
    println!("═══════════════════════════════════════════════════════════");
}
