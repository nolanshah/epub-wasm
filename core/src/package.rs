//! OPF Package parsing (metadata, manifest, spine)

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::{Deserialize, Serialize};

use crate::error::{EpubError, Result};

/// Book metadata from the OPF file
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metadata {
    pub title: String,
    pub creators: Vec<String>,
    pub language: Option<String>,
    pub identifier: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub date: Option<String>,
    pub subjects: Vec<String>,
    pub rights: Option<String>,
    pub cover_id: Option<String>,
}

/// An item in the manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestItem {
    pub id: String,
    /// href exactly as written in the OPF (relative to the OPF directory)
    pub href: String,
    pub media_type: String,
    pub properties: Vec<String>,
}

/// An item in the spine (reading order)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpineItem {
    pub id: String,
    pub idref: String,
    pub linear: bool,
    pub properties: Vec<String>,
}

/// The parsed OPF package
#[derive(Debug, Clone)]
pub struct Package {
    pub metadata: Metadata,
    pub manifest: HashMap<String, ManifestItem>,
    pub spine: Vec<SpineItem>,
    /// href (relative to OPF dir) of the EPUB3 navigation document
    pub nav_path: Option<String>,
    /// href (relative to OPF dir) of the EPUB2 NCX document
    pub ncx_path: Option<String>,
    /// `page-progression-direction` from `<spine>` (`ltr`, `rtl`, `default`)
    pub page_progression_direction: Option<String>,
}

/// Local (namespace-stripped) element name as an owned String.
fn local(name: &[u8]) -> String {
    let name = match name.iter().rposition(|&b| b == b':') {
        Some(pos) => &name[pos + 1..],
        None => name,
    };
    String::from_utf8_lossy(name).to_string()
}

fn attr_string(e: &BytesStart, key: &[u8]) -> Result<Option<String>> {
    for attr in e.attributes() {
        let attr = attr?;
        if attr.key.as_ref() == key {
            return Ok(Some(String::from_utf8_lossy(&attr.value).to_string()));
        }
    }
    Ok(None)
}

impl Package {
    /// Parse an OPF file
    pub fn parse(xml: &str) -> Result<Self> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        // Treat `<item/>` and `<item></item>` identically.
        reader.config_mut().expand_empty_elements = true;

        let mut metadata = Metadata::default();
        let mut manifest: HashMap<String, ManifestItem> = HashMap::new();
        let mut spine: Vec<SpineItem> = Vec::new();
        let mut nav_path = None;
        let mut ncx_path = None;
        let mut spine_toc_id: Option<String> = None;
        let mut page_progression_direction = None;

        let mut buf = Vec::new();
        let mut in_metadata = false;
        let mut current_element: Option<String> = None;
        let mut text_content = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = local(e.name().as_ref());

                    match name.as_str() {
                        "metadata" => in_metadata = true,
                        "item" => {
                            let item = parse_manifest_item(e)?;

                            // EPUB3 navigation document
                            if item.properties.iter().any(|p| p == "nav") {
                                nav_path = Some(item.href.clone());
                            }

                            // EPUB2 NCX (by media type; the spine `toc` attribute is checked below)
                            if item.media_type == "application/x-dtbncx+xml" && ncx_path.is_none() {
                                ncx_path = Some(item.href.clone());
                            }

                            manifest.insert(item.id.clone(), item);
                        }
                        "itemref" => {
                            spine.push(parse_spine_item(e)?);
                        }
                        "spine" => {
                            spine_toc_id = attr_string(e, b"toc")?;
                            page_progression_direction =
                                attr_string(e, b"page-progression-direction")?;
                        }
                        "meta" if in_metadata => {
                            // EPUB2 cover: <meta name="cover" content="cover-image-id"/>
                            if attr_string(e, b"name")?.as_deref() == Some("cover") {
                                if let Some(content) = attr_string(e, b"content")? {
                                    metadata.cover_id = Some(content);
                                }
                            }
                        }
                        _ if in_metadata => {
                            current_element = Some(name);
                            text_content.clear();
                        }
                        _ => {}
                    }
                }

                Ok(Event::End(ref e)) => {
                    let name = local(e.name().as_ref());

                    match name.as_str() {
                        "metadata" => in_metadata = false,
                        _ if in_metadata => {
                            if current_element.as_deref() == Some(name.as_str()) {
                                let value = text_content.trim().to_string();
                                match name.as_str() {
                                    "title" => metadata.title = value,
                                    "creator" => metadata.creators.push(value),
                                    "language" => metadata.language = Some(value),
                                    "identifier" => {
                                        // Prefer the first identifier (usually the unique-identifier)
                                        if metadata.identifier.is_none() {
                                            metadata.identifier = Some(value)
                                        }
                                    }
                                    "description" => metadata.description = Some(value),
                                    "publisher" => metadata.publisher = Some(value),
                                    "date" => metadata.date = Some(value),
                                    "subject" => metadata.subjects.push(value),
                                    "rights" => metadata.rights = Some(value),
                                    _ => {}
                                }
                                current_element = None;
                                text_content.clear();
                            }
                        }
                        _ => {}
                    }
                }

                Ok(Event::Text(ref e)) => {
                    if in_metadata && current_element.is_some() {
                        text_content.push_str(&e.unescape().unwrap_or_default());
                    }
                }

                Ok(Event::CData(ref e)) => {
                    if in_metadata && current_element.is_some() {
                        text_content.push_str(&String::from_utf8_lossy(e));
                    }
                }

                Ok(Event::Eof) => break,
                Err(e) => return Err(e.into()),
                _ => {}
            }
            buf.clear();
        }

        // `<spine toc="ncx">` is the authoritative NCX reference in EPUB2.
        if let Some(id) = spine_toc_id {
            if let Some(item) = manifest.get(&id) {
                ncx_path = Some(item.href.clone());
            }
        }

        Ok(Package {
            metadata,
            manifest,
            spine,
            nav_path,
            ncx_path,
            page_progression_direction,
        })
    }
}

fn parse_manifest_item(e: &BytesStart) -> Result<ManifestItem> {
    let mut id = String::new();
    let mut href = String::new();
    let mut media_type = String::new();
    let mut properties = Vec::new();

    for attr in e.attributes() {
        let attr = attr?;
        match attr.key.as_ref() {
            b"id" => id = String::from_utf8_lossy(&attr.value).to_string(),
            b"href" => href = String::from_utf8_lossy(&attr.value).to_string(),
            b"media-type" => media_type = String::from_utf8_lossy(&attr.value).to_string(),
            b"properties" => {
                properties = String::from_utf8_lossy(&attr.value)
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
            }
            _ => {}
        }
    }

    if id.is_empty() {
        return Err(EpubError::InvalidStructure(
            "Manifest item missing id".to_string(),
        ));
    }

    Ok(ManifestItem {
        id,
        href,
        media_type,
        properties,
    })
}

fn parse_spine_item(e: &BytesStart) -> Result<SpineItem> {
    let mut idref = String::new();
    let mut id = String::new();
    let mut linear = true;
    let mut properties = Vec::new();

    for attr in e.attributes() {
        let attr = attr?;
        match attr.key.as_ref() {
            b"idref" => idref = String::from_utf8_lossy(&attr.value).to_string(),
            b"id" => id = String::from_utf8_lossy(&attr.value).to_string(),
            b"linear" => linear = attr.value.as_ref() != b"no",
            b"properties" => {
                properties = String::from_utf8_lossy(&attr.value)
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
            }
            _ => {}
        }
    }

    if idref.is_empty() {
        return Err(EpubError::InvalidStructure(
            "Spine itemref missing idref".to_string(),
        ));
    }

    // Generate id from idref if not provided
    if id.is_empty() {
        id = format!("spine-{}", idref);
    }

    Ok(SpineItem {
        id,
        idref,
        linear,
        properties,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_metadata() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:creator>Test Author</dc:creator>
    <dc:language>en</dc:language>
    <dc:identifier>urn:isbn:1234567890</dc:identifier>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="chapter1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="chapter1"/>
  </spine>
</package>"#;

        let package = Package::parse(xml).unwrap();
        assert_eq!(package.metadata.title, "Test Book");
        assert_eq!(package.metadata.creators, vec!["Test Author"]);
        assert_eq!(package.metadata.language, Some("en".to_string()));
        assert_eq!(package.manifest.len(), 2);
        assert_eq!(package.spine.len(), 1);
        assert_eq!(package.nav_path, Some("nav.xhtml".to_string()));
    }

    #[test]
    fn parses_prefixed_elements_and_non_self_closing_items() {
        // Some producers prefix every OPF element with `opf:` and write
        // `<item></item>` instead of `<item/>`.
        let xml = r#"<?xml version="1.0"?>
<opf:package xmlns:opf="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="2.0">
  <opf:metadata>
    <dc:title>Prefixed</dc:title>
    <opf:meta name="cover" content="cover-img"></opf:meta>
    <dc:identifier id="a">first</dc:identifier>
    <dc:identifier id="b">second</dc:identifier>
  </opf:metadata>
  <opf:manifest>
    <opf:item id="cover-img" href="cover.jpg" media-type="image/jpeg"></opf:item>
    <opf:item id="toc" href="toc.ncx" media-type="application/x-dtbncx+xml"></opf:item>
    <opf:item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"></opf:item>
  </opf:manifest>
  <opf:spine toc="toc" page-progression-direction="rtl">
    <opf:itemref idref="c1"></opf:itemref>
  </opf:spine>
</opf:package>"#;

        let package = Package::parse(xml).unwrap();
        assert_eq!(package.metadata.title, "Prefixed");
        assert_eq!(package.metadata.cover_id, Some("cover-img".to_string()));
        assert_eq!(package.metadata.identifier, Some("first".to_string()));
        assert_eq!(package.manifest.len(), 3);
        assert_eq!(package.spine.len(), 1);
        assert_eq!(package.ncx_path, Some("toc.ncx".to_string()));
        assert_eq!(package.page_progression_direction, Some("rtl".to_string()));
    }

    #[test]
    fn epub3_meta_elements_do_not_clobber_metadata() {
        let xml = r##"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title id="t">Real Title</dc:title>
    <meta refines="#t" property="title-type">main</meta>
    <meta property="dcterms:modified">2024-01-01T00:00:00Z</meta>
  </metadata>
  <manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest>
  <spine><itemref idref="c1"/></spine>
</package>"##;

        let package = Package::parse(xml).unwrap();
        assert_eq!(package.metadata.title, "Real Title");
        assert_eq!(package.metadata.cover_id, None);
    }
}
