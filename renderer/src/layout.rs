//! Layout - pagination and display configuration

use wasm_bindgen::prelude::*;

/// Spread configuration (single or double page)
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Spread {
    /// Single page display
    #[default]
    None,
    /// Always show two pages
    Always,
    /// Automatic based on viewport
    Auto,
}

/// Layout options
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct LayoutOptions {
    /// Width of each column/page in pixels
    pub width: Option<u32>,
    /// Height of the viewport in pixels
    pub height: Option<u32>,
    /// Spread mode
    pub spread: Spread,
    /// Minimum spread width (for auto mode)
    pub min_spread_width: u32,
    /// Column gap in pixels
    pub gap: u32,
    /// Padding in pixels
    pub padding: u32,
}

#[wasm_bindgen]
impl LayoutOptions {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            spread: Spread::None,
            min_spread_width: 800,
            gap: 20,
            padding: 20,
        }
    }
}

/// Calculated layout values
#[derive(Debug, Clone)]
pub struct Layout {
    /// Column width in pixels
    column_width: u32,
    /// Column gap in pixels
    column_gap: u32,
    /// Padding in pixels
    padding: u32,
    /// Viewport width
    viewport_width: u32,
    /// Viewport height
    viewport_height: u32,
    /// Whether spread mode is active
    spread_active: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            column_width: 600,
            column_gap: 20,
            padding: 20,
            viewport_width: 800,
            viewport_height: 600,
            spread_active: false,
        }
    }
}

impl Layout {
    /// Create layout from options
    pub fn from_options(options: LayoutOptions) -> Self {
        let viewport_width = options.width.unwrap_or(800);
        let viewport_height = options.height.unwrap_or(600);

        let spread_active = match options.spread {
            Spread::None => false,
            Spread::Always => true,
            Spread::Auto => viewport_width >= options.min_spread_width,
        };

        // Calculate column width
        let available_width = viewport_width - (options.padding * 2);
        let column_width = if spread_active {
            (available_width - options.gap) / 2
        } else {
            available_width
        };

        Self {
            column_width,
            column_gap: options.gap,
            padding: options.padding,
            viewport_width,
            viewport_height,
            spread_active,
        }
    }

    /// Get column width
    pub fn column_width(&self) -> u32 {
        self.column_width
    }

    /// Get column gap
    pub fn column_gap(&self) -> u32 {
        self.column_gap
    }

    /// Get padding
    pub fn padding(&self) -> u32 {
        self.padding
    }

    /// Get viewport width
    pub fn viewport_width(&self) -> u32 {
        self.viewport_width
    }

    /// Get viewport height
    pub fn viewport_height(&self) -> u32 {
        self.viewport_height
    }

    /// Check if spread mode is active
    pub fn is_spread(&self) -> bool {
        self.spread_active
    }
}
