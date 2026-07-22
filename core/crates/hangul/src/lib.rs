//! Korean (Hangul) input engine.
//!
//! Implements Hangul syllable decomposition and phoneme mapping
//! for the Korean→Japanese input method.

pub mod automaton;
pub mod jamo;
pub mod phoneme;

pub use automaton::HangulEngine;
pub use jamo::{decompose, is_hangul_syllable};
pub use phoneme::{hangul_string_to_romaji, hangul_to_romaji_candidates};
