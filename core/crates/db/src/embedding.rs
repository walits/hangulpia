//! Token embedding vector storage and retrieval.
//!
//! Stores word2vec-style embedding vectors in SQLite as BLOBs.
//! Vectors represent word co-occurrence distributions at the sentence level,
//! enabling context-aware candidate disambiguation.

use rusqlite::{params, Connection, Result as SqlResult};
use std::collections::HashMap;

/// Dimension of embedding vectors (configurable at build time).
pub const DEFAULT_EMBEDDING_DIM: usize = 64;

/// A single embedding vector for a word.
#[derive(Debug, Clone)]
pub struct WordEmbedding {
    pub word: String,
    pub vector: Vec<f32>,
}

impl WordEmbedding {
    pub fn new(word: String, vector: Vec<f32>) -> Self {
        Self { word, vector }
    }

    /// Serialize the f32 vector to bytes for SQLite BLOB storage.
    pub fn vector_to_bytes(vector: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(vector.len() * 4);
        for &v in vector {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes
    }

    /// Deserialize bytes from SQLite BLOB back to f32 vector.
    pub fn bytes_to_vector(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|chunk| {
                let arr: [u8; 4] = [chunk[0], chunk[1], chunk[2], chunk[3]];
                f32::from_le_bytes(arr)
            })
            .collect()
    }
}

/// Compute cosine similarity between two vectors.
/// Returns a value in [-1.0, 1.0], where 1.0 means identical direction.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    assert_eq!(a.len(), b.len(), "Vector dimensions must match");
    if a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;

    for (&ai, &bi) in a.iter().zip(b.iter()) {
        let ai = ai as f64;
        let bi = bi as f64;
        dot += ai * bi;
        norm_a += ai * ai;
        norm_b += bi * bi;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < 1e-10 {
        0.0
    } else {
        dot / denom
    }
}

/// Compute the average (centroid) of multiple embedding vectors.
/// Used to create a "context vector" from surrounding words.
pub fn average_vectors(vectors: &[Vec<f32>]) -> Vec<f32> {
    if vectors.is_empty() {
        return Vec::new();
    }

    let dim = vectors[0].len();
    let mut avg = vec![0.0_f32; dim];
    let n = vectors.len() as f32;

    for v in vectors {
        for (i, &val) in v.iter().enumerate() {
            avg[i] += val;
        }
    }

    for val in &mut avg {
        *val /= n;
    }

    avg
}

/// Weighted average of vectors where closer context words have higher weight.
/// `weights` should correspond 1:1 with `vectors`.
pub fn weighted_average_vectors(vectors: &[Vec<f32>], weights: &[f32]) -> Vec<f32> {
    if vectors.is_empty() {
        return Vec::new();
    }

    let dim = vectors[0].len();
    let mut avg = vec![0.0_f32; dim];
    let total_weight: f32 = weights.iter().sum();

    if total_weight < 1e-10 {
        return avg;
    }

    for (v, &w) in vectors.iter().zip(weights.iter()) {
        for (i, &val) in v.iter().enumerate() {
            avg[i] += val * w;
        }
    }

    for val in &mut avg {
        *val /= total_weight;
    }

    avg
}

/// Manages embedding vector storage in SQLite.
pub struct EmbeddingStore<'a> {
    conn: &'a Connection,
}

impl<'a> EmbeddingStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Create the embedding tables.
    pub fn init_tables(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS word_embeddings (
                word TEXT PRIMARY KEY,
                vector BLOB NOT NULL,
                dim INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS embedding_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
    }

    /// Store an embedding vector for a word.
    pub fn store_embedding(&self, word: &str, vector: &[f32]) -> SqlResult<()> {
        let bytes = WordEmbedding::vector_to_bytes(vector);
        self.conn.execute(
            "INSERT OR REPLACE INTO word_embeddings (word, vector, dim) VALUES (?1, ?2, ?3)",
            params![word, bytes, vector.len() as i64],
        )?;
        Ok(())
    }

    /// Store multiple embeddings in a single transaction.
    pub fn store_embeddings_batch(&self, embeddings: &[(&str, &[f32])]) -> SqlResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO word_embeddings (word, vector, dim) VALUES (?1, ?2, ?3)",
            )?;
            for &(word, vector) in embeddings {
                let bytes = WordEmbedding::vector_to_bytes(vector);
                stmt.execute(params![word, bytes, vector.len() as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Retrieve the embedding vector for a word.
    pub fn get_embedding(&self, word: &str) -> SqlResult<Option<Vec<f32>>> {
        let mut stmt = self
            .conn
            .prepare("SELECT vector FROM word_embeddings WHERE word = ?1")?;
        let result = stmt.query_row(params![word], |row| {
            let bytes: Vec<u8> = row.get(0)?;
            Ok(WordEmbedding::bytes_to_vector(&bytes))
        });

        match result {
            Ok(vec) => Ok(Some(vec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get embeddings for multiple words at once.
    pub fn get_embeddings_batch(&self, words: &[&str]) -> SqlResult<HashMap<String, Vec<f32>>> {
        let mut result = HashMap::new();
        let mut stmt = self
            .conn
            .prepare("SELECT word, vector FROM word_embeddings WHERE word = ?1")?;

        for &word in words {
            if let Ok(row) = stmt.query_row(params![word], |row| {
                let w: String = row.get(0)?;
                let bytes: Vec<u8> = row.get(1)?;
                Ok((w, WordEmbedding::bytes_to_vector(&bytes)))
            }) {
                result.insert(row.0, row.1);
            }
        }

        Ok(result)
    }

    /// Set metadata (e.g., embedding dimension, training info).
    pub fn set_meta(&self, key: &str, value: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO embedding_meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// Get metadata value.
    pub fn get_meta(&self, key: &str) -> SqlResult<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM embedding_meta WHERE key = ?1")?;
        match stmt.query_row(params![key], |row| row.get(0)) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Count total embeddings stored.
    pub fn count(&self) -> SqlResult<usize> {
        let mut stmt = self
            .conn
            .prepare("SELECT COUNT(*) FROM word_embeddings")?;
        stmt.query_row([], |row| {
            let count: i64 = row.get(0)?;
            Ok(count as usize)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        let store = EmbeddingStore::new(&conn);
        store.init_tables().unwrap();
        conn
    }

    #[test]
    fn test_vector_serialization_roundtrip() {
        let original = vec![1.0_f32, -0.5, 0.0, 3.14, -2.718];
        let bytes = WordEmbedding::vector_to_bytes(&original);
        let recovered = WordEmbedding::bytes_to_vector(&bytes);
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_average_vectors() {
        let vecs = vec![vec![1.0, 2.0, 3.0], vec![3.0, 4.0, 5.0]];
        let avg = average_vectors(&vecs);
        assert_eq!(avg, vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_weighted_average_vectors() {
        let vecs = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        let weights = vec![3.0, 1.0];
        let avg = weighted_average_vectors(&vecs, &weights);
        assert!((avg[0] - 0.75).abs() < 1e-6);
        assert!((avg[1] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn test_store_and_retrieve_embedding() {
        let conn = setup_db();
        let store = EmbeddingStore::new(&conn);

        let vec = vec![0.1, 0.2, 0.3, 0.4];
        store.store_embedding("さくら", &vec).unwrap();

        let retrieved = store.get_embedding("さくら").unwrap().unwrap();
        assert_eq!(retrieved, vec);
    }

    #[test]
    fn test_get_missing_embedding() {
        let conn = setup_db();
        let store = EmbeddingStore::new(&conn);

        let result = store.get_embedding("存在しない").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_batch_store_and_retrieve() {
        let conn = setup_db();
        let store = EmbeddingStore::new(&conn);

        let v1 = vec![1.0, 0.0];
        let v2 = vec![0.0, 1.0];
        store
            .store_embeddings_batch(&[("桜", &v1), ("花", &v2)])
            .unwrap();

        let result = store.get_embeddings_batch(&["桜", "花"]).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result["桜"], vec![1.0, 0.0]);
        assert_eq!(result["花"], vec![0.0, 1.0]);
    }

    #[test]
    fn test_metadata() {
        let conn = setup_db();
        let store = EmbeddingStore::new(&conn);

        store.set_meta("dim", "64").unwrap();
        store.set_meta("source", "jawiki-word2vec").unwrap();

        assert_eq!(store.get_meta("dim").unwrap().unwrap(), "64");
        assert_eq!(store.get_meta("source").unwrap().unwrap(), "jawiki-word2vec");
        assert!(store.get_meta("missing").unwrap().is_none());
    }

    #[test]
    fn test_count() {
        let conn = setup_db();
        let store = EmbeddingStore::new(&conn);

        assert_eq!(store.count().unwrap(), 0);
        store.store_embedding("a", &[1.0]).unwrap();
        store.store_embedding("b", &[2.0]).unwrap();
        assert_eq!(store.count().unwrap(), 2);
    }
}
