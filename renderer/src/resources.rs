//! Resources - blob URL management for embedded assets

use std::collections::HashMap;

use wasm_bindgen::prelude::*;
use web_sys::{Blob, BlobPropertyBag, Url};

/// Manages blob URLs for embedded EPUB resources
pub struct Resources {
    /// Map from original href to blob URL
    urls: HashMap<String, String>,
}

impl Resources {
    /// Create a new resource manager
    pub fn new() -> Self {
        Self {
            urls: HashMap::new(),
        }
    }

    /// Create a blob URL for data
    pub fn create_blob_url(data: &[u8], mime_type: &str) -> Result<String, JsValue> {
        // Create Uint8Array from data
        let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
        array.copy_from(data);

        // Create blob
        let parts = js_sys::Array::new();
        parts.push(&array);

        let mut options = BlobPropertyBag::new();
        options.type_(mime_type);

        let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &options)?;

        // Create URL
        Url::create_object_url_with_blob(&blob)
    }

    /// Register a resource and get its blob URL
    pub fn register(&mut self, href: &str, data: &[u8], mime_type: &str) -> Result<String, JsValue> {
        if let Some(url) = self.urls.get(href) {
            return Ok(url.clone());
        }

        let url = Self::create_blob_url(data, mime_type)?;
        self.urls.insert(href.to_string(), url.clone());
        Ok(url)
    }

    /// Get an existing blob URL
    pub fn get(&self, href: &str) -> Option<&str> {
        self.urls.get(href).map(|s| s.as_str())
    }

    /// Revoke all blob URLs (call when done)
    pub fn revoke_all(&mut self) {
        for url in self.urls.values() {
            let _ = Url::revoke_object_url(url);
        }
        self.urls.clear();
    }

    /// Revoke a specific blob URL
    pub fn revoke(&mut self, href: &str) {
        if let Some(url) = self.urls.remove(href) {
            let _ = Url::revoke_object_url(&url);
        }
    }

    /// Replace hrefs in HTML content with blob URLs
    pub fn rewrite_urls(&self, html: &str) -> String {
        let mut result = html.to_string();

        for (href, blob_url) in &self.urls {
            // Replace src attributes
            result = result.replace(&format!("src=\"{}\"", href), &format!("src=\"{}\"", blob_url));
            result = result.replace(&format!("src='{}'", href), &format!("src='{}'", blob_url));

            // Replace href attributes (for stylesheets)
            result = result.replace(&format!("href=\"{}\"", href), &format!("href=\"{}\"", blob_url));
            result = result.replace(&format!("href='{}'", href), &format!("href='{}'", blob_url));

            // Replace url() in CSS
            result = result.replace(&format!("url({})", href), &format!("url({})", blob_url));
            result = result.replace(&format!("url(\"{}\")", href), &format!("url(\"{}\")", blob_url));
            result = result.replace(&format!("url('{}')", href), &format!("url('{}')", blob_url));
        }

        result
    }
}

impl Default for Resources {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Resources {
    fn drop(&mut self) {
        self.revoke_all();
    }
}

/// Detect MIME type from file extension
pub fn mime_type_from_extension(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();

    match ext.as_str() {
        // Images
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",

        // Fonts
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",

        // Styles
        "css" => "text/css",

        // Scripts
        "js" => "application/javascript",

        // Documents
        "xhtml" | "html" | "htm" => "application/xhtml+xml",
        "xml" => "application/xml",

        // Default
        _ => "application/octet-stream",
    }
}
