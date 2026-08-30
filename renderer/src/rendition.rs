//! Rendition - a display controller: one iframe, one section at a time,
//! with scrolled or paginated (CSS multi-column) flow. Internal links and
//! TOC hrefs navigate between sections; a `relocated` callback reports
//! position changes including page numbers in paginated flow.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::{Element, Event, HtmlElement, HtmlIFrameElement};

use crate::bindings::{js_err, JsBook, RenderOptions};

/// Horizontal/vertical page padding in px. The column gap is 2×PAD so the
/// page stride is exactly `page_width + GAP` and margins look symmetric.
const PAD: i32 = 24;
const GAP: i32 = 2 * PAD;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Scrolled,
    Paginated,
}

/// Which page to show once the section finishes loading.
#[derive(Debug, Clone, Copy)]
enum PendingPage {
    First,
    Last,
    At(usize),
}

struct Inner {
    book: JsBook,
    container: HtmlElement,
    iframe: HtmlIFrameElement,
    current: usize,
    pending_fragment: Option<String>,
    pending_page: PendingPage,
    styles: Option<String>,
    flow: Flow,
    current_page: usize,
    page_count: usize,
    /// The loaded content computes to `direction: rtl`: columns flow
    /// right-to-left, so paging transforms flip sign
    rtl: bool,
    /// Current section is fixed-layout (scaled, never columned)
    fxl: bool,
    /// page stride in px (page width + gap); 0 when not paginated/measurable
    stride: f64,
    /// last measured iframe size, to coalesce resize events
    last_size: (i32, i32),
    on_relocated: Option<js_sys::Function>,
    on_error: Option<js_sys::Function>,
    onload: Option<Closure<dyn FnMut()>>,
    onclick: Option<Closure<dyn FnMut(Event)>>,
    onresize: Option<Closure<dyn FnMut()>>,
    onfonts: Option<Closure<dyn FnMut(JsValue)>>,
}

/// Report an error that happened inside an event closure, where there is no
/// JS caller to return a Result to: the `on_error` callback if set, else
/// `console.error`. Must be called with no active borrow of `rc`.
fn report_error(rc: &Rc<RefCell<Inner>>, err: JsValue) {
    let cb = rc.borrow().on_error.clone();
    match cb {
        Some(cb) => {
            let _ = cb.call1(&JsValue::NULL, &err);
        }
        None => web_sys::console::error_2(&"epub-wasm Rendition:".into(), &err),
    }
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
        // `allow-scripts` is required or the browser suppresses ALL event
        // listeners in the iframe document, including ones we attach from
        // the parent (book scripts are stripped at render time regardless).
        iframe.set_attribute(
            "sandbox",
            "allow-same-origin allow-scripts allow-popups allow-popups-to-escape-sandbox",
        )?;
        container.append_child(&iframe)?;

        let inner = Rc::new(RefCell::new(Inner {
            book,
            container,
            iframe: iframe.clone(),
            current: 0,
            pending_fragment: None,
            pending_page: PendingPage::First,
            styles: None,
            flow: Flow::Scrolled,
            current_page: 0,
            page_count: 1,
            rtl: false,
            fxl: false,
            stride: 0.0,
            last_size: (0, 0),
            on_relocated: None,
            on_error: None,
            onload: None,
            onclick: None,
            onresize: None,
            onfonts: None,
        }));

        // Click handler: created once, attached to every loaded document.
        let click_rc = Rc::clone(&inner);
        let onclick = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
            let Some(target) = event.target() else {
                return;
            };
            // The target lives in the iframe's realm, where `instanceof
            // Element` against our window's constructor is always false —
            // `dyn_into` would fail. Cast unchecked; `closest` erroring on a
            // non-element is caught by the match below.
            let target: Element = target.unchecked_into();
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
            if let Err(e) = display_at(&click_rc, index, fragment, PendingPage::First) {
                report_error(&click_rc, e);
            }
        });

        // Fonts-ready handler: embedded fonts can finish loading after the
        // iframe's load event and reflow the columns, invalidating the
        // measured page count. Re-measure and re-clamp when they settle.
        let fonts_rc = Rc::clone(&inner);
        let onfonts = Closure::<dyn FnMut(JsValue)>::new(move |_: JsValue| {
            let relocation = {
                let mut inner = fonts_rc.borrow_mut();
                if inner.flow != Flow::Paginated || inner.stride <= 0.0 {
                    None
                } else {
                    let before = inner.page_count;
                    measure_pages(&mut inner);
                    inner.current_page = inner.current_page.min(inner.page_count - 1);
                    apply_transform(&inner);
                    (inner.page_count != before).then(|| collect_relocation(&inner))
                }
            };
            fire_relocated(relocation);
        });

        // Load handler: wire up clicks, paginate/scroll, notify.
        let load_rc = Rc::clone(&inner);
        let onload = Closure::<dyn FnMut()>::new(move || {
            let relocation = {
                let mut inner = load_rc.borrow_mut();

                let doc = inner.iframe.content_document();
                if let (Some(doc), Some(click)) = (&doc, &inner.onclick) {
                    let _ = doc.add_event_listener_with_callback(
                        "click",
                        click.as_ref().unchecked_ref(),
                    );
                }

                let fragment = inner.pending_fragment.take();

                if inner.flow == Flow::Paginated && inner.stride > 0.0 {
                    // Content-level dir="rtl" makes CSS columns flow
                    // right-to-left (overflow extends left, offsets go
                    // negative), so detect the computed direction and flip
                    // the paging math. Never force a direction via CSS —
                    // that would break bidi text in the content itself.
                    inner.rtl = doc
                        .as_ref()
                        .and_then(|d| d.body())
                        .and_then(|body| {
                            inner
                                .iframe
                                .content_window()?
                                .get_computed_style(&body)
                                .ok()
                                .flatten()
                        })
                        .and_then(|cs| cs.get_property_value("direction").ok())
                        .map(|d| d == "rtl")
                        .unwrap_or(false);

                    measure_pages(&mut inner);

                    let mut page = match inner.pending_page {
                        PendingPage::First => 0,
                        PendingPage::Last => inner.page_count - 1,
                        PendingPage::At(p) => p.min(inner.page_count - 1),
                    };
                    if let (Some(doc), Some(frag)) = (&doc, &fragment) {
                        if let Some(p) = page_of_fragment(doc, frag, inner.stride, inner.rtl) {
                            page = p.min(inner.page_count - 1);
                        }
                    }
                    inner.current_page = page;
                    apply_transform(&inner);

                    // Re-measure when embedded fonts finish loading.
                    if let (Some(doc), Some(cb)) = (&doc, &inner.onfonts) {
                        if let Ok(ready) = doc.fonts().ready() {
                            let _ = ready.then(cb);
                        }
                    }
                } else {
                    inner.current_page = 0;
                    inner.page_count = 1;
                    if let Some(doc) = &doc {
                        if let Some(frag) = &fragment {
                            if let Some(el) = doc.get_element_by_id(frag) {
                                el.scroll_into_view();
                            }
                        } else if let Some(el) = doc.document_element() {
                            el.set_scroll_top(0);
                        }
                    }
                }

                inner.pending_page = PendingPage::First;
                Some(collect_relocation(&inner))
            };
            fire_relocated(relocation);
        });

        iframe.set_onload(Some(onload.as_ref().unchecked_ref()));

        // Window resize: re-render the current section (columns depend on the
        // viewport size), keeping the current page. Coalesced by size.
        let resize_rc = Rc::clone(&inner);
        let onresize = Closure::<dyn FnMut()>::new(move || {
            let redisplay = {
                let mut inner = resize_rc.borrow_mut();
                if inner.flow != Flow::Paginated && !inner.fxl {
                    None
                } else {
                    let rect = inner.iframe.get_bounding_client_rect();
                    let size = (rect.width().floor() as i32, rect.height().floor() as i32);
                    if size == inner.last_size || size.0 <= 0 {
                        None
                    } else {
                        inner.last_size = size;
                        Some((inner.current, inner.current_page))
                    }
                }
            };
            if let Some((index, page)) = redisplay {
                if let Err(e) = display_at(&resize_rc, index, None, PendingPage::At(page)) {
                    report_error(&resize_rc, e);
                }
            }
        });
        if let Some(w) = web_sys::window() {
            let _ = w.add_event_listener_with_callback("resize", onresize.as_ref().unchecked_ref());
        }

        {
            let mut i = inner.borrow_mut();
            i.onclick = Some(onclick);
            i.onload = Some(onload);
            i.onresize = Some(onresize);
            i.onfonts = Some(onfonts);
        }

        Ok(Rendition { inner })
    }

    /// Display the first section
    pub fn display(&self) -> Result<(), JsValue> {
        display_at(&self.inner, 0, None, PendingPage::First)
    }

    /// Display a section by index
    pub fn display_section(&self, index: usize) -> Result<(), JsValue> {
        display_at(&self.inner, index, None, PendingPage::First)
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
                display_at(&self.inner, index, fragment, PendingPage::First)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Advance one page (paginated) or section (scrolled).
    /// Returns false at the end of the book.
    pub fn next(&self) -> Result<bool, JsValue> {
        let action = {
            let mut inner = self.inner.borrow_mut();
            if inner.flow == Flow::Paginated && inner.current_page + 1 < inner.page_count {
                inner.current_page += 1;
                apply_transform(&inner);
                Step::Page(collect_relocation(&inner))
            } else if inner.current + 1 < inner.book.section_count() {
                Step::Section(inner.current + 1, PendingPage::First)
            } else {
                Step::None
            }
        };
        self.run_step(action)
    }

    /// Go back one page (paginated) or section (scrolled).
    /// Returns false at the start of the book.
    pub fn prev(&self) -> Result<bool, JsValue> {
        let action = {
            let mut inner = self.inner.borrow_mut();
            if inner.flow == Flow::Paginated && inner.current_page > 0 {
                inner.current_page -= 1;
                apply_transform(&inner);
                Step::Page(collect_relocation(&inner))
            } else if inner.current > 0 {
                Step::Section(inner.current - 1, PendingPage::Last)
            } else {
                Step::None
            }
        };
        self.run_step(action)
    }

    /// Set the flow mode: `"scrolled"` (default) or `"paginated"`.
    pub fn set_flow(&self, flow: &str) -> Result<(), JsValue> {
        let parsed = match flow {
            "scrolled" => Flow::Scrolled,
            "paginated" => Flow::Paginated,
            other => return Err(js_err(format!("Unknown flow: {}", other))),
        };
        let current = {
            let mut inner = self.inner.borrow_mut();
            if inner.flow == parsed {
                return Ok(());
            }
            inner.flow = parsed;
            inner.current
        };
        display_at(&self.inner, current, None, PendingPage::First)
    }

    /// Current flow mode
    #[wasm_bindgen(getter)]
    pub fn flow(&self) -> String {
        match self.inner.borrow().flow {
            Flow::Scrolled => "scrolled".to_string(),
            Flow::Paginated => "paginated".to_string(),
        }
    }

    /// Reading direction from the spine (`"rtl"`, `"ltr"`), if declared
    #[wasm_bindgen(getter)]
    pub fn direction(&self) -> Option<String> {
        self.inner
            .borrow()
            .book
            .book()
            .page_progression_direction
            .clone()
    }

    /// `"pre-paginated"` for fixed-layout books, else `"reflowable"`
    #[wasm_bindgen(getter)]
    pub fn layout(&self) -> String {
        self.inner.borrow().book.layout()
    }

    /// Index of the section currently displayed
    pub fn current_section_index(&self) -> usize {
        self.inner.borrow().current
    }

    /// Current page within the section (0-based; always 0 in scrolled flow)
    pub fn current_page(&self) -> usize {
        self.inner.borrow().current_page
    }

    /// Number of pages in the current section (1 in scrolled flow)
    pub fn page_count(&self) -> usize {
        self.inner.borrow().page_count
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

    /// Build the locations index (see `JsBook.generate_locations`); after
    /// this, `relocated` events carry a `percentage`.
    pub fn generate_locations(&self, chars_per: usize) -> Result<usize, JsValue> {
        self.inner.borrow_mut().book.generate_locations(chars_per)
    }

    /// CSS injected into every section (fonts, colors, themes). Re-renders
    /// the current section.
    pub fn set_styles(&self, css: Option<String>) -> Result<(), JsValue> {
        let current = {
            let mut inner = self.inner.borrow_mut();
            inner.styles = css;
            inner.current
        };
        display_at(&self.inner, current, None, PendingPage::First)
    }

    /// Callback invoked with `{ index, href, page, page_count }` whenever the
    /// displayed position changes.
    pub fn on_relocated(&self, callback: Option<js_sys::Function>) {
        self.inner.borrow_mut().on_relocated = callback;
    }

    /// Callback for errors that occur inside event handlers (link clicks,
    /// resizes), where there is no direct caller to receive them. Without a
    /// callback such errors go to `console.error`.
    pub fn on_error(&self, callback: Option<js_sys::Function>) {
        self.inner.borrow_mut().on_error = callback;
    }

    /// Remove the iframe, revoke blob URLs and release callbacks
    pub fn destroy(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.iframe.set_onload(None);
        if let (Some(w), Some(cb)) = (web_sys::window(), &inner.onresize) {
            let _ = w.remove_event_listener_with_callback("resize", cb.as_ref().unchecked_ref());
        }
        let _ = inner.container.remove_child(&inner.iframe);
        inner.onload = None;
        inner.onclick = None;
        inner.onresize = None;
        inner.onfonts = None;
        inner.on_relocated = None;
        inner.on_error = None;
        inner.book.revoke_resources();
    }
}

enum Step {
    Page(Relocation),
    Section(usize, PendingPage),
    None,
}

impl Rendition {
    fn run_step(&self, step: Step) -> Result<bool, JsValue> {
        match step {
            Step::Page(r) => {
                fire_relocated(Some(r));
                Ok(true)
            }
            Step::Section(index, pending) => {
                display_at(&self.inner, index, None, pending)?;
                Ok(true)
            }
            Step::None => Ok(false),
        }
    }
}

struct Relocation {
    callback: Option<js_sys::Function>,
    index: usize,
    href: String,
    page: usize,
    page_count: usize,
    /// Progress percentage; None until locations have been generated
    percentage: Option<f64>,
}

fn collect_relocation(inner: &Inner) -> Relocation {
    let href = inner
        .book
        .book()
        .section(inner.current)
        .map(|s| s.href.clone())
        .unwrap_or_default();
    // How far through the section: end of the current page in paginated
    // flow, start of the section otherwise.
    let fraction = if inner.page_count > 1 {
        (inner.current_page as f64 + 1.0) / inner.page_count as f64
    } else {
        0.0
    };
    Relocation {
        callback: inner.on_relocated.clone(),
        index: inner.current,
        href,
        page: inner.current_page,
        page_count: inner.page_count,
        percentage: inner.book.percentage_at(inner.current, fraction),
    }
}

/// Call the relocated callback (outside any RefCell borrow).
fn fire_relocated(relocation: Option<Relocation>) {
    let Some(r) = relocation else {
        return;
    };
    let Some(cb) = r.callback else {
        return;
    };
    let obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&obj, &"index".into(), &(r.index as u32).into());
    let _ = js_sys::Reflect::set(&obj, &"href".into(), &r.href.into());
    let _ = js_sys::Reflect::set(&obj, &"page".into(), &(r.page as u32).into());
    let _ = js_sys::Reflect::set(&obj, &"page_count".into(), &(r.page_count as u32).into());
    let _ = js_sys::Reflect::set(
        &obj,
        &"percentage".into(),
        &r.percentage.map(JsValue::from).unwrap_or(JsValue::NULL),
    );
    let _ = cb.call1(&JsValue::NULL, &obj);
}

/// Page count from the laid-out column width. With GAP = 2×PAD the body's
/// scroll width is an exact multiple of the stride, so round-to-nearest
/// absorbs sub-pixel measurement error.
fn measure_pages(inner: &mut Inner) {
    let scroll_width = inner
        .iframe
        .content_document()
        .and_then(|d| d.body())
        .map(|b| b.scroll_width() as f64)
        .unwrap_or(0.0);

    inner.page_count = if inner.stride > 0.0 && scroll_width > 0.0 {
        ((scroll_width / inner.stride).round() as usize).max(1)
    } else {
        1
    };
}

fn apply_transform(inner: &Inner) {
    let Some(body) = inner.iframe.content_document().and_then(|d| d.body()) else {
        return;
    };
    let offset = inner.current_page as f64 * inner.stride;
    // RTL columns extend leftward, so advancing moves the body rightward.
    let value = if inner.rtl {
        format!("translateX({}px)", offset)
    } else {
        format!("translateX(-{}px)", offset)
    };
    let _ = body.style().set_property("transform", &value);
}

/// Which page a fragment element sits on. `offsetLeft` is measured in the
/// flowed layout and ignores the translateX, which is exactly what we need.
/// In RTL column layout the first column is rightmost and later columns have
/// decreasing (eventually negative) offsets.
fn page_of_fragment(
    doc: &web_sys::Document,
    fragment: &str,
    stride: f64,
    rtl: bool,
) -> Option<usize> {
    let el = doc.get_element_by_id(fragment)?;
    // Cross-realm: unchecked cast (see the click handler note).
    let el: HtmlElement = el.unchecked_into();
    let left = el.offset_left() as f64;
    let page = if rtl {
        ((PAD as f64 - left) / stride).ceil().max(0.0)
    } else {
        ((left - PAD as f64) / stride).max(0.0).floor()
    };
    Some(page as usize)
}

fn paginated_css(page_width: i32, page_height: i32) -> String {
    format!(
        r#"html, body {{ margin: 0; height: 100%; overflow: hidden; }}
body {{
    box-sizing: content-box;
    padding: {pad}px;
    width: {w}px;
    height: {h}px;
    column-width: {w}px;
    column-gap: {gap}px;
    column-fill: auto;
}}
img, svg, video {{ max-width: {w}px; max-height: {h}px; }}"#,
        pad = PAD,
        gap = GAP,
        w = page_width,
        h = page_height,
    )
}

/// CSS scaling a fixed-layout page of design size (w, h) into a viewport of
/// (vw, vh), centered. Margins position the box pre-transform; with
/// transform-origin 0 0 the scaled box lands visually centered.
fn fxl_css(w: f64, h: f64, vw: f64, vh: f64) -> String {
    let k = (vw / w).min(vh / h);
    format!(
        r#"html, body {{ margin: 0; padding: 0; overflow: hidden; }}
body {{
    width: {w}px;
    height: {h}px;
    transform: scale({k});
    transform-origin: 0 0;
    margin-left: {ml}px;
    margin-top: {mt}px;
}}"#,
        w = w,
        h = h,
        k = k,
        ml = (vw - w * k) / 2.0,
        mt = (vh - h * k) / 2.0,
    )
}

fn display_at(
    rc: &Rc<RefCell<Inner>>,
    index: usize,
    fragment: Option<String>,
    pending_page: PendingPage,
) -> Result<(), JsValue> {
    let mut inner = rc.borrow_mut();

    if index >= inner.book.section_count() {
        return Err(js_err(format!("Section {} out of range", index)));
    }

    // Measure the viewport before rendering (the iframe is already laid out).
    let mut styles = String::new();
    inner.stride = 0.0;
    inner.rtl = false;
    inner.fxl = inner.book.book().is_pre_paginated(index);

    let rect = inner.iframe.get_bounding_client_rect();
    let vw = rect.width().floor() as i32;
    let vh = rect.height().floor() as i32;
    inner.last_size = (vw, vh);

    if inner.fxl {
        // Fixed layout wins over the flow setting: scale the page to fit,
        // never inject column CSS onto pre-paginated content.
        let viewport = inner
            .book
            .book_mut()
            .load_section(index)
            .ok()
            .and_then(|s| s.viewport());
        if let Some((w, h)) = viewport {
            if vw > 0 && vh > 0 {
                styles.push_str(&fxl_css(w, h, vw as f64, vh as f64));
            }
        }
        // No numeric viewport meta: leave the page at natural size.
    } else if inner.flow == Flow::Paginated {
        let w = vw - 2 * PAD;
        let h = vh - 2 * PAD;
        if w > 0 && h > 0 {
            inner.stride = (w + GAP) as f64;
            styles.push_str(&paginated_css(w, h));
        }
    }
    if let Some(css) = &inner.styles {
        styles.push('\n');
        styles.push_str(css);
    }

    let opts = RenderOptions {
        styles: (!styles.is_empty()).then_some(styles),
        ..Default::default()
    };
    let html = inner.book.render_with(index, &opts)?;

    inner.current = index;
    inner.current_page = 0;
    inner.page_count = 1;
    inner.pending_fragment = fragment;
    inner.pending_page = pending_page;
    inner.iframe.set_srcdoc(&html);
    Ok(())
}
