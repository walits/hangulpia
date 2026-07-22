//
//  HJEngine.swift
//  HangulJapaneseIME
//
//  Swift wrapper around the Rust C FFI engine.
//

import Foundation

/// A single conversion candidate from the Rust engine.
struct Candidate {
    let reading: String   // Hiragana
    let surface: String   // Kanji/kana display form
    let score: Double
}

/// Swift wrapper for the Rust Hangul→Japanese engine.
final class HJEngine {
    static let shared = HJEngine()

    private var initialized = false

    private init() {}

    /// Initialize the engine with an optional DB path.
    /// Call once at app startup.
    func initialize(dbPath: String? = nil) {
        guard !initialized else { return }

        let result: Int32
        if let path = dbPath {
            result = path.withCString { hj_engine_init($0) }
        } else {
            result = hj_engine_init(nil)
        }

        if result == 0 {
            initialized = true
            let mapSize = hj_phonetic_map_size()
            NSLog("[HJEngine] Initialized. PhoneticMap size: \(mapSize)")
        } else {
            NSLog("[HJEngine] Failed to initialize engine")
        }
    }

    /// Shut down the engine.
    func shutdown() {
        guard initialized else { return }
        hj_engine_destroy()
        initialized = false
    }

    /// Convert hangul input to Japanese candidates.
    func convert(hangul: String) -> [Candidate] {
        guard initialized else { return [] }

        let list = hangul.withCString { hj_convert($0) }
        defer { hj_candidates_free(list) }

        guard list.count > 0, let ptr = list.candidates else { return [] }

        var results: [Candidate] = []
        for i in 0..<list.count {
            let c = ptr[i]
            let reading = c.reading.map { String(cString: $0) } ?? ""
            let surface = c.surface.map { String(cString: $0) } ?? ""
            results.append(Candidate(reading: reading, surface: surface, score: c.score))
        }
        return results
    }

    /// Quick hangul→hiragana conversion (top-1 only).
    func toHiragana(hangul: String) -> String? {
        guard initialized else { return nil }

        let ptr = hangul.withCString { hj_hangul_to_hiragana($0) }
        guard let p = ptr else { return nil }
        defer { hj_string_free(p) }
        return String(cString: p)
    }
}
