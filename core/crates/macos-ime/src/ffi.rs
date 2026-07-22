//! C-compatible FFI layer for the Hangul→Japanese conversion engine.
//!
//! This module exposes the Rust IME engine as a C API so that the Swift
//! InputMethodKit application can call into it.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;
use std::sync::Mutex;

use ime_db::phonetic_decoder::{PhoneticMap, BeamDecoder};
use ime_db::kana_hangul::hiragana_to_hangul;
use ime_db::vocab::build_vocab;
use ime_db::vocab_extended::build_extended_vocab;
use ime_db::vocab_large::build_vocab_large;
use ime_db::DictionaryDb;

/// Opaque engine handle passed across the FFI boundary.
pub struct HJEngine {
    db: DictionaryDb,
    phonetic_map: PhoneticMap,
}

static ENGINE: Mutex<Option<HJEngine>> = Mutex::new(None);

// ─── Lifecycle ──────────────────────────────────────────

/// Initialize the engine with a database path.
/// Returns 0 on success, -1 on failure.
#[no_mangle]
pub extern "C" fn hj_engine_init(db_path: *const c_char) -> i32 {
    let path = if db_path.is_null() {
        // Default: in-memory DB for quick startup
        ":memory:".to_string()
    } else {
        match unsafe { CStr::from_ptr(db_path) }.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return -1,
        }
    };

    let db = match DictionaryDb::open(&path) {
        Ok(d) => d,
        Err(_) => return -1,
    };

    // Build vocabulary and populate dictionary.
    // Combines all three vocab tiers (was: build_vocab() alone, ~730 entries)
    // for far better real-world coverage.
    let mut vocab = build_vocab();
    vocab.extend(build_extended_vocab());
    vocab.extend(build_vocab_large());
    let dict = db.kanji_dict();
    let entries: Vec<(&str, &str, i64)> = vocab
        .iter()
        .map(|v| (v.reading, v.surface, 100i64))
        .collect();
    let _ = dict.insert_batch(&entries);

    // Build phonetic map from vocabulary
    let mut pmap = PhoneticMap::new();
    let pairs: Vec<(String, String, u64)> = vocab
        .iter()
        .map(|v| {
            let hangul = hiragana_to_hangul(v.reading);
            (v.reading.to_string(), hangul, 100u64)
        })
        .collect();
    pmap.build_from_pairs(&pairs);

    let mut engine = ENGINE.lock().unwrap();
    *engine = Some(HJEngine { db, phonetic_map: pmap });
    0
}

/// Shut down the engine and free resources.
#[no_mangle]
pub extern "C" fn hj_engine_destroy() {
    let mut engine = ENGINE.lock().unwrap();
    *engine = None;
}

// ─── Conversion ─────────────────────────────────────────

/// Result struct for a single candidate.
#[repr(C)]
pub struct HJCandidate {
    /// Hiragana reading (UTF-8, null-terminated)
    pub reading: *mut c_char,
    /// Surface form (kanji/kana, null-terminated)
    pub surface: *mut c_char,
    /// Confidence score (0.0 – 1.0)
    pub score: f64,
}

/// Result list from conversion.
#[repr(C)]
pub struct HJCandidateList {
    pub candidates: *mut HJCandidate,
    pub count: usize,
}

/// Convert hangul input to Japanese candidates.
/// The caller MUST call hj_candidates_free() on the result.
#[no_mangle]
pub extern "C" fn hj_convert(hangul: *const c_char) -> HJCandidateList {
    let empty = HJCandidateList {
        candidates: ptr::null_mut(),
        count: 0,
    };

    if hangul.is_null() {
        return empty;
    }

    let hangul_str = match unsafe { CStr::from_ptr(hangul) }.to_str() {
        Ok(s) => s,
        Err(_) => return empty,
    };

    if hangul_str.is_empty() {
        return empty;
    }

    let engine = ENGINE.lock().unwrap();
    let eng = match engine.as_ref() {
        Some(e) => e,
        None => return empty,
    };

    // Step 1: Beam decode hangul → hiragana candidates
    let decoder = BeamDecoder::new(&eng.phonetic_map, 6, 10);
    let hira_candidates = decoder.decode(hangul_str);

    if hira_candidates.is_empty() {
        return empty;
    }

    // Step 2: For each hiragana candidate, look up kanji dictionary
    let dict = eng.db.kanji_dict();
    let mut results: Vec<(String, String, f64)> = Vec::new();

    for (hiragana, confidence) in &hira_candidates {
        // Look up kanji surface forms
        if let Ok(entries) = dict.lookup(hiragana) {
            for entry in entries.iter().take(5) {
                results.push((
                    hiragana.clone(),
                    entry.surface.clone(),
                    confidence * (entry.frequency as f64 / 10000.0).min(1.0),
                ));
            }
        }
        // Also add hiragana itself as a candidate
        results.push((hiragana.clone(), hiragana.clone(), confidence * 0.5));
    }

    // Sort by score descending and deduplicate by surface
    results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    results.dedup_by(|a, b| a.1 == b.1);
    results.truncate(9);

    // Convert to C structs
    let count = results.len();
    let mut c_candidates: Vec<HJCandidate> = Vec::with_capacity(count);

    for (reading, surface, score) in results {
        let c_reading = CString::new(reading).unwrap_or_default();
        let c_surface = CString::new(surface).unwrap_or_default();
        c_candidates.push(HJCandidate {
            reading: c_reading.into_raw(),
            surface: c_surface.into_raw(),
            score,
        });
    }

    let ptr = c_candidates.as_mut_ptr();
    std::mem::forget(c_candidates);

    HJCandidateList {
        candidates: ptr,
        count,
    }
}

/// Free a candidate list returned by hj_convert().
#[no_mangle]
pub extern "C" fn hj_candidates_free(list: HJCandidateList) {
    if list.candidates.is_null() || list.count == 0 {
        return;
    }
    unsafe {
        let candidates = Vec::from_raw_parts(list.candidates, list.count, list.count);
        for c in candidates {
            if !c.reading.is_null() {
                let _ = CString::from_raw(c.reading);
            }
            if !c.surface.is_null() {
                let _ = CString::from_raw(c.surface);
            }
        }
    }
}

/// Quick hangul-to-hiragana conversion (no dictionary lookup).
/// Returns a newly allocated C string. Caller must call hj_string_free().
#[no_mangle]
pub extern "C" fn hj_hangul_to_hiragana(hangul: *const c_char) -> *mut c_char {
    if hangul.is_null() {
        return ptr::null_mut();
    }
    let hangul_str = match unsafe { CStr::from_ptr(hangul) }.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    let engine = ENGINE.lock().unwrap();
    let eng = match engine.as_ref() {
        Some(e) => e,
        None => return ptr::null_mut(),
    };

    let decoder = BeamDecoder::new(&eng.phonetic_map, 4, 1);
    let hiragana = decoder.decode_sentence(hangul_str);

    if hiragana.is_empty() {
        ptr::null_mut()
    } else {
        CString::new(hiragana).unwrap_or_default().into_raw()
    }
}

/// Free a string returned by hj_hangul_to_hiragana().
#[no_mangle]
pub extern "C" fn hj_string_free(s: *mut c_char) {
    if !s.is_null() {
        unsafe { let _ = CString::from_raw(s); }
    }
}

/// Get the number of entries in the phonetic map (for diagnostics).
#[no_mangle]
pub extern "C" fn hj_phonetic_map_size() -> usize {
    let engine = ENGINE.lock().unwrap();
    match engine.as_ref() {
        Some(e) => e.phonetic_map.vocab_size(),
        None => 0,
    }
}
