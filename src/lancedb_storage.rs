use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::Path;
use uuid::Uuid;
use log::{info, warn};
use chrono;

use crate::storage::{Storage, MetaInfo};

const DB_VERSION: &str = "1.0.0";

pub struct LanceDBStorage {
    db_path: String,
    // Store metadata in memory for now - in production this would use LanceDB
    metadata: std::collections::HashMap<String, String>,
    documents: std::collections::HashMap<String, (String, Vec<u8>)>, // id -> (path, data)
    fragments: std::collections::HashMap<String, (String, i32, String)>, // id -> (doc_id, order, content)
    embeddings: std::collections::HashMap<String, Vec<f32>>, // fragment_id -> embedding_vector
}

impl LanceDBStorage {
    pub async fn new(db_path: &Path) -> Result<Self> {
        // Convert path to string and ensure it ends with .lancedb
        let db_path_str = if db_path.extension().map(|e| e.to_string_lossy()) == Some("lancedb".into()) {
            db_path.to_string_lossy().to_string()
        } else {
            format!("{}.lancedb", db_path.to_string_lossy())
        };

        warn!("LanceDB storage is currently using a in-memory stub implementation.");
        warn!("This is for demonstration purposes. Production use requires full LanceDB integration.");
        
        let mut storage = LanceDBStorage {
            db_path: db_path_str,
            metadata: std::collections::HashMap::new(),
            documents: std::collections::HashMap::new(),
            fragments: std::collections::HashMap::new(),
            embeddings: std::collections::HashMap::new(),
        };
        
        storage.initialize().await?;
        
        Ok(storage)
    }

    fn current_timestamp() -> String {
        chrono::Utc::now().to_rfc3339()
    }
}

#[async_trait]
impl Storage for LanceDBStorage {
    async fn initialize(&mut self) -> Result<()> {
        // Initialize metadata with default values
        self.metadata.insert("version".to_string(), DB_VERSION.to_string());
        
        info!("LanceDB storage initialized (in-memory stub)");
        Ok(())
    }

    async fn verify_or_set_model(&mut self, model_name: &str) -> Result<()> {
        // Check version
        if let Some(existing_version) = self.metadata.get("version") {
            if existing_version != DB_VERSION {
                anyhow::bail!(
                    "Database version mismatch. Expected: {}, Found: {}",
                    DB_VERSION, existing_version
                );
            }
        } else {
            self.metadata.insert("version".to_string(), DB_VERSION.to_string());
            info!("Set database version to {}", DB_VERSION);
        }

        // Check/set model
        if let Some(existing_model) = self.metadata.get("embedding_model") {
            if existing_model != model_name {
                anyhow::bail!(
                    "Embedding model mismatch. Expected: {}, Found: {}",
                    model_name, existing_model
                );
            }
            info!("Verified embedding model: {}", model_name);
        } else {
            self.metadata.insert("embedding_model".to_string(), model_name.to_string());
            info!("Set embedding model to {}", model_name);
        }

        Ok(())
    }

    async fn document_exists(&mut self, file_path: &Path) -> Result<bool> {
        let path_str = file_path.to_string_lossy();
        Ok(self.documents.values().any(|(path, _)| path == &path_str))
    }

    async fn store_document(&mut self, file_path: &Path, file_data: &[u8]) -> Result<String> {
        let document_id = Uuid::new_v4().to_string();
        let path_str = file_path.to_string_lossy().to_string();
        
        self.documents.insert(document_id.clone(), (path_str, file_data.to_vec()));
        
        Ok(document_id)
    }

    async fn store_text_fragment(
        &mut self,
        document_id: &str,
        order: i32,
        content: &str,
    ) -> Result<String> {
        let fragment_id = Uuid::new_v4().to_string();
        
        self.fragments.insert(
            fragment_id.clone(), 
            (document_id.to_string(), order, content.to_string())
        );
        
        Ok(fragment_id)
    }

    async fn update_fragment_embedding(
        &mut self,
        fragment_id: &str,
        embedding: &[f64],
    ) -> Result<()> {
        // Store the embedding in our in-memory HashMap
        let embedding_f32: Vec<f32> = embedding.iter().map(|&x| x as f32).collect();
        self.embeddings.insert(fragment_id.to_string(), embedding_f32);
        Ok(())
    }

    async fn get_fragments_without_embeddings(&mut self, limit: i32) -> Result<Vec<(String, String)>> {
        // Return only fragments that don't have embeddings yet
        let fragments: Vec<(String, String)> = self.fragments
            .iter()
            .filter(|(id, _)| !self.embeddings.contains_key(*id))
            .take(limit as usize)
            .map(|(id, (_, _, content))| (id.clone(), content.clone()))
            .collect();
            
        Ok(fragments)
    }

    async fn count_fragments_without_embeddings(&mut self) -> Result<i32> {
        // Count only fragments that don't have embeddings yet
        let count = self.fragments
            .iter()
            .filter(|(id, _)| !self.embeddings.contains_key(*id))
            .count();
        Ok(count as i32)
    }

    async fn get_meta_info(&mut self) -> Result<MetaInfo> {
        let version = self.metadata.get("version").unwrap_or(&"unknown".to_string()).clone();
        let embedding_model = self.metadata.get("embedding_model").unwrap_or(&"unknown".to_string()).clone();

        Ok(MetaInfo {
            version,
            embedding_model,
        })
    }

    async fn search_similar(
        &mut self,
        _query_embedding: &[f64],
        limit: usize,
    ) -> Result<Vec<(String, String, f64)>> {
        // In stub implementation, return fragments with dummy similarity scores
        let results: Vec<(String, String, f64)> = self.fragments
            .iter()
            .take(limit)
            .enumerate()
            .map(|(i, (id, (_, _, content)))| {
                // Dummy similarity score that decreases with index
                let similarity = 1.0 - (i as f64 * 0.1);
                (id.clone(), content.clone(), similarity.max(0.0))
            })
            .collect();
            
        Ok(results)
    }
}