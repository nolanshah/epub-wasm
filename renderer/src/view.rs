//! View - iframe management for content isolation

use wasm_bindgen::prelude::*;
use web_sys::{Document, HtmlElement, HtmlIFrameElement, Window};

use crate::layout::Layout;

/// An iframe-based view for rendering EPUB content
pub struct View {
    /// The iframe element
    iframe: HtmlIFrameElement,
    /// Calculated page count
    page_count: usize,
    /// Content width (for pagination)
    content_width: f64,
    /// Viewport width
    viewport_width: f64,
}

impl View {
    /// Create a new view inside the container
    pub fn create(container: &HtmlElement) -> Result<Self, JsValue> {
        let document = window()?.document().ok_or("No document")?;

        // Create iframe
        let iframe = document
            .create_element("iframe")?
            .dyn_into::<HtmlIFrameElement>()?;

        // Style iframe
        let style = iframe.style();
        style.set_property("width", "100%")?;
        style.set_property("height", "100%")?;
        style.set_property("border", "none")?;
        style.set_property("overflow", "hidden")?;

        // Set sandbox for security (allow scripts for pagination)
        iframe.set_attribute("sandbox", "allow-same-origin allow-scripts")?;

        // Append to container
        container.append_child(&iframe)?;

        Ok(View {
            iframe,
            page_count: 1,
            content_width: 0.0,
            viewport_width: 0.0,
        })
    }

    /// Render content into the iframe using srcdoc
    pub fn render(&self, content: &str, layout: &Layout) -> Result<(), JsValue> {
        // Build the full HTML with pagination styles
        let html = format!(
            r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<style>
html, body {{
    margin: 0;
    padding: 0;
    height: 100%;
    overflow: hidden;
}}
body {{
    column-width: {}px;
    column-gap: {}px;
    column-fill: auto;
    height: 100%;
    padding: {}px;
    box-sizing: border-box;
    overflow: hidden;
}}
img {{
    max-width: 100%;
    height: auto;
}}
</style>
</head>
<body>{}</body>
</html>"#,
            layout.column_width(),
            layout.column_gap(),
            layout.padding(),
            extract_body(content)
        );

        // Use srcdoc attribute to set content
        self.iframe.set_attribute("srcdoc", &html)?;

        Ok(())
    }

    /// Show a specific page
    pub fn show_page(&self, page: usize) -> Result<(), JsValue> {
        if let Some(doc) = self.iframe.content_document() {
            if let Some(body) = doc.body() {
                let offset = page as f64 * (self.viewport_width + 20.0); // +20 for gap
                body.style()
                    .set_property("transform", &format!("translateX(-{}px)", offset))?;
            }
        }

        Ok(())
    }

    /// Get the number of pages
    pub fn page_count(&self) -> usize {
        self.page_count
    }

    /// Calculate pagination after content is loaded
    pub fn calculate_pagination(&mut self) -> Result<(), JsValue> {
        if let Some(doc) = self.iframe.content_document() {
            if let Some(body) = doc.body() {
                let scroll_width = body.scroll_width() as f64;
                let client_width = body.client_width() as f64;

                self.content_width = scroll_width;
                self.viewport_width = client_width;

                if client_width > 0.0 {
                    self.page_count = (scroll_width / client_width).ceil() as usize;
                } else {
                    self.page_count = 1;
                }
            }
        }

        Ok(())
    }

    /// Get the iframe's document
    pub fn iframe_document(&self) -> Option<Document> {
        self.iframe.content_document()
    }

    /// Get the iframe element
    pub fn iframe(&self) -> &HtmlIFrameElement {
        &self.iframe
    }
}

/// Extract body content from HTML
fn extract_body(html: &str) -> &str {
    // Simple extraction - find body tags
    if let Some(start) = html.find("<body") {
        if let Some(end_tag) = html[start..].find('>') {
            let body_start = start + end_tag + 1;
            if let Some(body_end) = html[body_start..].find("</body>") {
                return &html[body_start..body_start + body_end];
            }
        }
    }
    // Fallback: return as-is
    html
}

/// Get the window object
fn window() -> Result<Window, JsValue> {
    web_sys::window().ok_or_else(|| JsValue::from_str("No window"))
}
