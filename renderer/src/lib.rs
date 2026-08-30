//! epub-reader-renderer: WASM bindings for the EPUB reader
//!
//! - [`JsBook`]: parse an EPUB and get display-ready HTML per section
//! - [`Rendition`]: a minimal iframe-based reader on top of `JsBook`
//! - [`JsCfi`]: parse/compare EPUB CFIs

mod bindings;
mod rendition;
mod resources;
pub mod rewrite;

pub use bindings::{HighlightRange, JsBook, JsCfi, RenderOptions};
pub use rendition::Rendition;

use wasm_bindgen::prelude::*;

/// Initialize the WASM module (called automatically)
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}
