//! HangulJapaneseIME for Windows
//!
//! System-tray application with low-level keyboard hook.
//! Intercepts QWERTY keystrokes, composes Hangul, converts to Hiragana,
//! and sends the result to the active window.
//!
//! Toggle: Ctrl+Space to enable/disable.

mod hangul;
mod engine;

#[cfg(target_os = "windows")]
mod windows_impl;

fn main() {
    #[cfg(target_os = "windows")]
    {
        windows_impl::run();
    }

    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("This binary is Windows-only. On macOS, use the macos-ime crate.");
        std::process::exit(1);
    }
}
