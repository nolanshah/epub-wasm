//! Resources - blob URL management for embedded assets

use std::collections::HashMap;

use wasm_bindgen::prelude::*;
use web_sys::{Blob, BlobPropertyBag, Url};

/// Manages blob URLs for embedded EPUB resources, keyed by archive path.
#[derive(Default)]
pub struct Resources {
    urls: HashMap<String, String>,
}

impl Resources {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a blob URL for data
    pub fn create_blob_url(data: &[u8], mime_type: &str) -> Result<String, JsValue> {
        let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
        array.copy_from(data);

        let parts = js_sys::Array::new();
        parts.push(&array);

        let options = BlobPropertyBag::new();
        options.set_type(mime_type);

        let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &options)?;
        Url::create_object_url_with_blob(&blob)
    }

    /// Register a resource and get its blob URL (cached per archive path)
    pub fn register(&mut self, path: &str, data: &[u8], mime_type: &str) -> Result<String, JsValue> {
        if let Some(url) = self.urls.get(path) {
            return Ok(url.clone());
        }

        let url = Self::create_blob_url(data, mime_type)?;
        self.urls.insert(path.to_string(), url.clone());
        Ok(url)
    }

    /// Get an existing blob URL
    pub fn get(&self, path: &str) -> Option<&str> {
        self.urls.get(path).map(|s| s.as_str())
    }

    /// Revoke all blob URLs (call when done)
    pub fn revoke_all(&mut self) {
        for url in self.urls.values() {
            let _ = Url::revoke_object_url(url);
        }
        self.urls.clear();
    }
}

impl Drop for Resources {
    fn drop(&mut self) {
        self.revoke_all();
    }
}
