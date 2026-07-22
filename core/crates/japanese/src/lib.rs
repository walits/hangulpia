//! Japanese input engine.
//!
//! Supports Romaji-to-Kana conversion and Kana-to-Kanji conversion
//! using a local SQLite dictionary.

pub mod romaji;

use ime_core::engine::{InputEngine, KeyResult};
use ime_core::{ComposingBuffer, Language};

/// Japanese input engine.
#[derive(Debug)]
pub struct JapaneseEngine {
    romaji_buffer: String,
}

impl JapaneseEngine {
    pub fn new() -> Self {
        Self {
            romaji_buffer: String::new(),
        }
    }
}

impl Default for JapaneseEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl InputEngine for JapaneseEngine {
    fn language(&self) -> Language {
        Language::Japanese
    }

    fn process_key(&mut self, _key: char, _buffer: &mut ComposingBuffer) -> KeyResult {
        // TODO: Implement romaji-to-kana conversion
        KeyResult::Pass
    }

    fn reset(&mut self) {
        self.romaji_buffer.clear();
    }
}
