//! Error types for EPUB parsing

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EpubError {
    #[error("Failed to read EPUB archive: {0}")]
    ArchiveError(#[from] zip::result::ZipError),

    #[error("Failed to read file from archive: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to parse XML: {0}")]
    XmlError(String),

    #[error("Missing required file: {0}")]
    MissingFile(String),

    #[error("Invalid EPUB structure: {0}")]
    InvalidStructure(String),

    #[error("Invalid CFI: {0}")]
    InvalidCfi(String),

    #[error("Content not found: {0}")]
    ContentNotFound(String),
}

impl From<quick_xml::Error> for EpubError {
    fn from(err: quick_xml::Error) -> Self {
        EpubError::XmlError(err.to_string())
    }
}

impl From<quick_xml::events::attributes::AttrError> for EpubError {
    fn from(err: quick_xml::events::attributes::AttrError) -> Self {
        EpubError::XmlError(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EpubError>;
