//! Hangul syllable composition automaton.

use ime_core::engine::{InputEngine, KeyResult};
use ime_core::{ComposingBuffer, Language};

/// State of the Hangul composition automaton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// No composition in progress.
    Empty,
    /// Choseong (initial consonant) entered.
    Choseong,
    /// Choseong + Jungseong (vowel) entered.
    Jungseong,
    /// Complete syllable with Jongseong (final consonant).
    Jongseong,
}

/// Hangul input engine using Dubeolsik (두벌식) keyboard layout.
#[derive(Debug)]
pub struct HangulEngine {
    state: State,
    choseong: Option<u32>,
    jungseong: Option<u32>,
    jongseong: Option<u32>,
}

impl HangulEngine {
    pub fn new() -> Self {
        Self {
            state: State::Empty,
            choseong: None,
            jungseong: None,
            jongseong: None,
        }
    }
}

impl Default for HangulEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl InputEngine for HangulEngine {
    fn language(&self) -> Language {
        Language::Korean
    }

    fn process_key(&mut self, _key: char, _buffer: &mut ComposingBuffer) -> KeyResult {
        // TODO: Implement Hangul composition automaton
        KeyResult::Pass
    }

    fn reset(&mut self) {
        self.state = State::Empty;
        self.choseong = None;
        self.jungseong = None;
        self.jongseong = None;
    }
}
