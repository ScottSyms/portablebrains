# Portable Brains

A Rust-based document indexing system that converts PDF documents into a searchable DuckDB archive optimized for generative AI querying. The system extracts text from PDFs, performs semantic chunking, generates embeddings, and stores everything in a structured database format.

## Features

- **DuckDB Storage**: Uses DuckDB as the backend with three optimized tables:
  - `meta`: Stores database version and embedding model information
  - `documents`: Stores original PDF files with metadata
  - `fragments`: Stores text chunks with their embeddings and ordering
- **PDF Processing**: Extracts text from PDF documents using `lopdf`
- **Semantic Chunking**: Intelligent text splitting that preserves semantic meaning
- **Embedding Generation**: Uses FastEmbed ONNX models for production-quality embeddings
- **Model Validation**: Ensures consistency of embedding models across database sessions
- **Incremental Updates**: Appends to existing databases or creates new ones as needed

## Installation

### Prerequisites

- Rust 1.70 or later
- Git

### Build from Source

```bash
git clone <repository-url>
cd portable-brains
cargo build --release
```

## Usage

### Basic Command

```bash
./target/release/portable-brains \
  --database /path/to/archive.duckdb \
  --model "BAAI/bge-small-en-v1.5" \
  --input-dir /path/to/pdf/documents \
  --verbose
```

### Command Line Arguments

- `--database, -d`: Path to the DuckDB file (created if doesn't exist)
- `--model, -m`: Name of the embedding model to use
- `--input-dir, -i`: Directory containing PDF documents to index
- `--verbose, -v`: Enable verbose logging

### Supported Embedding Models

The system supports the following FastEmbed ONNX models:

- `BAAI/bge-small-en-v1.5` (384 dimensions)
- `BAAI/bge-base-en-v1.5` (768 dimensions) 
- `BAAI/bge-large-en-v1.5` (1024 dimensions)
- `sentence-transformers/all-MiniLM-L6-v2` (384 dimensions)
- `sentence-transformers/all-MiniLM-L12-v2` (384 dimensions)
- `intfloat/multilingual-e5-large` (1024 dimensions)

### Example Usage

```bash
# Create a new archive with research papers
./target/release/portable-brains \
  --database ./research_archive.duckdb \
  --model "BAAI/bge-small-en-v1.5" \
  --input-dir ./research_papers/

# Add more documents to existing archive
./target/release/portable-brains \
  --database ./research_archive.duckdb \
  --model "BAAI/bge-small-en-v1.5" \
  --input-dir ./new_papers/
```

## Database Schema

### Meta Table
```sql
CREATE TABLE meta (
    key VARCHAR PRIMARY KEY,
    value VARCHAR NOT NULL
);
```

### Documents Table
```sql
CREATE TABLE documents (
    id VARCHAR PRIMARY KEY,
    filename VARCHAR NOT NULL,
    file_path VARCHAR NOT NULL,
    pdf_data BLOB NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(file_path)
);
```

### Fragments Table
```sql
CREATE TABLE fragments (
    id VARCHAR PRIMARY KEY,
    document_id VARCHAR NOT NULL,
    fragment_order INTEGER NOT NULL,
    content TEXT NOT NULL,
    embedding DOUBLE[],
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (document_id) REFERENCES documents(id)
);
```

## Configuration

### Text Chunking

The system uses semantic chunking with the following default settings:
- Chunk size: 512 characters
- Overlap: 50 characters
- Hierarchical splitting: paragraphs → sentences → whitespace

### Embedding Generation

Embeddings are generated using FastEmbed ONNX models and stored as arrays of double-precision floating-point numbers in DuckDB.

## Error Handling

The system provides comprehensive error handling for:
- Invalid PDF files
- Network issues during model downloads
- Database connection problems
- File system permissions
- Model compatibility validation

## Performance Considerations

- **Memory Usage**: The system processes one document at a time to manage memory usage
- **Disk Space**: Original PDFs are stored in the database; ensure adequate storage
- **Processing Speed**: Depends on PDF complexity and chosen embedding model size
- **Concurrent Access**: DuckDB handles concurrent reads; avoid concurrent writes

## Development

### Project Structure

```
src/
├── main.rs              # CLI interface and orchestration
├── database.rs          # DuckDB operations and schema management
├── document_processor.rs # PDF text extraction and chunking
├── embedding_manager.rs  # Embedding model management
└── error.rs            # Custom error types
```

### Running Tests

```bash
cargo test
```

### Adding New Embedding Models

To add support for new FastEmbed models:

1. Update the model matching in `embedding_manager.rs`
2. Add the appropriate dimension size
3. Update the documentation

## Querying the Archive

Once documents are indexed, you can query the DuckDB archive directly:

```sql
-- Find similar fragments using cosine similarity (example)
SELECT d.filename, f.content, f.fragment_order
FROM fragments f
JOIN documents d ON f.document_id = d.id
WHERE array_cosine_similarity(f.embedding, ?) > 0.7
ORDER BY array_cosine_similarity(f.embedding, ?) DESC
LIMIT 10;
```

## License

[Add your license information here]

## Contributing

[Add contributing guidelines here]
