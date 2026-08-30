//! Path and href utilities for resolving references inside an EPUB archive.
//!
//! EPUB hrefs are URLs (percent-encoded, may contain `../`, may carry a
//! `#fragment`), while ZIP entry names are plain, slash-separated paths.
//! Everything that maps one to the other goes through this module.

/// Returns `true` for hrefs that point outside the archive
/// (`http://…`, `mailto:`, `data:`, `blob:`, …).
pub fn is_external(href: &str) -> bool {
    let lower = href.trim_start().to_ascii_lowercase();
    lower.contains("://")
        || lower.starts_with("mailto:")
        || lower.starts_with("data:")
        || lower.starts_with("blob:")
        || lower.starts_with("javascript:")
        || lower.starts_with("tel:")
}

/// Split `path#fragment` into `(path, Some(fragment))`.
pub fn split_fragment(href: &str) -> (&str, Option<&str>) {
    match href.find('#') {
        Some(pos) => (&href[..pos], Some(&href[pos + 1..])),
        None => (href, None),
    }
}

/// Decode `%XX` escapes. Malformed escapes are left untouched; if the result
/// is not valid UTF-8 the input is returned unchanged.
pub fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }

    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
            out.push(u8::from_str_radix(hex, 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }

    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Normalize an archive path: strip a leading `/`, resolve `.` and `..`,
/// collapse repeated slashes.
pub fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

/// Directory part of an archive path, including the trailing slash
/// (`"OEBPS/Text/ch1.xhtml"` → `"OEBPS/Text/"`, `"ch1.xhtml"` → `""`).
pub fn dir_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(pos) => &path[..=pos],
        None => "",
    }
}

/// Resolve an href against a base directory into a normalized archive path.
///
/// Returns `(path, fragment)`. The href is percent-decoded and the fragment
/// is split off. A leading `/` makes the href archive-absolute.
pub fn resolve(base_dir: &str, href: &str) -> (String, Option<String>) {
    let (raw_path, fragment) = split_fragment(href);
    let decoded = percent_decode(raw_path);

    let path = if decoded.is_empty() {
        String::new()
    } else if decoded.starts_with('/') {
        normalize(&decoded)
    } else {
        normalize(&format!("{}{}", base_dir, decoded))
    };

    (path, fragment.map(|f| f.to_string()))
}

/// Best-effort MIME type from a file extension.
pub fn mime_type_from_extension(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "css" => "text/css",
        "js" => "application/javascript",
        "xhtml" | "xht" => "application/xhtml+xml",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "ncx" => "application/x-dtbncx+xml",
        "mp3" => "audio/mpeg",
        "mp4" | "m4v" => "video/mp4",
        "smil" => "application/smil+xml",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("My%20Chapter.xhtml"), "My Chapter.xhtml");
        assert_eq!(percent_decode("caf%C3%A9.xhtml"), "café.xhtml");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn normalizes_paths() {
        assert_eq!(normalize("a/b/c"), "a/b/c");
        assert_eq!(normalize("a/b/../c"), "a/c");
        assert_eq!(normalize("a/./b/c"), "a/b/c");
        assert_eq!(normalize("a/b/c/../../d"), "a/d");
        assert_eq!(normalize("/OEBPS//x.xhtml"), "OEBPS/x.xhtml");
        assert_eq!(normalize("../x"), "x");
    }

    #[test]
    fn resolves_relative_hrefs() {
        assert_eq!(
            resolve("OEBPS/Text/", "../Images/cover.jpg"),
            ("OEBPS/Images/cover.jpg".to_string(), None)
        );
        assert_eq!(
            resolve("OEBPS/Text/", "ch2.xhtml#sec1"),
            ("OEBPS/Text/ch2.xhtml".to_string(), Some("sec1".to_string()))
        );
        assert_eq!(
            resolve("OEBPS/", "Text/My%20Chapter.xhtml"),
            ("OEBPS/Text/My Chapter.xhtml".to_string(), None)
        );
        assert_eq!(
            resolve("OEBPS/", "/META-INF/x.xml"),
            ("META-INF/x.xml".to_string(), None)
        );
        assert_eq!(resolve("OEBPS/", "#top"), (String::new(), Some("top".to_string())));
    }

    #[test]
    fn detects_external() {
        assert!(is_external("https://example.com/x"));
        assert!(is_external("mailto:a@b.c"));
        assert!(is_external("data:image/png;base64,xx"));
        assert!(!is_external("Text/ch1.xhtml"));
        assert!(!is_external("#frag"));
    }

    #[test]
    fn dir_of_paths() {
        assert_eq!(dir_of("OEBPS/Text/ch1.xhtml"), "OEBPS/Text/");
        assert_eq!(dir_of("ch1.xhtml"), "");
    }
}
