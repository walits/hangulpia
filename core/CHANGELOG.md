# Changelog

이 프로젝트의 모든 주요 변경 사항을 기록합니다.

## [0.1.3] - 2026-07-22

### Fixed
- **조사 は 오변환 수정**: 문장에서 조사로 쓰인 '와'가 わ로 잘못 변환되던 문제 (예: 나마에와 → なまえわ, なまえは가 맞음). `BeamDecoder::decode_sentence()` 추가 — 문장을 공백 기준으로 단어 분리 후, 표준 조사/어미 규칙을 문자 단위 통계 모델보다 우선 적용. 실제 macOS 앱의 실시간 변환 경로(`hj_hangul_to_hiragana`)에도 동일하게 적용됨.
- **です/ます 활용형 탁음 오류 수정**: '데스', '데스카', '마스' 등으로 끝나는 단어가 ㄷ 초성의 무성음 우세 확률(t 60% vs d 40%) 때문에 てすか처럼 잘못 변환되던 문제 (난데스카 → なんてすか, なんですか가 맞음).
- **실제 앱 어휘 크기 확대**: `hj_engine_init()`이 731개 단어(`build_vocab()`)만 쓰던 것을, 24,914개 전체 어휘(`build_vocab()` + `build_extended_vocab()` + `build_vocab_large()`)를 쓰도록 변경. 참고: 저장소의 `data/hj-ime-large.db`(11만 문장 학습 산출물)는 실제 앱 초기화 경로에 연결된 적이 없었음 — 연구용 산출물로만 존재했음. 이번 수정은 그것과 무관하게, 앱이 기동 시 즉석에서 구축하는 어휘를 확장한 것.

## [0.1.2] - 2026-04-07

### Fixed
- **종성 ㄴ 변환 실패 수정**: 곤(こん), 산(さん), 칸(かん), 킨(きん) 등 받침 ㄴ이 있는 글자가 변환되지 않고 한글 그대로 출력되던 버그 수정. `romaji_to_hiragana_simple()`에서 `kon`, `san` 등이 `ends_with("on")`, `ends_with("an")` 조건에 걸려 ん 분리가 실패하던 문제. 종성을 romaji에 합치지 않고 별도로 처리하도록 구조 변경.
- **관용 표기 변환 지원**: '곤니찌와' → こんにちわ 정상 변환 (v0.1.0에서는 '곤にじわ' 출력)
- **ㅉ→ち 매핑 강화**: ㅉ(쌍지읒)의 초성 매핑에서 ch 확률을 0.35로 상향 (기존 j=0.7)
- **romaji nn+모음 파싱 수정**: `konnichiwa`가 `こんいちわ`로 파싱되던 버그 수정. `nn` 뒤에 모음이 올 때 첫 `n`만 ん으로 변환.

### Added
- **BeamDecoder 규칙 기반 폴백**: 학습 데이터에 없는 한글도 자모 분해 → 로마자 → 히라가나 규칙 기반 변환으로 처리
- **일본어 위키피디아 학습 데이터 통합**: 110K 문장 (191MB) 코퍼스 추가, `build_from_external_corpus()` API
- **종성 변환 테스트 추가**: `test_fallback_jongseong_n`, `test_fallback_jongseong_various`, `test_fallback_konnichiwa_full`

### Changed
- `phoneme.rs`: ㅉ(13) 매핑을 `[ch(0.4), j(0.3), z(0.2), ts(0.1)]`로 변경
- `phoneme.rs`: ㅘ(9) 매핑에 `a(0.3)` 대안 추가
- `phonetic_decoder.rs`: 종성 처리를 base syllable 변환 후 suffix 직접 부착 방식으로 변경

## [0.1.0] - 2026-04-06

### Added
- 초기 릴리스
- 한글 두벌식 → 일본어 변환 엔진 (Rust)
- macOS InputMethodKit 앱 (Swift + Rust FFI)
- Windows IME 스텁
- CLI 프로토타입 (`hj-ime`)
- BeamDecoder 기반 음소 변환
- 코사인 유사도 기반 문맥 랭킹 (3-factor: phoneme 0.3, context 0.5, freq 0.2)
- 양방향 재랭킹 (SentenceBuffer)
- N-gram 언어 모델
- 자동완성 엔진 (4-factor scoring)
- 학습 코퍼스 생성기 (합성 데이터)
- GloVe 스타일 임베딩 학습 (64차원)
