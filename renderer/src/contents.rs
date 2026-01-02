//! Contents - DOM manipulation helpers

use wasm_bindgen::prelude::*;
use web_sys::{Document, Element, Range, Selection};

/// Helper for manipulating iframe content
pub struct Contents {
    document: Document,
}

impl Contents {
    /// Create a new contents helper
    pub fn new(document: Document) -> Self {
        Self { document }
    }

    /// Get the document
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// Find an element by ID
    pub fn get_element_by_id(&self, id: &str) -> Option<Element> {
        self.document.get_element_by_id(id)
    }

    /// Query for an element
    pub fn query_selector(&self, selector: &str) -> Result<Option<Element>, JsValue> {
        self.document.query_selector(selector)
    }

    /// Get the current selection
    pub fn get_selection(&self) -> Result<Option<Selection>, JsValue> {
        self.document.get_selection()
    }

    /// Get the selected text
    pub fn selected_text(&self) -> Result<Option<String>, JsValue> {
        if let Some(selection) = self.get_selection()? {
            if selection.range_count() > 0 {
                return Ok(Some(selection.to_string().into()));
            }
        }
        Ok(None)
    }

    /// Get the selected range
    pub fn selected_range(&self) -> Result<Option<Range>, JsValue> {
        if let Some(selection) = self.get_selection()? {
            if selection.range_count() > 0 {
                return Ok(Some(selection.get_range_at(0)?));
            }
        }
        Ok(None)
    }

    /// Apply a CSS class to all elements matching a selector
    pub fn add_class(&self, selector: &str, class: &str) -> Result<(), JsValue> {
        let elements = self.document.query_selector_all(selector)?;

        for i in 0..elements.length() {
            if let Some(node) = elements.get(i) {
                if let Ok(element) = node.dyn_into::<Element>() {
                    let class_list = element.class_list();
                    class_list.add_1(class)?;
                }
            }
        }

        Ok(())
    }

    /// Remove a CSS class from all elements matching a selector
    pub fn remove_class(&self, selector: &str, class: &str) -> Result<(), JsValue> {
        let elements = self.document.query_selector_all(selector)?;

        for i in 0..elements.length() {
            if let Some(node) = elements.get(i) {
                if let Ok(element) = node.dyn_into::<Element>() {
                    let class_list = element.class_list();
                    class_list.remove_1(class)?;
                }
            }
        }

        Ok(())
    }

    /// Inject a stylesheet
    pub fn add_stylesheet(&self, css: &str) -> Result<Element, JsValue> {
        let style = self.document.create_element("style")?;
        style.set_text_content(Some(css));

        // Append to document element if no head
        if let Some(doc_element) = self.document.document_element() {
            doc_element.append_child(&style)?;
        }

        Ok(style)
    }

    /// Inject a script
    pub fn add_script(&self, js: &str) -> Result<Element, JsValue> {
        let script = self.document.create_element("script")?;
        script.set_text_content(Some(js));

        if let Some(body) = self.document.body() {
            body.append_child(&script)?;
        }

        Ok(script)
    }

    /// Get the scroll position
    pub fn scroll_top(&self) -> f64 {
        self.document
            .document_element()
            .map(|e| e.scroll_top() as f64)
            .unwrap_or(0.0)
    }

    /// Set the scroll position
    pub fn set_scroll_top(&self, top: f64) {
        if let Some(element) = self.document.document_element() {
            element.set_scroll_top(top as i32);
        }
    }

    /// Get the scroll width (for pagination)
    pub fn scroll_width(&self) -> f64 {
        self.document
            .body()
            .map(|b| b.scroll_width() as f64)
            .unwrap_or(0.0)
    }

    /// Get the client width
    pub fn client_width(&self) -> f64 {
        self.document
            .body()
            .map(|b| b.client_width() as f64)
            .unwrap_or(0.0)
    }
}
