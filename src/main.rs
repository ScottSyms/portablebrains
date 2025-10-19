use anyhow::{anyhow, Context, Result};
use clap::{Parser, ValueEnum};
// use log::{info, warn};
use std::path::{Path, PathBuf};

mod database;
mod document_processor;
mod storage;
mod duckdb_storage;
mod lancedb_storage;
mod embedding_manager;
mod error;

// use database::Database;  // Not used with storage abstraction
use document_processor::DocumentProcessor;
use embedding_manager::EmbeddingManager;
use storage::{Storage};
use duckdb_storage::DuckDBStorage;
use lancedb_storage::LanceDBStorage;

#[derive(Clone, ValueEnum)]
enum Backend {
    Duckdb,
    Lancedb,
}

#[derive(Clone, ValueEnum)]
enum EmbeddingProvider {
    Local,
    Remote,
}

#[derive(Parser)]
#[command(name = "portable-brains")]
#[command(about = "Portable Brains - Index documents with configurable storage backend")]
struct Args {
    /// Path to the database file (extension determines format: .db for DuckDB, .lancedb for LanceDB)
    #[arg(short, long)]
    database: PathBuf,
    
    /// Name of the embedding model
    #[arg(short, long)]
    model: String,
    
    /// Directory containing documents to index (PDF, TXT, HTML, DOCX, PPTX, XLSX)
    #[arg(short, long)]
    input_dir: PathBuf,
    
    /// Storage backend to use
    #[arg(short, long, value_enum, default_value = "duckdb")]
    backend: Backend,
    
    /// Embedding provider to use
    #[arg(short = 'p', long, value_enum, default_value = "local")]
    embedding_provider: EmbeddingProvider,
    
    /// API key for remote embedding providers (required for remote)
    #[arg(long)]
    api_key: Option<String>,
    
    /// Endpoint URL for remote embedding service (defaults to OpenAI if not specified)
    #[arg(long)]
    endpoint: Option<String>,
    
    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

async fn create_storage(backend: Backend, database_path: &Path) -> Result<Box<dyn Storage>> {
    match backend {
        Backend::Duckdb => {
            let storage = DuckDBStorage::new(database_path).await?;
            Ok(Box::new(storage))
        }
        Backend::Lancedb => {
            let storage = LanceDBStorage::new(database_path).await?;
            Ok(Box::new(storage))
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    // Initialize logging with cleaner output
    let log_level = if args.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
        .filter_module("lopdf", log::LevelFilter::Warn)         // Suppress lopdf debug messages
        .filter_module("duckdb", log::LevelFilter::Warn)        // Suppress duckdb debug messages  
        .filter_module("ort", log::LevelFilter::Warn)           // Suppress ONNX runtime debug messages
        .filter_module("html5ever", log::LevelFilter::Warn)     // Suppress HTML parser debug messages
        .filter_module("selectors", log::LevelFilter::Warn)     // Suppress CSS selector debug messages
        .filter_module("lancedb", log::LevelFilter::Warn)       // Suppress LanceDB debug messages
        .format_target(false)                                   // Hide module names in output
        .format_timestamp(None)                                 // Hide timestamps for cleaner output
        .init();
    
    let backend_name = match args.backend {
        Backend::Duckdb => "DuckDB",
        Backend::Lancedb => "LanceDB",
    };
    
    println!("🧠 Portable Brains - Document Indexer");
    println!("📁 Scanning directory: {}", args.input_dir.display());
    println!("💾 Using {} backend: {}", backend_name, args.database.display());
    
    // Validate input directory exists
    if !args.input_dir.exists() {
        anyhow::bail!("Input directory does not exist: {}", args.input_dir.display());
    }
    
    // Initialize storage backend
    let mut storage = create_storage(args.backend.clone(), &args.database).await
        .context("Failed to initialize storage backend")?;
    
    // Verify or set embedding model
    storage.verify_or_set_model(&args.model).await
        .context("Failed to verify embedding model")?;
    
    // Initialize embedding manager based on provider
    let mut embedding_manager = match args.embedding_provider {
        EmbeddingProvider::Local => {
            EmbeddingManager::new(&args.model).await
                .context("Failed to initialize local embedding manager")?
        },
        EmbeddingProvider::Remote => {
            let api_key = args.api_key
                .ok_or_else(|| anyhow::anyhow!("API key is required for remote embedding provider"))?;
            
            EmbeddingManager::new_remote(api_key, &args.model, args.endpoint).await
                .context("Failed to initialize remote embedding manager")?
        },
    };
    
    // Initialize document processor with memory-efficient sentence-based chunking
    let document_processor = DocumentProcessor::with_limits(
        800,        // chunk_size: Larger chunks for sentence-based approach
        100,        // overlap: Reasonable overlap in characters  
        50 * 1024 * 1024,  // max_file_size: 50MB per file (reduced from 100MB)
        5_000_000,  // max_text_length: 5M characters (reduced from 10M)
    );
    
    // Phase 1: Process all supported files and extract text (no embeddings yet)
    let supported_files = find_supported_files(&args.input_dir)?;
    println!("📂 Found {} documents to process", supported_files.len());
    
    if supported_files.is_empty() {
        println!("⚠️  No supported files found in directory: {}", args.input_dir.display());
        println!("📋 Supported formats: PDF, TXT, HTML, DOCX, PPTX, XLSX");
        return Ok(());
    }
    
    println!("\n🚀 Phase 1: Extracting text from documents...");
    for (i, file_path) in supported_files.iter().enumerate() {
        let filename = file_path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");
        let extension = file_path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("unknown");
        
        print!("📝 [{}/{}] Processing {} ({})... ", 
               i + 1, supported_files.len(), filename, extension.to_uppercase());
        
        match process_document(
            file_path,
            &mut *storage,
            &document_processor,
        ).await {
            Ok(fragment_count) => {
                println!("✅ Success! ({} fragments)", fragment_count);
            },
            Err(e) => {
                println!("❌ Failed: {}", e);
                if args.verbose {
                    eprintln!("   Error details: {:?}", e);
                }
                // Continue processing other files
            }
        }
    }
    
    // Phase 2: Generate embeddings in batches
    let total_fragments = storage.count_fragments_without_embeddings().await?;
    
    if total_fragments > 0 {
        println!("\n🧠 Phase 2: Generating embeddings for {} text fragments...", total_fragments);
        
        const EMBEDDING_BATCH_SIZE: i32 = 50;
        let mut processed = 0;
        
        loop {
            let batch_processed = process_embedding_batch(
                &mut *storage,
                &mut embedding_manager,
                EMBEDDING_BATCH_SIZE,
            ).await?;
            
            if batch_processed == 0 {
                break; // No more fragments to process
            }
            
            processed += batch_processed;
            let percentage = (processed as f64 / total_fragments as f64) * 100.0;
            print!("\r⚡ Generating embeddings: {}/{} ({:.1}%)", 
                   processed, total_fragments, percentage);
            std::io::Write::flush(&mut std::io::stdout()).unwrap();
            
            // Small delay between batches to prevent memory buildup
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        
        println!("\n✅ Completed all embeddings!");
    } else {
        println!("\nℹ️  All fragments already have embeddings");
    }
    
    println!("\n� Indexing completed successfully!");
    Ok(())
}

fn find_supported_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut supported_files = Vec::new();
    let supported_extensions = ["pdf", "txt", "text", "html", "htm", "docx", "pptx", "xlsx"];
    
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_file() {
            if let Some(extension) = path.extension() {
                let ext_lower = extension.to_string_lossy().to_lowercase();
                if supported_extensions.contains(&ext_lower.as_str()) {
                    supported_files.push(path);
                }
            }
        }
    }
    
    Ok(supported_files)
}

async fn process_document(
    file_path: &Path,
    storage: &mut dyn Storage,
    processor: &DocumentProcessor,
) -> Result<usize> {
    // Check if document already exists
    if storage.document_exists(file_path).await? {
        return Err(anyhow::anyhow!("Document already exists"));
    }
    
    // Check file size before loading
    let file_size = std::fs::metadata(file_path)?.len();
    
    if file_size > 100 * 1024 * 1024 {  // 100MB limit
        return Err(anyhow::anyhow!("File too large ({:.1} MB)", file_size as f64 / (1024.0 * 1024.0)));
    }
    
    // Read and store the original file
    let file_data = std::fs::read(file_path).context("Failed to read file")?;
    let document_id = storage.store_document(file_path, &file_data).await?;
    
    // Extract text from document with memory limits
    let text = processor.extract_text_from_document(file_path, &file_data)
        .context("Failed to extract text")?;
    
    // Free the file data from memory as soon as possible
    drop(file_data);
    
    // Split text into semantic chunks  
    let fragments = processor.chunk_text(&text)
        .context("Failed to chunk text")?;
    
    // Free the text from memory as soon as possible
    drop(text);
    
    let fragment_count = fragments.len();
    
    // Store all text fragments first (without embeddings) to free up memory immediately
    for (order, fragment) in fragments.iter().enumerate() {
        storage.store_text_fragment(&document_id, order as i32, fragment).await
            .with_context(|| format!("Failed to store text fragment {}", order))?;
    }
    
    Ok(fragment_count)
}

/// Process embeddings in batches for fragments without embeddings using FastEmbed batch processing
async fn process_embedding_batch(
    storage: &mut dyn Storage,
    embedding_manager: &mut EmbeddingManager,
    batch_size: i32,
) -> Result<i32> {
    let fragments = storage.get_fragments_without_embeddings(batch_size).await?;
    
    if fragments.is_empty() {
        return Ok(0);
    }
    
    // Extract texts and IDs separately for batch processing
    let texts: Vec<String> = fragments.iter().map(|(_, content)| content.clone()).collect();
    let fragment_ids: Vec<String> = fragments.iter().map(|(id, _)| id.clone()).collect();
    
    // Generate all embeddings in one batch call to FastEmbed
    let embeddings = embedding_manager.generate_embeddings_batch(&texts).await
        .context("Failed to generate batch embeddings")?;
    
    if embeddings.len() != fragment_ids.len() {
        anyhow::bail!("Embedding count mismatch: expected {}, got {}", fragment_ids.len(), embeddings.len());
    }
    
    // Store all embeddings in the database
    for (fragment_id, embedding) in fragment_ids.iter().zip(embeddings.iter()) {
        if embedding.is_empty() {
            continue;
        }
        
        storage.update_fragment_embedding(fragment_id, embedding).await
            .with_context(|| format!("Failed to update embedding for fragment {}", fragment_id))?;
    }
    
    Ok(fragments.len() as i32)
}