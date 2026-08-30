//! Rewrites references (`src`, `href`, `xlink:href`, `poster`, CSS `url()`)
//! inside an XHTML document without parsing it into a DOM, so the document
//! is otherwise emitted byte-for-byte. Pure Rust; unit-tested natively.

/// What kind of reference was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// `<a href>` / `<area href>` — a navigation link
    Link,
    /// `src`, `poster`, `xlink:href`, `<link href>`, `<image href>`, … — an asset
    Resource,
    /// `url(...)` inside a `<style>` element or `style=""` attribute
    CssUrl,
}

/// A reference handed to the callback.
#[derive(Debug, Clone, Copy)]
pub struct Reference<'a> {
    pub kind: RefKind,
    /// Lower-cased tag name (`"img"`, `"a"`, `"style"`, …)
    pub tag: &'a str,
    /// Attribute name as written (`"src"`, `"xlink:href"`, or `"url"` for CSS)
    pub attr: &'a str,
    /// The raw value (entities decoded for `&amp;` only)
    pub value: &'a str,
}

/// What to do with a reference.
pub enum Replacement {
    /// Replace only the value (works for every kind)
    Value(String),
    /// Replace the whole attribute with these `(name, value)` pairs
    /// (ignored for `CssUrl`)
    Attrs(Vec<(String, String)>),
}

/// Rewrite an HTML/XHTML document. `f` is called for every reference; return
/// `None` to leave it untouched. `<script>` elements are removed entirely when
/// `strip_scripts` is set.
pub fn rewrite_html<F>(html: &str, strip_scripts: bool, mut f: F) -> String
where
    F: FnMut(Reference<'_>) -> Option<Replacement>,
{
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len() + html.len() / 8);
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'<' {
            // Copy a run of plain text
            let start = i;
            while i < bytes.len() && bytes[i] != b'<' {
                i += 1;
            }
            out.push_str(&html[start..i]);
            continue;
        }

        let rest = &html[i..];

        // Constructs we copy verbatim
        let skip_to = if rest.starts_with("<!--") {
            Some(("-->", 3))
        } else if rest.starts_with("<![CDATA[") {
            Some(("]]>", 3))
        } else if rest.starts_with("<?") {
            Some(("?>", 2))
        } else if rest.starts_with("<!") || rest.starts_with("</") {
            Some((">", 1))
        } else {
            None
        };

        if let Some((terminator, len)) = skip_to {
            let end = rest.find(terminator).map(|p| p + len).unwrap_or(rest.len());
            out.push_str(&rest[..end]);
            i += end;
            continue;
        }

        // A start tag
        let Some(tag) = parse_tag(rest) else {
            // Not a well-formed tag (e.g. a stray '<'); copy the char
            out.push('<');
            i += 1;
            continue;
        };

        let tag_name = tag.name.to_ascii_lowercase();

        if strip_scripts && tag_name == "script" {
            i += tag.len;
            if !tag.self_closing {
                i += skip_past_close(&html[i..], "script");
            }
            continue;
        }

        // Rebuild the tag with rewritten attributes
        out.push_str(&rest[..tag.attrs_start]);
        let mut cursor = tag.attrs_start;

        for attr in &tag.attrs {
            // Text between previous attribute and this one (whitespace)
            out.push_str(&rest[cursor..attr.start]);
            cursor = attr.end;

            let attr_name_lc = attr.name.to_ascii_lowercase();
            let raw_value = attr.value.unwrap_or("");
            let value = decode_amp(raw_value);

            let kind = classify(&tag_name, &attr_name_lc);

            let replacement = match kind {
                Some(kind) if attr.value.is_some() => f(Reference {
                    kind,
                    tag: &tag_name,
                    attr: attr.name,
                    value: &value,
                }),
                None if attr_name_lc == "style" && attr.value.is_some() => {
                    let rewritten = rewrite_css(&value, &mut |url| {
                        match f(Reference {
                            kind: RefKind::CssUrl,
                            tag: &tag_name,
                            attr: "url",
                            value: url,
                        }) {
                            Some(Replacement::Value(v)) => Some(v),
                            _ => None,
                        }
                    });
                    if rewritten != value {
                        Some(Replacement::Value(rewritten))
                    } else {
                        None
                    }
                }
                _ => None,
            };

            match replacement {
                None => out.push_str(&rest[attr.start..attr.end]),
                Some(Replacement::Value(v)) => {
                    push_attr(&mut out, attr.name, &v);
                }
                Some(Replacement::Attrs(pairs)) => {
                    let mut first = true;
                    for (name, value) in pairs {
                        if !first {
                            out.push(' ');
                        }
                        first = false;
                        push_attr(&mut out, &name, &value);
                    }
                }
            }
        }

        out.push_str(&rest[cursor..tag.len]);
        i += tag.len;

        // <style> element content: rewrite url() references
        if tag_name == "style" && !tag.self_closing {
            let body_len = find_close(&html[i..], "style");
            let css = &html[i..i + body_len];
            let rewritten = rewrite_css(css, &mut |url| match f(Reference {
                kind: RefKind::CssUrl,
                tag: "style",
                attr: "url",
                value: url,
            }) {
                Some(Replacement::Value(v)) => Some(v),
                _ => None,
            });
            out.push_str(&rewritten);
            i += body_len;
        }
    }

    out
}

/// Rewrite every `url(...)` in a CSS string. `f` returns the replacement URL.
pub fn rewrite_css<F>(css: &str, f: &mut F) -> String
where
    F: FnMut(&str) -> Option<String>,
{
    let mut out = String::with_capacity(css.len());
    let mut rest = css;

    while let Some(pos) = find_ci(rest, "url(") {
        out.push_str(&rest[..pos + 4]);
        rest = &rest[pos + 4..];

        // Optional whitespace, optional quote
        let ws = rest.len() - rest.trim_start().len();
        out.push_str(&rest[..ws]);
        rest = &rest[ws..];

        let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'');
        let (value_end, close_len) = match quote {
            Some(q) => match rest[1..].find(q) {
                Some(p) => (p + 1, 1),
                None => break,
            },
            None => match rest.find(')') {
                Some(p) => (p, 0),
                None => break,
            },
        };

        let value_start = if quote.is_some() { 1 } else { 0 };
        let value = rest[value_start..value_end].trim();

        match f(value) {
            Some(new) => {
                // Always emit quoted so odd characters in blob/data URLs are safe
                out.push('"');
                out.push_str(&new.replace('"', "%22"));
                out.push('"');
                rest = &rest[value_end + close_len..];
            }
            None => {
                out.push_str(&rest[..value_end + close_len]);
                rest = &rest[value_end + close_len..];
            }
        }
    }

    out.push_str(rest);
    out
}

fn classify(tag: &str, attr: &str) -> Option<RefKind> {
    match attr {
        "src" | "poster" | "xlink:href" | "data" => Some(RefKind::Resource),
        "href" => match tag {
            "a" | "area" => Some(RefKind::Link),
            "link" | "image" | "use" => Some(RefKind::Resource),
            _ => None,
        },
        "srcset" => None, // not handled; rare in EPUB
        _ => None,
    }
}

fn push_attr(out: &mut String, name: &str, value: &str) {
    out.push_str(name);
    out.push_str("=\"");
    out.push_str(&escape_attr(value));
    out.push('"');
}

fn escape_attr(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;")
}

fn decode_amp(value: &str) -> String {
    if value.contains('&') {
        value
            .replace("&amp;", "&")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&apos;", "'")
    } else {
        value.to_string()
    }
}

struct Attr<'a> {
    name: &'a str,
    value: Option<&'a str>,
    /// Byte range of `name="value"` within the tag text
    start: usize,
    end: usize,
}

struct Tag<'a> {
    name: &'a str,
    attrs: Vec<Attr<'a>>,
    /// Offset where attributes begin (right after the name)
    attrs_start: usize,
    /// Total length of the tag including `<` and `>`
    len: usize,
    self_closing: bool,
}

/// Parse a start tag at the beginning of `s` (which starts with `<`).
fn parse_tag(s: &str) -> Option<Tag<'_>> {
    let b = s.as_bytes();
    let mut i = 1;

    // Tag name
    let name_start = i;
    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = &s[name_start..i];
    let attrs_start = i;

    let mut attrs = Vec::new();

    loop {
        // Skip whitespace
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        if b[i] == b'>' {
            return Some(Tag {
                name,
                attrs,
                attrs_start,
                len: i + 1,
                self_closing: false,
            });
        }
        if b[i] == b'/' {
            // Expect `/>`
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && b[j] == b'>' {
                return Some(Tag {
                    name,
                    attrs,
                    attrs_start,
                    len: j + 1,
                    self_closing: true,
                });
            }
            i += 1;
            continue;
        }

        // Attribute name
        let attr_start = i;
        while i < b.len()
            && !b[i].is_ascii_whitespace()
            && b[i] != b'='
            && b[i] != b'>'
            && b[i] != b'/'
        {
            i += 1;
        }
        let attr_name = &s[attr_start..i];
        if attr_name.is_empty() {
            i += 1;
            continue;
        }

        // Optional `= value`
        let mut j = i;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < b.len() && b[j] == b'=' {
            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= b.len() {
                return None;
            }
            let (value, end) = if b[j] == b'"' || b[j] == b'\'' {
                let q = b[j];
                let vs = j + 1;
                let mut k = vs;
                while k < b.len() && b[k] != q {
                    k += 1;
                }
                if k >= b.len() {
                    return None;
                }
                (&s[vs..k], k + 1)
            } else {
                let vs = j;
                let mut k = vs;
                while k < b.len() && !b[k].is_ascii_whitespace() && b[k] != b'>' {
                    k += 1;
                }
                (&s[vs..k], k)
            };
            attrs.push(Attr {
                name: attr_name,
                value: Some(value),
                start: attr_start,
                end,
            });
            i = end;
        } else {
            attrs.push(Attr {
                name: attr_name,
                value: None,
                start: attr_start,
                end: i,
            });
        }
    }
}

/// Length of content up to (not including) `</tag`, case-insensitive.
fn find_close(s: &str, tag: &str) -> usize {
    let needle = format!("</{}", tag);
    find_ci(s, &needle).unwrap_or(s.len())
}

/// Length of content up to and including `</tag ...>`.
fn skip_past_close(s: &str, tag: &str) -> usize {
    let body = find_close(s, tag);
    match s[body..].find('>') {
        Some(p) => body + p + 1,
        None => s.len(),
    }
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}

/// Insert `fragment` just before `</head>`; if there is no head, create one
/// before `<body`; otherwise prepend.
pub fn inject_into_head(html: &str, fragment: &str) -> String {
    if fragment.is_empty() {
        return html.to_string();
    }
    if let Some(pos) = find_ci(html, "</head>") {
        return format!("{}{}{}", &html[..pos], fragment, &html[pos..]);
    }
    if let Some(pos) = find_ci(html, "<body") {
        return format!("{}<head>{}</head>{}", &html[..pos], fragment, &html[pos..]);
    }
    format!("{}{}", fragment, html)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upper(r: Reference<'_>) -> Option<Replacement> {
        Some(Replacement::Value(format!("BLOB:{}", r.value)))
    }

    #[test]
    fn rewrites_common_attributes() {
        let html = r##"<html><head><link rel="stylesheet" href="../s.css"/></head>
<body><img src='a.png' alt="x"/><video poster="p.jpg"><source src="v.mp4"/></video>
<svg><image xlink:href="i.jpg"/><use href="#sym"/></svg><img src=bare.png></body></html>"##;

        let out = rewrite_html(html, true, upper);
        assert!(out.contains(r#"href="BLOB:../s.css""#));
        assert!(out.contains(r#"src="BLOB:a.png" alt="x""#));
        assert!(out.contains(r#"poster="BLOB:p.jpg""#));
        assert!(out.contains(r#"src="BLOB:v.mp4""#));
        assert!(out.contains(r#"xlink:href="BLOB:i.jpg""#));
        assert!(out.contains(r#"href="BLOB:#sym""#));
        assert!(out.contains(r#"src="BLOB:bare.png">"#));
    }

    #[test]
    fn links_are_reported_separately_and_can_become_attr_sets() {
        let html = r#"<p><a href="ch2.xhtml#s" class="x">go</a> <a href="https://e.com">out</a></p>"#;
        let out = rewrite_html(html, true, |r| {
            assert_eq!(r.kind, RefKind::Link);
            assert_eq!(r.tag, "a");
            if r.value.starts_with("http") {
                None
            } else {
                Some(Replacement::Attrs(vec![
                    ("href".into(), "#".into()),
                    ("data-epub-href".into(), format!("X/{}", r.value)),
                ]))
            }
        });
        assert_eq!(
            out,
            r##"<p><a href="#" data-epub-href="X/ch2.xhtml#s" class="x">go</a> <a href="https://e.com">out</a></p>"##
        );
    }

    #[test]
    fn untouched_documents_round_trip_exactly() {
        let html = r#"<?xml version="1.0"?><!DOCTYPE html><html><!-- <img src="c.png"> --><body><p a=b c='d' e>t</p><br/><![CDATA[<img src="x">]]></body></html>"#;
        let out = rewrite_html(html, false, |_| None);
        assert_eq!(out, html);
    }

    #[test]
    fn comments_and_cdata_are_not_rewritten() {
        let html = r#"<body><!-- <img src="c.png"> --><![CDATA[<img src="d.png">]]><img src="e.png"/></body>"#;
        let out = rewrite_html(html, false, upper);
        assert!(out.contains(r#"<!-- <img src="c.png"> -->"#));
        assert!(out.contains(r#"<![CDATA[<img src="d.png">]]>"#));
        assert!(out.contains(r#"src="BLOB:e.png""#));
    }

    #[test]
    fn scripts_are_stripped() {
        let html = r#"<body><script type="text/javascript">alert("<img src='x'>")</script><p>ok</p><script src="a.js"/></body>"#;
        let out = rewrite_html(html, true, upper);
        assert_eq!(out, "<body><p>ok</p></body>");

        let kept = rewrite_html(html, false, |_| None);
        assert_eq!(kept, html);
    }

    #[test]
    fn css_urls_in_style_elements_and_attributes() {
        let html = r#"<head><style>
body { background: url("bg.png"); }
@font-face { src: url(f.woff2) format("woff2"), url( 'g.ttf' ); }
</style></head><body><p style="background:url(inline.png)">x</p></body>"#;
        let out = rewrite_html(html, true, |r| {
            assert_eq!(r.kind, RefKind::CssUrl);
            Some(Replacement::Value(format!("B:{}", r.value)))
        });
        assert!(out.contains(r#"url("B:bg.png")"#));
        assert!(out.contains(r#"url("B:f.woff2")"#));
        assert!(out.contains(r#"url( "B:g.ttf" )"#));
        assert!(out.contains(r#"style="background:url(&quot;B:inline.png&quot;)""#));
    }

    #[test]
    fn amp_entities_are_decoded_for_callback_and_reencoded() {
        let html = r#"<a href="x.xhtml?a=1&amp;b=2">l</a>"#;
        let out = rewrite_html(html, true, |r| {
            assert_eq!(r.value, "x.xhtml?a=1&b=2");
            Some(Replacement::Value(r.value.to_string()))
        });
        assert_eq!(out, html);
    }

    #[test]
    fn injects_styles_into_head() {
        assert_eq!(
            inject_into_head("<html><head><title>t</title></head><body/></html>", "<style>x</style>"),
            "<html><head><title>t</title><style>x</style></head><body/></html>"
        );
        assert_eq!(
            inject_into_head("<html><body>b</body></html>", "<style>x</style>"),
            "<html><head><style>x</style></head><body>b</body></html>"
        );
        assert_eq!(inject_into_head("<p>x</p>", "S"), "S<p>x</p>");
    }

    #[test]
    fn malformed_input_does_not_panic() {
        for s in ["<", "<img src=\"unterminated", "<a href=", "<<<>>>", "</", "<!--", "<style>body{", "url("] {
            let _ = rewrite_html(s, true, upper);
            let _ = rewrite_css(s, &mut |u| Some(u.to_string()));
        }
    }
}
