//! Core input method engine for Korean (Hangul) and Japanese input.
//!
//! This crate provides the shared logic used by platform-specific IME
//! implementations (macOS InputMethodKit, Android InputMethodService).

pub mod engine;
pub mod keymap;

/// Supported input languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Korean,
    Japanese,
}

/// A composing buffer that tracks the current input state.
#[derive(Debug, Default)]
pub struct ComposingBuffer {
    /// Raw keystrokes not yet committed.
    pub pending: String,
    /// Candidate text ready to commit.
    pub committed: String,
}

impl ComposingBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.committed.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty() && self.committed.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composing_buffer_default_is_empty() {
        let buf = ComposingBuffer::new();
        assert!(buf.is_empty());
    }

    #[test]
    fn composing_buffer_clear() {
        let mut buf = ComposingBuffer::new();
        buf.pending.push_str("test");
        buf.clear();
        assert!(buf.is_empty());
    }
}
