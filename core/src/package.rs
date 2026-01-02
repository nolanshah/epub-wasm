//! OPF Package parsing (metadata, manifest, spine)

use std::collections::HashMap;

use quick_xml::events::Event;
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
    pub nav_path: Option<String>,
    pub ncx_path: Option<String>,
}

impl Package {
    /// Parse an OPF file
    pub fn parse(xml: &str) -> Result<Self> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut metadata = Metadata::default();
        let mut manifest: HashMap<String, ManifestItem> = HashMap::new();
        let mut spine: Vec<SpineItem> = Vec::new();
        let mut nav_path = None;
        let mut ncx_path = None;

        let mut buf = Vec::new();
        let mut in_metadata = false;
        let mut current_element: Option<String> = None;
        let mut text_content = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    match name.as_str() {
                        "metadata" | "opf:metadata" => in_metadata = true,
                        _ if in_metadata => {
                            current_element = Some(name);
                            text_content.clear();
                        }
                        _ => {}
                    }
                }

                Ok(Event::End(ref e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    match name.as_str() {
                        "metadata" | "opf:metadata" => in_metadata = false,
                        _ if in_metadata => {
                            if let Some(ref elem) = current_element {
                                match elem.as_str() {
                                    "dc:title" | "title" => {
                                        metadata.title = text_content.trim().to_string();
                                    }
                                    "dc:creator" | "creator" => {
                                        metadata.creators.push(text_content.trim().to_string());
                                    }
                                    "dc:language" | "language" => {
                                        metadata.language = Some(text_content.trim().to_string());
                                    }
                                    "dc:identifier" | "identifier" => {
                                        metadata.identifier = Some(text_content.trim().to_string());
                                    }
                                    "dc:description" | "description" => {
                                        metadata.description = Some(text_content.trim().to_string());
                                    }
                                    "dc:publisher" | "publisher" => {
                                        metadata.publisher = Some(text_content.trim().to_string());
                                    }
                                    "dc:date" | "date" => {
                                        metadata.date = Some(text_content.trim().to_string());
                                    }
                                    "dc:subject" | "subject" => {
                                        metadata.subjects.push(text_content.trim().to_string());
                                    }
                                    "dc:rights" | "rights" => {
                                        metadata.rights = Some(text_content.trim().to_string());
                                    }
                                    _ => {}
                                }
                            }
                            current_element = None;
                            text_content.clear();
                        }
                        _ => {}
                    }
                }

                Ok(Event::Empty(ref e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    match name.as_str() {
                        "item" => {
                            let item = parse_manifest_item(e)?;

                            // Check for NAV document (EPUB3)
                            if item.properties.contains(&"nav".to_string()) {
                                nav_path = Some(item.href.clone());
                            }

                            // Check for NCX (EPUB2)
                            if item.media_type == "application/x-dtbncx+xml" {
                                ncx_path = Some(item.href.clone());
                            }

                            manifest.insert(item.id.clone(), item);
                        }
                        "itemref" => {
                            let item = parse_spine_item(e)?;
                            spine.push(item);
                        }
                        "meta" if in_metadata => {
                            // Handle cover meta
                            let mut name_attr = None;
                            let mut content_attr = None;

                            for attr in e.attributes() {
                                let attr = attr?;
                                match attr.key.as_ref() {
                                    b"name" => {
                                        name_attr =
                                            Some(String::from_utf8_lossy(&attr.value).to_string());
                                    }
                                    b"content" => {
                                        content_attr =
                                            Some(String::from_utf8_lossy(&attr.value).to_string());
                                    }
                                    _ => {}
                                }
                            }

                            if name_attr.as_deref() == Some("cover") {
                                metadata.cover_id = content_attr;
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

                Ok(Event::Eof) => break,
                Err(e) => return Err(e.into()),
                _ => {}
            }
            buf.clear();
        }

        Ok(Package {
            metadata,
            manifest,
            spine,
            nav_path,
            ncx_path,
        })
    }
}

fn parse_manifest_item(e: &quick_xml::events::BytesStart) -> Result<ManifestItem> {
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

fn parse_spine_item(e: &quick_xml::events::BytesStart) -> Result<SpineItem> {
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
}
