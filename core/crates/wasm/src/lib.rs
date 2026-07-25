//! WASM bindings for the homepage demo (hangulpia.com).
//!
//! Compiles the *exact same* conversion engine the shipped macOS app uses
//! (`ime_db::phonetic_decoder`, same 4-tier vocab combination as
//! `crates/macos-ime/src/ffi.rs::hj_engine_init`) to WebAssembly, so the
//! browser demo and the downloaded app produce identical output — no more
//! hand-ported JavaScript reimplementation to keep in sync.
//!
//! rusqlite (used by ime-db's SQL-backed KanjiDict etc.) doesn't target
//! wasm32-unknown-unknown, so this crate depends on ime-db with the
//! `sqlite` feature disabled and builds its own plain HashMap kanji lookup
//! from the vocab instead — see `docs/MODEL.md`.

use std::cell::OnceCell;
use std::collections::HashMap;

use ime_db::kana_hangul::hiragana_to_hangul;
use ime_db::phonetic_decoder::{BeamDecoder, PhoneticMap};
use ime_db::vocab::build_vocab;
use ime_db::vocab_extended::build_extended_vocab;
use ime_db::vocab_gapfill::build_gapfill_vocab;
use wasm_bindgen::prelude::*;

struct Engine {
    map: PhoneticMap,
    kanji: HashMap<String, String>,
}

thread_local! {
    static ENGINE: OnceCell<Engine> = OnceCell::new();
}

fn build_engine() -> Engine {
    // Same combination as hj_engine_init() in crates/macos-ime/src/ffi.rs, in
    // the same order — genuinely the same model. vocab_large.rs is
    // deliberately excluded on both sides: ~81% of its unique entries are
    // mechanically generated cartesian-product kanji compounds that don't
    // exist as real words (see docs/MODEL.md section 6.3 / ffi.rs comment).
    let mut vocab = build_vocab();
    vocab.extend(build_extended_vocab());
    vocab.extend(build_gapfill_vocab());

    let pairs: Vec<(String, String, u64)> = vocab
        .iter()
        .map(|v| (v.reading.to_string(), hiragana_to_hangul(v.reading), 100u64))
        .collect();
    let mut map = PhoneticMap::new();
    map.build_from_pairs(&pairs);

    let mut kanji = HashMap::new();
    for v in &vocab {
        if v.surface != v.reading {
            kanji.entry(v.reading.to_string()).or_insert_with(|| v.surface.to_string());
        }
    }

    Engine { map, kanji }
}

/// Convert Hangul (Dubeolsik) input to Japanese, with kanji substitution
/// where the vocabulary has a verified reading.
#[wasm_bindgen]
pub fn convert(input: &str) -> String {
    ENGINE.with(|cell| {
        let engine = cell.get_or_init(build_engine);
        let decoder = BeamDecoder::new(&engine.map, 6, 5);
        decoder.decode_sentence_with_kanji(input, &engine.kanji)
    })
}

/// Hiragana only, no kanji — matches the real app's live-typing/composition
/// text (`hj_hangul_to_hiragana`), which never shows kanji inline.
#[wasm_bindgen]
pub fn convert_hiragana_only(input: &str) -> String {
    ENGINE.with(|cell| {
        let engine = cell.get_or_init(build_engine);
        let decoder = BeamDecoder::new(&engine.map, 6, 5);
        decoder.decode_sentence(input)
    })
}

/// Vocabulary size the engine was built from (for a "trained on N words"
/// style footer/caption, kept truthful without hardcoding the number twice).
#[wasm_bindgen]
pub fn vocab_size() -> usize {
    ENGINE.with(|cell| cell.get_or_init(build_engine).map.vocab_size())
}
