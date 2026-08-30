//! Generates the small EPUB used by the e2e test suite.
//!
//! Usage: cargo run -p epub-reader-core --example make_fixture -- <out.epub>
//!
//! The book is designed to exercise the click paths: multiple chapters,
//! a nested TOC, internal links (cross-section and fragment), an image,
//! a stylesheet, and a percent-encoded filename.

use std::io::{Cursor, Write as _};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

// A 1x1 red PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
    0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
    0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
    0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x9E, 0x7C, 0x39, 0x9B, 0x00, 0x00, 0x00,
    0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn chapter(n: usize, extra: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter {n}</title><link rel="stylesheet" href="../Styles/main.css"/></head>
<body>
<h1 id="top">Chapter {n}</h1>
<p class="opener">This is the opening paragraph of chapter {n}. The quick brown fox jumps over the lazy dog.</p>
{extra}
<p id="end">End of chapter {n}.</p>
</body></html>"#
    )
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: make_fixture <out.epub>");

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    let mut add = |name: &str, data: &[u8]| {
        zip.start_file(name, stored).unwrap();
        zip.write_all(data).unwrap();
    };

    add("mimetype", b"application/epub+zip");

    add(
        "META-INF/container.xml",
        br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
    );

    add(
        "OEBPS/content.opf",
        br#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:uuid:e2e-fixture-0001</dc:identifier>
    <dc:title>Click Path Fixture</dc:title>
    <dc:creator>epub-wasm tests</dc:creator>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">2026-08-30T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="c1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="Text/ch%202.xhtml" media-type="application/xhtml+xml"/>
    <item id="c3" href="Text/ch3.xhtml" media-type="application/xhtml+xml"/>
    <item id="pic" href="Images/dot.png" media-type="image/png" properties="cover-image"/>
    <item id="css" href="Styles/main.css" media-type="text/css"/>
  </manifest>
  <spine>
    <itemref idref="c1"/>
    <itemref idref="c2"/>
    <itemref idref="c3"/>
  </spine>
</package>"#,
    );

    add(
        "OEBPS/nav.xhtml",
        br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol>
  <li><a href="Text/ch1.xhtml">Chapter One</a></li>
  <li><span>Part Two</span>
    <ol>
      <li><a href="Text/ch%202.xhtml">Chapter Two</a></li>
      <li><a href="Text/ch%202.xhtml#end">Chapter Two, ending</a></li>
    </ol>
  </li>
  <li><a href="Text/ch3.xhtml">Chapter Three</a></li>
</ol></nav>
</body></html>"#,
    );

    add(
        "OEBPS/Text/ch1.xhtml",
        chapter(
            1,
            r#"<p><img src="../Images/dot.png" alt="a red dot" id="dot"/></p>
<p>Jump to <a href="ch%202.xhtml#end" id="link-to-ch2-end">the end of chapter two</a>.</p>
<p>Visit <a href="https://example.com" id="external-link">example.com</a>.</p>"#,
        )
        .as_bytes(),
    );

    add(
        "OEBPS/Text/ch 2.xhtml",
        chapter(
            2,
            r#"<p>Some searchable words: xylophone quartz xylophone.</p>
<p>Back to <a href="ch1.xhtml" id="link-to-ch1">chapter one</a>.</p>"#,
        )
        .as_bytes(),
    );

    add(
        "OEBPS/Text/ch3.xhtml",
        chapter(
            3,
            &(r#"<p>Filler to make this section long. </p>"#.repeat(1)
                + &r#"<p>Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.</p>"#.repeat(60)),
        )
        .as_bytes(),
    );

    add(
        "OEBPS/Styles/main.css",
        b".opener { color: rgb(0, 90, 200); } h1 { font-family: sans-serif; }",
    );

    add("OEBPS/Images/dot.png", PNG);

    let bytes = zip.finish().unwrap().into_inner();
    std::fs::write(&out, &bytes).unwrap();
    println!("wrote {} ({} bytes)", out, bytes.len());
}
