//! Locations - a stable position index across the whole book.
//!
//! The book's plain text is divided into positions every `chars_per`
//! characters (epub.js calls these "locations"). Each carries a point CFI
//! into the raw document and a percentage through the book, giving stable
//! progress numbers independent of rendering.

use serde::{Deserialize, Serialize};

/// One position in the book.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    /// Point CFI string addressing this position in the raw document
    pub cfi: String,
    /// Spine index of the section containing it
    pub section_index: usize,
    /// Byte offset in the section's plain text (`Book::section_text`)
    pub offset: usize,
    /// Progress through the book, 0.0 – 100.0
    pub percentage: f64,
}

/// The generated index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Locations {
    pub locations: Vec<Location>,
    /// Characters of plain text per section
    section_chars: Vec<usize>,
    total_chars: usize,
}

impl Locations {
    pub(crate) fn new(locations: Vec<Location>, section_chars: Vec<usize>) -> Self {
        let total_chars = section_chars.iter().sum();
        Locations {
            locations,
            section_chars,
            total_chars,
        }
    }

    /// Number of positions
    pub fn total(&self) -> usize {
        self.locations.len()
    }

    /// Progress percentage for a point within a section, where `fraction`
    /// is how far through the section the reader is (0.0 – 1.0).
    pub fn percentage_at(&self, section_index: usize, fraction: f64) -> f64 {
        if self.total_chars == 0 {
            return 0.0;
        }
        let before: usize = self.section_chars[..section_index.min(self.section_chars.len())]
            .iter()
            .sum();
        let within = self
            .section_chars
            .get(section_index)
            .map(|&c| c as f64 * fraction.clamp(0.0, 1.0))
            .unwrap_or(0.0);
        ((before as f64 + within) / self.total_chars as f64 * 100.0).clamp(0.0, 100.0)
    }

    /// Index of the location nearest to a point within a section.
    pub fn location_at(&self, section_index: usize, fraction: f64) -> usize {
        let pct = self.percentage_at(section_index, fraction);
        match self
            .locations
            .binary_search_by(|l| l.percentage.partial_cmp(&pct).unwrap())
        {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(section: usize, pct: f64) -> Location {
        Location {
            cfi: String::new(),
            section_index: section,
            offset: 0,
            percentage: pct,
        }
    }

    #[test]
    fn percentages_interpolate_within_sections() {
        let l = Locations::new(
            vec![loc(0, 0.0), loc(1, 50.0)],
            vec![100, 100],
        );
        assert_eq!(l.percentage_at(0, 0.0), 0.0);
        assert_eq!(l.percentage_at(0, 0.5), 25.0);
        assert_eq!(l.percentage_at(1, 0.0), 50.0);
        assert_eq!(l.percentage_at(1, 1.0), 100.0);
        // Out of range clamps
        assert_eq!(l.percentage_at(5, 0.0), 100.0);

        assert_eq!(l.location_at(0, 0.1), 0);
        assert_eq!(l.location_at(1, 0.1), 1);
    }

    #[test]
    fn empty_book_is_zero() {
        let l = Locations::new(vec![], vec![]);
        assert_eq!(l.total(), 0);
        assert_eq!(l.percentage_at(0, 0.5), 0.0);
    }
}
