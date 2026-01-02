//! Section (spine item) handling

use serde::{Deserialize, Serialize};

use crate::package::SpineItem;

/// A section of the book (corresponds to a spine item)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    /// Spine index
    pub index: usize,
    /// ID from manifest
    pub id: String,
    /// Relative href to content
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
        }
    }

    /// Check if content is loaded
    pub fn is_loaded(&self) -> bool {
        self.content.is_some()
    }

    /// Set the content
    pub fn set_content(&mut self, content: String) {
        self.content = Some(content);
    }

    /// Get the content if loaded
    pub fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }

    /// Take ownership of the content
    pub fn take_content(&mut self) -> Option<String> {
        self.content.take()
    }

    /// Get the base path for resolving relative URLs
    pub fn base_path(&self) -> &str {
        // Get directory part of href
        match self.href.rfind('/') {
            Some(pos) => &self.href[..=pos],
            None => "",
        }
    }

    /// Resolve a relative URL against this section's base path
    pub fn resolve_url(&self, relative: &str) -> String {
        if relative.starts_with('/') || relative.contains("://") {
            return relative.to_string();
        }

        let base = self.base_path();
        if base.is_empty() {
            relative.to_string()
        } else {
            normalize_path(&format!("{}{}", base, relative))
        }
    }

    /// Extract plain text from the XHTML content
    pub fn text_content(&self) -> Option<String> {
        self.content.as_ref().map(|html| {
            let document = scraper::Html::parse_document(html);
            let body_selector = scraper::Selector::parse("body").unwrap();

            if let Some(body) = document.select(&body_selector).next() {
                body.text().collect::<Vec<_>>().join(" ")
            } else {
                // Fallback: get all text
                document.root_element().text().collect::<Vec<_>>().join(" ")
            }
        })
    }
}

/// Normalize a path by resolving . and .. components
fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();

    for part in path.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }

    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_path() {
        let spine_item = SpineItem {
            id: "test".to_string(),
            idref: "chapter1".to_string(),
            linear: true,
            properties: vec![],
        };

        let section = Section::new(
            0,
            &spine_item,
            "OEBPS/Text/chapter1.xhtml".to_string(),
            "application/xhtml+xml".to_string(),
        );

        assert_eq!(section.base_path(), "OEBPS/Text/");
    }

    #[test]
    fn test_resolve_url() {
        let spine_item = SpineItem {
            id: "test".to_string(),
            idref: "chapter1".to_string(),
            linear: true,
            properties: vec![],
        };

        let section = Section::new(
            0,
            &spine_item,
            "OEBPS/Text/chapter1.xhtml".to_string(),
            "application/xhtml+xml".to_string(),
        );

        assert_eq!(
            section.resolve_url("../Images/cover.jpg"),
            "OEBPS/Images/cover.jpg"
        );
        assert_eq!(
            section.resolve_url("chapter2.xhtml"),
            "OEBPS/Text/chapter2.xhtml"
        );
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("a/b/c"), "a/b/c");
        assert_eq!(normalize_path("a/b/../c"), "a/c");
        assert_eq!(normalize_path("a/./b/c"), "a/b/c");
        assert_eq!(normalize_path("a/b/c/../../d"), "a/d");
    }
}
