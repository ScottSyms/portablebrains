use std::fmt;

#[derive(Debug)]
pub enum PortableBrainsError {
    DatabaseError(String),
    DocumentProcessingError(String),
    EmbeddingError(String),
    ValidationError(String),
    IoError(std::io::Error),
}

impl fmt::Display for PortableBrainsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortableBrainsError::DatabaseError(msg) => write!(f, "Database error: {}", msg),
            PortableBrainsError::DocumentProcessingError(msg) => {
                write!(f, "Document processing error: {}", msg)
            }
            PortableBrainsError::EmbeddingError(msg) => write!(f, "Embedding error: {}", msg),
            PortableBrainsError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
            PortableBrainsError::IoError(err) => write!(f, "IO error: {}", err),
        }
    }
}

impl std::error::Error for PortableBrainsError {}

impl From<std::io::Error> for PortableBrainsError {
    fn from(err: std::io::Error) -> Self {
        PortableBrainsError::IoError(err)
    }
}

impl From<duckdb::Error> for PortableBrainsError {
    fn from(err: duckdb::Error) -> Self {
        PortableBrainsError::DatabaseError(err.to_string())
    }
}