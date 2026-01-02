//! JavaScript bindings for CDN/embed usage
//!
//! This module provides a JavaScript-friendly API for the EPUB reader
//! that can be loaded from a CDN and used in vanilla JavaScript applications.

use wasm_bindgen::prelude::*;

use epub_reader_core::{Book, Cfi, Metadata, NavItem, SearchMatch, SearchOptions};

/// JavaScript-friendly wrapper for Book
#[wasm_bindgen]
pub struct JsBook {
    inner: Book,
}

#[wasm_bindgen]
impl JsBook {
    /// Load an EPUB from bytes
    #[wasm_bindgen(constructor)]
    pub fn new(data: &[u8]) -> Result<JsBook, JsValue> {
        let book = Book::from_bytes(data.to_vec())
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(JsBook { inner: book })
    }

    /// Get book metadata as JSON
    #[wasm_bindgen(getter)]
    pub fn metadata(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner.metadata)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get table of contents as JSON
    #[wasm_bindgen(getter)]
    pub fn toc(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner.toc)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get the number of sections
    #[wasm_bindgen(getter)]
    pub fn section_count(&self) -> usize {
        self.inner.section_count()
    }

    /// Get section metadata by index
    pub fn get_section(&self, index: usize) -> Result<JsValue, JsValue> {
        let section = self.inner.section(index)
            .ok_or_else(|| JsValue::from_str("Section not found"))?;
        serde_wasm_bindgen::to_value(section)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get section content by index
    pub fn get_section_content(&mut self, index: usize) -> Result<String, JsValue> {
        self.inner.section_content(index)
            .map(|s| s.to_string())
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get a resource by href
    pub fn get_resource(&self, href: &str) -> Option<Vec<u8>> {
        self.inner.get_resource(href).map(|d| d.to_vec())
    }

    /// Get cover image data
    pub fn get_cover(&self) -> Option<Vec<u8>> {
        self.inner.cover_image().map(|d| d.to_vec())
    }

    /// Search the book
    pub fn search(&mut self, query: &str, options: Option<JsSearchOptions>) -> Result<JsValue, JsValue> {
        let opts = options.map(|o| o.into()).unwrap_or_default();
        let matches = self.inner.search(query, &opts)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        serde_wasm_bindgen::to_value(&matches)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

/// JavaScript-friendly search options
#[wasm_bindgen]
pub struct JsSearchOptions {
    case_insensitive: bool,
    max_results: Option<usize>,
    context_chars: usize,
}

#[wasm_bindgen]
impl JsSearchOptions {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            case_insensitive: true,
            max_results: None,
            context_chars: 50,
        }
    }

    #[wasm_bindgen(setter)]
    pub fn set_case_insensitive(&mut self, value: bool) {
        self.case_insensitive = value;
    }

    #[wasm_bindgen(setter)]
    pub fn set_max_results(&mut self, value: Option<usize>) {
        self.max_results = value;
    }

    #[wasm_bindgen(setter)]
    pub fn set_context_chars(&mut self, value: usize) {
        self.context_chars = value;
    }
}

impl Default for JsSearchOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl From<JsSearchOptions> for SearchOptions {
    fn from(js: JsSearchOptions) -> Self {
        SearchOptions {
            case_insensitive: js.case_insensitive,
            max_results: js.max_results,
            context_chars: js.context_chars,
        }
    }
}

/// JavaScript-friendly CFI wrapper
#[wasm_bindgen]
pub struct JsCfi {
    inner: Cfi,
}

#[wasm_bindgen]
impl JsCfi {
    /// Parse a CFI string
    #[wasm_bindgen(constructor)]
    pub fn new(cfi_string: &str) -> Result<JsCfi, JsValue> {
        let cfi = Cfi::parse(cfi_string)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(JsCfi { inner: cfi })
    }

    /// Create a CFI for a spine index
    pub fn from_spine_index(index: usize) -> JsCfi {
        JsCfi {
            inner: Cfi::from_spine_index(index),
        }
    }

    /// Get the spine index
    #[wasm_bindgen(getter)]
    pub fn spine_index(&self) -> usize {
        self.inner.spine_index
    }

    /// Get the character offset
    #[wasm_bindgen(getter)]
    pub fn character_offset(&self) -> Option<usize> {
        self.inner.character_offset
    }

    /// Convert to string
    #[wasm_bindgen(js_name = toString)]
    pub fn to_js_string(&self) -> String {
        self.inner.to_string()
    }

    /// Compare with another CFI (-1 = less, 0 = equal, 1 = greater)
    pub fn compare(&self, other: &JsCfi) -> i32 {
        match self.inner.compare(&other.inner) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

/// Get version information
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
