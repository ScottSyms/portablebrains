# EatMyBrain - Conversational RAG

EatMyBrain is a conversational RAG (Retrieval-Augmented Generation) application that uses the vector databases created by Portable Brains for intelligent document-based conversations.

## Features

- 🧠 **Vector Search**: Uses embeddings to find relevant document fragments
- 💬 **Conversational Interface**: Interactive chat with your document knowledge base  
- 🔌 **Flexible LLM Integration**: Works with any OpenAI-compatible API endpoint
- 🎯 **Configurable Results**: Control how many document fragments are used for context
- 📚 **Multi-format Support**: Works with any documents indexed by Portable Brains

## Usage

### Basic Usage
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
- `--endpoint`: LLM API endpoint URL (OpenAI-compatible)
- `--api-key`: Your API key for the LLM service
- `--model`: Model to use (default: gpt-4)
- `--results`: Number of similar documents to retrieve (1-20, default: 5)
- `--embedding-model`: Must match the model used during indexing (default: BAAI/bge-small-en-v1.5)
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

EatMyBrain works with any OpenAI-compatible API, including:

### OpenAI
```bash
--endpoint https://api.openai.com/v1/chat/completions
--api-key sk-your-openai-key
--model gpt-4
```

### Anthropic Claude (via compatible proxy)
```bash
--endpoint https://your-proxy.com/v1/chat/completions
--api-key your-anthropic-key
--model claude-3-sonnet
```

### Local models (via Ollama, LM Studio, etc.)
```bash
--endpoint http://localhost:11434/v1/chat/completions
--api-key dummy-key
--model llama2
```

### Azure OpenAI
```bash
--endpoint https://your-resource.openai.azure.com/openai/deployments/your-deployment/chat/completions?api-version=2023-05-15
--api-key your-azure-key
--model gpt-4
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