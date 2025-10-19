# EatMyBrain - Conversational RAG

EatMyBrain is a conversational RAG (Retrieval-Augmented Generation) application that uses the vector databases created by Portable Brains for intelligent document-based conversations.

## Features

- 🧠 **Vector Search**: Uses embeddings to find relevant document fragments
- 💬 **Conversational Interface**: Interactive chat with your document knowledge base  
- 🔌 **Flexible LLM Integration**: Works with any OpenAI-compatible API endpoint
- 🎯 **Configurable Results**: Control how many document fragments are used for context
- 📚 **Multi-format Support**: Works with any documents indexed by Portable Brains

## Usage

### Basic Usage with AI Model Selection
```bash
# Quick start with popular AI models (auto-configures endpoints)
cargo run --bin eatmybrain -- \
  --database my_documents.db \
  --api-key sk-your-api-key \
  --ai-model gpt4

# Or use Claude
cargo run --bin eatmybrain -- \
  --database my_documents.db \
  --api-key your-anthropic-key \
  --ai-model claude3-sonnet
```

### Advanced Usage with Custom Configuration
```bash
cargo run --bin eatmybrain -- \
  --database my_documents.db \
  --endpoint https://api.openai.com/v1/chat/completions \
  --api-key sk-your-api-key \
  --model gpt-4 \
  --results 5
```

### Command Line Options

- `--database`: Path to DuckDB file created by portable-brains
- `--ai-model`: Select from popular AI models (auto-configures endpoint and model)
  - `gpt4`: OpenAI GPT-4 (most capable, slower, expensive)  
  - `gpt4-turbo`: OpenAI GPT-4 Turbo (faster than GPT-4, good balance)
  - `gpt35-turbo`: OpenAI GPT-3.5 Turbo (fast, cost-effective)
  - `claude3-opus`: Anthropic Claude 3 Opus (very capable, good for complex tasks)
  - `claude3-sonnet`: Anthropic Claude 3 Sonnet (balanced performance)
  - `claude3-haiku`: Anthropic Claude 3 Haiku (fast, cost-effective)
  - `custom`: Use custom model name (specify with --model)
- `--endpoint`: LLM API endpoint URL (auto-detected for known AI models)
- `--api-key`: Your API key for the LLM service
- `--model`: Custom model name (used with --ai-model=custom or when no --ai-model specified)
- `--results`: Number of similar documents to retrieve (1-20, default: 5)  
- `--embedding-model` (`-E`): Must match the model used during indexing (default: BAAI/bge-small-en-v1.5)
- `--verbose`: Enable debug logging

### Interactive Commands

Once running, you can use these commands:
- `help` - Show available commands
- `quit` or `exit` - Exit the program
- Any other text - Ask a question about your documents

### Example Session

```
🧠 EatMyBrain - Conversational RAG
💬 Type your questions or 'quit' to exit
🔍 Retrieving 5 similar documents per query

❯ What are the main features of the product?
🔍 Searching knowledge base...
📚 Found 3 relevant documents
🤔 Generating response...

Based on the documentation, the main features include:
1. Advanced vector search capabilities
2. Multi-format document support
3. Real-time embedding generation
...

❯ quit
👋 Goodbye!
```

## Supported LLM Providers

### Quick Setup with AI Model Selection

```bash
# OpenAI models (easiest)
--ai-model gpt4 --api-key sk-your-openai-key
--ai-model gpt4-turbo --api-key sk-your-openai-key  
--ai-model gpt35-turbo --api-key sk-your-openai-key

# Anthropic Claude models
--ai-model claude3-opus --api-key your-anthropic-key
--ai-model claude3-sonnet --api-key your-anthropic-key
--ai-model claude3-haiku --api-key your-anthropic-key

# Local models (Ollama, LM Studio, etc.)
--ai-model custom \
--model llama2 \
--endpoint http://localhost:11434/v1/chat/completions \
--api-key dummy-key
```

### Manual Configuration (Advanced)

EatMyBrain works with any OpenAI-compatible API:

```bash
# OpenAI
--endpoint https://api.openai.com/v1/chat/completions
--api-key sk-your-openai-key
--model gpt-4

# Azure OpenAI  
--endpoint https://your-resource.openai.azure.com/openai/deployments/your-deployment/chat/completions?api-version=2023-05-15
--api-key your-azure-key
--model gpt-4

# Local models
--endpoint http://localhost:11434/v1/chat/completions
--api-key dummy-key
--model llama2
```

## Prerequisites

1. **Indexed Documents**: Use `portable-brains` to create a DuckDB database first:
   ```bash
   cargo run --bin portable-brains -- \
     --database my_docs.db \
     --model "BAAI/bge-small-en-v1.5" \
     --input-dir ./documents \
     --backend duckdb
   ```

2. **LLM API Access**: Obtain API credentials for your chosen LLM provider

3. **Matching Embedding Model**: The `--embedding-model` must match what was used during indexing

## Tips for Best Results

1. **Use appropriate result counts**:
   - Start with 5 results for most queries
   - Use 10-15 for complex questions needing more context
   - Use 1-3 for specific fact lookups

2. **Choose the right model**:
   - GPT-4: Best quality but slower/more expensive
   - GPT-3.5-turbo: Good balance of speed and quality
   - Local models: Private but may need more context

3. **Optimize your document collection**:
   - More diverse documents = better coverage
   - Well-structured documents = better chunking
   - Regular re-indexing keeps content fresh

## Troubleshooting

### "Database file does not exist"
- Make sure you've run `portable-brains` first to create the database
- Check the path to your .db file

### "Failed to generate query embedding"
- Ensure the embedding model matches what was used for indexing
- Check that FastEmbed can load the model

### "LLM API error"
- Verify your API key is correct
- Check the endpoint URL format
- Ensure you have sufficient API credits

### "No relevant documents found"
- Try rephrasing your question
- Check if documents were successfully indexed
- Consider increasing the results count