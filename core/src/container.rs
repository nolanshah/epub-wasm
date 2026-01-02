//! Container XML parsing (META-INF/container.xml)

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{EpubError, Result};

const CONTAINER_PATH: &str = "META-INF/container.xml";

/// Parse the container.xml file to find the rootfile path
pub fn parse_container(xml: &str) -> Result<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) if e.name().as_ref() == b"rootfile" => {
                for attr in e.attributes() {
                    let attr = attr?;
                    if attr.key.as_ref() == b"full-path" {
                        let path = String::from_utf8(attr.value.to_vec()).map_err(|e| {
                            EpubError::InvalidStructure(format!("Invalid UTF-8 in rootfile path: {}", e))
                        })?;
                        return Ok(path);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(e.into()),
            _ => {}
        }
        buf.clear();
    }

    Err(EpubError::InvalidStructure(
        "No rootfile found in container.xml".to_string(),
    ))
}

/// Get the container.xml path constant
pub fn container_path() -> &'static str {
    CONTAINER_PATH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_container() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

        let path = parse_container(xml).unwrap();
        assert_eq!(path, "OEBPS/content.opf");
    }

    #[test]
    fn test_parse_container_alternate_path() {
        let xml = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="package.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

        let path = parse_container(xml).unwrap();
        assert_eq!(path, "package.opf");
    }
}
