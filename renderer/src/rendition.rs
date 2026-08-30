//! Rendition - a minimal display controller: one iframe, one section at a
//! time, scrolled flow. Internal links and TOC hrefs navigate between
//! sections; a `relocated` callback reports position changes.
//!
//! Column pagination is not implemented yet (see README roadmap).

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::{Element, Event, HtmlElement, HtmlIFrameElement};

use crate::bindings::{js_err, JsBook, RenderOptions};

struct Inner {
    book: JsBook,
    container: HtmlElement,
    iframe: HtmlIFrameElement,
    current: usize,
    pending_fragment: Option<String>,
    styles: Option<String>,
    on_relocated: Option<js_sys::Function>,
    onload: Option<Closure<dyn FnMut()>>,
    onclick: Option<Closure<dyn FnMut(Event)>>,
}

/// The main EPUB rendition controller
#[wasm_bindgen]
pub struct Rendition {
    inner: Rc<RefCell<Inner>>,
}

#[wasm_bindgen]
impl Rendition {
    /// Create a rendition that renders into `container` (an empty block element)
    #[wasm_bindgen(constructor)]
    pub fn new(book_data: &[u8], container: HtmlElement) -> Result<Rendition, JsValue> {
        let book = JsBook::new(book_data)?;

        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or_else(|| js_err("No document"))?;

        let iframe = document
            .create_element("iframe")?
            .dyn_into::<HtmlIFrameElement>()?;
        let style = iframe.style();
        style.set_property("width", "100%")?;
        style.set_property("height", "100%")?;
        style.set_property("border", "none")?;
        style.set_property("display", "block")?;
        iframe.set_attribute(
            "sandbox",
            "allow-same-origin allow-popups allow-popups-to-escape-sandbox",
        )?;
        container.append_child(&iframe)?;

        let inner = Rc::new(RefCell::new(Inner {
            book,
            container,
            iframe: iframe.clone(),
            current: 0,
            pending_fragment: None,
            styles: None,
            on_relocated: None,
            onload: None,
            onclick: None,
        }));

        // Click handler: created once, attached to every loaded document.
        let click_rc = Rc::clone(&inner);
        let onclick = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
            let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
                return;
            };
            let Ok(Some(link)) = target.closest("a[data-epub-section]") else {
                return;
            };
            event.prevent_default();
            let Some(index) = link
                .get_attribute("data-epub-section")
                .and_then(|s| s.parse::<usize>().ok())
            else {
                return;
            };
            let fragment = link
                .get_attribute("data-epub-fragment")
                .filter(|f| !f.is_empty());
            let _ = display_at(&click_rc, index, fragment);
        });

        // Load handler: wire up clicks, scroll to fragment, notify.
        let load_rc = Rc::clone(&inner);
        let onload = Closure::<dyn FnMut()>::new(move || {
            let (doc, fragment, current, href, callback, click) = {
                let mut inner = load_rc.borrow_mut();
                let doc = inner.iframe.content_document();
                let fragment = inner.pending_fragment.take();
                let current = inner.current;
                let href = inner
                    .book
                    .book()
                    .section(current)
                    .map(|s| s.href.clone())
                    .unwrap_or_default();
                let callback = inner.on_relocated.clone();
                let click = inner
                    .onclick
                    .as_ref()
                    .map(|c| c.as_ref().unchecked_ref::<js_sys::Function>().clone());
                (doc, fragment, current, href, callback, click)
            };

            if let Some(doc) = doc {
                if let Some(click) = click {
                    let _ = doc.add_event_listener_with_callback("click", &click);
                }
                if let Some(frag) = fragment {
                    if let Some(el) = doc.get_element_by_id(&frag) {
                        el.scroll_into_view();
                    }
                } else if let Some(el) = doc.document_element() {
                    el.set_scroll_top(0);
                }
            }

            if let Some(cb) = callback {
                let obj = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&obj, &"index".into(), &(current as u32).into());
                let _ = js_sys::Reflect::set(&obj, &"href".into(), &href.into());
                let _ = cb.call1(&JsValue::NULL, &obj);
            }
        });

        iframe.set_onload(Some(onload.as_ref().unchecked_ref()));

        {
            let mut i = inner.borrow_mut();
            i.onclick = Some(onclick);
            i.onload = Some(onload);
        }

        Ok(Rendition { inner })
    }

    /// Display the first section
    pub fn display(&self) -> Result<(), JsValue> {
        display_at(&self.inner, 0, None)
    }

    /// Display a section by index
    pub fn display_section(&self, index: usize) -> Result<(), JsValue> {
        display_at(&self.inner, index, None)
    }

    /// Display the target of an href (e.g. a TOC entry's `href`)
    pub fn display_href(&self, href: &str) -> Result<bool, JsValue> {
        let resolved = {
            let inner = self.inner.borrow();
            let book = inner.book.book();
            let (p, frag) = epub_reader_core::path::split_fragment(href);
            book.section_index_by_href(p)
                .map(|i| (i, frag.map(str::to_string)))
        };
        match resolved {
            Some((index, fragment)) => {
                display_at(&self.inner, index, fragment)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Go to the next section. Returns false at the end of the book.
    pub fn next(&self) -> Result<bool, JsValue> {
        let (current, count) = {
            let inner = self.inner.borrow();
            (inner.current, inner.book.section_count())
        };
        if current + 1 < count {
            display_at(&self.inner, current + 1, None)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Go to the previous section. Returns false at the start of the book.
    pub fn prev(&self) -> Result<bool, JsValue> {
        let current = self.inner.borrow().current;
        if current > 0 {
            display_at(&self.inner, current - 1, None)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Index of the section currently displayed
    pub fn current_section_index(&self) -> usize {
        self.inner.borrow().current
    }

    /// Number of sections
    #[wasm_bindgen(getter)]
    pub fn section_count(&self) -> usize {
        self.inner.borrow().book.section_count()
    }

    /// Book metadata (see `JsBook.metadata`)
    #[wasm_bindgen(getter)]
    pub fn metadata(&self) -> Result<JsValue, JsValue> {
        self.inner.borrow().book.metadata()
    }

    /// Table of contents (see `JsBook.toc`)
    #[wasm_bindgen(getter)]
    pub fn toc(&self) -> Result<JsValue, JsValue> {
        self.inner.borrow().book.toc()
    }

    /// Full-text search (see `JsBook.search`)
    pub fn search(&self, query: &str, options: JsValue) -> Result<JsValue, JsValue> {
        self.inner.borrow_mut().book.search(query, options)
    }

    /// CSS injected into every section (fonts, colors, themes). Re-renders
    /// the current section.
    pub fn set_styles(&self, css: Option<String>) -> Result<(), JsValue> {
        let current = {
            let mut inner = self.inner.borrow_mut();
            inner.styles = css;
            inner.current
        };
        display_at(&self.inner, current, None)
    }

    /// Callback invoked with `{ index, href }` whenever a section finishes loading
    pub fn on_relocated(&self, callback: Option<js_sys::Function>) {
        self.inner.borrow_mut().on_relocated = callback;
    }

    /// Remove the iframe, revoke blob URLs and release callbacks
    pub fn destroy(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.iframe.set_onload(None);
        let _ = inner.container.remove_child(&inner.iframe);
        inner.onload = None;
        inner.onclick = None;
        inner.on_relocated = None;
        inner.book.revoke_resources();
    }
}

fn display_at(rc: &Rc<RefCell<Inner>>, index: usize, fragment: Option<String>) -> Result<(), JsValue> {
    let mut inner = rc.borrow_mut();

    if index >= inner.book.section_count() {
        return Err(js_err(format!("Section {} out of range", index)));
    }

    let opts = RenderOptions {
        styles: inner.styles.clone(),
        ..Default::default()
    };
    let html = inner.book.render_with(index, &opts)?;

    inner.current = index;
    inner.pending_fragment = fragment;
    inner.iframe.set_srcdoc(&html);
    Ok(())
}
