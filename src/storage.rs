use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DocumentInfo {
    pub id: String,
    pub filename: String,
    pub file_path: String,
    pub file_type: String,
    pub file_data: Vec<u8>,
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FragmentInfo {
    pub id: String,
    pub document_id: String,
    pub fragment_order: i32,
    pub content: String,
    pub embedding: Option<Vec<f64>>,
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetaInfo {
    pub version: String,
    pub embedding_model: String,
}

#[derive(Debug, Clone)]
pub enum StorageBackend {
    DuckDB,
    LanceDB,
}

impl StorageBackend {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "duckdb" => Some(StorageBackend::DuckDB),
            "lancedb" => Some(StorageBackend::LanceDB),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            StorageBackend::DuckDB => "duckdb",
            StorageBackend::LanceDB => "lancedb",
        }
    }
}

/// Abstract storage interface for different backend implementations
#[async_trait]
pub trait Storage: Send {
    /// Initialize the storage backend
    async fn initialize(&mut self) -> Result<()>;

    /// Verify or set the embedding model
    async fn verify_or_set_model(&mut self, model_name: &str) -> Result<()>;

    /// Check if a document already exists
    async fn document_exists(&mut self, file_path: &Path) -> Result<bool>;

    /// Store a document and return its ID
    async fn store_document(&mut self, file_path: &Path, file_data: &[u8]) -> Result<String>;

    /// Store a text fragment without embedding initially
    async fn store_text_fragment(
        &mut self,
        document_id: &str,
        order: i32,
        content: &str,
    ) -> Result<String>;

    /// Update fragment with embedding
    async fn update_fragment_embedding(
        &mut self,
        fragment_id: &str,
        embedding: &[f64],
    ) -> Result<()>;

    /// Get fragments without embeddings for batch processing
    async fn get_fragments_without_embeddings(&mut self, limit: i32) -> Result<Vec<(String, String)>>;

    /// Count fragments without embeddings
    async fn count_fragments_without_embeddings(&mut self) -> Result<i32>;

    /// Get metadata information
    async fn get_meta_info(&mut self) -> Result<MetaInfo>;

    /// Search for similar documents using vector similarity
    async fn search_similar(
        &mut self,
        query_embedding: &[f64],
        limit: usize,
    ) -> Result<Vec<(String, String, f64)>>; // (fragment_id, content, similarity_score)
}