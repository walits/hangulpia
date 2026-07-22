/// Hiragana to Hangul automatic converter for Japanese IME
/// Converts Japanese hiragana readings into natural Korean phonetic transcriptions

/// Main conversion function: hiragana → hangul
pub fn hiragana_to_hangul(input: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        // Handle sokuon (っ): double the next consonant
        if ch == 'っ' {
            // Add tensed consonant as batchim on previous character
            if i + 1 < chars.len() {
                let next_ch = chars[i + 1];
                let tensed_batchim = get_tensed_consonant(next_ch);
                if !tensed_batchim.is_empty() {
                    // Add batchim to the last hangul character in result
                    add_batchim(&mut result, tensed_batchim);
                    i += 1;
                    continue;
                }
            }
            i += 1;
            continue;
        }

        // Try youon (2-char sequences) first (greedy longest-match)
        if i + 1 < chars.len() {
            let two_char = format!("{}{}", ch, chars[i + 1]);
            if let Some(hangul) = get_youon(&two_char) {
                result.push_str(hangul);
                i += 2;
                continue;
            }
        }

        // Handle single hiragana
        if let Some(hangul) = get_hiragana(ch) {
            result.push_str(hangul);
            i += 1;
            continue;
        }

        // Handle ん with context-aware batchim selection
        if ch == 'ん' {
            let batchim = get_n_batchim(&chars, i);
            // Add batchim to the last hangul character in result
            add_batchim(&mut result, batchim);
            i += 1;
            continue;
        }

        // Handle long vowel mark ー: extend the previous vowel
        if ch == 'ー' {
            if !result.is_empty() {
                // Get last vowel and extend it
                let last_vowel = get_last_vowel(&result);
                if !last_vowel.is_empty() {
                    result.push_str(last_vowel);
                }
            }
            i += 1;
            continue;
        }

        // Unknown character: keep as-is
        result.push(ch);
        i += 1;
    }

    result
}

/// Get single hiragana → hangul mapping
fn get_hiragana(ch: char) -> Option<&'static str> {
    match ch {
        // Vowels (あ行)
        'あ' => Some("아"),
        'い' => Some("이"),
        'う' => Some("우"),
        'え' => Some("에"),
        'お' => Some("오"),

        // か行
        'か' => Some("카"),
        'き' => Some("키"),
        'く' => Some("쿠"),
        'け' => Some("케"),
        'こ' => Some("코"),

        // さ行
        'さ' => Some("사"),
        'し' => Some("시"),
        'す' => Some("스"),
        'せ' => Some("세"),
        'そ' => Some("소"),

        // た行
        'た' => Some("타"),
        'ち' => Some("치"),
        'つ' => Some("츠"),
        'て' => Some("테"),
        'と' => Some("토"),

        // な行
        'な' => Some("나"),
        'に' => Some("니"),
        'ぬ' => Some("누"),
        'ね' => Some("네"),
        'の' => Some("노"),

        // は行
        'は' => Some("하"),
        'ひ' => Some("히"),
        'ふ' => Some("후"),
        'へ' => Some("헤"),
        'ほ' => Some("호"),

        // ま行
        'ま' => Some("마"),
        'み' => Some("미"),
        'む' => Some("무"),
        'め' => Some("메"),
        'も' => Some("모"),

        // や行
        'や' => Some("야"),
        'ゆ' => Some("유"),
        'よ' => Some("요"),

        // ら行
        'ら' => Some("라"),
        'り' => Some("리"),
        'る' => Some("루"),
        'れ' => Some("레"),
        'ろ' => Some("로"),

        // わ行
        'わ' => Some("와"),
        'を' => Some("오"),
        // ん is handled separately for context-aware batchim selection

        // が行 (dakuten)
        'が' => Some("가"),
        'ぎ' => Some("기"),
        'ぐ' => Some("구"),
        'げ' => Some("게"),
        'ご' => Some("고"),

        // ざ行 (dakuten)
        'ざ' => Some("자"),
        'じ' => Some("지"),
        'ず' => Some("즈"),
        'ぜ' => Some("제"),
        'ぞ' => Some("조"),

        // だ行 (dakuten)
        'だ' => Some("다"),
        'ぢ' => Some("지"),
        'づ' => Some("즈"),
        'で' => Some("데"),
        'ど' => Some("도"),

        // ば行 (dakuten)
        'ば' => Some("바"),
        'び' => Some("비"),
        'ぶ' => Some("부"),
        'べ' => Some("베"),
        'ぼ' => Some("보"),

        // ぱ行 (handakuten)
        'ぱ' => Some("파"),
        'ぴ' => Some("피"),
        'ぷ' => Some("푸"),
        'ぺ' => Some("페"),
        'ぽ' => Some("포"),

        _ => None,
    }
}

/// Get youon (拗音) 2-character sequences → hangul
fn get_youon(seq: &str) -> Option<&'static str> {
    match seq {
        // きゃ/きゅ/きょ (ki-row)
        "きゃ" => Some("캬"),
        "きゅ" => Some("큐"),
        "きょ" => Some("쿄"),

        // しゃ/しゅ/しょ (si-row)
        "しゃ" => Some("샤"),
        "しゅ" => Some("슈"),
        "しょ" => Some("쇼"),

        // ちゃ/ちゅ/ちょ (ti-row)
        "ちゃ" => Some("차"),
        "ちゅ" => Some("추"),
        "ちょ" => Some("초"),

        // にゃ/にゅ/にょ (ni-row)
        "にゃ" => Some("냐"),
        "にゅ" => Some("뉴"),
        "にょ" => Some("뇨"),

        // ひゃ/ひゅ/ひょ (hi-row)
        "ひゃ" => Some("햐"),
        "ひゅ" => Some("휴"),
        "ひょ" => Some("효"),

        // みゃ/みゅ/みょ (mi-row)
        "みゃ" => Some("먀"),
        "みゅ" => Some("뮤"),
        "みょ" => Some("묘"),

        // りゃ/りゅ/りょ (ri-row)
        "りゃ" => Some("랴"),
        "りゅ" => Some("류"),
        "りょ" => Some("료"),

        // ぎゃ/ぎゅ/ぎょ (gi-row dakuten)
        "ぎゃ" => Some("갸"),
        "ぎゅ" => Some("규"),
        "ぎょ" => Some("교"),

        // じゃ/じゅ/じょ (zi-row dakuten)
        "じゃ" => Some("자"),
        "じゅ" => Some("주"),
        "じょ" => Some("조"),

        // びゃ/びゅ/びょ (bi-row dakuten)
        "びゃ" => Some("뱌"),
        "びゅ" => Some("뷰"),
        "びょ" => Some("뵤"),

        // ぴゃ/ぴゅ/ぴょ (pi-row handakuten)
        "ぴゃ" => Some("퍄"),
        "ぴゅ" => Some("퓨"),
        "ぴょ" => Some("표"),

        _ => None,
    }
}

/// Get tensed consonant batchim for sokuon (っ)
/// Sokuon always produces ㅅ batchim in standard Korean transcription of Japanese
fn get_tensed_consonant(_next_ch: char) -> &'static str {
    // Sokuon (small っ) always becomes ㅅ batchim in Korean
    "ㅅ"
}

/// Determine appropriate ㄴ/ㅁ batchim for ん based on next character
fn get_n_batchim(chars: &[char], pos: usize) -> &'static str {
    if pos + 1 >= chars.len() {
        // End of string: use ㄴ
        return "ㄴ";
    }

    match chars[pos + 1] {
        // Before ば/ぱ/ま行 → ㅁ
        'ば' | 'び' | 'ぶ' | 'べ' | 'ぼ' | 'ぱ' | 'ぴ' | 'ぷ' | 'ぺ' | 'ぽ'
        | 'ま' | 'み' | 'む' | 'め' | 'も' => "ㅁ",

        // Before all others → ㄴ (including か/が行)
        _ => "ㄴ",
    }
}

/// Extract the last vowel from the result string for ー handling
fn get_last_vowel(result: &str) -> &'static str {
    if result.is_empty() {
        return "";
    }

    // Check last hangul character(s) to determine vowel
    if result.ends_with("아") {
        "아"
    } else if result.ends_with("이") {
        "이"
    } else if result.ends_with("우") {
        "우"
    } else if result.ends_with("에") {
        "에"
    } else if result.ends_with("오") {
        "오"
    } else if result.ends_with("유") {
        "유"
    } else {
        // Default: extend with 우 (ー often represents long u-sound)
        "우"
    }
}

/// Add batchim (final consonant) to the last hangul character in the result string
/// Combines the batchim with the last complete hangul syllable
fn add_batchim(result: &mut String, batchim: &str) {
    if result.is_empty() {
        // No previous character to attach batchim to
        // In rare cases, just append the batchim (like standalone ン at start)
        result.push_str(batchim);
        return;
    }

    // Get the last character
    let last_char = result.chars().last().unwrap();

    // Try to compose with batchim
    if let Some(composed) = compose_with_batchim(last_char, batchim) {
        // Remove the last character and add the composed version
        result.pop();
        result.push_str(composed);
    } else {
        // If composition fails, just append (shouldn't happen with valid hangul)
        result.push_str(batchim);
    }
}

/// Compose a hangul base syllable with a batchim (final consonant)
/// Handles the standard Hangul composition for adding batchim to a syllable
fn compose_with_batchim(base_char: char, batchim: &str) -> Option<&'static str> {
    // Map of (base hangul, batchim) → composed hangul
    // This is a lookup table for common combinations
    match (base_char, batchim) {
        // ㄴ batchim
        ('가', "ㄴ") => Some("간"),
        ('게', "ㄴ") => Some("겐"),
        ('고', "ㄴ") => Some("곤"),
        ('구', "ㄴ") => Some("군"),
        ('기', "ㄴ") => Some("긴"),
        ('나', "ㄴ") => Some("난"),
        ('네', "ㄴ") => Some("넨"),
        ('니', "ㄴ") => Some("닌"),
        ('노', "ㄴ") => Some("논"),
        ('누', "ㄴ") => Some("눈"),
        ('다', "ㄴ") => Some("단"),
        ('데', "ㄴ") => Some("덴"),
        ('더', "ㄴ") => Some("던"),
        ('도', "ㄴ") => Some("돈"),
        ('두', "ㄴ") => Some("둔"),
        ('마', "ㄴ") => Some("만"),
        ('메', "ㄴ") => Some("멘"),
        ('머', "ㄴ") => Some("먼"),
        ('모', "ㄴ") => Some("몬"),
        ('무', "ㄴ") => Some("문"),
        ('사', "ㄴ") => Some("산"),
        ('세', "ㄴ") => Some("센"),
        ('서', "ㄴ") => Some("선"),
        ('소', "ㄴ") => Some("손"),
        ('수', "ㄴ") => Some("순"),
        ('시', "ㄴ") => Some("신"),
        ('아', "ㄴ") => Some("안"),
        ('에', "ㄴ") => Some("엔"),
        ('어', "ㄴ") => Some("언"),
        ('오', "ㄴ") => Some("온"),
        ('우', "ㄴ") => Some("운"),
        ('이', "ㄴ") => Some("인"),
        ('자', "ㄴ") => Some("잔"),
        ('제', "ㄴ") => Some("젠"),
        ('저', "ㄴ") => Some("전"),
        ('조', "ㄴ") => Some("존"),
        ('주', "ㄴ") => Some("준"),
        ('타', "ㄴ") => Some("탄"),
        ('테', "ㄴ") => Some("텐"),
        ('터', "ㄴ") => Some("턴"),
        ('토', "ㄴ") => Some("톤"),
        ('투', "ㄴ") => Some("툰"),
        ('하', "ㄴ") => Some("한"),
        ('헤', "ㄴ") => Some("헨"),
        ('허', "ㄴ") => Some("헌"),
        ('호', "ㄴ") => Some("혼"),
        ('후', "ㄴ") => Some("훈"),
        ('히', "ㄴ") => Some("힌"),
        ('카', "ㄴ") => Some("칸"),
        ('케', "ㄴ") => Some("켠"),
        ('커', "ㄴ") => Some("컨"),
        ('코', "ㄴ") => Some("콘"),
        ('쿠', "ㄴ") => Some("쿤"),
        ('키', "ㄴ") => Some("킨"),
        ('파', "ㄴ") => Some("판"),
        ('페', "ㄴ") => Some("펜"),
        ('퍼', "ㄴ") => Some("펀"),
        ('포', "ㄴ") => Some("폰"),
        ('푸', "ㄴ") => Some("푼"),
        ('피', "ㄴ") => Some("핀"),
        ('바', "ㄴ") => Some("반"),
        ('베', "ㄴ") => Some("벤"),
        ('버', "ㄴ") => Some("번"),
        ('보', "ㄴ") => Some("본"),
        ('부', "ㄴ") => Some("분"),
        ('비', "ㄴ") => Some("빈"),
        // Youon with ㄴ
        ('냐', "ㄴ") => Some("냔"),
        ('냥', "ㄴ") => Some("냥"),
        ('냉', "ㄴ") => Some("냉"),
        ('냬', "ㄴ") => Some("냬"),
        ('뉴', "ㄴ") => Some("눈"),
        ('뉭', "ㄴ") => Some("뉭"),
        ('뇨', "ㄴ") => Some("뇨"),
        ('뇽', "ㄴ") => Some("뇽"),
        ('샤', "ㄴ") => Some("샨"),
        ('샤', "ㄴ") => Some("샨"),
        ('샬', "ㄴ") => Some("샬"),
        ('슈', "ㄴ") => Some("슌"),
        ('슬', "ㄴ") => Some("슬"),
        ('쇄', "ㄴ") => Some("쇄"),
        ('쇠', "ㄴ") => Some("쇠"),
        ('쇼', "ㄴ") => Some("숀"),
        ('캬', "ㄴ") => Some("캔"),
        ('큐', "ㄴ") => Some("큔"),
        ('쿄', "ㄴ") => Some("쿤"),

        // ㅁ batchim
        ('가', "ㅁ") => Some("감"),
        ('게', "ㅁ") => Some("겜"),
        ('고', "ㅁ") => Some("곰"),
        ('구', "ㅁ") => Some("굼"),
        ('기', "ㅁ") => Some("김"),
        ('나', "ㅁ") => Some("남"),
        ('네', "ㅁ") => Some("넴"),
        ('니', "ㅁ") => Some("님"),
        ('노', "ㅁ") => Some("놈"),
        ('누', "ㅁ") => Some("눔"),
        ('다', "ㅁ") => Some("담"),
        ('데', "ㅁ") => Some("뎀"),
        ('더', "ㅁ") => Some("덤"),
        ('도', "ㅁ") => Some("돔"),
        ('두', "ㅁ") => Some("둠"),
        ('마', "ㅁ") => Some("맘"),
        ('메', "ㅁ") => Some("멤"),
        ('머', "ㅁ") => Some("멈"),
        ('모', "ㅁ") => Some("몸"),
        ('무', "ㅁ") => Some("뭄"),
        ('사', "ㅁ") => Some("삼"),
        ('세', "ㅁ") => Some("섬"),
        ('서', "ㅁ") => Some("섬"),
        ('소', "ㅁ") => Some("솜"),
        ('수', "ㅁ") => Some("숨"),
        ('시', "ㅁ") => Some("심"),
        ('아', "ㅁ") => Some("암"),
        ('에', "ㅁ") => Some("엠"),
        ('어', "ㅁ") => Some("엄"),
        ('오', "ㅁ") => Some("옴"),
        ('우', "ㅁ") => Some("움"),
        ('이', "ㅁ") => Some("임"),
        ('자', "ㅁ") => Some("잠"),
        ('제', "ㅁ") => Some("젬"),
        ('저', "ㅁ") => Some("점"),
        ('조', "ㅁ") => Some("좀"),
        ('주', "ㅁ") => Some("줌"),
        ('타', "ㅁ") => Some("탐"),
        ('테', "ㅁ") => Some("템"),
        ('터', "ㅁ") => Some("텀"),
        ('토', "ㅁ") => Some("톰"),
        ('투', "ㅁ") => Some("툼"),
        ('하', "ㅁ") => Some("함"),
        ('헤', "ㅁ") => Some("헴"),
        ('허', "ㅁ") => Some("험"),
        ('호', "ㅁ") => Some("홈"),
        ('후', "ㅁ") => Some("훔"),
        ('히', "ㅁ") => Some("힘"),
        ('카', "ㅁ") => Some("캄"),
        ('케', "ㅁ") => Some("켐"),
        ('커', "ㅁ") => Some("컴"),
        ('코', "ㅁ") => Some("콤"),
        ('쿠', "ㅁ") => Some("쿰"),
        ('키', "ㅁ") => Some("킴"),
        ('파', "ㅁ") => Some("팜"),
        ('페', "ㅁ") => Some("펨"),
        ('퍼', "ㅁ") => Some("펌"),
        ('포', "ㅁ") => Some("폼"),
        ('푸', "ㅁ") => Some("품"),
        ('피', "ㅁ") => Some("핌"),
        ('바', "ㅁ") => Some("밤"),
        ('베', "ㅁ") => Some("뱀"),
        ('버', "ㅁ") => Some("범"),
        ('보', "ㅁ") => Some("봄"),
        ('부', "ㅁ") => Some("붐"),
        ('비', "ㅁ") => Some("빔"),

        // ㅅ batchim
        ('가', "ㅅ") => Some("갓"),
        ('게', "ㅅ") => Some("겟"),
        ('고', "ㅅ") => Some("곳"),
        ('구', "ㅅ") => Some("굿"),
        ('기', "ㅅ") => Some("깃"),
        ('나', "ㅅ") => Some("낫"),
        ('네', "ㅅ") => Some("넷"),
        ('니', "ㅅ") => Some("닏"),
        ('노', "ㅅ") => Some("놋"),
        ('누', "ㅅ") => Some("뉻"),
        ('다', "ㅅ") => Some("닷"),
        ('데', "ㅅ") => Some("댓"),
        ('더', "ㅅ") => Some("덛"),
        ('도', "ㅅ") => Some("닷"),
        ('두', "ㅅ") => Some("뒷"),
        ('마', "ㅅ") => Some("맛"),
        ('메', "ㅅ") => Some("멧"),
        ('머', "ㅅ") => Some("멋"),
        ('모', "ㅅ") => Some("못"),
        ('무', "ㅅ") => Some("뭇"),
        ('사', "ㅅ") => Some("삿"),
        ('세', "ㅅ") => Some("섯"),
        ('서', "ㅅ") => Some("섯"),
        ('소', "ㅅ") => Some("솟"),
        ('수', "ㅅ") => Some("숫"),
        ('시', "ㅅ") => Some("싯"),
        ('아', "ㅅ") => Some("앗"),
        ('에', "ㅅ") => Some("엣"),
        ('어', "ㅅ") => Some("엇"),
        ('오', "ㅅ") => Some("옷"),
        ('우', "ㅅ") => Some("웃"),
        ('이', "ㅅ") => Some("잇"),
        ('자', "ㅅ") => Some("잣"),
        ('제', "ㅅ") => Some("젯"),
        ('저', "ㅅ") => Some("젯"),
        ('조', "ㅅ") => Some("좋"),
        ('주', "ㅅ") => Some("줏"),
        ('타', "ㅅ") => Some("탓"),
        ('테', "ㅅ") => Some("텟"),
        ('터', "ㅅ") => Some("텃"),
        ('토', "ㅅ") => Some("톳"),
        ('투', "ㅅ") => Some("툿"),
        ('하', "ㅅ") => Some("핳"),
        ('헤', "ㅅ") => Some("헷"),
        ('허', "ㅅ") => Some("헛"),
        ('호', "ㅅ") => Some("홓"),
        ('후', "ㅅ") => Some("훋"),
        ('히', "ㅅ") => Some("힛"),
        ('카', "ㅅ") => Some("캇"),
        ('케', "ㅅ") => Some("켓"),
        ('커', "ㅅ") => Some("컷"),
        ('코', "ㅅ") => Some("콧"),
        ('쿠', "ㅅ") => Some("쿳"),
        ('키', "ㅅ") => Some("킷"),
        ('파', "ㅅ") => Some("팓"),
        ('페', "ㅅ") => Some("펫"),
        ('퍼', "ㅅ") => Some("펫"),
        ('포', "ㅅ") => Some("팟"),
        ('푸', "ㅅ") => Some("풋"),
        ('피', "ㅅ") => Some("핏"),
        ('바', "ㅅ") => Some("밧"),
        ('베', "ㅅ") => Some("벧"),
        ('버', "ㅅ") => Some("벗"),
        ('보', "ㅅ") => Some("봇"),
        ('부', "ㅅ") => Some("붓"),
        ('비', "ㅅ") => Some("빚"),

        // ㅂ batchim
        ('가', "ㅂ") => Some("갑"),
        ('기', "ㅂ") => Some("깁"),
        ('고', "ㅂ") => Some("곱"),
        ('구', "ㅂ") => Some("굽"),
        ('나', "ㅂ") => Some("납"),
        ('니', "ㅂ") => Some("닙"),
        ('노', "ㅂ") => Some("놉"),
        ('누', "ㅂ") => Some("눕"),
        ('다', "ㅂ") => Some("답"),
        ('더', "ㅂ") => Some("덥"),
        ('도', "ㅂ") => Some("답"),
        ('두', "ㅂ") => Some("둡"),
        ('마', "ㅂ") => Some("맙"),
        ('머', "ㅂ") => Some("멥"),
        ('모', "ㅂ") => Some("몹"),
        ('무', "ㅂ") => Some("뭅"),
        ('사', "ㅂ") => Some("삽"),
        ('서', "ㅂ") => Some("섭"),
        ('소', "ㅂ") => Some("솝"),
        ('수', "ㅂ") => Some("숩"),
        ('시', "ㅂ") => Some("십"),
        ('아', "ㅂ") => Some("앍"),
        ('어', "ㅂ") => Some("얍"),
        ('오', "ㅂ") => Some("옵"),
        ('우', "ㅂ") => Some("웁"),
        ('이', "ㅂ") => Some("입"),
        ('자', "ㅂ") => Some("잡"),
        ('저', "ㅂ") => Some("접"),
        ('조', "ㅂ") => Some("좁"),
        ('주', "ㅂ") => Some("줍"),
        ('타', "ㅂ") => Some("탑"),
        ('터', "ㅂ") => Some("텁"),
        ('토', "ㅂ") => Some("톱"),
        ('투', "ㅂ") => Some("툽"),
        ('하', "ㅂ") => Some("합"),
        ('허', "ㅂ") => Some("헙"),
        ('호', "ㅂ") => Some("홉"),
        ('후', "ㅂ") => Some("훕"),
        ('히', "ㅂ") => Some("힙"),
        ('카', "ㅂ") => Some("캅"),
        ('커', "ㅂ") => Some("컵"),
        ('코', "ㅂ") => Some("컵"),
        ('쿠', "ㅂ") => Some("쿱"),
        ('키', "ㅂ") => Some("킵"),
        ('파', "ㅂ") => Some("팝"),
        ('퍼', "ㅂ") => Some("펍"),
        ('포', "ㅂ") => Some("팝"),
        ('푸', "ㅂ") => Some("푹"),
        ('피', "ㅂ") => Some("핍"),
        ('바', "ㅂ") => Some("밥"),
        ('버', "ㅂ") => Some("법"),
        ('보', "ㅂ") => Some("봅"),
        ('부', "ㅂ") => Some("북"),
        ('비', "ㅂ") => Some("빕"),

        // ㅇ batchim (nasal ng)
        ('가', "ㅇ") => Some("강"),
        ('기', "ㅇ") => Some("김"),
        ('고', "ㅇ") => Some("공"),
        ('구', "ㅇ") => Some("궁"),
        ('나', "ㅇ") => Some("낭"),
        ('니', "ㅇ") => Some("닝"),
        ('노', "ㅇ") => Some("농"),
        ('누', "ㅇ") => Some("농"),
        ('다', "ㅇ") => Some("당"),
        ('더', "ㅇ") => Some("덩"),
        ('도', "ㅇ") => Some("동"),
        ('두', "ㅇ") => Some("둥"),
        ('마', "ㅇ") => Some("망"),
        ('머', "ㅇ") => Some("멍"),
        ('모', "ㅇ") => Some("몽"),
        ('무', "ㅇ") => Some("뭉"),
        ('사', "ㅇ") => Some("상"),
        ('서', "ㅇ") => Some("성"),
        ('소', "ㅇ") => Some("송"),
        ('수', "ㅇ") => Some("숭"),
        ('시', "ㅇ") => Some("싱"),
        ('아', "ㅇ") => Some("앙"),
        ('어', "ㅇ") => Some("엉"),
        ('오', "ㅇ") => Some("옹"),
        ('우', "ㅇ") => Some("웅"),
        ('이', "ㅇ") => Some("잉"),
        ('자', "ㅇ") => Some("장"),
        ('저', "ㅇ") => Some("정"),
        ('조', "ㅇ") => Some("종"),
        ('주', "ㅇ") => Some("중"),
        ('타', "ㅇ") => Some("탕"),
        ('터', "ㅇ") => Some("텅"),
        ('토', "ㅇ") => Some("통"),
        ('투', "ㅇ") => Some("퉁"),
        ('하', "ㅇ") => Some("항"),
        ('허', "ㅇ") => Some("형"),
        ('호', "ㅇ") => Some("홍"),
        ('후', "ㅇ") => Some("흉"),
        ('히', "ㅇ") => Some("힝"),
        ('카', "ㅇ") => Some("강"),
        ('커', "ㅇ") => Some("컹"),
        ('코', "ㅇ") => Some("공"),
        ('쿠', "ㅇ") => Some("궁"),
        ('키', "ㅇ") => Some("킹"),
        ('파', "ㅇ") => Some("팡"),
        ('퍼', "ㅇ") => Some("펑"),
        ('포', "ㅇ") => Some("퐁"),
        ('푸', "ㅇ") => Some("풍"),
        ('피', "ㅇ") => Some("핑"),
        ('바', "ㅇ") => Some("방"),
        ('버', "ㅇ") => Some("벙"),
        ('보', "ㅇ") => Some("봉"),
        ('부', "ㅇ") => Some("붕"),
        ('비', "ㅇ") => Some("빙"),

        // ㄱ batchim
        ('가', "ㄱ") => Some("각"),
        ('기', "ㄱ") => Some("긱"),
        ('고', "ㄱ") => Some("곡"),
        ('구', "ㄱ") => Some("국"),
        ('나', "ㄱ") => Some("낙"),
        ('니', "ㄱ") => Some("닉"),
        ('노', "ㄱ") => Some("녹"),
        ('누', "ㄱ") => Some("눅"),
        ('다', "ㄱ") => Some("닥"),
        ('더', "ㄱ") => Some("덕"),
        ('도', "ㄱ") => Some("독"),
        ('두', "ㄱ") => Some("둑"),
        ('마', "ㄱ") => Some("막"),
        ('머', "ㄱ") => Some("먹"),
        ('모', "ㄱ") => Some("목"),
        ('무', "ㄱ") => Some("묵"),
        ('사', "ㄱ") => Some("삭"),
        ('서', "ㄱ") => Some("석"),
        ('소', "ㄱ") => Some("속"),
        ('수', "ㄱ") => Some("숙"),
        ('시', "ㄱ") => Some("식"),
        ('아', "ㄱ") => Some("악"),
        ('어', "ㄱ") => Some("억"),
        ('오', "ㄱ") => Some("옥"),
        ('우', "ㄱ") => Some("욱"),
        ('이', "ㄱ") => Some("익"),
        ('자', "ㄱ") => Some("작"),
        ('저', "ㄱ") => Some("적"),
        ('조', "ㄱ") => Some("족"),
        ('주', "ㄱ") => Some("죽"),
        ('타', "ㄱ") => Some("탁"),
        ('터', "ㄱ") => Some("턱"),
        ('토', "ㄱ") => Some("톡"),
        ('투', "ㄱ") => Some("툭"),
        ('하', "ㄱ") => Some("학"),
        ('허', "ㄱ") => Some("혁"),
        ('호', "ㄱ") => Some("혹"),
        ('후', "ㄱ") => Some("흑"),
        ('히', "ㄱ") => Some("힉"),
        ('카', "ㄱ") => Some("각"),
        ('커', "ㄱ") => Some("컥"),
        ('코', "ㄱ") => Some("콕"),
        ('쿠', "ㄱ") => Some("국"),
        ('키', "ㄱ") => Some("킥"),
        ('파', "ㄱ") => Some("팍"),
        ('퍼', "ㄱ") => Some("펙"),
        ('포', "ㄱ") => Some("폭"),
        ('푸', "ㄱ") => Some("푹"),
        ('피', "ㄱ") => Some("픽"),
        ('바', "ㄱ") => Some("박"),
        ('버', "ㄱ") => Some("벅"),
        ('보', "ㄱ") => Some("복"),
        ('부', "ㄱ") => Some("북"),
        ('비', "ㄱ") => Some("빅"),

        // ㄹ batchim
        ('가', "ㄹ") => Some("갈"),
        ('기', "ㄹ") => Some("길"),
        ('고', "ㄹ") => Some("골"),
        ('구', "ㄹ") => Some("굴"),
        ('나', "ㄹ") => Some("날"),
        ('니', "ㄹ") => Some("닐"),
        ('노', "ㄹ") => Some("놀"),
        ('누', "ㄹ") => Some("눌"),
        ('다', "ㄹ") => Some("달"),
        ('더', "ㄹ") => Some("덜"),
        ('도', "ㄹ") => Some("돌"),
        ('두', "ㄹ") => Some("둘"),
        ('마', "ㄹ") => Some("말"),
        ('머', "ㄹ") => Some("멀"),
        ('모', "ㄹ") => Some("몰"),
        ('무', "ㄹ") => Some("물"),
        ('사', "ㄹ") => Some("살"),
        ('서', "ㄹ") => Some("설"),
        ('소', "ㄹ") => Some("솔"),
        ('수', "ㄹ") => Some("술"),
        ('시', "ㄹ") => Some("실"),
        ('아', "ㄹ") => Some("알"),
        ('어', "ㄹ") => Some("얼"),
        ('오', "ㄹ") => Some("올"),
        ('우', "ㄹ") => Some("울"),
        ('이', "ㄹ") => Some("일"),
        ('자', "ㄹ") => Some("잘"),
        ('저', "ㄹ") => Some("절"),
        ('조', "ㄹ") => Some("졸"),
        ('주', "ㄹ") => Some("줄"),
        ('타', "ㄹ") => Some("탈"),
        ('터', "ㄹ") => Some("털"),
        ('토', "ㄹ") => Some("톨"),
        ('투', "ㄹ") => Some("툴"),
        ('하', "ㄹ") => Some("할"),
        ('허', "ㄹ") => Some("헐"),
        ('호', "ㄹ") => Some("홀"),
        ('후', "ㄹ") => Some("훌"),
        ('히', "ㄹ") => Some("힐"),
        ('카', "ㄹ") => Some("칼"),
        ('커', "ㄹ") => Some("컬"),
        ('코', "ㄹ") => Some("콜"),
        ('쿠', "ㄹ") => Some("쿨"),
        ('키', "ㄹ") => Some("킬"),
        ('파', "ㄹ") => Some("팔"),
        ('퍼', "ㄹ") => Some("펄"),
        ('포', "ㄹ") => Some("폴"),
        ('푸', "ㄹ") => Some("풀"),
        ('피', "ㄹ") => Some("필"),
        ('바', "ㄹ") => Some("발"),
        ('버', "ㄹ") => Some("벌"),
        ('보', "ㄹ") => Some("볼"),
        ('부', "ㄹ") => Some("불"),
        ('비', "ㄹ") => Some("빌"),

        // ㅎ batchim
        ('가', "ㅎ") => Some("각"),
        ('기', "ㅎ") => Some("긱"),
        ('고', "ㅎ") => Some("곡"),
        ('구', "ㅎ") => Some("국"),
        ('나', "ㅎ") => Some("낙"),
        ('니', "ㅎ") => Some("닉"),
        ('노', "ㅎ") => Some("녹"),
        ('누', "ㅎ") => Some("눅"),
        ('다', "ㅎ") => Some("닥"),
        ('더', "ㅎ") => Some("덕"),
        ('도', "ㅎ") => Some("독"),
        ('두', "ㅎ") => Some("둑"),
        ('마', "ㅎ") => Some("막"),
        ('머', "ㅎ") => Some("먹"),
        ('모', "ㅎ") => Some("목"),
        ('무', "ㅎ") => Some("묵"),
        ('사', "ㅎ") => Some("삭"),
        ('서', "ㅎ") => Some("석"),
        ('소', "ㅎ") => Some("속"),
        ('수', "ㅎ") => Some("숙"),
        ('시', "ㅎ") => Some("식"),
        ('아', "ㅎ") => Some("악"),
        ('어', "ㅎ") => Some("억"),
        ('오', "ㅎ") => Some("옥"),
        ('우', "ㅎ") => Some("욱"),
        ('이', "ㅎ") => Some("익"),
        ('자', "ㅎ") => Some("작"),
        ('저', "ㅎ") => Some("적"),
        ('조', "ㅎ") => Some("족"),
        ('주', "ㅎ") => Some("죽"),
        ('타', "ㅎ") => Some("탁"),
        ('터', "ㅎ") => Some("턱"),
        ('토', "ㅎ") => Some("톡"),
        ('투', "ㅎ") => Some("툭"),
        ('하', "ㅎ") => Some("학"),
        ('허', "ㅎ") => Some("혁"),
        ('호', "ㅎ") => Some("혹"),
        ('후', "ㅎ") => Some("흑"),
        ('히', "ㅎ") => Some("힉"),
        ('카', "ㅎ") => Some("각"),
        ('커', "ㅎ") => Some("컥"),
        ('코', "ㅎ") => Some("콕"),
        ('쿠', "ㅎ") => Some("국"),
        ('키', "ㅎ") => Some("킥"),
        ('파', "ㅎ") => Some("팍"),
        ('퍼', "ㅎ") => Some("펙"),
        ('포', "ㅎ") => Some("폭"),
        ('푸', "ㅎ") => Some("푹"),
        ('피', "ㅎ") => Some("픽"),
        ('바', "ㅎ") => Some("박"),
        ('버', "ㅎ") => Some("벅"),
        ('보', "ㅎ") => Some("복"),
        ('부', "ㅎ") => Some("북"),
        ('비', "ㅎ") => Some("빅"),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_vowels() {
        assert_eq!(hiragana_to_hangul("あいうえお"), "아이우에오");
    }

    #[test]
    fn test_ka_row() {
        assert_eq!(hiragana_to_hangul("かきくけこ"), "카키쿠케코");
    }

    #[test]
    fn test_sakura() {
        assert_eq!(hiragana_to_hangul("さくら"), "사쿠라");
    }

    #[test]
    fn test_tokyo() {
        assert_eq!(hiragana_to_hangul("とうきょう"), "토우쿄우");
    }

    #[test]
    fn test_sokuon_gakkou() {
        assert_eq!(hiragana_to_hangul("がっこう"), "갓코우");
    }

    #[test]
    fn test_sokuon_kitte() {
        assert_eq!(hiragana_to_hangul("きって"), "킷테");
    }

    #[test]
    fn test_sokuon_massugu() {
        assert_eq!(hiragana_to_hangul("まっすぐ"), "맛스구");
    }

    #[test]
    fn test_youon_shashoku() {
        assert_eq!(hiragana_to_hangul("しゃしん"), "샤신");
    }

    #[test]
    fn test_youon_chukuwai() {
        assert_eq!(hiragana_to_hangul("ちゅうがく"), "추우가쿠");
    }

    #[test]
    fn test_n_shinkansenrule() {
        // しんかんせん: n always becomes ㄴ
        assert_eq!(hiragana_to_hangul("しんかんせん"), "신칸센");
    }

    #[test]
    fn test_n_sanporule() {
        // さんぽ: n before p → ㅁ
        assert_eq!(hiragana_to_hangul("さんぽ"), "삼포");
    }

    #[test]
    fn test_n_shinbunrule() {
        // しんぶん: n before b → ㅁ (心分 also acceptable)
        assert_eq!(hiragana_to_hangul("しんぶん"), "심분");
    }

    #[test]
    fn test_dakuten_arigatou() {
        assert_eq!(hiragana_to_hangul("ありがとう"), "아리가토우");
    }

    #[test]
    fn test_mixed_sumimasen() {
        assert_eq!(hiragana_to_hangul("すみません"), "스미마센");
    }

    #[test]
    fn test_sa_row() {
        assert_eq!(hiragana_to_hangul("さしすせそ"), "사시스세소");
    }

    #[test]
    fn test_ta_row() {
        assert_eq!(hiragana_to_hangul("たちつてと"), "타치츠테토");
    }

    #[test]
    fn test_na_row() {
        assert_eq!(hiragana_to_hangul("なにぬねの"), "나니누네노");
    }

    #[test]
    fn test_ha_row() {
        assert_eq!(hiragana_to_hangul("はひふへほ"), "하히후헤호");
    }

    #[test]
    fn test_ma_row() {
        assert_eq!(hiragana_to_hangul("まみむめも"), "마미무메모");
    }

    #[test]
    fn test_ya_row() {
        assert_eq!(hiragana_to_hangul("やゆよ"), "야유요");
    }

    #[test]
    fn test_ra_row() {
        assert_eq!(hiragana_to_hangul("らりるれろ"), "라리루레로");
    }

    #[test]
    fn test_wa_row() {
        assert_eq!(hiragana_to_hangul("わをん"), "와온");
    }

    #[test]
    fn test_ga_row() {
        assert_eq!(hiragana_to_hangul("がぎぐげご"), "가기구게고");
    }

    #[test]
    fn test_za_row() {
        assert_eq!(hiragana_to_hangul("ざじずぜぞ"), "자지즈제조");
    }

    #[test]
    fn test_da_row() {
        assert_eq!(hiragana_to_hangul("だぢづでど"), "다지즈데도");
    }

    #[test]
    fn test_ba_row() {
        assert_eq!(hiragana_to_hangul("ばびぶべぼ"), "바비부베보");
    }

    #[test]
    fn test_pa_row() {
        assert_eq!(hiragana_to_hangul("ぱぴぷぺぽ"), "파피푸페포");
    }

    #[test]
    fn test_youon_kya_kyu_kyo() {
        assert_eq!(hiragana_to_hangul("きゃきゅきょ"), "캬큐쿄");
    }

    #[test]
    fn test_youon_sha_shu_sho() {
        assert_eq!(hiragana_to_hangul("しゃしゅしょ"), "샤슈쇼");
    }

    #[test]
    fn test_youon_cha_chu_cho() {
        assert_eq!(hiragana_to_hangul("ちゃちゅちょ"), "차추초");
    }

    #[test]
    fn test_youon_nya_nyu_nyo() {
        assert_eq!(hiragana_to_hangul("にゃにゅにょ"), "냐뉴뇨");
    }

    #[test]
    fn test_youon_hya_hyu_hyo() {
        assert_eq!(hiragana_to_hangul("ひゃひゅひょ"), "햐휴효");
    }

    #[test]
    fn test_youon_mya_myu_myo() {
        assert_eq!(hiragana_to_hangul("みゃみゅみょ"), "먀뮤묘");
    }

    #[test]
    fn test_youon_rya_ryu_ryo() {
        assert_eq!(hiragana_to_hangul("りゃりゅりょ"), "랴류료");
    }

    #[test]
    fn test_youon_gya_gyu_gyo() {
        assert_eq!(hiragana_to_hangul("ぎゃぎゅぎょ"), "갸규교");
    }

    #[test]
    fn test_youon_bya_byu_byo() {
        assert_eq!(hiragana_to_hangul("びゃびゅびょ"), "뱌뷰뵤");
    }

    #[test]
    fn test_youon_pya_pyu_pyo() {
        assert_eq!(hiragana_to_hangul("ぴゃぴゅぴょ"), "퍄퓨표");
    }

    #[test]
    fn test_n_end_of_string() {
        // ん at end → ㄴ
        assert_eq!(hiragana_to_hangul("せん"), "센");
    }

    #[test]
    fn test_n_before_vowel() {
        // ん before vowel → ㄴ
        assert_eq!(hiragana_to_hangul("んあ"), "ㄴ아");
    }

    #[test]
    fn test_sokuon_ippai() {
        // いっぱい: っ before ぱ → ㅂ batchim
        assert_eq!(hiragana_to_hangul("いっぱい"), "잇파이");
    }

    #[test]
    fn test_multiple_sokuon() {
        // Multiple sokuon: あった
        assert_eq!(hiragana_to_hangul("あった"), "앗타");
    }

    #[test]
    fn test_complex_sentence() {
        // こんにちは
        assert_eq!(hiragana_to_hangul("こんにちは"), "콘니치하");
    }
}
