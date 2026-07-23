# Hangulpia 변환 엔진 가이드

한글(두벌식) → 일본어(히라가나) 변환 "모델"이 실제로 무엇이고, 어떻게 만들어지고,
어디서 어떻게 구동되는지 정리한 문서입니다. 딥러닝 모델이 아니라 **통계적 음소 정렬
테이블(PhoneticMap) + 손으로 정리한 문법 예외 규칙**으로 이루어져 있습니다.

- 코드 위치: `core/crates/db/src/phonetic_decoder.rs` (핵심 엔진)
- 관련 데이터: `core/crates/db/src/vocab.rs`, `vocab_extended.rs`, `vocab_large.rs`
- 실사용처: macOS 앱(`core/crates/macos-ime`), 홈페이지 데모(`homepage/index.html`)

---

## 0. 한눈에 보기

```
                    ┌─────────────────────────────┐
                    │   어휘 3종 세트 (24,914개)    │
                    │  vocab.rs + vocab_extended   │
                    │  .rs + vocab_large.rs        │
                    └───────────────┬───────────────┘
                                    │ build_from_pairs()
                                    ▼
                    ┌─────────────────────────────┐
                    │       PhoneticMap            │
                    │  hangul_token → [(hiragana,  │
                    │       probability), ...]      │
                    └───────────────┬───────────────┘
                                    │
                                    ▼
                    ┌─────────────────────────────┐
                    │       BeamDecoder            │
                    │  .decode()        원시 후보  │
                    │  .decode_sentence() 추천 API │
                    │   └─ KNOWN_WORDS 사전 매칭    │
                    │   └─ KNOWN_SUFFIXES 문법 규칙 │
                    │   └─ 실패 시 자모 분해 fallback│
                    └───────┬───────────────┬───────┘
                            │               │
              ┌─────────────┘               └─────────────┐
              ▼                                            ▼
   macOS 앱 (FFI, 24,914개 전체)              홈페이지 데모 (JS 포팅, 10개 사전만)
```

---

## 1. 모델(PhoneticMap)을 생성하는 방법

### 1.1 이게 "학습된 모델"이 아닌 이유

이름은 통계적이지만 신경망 학습이 아닙니다. **결정론적으로 계산 가능한 역방향
테이블**을 만드는 것에 가깝습니다.

- 일본어 히라가나 → 한글 표기는 규칙만으로 결정됩니다 (`kana_hangul.rs`의
  `hiragana_to_hangul()` — つ→츠, ち→치 같은 고정 매핑).
- 그래서 (히라가나 읽기, 한글 표기) 쌍이 있으면, 그 반대 방향(한글→히라가나)의
  빈도 기반 확률 테이블을 **역산**할 수 있습니다. 이게 `PhoneticMap`입니다.

### 1.2 실제 빌드 코드

`core/crates/db/src/phonetic_decoder.rs`:

```rust
pub struct PhoneticMap {
    pub map: HashMap<String, Vec<PhoneticMapping>>, // hangul_token → 후보들
    pub total_pairs: u64,
}

impl PhoneticMap {
    pub fn build_from_pairs(&mut self, pairs: &[(String, String, u64)]) {
        // pairs = (hiragana_reading, hangul, frequency)
        // align_hiragana_hangul()로 문자 단위 정렬 후 빈도 카운트 → 확률로 정규화
    }
}
```

`align_hiragana_hangul()`은 ん(받침 처리)/っ(촉음)/요음(2글자 히라가나 ↔ 1글자
한글) 같은 비1:1 매칭까지 처리합니다.

### 1.3 실제로 모델을 만드는 명령

```bash
cd core

# 방법 A — 앱이 매번 부팅 시 하는 것과 동일 (in-memory, 파일 없음)
#   crates/macos-ime/src/ffi.rs::hj_engine_init() 참고

# 방법 B — CLI로 SQLite에 영구 저장 (연구/실험용, 실제 앱은 이 파일을 안 읽음)
cargo run -p ime-cli --bin hj-ime -- db-build-large my-model.db --count 100000
```

`db-build-large`는 `generator.rs`의 템플릿 기반으로 **합성 문장**을 생성해 학습
쌍을 만듭니다 (실제 위키피디아 원문이 아님 — 1.4절 참고).

### 1.4 어휘 소스 3종 (2026-07-23 기준, v0.1.3)

| 소스 | 함수 | 항목 수 | 성격 |
|---|---|---|---|
| `vocab.rs` | `build_vocab()` | 731 | 손으로 큐레이션한 인사말/가족/기초 단어 |
| `vocab_extended.rs` | `build_extended_vocab()` | 711 | 확장 어휘 |
| `vocab_large.rs` | `build_vocab_large()` | 22,741 | `gen_vocab_large.py`로 자동 생성 (스크립트 자체는 저장소에 없음) |
| **합계** | `build_vocab()+build_extended_vocab()+build_vocab_large()` | **24,914** | v0.1.3부터 실제 앱이 이걸 사용 |

**⚠️ 중요한 함정 (직접 검증됨)**: 저장소에 있던 `data/hj-ime-large.db`(2.8MB,
CHANGELOG에 "11만 문장 위키피디아 코퍼스"로 기록됨)는 **실제 앱 초기화 경로에
연결된 적이 없습니다.** `hj_engine_init()`은 항상 위 표의 어휘를 즉석에서
새로 빌드합니다. 그 DB 파일은 연구용 산출물로만 존재했고, 지금은 로컬에서
빈 파일(0바이트)로 확인됨 — git에도 추적된 적 없음(`*.db`는 `.gitignore`
대상). 진짜 11만 문장 코퍼스 원본(191MB)도 저장소에 없습니다.

### 1.5 예외 규칙 (통계로 안 잡히는 것들)

`PhoneticMap`은 문자 정렬 통계라서 **문법**을 모릅니다. 조사 は(발음 wa)나
です/ます 활용형처럼 "문맥에 따라 다르게 써야 하는" 패턴은 학습으로 안 풀려서,
`phonetic_decoder.rs`에 하드코딩된 예외 테이블로 별도 처리합니다:

```rust
const KNOWN_WORDS: &[(&str, &str)] = &[
    ("사쿠라", "さくら"), ("아리가토", "ありがとう"), ("곤니찌와", "こんにちわ"),
    /* ... */
];

const KNOWN_SUFFIXES: &[(&str, &str)] = &[
    ("데스카", "ですか"), ("데스", "です"), ("마스", "ます"),
    ("와", "は"), // 조사, 반드시 맨 뒤(최단 매치)에 위치
];
```

새 예외를 추가하고 싶으면 이 두 배열에 추가 + `homepage/index.html`의 동일한
이름의 JS 객체(`KNOWN_WORDS`/`KNOWN_SUFFIXES`)에도 **반드시 동기화**해야 합니다
(3절 참고).

---

## 2. 모델을 이용하는 방법 (Rust API)

### 2.1 최소 예시

```rust
use ime_db::phonetic_decoder::{PhoneticMap, BeamDecoder};
use ime_db::kana_hangul::hiragana_to_hangul;
use ime_db::vocab::build_vocab;
use ime_db::vocab_extended::build_extended_vocab;
use ime_db::vocab_large::build_vocab_large;

let mut vocab = build_vocab();
vocab.extend(build_extended_vocab());
vocab.extend(build_vocab_large());

let pairs: Vec<(String, String, u64)> = vocab.iter()
    .map(|v| (v.reading.to_string(), hiragana_to_hangul(v.reading), 100u64))
    .collect();

let mut map = PhoneticMap::new();
map.build_from_pairs(&pairs);

let decoder = BeamDecoder::new(&map, /* beam_width */ 6, /* max_candidates */ 5);

// 추천 API — 문장 단위, 예외 규칙 자동 적용, 최선 후보 1개만 반환
let result: String = decoder.decode_sentence("나마에와 난데스카");
// => "なまえは なんですか"

// 원시 API — 후보 여러 개 + 신뢰도 (단어 하나 단위, 예외 규칙 미적용)
let candidates: Vec<(String, f64)> = decoder.decode("사쿠라");
// => [("さくら", 0.97..), ...]
```

`decode_sentence()`가 실제 앱과 동일한 결과를 주는 진입점입니다.
`decode()`를 직접 쓸 거면 조사/활용형 예외가 안 걸린다는 점을 기억하세요.

### 2.2 CLI로 써보기

```bash
cd core
cargo run -p ime-cli --bin hj-ime -- "사쿠라"
cargo run -p ime-cli --bin hj-ime -- --interactive
```

⚠️ 이 기본 CLI 경로(`crates/cli/src/main.rs`)는 **아직 위 `BeamDecoder`가 아니라
더 오래된 규칙 기반 파이프라인**(`ime_hangul::phoneme` + `ime_japanese::romaji`)을
씁니다. `BeamDecoder`/`decode_sentence()`를 실제로 검증하려면 1.3절처럼 별도
example을 짜서 돌리거나, `--db` 옵션으로 미리 만든 DB를 지정해야 합니다.

### 2.3 테스트

```bash
cargo test -p ime-db phonetic_decoder
```

`곤니찌와`, `와`(조사), `데스카` 등 알려진 함정 케이스가 회귀 테스트로 고정되어
있습니다 (`phonetic_decoder.rs` 하단 `mod tests`).

---

## 3. 웹페이지에서 모델을 이용하는 방법 (구동·연동 메커니즘)

### 3.1 요약

홈페이지(`homepage/index.html`)의 "Try it yourself" / "sentence mode" 데모는
**진짜 24,914개 PhoneticMap을 쓰지 않습니다.** 그 데이터를 브라우저로 보내는 건
비현실적이라, `phonetic_decoder.rs`의 **규칙 기반 fallback 경로**
(`hangul_char_to_hiragana_fallback` + `romaji_to_hiragana_simple`)와 **예외
테이블**(`KNOWN_WORDS`/`KNOWN_SUFFIXES`)을 그대로 JavaScript로 손으로 포팅해서
씁니다.

```
core/crates/db/src/phonetic_decoder.rs   (Rust, 정답 소스)
        │  사람이 손으로 동일 로직을 JS로 옮김 (자동 변환 아님)
        ▼
homepage/index.html <script> 안의 IIFE   (JS, 브라우저에서 실행)
```

### 3.2 구동 메커니즘 (런타임)

1. 정적 HTML/CSS/JS 한 파일. 빌드 스텝도, 서버도, API 호출도 없습니다.
2. 페이지 로드 시 즉시 실행되는 `(function () { ... })()` 블록 하나가:
   - 자모 분해 테이블(`CHOSEONG_MAP`, `JUNGSEONG_MAP`), 로마자→히라가나 표
     (`HIRA_TABLE`), 예외 사전(`KNOWN_WORDS`, `KNOWN_SUFFIXES`)을 정의
   - `wire(inputId, outputId)` 함수로 두 개의 입력창을 연결:
     - `#tryInput` → `#mockOutput` (짧은 단어 데모)
     - `#playgroundInput` → `#playgroundOutput` (긴 문장 연습)
   - 각 입력창의 `input` 이벤트마다 `hangulToHiragana(value)`를 호출해 실시간 렌더링
3. `hangulToHiragana()`는 공백 기준으로 단어를 쪼갠 뒤 각 단어를
   `convertWord()`에 넘기고, `convertWord()`는:
   1. `KNOWN_WORDS` 정확히 일치 → 그 값 반환
   2. `KNOWN_SUFFIXES` 중 가장 긴 것부터 접미사 매치 → 어간은 재귀 처리 + 어미는 고정값
   3. 둘 다 아니면 글자 하나씩 `hangulCharToHiraganaFallback()`으로 분해 (자모 → 로마자 후보들 → 확률 최고값 픽)

### 3.3 실제 엔진과의 차이 (정직하게)

| | Rust 실제 엔진 | 홈페이지 JS 데모 |
|---|---|---|
| 어휘 규모 | 24,914개 학습 | 10개 하드코딩 |
| 통계적 확률 결합 | O (BeamDecoder 빔서치) | X (단순 최고 확률 1개만) |
| 조사/활용형 예외 | O | O (동일 목록 수동 동기화) |
| 실행 위치 | 사용자 기기의 네이티브 프로세스 | 사용자 브라우저 (JS) |

`KNOWN_WORDS`/`KNOWN_SUFFIXES`를 한쪽만 고치면 데모와 실제 앱 결과가 어긋나므로
**항상 양쪽 다 수정**하세요:
- `core/crates/db/src/phonetic_decoder.rs` (Rust `const` 배열)
- `homepage/index.html` (JS 객체/배열, 같은 이름)

### 3.4 배포 메커니즘

```
git push (homepage/** 변경)
        │
        ▼
.github/workflows/deploy-homepage.yml   (GitHub Actions)
   actions/upload-pages-artifact  ← homepage/ 폴더 전체를 그대로 업로드
   actions/deploy-pages           → GitHub Pages
        │
        ▼
hangulpia.com  (GoDaddy DNS → GitHub Pages IP, 커스텀 도메인)
```

- 리포지토리: `walits/hangulpia` (모노레포 — `homepage/`, `core/`, `docs/` 전부 한 레포)
- Pages 배포 방식: Actions 기반 (레거시 브랜치 배포 아님) — `homepage/` 서브폴더만 골라 배포
- 다운로드 파일(`homepage/downloads/HangulJapaneseIME-latest.zip`)도 이 폴더 안에 있어서 같이 배포됨
- 비용: $0/월 (GitHub Pages 무료 티어, AWS 등 별도 인프라 없음)

---

## 4. 다운로드해서 이용하는 방법 및 구동 메커니즘 (macOS 앱)

### 4.1 사용자 입장에서

```bash
# hangulpia.com에서 HangulJapaneseIME-latest.zip 다운로드 후
unzip HangulJapaneseIME-latest.zip
cd HangulJapaneseIME-v0.1.3-arm64
bash install.sh
# 로그아웃 → 재로그인
# 시스템 설정 → 키보드 → 입력 소스 편집 → + → 일본어 카테고리 → 한글일본어입력기
```

- 요구사항: macOS 13+ (Ventura), Apple Silicon(arm64) 전용 — Intel Mac/Windows 미지원
  (Windows 소스는 `core/crates/windows-ime`에 있지만 빌드된 배포판 없음)
- 제거: `bash uninstall.sh`

### 4.2 아키텍처

```
┌─────────────────────────────────────────┐
│  Swift (InputMethodKit 앱)                │
│  HJInputController.swift  ← 키 입력 수신   │
│  HJEngine.swift           ← FFI 래퍼      │
└───────────────┬───────────────────────────┘
                │ C FFI (extern "C")
                ▼
┌─────────────────────────────────────────┐
│  Rust (libhj_engine.a, 정적 라이브러리)    │
│  crates/macos-ime/src/ffi.rs              │
│    hj_engine_init()                       │
│    hj_hangul_to_hiragana()  ← 실시간 조합용│
│    hj_convert()             ← 후보창용     │
└─────────────────────────────────────────┘
```

### 4.3 구동 메커니즘 (런타임)

1. **앱 시작 시** (`HJEngine.shared.initialize()` → `hj_engine_init()`):
   - 24,914개 전체 어휘(1.4절)로 `PhoneticMap`을 **매번 새로 빌드** (파일 저장/로드 없음, 순수 in-memory)
   - 칸지 사전도 같은 어휘로 in-memory SQLite(`:memory:`)에 적재
2. **타이핑 중** (실시간 조합 텍스트):
   - `HJInputController`가 두벌식 자모를 `hangulBuffer`에 누적
   - 매 키 입력마다 `HJEngine.shared.toHiragana()` → `hj_hangul_to_hiragana()`
     → `BeamDecoder::decode_sentence()` (v0.1.3부터 조사/활용형 예외 규칙 포함)
   - 결과를 조합 중 텍스트로 화면에 표시
3. **후보 선택** (숫자 1-9로 대체 후보 고를 때):
   - `HJEngine.shared.convert()` → `hj_convert()` → `BeamDecoder::decode()`로
     후보 여러 개 생성 → 각 후보로 칸지 사전 조회 → 점수순 정렬해 후보창에 표시
4. **확정** (스페이스/엔터): 조합 중이던 히라가나(또는 선택한 칸지 후보)가 커밋됨

### 4.4 빌드 메커니즘 (개발자용)

```bash
cd core/crates/macos-ime
./build.sh build      # 빌드만
./build.sh install    # 빌드 + ~/Library/Input Methods/ 에 설치 + 코드사인
./dist.sh              # 빌드 + 배포용 zip 생성 (dist/HangulJapaneseIME-vX.Y.Z-arm64.zip)
```

`build.sh` 내부:
1. `cargo build --release -p ime-macos --target aarch64-apple-darwin`
   → `target/aarch64-apple-darwin/release/libhj_engine.a`
2. `swiftc`로 Swift 소스들을 컴파일하면서 위 정적 라이브러리를 링크
   (`-framework Cocoa -framework Carbon -framework InputMethodKit -framework Security`)
3. `.app` 번들 조립 (`Info.plist`, 메뉴바 아이콘 생성, 실행 파일)

`dist.sh`는 `build.sh build`를 호출한 뒤 `.app` + `install.sh`/`uninstall.sh` +
README를 묶어 zip으로 패키징합니다. **버전을 올릴 땐 아래 세 곳을 다 맞춰야
합니다** (안 그러면 zip 파일명과 앱 내부 버전이 어긋남):

- `core/Cargo.toml` → `[workspace.package] version`
- `core/crates/macos-ime/dist.sh` → `VERSION=` 및 안내 문구 속 버전 문자열들
- `core/crates/macos-ime/HangulJapaneseIME/Resources/Info.plist` → `CFBundleShortVersionString`/`CFBundleVersion`

### 4.5 알려진 한계

- 탁음 모호성(が/か, ば/ぱ 등)은 통계 확률이 완벽히 못 잡음 — 한글 초성 하나가
  일본어 청음/탁음 둘 다에 대응되는 경우가 많아서 구조적으로 남는 문제
- 장음(長音) 누락 — 한글 표기 자체에 장음을 나타낼 방법이 마땅치 않아 「ありがとう」의
  「う」 같은 게 학습 데이터에 없으면 빠질 수 있음
- `KNOWN_WORDS`/`KNOWN_SUFFIXES`는 손으로 짠 유한 목록이라 다루지 않는 문법
  패턴은 여전히 많음 (조사 へ/を 등은 아직 미대응 — 표준 대응 후보 대비 빗나갈
  때가 더 많아서 일부러 안 넣음)
- Windows 빌드 없음, Intel Mac 미지원
