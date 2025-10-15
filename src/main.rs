use anyhow::{Context, Result};
use clap::Parser;
use log::{info, warn, error};
use std::path::{Path, PathBuf};

mod database;
mod document_processor;
mod embedding_manager;
mod error;

use database::Database;
use document_processor::DocumentProcessor;
use embedding_manager::EmbeddingManager;

#[derive(Parser)]
#[command(name = "portable-brains")]
#[command(about = "Portable Brains - Index documents into DuckDB for AI querying")]
struct Args {
    /// Path to the DuckDB file
    #[arg(short, long)]
    database: PathBuf,
    
    /// Name of the embedding model
    #[arg(short, long)]
    model: String,
    
    /// Directory containing PDF documents to index
    #[arg(short, long)]
    input_dir: PathBuf,
    
    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    // Initialize logging with better filtering
    let log_level = if args.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
        .filter_module("lopdf", log::LevelFilter::Warn) // Suppress lopdf debug messages
        .filter_module("duckdb", log::LevelFilter::Warn) // Suppress duckdb debug messages  
        .filter_module("ort", log::LevelFilter::Warn) // Suppress ONNX runtime debug messages
        .init();
    
    info!("Starting Portable Brains indexing process");
    
    // Validate input directory exists
    if !args.input_dir.exists() {
        anyhow::bail!("Input directory does not exist: {:?}", args.input_dir);
    }
    
    // Initialize database
    let mut db = Database::new(&args.database).await
        .context("Failed to initialize database")?;
    
    // Verify or set embedding model
    db.verify_or_set_model(&args.model).await
        .context("Failed to verify embedding model")?;
    
    // Initialize embedding manager
    let mut embedding_manager = EmbeddingManager::new(&args.model).await
        .context("Failed to initialize embedding manager")?;
    
    // Initialize document processor with memory-efficient sentence-based chunking
    let document_processor = DocumentProcessor::with_limits(
        800,        // chunk_size: Larger chunks for sentence-based approach
        100,        // overlap: Reasonable overlap in characters  
        50 * 1024 * 1024,  // max_file_size: 50MB per file (reduced from 100MB)
        5_000_000,  // max_text_length: 5M characters (reduced from 10M)
    );
    
    // Phase 1: Process all supported files and extract text (no embeddings yet)
    let supported_files = find_supported_files(&args.input_dir)?;
    info!("🎯 Found {} supported files to process", supported_files.len());
    
    if supported_files.is_empty() {
        warn!("No supported files found in directory: {:?}", args.input_dir);
        warn!("Supported formats: PDF, TXT, HTML, DOCX, PPTX, XLSX");
        return Ok(());
    }
    
    info!("🚀 Phase 1: Extracting text from documents...");
    for (i, file_path) in supported_files.iter().enumerate() {
        let extension = file_path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("unknown");
        
        info!("📝 Processing file {}/{}: {:?} ({})", 
              i + 1, supported_files.len(), file_path, extension.to_uppercase());
        
        match process_document(
            file_path,
            &mut db,
            &document_processor,
        ).await {
            Ok(_) => info!("✅ Successfully extracted text from: {:?}", file_path),
            Err(e) => {
                error!("❌ Failed to process {:?}: {}", file_path, e);
                // Continue processing other files
            }
        }
    }
    
    // Phase 2: Generate embeddings in batches of 20
    info!("🚀 Phase 2: Generating embeddings for stored text fragments...");
    let total_fragments = db.count_fragments_without_embeddings().await?;
    
    if total_fragments > 0 {
        info!("🧠 Found {} fragments requiring embeddings", total_fragments);
        
        const EMBEDDING_BATCH_SIZE: i32 = 50; // Reduced batch size for batched processing for better memory management
        let mut processed = 0;
        
        loop {
            let batch_processed = process_embedding_batch(
                &mut db,
                &mut embedding_manager,
                EMBEDDING_BATCH_SIZE,
            ).await?;
            
            if batch_processed == 0 {
                break; // No more fragments to process
            }
            
            processed += batch_processed;
            info!("✨ Progress: {}/{} embeddings complete ({:.1}%)", 
                  processed, total_fragments, 
                  (processed as f64 / total_fragments as f64) * 100.0);
            
            // Small delay between batches to prevent memory buildup
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        
        info!("🎉 Completed all {} embeddings!", processed);
    } else {
        info!("ℹ️  No fragments requiring embeddings found");
    }
    
    info!("🏁 Indexing process completed successfully!");
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
    db: &mut Database,
    processor: &DocumentProcessor,
) -> Result<()> {
    // Check if document already exists
    if db.document_exists(file_path).await? {
        warn!("Document already exists, skipping: {:?}", file_path);
        return Ok(());
    }
    
    // Check file size before loading
    let file_size = std::fs::metadata(file_path)?.len();
    info!("📄 Processing document: {:?} ({:.2} MB)", file_path, file_size as f64 / (1024.0 * 1024.0));
    
    if file_size > 100 * 1024 * 1024 {  // 100MB limit
        warn!("⚠️  Skipping large file: {:?} ({:.2} MB)", file_path, file_size as f64 / (1024.0 * 1024.0));
        return Ok(());
    }
    
    info!("📖 Reading file...");
    // Read and store the original file
    let file_data = std::fs::read(file_path).context("Failed to read file")?;
    let document_id = db.store_document(file_path, &file_data).await?;
    
    info!("🔍 Extracting text from file...");
    // Extract text from document with memory limits
    let text = processor.extract_text_from_document(file_path, &file_data)
        .context("Failed to extract text from file")?;
    
    // Free the file data from memory as soon as possible
    drop(file_data);
    
    info!("✂️  Splitting text into chunks...");
    // Split text into semantic chunks  
    let fragments = processor.chunk_text(&text)
        .context("Failed to chunk text")?;
    
    // Free the text from memory as soon as possible
    drop(text);
    
    info!("💾 Storing {} text fragments in database...", fragments.len());
    
    // Store all text fragments first (without embeddings) to free up memory immediately
    for (order, fragment) in fragments.iter().enumerate() {
        db.store_text_fragment(&document_id, order as i32, fragment).await
            .with_context(|| format!("Failed to store text fragment {}", order))?;
        
        if order > 0 && order % 100 == 0 {
            info!("💾 Stored {} text fragments...", order + 1);
        }
    }
    
    info!("✅ Stored all {} text fragments for document: {:?}", fragments.len(), file_path);
    Ok(())
}

/// Process embeddings in batches for fragments without embeddings using FastEmbed batch processing
async fn process_embedding_batch(
    db: &mut Database,
    embedding_manager: &mut EmbeddingManager,
    batch_size: i32,
) -> Result<i32> {
    let fragments = db.get_fragments_without_embeddings(batch_size).await?;
    
    if fragments.is_empty() {
        return Ok(0);
    }
    
    info!("🧠 Processing embeddings for {} fragments using batched FastEmbed...", fragments.len());
    
    // Extract texts and IDs separately for batch processing
    let texts: Vec<String> = fragments.iter().map(|(_, content)| content.clone()).collect();
    let fragment_ids: Vec<String> = fragments.iter().map(|(id, _)| id.clone()).collect();
    
    info!("⚡ Generating {} embeddings in a single batch...", texts.len());
    
    // Generate all embeddings in one batch call to FastEmbed
    let embeddings = embedding_manager.generate_embeddings_batch(&texts).await
        .context("Failed to generate batch embeddings")?;
    
    if embeddings.len() != fragment_ids.len() {
        anyhow::bail!("Embedding count mismatch: expected {}, got {}", fragment_ids.len(), embeddings.len());
    }
    
    info!("💾 Storing {} embeddings in database...", embeddings.len());
    
    // Store all embeddings in the database
    for (i, (fragment_id, embedding)) in fragment_ids.iter().zip(embeddings.iter()).enumerate() {
        if embedding.is_empty() {
            warn!("⚠️  Skipping empty embedding for fragment {}", fragment_id);
            continue;
        }
        
        db.update_fragment_embedding(fragment_id, embedding).await
            .with_context(|| format!("Failed to update embedding for fragment {}", fragment_id))?;
            
        if (i + 1) % 100 == 0 {
            info!("💾 Stored {}/{} embeddings...", i + 1, embeddings.len());
        }
    }
    
    Ok(fragments.len() as i32)
}