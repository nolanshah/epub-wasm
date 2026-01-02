//! Full-text search functionality

use serde::{Deserialize, Serialize};

use crate::cfi::Cfi;

/// Options for search
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// Case-insensitive search
    pub case_insensitive: bool,
    /// Maximum number of results
    pub max_results: Option<usize>,
    /// Number of context characters before/after match
    pub context_chars: usize,
}

impl SearchOptions {
    pub fn new() -> Self {
        Self {
            case_insensitive: true,
            context_chars: 50,
            max_results: None,
        }
    }
}

/// A search match result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    /// CFI pointing to the match location
    pub cfi: Cfi,
    /// Section index where match was found
    pub section_index: usize,
    /// The matched text
    pub matched_text: String,
    /// Text excerpt with surrounding context
    pub excerpt: String,
    /// Character offset within the section's text content
    pub offset: usize,
}

/// Search for text in content and return matches
pub fn search_content(
    content: &str,
    query: &str,
    section_index: usize,
    options: &SearchOptions,
) -> Vec<SearchMatch> {
    let mut matches = Vec::new();

    // Extract plain text from HTML
    let text = extract_text(content);

    let search_text = if options.case_insensitive {
        text.to_lowercase()
    } else {
        text.clone()
    };

    let search_query = if options.case_insensitive {
        query.to_lowercase()
    } else {
        query.to_string()
    };

    let mut search_start = 0;

    while let Some(pos) = search_text[search_start..].find(&search_query) {
        let absolute_pos = search_start + pos;

        // Get the actual matched text (preserving original case)
        let matched_text = &text[absolute_pos..absolute_pos + query.len()];

        // Build excerpt with context
        let excerpt_start = absolute_pos.saturating_sub(options.context_chars);
        let excerpt_end = (absolute_pos + query.len() + options.context_chars).min(text.len());

        let mut excerpt = String::new();
        if excerpt_start > 0 {
            excerpt.push_str("...");
        }
        excerpt.push_str(&text[excerpt_start..excerpt_end]);
        if excerpt_end < text.len() {
            excerpt.push_str("...");
        }

        // Create CFI for this match
        // Note: This is a simplified CFI - full implementation would need DOM traversal
        let cfi = Cfi::from_spine_index(section_index);

        matches.push(SearchMatch {
            cfi,
            section_index,
            matched_text: matched_text.to_string(),
            excerpt,
            offset: absolute_pos,
        });

        // Check max results
        if let Some(max) = options.max_results {
            if matches.len() >= max {
                break;
            }
        }

        search_start = absolute_pos + 1;
    }

    matches
}

/// Extract plain text from HTML content
fn extract_text(html: &str) -> String {
    let document = scraper::Html::parse_document(html);
    let body_selector = scraper::Selector::parse("body").unwrap();

    if let Some(body) = document.select(&body_selector).next() {
        body.text().collect::<Vec<_>>().join(" ")
    } else {
        document
            .root_element()
            .text()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_simple() {
        let content = r#"<html><body><p>Hello world, this is a test.</p></body></html>"#;
        let options = SearchOptions::new();

        let matches = search_content(content, "world", 0, &options);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "world");
    }

    #[test]
    fn test_search_case_insensitive() {
        let content = r#"<html><body><p>Hello World, this is a test.</p></body></html>"#;
        let options = SearchOptions {
            case_insensitive: true,
            ..Default::default()
        };

        let matches = search_content(content, "world", 0, &options);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "World");
    }

    #[test]
    fn test_search_multiple_matches() {
        let content = r#"<html><body><p>Test one. Test two. Test three.</p></body></html>"#;
        let options = SearchOptions::new();

        let matches = search_content(content, "test", 0, &options);
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_search_max_results() {
        let content = r#"<html><body><p>Test one. Test two. Test three.</p></body></html>"#;
        let options = SearchOptions {
            case_insensitive: true,
            max_results: Some(2),
            ..Default::default()
        };

        let matches = search_content(content, "test", 0, &options);
        assert_eq!(matches.len(), 2);
    }
}
