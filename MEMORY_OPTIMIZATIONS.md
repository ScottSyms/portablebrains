# Memory Optimizations for Portable Brains

## Problem
The original implementation was experiencing out-of-memory errors (`malloc: Failed to allocate segment from range group - out of space`) when processing large PDF documents.

## Root Causes Identified

1. **Large PDF files loaded entirely into memory** - `std::fs::read()` loads entire PDF before processing
2. **Unbounded text extraction** - No limits on extracted text size, could extract gigabytes of text
3. **Inefficient chunking** - Converting text to `Vec<char>` doubles memory usage
4. **Batch processing bottleneck** - Processing all fragments at once without memory cleanup

## Implemented Fixes

### 1. File Size Limits
- **Pre-flight size check** - Check file size before loading (100MB default limit)
- **Early termination** - Skip oversized files with warning instead of crashing

### 2. Text Extraction Limits
- **Maximum text length** - Limit extracted text to 10M characters
- **Page-by-page monitoring** - Check memory usage after each page
- **Graceful truncation** - Stop extraction when limits reached, don't crash
- **Progress logging** - Log progress every 50 pages for large documents

### 3. Memory-Efficient Chunking
- **Byte-aware processing** - Work with UTF-8 byte boundaries to avoid string duplication
- **Streaming approach** - Process text in chunks without converting entire text to char vector
- **Early memory release** - Free large text strings as soon as chunking is complete

### 4. Batch Processing
- **Fragment batching** - Process embeddings in batches of 10 instead of all at once
- **Memory cleanup delays** - Add small delays between batches for garbage collection
- **Explicit drops** - Free PDF data and text from memory immediately after use

## Configuration Options

The `DocumentProcessor` now supports configurable limits:

```rust
// Default limits
DocumentProcessor::new()

// Custom limits  
DocumentProcessor::with_limits(
    chunk_size: 512,        // Characters per chunk
    overlap: 50,            // Overlap between chunks
    max_file_size: 50_000_000,    // 50MB file limit
    max_text_length: 5_000_000,   // 5M character limit
)
```

## Default Limits

- **Max file size**: 100MB
- **Max text length**: 10M characters  
- **Chunk size**: 512 characters
- **Chunk overlap**: 50 characters
- **Batch size**: 10 fragments

## Memory Usage Improvements

### Before Optimization:
- Large PDFs could consume several GB of RAM
- Memory usage grew linearly with document size
- No protection against malicious or corrupted large files

### After Optimization:
- Memory usage capped at reasonable limits
- Predictable memory consumption regardless of input size
- Graceful handling of large documents with warnings
- Streaming processing reduces peak memory usage

## Monitoring and Logging

Added comprehensive logging to track memory usage:

```
[INFO] Processing document: large.pdf (45.2 MB)
[DEBUG] Processing PDF with 1200 pages  
[DEBUG] Processed 50 pages, current text length: 125000 chars
[WARN] Reached maximum text length limit, stopping at page 800/1200
[DEBUG] Extracted 9999999 characters of text from 800 pages
[DEBUG] Split text into 19531 chunks
[INFO] Created 19531 fragments for document: large.pdf
[DEBUG] Processing batch 0-9 of 19531 fragments
[DEBUG] Processing batch 10-19 of 19531 fragments
...
```

## Error Handling

The system now handles memory constraints gracefully:

- **Large files**: Skip with warning instead of attempting to load
- **Memory limits**: Truncate gracefully with logging
- **Failed extractions**: Continue processing other pages
- **Batch failures**: Provide detailed error context

## Performance Impact

- **Startup time**: Slightly faster due to pre-flight size checks
- **Processing time**: Similar for normal documents, much better for large ones
- **Memory usage**: Significantly reduced and predictable
- **Reliability**: Much more stable, no more out-of-memory crashes

## Usage Recommendations

1. **Monitor logs** - Watch for truncation warnings in verbose mode
2. **Adjust limits** - Increase limits for high-memory systems if needed
3. **File preparation** - Consider splitting very large PDFs before processing
4. **System resources** - Ensure adequate disk space for database growth

The optimizations maintain full functionality while providing robust protection against memory exhaustion.