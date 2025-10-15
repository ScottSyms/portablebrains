use anyhow::{Context, Result};
use lopdf::Document;
use regex::Regex;
use log::{debug, warn};


pub struct DocumentProcessor {
    chunk_size: usize,
    overlap: usize,
    cleanup_regex: Regex,
    max_file_size: usize,      // Maximum file size to process (in bytes)
    max_text_length: usize,    // Maximum extracted text length (in chars)
}

impl DocumentProcessor {
    pub fn new() -> Self {
        // Regex to clean up extracted text
        let cleanup_regex = Regex::new(r"\s+").unwrap();
        
        Self {
            chunk_size: 512,
            overlap: 50,
            cleanup_regex,
            max_file_size: 100 * 1024 * 1024,  // 100MB max file size
            max_text_length: 10_000_000,        // 10M characters max
        }
    }
    
    pub fn with_limits(chunk_size: usize, overlap: usize, max_file_size: usize, max_text_length: usize) -> Self {
        let cleanup_regex = Regex::new(r"\s+").unwrap();
        
        Self {
            chunk_size,
            overlap,
            cleanup_regex,
            max_file_size,
            max_text_length,
        }
    }
    
    /// Extract text from PDF with memory limits and streaming processing
    pub fn extract_text_from_pdf(&self, pdf_data: &[u8]) -> Result<String> {
        // Check file size limit
        if pdf_data.len() > self.max_file_size {
            anyhow::bail!(
                "PDF file too large: {} bytes (max: {} bytes)", 
                pdf_data.len(), 
                self.max_file_size
            );
        }
        
        let document = Document::load_mem(pdf_data)
            .context("Failed to load PDF document")?;
        
        let mut text_content = String::new();
        let page_count = document.get_pages().len();
        
        debug!("Processing PDF with {} pages", page_count);
        
        // Extract text from each page with memory monitoring
        for page_num in 1..=page_count {
            // Check if we're approaching text length limit
            if text_content.len() > self.max_text_length {
                warn!("Reached maximum text length limit, stopping at page {}/{}", page_num - 1, page_count);
                break;
            }
            
            match document.extract_text(&[page_num as u32]) {
                Ok(page_text) => {
                    // Only add page text if it won't exceed our limit
                    if text_content.len() + page_text.len() <= self.max_text_length {
                        text_content.push_str(&page_text);
                        text_content.push('\n');
                    } else {
                        warn!("Page {} would exceed text limit, truncating document", page_num);
                        // Add as much as we can
                        let remaining_capacity = self.max_text_length.saturating_sub(text_content.len());
                        if remaining_capacity > 0 {
                            let truncated_text: String = page_text.chars().take(remaining_capacity).collect();
                            text_content.push_str(&truncated_text);
                        }
                        break;
                    }
                }
                Err(e) => {
                    debug!("Failed to extract text from page {}: {}", page_num, e);
                    // Continue with other pages
                }
            }
            
            // Periodic memory cleanup hint for large documents
            if page_num % 50 == 0 {
                debug!("Processed {} pages, current text length: {} chars", page_num, text_content.len());
            }
        }
        
        if text_content.trim().is_empty() {
            anyhow::bail!("No text could be extracted from PDF");
        }
        
        // Clean up the extracted text
        let cleaned_text = self.cleanup_text(&text_content);
        
        debug!("Extracted {} characters of text from {} pages", cleaned_text.len(), page_count);
        Ok(cleaned_text)
    }
    
    /// Chunk text with memory-efficient processing
    pub fn chunk_text(&self, text: &str) -> anyhow::Result<Vec<String>> {
        let mut chunks = Vec::new();
        
        if text.is_empty() {
            return Ok(chunks);
        }
        
        debug!("Starting sentence-based chunking of {} chars", text.len());
        
        // Split text into sentences first
        let sentences = self.split_into_sentences(text);
        debug!("Found {} sentences", sentences.len());
        
        let mut current_chunk = String::new();
        let mut i = 0;
        
        while i < sentences.len() {
            let sentence = &sentences[i];
            
            // If adding this sentence would exceed chunk size, finalize current chunk
            if !current_chunk.is_empty() && 
               (current_chunk.len() + sentence.len() + 1) > self.chunk_size {
                
                let trimmed_chunk = current_chunk.trim();
                if !trimmed_chunk.is_empty() && trimmed_chunk.len() > 10 {
                    chunks.push(trimmed_chunk.to_string());
                }
                
                // Start new chunk with overlap
                current_chunk = self.create_overlap_chunk(&chunks, &sentences, i);
            }
            
            // Add current sentence to chunk
            if !current_chunk.is_empty() {
                current_chunk.push(' ');
            }
            current_chunk.push_str(sentence);
            
            i += 1;
        }
        
        // Add final chunk if it has content
        let final_chunk = current_chunk.trim();
        if !final_chunk.is_empty() && final_chunk.len() > 10 {
            chunks.push(final_chunk.to_string());
        }
        
        debug!("Created {} chunks using sentence-based segmentation", chunks.len());
        Ok(chunks)
    }
    
    fn split_into_sentences(&self, text: &str) -> Vec<String> {
        let mut sentences = Vec::new();
        let mut current_sentence = String::new();
        
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            
            // Split on sentence boundaries
            let mut chars = line.chars().peekable();
            while let Some(ch) = chars.next() {
                current_sentence.push(ch);
                
                // Check for sentence ending
                if matches!(ch, '.' | '!' | '?') {
                    // Look ahead to see if this is really a sentence end
                    if let Some(&next_ch) = chars.peek() {
                        if next_ch.is_whitespace() || next_ch.is_uppercase() {
                            // This looks like a real sentence boundary
                            let sentence = current_sentence.trim().to_string();
                            if !sentence.is_empty() && sentence.len() > 5 {
                                sentences.push(sentence);
                            }
                            current_sentence.clear();
                        }
                    } else {
                        // End of line after sentence punctuation
                        let sentence = current_sentence.trim().to_string();
                        if !sentence.is_empty() && sentence.len() > 5 {
                            sentences.push(sentence);
                        }
                        current_sentence.clear();
                    }
                }
            }
            
            // If we have leftover content at end of line, treat as sentence
            if !current_sentence.trim().is_empty() {
                current_sentence.push(' '); // Add space between lines
            }
        }
        
        // Add any remaining content as final sentence
        let final_sentence = current_sentence.trim();
        if !final_sentence.is_empty() && final_sentence.len() > 5 {
            sentences.push(final_sentence.to_string());
        }
        
        sentences
    }
    
    fn create_overlap_chunk(&self, chunks: &[String], sentences: &[String], current_index: usize) -> String {
        if chunks.is_empty() || self.overlap == 0 {
            return String::new();
        }
        
        // Try to create overlap by including some sentences from the previous chunk
        let mut overlap_text = String::new();
        let mut overlap_chars = 0;
        
        // Work backwards from current sentence to find overlap content
        for i in (0..current_index).rev() {
            let sentence = &sentences[i];
            if overlap_chars + sentence.len() + 1 <= self.overlap {
                if !overlap_text.is_empty() {
                    overlap_text = format!("{} {}", sentence, overlap_text);
                } else {
                    overlap_text = sentence.clone();
                }
                overlap_chars += sentence.len() + 1;
            } else {
                break;
            }
        }
        
        overlap_text
    }
    
    fn cleanup_text(&self, text: &str) -> String {
        // Remove excessive whitespace and normalize line breaks
        let normalized = self.cleanup_regex.replace_all(text, " ");
        
        // Remove control characters but keep basic punctuation
        let cleaned: String = normalized
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .collect();
        
        // Normalize paragraph breaks
        let with_paragraphs = cleaned
            .split('\n')
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        
        with_paragraphs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_text_cleanup() {
        let processor = DocumentProcessor::new();
        let messy_text = "This   is    a\n\n\ntest    text\twith\nexcessive\n\n   whitespace.";
        let cleaned = processor.cleanup_text(messy_text);
        
        assert_eq!(cleaned, "This is a\n\ntest text with\n\nexcessive\n\nwhitespace.");
    }
    
    #[test]
    fn test_text_chunking() {
        let processor = DocumentProcessor::new();
        let text = "This is a test document. It has multiple sentences. ".repeat(20);
        let chunks = processor.chunk_text(&text).unwrap();
        
        assert!(!chunks.is_empty());
        
        // Check that chunks have reasonable length
        for chunk in &chunks {
            assert!(chunk.len() <= 600); // Should be around 512 + some extra
            assert!(chunk.len() >= 10); // Should filter out very short chunks
        }
    }
}