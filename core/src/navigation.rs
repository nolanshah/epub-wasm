//! Navigation parsing (NAV for EPUB3, NCX for EPUB2)

use quick_xml::events::Event;
use quick_xml::Reader;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

use crate::error::{EpubError, Result};

/// A navigation item (TOC entry)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavItem {
    pub id: Option<String>,
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

/// Find the TOC nav element using various strategies
fn find_toc_nav(document: &Html) -> Result<scraper::ElementRef<'_>> {
    // Strategy 1: Try nav elements and check for epub:type attribute
    let nav_selector = Selector::parse("nav").unwrap();
    for nav in document.select(&nav_selector) {
        // Check for epub:type="toc" attribute
        if let Some(epub_type) = nav.value().attr("epub:type") {
            if epub_type == "toc" {
                return Ok(nav);
            }
        }
        // Check for class="toc"
        if let Some(class) = nav.value().attr("class") {
            if class.contains("toc") {
                return Ok(nav);
            }
        }
        // Check for id="toc"
        if let Some(id) = nav.value().attr("id") {
            if id.contains("toc") {
                return Ok(nav);
            }
        }
    }

    // Strategy 2: Just find first nav with an ol/ul
    for nav in document.select(&nav_selector) {
        let list_selector = Selector::parse("ol, ul").unwrap();
        if nav.select(&list_selector).next().is_some() {
            return Ok(nav);
        }
    }

    Err(EpubError::InvalidStructure("No TOC nav element found".to_string()))
}

/// Parse EPUB3 NAV document (HTML-based)
pub fn parse_nav(html: &str) -> Result<Vec<NavItem>> {
    let document = Html::parse_document(html);

    // Find the TOC nav element - try multiple selector strategies
    let nav = find_toc_nav(&document)?;

    // Find the top-level ol/ul
    let list_selector = Selector::parse("ol, ul")
        .map_err(|_| EpubError::InvalidStructure("Invalid selector".to_string()))?;

    let list = nav
        .select(&list_selector)
        .next()
        .ok_or_else(|| EpubError::InvalidStructure("No list in TOC nav".to_string()))?;

    parse_nav_list(&list)
}

fn parse_nav_list(list: &scraper::ElementRef) -> Result<Vec<NavItem>> {
    let li_selector =
        Selector::parse(":scope > li").unwrap_or_else(|_| Selector::parse("li").unwrap());
    let a_selector = Selector::parse("a").unwrap();
    let ol_selector = Selector::parse("ol, ul").unwrap();

    let mut items = Vec::new();

    for li in list.select(&li_selector) {
        if let Some(a) = li.select(&a_selector).next() {
            let href = a.value().attr("href").unwrap_or("").to_string();
            let label = a.text().collect::<Vec<_>>().join(" ").trim().to_string();
            let id = a.value().attr("id").map(|s| s.to_string());

            let mut item = NavItem {
                id,
                href,
                label,
                children: Vec::new(),
            };

            // Check for nested list
            if let Some(nested_list) = li.select(&ol_selector).next() {
                item.children = parse_nav_list(&nested_list)?;
            }

            items.push(item);
        }
    }

    Ok(items)
}

/// Parse EPUB2 NCX document (XML-based)
pub fn parse_ncx(xml: &str) -> Result<Vec<NavItem>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut items = Vec::new();
    let mut stack: Vec<NavItem> = Vec::new();
    let mut current_text = String::new();
    let mut current_src = String::new();
    let mut in_text = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                match name.as_str() {
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
                    _ => {}
                }
            }

            Ok(Event::Empty(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                if name == "content" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"src" {
                            current_src = String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }

                    if let Some(item) = stack.last_mut() {
                        item.href = current_src.clone();
                    }
                }
            }

            Ok(Event::End(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                match name.as_str() {
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
                            item.label = current_text.trim().to_string();
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
        assert_eq!(items[1].children.len(), 1);
    }
}
