//! Rendition - Main display controller for EPUB content

use std::rc::Rc;

use epub_reader_core::Book;
use wasm_bindgen::prelude::*;
use web_sys::{Element, HtmlElement};

use crate::layout::{Layout, LayoutOptions};
use crate::locations::Locations;
use crate::view::View;

/// The main EPUB rendition controller
#[wasm_bindgen]
pub struct Rendition {
    /// The book being rendered
    book: Rc<Book>,
    /// Container element
    container: HtmlElement,
    /// Current section index
    current_section: usize,
    /// Current page within section
    current_page: usize,
    /// Layout configuration
    layout: Layout,
    /// Location tracking
    locations: Option<Locations>,
    /// Active view (iframe)
    view: Option<View>,
}

#[wasm_bindgen]
impl Rendition {
    /// Create a new rendition attached to a container element
    #[wasm_bindgen(constructor)]
    pub fn new(book_data: &[u8], container: HtmlElement) -> Result<Rendition, JsValue> {
        let book = Book::from_bytes(book_data.to_vec())
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(Rendition {
            book: Rc::new(book),
            container,
            current_section: 0,
            current_page: 0,
            layout: Layout::default(),
            locations: None,
            view: None,
        })
    }

    /// Display the book starting at the beginning
    pub fn display(&mut self) -> Result<(), JsValue> {
        self.display_section(0)
    }

    /// Display a specific section
    pub fn display_section(&mut self, index: usize) -> Result<(), JsValue> {
        if index >= self.book.section_count() {
            return Err(JsValue::from_str("Section index out of bounds"));
        }

        self.current_section = index;
        self.current_page = 0;

        // Create view if needed
        if self.view.is_none() {
            let view = View::create(&self.container)?;
            self.view = Some(view);
        }

        // Load and display section content
        self.render_current_section()
    }

    /// Navigate to the next page or section
    pub fn next(&mut self) -> Result<bool, JsValue> {
        // Try next page in current section
        if let Some(ref view) = self.view {
            let total_pages = view.page_count();
            if self.current_page + 1 < total_pages {
                self.current_page += 1;
                view.show_page(self.current_page)?;
                return Ok(true);
            }
        }

        // Try next section
        if self.current_section + 1 < self.book.section_count() {
            self.display_section(self.current_section + 1)?;
            Ok(true)
        } else {
            Ok(false) // End of book
        }
    }

    /// Navigate to the previous page or section
    pub fn prev(&mut self) -> Result<bool, JsValue> {
        // Try previous page in current section
        if self.current_page > 0 {
            self.current_page -= 1;
            if let Some(ref view) = self.view {
                view.show_page(self.current_page)?;
            }
            return Ok(true);
        }

        // Try previous section
        if self.current_section > 0 {
            self.display_section(self.current_section - 1)?;
            // Go to last page of section
            if let Some(ref view) = self.view {
                let last_page = view.page_count().saturating_sub(1);
                self.current_page = last_page;
                view.show_page(self.current_page)?;
            }
            Ok(true)
        } else {
            Ok(false) // Beginning of book
        }
    }

    /// Get the current section index
    pub fn current_section_index(&self) -> usize {
        self.current_section
    }

    /// Get the current page within the section
    pub fn current_page(&self) -> usize {
        self.current_page
    }

    /// Get the total number of pages in current section
    pub fn page_count(&self) -> usize {
        self.view.as_ref().map(|v| v.page_count()).unwrap_or(0)
    }

    /// Set the layout options
    pub fn set_layout(&mut self, options: LayoutOptions) {
        self.layout = Layout::from_options(options);
    }

    /// Get the book metadata as JSON
    pub fn metadata_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.book.metadata)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get the table of contents as JSON
    pub fn toc_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.book.toc)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    fn render_current_section(&mut self) -> Result<(), JsValue> {
        // Clone data needed for rendering
        let book = Rc::clone(&self.book);
        let section_index = self.current_section;

        // We need mutable access to load section
        // For now, we'll use interior mutability in a real implementation
        // This is a simplified version that assumes content is pre-loaded
        let section = book
            .section(section_index)
            .ok_or_else(|| JsValue::from_str("Section not found"))?;

        if let Some(ref view) = self.view {
            // Get content - in real impl, we'd load from archive
            // For now, show placeholder
            let content = section.content().unwrap_or("<html><body><p>Loading...</p></body></html>");
            view.render(content, &self.layout)?;
        }

        Ok(())
    }
}

impl Rendition {
    /// Create from a pre-loaded Book (for Rust API)
    pub fn from_book(book: Book, container: HtmlElement) -> Result<Self, JsValue> {
        Ok(Rendition {
            book: Rc::new(book),
            container,
            current_section: 0,
            current_page: 0,
            layout: Layout::default(),
            locations: None,
            view: None,
        })
    }

    /// Get a reference to the book
    pub fn book(&self) -> &Book {
        &self.book
    }
}
