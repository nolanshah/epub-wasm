//! Section (spine item) handling

use serde::{Deserialize, Serialize};

use crate::package::SpineItem;
use crate::path;
use crate::text_map::TextMap;

/// A section of the book (corresponds to a spine item)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    /// Spine index
    pub index: usize,
    /// ID from manifest
    pub id: String,
    /// Normalized archive path of the content document
    /// (e.g. `OEBPS/Text/chapter1.xhtml`)
    pub href: String,
    /// Media type
    pub media_type: String,
    /// Whether this is a linear section
    pub linear: bool,
    /// Spine properties
    pub properties: Vec<String>,
    /// Raw content (loaded on demand)
    #[serde(skip)]
    content: Option<String>,
    /// Text/position map computed from the content (on demand)
    #[serde(skip)]
    map: Option<TextMap>,
}

impl Section {
    /// Create a section from spine and manifest items
    pub fn new(
        index: usize,
        spine_item: &SpineItem,
        href: String,
        media_type: String,
    ) -> Self {
        Self {
            index,
            id: spine_item.idref.clone(),
            href,
            media_type,
            linear: spine_item.linear,
            properties: spine_item.properties.clone(),
            content: None,
            map: None,
        }
    }

    /// Check if content is loaded
    pub fn is_loaded(&self) -> bool {
        self.content.is_some()
    }

    /// Set the content
    pub fn set_content(&mut self, content: String) {
        self.content = Some(content);
        self.map = None;
    }

    /// Get the content if loaded
    pub fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }

    /// Take ownership of the content
    pub fn take_content(&mut self) -> Option<String> {
        self.map = None;
        self.content.take()
    }

    /// Directory containing this section, with trailing slash (`""` for root)
    pub fn base_path(&self) -> &str {
        path::dir_of(&self.href)
    }

    /// Resolve an href relative to this section into `(archive_path, fragment)`.
    ///
    /// External URLs (`http://…`, `data:`, …) are returned unchanged with no fragment.
    pub fn resolve_href(&self, href: &str) -> (String, Option<String>) {
        if path::is_external(href) {
            return (href.to_string(), None);
        }
        path::resolve(self.base_path(), href)
    }

    /// Resolve a relative URL against this section's base path (path only).
    pub fn resolve_url(&self, relative: &str) -> String {
        self.resolve_href(relative).0
    }

    /// Text/position map of the section (normalized text plus offset↔CFI
    /// mappings). Cached after the first call. Returns `None` if content has
    /// not been loaded.
    pub fn map(&mut self) -> Option<&TextMap> {
        if self.map.is_none() {
            let content = self.content.as_deref()?;
            self.map = Some(TextMap::parse(content));
        }
        self.map.as_ref()
    }

    /// Plain text of the section, normalized. Cached after the first call.
    /// Returns `None` if content has not been loaded.
    pub fn text(&mut self) -> Option<&str> {
        self.map().map(|m| m.text())
    }

    /// Extract plain text from the XHTML content (uncached).
    pub fn text_content(&self) -> Option<String> {
        self.content.as_deref().map(crate::search::extract_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(href: &str) -> Section {
        let spine_item = SpineItem {
            id: "test".to_string(),
            idref: "chapter1".to_string(),
            linear: true,
            properties: vec![],
        };
        Section::new(0, &spine_item, href.to_string(), "application/xhtml+xml".to_string())
    }

    #[test]
    fn test_base_path() {
        assert_eq!(section("OEBPS/Text/chapter1.xhtml").base_path(), "OEBPS/Text/");
        assert_eq!(section("chapter1.xhtml").base_path(), "");
    }

    #[test]
    fn test_resolve_url() {
        let s = section("OEBPS/Text/chapter1.xhtml");
        assert_eq!(s.resolve_url("../Images/cover.jpg"), "OEBPS/Images/cover.jpg");
        assert_eq!(s.resolve_url("chapter2.xhtml"), "OEBPS/Text/chapter2.xhtml");
        assert_eq!(s.resolve_url("https://example.com/x"), "https://example.com/x");
        assert_eq!(
            s.resolve_href("chapter2.xhtml#sec"),
            ("OEBPS/Text/chapter2.xhtml".to_string(), Some("sec".to_string()))
        );
    }

    #[test]
    fn text_is_cached_and_reset_on_new_content() {
        let mut s = section("a.xhtml");
        assert!(s.text().is_none());
        s.set_content("<html><body><p>Hello   \n world</p></body></html>".into());
        assert_eq!(s.text(), Some("Hello world"));
        s.set_content("<html><body><p>Other</p></body></html>".into());
        assert_eq!(s.text(), Some("Other"));
    }
}
