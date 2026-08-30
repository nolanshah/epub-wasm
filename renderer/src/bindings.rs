//! JavaScript bindings
//!
//! `JsBook` is the plug-and-play API: give it EPUB bytes, get back metadata,
//! a resolved table of contents, and per-section HTML that is ready to drop
//! into an iframe (`render_section`). All hrefs it returns are normalized
//! archive paths, so consumers never have to do path math.

use serde::Deserialize;
use wasm_bindgen::prelude::*;

use epub_reader_core::{path, Book, Cfi, Location, Locations, SearchOptions, TextMap};

use crate::resources::Resources;
use crate::rewrite::{inject_into_head, rewrite_css, rewrite_html, RefKind, Reference, Replacement};

pub(crate) fn js_err(e: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&e.to_string()).into()
}

fn to_js<T: serde::Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    value.serialize(&serializer).map_err(js_err)
}

fn from_js<T: for<'de> Deserialize<'de> + Default>(value: JsValue) -> Result<T, JsValue> {
    if value.is_undefined() || value.is_null() {
        Ok(T::default())
    } else {
        serde_wasm_bindgen::from_value(value).map_err(js_err)
    }
}

/// A byte range in the section's plain text (`Book::section_text`) to wrap
/// in `<mark class="epub-highlight">`. Search results supply these directly:
/// `{ start: m.offset, end: m.offset + m.len }`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct HighlightRange {
    pub start: usize,
    pub end: usize,
}

/// Options for `render_section`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderOptions {
    /// Extra CSS appended to the document `<head>`
    pub styles: Option<String>,
    /// Include a small default stylesheet (responsive images). Default: true
    pub base_styles: bool,
    /// Remove `<script>` elements. Default: true
    pub strip_scripts: bool,
    /// Rewrite internal `<a href>` links to `href="#"` with `data-epub-*`
    /// attributes, and external links to open in a new tab. Default: true
    pub resolve_links: bool,
    /// Plain-text ranges to highlight with `<mark>` elements. The first
    /// mark of each range carries `data-epub-offset="<start>"` so it can be
    /// scrolled to. Default: none
    pub highlights: Vec<HighlightRange>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            styles: None,
            base_styles: true,
            strip_scripts: true,
            resolve_links: true,
            highlights: Vec::new(),
        }
    }
}

const HIGHLIGHT_STYLES: &str = "mark.epub-highlight { background: #ffe08a; color: inherit; }";

/// Splice `<mark>` elements into the raw document around each range, one
/// mark per source segment so ranges crossing element boundaries stay
/// well-formed.
fn inject_marks(src: &str, map: &TextMap, ranges: &[HighlightRange]) -> String {
    // Normalize, sort, merge overlaps
    let mut rs: Vec<(usize, usize)> = ranges
        .iter()
        .filter(|r| r.start < r.end)
        .map(|r| (r.start, r.end))
        .collect();
    rs.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for r in rs {
        match merged.last_mut() {
            Some(last) if r.0 <= last.1 => last.1 = last.1.max(r.1),
            _ => merged.push(r),
        }
    }

    // (position, open?, markup); applied back-to-front so positions stay valid
    let mut inserts: Vec<(usize, bool, String)> = Vec::new();
    for (start, end) in &merged {
        for (i, (s, t)) in map.source_segments(*start, *end).into_iter().enumerate() {
            let open = if i == 0 {
                format!(
                    "<mark class=\"epub-highlight\" data-epub-offset=\"{}\">",
                    start
                )
            } else {
                "<mark class=\"epub-highlight\">".to_string()
            };
            inserts.push((s, true, open));
            inserts.push((t, false, "</mark>".to_string()));
        }
    }

    // Descending by position; at equal positions apply the open first so the
    // close ends up before it in the final string (…</mark><mark>…).
    inserts.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));

    let mut out = src.to_string();
    for (pos, _, markup) in inserts {
        if pos <= out.len() {
            out.insert_str(pos, &markup);
        }
    }
    out
}

const BASE_STYLES: &str = "img, svg, video { max-width: 100%; height: auto; }";

/// JavaScript-friendly wrapper for Book
#[wasm_bindgen]
pub struct JsBook {
    inner: Book,
    resources: Resources,
    locations: Option<Locations>,
}

#[wasm_bindgen]
impl JsBook {
    /// Load an EPUB from bytes
    #[wasm_bindgen(constructor)]
    pub fn new(data: &[u8]) -> Result<JsBook, JsValue> {
        let book = Book::from_bytes(data.to_vec()).map_err(js_err)?;
        Ok(JsBook {
            inner: book,
            resources: Resources::new(),
            locations: None,
        })
    }

    /// Book metadata: `{ title, creators, language, identifier, description,
    /// publisher, date, subjects, rights, cover_id }`
    #[wasm_bindgen(getter)]
    pub fn metadata(&self) -> Result<JsValue, JsValue> {
        to_js(&self.inner.metadata)
    }

    /// Table of contents as a nested array of `{ id, href, label, children }`.
    /// `href` is an archive path plus optional `#fragment`; pass it to
    /// `section_index_for_href` / `resolve_href`.
    #[wasm_bindgen(getter)]
    pub fn toc(&self) -> Result<JsValue, JsValue> {
        to_js(&self.inner.toc)
    }

    /// Number of spine sections
    #[wasm_bindgen(getter)]
    pub fn section_count(&self) -> usize {
        self.inner.section_count()
    }

    /// Metadata for every section: `[{ index, id, href, media_type, linear, properties }]`
    #[wasm_bindgen(getter)]
    pub fn sections(&self) -> Result<JsValue, JsValue> {
        let all: Vec<_> = self.inner.sections().collect();
        to_js(&all)
    }

    /// Reading direction declared by the spine (`"ltr"`, `"rtl"`), if any
    #[wasm_bindgen(getter)]
    pub fn direction(&self) -> Option<String> {
        self.inner.page_progression_direction.clone()
    }

    /// `"pre-paginated"` for fixed-layout books, else `"reflowable"`
    #[wasm_bindgen(getter)]
    pub fn layout(&self) -> String {
        if self.inner.rendition_layout.as_deref() == Some("pre-paginated") {
            "pre-paginated".to_string()
        } else {
            "reflowable".to_string()
        }
    }

    /// Fixed-layout design size of a section from its `<meta name="viewport">`:
    /// `{ width, height }`, or `null` if not declared numerically
    pub fn section_viewport(&mut self, index: usize) -> Result<JsValue, JsValue> {
        let section = self.inner.load_section(index).map_err(js_err)?;
        match section.viewport() {
            Some((width, height)) => {
                let obj = js_sys::Object::new();
                js_sys::Reflect::set(&obj, &"width".into(), &width.into())?;
                js_sys::Reflect::set(&obj, &"height".into(), &height.into())?;
                Ok(obj.into())
            }
            None => Ok(JsValue::NULL),
        }
    }

    /// Get section metadata by index
    pub fn get_section(&self, index: usize) -> Result<JsValue, JsValue> {
        let section = self
            .inner
            .section(index)
            .ok_or_else(|| js_err(format!("Section {} not found", index)))?;
        to_js(section)
    }

    /// Raw XHTML of a section, exactly as stored in the EPUB
    pub fn get_section_content(&mut self, index: usize) -> Result<String, JsValue> {
        self.inner
            .section_content(index)
            .map(|s| s.to_string())
            .map_err(js_err)
    }

    /// Plain text of a section (whitespace collapsed)
    pub fn get_section_text(&mut self, index: usize) -> Result<String, JsValue> {
        self.inner
            .section_text(index)
            .map(|s| s.to_string())
            .map_err(js_err)
    }

    /// Section XHTML ready to display: images, stylesheets, fonts and other
    /// assets are rewritten to blob URLs; internal links become
    /// `<a href="#" data-epub-href="…" data-epub-section="N" data-epub-fragment="…">`;
    /// scripts are removed. Set the result as an iframe's `srcdoc`.
    ///
    /// `options`: `{ styles?: string, baseStyles?: boolean, stripScripts?: boolean, resolveLinks?: boolean }`
    pub fn render_section(&mut self, index: usize, options: JsValue) -> Result<String, JsValue> {
        let opts: RenderOptions = from_js(options)?;
        self.render_with(index, &opts)
    }

    /// Resolve an href (from the TOC, or from a link inside `section_index`)
    /// to `{ index, fragment }`, or `null` if it is external / not a section.
    pub fn resolve_href(&self, section_index: usize, href: &str) -> Result<JsValue, JsValue> {
        match self.inner.resolve_href(section_index, href) {
            Some((index, fragment)) => {
                let obj = js_sys::Object::new();
                js_sys::Reflect::set(&obj, &"index".into(), &(index as u32).into())?;
                js_sys::Reflect::set(
                    &obj,
                    &"fragment".into(),
                    &fragment.map(JsValue::from).unwrap_or(JsValue::NULL),
                )?;
                Ok(obj.into())
            }
            None => Ok(JsValue::NULL),
        }
    }

    /// Section index for an href (archive path, OPF-relative path, or bare
    /// filename). Fragments are ignored.
    pub fn section_index_for_href(&self, href: &str) -> Option<usize> {
        self.inner.section_index_by_href(href)
    }

    /// Raw bytes of a resource by href (archive path or OPF-relative)
    pub fn get_resource(&self, href: &str) -> Option<Vec<u8>> {
        self.inner.get_resource(href).map(|d| d.to_vec())
    }

    /// Blob URL for a resource (cached; revoke with `revoke_resources`)
    pub fn get_resource_url(&mut self, href: &str) -> Result<Option<String>, JsValue> {
        let Self { inner, resources, .. } = self;
        blob_url_for(inner, resources, "", href, 0)
    }

    /// MIME type for a resource href
    pub fn media_type(&self, href: &str) -> String {
        let (p, _) = path::resolve("", href);
        self.inner.media_type_for(&p)
    }

    /// Cover image bytes, if the book declares one
    pub fn get_cover(&self) -> Option<Vec<u8>> {
        self.inner.cover_image().map(|d| d.to_vec())
    }

    /// Blob URL of the cover image, if any
    pub fn get_cover_url(&mut self) -> Result<Option<String>, JsValue> {
        let Some(p) = self.inner.cover_path() else {
            return Ok(None);
        };
        let Self { inner, resources, .. } = self;
        blob_url_for(inner, resources, "", &p, 0)
    }

    /// Full-text search. `options`: `{ caseInsensitive?: boolean, maxResults?: number, contextChars?: number }`.
    /// Returns `[{ section_index, matched_text, excerpt, offset, cfi }]`.
    pub fn search(&mut self, query: &str, options: JsValue) -> Result<JsValue, JsValue> {
        let opts: SearchOptions = from_js(options)?;
        let matches = self.inner.search(query, &opts).map_err(js_err)?;
        to_js(&matches)
    }

    /// Build a stable position index: one location every `chars_per`
    /// characters of plain text (a common value is 1024). Loads and scans
    /// every section; returns the number of positions. Enables
    /// `percentage_at` and the `locations` getter.
    pub fn generate_locations(&mut self, chars_per: usize) -> Result<usize, JsValue> {
        let locations = self.inner.generate_locations(chars_per).map_err(js_err)?;
        let total = locations.total();
        self.locations = Some(locations);
        Ok(total)
    }

    /// The generated positions: `[{ cfi, section_index, offset, percentage }]`
    /// (empty until `generate_locations` is called)
    #[wasm_bindgen(getter)]
    pub fn locations(&self) -> Result<JsValue, JsValue> {
        match &self.locations {
            Some(l) => to_js(&l.locations),
            None => to_js(&Vec::<Location>::new()),
        }
    }

    /// Progress through the book (0–100) at `fraction` (0.0–1.0) of the way
    /// through a section. `undefined` until `generate_locations` is called.
    pub fn percentage_at(&self, section_index: usize, fraction: f64) -> Option<f64> {
        self.locations
            .as_ref()
            .map(|l| l.percentage_at(section_index, fraction))
    }

    /// Revoke every blob URL this book created. Call when discarding the book.
    pub fn revoke_resources(&mut self) {
        self.resources.revoke_all();
    }
}

impl JsBook {
    /// Access the underlying core book
    pub fn book(&self) -> &Book {
        &self.inner
    }

    /// Mutable access to the underlying core book
    pub fn book_mut(&mut self) -> &mut Book {
        &mut self.inner
    }

    pub(crate) fn render_with(&mut self, index: usize, opts: &RenderOptions) -> Result<String, JsValue> {
        let mut content = self.inner.section_content(index).map_err(js_err)?.to_string();
        let section_dir = self.inner.section(index).unwrap().base_path().to_string();

        if !opts.highlights.is_empty() {
            let map = self.inner.section_map(index).map_err(js_err)?;
            content = inject_marks(&content, map, &opts.highlights);
        }

        let Self { inner, resources, .. } = self;
        let mut first_err: Option<JsValue> = None;

        let html = rewrite_html(&content, opts.strip_scripts, |r: Reference<'_>| {
            if r.value.is_empty() {
                return None;
            }
            match r.kind {
                RefKind::Link => {
                    if !opts.resolve_links {
                        return None;
                    }
                    if path::is_external(r.value) {
                        return Some(Replacement::Attrs(vec![
                            ("href".into(), r.value.into()),
                            ("target".into(), "_blank".into()),
                            ("rel".into(), "noopener".into()),
                        ]));
                    }
                    match inner.resolve_href(index, r.value) {
                        Some((target, fragment)) => {
                            let target_href = inner.section(target).map(|s| s.href.clone()).unwrap_or_default();
                            let full = match &fragment {
                                Some(f) => format!("{}#{}", target_href, f),
                                None => target_href,
                            };
                            Some(Replacement::Attrs(vec![
                                ("href".into(), "#".into()),
                                ("data-epub-href".into(), full),
                                ("data-epub-section".into(), target.to_string()),
                                ("data-epub-fragment".into(), fragment.unwrap_or_default()),
                            ]))
                        }
                        // Link to a non-spine file (e.g. an image): serve it
                        None => match blob_url_for(inner, resources, &section_dir, r.value, 0) {
                            Ok(Some(url)) => Some(Replacement::Attrs(vec![
                                ("href".into(), url),
                                ("target".into(), "_blank".into()),
                            ])),
                            Ok(None) => None,
                            Err(e) => {
                                first_err.get_or_insert(e);
                                None
                            }
                        },
                    }
                }
                RefKind::Resource | RefKind::CssUrl => {
                    if path::is_external(r.value) || r.value.starts_with('#') {
                        return None;
                    }
                    match blob_url_for(inner, resources, &section_dir, r.value, 0) {
                        Ok(Some(url)) => Some(Replacement::Value(url)),
                        Ok(None) => None,
                        Err(e) => {
                            first_err.get_or_insert(e);
                            None
                        }
                    }
                }
            }
        });

        if let Some(e) = first_err {
            return Err(e);
        }

        let mut head = String::new();
        if opts.base_styles {
            head.push_str("<style>");
            head.push_str(BASE_STYLES);
            head.push_str("</style>");
        }
        if !opts.highlights.is_empty() {
            head.push_str("<style>");
            head.push_str(HIGHLIGHT_STYLES);
            head.push_str("</style>");
        }
        if let Some(css) = &opts.styles {
            head.push_str("<style>");
            head.push_str(css);
            head.push_str("</style>");
        }

        Ok(inject_into_head(&html, &head))
    }
}

/// Resolve `href` against `base_dir`, and return a (cached) blob URL for the
/// resource it points to. CSS files have their own `url()` references
/// rewritten first, so fonts and background images inside stylesheets work.
fn blob_url_for(
    book: &Book,
    resources: &mut Resources,
    base_dir: &str,
    href: &str,
    depth: u8,
) -> Result<Option<String>, JsValue> {
    let (p, _) = path::resolve(base_dir, href);
    let archive_path = match book.get_resource(&p) {
        Some(_) => p,
        None => {
            // Maybe OPF-relative
            let (alt, _) = path::resolve(book.base_path(), href);
            if book.get_resource(&alt).is_some() {
                alt
            } else {
                return Ok(None);
            }
        }
    };

    if let Some(url) = resources.get(&archive_path) {
        return Ok(Some(url.to_string()));
    }

    let data = book.get_resource(&archive_path).unwrap();
    let mime = book.media_type_for(&archive_path);

    if mime == "text/css" && depth < 3 {
        let css = String::from_utf8_lossy(data);
        let css_dir = path::dir_of(&archive_path).to_string();
        let mut err: Option<JsValue> = None;
        let rewritten = rewrite_css(&css, &mut |url| {
            if path::is_external(url) || url.starts_with('#') {
                return None;
            }
            match blob_url_for(book, resources, &css_dir, url, depth + 1) {
                Ok(v) => v,
                Err(e) => {
                    err.get_or_insert(e);
                    None
                }
            }
        });
        if let Some(e) = err {
            return Err(e);
        }
        return resources
            .register(&archive_path, rewritten.as_bytes(), &mime)
            .map(Some);
    }

    resources.register(&archive_path, data, &mime).map(Some)
}

/// JavaScript-friendly CFI wrapper
#[wasm_bindgen]
pub struct JsCfi {
    inner: Cfi,
}

#[wasm_bindgen]
impl JsCfi {
    /// Parse a CFI string
    #[wasm_bindgen(constructor)]
    pub fn new(cfi_string: &str) -> Result<JsCfi, JsValue> {
        let cfi = Cfi::parse(cfi_string).map_err(js_err)?;
        Ok(JsCfi { inner: cfi })
    }

    /// Create a CFI for a spine index
    pub fn from_spine_index(index: usize) -> JsCfi {
        JsCfi {
            inner: Cfi::from_spine_index(index),
        }
    }

    /// Get the spine index
    #[wasm_bindgen(getter)]
    pub fn spine_index(&self) -> usize {
        self.inner.spine_index
    }

    /// Get the character offset
    #[wasm_bindgen(getter)]
    pub fn character_offset(&self) -> Option<usize> {
        self.inner.character_offset
    }

    /// Convert to string
    #[wasm_bindgen(js_name = toString)]
    pub fn to_js_string(&self) -> String {
        self.inner.to_string()
    }

    /// Compare with another CFI (-1 = less, 0 = equal, 1 = greater)
    pub fn compare(&self, other: &JsCfi) -> i32 {
        match self.inner.compare(&other.inner) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

/// Get version information
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
