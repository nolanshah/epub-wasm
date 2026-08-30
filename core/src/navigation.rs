//! Navigation parsing (NAV for EPUB3, NCX for EPUB2)

use quick_xml::events::Event;
use quick_xml::Reader;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};

use crate::error::{EpubError, Result};
use crate::path;

/// A navigation item (TOC entry)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavItem {
    pub id: Option<String>,
    /// Target. After `Book` loads it this is a normalized archive path plus
    /// optional `#fragment` (e.g. `OEBPS/ch1.xhtml#sec2`); empty for
    /// heading-only entries.
    pub href: String,
    pub label: String,
    pub children: Vec<NavItem>,
}

impl NavItem {
    /// Create a new navigation item
    pub fn new(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            id: None,
            href: href.into(),
            label: label.into(),
            children: Vec::new(),
        }
    }

    /// Flatten the navigation tree into a list
    pub fn flatten(&self) -> Vec<&NavItem> {
        let mut items = vec![self];
        for child in &self.children {
            items.extend(child.flatten());
        }
        items
    }
}

/// Rewrite every href in the tree to a normalized archive path (keeping any
/// fragment), resolving relative to `base_dir` (the directory of the document
/// the TOC came from). External URLs are left untouched.
pub fn resolve_hrefs(items: &mut [NavItem], base_dir: &str) {
    for item in items {
        if !item.href.is_empty() && !path::is_external(&item.href) {
            let (p, frag) = path::resolve(base_dir, &item.href);
            item.href = match frag {
                Some(f) => format!("{}#{}", p, f),
                None => p,
            };
        }
        resolve_hrefs(&mut item.children, base_dir);
    }
}

/// Find the TOC nav element using various strategies
fn find_toc_nav(document: &Html) -> Result<ElementRef<'_>> {
    let nav_selector = Selector::parse("nav").unwrap();

    // Strategy 1: nav with epub:type="toc" (may be a space-separated list)
    for nav in document.select(&nav_selector) {
        if let Some(epub_type) = nav.value().attr("epub:type") {
            if epub_type.split_whitespace().any(|t| t == "toc") {
                return Ok(nav);
            }
        }
    }

    // Strategy 2: class/id hints
    for nav in document.select(&nav_selector) {
        let hinted = nav
            .value()
            .attr("class")
            .map(|c| c.contains("toc"))
            .unwrap_or(false)
            || nav
                .value()
                .attr("id")
                .map(|i| i.contains("toc"))
                .unwrap_or(false);
        if hinted {
            return Ok(nav);
        }
    }

    // Strategy 3: first nav with an ol/ul
    let list_selector = Selector::parse("ol, ul").unwrap();
    for nav in document.select(&nav_selector) {
        if nav.select(&list_selector).next().is_some() {
            return Ok(nav);
        }
    }

    Err(EpubError::InvalidStructure(
        "No TOC nav element found".to_string(),
    ))
}

/// Parse EPUB3 NAV document (HTML-based). Hrefs are returned as written.
pub fn parse_nav(html: &str) -> Result<Vec<NavItem>> {
    let document = Html::parse_document(html);
    let nav = find_toc_nav(&document)?;

    let list_selector = Selector::parse(":scope > ol, :scope > ul").unwrap();
    let any_list_selector = Selector::parse("ol, ul").unwrap();

    let list = nav
        .select(&list_selector)
        .next()
        .or_else(|| nav.select(&any_list_selector).next())
        .ok_or_else(|| EpubError::InvalidStructure("No list in TOC nav".to_string()))?;

    parse_nav_list(&list)
}

fn parse_nav_list(list: &ElementRef) -> Result<Vec<NavItem>> {
    let li_selector = Selector::parse(":scope > li").unwrap();
    let direct_a = Selector::parse(":scope > a").unwrap();
    let direct_span = Selector::parse(":scope > span").unwrap();
    let nested_list = Selector::parse(":scope > ol, :scope > ul").unwrap();
    let any_a = Selector::parse("a").unwrap();

    let mut items = Vec::new();

    for li in list.select(&li_selector) {
        let nested = li.select(&nested_list).next();

        // EPUB3 allows either <a href> or a heading-only <span> as the label.
        let (href, label, id) = if let Some(a) = li.select(&direct_a).next() {
            (
                a.value().attr("href").unwrap_or("").to_string(),
                text_of(&a),
                a.value().attr("id").map(str::to_string),
            )
        } else if let Some(span) = li.select(&direct_span).next() {
            (
                String::new(),
                text_of(&span),
                span.value().attr("id").map(str::to_string),
            )
        } else if nested.is_none() {
            // Tolerate wrappers like <li><p><a>…</a></p></li> when there is
            // no nested list to confuse the lookup.
            match li.select(&any_a).next() {
                Some(a) => (
                    a.value().attr("href").unwrap_or("").to_string(),
                    text_of(&a),
                    a.value().attr("id").map(str::to_string),
                ),
                None => continue,
            }
        } else {
            continue;
        };

        let children = match nested {
            Some(list) => parse_nav_list(&list)?,
            None => Vec::new(),
        };

        items.push(NavItem {
            id,
            href,
            label,
            children,
        });
    }

    Ok(items)
}

fn text_of(el: &ElementRef) -> String {
    el.text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse EPUB2 NCX document (XML-based). Hrefs are returned as written.
pub fn parse_ncx(xml: &str) -> Result<Vec<NavItem>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    reader.config_mut().expand_empty_elements = true;

    let mut items = Vec::new();
    let mut stack: Vec<NavItem> = Vec::new();
    let mut current_text = String::new();
    let mut in_text = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let qname = e.name();
                let name = local_name(qname.as_ref());

                match name {
                    "navPoint" => {
                        let mut id = None;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"id" {
                                id = Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }

                        stack.push(NavItem {
                            id,
                            href: String::new(),
                            label: String::new(),
                            children: Vec::new(),
                        });
                    }
                    "text" => {
                        in_text = true;
                        current_text.clear();
                    }
                    "content" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"src" {
                                if let Some(item) = stack.last_mut() {
                                    item.href = String::from_utf8_lossy(&attr.value).to_string();
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            Ok(Event::End(ref e)) => {
                let qname = e.name();
                let name = local_name(qname.as_ref());

                match name {
                    "navPoint" => {
                        if let Some(item) = stack.pop() {
                            if let Some(parent) = stack.last_mut() {
                                parent.children.push(item);
                            } else {
                                items.push(item);
                            }
                        }
                    }
                    "text" => {
                        in_text = false;
                        if let Some(item) = stack.last_mut() {
                            // Only the navPoint's own navLabel sets the label;
                            // a child's label arrives after this item's label
                            // is already set.
                            if item.label.is_empty() {
                                item.label = current_text.trim().to_string();
                            }
                        }
                    }
                    _ => {}
                }
            }

            Ok(Event::Text(ref e)) => {
                if in_text {
                    current_text.push_str(&e.unescape().unwrap_or_default());
                }
            }

            Ok(Event::Eof) => break,
            Err(e) => return Err(e.into()),
            _ => {}
        }
        buf.clear();
    }

    Ok(items)
}

fn local_name(name: &[u8]) -> &str {
    let name = match name.iter().rposition(|&b| b == b':') {
        Some(pos) => &name[pos + 1..],
        None => name,
    };
    std::str::from_utf8(name).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nav() {
        let html = r#"<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>TOC</title></head>
<body>
<nav epub:type="toc">
  <ol>
    <li><a href="chapter1.xhtml">Chapter 1</a></li>
    <li><a href="chapter2.xhtml">Chapter 2</a>
      <ol>
        <li><a href="chapter2.xhtml#section1">Section 2.1</a></li>
      </ol>
    </li>
  </ol>
</nav>
</body>
</html>"#;

        let items = parse_nav(html).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "Chapter 1");
        assert_eq!(items[1].children.len(), 1);
        assert_eq!(items[1].children[0].label, "Section 2.1");
    }

    #[test]
    fn nav_span_heading_with_nested_list() {
        // A <span> heading must not steal the first nested <a>.
        let html = r#"<html><body>
<nav epub:type="toc"><ol>
  <li><span>Part One</span>
    <ol>
      <li><a href="c1.xhtml">Chapter 1</a></li>
      <li><a href="c2.xhtml">Chapter 2</a></li>
    </ol>
  </li>
  <li><a href="c3.xhtml">Chapter 3</a></li>
</ol></nav>
</body></html>"#;

        let items = parse_nav(html).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "Part One");
        assert_eq!(items[0].href, "");
        assert_eq!(items[0].children.len(), 2);
        assert_eq!(items[0].children[0].href, "c1.xhtml");
        assert_eq!(items[1].label, "Chapter 3");
    }

    #[test]
    fn nav_picks_toc_over_landmarks() {
        let html = r#"<html><body>
<nav epub:type="landmarks"><ol><li><a href="cover.xhtml">Cover</a></li></ol></nav>
<nav epub:type="toc"><ol><li><a href="c1.xhtml">Chapter 1</a></li></ol></nav>
</body></html>"#;

        let items = parse_nav(html).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Chapter 1");
    }

    #[test]
    fn test_parse_ncx() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <navMap>
    <navPoint id="ch1">
      <navLabel><text>Chapter 1</text></navLabel>
      <content src="chapter1.xhtml"/>
    </navPoint>
    <navPoint id="ch2">
      <navLabel><text>Chapter 2</text></navLabel>
      <content src="chapter2.xhtml"/>
      <navPoint id="ch2-1">
        <navLabel><text>Section 2.1</text></navLabel>
        <content src="chapter2.xhtml#section1"/>
      </navPoint>
    </navPoint>
  </navMap>
</ncx>"#;

        let items = parse_ncx(xml).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "Chapter 1");
        assert_eq!(items[0].href, "chapter1.xhtml");
        assert_eq!(items[1].label, "Chapter 2");
        assert_eq!(items[1].children.len(), 1);
        assert_eq!(items[1].children[0].label, "Section 2.1");
        assert_eq!(items[1].children[0].href, "chapter2.xhtml#section1");
    }

    #[test]
    fn ncx_with_prefixed_elements_and_non_self_closing_content() {
        let xml = r#"<ncx:ncx xmlns:ncx="http://www.daisy.org/z3986/2005/ncx/">
  <ncx:navMap>
    <ncx:navPoint id="a">
      <ncx:navLabel><ncx:text>One</ncx:text></ncx:navLabel>
      <ncx:content src="one.xhtml"></ncx:content>
    </ncx:navPoint>
  </ncx:navMap>
</ncx:ncx>"#;

        let items = parse_ncx(xml).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "One");
        assert_eq!(items[0].href, "one.xhtml");
    }

    #[test]
    fn resolves_hrefs_against_base_dir() {
        let mut items = vec![NavItem {
            id: None,
            href: "../Text/My%20Ch.xhtml#s1".into(),
            label: "x".into(),
            children: vec![
                NavItem::new("ext", "https://example.com"),
                NavItem::new("heading", ""),
            ],
        }];
        resolve_hrefs(&mut items, "OEBPS/Nav/");
        assert_eq!(items[0].href, "OEBPS/Text/My Ch.xhtml#s1");
        assert_eq!(items[0].children[0].href, "https://example.com");
        assert_eq!(items[0].children[1].href, "");
    }
}
