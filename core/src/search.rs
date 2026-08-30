//! Full-text search functionality

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::cfi::Cfi;
use crate::text_map::TextMap;

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
    /// CFI string for the match. A range CFI targeting the matched text when
    /// document structure was available (`Book::search`, `search_content`);
    /// a spine-level point CFI otherwise.
    pub cfi: String,
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

/// Find `query` in `text`, returning `(byte_offset, byte_len)` pairs.
///
/// Safe for arbitrary Unicode: all offsets are char boundaries in `text`,
/// and case-insensitive matching maps positions back correctly even when
/// lowercasing changes byte lengths.
pub fn find_matches(text: &str, query: &str, options: &SearchOptions) -> Vec<(usize, usize)> {
    let mut out = Vec::new();

    if query.is_empty() || text.is_empty() {
        return out;
    }

    let (haystack, needle, map): (Cow<str>, String, Option<Vec<usize>>) =
        if options.case_insensitive {
            let (lower, map) = lowercase_with_map(text);
            (Cow::Owned(lower), query.to_lowercase(), Some(map))
        } else {
            (Cow::Borrowed(text), query.to_string(), None)
        };

    if needle.is_empty() {
        return out;
    }

    let to_orig = |idx: usize| -> usize {
        match &map {
            Some(m) => m[idx.min(m.len() - 1)],
            None => idx,
        }
    };

    let mut start = 0;
    while start < haystack.len() {
        let Some(pos) = haystack[start..].find(&needle) else {
            break;
        };
        let h_start = start + pos;
        let h_end = h_start + needle.len();

        // Map back to the original text. The match covers the original chars
        // that produced haystack[h_start..h_end]; the last such char begins
        // at to_orig(h_end - 1), so the match ends at the end of that char.
        let o_start = floor_boundary(text, to_orig(h_start));
        let o_end = ceil_boundary(text, to_orig(h_end - 1) + 1);

        out.push((o_start, o_end - o_start));

        if let Some(max) = options.max_results {
            if out.len() >= max {
                break;
            }
        }

        // Advance by one character (allows overlapping matches, never splits a char).
        start = ceil_boundary(&haystack, h_start + 1);
    }

    out
}

fn build_match(
    text: &str,
    offset: usize,
    len: usize,
    cfi: String,
    section_index: usize,
    options: &SearchOptions,
) -> SearchMatch {
    let end = offset + len;
    let ctx = options.context_chars;

    let excerpt_start = floor_boundary(text, offset.saturating_sub(ctx));
    let excerpt_end = ceil_boundary(text, end.saturating_add(ctx).min(text.len()));

    let mut excerpt = String::new();
    if excerpt_start > 0 {
        excerpt.push_str("...");
    }
    excerpt.push_str(&text[excerpt_start..excerpt_end]);
    if excerpt_end < text.len() {
        excerpt.push_str("...");
    }

    SearchMatch {
        cfi,
        section_index,
        matched_text: text[offset..end].to_string(),
        excerpt,
        offset,
    }
}

/// Search plain text. Matches carry spine-level point CFIs (no document
/// structure is available here); use `search_content` or `Book::search` for
/// precise range CFIs.
pub fn search_text(
    text: &str,
    query: &str,
    section_index: usize,
    options: &SearchOptions,
) -> Vec<SearchMatch> {
    let point = Cfi::from_spine_index(section_index).to_string();
    find_matches(text, query, options)
        .into_iter()
        .map(|(o, l)| build_match(text, o, l, point.clone(), section_index, options))
        .collect()
}

/// Search HTML content. Matches carry range CFIs targeting the matched text.
pub fn search_content(
    content: &str,
    query: &str,
    section_index: usize,
    options: &SearchOptions,
) -> Vec<SearchMatch> {
    let map = TextMap::parse(content);
    matches_in_map(&map, query, section_index, options)
}

/// Search an already-scanned document.
pub fn matches_in_map(
    map: &TextMap,
    query: &str,
    section_index: usize,
    options: &SearchOptions,
) -> Vec<SearchMatch> {
    let text = map.text();
    let fallback = Cfi::from_spine_index(section_index).to_string();

    find_matches(text, query, options)
        .into_iter()
        .map(|(o, l)| {
            let cfi = map
                .cfi_range(section_index, o, o + l)
                .map(|r| r.to_string())
                .unwrap_or_else(|| fallback.clone());
            build_match(text, o, l, cfi, section_index, options)
        })
        .collect()
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

/// Extract normalized plain text from HTML content: entities decoded,
/// whitespace collapsed, block boundaries becoming single spaces.
pub(crate) fn extract_text(html: &str) -> String {
    TextMap::parse(html).into_text()
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
        // Precise range CFI into the paragraph's first text chunk
        // (no <head> in this doc, so body is element child 1 → step 2)
        assert_eq!(matches[0].cfi, "epubcfi(/6/2!/2/2,/1:6,/1:11)");
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
    fn matches_across_inline_elements_are_found() {
        // The old scraper-based extraction inserted a space at EVERY tag
        // boundary, making words split by inline markup unsearchable.
        let content = "<p>He<b>ll</b>o world</p>";
        let matches = search_content(content, "hello", 0, &SearchOptions::new());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "Hello");
        // Range CFI spans from the p's first chunk into the b's text
        assert!(matches[0].cfi.contains(','));
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
