//! Diagnostic tool: analyzes WHY phonetic embedding fails on the remaining ~9.5%.
//!
//! For each test word where BeamDecoder fails to produce the correct hiragana,
//! prints the failure mode: alignment issue, unknown token, ambiguity, etc.

use ime_db::generator::generate_corpus_with_seed;
use ime_db::phonetic_decoder::{BeamDecoder, PhoneticMap, build_phonetic_map_chunked};
use ime_db::kana_hangul::hiragana_to_hangul;
use ime_db::vocab::build_full_vocab;
use std::collections::HashMap;

fn main() {
    let vocab = build_full_vocab();
    println!("어휘: {} 항목", vocab.len());

    // Build phonetic map from 500K (same result as 10M anyway)
    println!("음소 맵 구축 중 (500K)...");
    let phonetic_map = build_phonetic_map_chunked(&vocab, 500_000);
    println!("{}", phonetic_map.stats());
    println!();

    // Generate test data
    let test_sentences = generate_corpus_with_seed(&vocab, 2000, 99);

    let decoder = BeamDecoder::new(&phonetic_map, 8, 20);

    let mut total_words = 0usize;
    let mut hira_top1_ok = 0usize;
    let mut hira_any_ok = 0usize;
    let mut fail_no_candidate = 0usize;
    let mut fail_wrong_top1 = 0usize;
    let mut fail_not_in_any = 0usize;

    // Failure pattern tracking
    let mut failure_patterns: HashMap<String, usize> = HashMap::new();
    let mut failure_examples: Vec<(String, String, String, String)> = Vec::new(); // (surface, reading, hangul, decoded_top1)

    for sentence in &test_sentences {
        for word in &sentence.words {
            total_words += 1;

            let candidates = decoder.decode(&word.hangul);

            if candidates.is_empty() {
                fail_no_candidate += 1;
                let pattern = format!("NO_CANDIDATES: hangul={}", word.hangul);
                *failure_patterns.entry(pattern).or_insert(0) += 1;
                continue;
            }

            let top1_hira = &candidates[0].0;
            let any_hit = candidates.iter().any(|(h, _)| *h == word.reading);

            if *top1_hira == word.reading {
                hira_top1_ok += 1;
                hira_any_ok += 1;
            } else if any_hit {
                hira_any_ok += 1;
                fail_wrong_top1 += 1;
                // Top-1 is wrong but correct is in the list
                let pattern = format!("WRONG_TOP1: expected={} got={}", word.reading, top1_hira);
                *failure_patterns.entry(pattern.clone()).or_insert(0) += 1;
                if failure_examples.len() < 100 {
                    failure_examples.push((word.surface.clone(), word.reading.clone(), word.hangul.clone(), top1_hira.clone()));
                }
            } else {
                fail_not_in_any += 1;
                // Correct hiragana not in ANY candidate

                // Diagnose why: check each hangul character
                let hangul_chars: Vec<char> = word.hangul.chars().collect();
                let reading_chars: Vec<char> = word.reading.chars().collect();

                // Check for ん or っ in reading (alignment issues)
                let has_n = word.reading.contains('ん');
                let has_sokuon = word.reading.contains('っ');
                let has_youon = word.reading.contains('ゃ') || word.reading.contains('ゅ') || word.reading.contains('ょ');
                let has_long_vowel = word.reading.contains('ー');

                let mut reason = String::new();
                if has_n { reason.push_str("+ん"); }
                if has_sokuon { reason.push_str("+っ"); }
                if has_youon { reason.push_str("+拗音"); }
                if has_long_vowel { reason.push_str("+長音"); }

                // Check if hangul chars are all in the map
                let mut unknown_tokens = Vec::new();
                for ch in &hangul_chars {
                    let token = ch.to_string();
                    if phonetic_map.get_candidates(&token).is_none() {
                        unknown_tokens.push(token);
                    }
                }
                if !unknown_tokens.is_empty() {
                    reason.push_str(&format!("+UNKNOWN:{}", unknown_tokens.join(",")));
                }

                // Length mismatch
                if hangul_chars.len() != reading_chars.len() {
                    let has_special = reading_chars.iter().any(|c| *c == 'ん' || *c == 'っ' || *c == 'ー');
                    if has_special {
                        reason.push_str("+LENGTH_MISMATCH_SPECIAL");
                    } else {
                        reason.push_str(&format!("+LENGTH_MISMATCH(h{}:r{})", hangul_chars.len(), reading_chars.len()));
                    }
                }

                if reason.is_empty() {
                    reason = "UNKNOWN_REASON".to_string();
                }

                let pattern = format!("NOT_IN_ANY: {}", reason);
                *failure_patterns.entry(pattern).or_insert(0) += 1;

                if failure_examples.len() < 100 {
                    failure_examples.push((word.surface.clone(), word.reading.clone(), word.hangul.clone(), top1_hira.clone()));
                }
            }
        }
    }

    // ═══ Report ═══
    println!("╔═══════════════════════════════════════════════════════════════════════╗");
    println!("║             음소 임베딩 실패 원인 진단 보고서                          ║");
    println!("╚═══════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  총 단어: {}", total_words);
    println!("  Hira Top-1 정답: {} ({:.1}%)", hira_top1_ok, hira_top1_ok as f64 / total_words as f64 * 100.0);
    println!("  Hira Any-Hit:    {} ({:.1}%)", hira_any_ok, hira_any_ok as f64 / total_words as f64 * 100.0);
    println!();
    println!("  실패 분류:");
    println!("    후보 없음:       {} ({:.1}%)", fail_no_candidate, fail_no_candidate as f64 / total_words as f64 * 100.0);
    println!("    Top-1 오류:      {} ({:.1}%)", fail_wrong_top1, fail_wrong_top1 as f64 / total_words as f64 * 100.0);
    println!("    후보에 없음:     {} ({:.1}%)", fail_not_in_any, fail_not_in_any as f64 / total_words as f64 * 100.0);
    println!();

    // Sort failure patterns by count
    let mut patterns: Vec<_> = failure_patterns.into_iter().collect();
    patterns.sort_by(|a, b| b.1.cmp(&a.1));

    println!("  실패 패턴 Top-30:");
    println!("  ┌────┬──────────────────────────────────────────────────────────────────┐");
    for (i, (pattern, count)) in patterns.iter().take(30).enumerate() {
        println!("  │{:>3} │ {:>4} ({:>4.1}%) │ {}", i + 1, count, *count as f64 / total_words as f64 * 100.0, pattern);
    }
    println!("  └────┴──────────────────────────────────────────────────────────────────┘");

    // Show failure examples
    println!();
    println!("  실패 예시 (surface / reading / hangul / decoded_top1):");
    println!("  ┌──────────────┬──────────────┬──────────────┬──────────────┐");
    println!("  │ surface      │ reading      │ hangul       │ decoded      │");
    println!("  ├──────────────┼──────────────┼──────────────┼──────────────┤");
    for (surface, reading, hangul, decoded) in failure_examples.iter().take(40) {
        let mark = if *decoded == *reading { "✓" } else { "✗" };
        println!("  │ {:>12} │ {:>12} │ {:>12} │ {:>12} │ {}", surface, reading, hangul, decoded, mark);
    }
    println!("  └──────────────┴──────────────┴──────────────┴──────────────┘");

    // Analysis of phonetic map coverage
    println!();
    println!("  음소 맵 커버리지 분석:");

    // Check: how many unique hangul tokens appear in test data?
    let mut test_hangul_tokens: HashMap<String, usize> = HashMap::new();
    let mut test_hangul_unknown: HashMap<String, usize> = HashMap::new();
    for sentence in &test_sentences {
        for word in &sentence.words {
            for ch in word.hangul.chars() {
                let token = ch.to_string();
                *test_hangul_tokens.entry(token.clone()).or_insert(0) += 1;
                if phonetic_map.get_candidates(&token).is_none() {
                    *test_hangul_unknown.entry(token).or_insert(0) += 1;
                }
            }
        }
    }
    println!("    테스트 고유 한글 토큰: {}", test_hangul_tokens.len());
    println!("    맵에 없는 토큰:       {}", test_hangul_unknown.len());
    if !test_hangul_unknown.is_empty() {
        let mut unknown_sorted: Vec<_> = test_hangul_unknown.into_iter().collect();
        unknown_sorted.sort_by(|a, b| b.1.cmp(&a.1));
        println!("    미등록 토큰:");
        for (token, count) in unknown_sorted.iter().take(20) {
            // Show what hiragana_to_hangul would produce for reverse lookup
            println!("      '{}' (출현 {}회)", token, count);
        }
    }

    // ん/っ analysis
    println!();
    println!("  특수 문자 분석:");
    let mut n_count = 0;
    let mut sokuon_count = 0;
    let mut youon_count = 0;
    for sentence in &test_sentences {
        for word in &sentence.words {
            if word.reading.contains('ん') { n_count += 1; }
            if word.reading.contains('っ') { sokuon_count += 1; }
            if word.reading.contains('ゃ') || word.reading.contains('ゅ') || word.reading.contains('ょ') { youon_count += 1; }
        }
    }
    println!("    ん 포함 단어: {} ({:.1}%)", n_count, n_count as f64 / total_words as f64 * 100.0);
    println!("    っ 포함 단어: {} ({:.1}%)", sokuon_count, sokuon_count as f64 / total_words as f64 * 100.0);
    println!("    拗音 포함 단어: {} ({:.1}%)", youon_count, youon_count as f64 / total_words as f64 * 100.0);
}
