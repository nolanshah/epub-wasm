//! Locations - progress tracking and position mapping

use epub_reader_core::Cfi;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// A location in the book
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    /// CFI for this location
    pub cfi: Cfi,
    /// Section index
    pub section_index: usize,
    /// Page within section
    pub page: usize,
    /// Absolute position (0-based index across all pages)
    pub position: usize,
    /// Progress as a percentage (0.0 - 100.0)
    pub percentage: f64,
}

/// Location tracking for the entire book
#[wasm_bindgen]
pub struct Locations {
    /// All locations in order
    locations: Vec<Location>,
    /// Total number of positions
    total_positions: usize,
    /// Characters per position (for estimation)
    chars_per_position: usize,
}

#[wasm_bindgen]
impl Locations {
    /// Create a new empty locations tracker
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            locations: Vec::new(),
            total_positions: 0,
            chars_per_position: 150, // Default estimate
        }
    }

    /// Get the total number of positions
    pub fn total(&self) -> usize {
        self.total_positions
    }

    /// Get current progress percentage for a position
    pub fn percentage(&self, position: usize) -> f64 {
        if self.total_positions == 0 {
            return 0.0;
        }
        (position as f64 / self.total_positions as f64) * 100.0
    }

    /// Get position for a percentage
    pub fn position_for_percentage(&self, percentage: f64) -> usize {
        let clamped = percentage.clamp(0.0, 100.0);
        ((clamped / 100.0) * self.total_positions as f64) as usize
    }
}

impl Locations {
    /// Generate locations from book content
    pub fn generate(section_lengths: &[usize], chars_per_position: usize) -> Self {
        let mut locations = Vec::new();
        let mut position = 0;

        for (section_index, &length) in section_lengths.iter().enumerate() {
            let section_positions = (length / chars_per_position).max(1);

            for page in 0..section_positions {
                let percentage = if section_lengths.iter().sum::<usize>() > 0 {
                    let chars_before: usize = section_lengths[..section_index].iter().sum();
                    let chars_in_section = page * chars_per_position;
                    let total_chars: usize = section_lengths.iter().sum();
                    ((chars_before + chars_in_section) as f64 / total_chars as f64) * 100.0
                } else {
                    0.0
                };

                locations.push(Location {
                    cfi: Cfi::from_spine_index(section_index),
                    section_index,
                    page,
                    position,
                    percentage,
                });

                position += 1;
            }
        }

        let total_positions = position;

        Locations {
            locations,
            total_positions,
            chars_per_position,
        }
    }

    /// Get location by position
    pub fn get(&self, position: usize) -> Option<&Location> {
        self.locations.get(position)
    }

    /// Find location for a section and page
    pub fn find(&self, section_index: usize, page: usize) -> Option<&Location> {
        self.locations
            .iter()
            .find(|l| l.section_index == section_index && l.page == page)
    }

    /// Get all locations
    pub fn all(&self) -> &[Location] {
        &self.locations
    }
}

impl Default for Locations {
    fn default() -> Self {
        Self::new()
    }
}
