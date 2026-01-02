//! ZIP archive handling for EPUB files

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

use crate::error::{EpubError, Result};

/// Wrapper around a ZIP archive that provides EPUB-specific access methods
pub struct Archive {
    files: HashMap<String, Vec<u8>>,
}

impl Archive {
    /// Open an EPUB from a file path
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let mut archive = ZipArchive::new(file)?;
        Self::load_files(&mut archive)
    }

    /// Open an EPUB from bytes (useful for WASM)
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let cursor = Cursor::new(bytes);
        let mut archive = ZipArchive::new(cursor)?;
        Self::load_files(&mut archive)
    }

    fn load_files<R: Read + std::io::Seek>(archive: &mut ZipArchive<R>) -> Result<Self> {
        let mut files = HashMap::new();

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            if file.is_file() {
                let name = file.name().to_string();
                let mut contents = Vec::new();
                file.read_to_end(&mut contents)?;
                files.insert(name, contents);
            }
        }

        Ok(Self { files })
    }

    /// Get a file from the archive as bytes
    pub fn get_file(&self, path: &str) -> Option<&[u8]> {
        // Normalize path - remove leading slash if present
        let normalized = path.trim_start_matches('/');
        self.files.get(normalized).map(|v| v.as_slice())
    }

    /// Get a file from the archive as a UTF-8 string
    pub fn get_file_string(&self, path: &str) -> Result<String> {
        let bytes = self
            .get_file(path)
            .ok_or_else(|| EpubError::MissingFile(path.to_string()))?;

        String::from_utf8(bytes.to_vec())
            .map_err(|e| EpubError::InvalidStructure(format!("Invalid UTF-8 in {}: {}", path, e)))
    }

    /// Check if a file exists in the archive
    pub fn has_file(&self, path: &str) -> bool {
        let normalized = path.trim_start_matches('/');
        self.files.contains_key(normalized)
    }

    /// List all files in the archive
    pub fn list_files(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(|s| s.as_str())
    }

    /// Get all files matching a predicate
    pub fn files_matching<F>(&self, predicate: F) -> Vec<&str>
    where
        F: Fn(&str) -> bool,
    {
        self.files
            .keys()
            .filter(|k| predicate(k))
            .map(|s| s.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        // Test that paths are normalized consistently
        let archive = Archive {
            files: HashMap::from([(
                "OEBPS/content.opf".to_string(),
                b"test".to_vec(),
            )]),
        };

        assert!(archive.has_file("OEBPS/content.opf"));
        assert!(archive.has_file("/OEBPS/content.opf"));
    }
}
