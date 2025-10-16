use anyhow::{Context, Result};
use duckdb::{Connection, params};
use log::info;
use std::path::Path;
use uuid::Uuid;
use async_trait::async_trait;

use crate::storage::{Storage, MetaInfo};

const DB_VERSION: &str = "1.0.0";

pub struct DuckDBStorage {
    conn: Connection,
}

impl DuckDBStorage {
    pub async fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)
            .context("Failed to open DuckDB connection")?;
        
        let mut storage = DuckDBStorage { conn };
        storage.initialize().await?;
        
        Ok(storage)
    }
    
    async fn initialize_tables(&mut self) -> Result<()> {
        // Create meta table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS meta (
                key VARCHAR PRIMARY KEY,
                value VARCHAR NOT NULL
            )",
            [],
        ).context("Failed to create meta table")?;
        
        // Create documents table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS documents (
                id VARCHAR PRIMARY KEY,
                filename VARCHAR NOT NULL,
                file_path VARCHAR NOT NULL,
                file_type VARCHAR NOT NULL,
                file_data BLOB NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(file_path)
            )",
            [],
        ).context("Failed to create documents table")?;
        
        // Add file_type column if it doesn't exist (for existing databases)
        let _ = self.conn.execute(
            "ALTER TABLE documents ADD COLUMN file_type VARCHAR",
            [],
        );
        
        // Rename pdf_data column to file_data if needed (for existing databases)
        let _ = self.conn.execute(
            "ALTER TABLE documents RENAME COLUMN pdf_data TO file_data",
            [],
        );
        
        // Create fragments table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS fragments (
                id VARCHAR PRIMARY KEY,
                document_id VARCHAR NOT NULL,
                fragment_order INTEGER NOT NULL,
                content TEXT NOT NULL,
                embedding DOUBLE[],
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (document_id) REFERENCES documents(id)
            )",
            [],
        ).context("Failed to create fragments table")?;
        
        // Create index on document_id and fragment_order
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_fragments_doc_order 
             ON fragments(document_id, fragment_order)",
            [],
        ).context("Failed to create fragments index")?;
        
        info!("DuckDB tables initialized successfully");
        Ok(())
    }
}

#[async_trait]
impl Storage for DuckDBStorage {
    async fn initialize(&mut self) -> Result<()> {
        self.initialize_tables().await
    }

    async fn verify_or_set_model(&mut self, model_name: &str) -> Result<()> {
        // Check if version and model are already set
        let mut stmt = self.conn.prepare(
            "SELECT value FROM meta WHERE key = ?"
        )?;
        
        // Check version
        let version_result: Result<String, _> = stmt.query_row(params!["version"], |row| {
            Ok(row.get(0)?)
        });
        
        match version_result {
            Ok(existing_version) => {
                if existing_version != DB_VERSION {
                    anyhow::bail!(
                        "Database version mismatch. Expected: {}, Found: {}",
                        DB_VERSION, existing_version
                    );
                }
            }
            Err(_) => {
                // Version not set, initialize it
                self.conn.execute(
                    "INSERT INTO meta (key, value) VALUES (?, ?)",
                    params!["version", DB_VERSION],
                )?;
                info!("Set database version to {}", DB_VERSION);
            }
        }
        
        // Check embedding model
        let model_result: Result<String, _> = stmt.query_row(params!["embedding_model"], |row| {
            Ok(row.get(0)?)
        });
        
        match model_result {
            Ok(existing_model) => {
                if existing_model != model_name {
                    anyhow::bail!(
                        "Embedding model mismatch. Expected: {}, Found: {}",
                        model_name, existing_model
                    );
                }
                info!("Verified embedding model: {}", model_name);
            }
            Err(_) => {
                // Model not set, initialize it
                self.conn.execute(
                    "INSERT INTO meta (key, value) VALUES (?, ?)",
                    params!["embedding_model", model_name],
                )?;
                info!("Set embedding model to {}", model_name);
            }
        }
        
        Ok(())
    }

    async fn document_exists(&mut self, file_path: &Path) -> Result<bool> {
        let path_str = file_path.to_string_lossy();
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*) FROM documents WHERE file_path = ?"
        )?;
        
        let count: i64 = stmt.query_row(params![path_str.as_ref()], |row| {
            Ok(row.get(0)?)
        })?;
        
        Ok(count > 0)
    }

    async fn store_document(&mut self, file_path: &Path, file_data: &[u8]) -> Result<String> {
        let document_id = Uuid::new_v4().to_string();
        let filename = file_path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");
        let path_str = file_path.to_string_lossy();
        
        // Determine file type from extension
        let file_type = file_path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("unknown")
            .to_lowercase();
        
        self.conn.execute(
            "INSERT INTO documents (id, filename, file_path, file_type, file_data) VALUES (?, ?, ?, ?, ?)",
            params![&document_id, filename, path_str.as_ref(), &file_type, file_data],
        ).context("Failed to store document")?;
        
        Ok(document_id)
    }

    async fn store_text_fragment(
        &mut self,
        document_id: &str,
        order: i32,
        content: &str,
    ) -> Result<String> {
        let fragment_id = Uuid::new_v4().to_string();
        
        self.conn.execute(
            "INSERT INTO fragments (id, document_id, fragment_order, content) 
             VALUES (?, ?, ?, ?)",
            params![&fragment_id, document_id, order, content],
        ).context("Failed to store text fragment")?;
        
        Ok(fragment_id)
    }

    async fn update_fragment_embedding(
        &mut self,
        fragment_id: &str,
        embedding: &[f64],
    ) -> Result<()> {
        // Convert embedding to JSON for DuckDB storage
        let embedding_json = serde_json::to_string(embedding)
            .context("Failed to serialize embedding")?;
        
        self.conn.execute(
            "UPDATE fragments SET embedding = CAST(? AS DOUBLE[]) WHERE id = ?",
            params![embedding_json, fragment_id],
        ).context("Failed to update fragment embedding")?;
        
        Ok(())
    }

    async fn get_fragments_without_embeddings(&mut self, limit: i32) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content FROM fragments 
             WHERE embedding IS NULL 
             ORDER BY document_id, fragment_order 
             LIMIT ?"
        )?;
        
        let rows = stmt.query_map(params![limit], |row| {
            Ok((
                row.get::<_, String>(0)?,  // id
                row.get::<_, String>(1)?,  // content
            ))
        })?;
        
        let mut fragments = Vec::new();
        for row in rows {
            fragments.push(row?);
        }
        
        Ok(fragments)
    }

    async fn count_fragments_without_embeddings(&mut self) -> Result<i32> {
        let mut stmt = self.conn.prepare("SELECT COUNT(*) FROM fragments WHERE embedding IS NULL")?;
        
        let count: i64 = stmt.query_row([], |row| {
            Ok(row.get(0)?)
        })?;
        
        Ok(count as i32)
    }

    async fn get_meta_info(&mut self) -> Result<MetaInfo> {
        let mut stmt = self.conn.prepare(
            "SELECT key, value FROM meta WHERE key IN ('version', 'embedding_model')"
        )?;
        
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        
        let mut version = None;
        let mut embedding_model = None;
        
        for row in rows {
            let (key, value) = row?;
            match key.as_str() {
                "version" => version = Some(value),
                "embedding_model" => embedding_model = Some(value),
                _ => {}
            }
        }
        
        Ok(MetaInfo {
            version: version.unwrap_or_else(|| "unknown".to_string()),
            embedding_model: embedding_model.unwrap_or_else(|| "unknown".to_string()),
        })
    }

    async fn search_similar(
        &mut self,
        query_embedding: &[f64],
        limit: usize,
    ) -> Result<Vec<(String, String, f64)>> {
        // Convert query embedding to JSON for DuckDB
        let query_json = serde_json::to_string(query_embedding)
            .context("Failed to serialize query embedding")?;
        
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, content, array_cosine_similarity(embedding, CAST(? AS DOUBLE[])) AS similarity 
             FROM fragments 
             WHERE embedding IS NOT NULL 
             ORDER BY similarity DESC 
             LIMIT {}", limit
        ))?;
        
        let rows = stmt.query_map(params![query_json], |row| {
            Ok((
                row.get::<_, String>(0)?,  // id
                row.get::<_, String>(1)?,  // content
                row.get::<_, f64>(2)?,     // similarity
            ))
        })?;
        
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        
        Ok(results)
    }
}