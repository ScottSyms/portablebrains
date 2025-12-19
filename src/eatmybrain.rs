use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use console::{style, Term};
use std::path::PathBuf;
use std::io::{self, Write};
use tokio;
use log;

mod duckdb_storage;
mod embedding_manager;
mod storage;
mod mcp_server;

use duckdb_storage::DuckDBStorage;
use embedding_manager::EmbeddingManager;
use storage::Storage;
use mcp_server::{McpServer, run_http_server};

#[derive(Clone, ValueEnum)]
enum AIModel {
    /// OpenAI GPT-4 (most capable, slower, expensive)
    Gpt4,
    /// OpenAI GPT-4 Turbo (faster than GPT-4, good balance)
    Gpt4Turbo,
    /// OpenAI GPT-3.5 Turbo (fast, cost-effective)
    Gpt35Turbo,
    /// Anthropic Claude 3 Opus (very capable, good for complex tasks)
    Claude3Opus,
    /// Anthropic Claude 3 Sonnet (balanced performance)
    Claude3Sonnet,
    /// Anthropic Claude 3 Haiku (fast, cost-effective)
    Claude3Haiku,
    /// Use custom model name (specify with --model)
    Custom,
}

impl AIModel {
    fn model_name(&self) -> &'static str {
        match self {
            AIModel::Gpt4 => "gpt-4",
            AIModel::Gpt4Turbo => "gpt-4-turbo",
            AIModel::Gpt35Turbo => "gpt-3.5-turbo",
            AIModel::Claude3Opus => "claude-3-opus-20240229",
            AIModel::Claude3Sonnet => "claude-3-sonnet-20240229",
            AIModel::Claude3Haiku => "claude-3-haiku-20240307",
            AIModel::Custom => "", // Will use the --model parameter
        }
    }

    fn suggested_endpoint(&self) -> &'static str {
        match self {
            AIModel::Gpt4 | AIModel::Gpt4Turbo | AIModel::Gpt35Turbo => 
                "https://api.openai.com/v1/chat/completions",
            AIModel::Claude3Opus | AIModel::Claude3Sonnet | AIModel::Claude3Haiku => 
                "https://api.anthropic.com/v1/messages",
            AIModel::Custom => "", // User must specify
        }
    }
}

#[derive(Parser)]
#[command(name = "eatmybrain")]
#[command(about = "Conversational RAG using Portable Brains vector database")]
struct Args {
    /// Path to the DuckDB database file created by portable-brains
    #[arg(short, long)]
    database: PathBuf,
    
    /// LLM API endpoint URL (auto-detected for known AI models if not specified)
    #[arg(short, long)]
    endpoint: Option<String>,
    
    /// API key for the LLM service
    #[arg(short, long)]
    api_key: String,

    /// Select from popular AI models (auto-configures endpoint and model name)
    #[arg(long, value_enum)]
    ai_model: Option<AIModel>,
    
    /// Custom model name (used when --ai-model=custom or no --ai-model specified)
    #[arg(short, long, default_value = "gpt-4")]
    model: String,
    
    /// Number of similar documents to retrieve for context (1-20)
    #[arg(short, long, default_value = "5")]
    results: usize,
    
    /// Embedding model name (must match what was used for indexing)
    /// Popular options: BAAI/bge-small-en-v1.5, sentence-transformers/all-MiniLM-L6-v2, 
    /// sentence-transformers/all-mpnet-base-v2, nomic-ai/nomic-embed-text-v1
    #[arg(short = 'E', long, default_value = "BAAI/bge-small-en-v1.5")]
    embedding_model: String,
    
    /// Run as Model Context Protocol (MCP) server on stdio
    #[arg(long)]
    mcp: bool,
    
    /// Run as MCP HTTP server on the specified host:port (e.g., 127.0.0.1:3000)
    #[arg(long)]
    http: Option<String>,
    
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
        // Process AI model selection and auto-configure endpoint/model
        let (final_endpoint, final_model) = match &args.ai_model {
            Some(ai_model) => {
                let suggested_endpoint = ai_model.suggested_endpoint();
                let model_name = ai_model.model_name();
                
                // Use suggested endpoint if no endpoint provided
                let endpoint = args.endpoint.clone().unwrap_or_else(|| {
                    if suggested_endpoint.is_empty() {
                        panic!("Custom AI model requires --endpoint to be specified");
                    }
                    suggested_endpoint.to_string()
                });
                
                // Use model from AI selection unless it's custom
                let model = if matches!(ai_model, AIModel::Custom) {
                    args.model.clone()
                } else {
                    model_name.to_string()
                };
                
                (endpoint, model)
            }
            None => {
                // No AI model specified, require endpoint and use provided model
                let endpoint = args.endpoint.clone()
                    .ok_or_else(|| anyhow::anyhow!("--endpoint is required when not using --ai-model"))?;
                (endpoint, args.model.clone())
            }
        };

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
            endpoint: final_endpoint,
            api_key: args.api_key,
            model: final_model,
            max_results,
            verbose: args.verbose,
        })
    }

    /// Public method for querying the knowledge base with RAG
    /// Returns the LLM-generated response based on retrieved documents
    pub async fn query(&mut self, query: &str, max_results: usize) -> Result<String> {
        // Temporarily override max_results if different
        let original_max = self.max_results;
        self.max_results = max_results;
        
        let context = self.search_similar_content(query).await?;
        let response = self.generate_response(query, &context).await?;
        
        // Restore original max_results
        self.max_results = original_max;
        
        Ok(response)
    }

    /// Public method for searching similar documents without LLM generation
    /// Returns raw text snippets with similarity scores
    pub async fn search_similar(&mut self, query: &str, max_results: usize) -> Result<Vec<(String, f64)>> {
        // Generate embedding for the query
        let query_embedding = self.embedding_manager.generate_embeddings_batch(&[query.to_string()]).await
            .context("Failed to generate query embedding")?;

        if query_embedding.is_empty() {
            anyhow::bail!("Failed to generate embedding for query");
        }

        // Search for similar content in the database
        let results = self.storage.search_similar(&query_embedding[0], max_results).await
            .context("Failed to search similar content")?;

        // Return content with similarity scores (ignore fragment_id)
        let content_with_scores: Vec<(String, f64)> = results.into_iter()
            .map(|(_, content, similarity)| (content, similarity))
            .collect();

        Ok(content_with_scores)
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
    let mut builder = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level));
    builder
        .filter_module("duckdb", log::LevelFilter::Warn)
        .filter_module("ort", log::LevelFilter::Warn)
        .filter_module("reqwest", log::LevelFilter::Warn)
        .format_target(false)
        .format_timestamp(None);
    
    // In MCP mode, write logs to stderr to avoid interfering with JSON-RPC on stdout
    if args.mcp {
        builder.target(env_logger::Target::Stderr);
    }
    
    builder.init();

    // Validate arguments
    if args.results == 0 {
        anyhow::bail!("Results count must be at least 1");
    }
    
    if args.results > 20 {
        anyhow::bail!("Results count cannot exceed 20");
    }
    
    // Check if both MCP modes are enabled
    if args.mcp && args.http.is_some() {
        anyhow::bail!("Cannot use both --mcp (stdio) and --http modes simultaneously");
    }
    
    // Clone http binding address before consuming args
    let http_bind_addr = args.http.clone();

    // Check if running in MCP stdio mode
    if args.mcp {
        // Initialize RAG engine for MCP
        log::info!("Initializing EatMyBrain as MCP server (stdio)");
        log::info!("Database: {}", args.database.display());

        let rag_engine = RagEngine::new(args).await
            .context("Failed to initialize RAG engine")?;

        log::info!("LLM Endpoint: {}", rag_engine.endpoint);
        log::info!("Model: {}", rag_engine.model);
        log::info!("MCP server ready on stdio");

        // Start MCP server on stdio
        let mut mcp_server = McpServer::new(rag_engine);
        mcp_server.run().await?;
    } else if let Some(bind_addr) = http_bind_addr {
        // HTTP MCP server mode
        log::info!("Initializing EatMyBrain as MCP HTTP server");
        log::info!("Database: {}", args.database.display());
        log::info!("Binding to: {}", bind_addr);

        let rag_engine = RagEngine::new(args).await
            .context("Failed to initialize RAG engine")?;

        log::info!("LLM Endpoint: {}", rag_engine.endpoint);
        log::info!("Model: {}", rag_engine.model);
        
        // Start HTTP server
        run_http_server(rag_engine, bind_addr).await?;
    } else {
        // Standard interactive mode
        println!("🚀 Initializing EatMyBrain RAG engine...");
        println!("📊 Database: {}", args.database.display());

        let mut rag_engine = RagEngine::new(args).await
            .context("Failed to initialize RAG engine")?;

        println!("🌐 LLM Endpoint: {}", rag_engine.endpoint);
        println!("🤖 Model: {}", rag_engine.model);
        println!("✅ Ready!");
        println!();

        // Start the chat loop
        rag_engine.chat_loop().await?;
    }

    Ok(())
}