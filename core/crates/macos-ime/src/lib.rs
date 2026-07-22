//! macOS InputMethodKit bridge.
//!
//! This crate provides two layers:
//! 1. `ffi` — C-compatible API for the Rust conversion engine
//! 2. The Swift InputMethodKit app calls into `ffi` functions

pub mod ffi;
