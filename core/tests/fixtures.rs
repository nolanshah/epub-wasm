//! Integration tests against small EPUBs built in memory.

use std::io::{Cursor, Write};

use epub_reader_core::{Book, CfiRange, SearchOptions};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

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

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

/// EPUB3 with every awkward-but-valid thing we have hit in the wild:
/// `opf:`-prefixed elements, non-self-closing `<item>`, a percent-encoded
/// href with a space, `../` paths, a `<span>` heading in the nav, and
/// curly quotes in the text.
fn tricky_epub3() -> Vec<u8> {
    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<opf:package xmlns:opf="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0" unique-identifier="uid">
  <opf:metadata>
    <dc:identifier id="uid">urn:uuid:1234</dc:identifier>
    <dc:title>Tricky Book</dc:title>
    <dc:creator>Someone</dc:creator>
    <dc:language>en</dc:language>
    <opf:meta property="dcterms:modified">2024-01-01T00:00:00Z</opf:meta>
  </opf:metadata>
  <opf:manifest>
    <opf:item id="nav" href="Nav/nav.xhtml" media-type="application/xhtml+xml" properties="nav"></opf:item>
    <opf:item id="c1" href="Text/My%20Chapter.xhtml" media-type="application/xhtml+xml"></opf:item>
    <opf:item id="c2" href="Text/ch2.xhtml" media-type="application/xhtml+xml"/>
    <opf:item id="pic" href="Images/pic.png" media-type="image/png" properties="cover-image"/>
    <opf:item id="css" href="Styles/main.css" media-type="text/css"/>
  </opf:manifest>
  <opf:spine>
    <opf:itemref idref="c1"/>
    <opf:itemref idref="c2" linear="no"/>
    <opf:itemref idref="does-not-exist"/>
  </opf:spine>
</opf:package>"#;

    let nav = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="landmarks"><ol><li><a href="../Text/ch2.xhtml">Start</a></li></ol></nav>
<nav epub:type="toc"><ol>
  <li><span>Part One</span>
    <ol>
      <li><a href="../Text/My%20Chapter.xhtml#top">Chapter 1</a></li>
      <li><a href="../Text/ch2.xhtml">Chapter 2</a></li>
    </ol>
  </li>
</ol></nav>
</body></html>"#;

    let ch1 = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="../Styles/main.css"/></head>
<body id="top">
<h1>Chapter 1</h1>
<p>Alice’s Adventures… the white rabbit was late.</p>
<img src="../Images/pic.png" alt="pic"/>
<p><a href="ch2.xhtml#sec">next</a> <a href="https://example.com">out</a></p>
</body></html>"#;

    let ch2 = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1 id="sec">Chapter 2</h1><p>The Rabbit again.</p></body></html>"#;

    build_epub(&[
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/Nav/nav.xhtml", nav.as_bytes()),
        ("OEBPS/Text/My Chapter.xhtml", ch1.as_bytes()),
        ("OEBPS/Text/ch2.xhtml", ch2.as_bytes()),
        ("OEBPS/Images/pic.png", b"\x89PNG\r\n\x1a\nfake"),
        ("OEBPS/Styles/main.css", b"body { color: red }"),
    ])
}

#[test]
fn tricky_epub3_parses_completely() {
    let book = Book::from_bytes(tricky_epub3()).unwrap();

    assert_eq!(book.metadata.title, "Tricky Book");
    assert_eq!(book.metadata.creators, vec!["Someone"]);
    assert_eq!(book.metadata.identifier.as_deref(), Some("urn:uuid:1234"));

    // Spine item with a missing manifest entry is dropped; the rest keep order.
    assert_eq!(book.section_count(), 2);
    let s0 = book.section(0).unwrap();
    assert_eq!(s0.href, "OEBPS/Text/My Chapter.xhtml");
    assert!(s0.linear);
    assert!(!book.section(1).unwrap().linear);

    // TOC comes from the epub:type="toc" nav, not landmarks; span heading kept.
    assert_eq!(book.toc.len(), 1);
    assert_eq!(book.toc[0].label, "Part One");
    assert_eq!(book.toc[0].href, "");
    assert_eq!(book.toc[0].children.len(), 2);
    assert_eq!(
        book.toc[0].children[0].href,
        "OEBPS/Text/My Chapter.xhtml#top"
    );
    assert_eq!(book.toc[0].children[1].href, "OEBPS/Text/ch2.xhtml");

    assert_eq!(book.cover_path().as_deref(), Some("OEBPS/Images/pic.png"));
    assert!(book.cover_image().unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn hrefs_resolve_from_any_reasonable_form() {
    let book = Book::from_bytes(tricky_epub3()).unwrap();

    // TOC href (archive path + fragment)
    assert_eq!(
        book.section_index_by_href("OEBPS/Text/My Chapter.xhtml#top"),
        Some(0)
    );
    // Still percent-encoded, relative to OPF dir
    assert_eq!(book.section_index_by_href("Text/My%20Chapter.xhtml"), Some(0));
    // Bare filename fallback
    assert_eq!(book.section_index_by_href("ch2.xhtml"), Some(1));
    assert_eq!(book.section_index_by_href("nope.xhtml"), None);

    // Links inside a section
    assert_eq!(
        book.resolve_href(0, "ch2.xhtml#sec"),
        Some((1, Some("sec".to_string())))
    );
    assert_eq!(book.resolve_href(0, "#top"), Some((0, Some("top".to_string()))));
    assert_eq!(book.resolve_href(0, "https://example.com"), None);
    assert_eq!(book.resolve_href(0, "../Images/pic.png"), None);

    let toc_item = book.find_toc_item("Text/ch2.xhtml").unwrap();
    assert_eq!(toc_item.label, "Chapter 2");
}

#[test]
fn resources_resolve_relative_to_section() {
    let book = Book::from_bytes(tricky_epub3()).unwrap();

    let (p, data) = book.resource_from_section(0, "../Images/pic.png").unwrap();
    assert_eq!(p, "OEBPS/Images/pic.png");
    assert!(data.starts_with(b"\x89PNG"));
    assert_eq!(book.media_type_for(&p), "image/png");

    let (p, data) = book.resource_from_section(0, "../Styles/main.css").unwrap();
    assert_eq!(p, "OEBPS/Styles/main.css");
    assert_eq!(data, b"body { color: red }");

    // get_resource accepts OPF-relative or archive-absolute
    assert!(book.get_resource("Images/pic.png").is_some());
    assert!(book.get_resource("OEBPS/Images/pic.png").is_some());
    assert!(book.get_resource("/OEBPS/Images/pic.png").is_some());
    assert!(book.get_resource("Text/My%20Chapter.xhtml").is_some());
    assert!(book.get_resource("missing.png").is_none());

    // Unknown extension, not in manifest -> octet-stream
    assert_eq!(book.media_type_for("x/y.zzz"), "application/octet-stream");
}

#[test]
fn search_across_sections_with_unicode_text() {
    let mut book = Book::from_bytes(tricky_epub3()).unwrap();

    let matches = book.search("rabbit", &SearchOptions::new()).unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].section_index, 0);
    assert_eq!(matches[0].matched_text, "rabbit");
    assert!(matches[0].excerpt.contains("Alice’s"));
    assert_eq!(matches[1].section_index, 1);
    assert_eq!(matches[1].matched_text, "Rabbit");

    // Matches carry parseable range CFIs targeting the matched text
    for m in &matches {
        let range = CfiRange::parse(&m.cfi).unwrap();
        assert_eq!(range.start.spine_index, m.section_index);
        assert!(range.start.character_offset.is_some());
        assert_eq!(range.to_string(), m.cfi);
    }
    assert!(matches[0].cfi.starts_with("epubcfi(/6/2!"));

    let limited = book
        .search(
            "rabbit",
            &SearchOptions {
                max_results: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(limited.len(), 1);

    assert_eq!(
        book.section_text(1).unwrap(),
        "Chapter 2 The Rabbit again."
    );
}

#[test]
fn locations_index_is_monotonic_with_valid_cfis() {
    use epub_reader_core::Cfi;

    let mut book = Book::from_bytes(tricky_epub3()).unwrap();
    let locations = book.generate_locations(20).unwrap();

    assert!(locations.total() >= 3, "expected several positions");

    let mut last_pct = -1.0;
    let mut last_section = 0;
    for l in &locations.locations {
        assert!(l.percentage >= last_pct, "percentages must be monotonic");
        assert!(l.section_index >= last_section);
        last_pct = l.percentage;
        last_section = l.section_index;

        let cfi = Cfi::parse(&l.cfi).unwrap();
        assert_eq!(cfi.spine_index, l.section_index);
    }
    assert!(last_pct < 100.0);

    // Interpolation: start of book = 0, end of last section = 100
    assert_eq!(locations.percentage_at(0, 0.0), 0.0);
    assert_eq!(locations.percentage_at(1, 1.0), 100.0);
    let mid = locations.percentage_at(1, 0.0);
    assert!(mid > 0.0 && mid < 100.0);
}

fn epub2_ncx_only(nav_href: Option<&str>) -> Vec<u8> {
    let nav_item = nav_href
        .map(|h| format!(r#"<item id="nav" href="{}" media-type="application/xhtml+xml" properties="nav"/>"#, h))
        .unwrap_or_default();

    let opf = format!(
        r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Old Book</dc:title>
    <meta name="cover" content="cov"/>
  </metadata>
  <manifest>
    {nav_item}
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="cov" href="cover.jpg" media-type="image/jpeg"/>
    <item id="a" href="a.html" media-type="application/xhtml+xml"/>
    <item id="b" href="b.html" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="a"/>
    <itemref idref="b"/>
  </spine>
</package>"#
    );

    let ncx = r#"<?xml version="1.0"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap>
  <navPoint id="n1"><navLabel><text>A</text></navLabel><content src="a.html"/>
    <navPoint id="n2"><navLabel><text>B</text></navLabel><content src="b.html#x"/></navPoint>
  </navPoint>
</navMap></ncx>"#;

    build_epub(&[
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/toc.ncx", ncx.as_bytes()),
        ("OEBPS/cover.jpg", b"\xff\xd8jpeg"),
        ("OEBPS/a.html", b"<html><body><p>A</p></body></html>"),
        ("OEBPS/b.html", b"<html><body><p id=\"x\">B</p></body></html>"),
    ])
}

#[test]
fn epub2_uses_ncx_and_meta_cover() {
    let book = Book::from_bytes(epub2_ncx_only(None)).unwrap();

    assert_eq!(book.metadata.title, "Old Book");
    assert_eq!(book.section_count(), 2);
    assert_eq!(book.toc.len(), 1);
    assert_eq!(book.toc[0].label, "A");
    assert_eq!(book.toc[0].href, "OEBPS/a.html");
    assert_eq!(book.toc[0].children[0].label, "B");
    assert_eq!(book.toc[0].children[0].href, "OEBPS/b.html#x");
    assert_eq!(book.cover_path().as_deref(), Some("OEBPS/cover.jpg"));
}

#[test]
fn missing_nav_document_falls_back_to_ncx() {
    // The manifest promises a nav document that isn't in the archive.
    // Previously this aborted loading the whole book.
    let book = Book::from_bytes(epub2_ncx_only(Some("missing-nav.xhtml"))).unwrap();
    assert_eq!(book.toc.len(), 1);
    assert_eq!(book.toc[0].label, "A");
}

#[test]
fn garbage_input_is_an_error_not_a_panic() {
    assert!(Book::from_bytes(b"not a zip".to_vec()).is_err());
    assert!(Book::from_bytes(Vec::new()).is_err());

    let no_container = build_epub(&[("OEBPS/content.opf", b"<package/>")]);
    assert!(Book::from_bytes(no_container).is_err());

    let empty_spine = build_epub(&[
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        (
            "OEBPS/content.opf",
            br#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest/><spine/></package>"#,
        ),
    ]);
    assert!(Book::from_bytes(empty_spine).is_err());
}
