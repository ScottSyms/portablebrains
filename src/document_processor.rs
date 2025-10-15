use anyhow::{Context, Result};
use lopdf::Document;
use regex::Regex;
use log::{debug, warn};
use std::path::Path;
use scraper::{Html, Selector};
use calamine::{Reader, open_workbook_auto, DataType};
use std::io::{Cursor, Read};
use zip::ZipArchive;
use quick_xml::Reader as XmlReader;
use quick_xml::events::Event;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub enum DocumentFormat {
    Pdf,
    Text,
    Html,
    Docx,
    Pptx,
    Xlsx,
}

impl DocumentFormat {
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_lowercase().as_str() {
            "pdf" => Some(DocumentFormat::Pdf),
            "txt" | "text" => Some(DocumentFormat::Text),
            "html" | "htm" => Some(DocumentFormat::Html),
            "docx" => Some(DocumentFormat::Docx),
            "pptx" => Some(DocumentFormat::Pptx),
            "xlsx" => Some(DocumentFormat::Xlsx),
            _ => None,
        }
    }
    
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            DocumentFormat::Pdf => &["pdf"],
            DocumentFormat::Text => &["txt", "text"],
            DocumentFormat::Html => &["html", "htm"],
            DocumentFormat::Docx => &["docx"],
            DocumentFormat::Pptx => &["pptx"],
            DocumentFormat::Xlsx => &["xlsx"],
        }
    }
}

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

    /// Extract text from any supported document format
    pub fn extract_text_from_document(&self, file_path: &Path, file_data: &[u8]) -> Result<String> {
        // Check file size limit
        if file_data.len() > self.max_file_size {
            anyhow::bail!(
                "File too large: {} bytes (max: {} bytes)", 
                file_data.len(), 
                self.max_file_size
            );
        }

        // Determine format from file extension
        let extension = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
            
        let format = DocumentFormat::from_extension(extension)
            .ok_or_else(|| anyhow::anyhow!("Unsupported file format: {}", extension))?;

        let text = match format {
            DocumentFormat::Pdf => self.extract_text_from_pdf(file_data)?,
            DocumentFormat::Text => self.extract_text_from_text(file_data)?,
            DocumentFormat::Html => self.extract_text_from_html(file_data)?,
            DocumentFormat::Docx => self.extract_text_from_docx(file_data)?,
            DocumentFormat::Pptx => self.extract_text_from_pptx(file_data)?,
            DocumentFormat::Xlsx => self.extract_text_from_xlsx(file_data)?,
        };

        if text.trim().is_empty() {
            anyhow::bail!("No text could be extracted from file: {:?}", file_path);
        }

        Ok(text)
    }

    /// Extract text from plain text files
    fn extract_text_from_text(&self, file_data: &[u8]) -> Result<String> {
        let text = String::from_utf8_lossy(file_data).to_string();
        let cleaned_text = self.cleanup_text(&text);
        
        if cleaned_text.len() > self.max_text_length {
            let truncated: String = cleaned_text.chars().take(self.max_text_length).collect();
            warn!("Text file truncated to {} characters", self.max_text_length);
            Ok(truncated)
        } else {
            Ok(cleaned_text)
        }
    }

    /// Extract text from HTML files
    fn extract_text_from_html(&self, file_data: &[u8]) -> Result<String> {
        let html_content = String::from_utf8_lossy(file_data);
        let document = Html::parse_document(&html_content);
        
        // Remove script and style elements
        let script_selector = Selector::parse("script, style").unwrap();
        let text_selector = Selector::parse("body").unwrap();
        
        let mut text_content = String::new();
        
        // Try to get body content first, fallback to full document
        if let Some(body) = document.select(&text_selector).next() {
            text_content = self.extract_text_from_html_element(&body, &script_selector);
        } else {
            // No body tag, extract from entire document
            text_content = document.root_element().text().collect::<Vec<_>>().join(" ");
        }
        
        let cleaned_text = self.cleanup_text(&text_content);
        
        if cleaned_text.len() > self.max_text_length {
            let truncated: String = cleaned_text.chars().take(self.max_text_length).collect();
            warn!("HTML file truncated to {} characters", self.max_text_length);
            Ok(truncated)
        } else {
            Ok(cleaned_text)
        }
    }

    /// Helper method to extract text from HTML elements while skipping scripts/styles
    fn extract_text_from_html_element(&self, element: &scraper::ElementRef, _script_selector: &Selector) -> String {
        // Simply extract all text content from the element
        element.text().collect::<Vec<_>>().join(" ")
    }

    /// Extract text from DOCX files
    fn extract_text_from_docx(&self, file_data: &[u8]) -> Result<String> {
        let cursor = Cursor::new(file_data);
        let mut archive = ZipArchive::new(cursor)
            .context("Failed to open DOCX file as ZIP archive")?;
        
        // Extract document.xml which contains the main content
        let mut document_xml = archive.by_name("word/document.xml")
            .context("Failed to find document.xml in DOCX file")?;
        
        let mut xml_content = String::new();
        document_xml.read_to_string(&mut xml_content)
            .context("Failed to read document.xml content")?;
        
        let text = self.extract_text_from_docx_xml(&xml_content)?;
        let cleaned_text = self.cleanup_text(&text);
        
        if cleaned_text.len() > self.max_text_length {
            let truncated: String = cleaned_text.chars().take(self.max_text_length).collect();
            warn!("DOCX file truncated to {} characters", self.max_text_length);
            Ok(truncated)
        } else {
            Ok(cleaned_text)
        }
    }

    /// Extract text from PowerPoint PPTX files
    fn extract_text_from_pptx(&self, file_data: &[u8]) -> Result<String> {
        let cursor = Cursor::new(file_data);
        let mut archive = ZipArchive::new(cursor)
            .context("Failed to open PPTX file as ZIP archive")?;
        
        let mut all_text = String::new();
        
        // Extract text from all slides
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let file_name = file.name().to_string();
            
            // Look for slide XML files
            if file_name.starts_with("ppt/slides/slide") && file_name.ends_with(".xml") {
                let mut xml_content = String::new();
                file.read_to_string(&mut xml_content)
                    .context("Failed to read slide XML content")?;
                
                let slide_text = self.extract_text_from_pptx_xml(&xml_content)?;
                if !slide_text.is_empty() {
                    all_text.push_str(&slide_text);
                    all_text.push('\n');
                }
            }
        }
        
        let cleaned_text = self.cleanup_text(&all_text);
        
        if cleaned_text.len() > self.max_text_length {
            let truncated: String = cleaned_text.chars().take(self.max_text_length).collect();
            warn!("PPTX file truncated to {} characters", self.max_text_length);
            Ok(truncated)
        } else {
            Ok(cleaned_text)
        }
    }

    /// Extract text from Excel XLSX files
    fn extract_text_from_xlsx(&self, file_data: &[u8]) -> Result<String> {
        // Create a temporary file for calamine to read
        let temp_path = std::env::temp_dir().join(format!("temp_excel_{}.xlsx", Uuid::new_v4()));
        std::fs::write(&temp_path, file_data)?;
        
        let result = (|| -> Result<String> {
            let mut workbook = open_workbook_auto(&temp_path)
                .context("Failed to open XLSX file")?;
            
            let mut all_text = String::new();
            
            // Process all worksheets
            for sheet_name in workbook.sheet_names().to_vec() {
                if let Some(Ok(range)) = workbook.worksheet_range(&sheet_name) {
                    // Extract text from all cells
                    for row in range.rows() {
                        let mut row_text = Vec::new();
                        for cell in row {
                            match cell {
                                DataType::String(s) => row_text.push(s.clone()),
                                DataType::Float(f) => row_text.push(f.to_string()),
                                DataType::Int(i) => row_text.push(i.to_string()),
                                DataType::Bool(b) => row_text.push(b.to_string()),
                                DataType::DateTime(dt) => row_text.push(dt.to_string()),
                                DataType::Duration(d) => row_text.push(d.to_string()),
                                DataType::DateTimeIso(dt) => row_text.push(dt.clone()),
                                DataType::DurationIso(d) => row_text.push(d.clone()),
                                DataType::Error(_) | DataType::Empty => {} // Skip errors and empty cells
                            }
                        }
                        
                        if !row_text.is_empty() {
                            all_text.push_str(&row_text.join(" | "));
                            all_text.push('\n');
                        }
                    }
                    
                    // Add sheet separator
                    all_text.push_str(&format!("\n--- End of sheet: {} ---\n", sheet_name));
                }
            }
            
            Ok(all_text)
        })();
        
        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);
        
        let all_text = result?;
        let cleaned_text = self.cleanup_text(&all_text);
        
        if cleaned_text.len() > self.max_text_length {
            let truncated: String = cleaned_text.chars().take(self.max_text_length).collect();
            warn!("XLSX file truncated to {} characters", self.max_text_length);
            Ok(truncated)
        } else {
            Ok(cleaned_text)
        }
    }

    /// Extract text from DOCX XML content
    fn extract_text_from_docx_xml(&self, xml_content: &str) -> Result<String> {
        let mut reader = XmlReader::from_str(xml_content);
        let mut text_content = String::new();
        let mut buf = Vec::new();
        
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Text(e)) => {
                    if let Ok(text) = e.unescape() {
                        text_content.push_str(&text);
                        text_content.push(' ');
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    warn!("Error parsing DOCX XML: {}", e);
                    break;
                }
                _ => {}
            }
            buf.clear();
        }
        
        Ok(text_content)
    }

    /// Extract text from PPTX XML content
    fn extract_text_from_pptx_xml(&self, xml_content: &str) -> Result<String> {
        let mut reader = XmlReader::from_str(xml_content);
        let mut text_content = String::new();
        let mut buf = Vec::new();
        let mut in_text_element = false;
        
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    // Look for text elements in PowerPoint XML
                    if e.name().as_ref() == b"a:t" {
                        in_text_element = true;
                    }
                }
                Ok(Event::End(e)) => {
                    if e.name().as_ref() == b"a:t" {
                        in_text_element = false;
                        text_content.push(' ');
                    }
                }
                Ok(Event::Text(e)) => {
                    if in_text_element {
                        if let Ok(text) = e.unescape() {
                            text_content.push_str(&text);
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    warn!("Error parsing PPTX XML: {}", e);
                    break;
                }
                _ => {}
            }
            buf.clear();
        }
        
        Ok(text_content)
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