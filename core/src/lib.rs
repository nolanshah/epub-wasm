//! epub-reader-core: Pure Rust EPUB parsing library
//!
//! This library provides EPUB parsing functionality without any WASM dependencies,
//! making it suitable for both server-side and client-side use.

mod archive;
mod cfi;
mod container;
mod error;
mod navigation;
mod package;
mod search;
mod section;

pub mod book;
pub mod path;

pub use archive::Archive;
pub use book::Book;
pub use cfi::{Cfi, CfiStep};
pub use error::{EpubError, Result};
pub use navigation::NavItem;
pub use package::{ManifestItem, Metadata, SpineItem};
pub use search::{search_content, search_text, SearchMatch, SearchOptions};
pub use section::Section;
