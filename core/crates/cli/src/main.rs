//! CLI prototype for the Hangul→Japanese input method.
//!
//! Usage:
//!   hj-ime "사쿠라"        → さくら (sakura)
//!   hj-ime "도쿄"          → ときょ (tokyo)
//!   hj-ime "니혼"          → にほん (nihon)
//!   hj-ime --interactive   → REPL mode
//!   hj-ime --db path.db    → use custom dictionary DB

use ime_db::{DbBuilder, DictionaryDb, RankedCandidate, SentenceBuffer, TrainerConfig};
use ime_hangul::phoneme;
use ime_japanese::romaji;

/// Convert Hangul input to Japanese candidates (hiragana only, no context).
fn convert_hangul_to_japanese(input: &str) -> Vec<(String, String, f64)> {
    // Step 1: Hangul → romaji candidates
    let romaji_candidates = phoneme::hangul_string_to_romaji(input, 10);

    // Step 2: romaji → hiragana for each candidate
    let mut results: Vec<(String, String, f64)> = romaji_candidates
        .into_iter()
        .map(|(rom, conf)| {
            let hiragana = romaji::romaji_to_hiragana(&rom);
            (hiragana, rom, conf)
        })
        .collect();

    // Deduplicate by hiragana
    results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    results.dedup_by(|a, b| a.0 == b.0);

    results
}

/// Convert with context-aware ranking using embedding vectors.
fn convert_with_context(
    input: &str,
    context_words: &[&str],
    db: &DictionaryDb,
) -> Vec<RankedCandidate> {
    let hiragana_candidates = convert_hangul_to_japanese(input);

    if hiragana_candidates.is_empty() {
        return Vec::new();
    }

    let ranker = db.context_ranker();
    ranker.rank_candidates(&hiragana_candidates, context_words, 10000)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        println!("한글일본어입력기 CLI Prototype");
        println!();
        println!("Usage:");
        println!("  hj-ime <한글>                      한글을 일본어(히라가나)로 변환");
        println!("  hj-ime --interactive               대화형 모드");
        println!("  hj-ime --db <path> <한글>           사전 DB를 사용하여 변환");
        println!("  hj-ime --db <path> --interactive    사전 DB + 대화형 모드");
        println!("  hj-ime db-build <output.db>         학습 DB 생성 (소규모, 151문장)");
        println!("  hj-ime db-build <output.db> --dim N 임베딩 차원 지정 (기본 64)");
        println!("  hj-ime db-build-large <output.db>   대규모 학습 DB 생성 (10만건)");
        println!("  hj-ime db-build-large <out> --count N --dim D --iter I");
        println!();
        println!("Examples:");
        println!("  hj-ime 사쿠라    → さくら");
        println!("  hj-ime 도쿄      → ときょ");
        println!("  hj-ime 니혼      → にほん");
        println!("  hj-ime 아리가도  → ありがと");
        println!();
        println!("DB Build:");
        println!("  hj-ime db-build hj-ime.db");
        println!("  → 코퍼스 학습 → 임베딩 벡터 생성 → 사전 구축");
        println!();
        println!("Context-aware mode (with --db):");
        println!("  문장 맥락에서 토큰 임베딩 벡터를 사용하여");
        println!("  앞뒤 어휘 분포 거리로 후보를 재정렬합니다.");
        return;
    }

    // Handle db-build subcommand
    if args[1] == "db-build" {
        let output_path = if args.len() > 2 {
            &args[2]
        } else {
            "hj-ime.db"
        };

        // Parse optional --dim flag
        let mut dim = 64_usize;
        let mut iter_count = 50_usize;
        let mut i = 3;
        while i < args.len() {
            match args[i].as_str() {
                "--dim" if i + 1 < args.len() => {
                    dim = args[i + 1].parse().unwrap_or(64);
                    i += 2;
                }
                "--iter" if i + 1 < args.len() => {
                    iter_count = args[i + 1].parse().unwrap_or(50);
                    i += 2;
                }
                _ => { i += 1; }
            }
        }

        println!("학습 DB 생성 시작...");
        println!("  출력: {}", output_path);
        println!("  임베딩 차원: {}", dim);
        println!("  학습 반복: {}", iter_count);
        println!();

        match DictionaryDb::open(output_path) {
            Ok(db) => {
                let config = TrainerConfig {
                    dim,
                    iterations: iter_count,
                    ..Default::default()
                };
                let builder = DbBuilder::new(db.conn()).with_config(config);
                match builder.build() {
                    Ok(stats) => {
                        println!("{}", stats);
                        println!("DB 생성 완료: {}", output_path);
                    }
                    Err(e) => {
                        eprintln!("DB 생성 실패: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("DB 열기 실패: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // Handle db-build-large subcommand (100K generated sentences)
    if args[1] == "db-build-large" {
        let output_path = if args.len() > 2 {
            &args[2]
        } else {
            "hj-ime-large.db"
        };

        let mut dim = 64_usize;
        let mut iter_count = 50_usize;
        let mut sentence_count = 100_000_usize;
        let mut i = 3;
        while i < args.len() {
            match args[i].as_str() {
                "--dim" if i + 1 < args.len() => {
                    dim = args[i + 1].parse().unwrap_or(64);
                    i += 2;
                }
                "--iter" if i + 1 < args.len() => {
                    iter_count = args[i + 1].parse().unwrap_or(50);
                    i += 2;
                }
                "--count" if i + 1 < args.len() => {
                    sentence_count = args[i + 1].parse().unwrap_or(100_000);
                    i += 2;
                }
                _ => { i += 1; }
            }
        }

        println!("대규모 학습 DB 생성 시작...");
        println!("  출력: {}", output_path);
        println!("  문장 수: {}건", sentence_count);
        println!("  임베딩 차원: {}", dim);
        println!("  학습 반복: {}", iter_count);
        println!();

        match DictionaryDb::open(output_path) {
            Ok(db) => {
                let config = TrainerConfig {
                    dim,
                    iterations: iter_count,
                    ..Default::default()
                };
                let builder = DbBuilder::new(db.conn()).with_config(config);
                match builder.build_large(sentence_count) {
                    Ok(stats) => {
                        println!("{}", stats);
                        println!("대규모 DB 생성 완료: {}", output_path);
                    }
                    Err(e) => {
                        eprintln!("DB 생성 실패: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("DB 열기 실패: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // Parse --db flag
    let mut db_path: Option<String> = None;
    let mut remaining_args: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--db" && i + 1 < args.len() {
            db_path = Some(args[i + 1].clone());
            i += 2;
        } else {
            remaining_args.push(args[i].clone());
            i += 1;
        }
    }

    let is_interactive = remaining_args.first().map_or(false, |a| {
        a == "--interactive" || a == "-i"
    });

    if is_interactive {
        if let Some(path) = &db_path {
            match DictionaryDb::open(path) {
                Ok(db) => interactive_mode_with_context(&db),
                Err(e) => {
                    eprintln!("DB 열기 실패: {}", e);
                    eprintln!("기본 모드로 전환합니다.");
                    interactive_mode();
                }
            }
        } else {
            interactive_mode();
        }
        return;
    }

    // Single conversion mode
    let input = if remaining_args.is_empty() {
        eprintln!("입력이 필요합니다. --help를 참조하세요.");
        return;
    } else {
        &remaining_args[0]
    };

    println!("입력: {}", input);
    println!();

    if let Some(path) = &db_path {
        match DictionaryDb::open(path) {
            Ok(db) => {
                let results = convert_with_context(input, &[], &db);
                if results.is_empty() {
                    println!("변환 결과 없음");
                    return;
                }
                print_ranked_results(&results);
            }
            Err(e) => {
                eprintln!("DB 열기 실패: {}, 기본 모드 사용", e);
                print_basic_results(input);
            }
        }
    } else {
        print_basic_results(input);
    }
}

fn print_basic_results(input: &str) {
    let results = convert_hangul_to_japanese(input);

    if results.is_empty() {
        println!("변환 결과 없음");
        return;
    }

    println!("  순위 | 히라가나     | 로마지       | 신뢰도");
    println!("  ─────┼──────────────┼──────────────┼────────");
    for (i, (hiragana, rom, conf)) in results.iter().take(10).enumerate() {
        println!(
            "  {:>3}  | {:<12} | {:<12} | {:.1}%",
            i + 1,
            hiragana,
            rom,
            conf * 100.0
        );
    }

    println!();
    println!("  ▶ 최우선 후보: {}", results[0].0);
}

fn print_ranked_results(results: &[RankedCandidate]) {
    println!("  순위 | 표기         | 읽기         | 음소   | 문맥   | 최종");
    println!("  ─────┼──────────────┼──────────────┼────────┼────────┼────────");
    for (i, r) in results.iter().take(10).enumerate() {
        println!(
            "  {:>3}  | {:<12} | {:<12} | {:.1}%  | {:.1}%  | {:.1}%",
            i + 1,
            r.surface,
            r.reading,
            r.phoneme_score * 100.0,
            r.context_score * 100.0,
            r.final_score * 100.0,
        );
    }

    println!();
    println!("  ▶ 최우선 후보: {}", results[0].surface);
}

fn interactive_mode() {
    println!("한글일본어입력기 - 대화형 모드");
    println!("한글을 입력하면 일본어(히라가나)로 변환합니다.");
    println!("종료: Ctrl+C 또는 'quit' 입력");
    println!();

    let stdin = std::io::stdin();
    let mut input = String::new();

    loop {
        input.clear();
        print!("한글> ");
        use std::io::Write;
        std::io::stdout().flush().unwrap();

        if stdin.read_line(&mut input).is_err() {
            break;
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "quit" || trimmed == "exit" {
            break;
        }

        let results = convert_hangul_to_japanese(trimmed);

        if results.is_empty() {
            println!("  (변환 결과 없음)");
        } else {
            for (i, (hiragana, rom, conf)) in results.iter().take(5).enumerate() {
                let marker = if i == 0 { "▶" } else { " " };
                println!(
                    "  {} {} [{}] ({:.0}%)",
                    marker,
                    hiragana,
                    rom,
                    conf * 100.0
                );
            }
        }
        println!();
    }
}

/// Interactive mode with sentence buffer and bidirectional re-ranking.
///
/// Unlike the simple context mode, this maintains a sentence buffer where
/// ALL segments are re-ranked whenever a new word is added. This enables
/// retroactive correction: earlier words change based on later input.
fn interactive_mode_with_context(db: &DictionaryDb) {
    println!("한글일본어입력기 - 문장 버퍼 양방향 추론 모드");
    println!("한글을 입력하면 문장 전체의 문맥을 양방향으로 고려합니다.");
    println!("뒤에 입력한 단어에 의해 앞 단어가 자동으로 수정됩니다.");
    println!();
    println!("명령어:");
    println!("  <한글>       단어 추가 (문장 버퍼에 추가 + 전체 재추론)");
    println!("  .show        현재 문장 버퍼 상세 표시");
    println!("  .commit      현재 문장 확정 (Enter)");
    println!("  .undo        마지막 단어 삭제 + 재추론");
    println!("  .clear       버퍼 초기화");
    println!("  .select N M  N번째 세그먼트를 M번째 후보로 수동 변경");
    println!("  quit         종료");
    println!();

    let stdin = std::io::stdin();
    let mut input = String::new();
    let mut buf = SentenceBuffer::new(db.conn());

    loop {
        input.clear();

        // Show current composed sentence.
        if buf.is_empty() {
            print!("한글> ");
        } else {
            print!("  {} \n한글> ", buf.composed_debug());
        }
        use std::io::Write;
        std::io::stdout().flush().unwrap();

        if stdin.read_line(&mut input).is_err() {
            break;
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "quit" || trimmed == "exit" {
            break;
        }

        if trimmed == ".clear" {
            buf.clear();
            println!("  버퍼가 초기화되었습니다.");
            println!();
            continue;
        }

        if trimmed == ".commit" {
            if buf.is_empty() {
                println!("  (버퍼가 비어있습니다)");
            } else {
                let committed = buf.commit();
                println!("  ✓ 확정: {}", committed);
            }
            println!();
            continue;
        }

        if trimmed == ".undo" {
            if buf.is_empty() {
                println!("  (삭제할 세그먼트 없음)");
            } else {
                let popped = buf.pop_segment();
                if let Some(seg) = popped {
                    println!("  ✗ 삭제: {} ({})", seg.hangul, seg.selected_surface());
                    if !buf.is_empty() {
                        println!("  → 재추론 결과: {}", buf.composed());
                    }
                }
            }
            println!();
            continue;
        }

        if trimmed == ".show" {
            if buf.is_empty() {
                println!("  (버퍼 비어있음)");
            } else {
                println!("  === 문장 버퍼 상세 ===");
                for (i, seg) in buf.segments.iter().enumerate() {
                    println!(
                        "  [{:>2}] {} → {} (선택: {})",
                        i,
                        seg.hangul,
                        seg.selected_surface(),
                        seg.selected_idx
                    );
                    for (j, r) in seg.ranked.iter().take(5).enumerate() {
                        let marker = if j == seg.selected_idx {
                            "▶"
                        } else {
                            " "
                        };
                        println!(
                            "       {} {} (음소:{:.0}% 문맥:{:.0}% → {:.0}%)",
                            marker,
                            r.surface,
                            r.phoneme_score * 100.0,
                            r.context_score * 100.0,
                            r.final_score * 100.0,
                        );
                    }
                }
                println!("  현재 문장: {}", buf.composed());
            }
            println!();
            continue;
        }

        if trimmed.starts_with(".select ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() == 3 {
                if let (Ok(seg_idx), Ok(cand_idx)) =
                    (parts[1].parse::<usize>(), parts[2].parse::<usize>())
                {
                    if seg_idx < buf.len() {
                        let old = buf.segments[seg_idx].selected_surface().to_string();
                        buf.select_candidate(seg_idx, cand_idx);
                        let new = buf.segments[seg_idx].selected_surface().to_string();
                        println!("  세그먼트 {} 변경: {} → {}", seg_idx, old, new);
                        println!("  → 재추론 결과: {}", buf.composed());
                    } else {
                        println!("  세그먼트 인덱스 범위 초과");
                    }
                } else {
                    println!("  사용법: .select <세그먼트번호> <후보번호>");
                }
            } else {
                println!("  사용법: .select <세그먼트번호> <후보번호>");
            }
            println!();
            continue;
        }

        // Regular input: convert Hangul and add to sentence buffer.
        let candidates = convert_hangul_to_japanese(trimmed);

        if candidates.is_empty() {
            println!("  (변환 결과 없음)");
            println!();
            continue;
        }

        // Save what segment 0 was BEFORE adding new input.
        let prev_tops: Vec<String> = buf
            .segments
            .iter()
            .map(|s| s.selected_surface().to_string())
            .collect();

        buf.add_segment(trimmed.to_string(), candidates);

        // Check if any previous segment changed (retroactive correction).
        let mut changes = Vec::new();
        for (i, prev) in prev_tops.iter().enumerate() {
            if i < buf.segments.len() - 1 {
                let current = buf.segments[i].selected_surface();
                if current != prev {
                    changes.push((i, prev.clone(), current.to_string()));
                }
            }
        }

        // Show the newly added segment's top candidates.
        let last_seg = buf.segments.last().unwrap();
        for (j, r) in last_seg.ranked.iter().take(3).enumerate() {
            let marker = if j == 0 { "▶" } else { " " };
            println!(
                "  {} {} [{}] (음소:{:.0}% 문맥:{:.0}% → {:.0}%)",
                marker,
                r.surface,
                r.reading,
                r.phoneme_score * 100.0,
                r.context_score * 100.0,
                r.final_score * 100.0,
            );
        }

        // Report retroactive changes.
        if !changes.is_empty() {
            println!();
            println!("  ⟲ 역방향 수정 발생:");
            for (i, old, new) in &changes {
                println!(
                    "    세그먼트 [{}] \"{}\" → \"{}\" → \"{}\"",
                    i, buf.segments[*i].hangul, old, new
                );
            }
        }

        println!("  문장: {}", buf.composed());
        println!();
    }
}
