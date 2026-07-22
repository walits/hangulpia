//! Input method engine trait and shared logic.

use crate::{ComposingBuffer, Language};

/// Result of processing a key event.
#[derive(Debug, PartialEq, Eq)]
pub enum KeyResult {
    /// The key was consumed and composition updated.
    Consumed,
    /// The key was consumed and text should be committed.
    Commit(String),
    /// The key was not handled by the IME.
    Pass,
}

/// Trait that each language-specific engine must implement.
pub trait InputEngine {
    /// The language this engine handles.
    fn language(&self) -> Language;

    /// Process a key event, updating the composing buffer.
    fn process_key(&mut self, key: char, buffer: &mut ComposingBuffer) -> KeyResult;

    /// Reset the engine state.
    fn reset(&mut self);
}
