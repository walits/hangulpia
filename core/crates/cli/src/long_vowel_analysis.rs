//! Long vowel (長音) analysis and marker experiment.
//!
//! Analyzes:
//!   1. How many vocabulary words contain long vowels
//!   2. How many hiragana decoding errors are caused by long vowel ambiguity
//!   3. Impact of a '~' marker on hiragana decoding accuracy
//!   4. Impact on final Top-1 surface accuracy

use ime_db::generator::generate_corpus_with_seed;
use ime_db::kana_hangul::hiragana_to_hangul;
use ime_db::ngram::{build_ngram_model_chunked, NgramModel};
use ime_db::phonetic_decoder::*;
use ime_db::vocab::build_full_vocab;
use ime_db::{cosine_similarity, weighted_average_vectors};
use ime_db::{DbBuilder, DictionaryDb, TrainerConfig};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Long vowel patterns in Japanese hiragana
const LONG_VOWEL_PATTERNS: &[(&str, &str)] = &[
    // おう → おお (o-column long vowels written as おう)
    ("おう", "오우→오~"), ("こう", "코우→코~"), ("そう", "소우→소~"),
    ("とう", "토우→토~"), ("のう", "노우→노~"), ("ほう", "호우→호~"),
    ("もう", "모우→모~"), ("よう", "요우→요~"), ("ろう", "로우→로~"),
    ("ごう", "고우→고~"), ("ぞう", "조우→조~"), ("どう", "도우→도~"),
    ("ぼう", "보우→보~"), ("ぽう", "포우→포~"),
    ("きょう", "쿄우→쿄~"), ("しょう", "쇼우→쇼~"), ("ちょう", "쵸우→쵸~"),
    ("にょう", "뇨우→뇨~"), ("ひょう", "효우→효~"), ("みょう", "묘우→묘~"),
    ("りょう", "료우→료~"), ("ぎょう", "교우→교~"), ("じょう", "죠우→죠~"),
    ("びょう", "뵤우→뵤~"),
    // えい → ええ (e-column long vowels written as えい)
    ("えい", "에이→에~"), ("けい", "케이→케~"), ("せい", "세이→세~"),
    ("てい", "테이→테~"), ("ねい", "네이→네~"), ("へい", "헤이→헤~"),
    ("めい", "메이→메~"), ("れい", "레이→레~"),
    ("げい", "게이→게~"), ("ぜい", "제이→제~"), ("でい", "데이→데~"),
    ("べい", "베이→베~"), ("ぺい", "페이→페~"),
    // うう (u-column double vowels)
    ("くう", "쿠우→쿠~"), ("すう", "스우→스~"), ("つう", "츠우→츠~"),
    ("ふう", "후우→후~"), ("ぐう", "구우→구~"), ("ずう", "즈우→즈~"),
    // おお (actual おお)
    ("おお", "오오→오~"),
    // いい (i-column)
    ("いい", "이이→이~"),
    // ああ (a-column)
    ("ああ", "아아→아~"),
    // ー (katakana long vowel mark)
    ("ー", "ー→~"),
];

/// Check if a hiragana reading contains a long vowel pattern
fn find_long_vowels(reading: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    let chars: Vec<char> = reading.chars().collect();

    for i in 0..chars.len() {
        let remaining: String = chars[i..].iter().collect();
        for &(pattern, _) in LONG_VOWEL_PATTERNS {
            if remaining.starts_with(pattern) {
                found.push((i, pattern));
                break; // only first match at this position
            }
        }
    }
    found
}

/// Generate hangul with long vowel markers.
/// Replaces the second vowel of a long vowel with '~'.
fn hangul_with_markers(reading: &str) -> String {
    let chars: Vec<char> = reading.chars().collect();
    let mut result = String::new();
    let mut i = 0;

    while i < chars.len() {
        let remaining: String = chars[i..].iter().collect();
        let mut matched = false;

        // Check for youon + long vowel first (e.g., きょう → 3 chars)
        if i + 3 <= chars.len() {
            let three: String = chars[i..i+3].iter().collect();
            // きょう, しょう, etc.
            let youon_long = [
                "きょう", "しょう", "ちょう", "にょう", "ひょう", "みょう", "りょう",
                "ぎょう", "じょう", "びょう", "ぴょう",
                "きゅう", "しゅう", "ちゅう", "にゅう", "ひゅう", "みゅう", "りゅう",
                "ぎゅう", "じゅう", "びゅう", "ぴゅう",
            ];
            if youon_long.contains(&three.as_str()) {
                // Convert youon part to hangul, then add ~
                let youon: String = chars[i..i+2].iter().collect();
                let h = hiragana_to_hangul(&youon);
                result.push_str(&h);
                result.push('~');
                i += 3;
                matched = true;
            }
        }

        if !matched && i + 2 <= chars.len() {
            let two: String = chars[i..i+2].iter().collect();

            // おう-type: second char is う after o-column
            let ou_patterns = [
                "おう", "こう", "そう", "とう", "のう", "ほう", "もう", "よう", "ろう",
                "ごう", "ぞう", "どう", "ぼう", "ぽう",
            ];
            // えい-type: second char is い after e-column
            let ei_patterns = [
                "えい", "けい", "せい", "てい", "ねい", "へい", "めい", "れい",
                "げい", "ぜい", "でい", "べい", "ぺい",
            ];
            // uu-type
            let uu_patterns = [
                "くう", "すう", "つう", "ふう", "ぐう", "ずう", "ゆう",
            ];
            // double vowels
            let double_patterns = [
                "おお", "いい", "ああ", "ええ", "うう",
            ];

            if ou_patterns.contains(&two.as_str())
                || ei_patterns.contains(&two.as_str())
                || uu_patterns.contains(&two.as_str())
                || double_patterns.contains(&two.as_str())
            {
                let first: String = chars[i..i+1].iter().collect();
                let h = hiragana_to_hangul(&first);
                result.push_str(&h);
                result.push('~');
                i += 2;
                matched = true;
            }
        }

        if !matched {
            // Single character
            let ch: String = chars[i..i+1].iter().collect();
            let h = hiragana_to_hangul(&ch);
            result.push_str(&h);
            i += 1;
        }
    }

    result
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
    let mut seen = HashSet::new();
    ranked.retain(|(s, _, _)| seen.insert(s.clone()));
    ranked
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let test_count: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(300);
    let train_vol: usize = 500_000;

    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║   장음 마커(~) 효과 분석 벤치마크                                            ║");
    println!("║   가설: 한글 입력에 '~'로 장음을 표시하면 히라가나 추정이 개선되는가?          ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════╝");
    println!();

    let global_start = Instant::now();
    let vocab = build_full_vocab();

    // ═══════════════════════════════════════════════════════
    //  Part 1: Vocabulary-level long vowel analysis
    // ═══════════════════════════════════════════════════════
    println!("═══ ① 어휘 장음 분포 분석 ═══════════════════════════════════════════════════");
    println!();

    let mut words_with_lv = 0;
    let mut total_lv_occurrences = 0;
    let mut pattern_counts: HashMap<&str, usize> = HashMap::new();
    let mut ambiguity_pairs: Vec<(String, String, String, String)> = Vec::new(); // (reading, hangul, hangul_marked, surface)

    for entry in &vocab {
        let lv = find_long_vowels(entry.reading);
        if !lv.is_empty() {
            words_with_lv += 1;
            total_lv_occurrences += lv.len();
            for (_, pat) in &lv {
                *pattern_counts.entry(pat).or_insert(0) += 1;
            }

            let h_plain = hiragana_to_hangul(entry.reading);
            let h_marked = hangul_with_markers(entry.reading);
            if h_plain != h_marked {
                ambiguity_pairs.push((
                    entry.reading.to_string(),
                    h_plain,
                    h_marked,
                    entry.surface.to_string(),
                ));
            }
        }
    }

    println!("  전체 어휘: {} | 장음 포함 단어: {} ({:.1}%)",
        vocab.len(), words_with_lv,
        words_with_lv as f64 / vocab.len() as f64 * 100.0);
    println!("  장음 출현 횟수: {} (단어당 평균 {:.1})",
        total_lv_occurrences,
        total_lv_occurrences as f64 / words_with_lv.max(1) as f64);
    println!();

    // Top patterns
    let mut sorted_patterns: Vec<_> = pattern_counts.iter().collect();
    sorted_patterns.sort_by(|a, b| b.1.cmp(a.1));
    println!("  장음 패턴 분포 (상위 15):");
    for (pat, count) in sorted_patterns.iter().take(15) {
        let bar = "█".repeat((*count / 2).min(30));
        println!("    {:>6} │ {:>4} │ {}", pat, count, bar);
    }
    println!();

    // Show example ambiguity pairs
    println!("  장음 마커 적용 예시 (상위 20):");
    println!("    {:>12}  {:>10} → {:>10}  ({})", "ひらがな", "한글(기존)", "한글(마커)", "표기");
    for (reading, plain, marked, surface) in ambiguity_pairs.iter().take(20) {
        println!("    {:>12}  {:>10} → {:>10}  ({})", reading, plain, marked, surface);
    }
    println!("  ... 총 {} 건에서 마커 적용 가능", ambiguity_pairs.len());
    println!();

    // ═══════════════════════════════════════════════════════
    //  Part 2: Build phonetic maps and compare
    // ═══════════════════════════════════════════════════════
    println!("═══ ② 히라가나 디코딩 정확도 비교 ═══════════════════════════════════════════");
    println!();

    let cfg = TrainerConfig { dim: 64, iterations: 50, ..Default::default() };

    eprint!("  인접문장 DB...");
    let t = Instant::now();
    let db = DictionaryDb::open_in_memory().expect("db");
    DbBuilder::new(db.conn()).with_config(cfg)
        .build_large_with_adjacent_sentences(&vocab, train_vol, 0.2).expect("build");
    eprintln!(" {:.1}s", t.elapsed().as_secs_f64());

    eprint!("  음소맵(기존)...");
    let t = Instant::now();
    let pmap_base = build_phonetic_map_chunked(&vocab, train_vol);
    eprintln!(" {:.1}s (mappings: {})", t.elapsed().as_secs_f64(), pmap_base.map.len());

    // Build marker-aware phonetic map
    // We need to add '~' token mappings to the phonetic map
    eprint!("  음소맵(장음마커)...");
    let t = Instant::now();
    let mut pmap_marked = pmap_base.clone();

    // Add '~' as a long vowel continuation marker
    // '~' after an o-column hangul → う, after e-column → い, after u-column → う, etc.
    // The key insight: '~' means "repeat the vowel of the previous kana"
    // But in the phonetic map, we map hangul tokens to hiragana.
    // So we need: hangul_char + '~' → hiragana with long vowel
    //
    // Strategy: add explicit mappings for common patterns with ~
    // e.g., "코~" → "こう", "세~" → "せい", "쿠~" → "くう"

    let marker_mappings: Vec<(&str, &str)> = vec![
        // o-column + ~ → おう
        ("오~", "おう"), ("코~", "こう"), ("소~", "そう"), ("토~", "とう"),
        ("노~", "のう"), ("호~", "ほう"), ("모~", "もう"), ("요~", "よう"),
        ("로~", "ろう"), ("고~", "ごう"), ("조~", "ぞう"), ("도~", "どう"),
        ("보~", "ぼう"), ("포~", "ぽう"),
        // e-column + ~ → えい
        ("에~", "えい"), ("케~", "けい"), ("세~", "せい"), ("테~", "てい"),
        ("네~", "ねい"), ("헤~", "へい"), ("메~", "めい"), ("레~", "れい"),
        ("게~", "げい"), ("제~", "ぜい"), ("데~", "でい"), ("베~", "べい"),
        ("페~", "ぺい"),
        // u-column + ~ → うう
        ("쿠~", "くう"), ("스~", "すう"), ("츠~", "つう"), ("후~", "ふう"),
        ("구~", "ぐう"), ("즈~", "ずう"), ("유~", "ゆう"),
        // youon + ~
        ("쿄~", "きょう"), ("쇼~", "しょう"), ("쵸~", "ちょう"),
        ("뇨~", "にょう"), ("효~", "ひょう"), ("묘~", "みょう"),
        ("료~", "りょう"), ("교~", "ぎょう"), ("죠~", "じょう"),
        ("뵤~", "びょう"),
        ("큐~", "きゅう"), ("슈~", "しゅう"), ("츄~", "ちゅう"),
        ("뉴~", "にゅう"), ("휴~", "ひゅう"), ("뮤~", "みゅう"),
        ("류~", "りゅう"), ("규~", "ぎゅう"), ("쥬~", "じゅう"),
        ("뷰~", "びゅう"),
        // a-column double
        ("아~", "ああ"),
        // i-column double
        ("이~", "いい"),
        // o-column double (おお)
        // Already handled by 오~ → おう, but おお case is rare
    ];

    let base_total = pmap_marked.total_pairs.max(1);
    for (hangul, hira) in &marker_mappings {
        let entry = pmap_marked.map.entry(hangul.to_string()).or_insert_with(Vec::new);
        // Give marker mappings high confidence
        let freq = base_total / 100; // ~1% of total = strong signal
        entry.push(PhoneticMapping {
            hiragana: hira.to_string(),
            frequency: freq,
            probability: 0.95, // High confidence for explicit markers
        });
        // Re-sort
        entry.sort_by(|a, b| b.probability.partial_cmp(&a.probability).unwrap_or(std::cmp::Ordering::Equal));
    }
    eprintln!(" {:.1}s (added {} marker mappings)", t.elapsed().as_secs_f64(), marker_mappings.len());

    eprint!("  N-gram...");
    let t = Instant::now();
    let ngram = build_ngram_model_chunked(&vocab, train_vol);
    eprintln!(" {:.1}s", t.elapsed().as_secs_f64());

    // Generate test data
    let test = generate_corpus_with_seed(&vocab, test_count, 99);
    println!();

    // ═══════════════════════════════════════════════════════
    //  Part 3: Compare decoding accuracy
    // ═══════════════════════════════════════════════════════
    println!("═══ ③ A/B 비교: 기존 한글 vs 장음 마커 한글 ═══════════════════════════════════");
    println!();

    let mut hira_hit_base = 0usize;
    let mut hira_top1_base = 0usize;
    let mut hira_hit_marked = 0usize;
    let mut hira_top1_marked = 0usize;
    let mut exact_base = 0usize;
    let mut exact_marked = 0usize;
    let mut top3_base = 0usize;
    let mut top3_marked = 0usize;
    let mut total_words = 0usize;
    let mut total_sentences = test.len();
    let mut doc_match_base = 0usize;
    let mut doc_match_marked = 0usize;

    let mut lv_words_total = 0usize;
    let mut lv_hira_base = 0usize;
    let mut lv_hira_marked = 0usize;
    let mut lv_exact_base = 0usize;
    let mut lv_exact_marked = 0usize;

    let mut non_lv_words_total = 0usize;
    let mut non_lv_exact_base = 0usize;
    let mut non_lv_exact_marked = 0usize;

    for sentence in &test {
        let mut ctx: Vec<String> = Vec::new();
        let (mut de_b, mut de_m, mut dw) = (0, 0, 0);

        for word in &sentence.words {
            total_words += 1;
            dw += 1;

            let has_lv = !find_long_vowels(&word.reading).is_empty();
            if has_lv { lv_words_total += 1; } else { non_lv_words_total += 1; }

            // A: Base (no marker)
            let dec_b = BeamDecoder::new(&pmap_base, 8, 20);
            let cands_b: Vec<(String, String, f64)> = dec_b.decode(&word.hangul)
                .into_iter().map(|(h, c)| (h, String::new(), c)).collect();

            if cands_b.iter().any(|(h, _, _)| *h == word.reading) {
                hira_hit_base += 1;
                if has_lv { lv_hira_base += 1; }
            }
            if cands_b.first().map(|(h, _, _)| h.as_str()) == Some(word.reading.as_str()) {
                hira_top1_base += 1;
            }

            let cv: Vec<&str> = ctx.iter().map(|s| s.as_str()).collect();
            let ranked_b = rank_4factor(&cands_b, &cv, &db, &ngram);
            if ranked_b.first().map(|(s, _, _)| s.as_str()) == Some(word.surface.as_str()) {
                exact_base += 1; de_b += 1;
                if has_lv { lv_exact_base += 1; } else { non_lv_exact_base += 1; }
            }
            if ranked_b.iter().take(3).any(|x| x.0 == word.surface) { top3_base += 1; }

            // B: With marker
            let hangul_marked = hangul_with_markers(&word.reading);
            let dec_m = BeamDecoder::new(&pmap_marked, 8, 20);
            let cands_m: Vec<(String, String, f64)> = dec_m.decode(&hangul_marked)
                .into_iter().map(|(h, c)| (h, String::new(), c)).collect();

            if cands_m.iter().any(|(h, _, _)| *h == word.reading) {
                hira_hit_marked += 1;
                if has_lv { lv_hira_marked += 1; }
            }
            if cands_m.first().map(|(h, _, _)| h.as_str()) == Some(word.reading.as_str()) {
                hira_top1_marked += 1;
            }

            let ranked_m = rank_4factor(&cands_m, &cv, &db, &ngram);
            if ranked_m.first().map(|(s, _, _)| s.as_str()) == Some(word.surface.as_str()) {
                exact_marked += 1; de_m += 1;
                if has_lv { lv_exact_marked += 1; } else { non_lv_exact_marked += 1; }
            }
            if ranked_m.iter().take(3).any(|x| x.0 == word.surface) { top3_marked += 1; }

            ctx.push(word.surface.clone());
        }

        if de_b == dw { doc_match_base += 1; }
        if de_m == dw { doc_match_marked += 1; }
    }

    let pct = |n: usize, d: usize| -> f64 {
        if d == 0 { 0.0 } else { n as f64 / d as f64 * 100.0 }
    };

    println!("  ┌───────────────────────┬──────────┬──────────┬──────────┐");
    println!("  │ 메트릭                 │ 기존 한글 │ 장음마커  │  변화    │");
    println!("  ├───────────────────────┼──────────┼──────────┼──────────┤");
    println!("  │ 히라가나 Hit (전체)    │ {:>6.1}%  │ {:>6.1}%  │ {:>+5.1}%p │",
        pct(hira_hit_base, total_words), pct(hira_hit_marked, total_words),
        pct(hira_hit_marked, total_words) - pct(hira_hit_base, total_words));
    println!("  │ 히라가나 Top-1         │ {:>6.1}%  │ {:>6.1}%  │ {:>+5.1}%p │",
        pct(hira_top1_base, total_words), pct(hira_top1_marked, total_words),
        pct(hira_top1_marked, total_words) - pct(hira_top1_base, total_words));
    println!("  │ Top-1 Surface (전체)   │ {:>6.1}%  │ {:>6.1}%  │ {:>+5.1}%p │",
        pct(exact_base, total_words), pct(exact_marked, total_words),
        pct(exact_marked, total_words) - pct(exact_base, total_words));
    println!("  │ Top-3 Surface          │ {:>6.1}%  │ {:>6.1}%  │ {:>+5.1}%p │",
        pct(top3_base, total_words), pct(top3_marked, total_words),
        pct(top3_marked, total_words) - pct(top3_base, total_words));
    println!("  │ 문장 전체 일치         │ {:>6.1}%  │ {:>6.1}%  │ {:>+5.1}%p │",
        pct(doc_match_base, total_sentences), pct(doc_match_marked, total_sentences),
        pct(doc_match_marked, total_sentences) - pct(doc_match_base, total_sentences));
    println!("  └───────────────────────┴──────────┴──────────┴──────────┘");

    println!();
    println!("═══ ④ 장음 단어 vs 비장음 단어 분리 분석 ═══════════════════════════════════");
    println!();
    println!("  ┌───────────────────────┬──────────┬──────────┬──────────┐");
    println!("  │ 분류                   │ 기존 한글 │ 장음마커  │  변화    │");
    println!("  ├───────────────────────┼──────────┼──────────┼──────────┤");
    println!("  │ 장음 단어 히라가나 Hit │ {:>6.1}%  │ {:>6.1}%  │ {:>+5.1}%p │ ({}/{})",
        pct(lv_hira_base, lv_words_total), pct(lv_hira_marked, lv_words_total),
        pct(lv_hira_marked, lv_words_total) - pct(lv_hira_base, lv_words_total),
        lv_words_total, total_words);
    println!("  │ 장음 단어 Top-1 Surf  │ {:>6.1}%  │ {:>6.1}%  │ {:>+5.1}%p │",
        pct(lv_exact_base, lv_words_total), pct(lv_exact_marked, lv_words_total),
        pct(lv_exact_marked, lv_words_total) - pct(lv_exact_base, lv_words_total));
    println!("  │ 비장음 단어 Top-1 Surf│ {:>6.1}%  │ {:>6.1}%  │ {:>+5.1}%p │",
        pct(non_lv_exact_base, non_lv_words_total), pct(non_lv_exact_marked, non_lv_words_total),
        pct(non_lv_exact_marked, non_lv_words_total) - pct(non_lv_exact_base, non_lv_words_total));
    println!("  └───────────────────────┴──────────┴──────────┴──────────┘");
    println!();
    println!("  장음 포함 단어: {}/{} ({:.1}%)",
        lv_words_total, total_words, pct(lv_words_total, total_words));

    println!();
    println!("  총 소요시간: {:.1}s", global_start.elapsed().as_secs_f64());
}
