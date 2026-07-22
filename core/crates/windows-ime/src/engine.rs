//! Conversion engine: wraps the Rust DB/phonetic decoder for Windows.
//!
//! Same logic as the macOS FFI layer but called directly (no C FFI needed).

use ime_db::phonetic_decoder::{PhoneticMap, BeamDecoder};
use ime_db::kana_hangul::hiragana_to_hangul;
use ime_db::vocab::build_vocab;
use ime_db::DictionaryDb;

/// The core conversion engine.
pub struct ConversionEngine {
    db: DictionaryDb,
    phonetic_map: PhoneticMap,
}

/// A single conversion candidate.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub reading: String,
    pub surface: String,
    pub score: f64,
}

impl ConversionEngine {
    /// Create and initialize the engine.
    pub fn new(db_path: Option<&str>) -> Result<Self, String> {
        let db = match db_path {
            Some(path) => DictionaryDb::open(path).map_err(|e| format!("DB error: {e}"))?,
            None => DictionaryDb::open_in_memory().map_err(|e| format!("DB error: {e}"))?,
        };

        // Build vocabulary and populate dictionary
        let vocab = build_vocab();
        let dict = db.kanji_dict();
        let entries: Vec<(&str, &str, i64)> = vocab
            .iter()
            .map(|v| (v.reading, v.surface, 100i64))
            .collect();
        let _ = dict.insert_batch(&entries);

        // Build phonetic map
        let mut pmap = PhoneticMap::new();
        let pairs: Vec<(String, String, u64)> = vocab
            .iter()
            .map(|v| {
                let hangul = hiragana_to_hangul(v.reading);
                (v.reading.to_string(), hangul, 100u64)
            })
            .collect();
        pmap.build_from_pairs(&pairs);

        Ok(ConversionEngine {
            db,
            phonetic_map: pmap,
        })
    }

    /// Convert hangul to hiragana (top-1 only).
    pub fn to_hiragana(&self, hangul: &str) -> Option<String> {
        if hangul.is_empty() { return None; }
        let decoder = BeamDecoder::new(&self.phonetic_map, 4, 1);
        let candidates = decoder.decode(hangul);
        candidates.into_iter().next().map(|(h, _)| h)
    }

    /// Convert hangul to ranked Japanese candidates.
    pub fn convert(&self, hangul: &str) -> Vec<Candidate> {
        if hangul.is_empty() { return vec![]; }

        let decoder = BeamDecoder::new(&self.phonetic_map, 6, 10);
        let hira_candidates = decoder.decode(hangul);
        let dict = self.db.kanji_dict();
        let mut results: Vec<Candidate> = Vec::new();

        for (hiragana, confidence) in &hira_candidates {
            if let Ok(entries) = dict.lookup(hiragana) {
                for entry in entries.iter().take(5) {
                    results.push(Candidate {
                        reading: hiragana.clone(),
                        surface: entry.surface.clone(),
                        score: confidence * (entry.frequency as f64 / 10000.0).min(1.0),
                    });
                }
            }
            results.push(Candidate {
                reading: hiragana.clone(),
                surface: hiragana.clone(),
                score: confidence * 0.5,
            });
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.dedup_by(|a, b| a.surface == b.surface);
        results.truncate(9);
        results
    }

    /// Get phonetic map size (for diagnostics).
    pub fn phonetic_map_size(&self) -> usize {
        self.phonetic_map.vocab_size()
    }
}
