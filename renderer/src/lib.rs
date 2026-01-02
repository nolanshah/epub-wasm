//! epub-reader-renderer: WASM-based EPUB renderer
//!
//! This library provides browser-based EPUB rendering using iframes
//! for content isolation and CSS columns for pagination.

mod bindings;
mod contents;
mod layout;
mod locations;
mod rendition;
mod resources;
mod view;

pub use bindings::{JsBook, JsCfi, JsSearchOptions};
pub use layout::{Layout, LayoutOptions, Spread};
pub use locations::{Location, Locations};
pub use rendition::Rendition;
pub use view::View;

use wasm_bindgen::prelude::*;

/// Initialize the WASM module (called automatically)
#[wasm_bindgen(start)]
pub fn init() {
    // Set up panic hook for better error messages
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}
