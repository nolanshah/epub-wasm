//! Generates the EPUB fixtures used by the e2e test suite.
//!
//! Usage: cargo run -p epub-reader-core --example make_fixture -- <out-dir>
//!
//! - fixture.epub      — reflowable LTR book exercising the click paths:
//!                       nested TOC, internal/fragment links, image, CSS,
//!                       percent-encoded filename, long sections
//! - fixture-rtl.epub  — reflowable book with dir="rtl" content and
//!                       page-progression-direction="rtl" (pagination flip)
//! - fixture-fxl.epub  — pre-paginated (fixed layout) book with viewport
//!                       metas (scaling)

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

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

fn build_epub(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();
    for (name, data) in files {
        zip.start_file(*name, stored).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

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

fn main_fixture() -> Vec<u8> {
    let opf = br#"<?xml version="1.0" encoding="UTF-8"?>
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
</package>"#;

    let nav = br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
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
</body></html>"#;

    let ch1 = chapter(
        1,
        r#"<p><img src="../Images/dot.png" alt="a red dot" id="dot"/></p>
<p>Jump to <a href="ch%202.xhtml#end" id="link-to-ch2-end">the end of chapter two</a>.</p>
<p>Visit <a href="https://example.com" id="external-link">example.com</a>.</p>"#,
    );

    // Chapter 2 is long so that paginated flow has multiple pages and the
    // "#end" fragment lands on a late page. The link/search lines stay early.
    let ch2 = chapter(
        2,
        &(r#"<p>Some searchable words: xylophone quartz xylophone.</p>
<p>Back to <a href="ch1.xhtml" id="link-to-ch1">chapter one</a>.</p>"#
            .to_string()
            + &r#"<p>Chapter two filler. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.</p>"#.repeat(60)),
    );

    let ch3 = chapter(
        3,
        &r#"<p>Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.</p>"#.repeat(60),
    );

    build_epub(&[
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OEBPS/content.opf", opf),
        ("OEBPS/nav.xhtml", nav),
        ("OEBPS/Text/ch1.xhtml", ch1.as_bytes()),
        ("OEBPS/Text/ch 2.xhtml", ch2.as_bytes()),
        ("OEBPS/Text/ch3.xhtml", ch3.as_bytes()),
        ("OEBPS/Styles/main.css", b".opener { color: rgb(0, 90, 200); } h1 { font-family: sans-serif; }"),
        ("OEBPS/Images/dot.png", PNG),
    ])
}

fn rtl_fixture() -> Vec<u8> {
    let opf = br#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:uuid:e2e-fixture-rtl</dc:identifier>
    <dc:title>RTL Fixture</dc:title>
    <dc:language>ar</dc:language>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="r1" href="r1.xhtml" media-type="application/xhtml+xml"/>
    <item id="r2" href="r2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine page-progression-direction="rtl">
    <itemref idref="r1"/>
    <itemref idref="r2"/>
  </spine>
</package>"#;

    let nav = br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol>
  <li><a href="r1.xhtml">First</a></li>
  <li><a href="r2.xhtml">Second</a></li>
</ol></nav></body></html>"#;

    let r1 = br#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" dir="rtl">
<head><title>First</title></head>
<body>
<h1 id="top">First</h1>
<p>Short opening section. <a href="r2.xhtml#end" id="rtl-link">Jump to the end of the long section</a>.</p>
</body></html>"#;

    let filler = r#"<p>Right-to-left filler paragraph with enough words to overflow across many columns when paginated in a narrow viewport.</p>"#.repeat(60);
    let r2 = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" dir="rtl">
<head><title>Second</title></head>
<body>
<h1 id="top">Second</h1>
{filler}
<p id="end">The very end of the long section.</p>
</body></html>"#
    );

    build_epub(&[
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OEBPS/content.opf", opf),
        ("OEBPS/nav.xhtml", nav),
        ("OEBPS/r1.xhtml", r1),
        ("OEBPS/r2.xhtml", r2.as_bytes()),
    ])
}

fn fxl_fixture() -> Vec<u8> {
    let opf = br#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:uuid:e2e-fixture-fxl</dc:identifier>
    <dc:title>Fixed Layout Fixture</dc:title>
    <dc:language>en</dc:language>
    <meta property="rendition:layout">pre-paginated</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="p1" href="page1.xhtml" media-type="application/xhtml+xml"/>
    <item id="p2" href="page2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="p1"/>
    <itemref idref="p2"/>
  </spine>
</package>"#;

    let nav = br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol>
  <li><a href="page1.xhtml">Page 1</a></li>
  <li><a href="page2.xhtml">Page 2</a></li>
</ol></nav></body></html>"#;

    let page = |n: usize, color: &str| {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
<title>Page {n}</title>
<meta name="viewport" content="width=400, height=600"/>
<style>
body {{ margin: 0; }}
.page {{ position: absolute; top: 0; left: 0; width: 400px; height: 600px; background: {color}; }}
.label {{ position: absolute; top: 280px; width: 100%; text-align: center; font: 32px sans-serif; }}
</style>
</head>
<body><div class="page"><div class="label" id="label">PAGE {n}</div></div></body></html>"#
        )
    };

    build_epub(&[
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OEBPS/content.opf", opf),
        ("OEBPS/nav.xhtml", nav),
        ("OEBPS/page1.xhtml", page(1, "#dbeafe").as_bytes()),
        ("OEBPS/page2.xhtml", page(2, "#dcfce7").as_bytes()),
    ])
}

fn main() {
    let out_dir = std::env::args()
        .nth(1)
        .expect("usage: make_fixture <out-dir>");

    for (name, bytes) in [
        ("fixture.epub", main_fixture()),
        ("fixture-rtl.epub", rtl_fixture()),
        ("fixture-fxl.epub", fxl_fixture()),
    ] {
        let path = format!("{}/{}", out_dir.trim_end_matches('/'), name);
        std::fs::write(&path, &bytes).unwrap();
        println!("wrote {} ({} bytes)", path, bytes.len());
    }
}
