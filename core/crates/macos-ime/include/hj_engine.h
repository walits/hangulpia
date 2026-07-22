//
//  hj_engine.h
//  Hangul→Japanese conversion engine C API
//
//  This header exposes the Rust IME engine for use from Swift/Objective-C.
//

#ifndef HJ_ENGINE_H
#define HJ_ENGINE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// ─── Lifecycle ──────────────────────────────────────────

/// Initialize the engine. Pass NULL for in-memory database.
/// Returns 0 on success, -1 on failure.
int32_t hj_engine_init(const char *db_path);

/// Shut down the engine and free all resources.
void hj_engine_destroy(void);

// ─── Conversion ─────────────────────────────────────────

/// A single conversion candidate.
typedef struct {
    char *reading;   // Hiragana reading (UTF-8)
    char *surface;   // Surface form — kanji or kana (UTF-8)
    double score;    // Confidence score (0.0 – 1.0)
} HJCandidate;

/// A list of candidates returned by hj_convert().
typedef struct {
    HJCandidate *candidates;
    size_t count;
} HJCandidateList;

/// Convert hangul input to Japanese candidates.
/// Caller MUST free the result with hj_candidates_free().
HJCandidateList hj_convert(const char *hangul);

/// Free a candidate list.
void hj_candidates_free(HJCandidateList list);

// ─── Utilities ──────────────────────────────────────────

/// Quick hangul→hiragana (top-1 only, no dictionary).
/// Caller MUST free with hj_string_free().
char *hj_hangul_to_hiragana(const char *hangul);

/// Free a string returned by hj_hangul_to_hiragana().
void hj_string_free(char *s);

/// Get the phonetic map size (for diagnostics).
size_t hj_phonetic_map_size(void);

#ifdef __cplusplus
}
#endif

#endif // HJ_ENGINE_H
