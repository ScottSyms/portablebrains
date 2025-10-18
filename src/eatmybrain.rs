use anyhow::{Context, Result};
use clap::Parser;
use console::{style, Term};
use std::path::PathBuf;
use std::io::{self, Write};
use tokio;
use log;

mod duckdb_storage;
mod embedding_manager;
mod storage;

use duckdb_storage::DuckDBStorage;
use embedding_manager::EmbeddingManager;
use storage::Storage;

#[derive(Parser)]
#[command(name = "eatmybrain")]
#[command(about = "Conversational RAG using Portable Brains vector database")]
struct Args {
    /// Path to the DuckDB database file created by portable-brains
    #[arg(short, long)]
    database: PathBuf,
    
    /// LLM API endpoint URL
    #[arg(short, long)]
    endpoint: String,
    
    /// API key for the LLM service
    #[arg(short, long)]
    api_key: String,
    
    /// Model name to use for the LLM
    #[arg(short, long, default_value = "gpt-4")]
    model: String,
    
    /// Number of similar documents to retrieve for context (1-20)
    #[arg(short, long, default_value = "5")]
    results: usize,
    
    /// Embedding model name (must match what was used for indexing)
    #[arg(long, default_value = "BAAI/bge-small-en-v1.5")]
    embedding_model: String,
    
    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(serde::Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
}

#[derive(serde::Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(serde::Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

struct RagEngine {
    storage: Box<dyn Storage>,
    embedding_manager: EmbeddingManager,
    llm_client: reqwest::Client,
    endpoint: String,
    api_key: String,
    model: String,
    max_results: usize,
    verbose: bool,
}

impl RagEngine {
    async fn new(args: Args) -> Result<Self> {
        // Validate database exists
        if !args.database.exists() {
            anyhow::bail!("Database file does not exist: {}", args.database.display());
        }

        // Initialize storage
        let storage = Box::new(DuckDBStorage::new(&args.database).await
            .context("Failed to open database")?);

        // Initialize embedding manager
        let embedding_manager = EmbeddingManager::new(&args.embedding_model).await
            .context("Failed to initialize embedding manager")?;

        // Create HTTP client
        let llm_client = reqwest::Client::new();

        // Validate results count
        let max_results = if args.results == 0 || args.results > 20 {
            println!("⚠️  Results count must be between 1 and 20. Using default: 5");
            5
        } else {
            args.results
        };

        Ok(RagEngine {
            storage,
            embedding_manager,
            llm_client,
            endpoint: args.endpoint,
            api_key: args.api_key,
            model: args.model,
            max_results,
            verbose: args.verbose,
        })
    }

    async fn search_similar_content(&mut self, query: &str) -> Result<Vec<String>> {
        // Generate embedding for the query
        let query_embedding = self.embedding_manager.generate_embeddings_batch(&[query.to_string()]).await
            .context("Failed to generate query embedding")?;

        if query_embedding.is_empty() {
            anyhow::bail!("Failed to generate embedding for query");
        }

        // Search for similar content in the database
        let results = self.storage.search_similar(&query_embedding[0], self.max_results).await
            .context("Failed to search similar content")?;

        // Extract just the content from the results (ignore fragment_id and similarity_score)
        let content: Vec<String> = results.into_iter()
            .map(|(_, content, _)| content)
            .collect();

        Ok(content)
    }

    async fn generate_response(&self, query: &str, context: &[String]) -> Result<String> {
        // Prepare context for the LLM
        let context_text = if context.is_empty() {
            "No relevant documents found.".to_string()
        } else {
            context.join("\n\n")
        };

        let system_prompt = format!(
            "You are a helpful AI assistant with access to a knowledge base. \
            Use the following context to answer the user's question. If the context \
            doesn't contain relevant information, say so politely.\n\nContext:\n{}",
            context_text
        );

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt,
            },
            ChatMessage {
                role: "user".to_string(),
                content: query.to_string(),
            },
        ];

        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            max_tokens: Some(1000),
            temperature: Some(0.7),
        };

        // Make API call to LLM
        let response = self.llm_client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to send request to LLM API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("LLM API error {}: {}", status, error_text);
        }

        let chat_response: ChatResponse = response.json().await
            .context("Failed to parse LLM response")?;

        if chat_response.choices.is_empty() {
            anyhow::bail!("No response choices received from LLM");
        }

        Ok(chat_response.choices[0].message.content.clone())
    }

    async fn chat_loop(&mut self) -> Result<()> {
        let term = Term::stdout();
        
        println!("🧠 {} - Conversational RAG", style("EatMyBrain").bold().cyan());
        println!("💬 Type your questions or 'quit' to exit");
        println!("🔍 Retrieving {} similar documents per query", self.max_results);
        println!();

        loop {
            // Print prompt
            print!("{} ", style("❯").bold().green());
            io::stdout().flush().unwrap();

            // Read user input
            let input = term.read_line().context("Failed to read input")?;
            let query = input.trim();

            // Handle special commands
            if query.is_empty() {
                continue;
            }
            
            if query.eq_ignore_ascii_case("quit") || query.eq_ignore_ascii_case("exit") {
                println!("👋 Goodbye!");
                break;
            }

            if query.eq_ignore_ascii_case("help") {
                self.show_help();
                continue;
            }

            // Process the query
            println!("{} Searching knowledge base...", style("🔍").dim());
            
            match self.search_similar_content(query).await {
                Ok(context) => {
                    if !context.is_empty() {
                        println!("{} Found {} relevant documents", 
                               style("📚").dim(), context.len());
                    } else {
                        println!("{} No relevant documents found for your query", style("💭").dim());
                    }
                    
                    println!("{} Generating response...", style("🤔").dim());
                    
                    match self.generate_response(query, &context).await {
                        Ok(response) => {
                            println!();
                            println!("{}", style(&response).white());
                            println!();
                        }
                        Err(e) => {
                            println!("{} LLM Error: {}", style("❌").red(), e);
                            if self.verbose {
                                println!("   Debug: {:?}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("{} Search Error: {}", style("❌").red(), e);
                    if self.verbose {
                        println!("   Debug: {:?}", e);
                    }
                }
            }
        }

        Ok(())
    }

    fn show_help(&self) {
        println!();
        println!("{}", style("Available commands:").bold());
        println!("  help  - Show this help message");
        println!("  quit  - Exit the program");
        println!("  Any other text will be treated as a query");
        println!();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
        .filter_module("duckdb", log::LevelFilter::Warn)
        .filter_module("ort", log::LevelFilter::Warn)
        .filter_module("reqwest", log::LevelFilter::Warn)
        .format_target(false)
        .format_timestamp(None)
        .init();

    // Validate arguments
    if args.results == 0 {
        anyhow::bail!("Results count must be at least 1");
    }
    
    if args.results > 20 {
        anyhow::bail!("Results count cannot exceed 20");
    }

    // Initialize RAG engine
    println!("🚀 Initializing EatMyBrain RAG engine...");
    println!("📊 Database: {}", args.database.display());
    println!("🌐 LLM Endpoint: {}", args.endpoint);
    println!("🤖 Model: {}", args.model);

    let mut rag_engine = RagEngine::new(args).await
        .context("Failed to initialize RAG engine")?;

    println!("✅ Ready!");
    println!();

    // Start the chat loop
    rag_engine.chat_loop().await?;

    Ok(())
}