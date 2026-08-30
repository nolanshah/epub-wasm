//! Full-text search functionality

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::cfi::Cfi;

/// Options for search
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchOptions {
    /// Case-insensitive search (default: true)
    pub case_insensitive: bool,
    /// Maximum number of results across the whole book (default: unlimited)
    pub max_results: Option<usize>,
    /// Number of context characters before/after match in the excerpt (default: 50)
    pub context_chars: usize,
}

impl SearchOptions {
    pub fn new() -> Self {
        Self {
            case_insensitive: true,
            max_results: None,
            context_chars: 50,
        }
    }
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// A search match result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    /// CFI pointing to the match location (spine-level only; see roadmap)
    pub cfi: Cfi,
    /// Section index where match was found
    pub section_index: usize,
    /// The matched text (original casing)
    pub matched_text: String,
    /// Text excerpt with surrounding context
    pub excerpt: String,
    /// Byte offset of the match within the section's plain text
    /// (as returned by `Book::section_text`)
    pub offset: usize,
}

/// Search for text in HTML content and return matches
pub fn search_content(
    content: &str,
    query: &str,
    section_index: usize,
    options: &SearchOptions,
) -> Vec<SearchMatch> {
    let text = extract_text(content);
    search_text(&text, query, section_index, options)
}

/// Search for `query` in already-extracted plain text.
///
/// Safe for arbitrary Unicode: all slicing is done on char boundaries, and
/// case-insensitive matching maps positions back to the original text even
/// when lowercasing changes byte lengths.
pub fn search_text(
    text: &str,
    query: &str,
    section_index: usize,
    options: &SearchOptions,
) -> Vec<SearchMatch> {
    let mut matches = Vec::new();

    if query.is_empty() || text.is_empty() {
        return matches;
    }

    // Build the haystack we actually scan, plus a map from haystack byte
    // offsets back to original byte offsets when lowercasing was applied.
    let (haystack, needle, map): (Cow<str>, String, Option<Vec<usize>>) =
        if options.case_insensitive {
            let (lower, map) = lowercase_with_map(text);
            (Cow::Owned(lower), query.to_lowercase(), Some(map))
        } else {
            (Cow::Borrowed(text), query.to_string(), None)
        };

    if needle.is_empty() {
        return matches;
    }

    let to_orig = |idx: usize| -> usize {
        match &map {
            Some(m) => m[idx.min(m.len() - 1)],
            None => idx,
        }
    };

    let ctx = options.context_chars;
    let mut start = 0;

    while start < haystack.len() {
        let Some(pos) = haystack[start..].find(&needle) else {
            break;
        };
        let h_start = start + pos;
        let h_end = h_start + needle.len();

        // Map back to the original text. The match covers the original chars
        // that produced haystack[h_start..h_end]; the last such char begins at
        // to_orig(h_end - 1), so the match ends at the end of that char.
        let o_start = floor_boundary(text, to_orig(h_start));
        let o_end = ceil_boundary(text, to_orig(h_end - 1) + 1);

        let matched_text = &text[o_start..o_end];

        let excerpt_start = floor_boundary(text, o_start.saturating_sub(ctx));
        let excerpt_end = ceil_boundary(text, o_end.saturating_add(ctx).min(text.len()));

        let mut excerpt = String::new();
        if excerpt_start > 0 {
            excerpt.push_str("...");
        }
        excerpt.push_str(&text[excerpt_start..excerpt_end]);
        if excerpt_end < text.len() {
            excerpt.push_str("...");
        }

        matches.push(SearchMatch {
            cfi: Cfi::from_spine_index(section_index),
            section_index,
            matched_text: matched_text.to_string(),
            excerpt,
            offset: o_start,
        });

        if let Some(max) = options.max_results {
            if matches.len() >= max {
                break;
            }
        }

        // Advance by one character (allows overlapping matches, never splits a char).
        start = ceil_boundary(&haystack, h_start + 1);
    }

    matches
}

/// Lowercase `text`, returning the lowercased string and a map from each byte
/// offset in the lowercased string to the byte offset of the originating char
/// in `text`. The map has one extra trailing entry equal to `text.len()`.
fn lowercase_with_map(text: &str) -> (String, Vec<usize>) {
    let mut lower = String::with_capacity(text.len());
    let mut map = Vec::with_capacity(text.len() + 1);

    for (orig_idx, ch) in text.char_indices() {
        for lc in ch.to_lowercase() {
            map.extend(std::iter::repeat(orig_idx).take(lc.len_utf8()));
            lower.push(lc);
        }
    }
    map.push(text.len());

    (lower, map)
}

/// Largest char boundary <= `i`.
fn floor_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Smallest char boundary >= `i`.
fn ceil_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Extract plain text from HTML content, collapsing runs of whitespace to a
/// single space.
pub(crate) fn extract_text(html: &str) -> String {
    let document = scraper::Html::parse_document(html);
    let body_selector = scraper::Selector::parse("body").unwrap();

    let pieces: Vec<&str> = match document.select(&body_selector).next() {
        Some(body) => body.text().collect(),
        None => document.root_element().text().collect(),
    };

    collapse_whitespace(&pieces.join(" "))
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;

    for ch in s.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(ch);
        }
    }

    out
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
        assert_eq!(matches[0].offset, 6);
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
    fn test_search_case_sensitive() {
        let content = r#"<html><body><p>Hello World, world.</p></body></html>"#;
        let options = SearchOptions {
            case_insensitive: false,
            ..Default::default()
        };

        let matches = search_content(content, "world", 0, &options);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "world");
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

    #[test]
    fn default_options_match_new() {
        let d = SearchOptions::default();
        assert!(d.case_insensitive);
        assert_eq!(d.context_chars, 50);
        assert_eq!(d.max_results, None);
    }

    #[test]
    fn empty_query_returns_nothing() {
        assert!(search_text("some text", "", 0, &SearchOptions::new()).is_empty());
    }

    #[test]
    fn multibyte_in_context_window_does_not_panic() {
        // Curly quote and ellipsis sit inside the 50-char context window
        // around the match, at offsets that are not char boundaries.
        let text = "Alice’s Adventures… were many and the rabbit was late again and again";
        let matches = search_text(text, "rabbit", 0, &SearchOptions::new());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "rabbit");
        assert!(matches[0].excerpt.contains("Alice’s"));
        assert!(matches[0].excerpt.contains("rabbit"));
    }

    #[test]
    fn small_context_lands_inside_multibyte_char() {
        // context_chars=1 puts excerpt_start one byte into the 3-byte '’'
        let text = "a’rabbit’b";
        let opts = SearchOptions {
            context_chars: 1,
            ..Default::default()
        };
        let matches = search_text(text, "rabbit", 0, &opts);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].excerpt, "...’rabbit’...");
    }

    #[test]
    fn match_starting_with_multibyte_char() {
        // The old code advanced `start` by one byte after a match, which is
        // not a char boundary when the match begins with a multibyte char.
        let text = "é and é and é";
        let matches = search_text(text, "é", 0, &SearchOptions::new());
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].offset, 0);
        assert_eq!(matches[1].offset, "é and ".len());
    }

    #[test]
    fn lowercasing_that_changes_byte_length() {
        // 'İ' (2 bytes) lowercases to "i̇" (3 bytes): offsets in the
        // lowercased haystack no longer line up with the original text.
        let text = "Visit İstanbul today";
        let matches = search_text(text, "istanbul", 0, &SearchOptions::new());
        assert_eq!(matches.len(), 0, "i̇ (dotted) != i; no match expected");

        let matches = search_text(text, "i̇stanbul", 0, &SearchOptions::new());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "İstanbul");
        assert_eq!(matches[0].offset, "Visit ".len());
        assert_eq!(matches[0].excerpt, "Visit İstanbul today");
    }

    #[test]
    fn overlapping_matches_are_found() {
        let matches = search_text("aaa", "aa", 0, &SearchOptions::new());
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn whitespace_is_collapsed_in_extracted_text() {
        let html = "<html><body><p>one\n\n   two</p>\n<p>three</p></body></html>";
        assert_eq!(extract_text(html), "one two three");
    }
}
