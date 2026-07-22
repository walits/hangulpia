//! SQLite-based dictionary, embedding vector, and user preference storage.
//!
//! This crate provides:
//! - Basic dictionary lookup (reading→surface)
//! - Kanji dictionary with frequency data
//! - Token embedding vector storage for sentence-level disambiguation
//! - Context-aware candidate ranking using cosine similarity

pub mod autocomplete;
pub mod corpus;
pub mod dictionary;
pub mod embedding;
pub mod generator;
pub mod kana_hangul;
pub mod sentence;
pub mod trainer;
pub mod ngram;
pub mod phonetic_decoder;
pub mod vocab;
pub mod vocab_extended;
pub mod vocab_large;

use rusqlite::{Connection, Result as SqlResult};
use thiserror::Error;

pub use dictionary::{ContextRanker, DirectionalContextRanker, DictEntry, KanjiDict, RankedCandidate};
pub use embedding::{
    cosine_similarity, average_vectors, weighted_average_vectors,
    EmbeddingStore, WordEmbedding, DEFAULT_EMBEDDING_DIM,
};
pub use sentence::{Segment, SentenceBuffer};
pub use trainer::{DbBuilder, TrainerConfig, BuildStats};

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Dictionary not found: {0}")]
    DictionaryNotFound(String),
    #[error("Embedding dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },
}

/// Manages the local SQLite database for dictionaries and user data.
pub struct DictionaryDb {
    conn: Connection,
}

impl DictionaryDb {
    /// Open (or create) a database at the given path.
    pub fn open(path: &str) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_all_tables()?;
        Ok(db)
    }

    /// Open an in-memory database (useful for testing).
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init_all_tables()?;
        Ok(db)
    }

    /// Initialize all tables.
    fn init_all_tables(&self) -> Result<(), DbError> {
        self.init_legacy_tables()?;
        let embed_store = EmbeddingStore::new(&self.conn);
        embed_store.init_tables()?;
        let kanji_dict = KanjiDict::new(&self.conn);
        kanji_dict.init_tables()?;
        Ok(())
    }

    fn init_legacy_tables(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS dictionary (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                reading TEXT NOT NULL,
                surface TEXT NOT NULL,
                frequency INTEGER DEFAULT 0,
                language TEXT NOT NULL CHECK(language IN ('korean', 'japanese'))
            );
            CREATE INDEX IF NOT EXISTS idx_reading ON dictionary(reading);

            CREATE TABLE IF NOT EXISTS user_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                input TEXT NOT NULL,
                selected TEXT NOT NULL,
                count INTEGER DEFAULT 1,
                last_used DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_user_input ON user_history(input);
            ",
        )?;
        Ok(())
    }

    /// Get a reference to the underlying connection (for embedding/dict operations).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Get an EmbeddingStore bound to this database.
    pub fn embedding_store(&self) -> EmbeddingStore<'_> {
        EmbeddingStore::new(&self.conn)
    }

    /// Get a KanjiDict bound to this database.
    pub fn kanji_dict(&self) -> KanjiDict<'_> {
        KanjiDict::new(&self.conn)
    }

    /// Get a ContextRanker bound to this database.
    pub fn context_ranker(&self) -> ContextRanker<'_> {
        ContextRanker::new(&self.conn)
    }

    /// Get a DirectionalContextRanker bound to this database.
    pub fn directional_ranker(&self) -> DirectionalContextRanker<'_> {
        DirectionalContextRanker::new(&self.conn)
    }

    /// Look up candidates for the given reading (legacy API).
    pub fn lookup(&self, reading: &str, language: &str) -> Result<Vec<String>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT surface FROM dictionary WHERE reading = ?1 AND language = ?2 ORDER BY frequency DESC",
        )?;
        let results = stmt
            .query_map([reading, language], |row| row.get(0))?
            .collect::<SqlResult<Vec<String>>>()?;
        Ok(results)
    }

    /// Record a user selection for adaptive learning.
    pub fn record_selection(&self, input: &str, selected: &str) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO user_history (input, selected, count, last_used)
             VALUES (?1, ?2, 1, CURRENT_TIMESTAMP)
             ON CONFLICT(id) DO UPDATE SET
                count = count + 1,
                last_used = CURRENT_TIMESTAMP",
            rusqlite::params![input, selected],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory() {
        let db = DictionaryDb::open_in_memory();
        assert!(db.is_ok());
    }

    #[test]
    fn lookup_empty() {
        let db = DictionaryDb::open_in_memory().unwrap();
        let results = db.lookup("test", "japanese").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn embedding_store_via_db() {
        let db = DictionaryDb::open_in_memory().unwrap();
        let store = db.embedding_store();
        store.store_embedding("テスト", &[1.0, 2.0, 3.0]).unwrap();
        let emb = store.get_embedding("テスト").unwrap().unwrap();
        assert_eq!(emb, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn kanji_dict_via_db() {
        let db = DictionaryDb::open_in_memory().unwrap();
        let dict = db.kanji_dict();
        dict.insert("てすと", "テスト", 100).unwrap();
        let results = dict.lookup("てすと").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].surface, "テスト");
    }

    #[test]
    fn context_ranker_via_db() {
        let db = DictionaryDb::open_in_memory().unwrap();
        let dict = db.kanji_dict();
        dict.insert("はな", "花", 5000).unwrap();

        let ranker = db.context_ranker();
        let candidates = vec![("はな".to_string(), "hana".to_string(), 0.9)];
        let ranked = ranker.rank_candidates(&candidates, &[], 5000);
        assert!(!ranked.is_empty());
    }
}
