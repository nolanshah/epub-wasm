//! TextMap - scans an XHTML document once and produces:
//!
//! - the **normalized text** used for search (entities decoded, whitespace
//!   collapsed, block boundaries becoming single spaces),
//! - a mapping from normalized byte offsets back to **source byte spans**
//!   (for splicing markup such as highlights into the raw document), and
//! - a mapping to **CFI positions**: the chunk (character-data run) path per
//!   the EPUB CFI child numbering, plus the character offset within that
//!   chunk counted over the *decoded but uncollapsed* character data in
//!   code points.
//!
//! CFI child numbering: element children get even indices 2,4,6…; the
//! character-data chunk following the k-th element child gets index 2k+1.
//! The first step of a document path addresses the root element's children
//! (head = 2, body = 4). Comments and processing instructions neither split
//! chunks nor count toward offsets.
//!
//! CFIs are computed against the *raw* stored document — the rendered DOM
//! may differ (scripts stripped, marks injected), so persistent references
//! must target the file, not the live view.

use crate::cfi::{Cfi, CfiRange, CfiStep};

/// One run of normalized output bytes with a shared origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Bytes copied verbatim from the source (UTF-8 identical).
    Run,
    /// A single collapsed space produced by a run of source whitespace.
    Ws,
    /// A single space synthesized at a block-element boundary (no chunk).
    Synthetic,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    kind: Kind,
    /// Range in the normalized text
    norm_start: u32,
    norm_len: u32,
    /// Range in the source document
    src_start: u32,
    src_end: u32,
    /// Chunk this came from (u32::MAX for Synthetic)
    chunk: u32,
    /// Character offset within the chunk (decoded, uncollapsed, code points)
    char_start: u32,
    /// Characters this entry covers in chunk space
    char_len: u32,
}

#[derive(Debug, Clone)]
struct Chunk {
    /// CFI steps addressing this chunk, ending with the odd text index
    path: Vec<usize>,
    /// Running character count (decoded, uncollapsed)
    chars: u32,
}

/// End of a normalized-offset conversion: does the offset mark the start of
/// a range (snap forward past synthetic spaces) or its exclusive end (snap
/// backward)?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Start,
    End,
}

#[derive(Debug, Clone)]
pub struct TextMap {
    text: String,
    entries: Vec<Entry>,
    chunks: Vec<Chunk>,
}

impl TextMap {
    /// Scan a document. Never fails; malformed markup degrades gracefully.
    pub fn parse(html: &str) -> TextMap {
        Scanner::new(html).run()
    }

    /// The normalized text (what search operates on).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Consume, returning just the normalized text.
    pub fn into_text(self) -> String {
        self.text
    }

    /// CFI point for a normalized byte offset: `(chunk steps, char offset)`.
    fn point(&self, norm_offset: usize, side: Side) -> Option<(&[usize], usize)> {
        if self.entries.is_empty() {
            return None;
        }
        let target = match side {
            Side::Start => norm_offset,
            // The entry containing the last byte before the boundary
            Side::End => norm_offset.checked_sub(1)?,
        };

        let mut i = match self
            .entries
            .binary_search_by(|e| (e.norm_start as usize).cmp(&target))
        {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        };
        if target >= self.entries[i].norm_start as usize + self.entries[i].norm_len as usize {
            return None; // past the end
        }

        // Snap past synthetic block-boundary spaces, which belong to no chunk
        while self.entries[i].kind == Kind::Synthetic {
            match side {
                Side::Start => {
                    i += 1;
                    if i >= self.entries.len() {
                        return None;
                    }
                }
                Side::End => {
                    i = i.checked_sub(1)?;
                }
            }
        }

        let e = &self.entries[i];
        let chunk = self.chunks.get(e.chunk as usize)?;

        let char_offset = match (e.kind, side) {
            (Kind::Run, Side::Start) => {
                let within = target.max(e.norm_start as usize) - e.norm_start as usize;
                e.char_start as usize + self.text[e.norm_start as usize..][..within].chars().count()
            }
            (Kind::Run, Side::End) => {
                let within = target - e.norm_start as usize;
                e.char_start as usize
                    + self.text[e.norm_start as usize..][..within].chars().count()
                    + 1
            }
            (Kind::Ws, Side::Start) => e.char_start as usize,
            (Kind::Ws, Side::End) => (e.char_start + e.char_len) as usize,
            (Kind::Synthetic, _) => unreachable!(),
        };

        Some((&chunk.path, char_offset))
    }

    /// Point CFI for a normalized byte offset within spine item `spine_index`.
    pub fn cfi_point(&self, spine_index: usize, offset: usize) -> Option<Cfi> {
        let (path, char_offset) = self.point(offset, Side::Start)?;
        Some(Cfi {
            spine_index,
            path: path.iter().map(|&i| CfiStep { index: i, id: None }).collect(),
            character_offset: Some(char_offset),
            temporal_offset: None,
            spatial_offset: None,
        })
    }

    /// Range CFI for a normalized byte range within spine item `spine_index`.
    pub fn cfi_range(&self, spine_index: usize, start: usize, end: usize) -> Option<CfiRange> {
        if start >= end || end > self.text.len() {
            return None;
        }
        let (start_path, start_off) = self.point(start, Side::Start)?;
        let (end_path, end_off) = self.point(end, Side::End)?;

        let mk = |path: &[usize], offset: usize| Cfi {
            spine_index,
            path: path.iter().map(|&i| CfiStep { index: i, id: None }).collect(),
            character_offset: Some(offset),
            temporal_offset: None,
            spatial_offset: None,
        };

        Some(CfiRange {
            start: mk(start_path, start_off),
            end: mk(end_path, end_off),
        })
    }

    /// Source byte span covering a normalized range (for locating it in the
    /// raw document). Ends never land inside an entity or a tag.
    pub fn source_range(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        let segments = self.source_segments(start, end);
        let first = segments.first()?;
        let last = segments.last()?;
        Some((first.0, last.1))
    }

    /// Source byte spans covering a normalized range, split wherever markup
    /// intervenes. Splicing tags (e.g. `<mark>`) around each span keeps the
    /// document well-formed even when the range crosses element boundaries.
    pub fn source_segments(&self, start: usize, end: usize) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = Vec::new();
        if start >= end {
            return out;
        }

        for e in &self.entries {
            let e_start = e.norm_start as usize;
            let e_end = e_start + e.norm_len as usize;
            if e_end <= start {
                continue;
            }
            if e_start >= end {
                break;
            }
            if e.kind == Kind::Synthetic {
                continue;
            }

            let (s, t) = match e.kind {
                Kind::Run => {
                    // Byte-for-byte correspondence within a run
                    let from = start.max(e_start) - e_start;
                    let to = end.min(e_end) - e_start;
                    (e.src_start as usize + from, e.src_start as usize + to)
                }
                // Whitespace runs and entities are atomic in source space
                _ => (e.src_start as usize, e.src_end as usize),
            };

            match out.last_mut() {
                Some(last) if last.1 == s => last.1 = t,
                _ => out.push((s, t)),
            }
        }

        out
    }
}

// ---------------------------------------------------------------------------
// Scanner
// ---------------------------------------------------------------------------

const BLOCK_ELEMENTS: &[&str] = &[
    "address", "article", "aside", "blockquote", "br", "caption", "dd", "div", "dl", "dt",
    "fieldset", "figcaption", "figure", "footer", "h1", "h2", "h3", "h4", "h5", "h6", "header",
    "hr", "li", "main", "nav", "ol", "p", "pre", "section", "table", "tbody", "td", "tfoot", "th",
    "thead", "tr", "ul",
];

const SKIP_ELEMENTS: &[&str] = &["head", "script", "style", "template", "title"];

struct StackEl {
    /// CFI step index of this element among its parent's children
    step: usize,
    /// Element children seen so far
    child_elems: usize,
    /// Currently open text chunk, if any
    open_chunk: Option<u32>,
}

struct PendingWs {
    chunk: u32,
    char_start: u32,
    char_len: u32,
    src_start: u32,
    src_end: u32,
    /// A tag boundary was crossed: the source span must not grow past it,
    /// or markup spliced around ranges would swallow the tag.
    frozen: bool,
}

struct Scanner<'a> {
    src: &'a str,
    text: String,
    entries: Vec<Entry>,
    chunks: Vec<Chunk>,
    /// stack[0] is a virtual document node; stack[1] the root element
    stack: Vec<StackEl>,
    skip_depth: usize,
    pending_ws: Option<PendingWs>,
    pending_block: bool,
}

impl<'a> Scanner<'a> {
    fn new(src: &'a str) -> Self {
        Scanner {
            src,
            text: String::with_capacity(src.len() / 2),
            entries: Vec::new(),
            chunks: Vec::new(),
            stack: vec![StackEl {
                step: 0,
                child_elems: 0,
                open_chunk: None,
            }],
            skip_depth: 0,
            pending_ws: None,
            pending_block: false,
        }
    }

    fn run(mut self) -> TextMap {
        let bytes = self.src.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            if bytes[i] == b'<' {
                let rest = &self.src[i..];
                if rest.starts_with("<!--") {
                    i += rest.find("-->").map(|p| p + 3).unwrap_or(rest.len());
                } else if rest.starts_with("<![CDATA[") {
                    let end = rest.find("]]>").unwrap_or(rest.len());
                    let inner_start = i + 9;
                    let inner_end = i + end;
                    self.character_data(inner_start, inner_end);
                    i += rest.find("]]>").map(|p| p + 3).unwrap_or(rest.len());
                } else if rest.starts_with("<?") {
                    i += rest.find("?>").map(|p| p + 2).unwrap_or(rest.len());
                } else if rest.starts_with("<!") {
                    i += rest.find('>').map(|p| p + 1).unwrap_or(rest.len());
                } else if rest.starts_with("</") {
                    let len = rest.find('>').map(|p| p + 1).unwrap_or(rest.len());
                    let name = tag_name(&rest[2..len.saturating_sub(1)]);
                    self.end_element(&name);
                    i += len;
                } else {
                    match scan_tag(rest) {
                        Some((name, self_closing, len)) => {
                            self.start_element(&name, self_closing);
                            i += len;
                        }
                        None => {
                            // Stray '<': treat as text
                            self.push_char('<', i, i + 1);
                            i += 1;
                        }
                    }
                }
            } else {
                let next_tag = self.src[i..].find('<').map(|p| i + p).unwrap_or(bytes.len());
                self.character_data(i, next_tag);
                i = next_tag;
            }
        }

        TextMap {
            text: self.text,
            entries: self.entries,
            chunks: self.chunks,
        }
    }

    fn in_skip(&self) -> bool {
        self.skip_depth > 0
    }

    fn start_element(&mut self, name: &str, self_closing: bool) {
        self.flush_ws_boundary();

        // Any tag closes the parent's open chunk
        if let Some(top) = self.stack.last_mut() {
            top.open_chunk = None;
            top.child_elems += 1;
        }

        if BLOCK_ELEMENTS.contains(&name) {
            self.pending_block = true;
        }

        if self_closing {
            return;
        }

        let step = self.stack.last().map(|t| t.child_elems * 2).unwrap_or(2);
        self.stack.push(StackEl {
            step,
            child_elems: 0,
            open_chunk: None,
        });

        if self.in_skip() || SKIP_ELEMENTS.contains(&name) {
            self.skip_depth += 1;
        }
    }

    fn end_element(&mut self, name: &str) {
        self.flush_ws_boundary();

        if self.stack.len() > 1 {
            self.stack.pop();
        }
        if let Some(top) = self.stack.last_mut() {
            top.open_chunk = None;
        }
        if self.skip_depth > 0 {
            self.skip_depth -= 1;
        } else if SKIP_ELEMENTS.contains(&name) {
            // unbalanced close of a skip element; nothing to do
        }
        if BLOCK_ELEMENTS.contains(&name) {
            self.pending_block = true;
        }
    }

    /// On a tag boundary, pending source whitespace stays pending (it may
    /// merge with a block boundary space), but its source span must not
    /// extend across the tag.
    fn flush_ws_boundary(&mut self) {
        if let Some(ws) = &mut self.pending_ws {
            ws.frozen = true;
        }
    }

    /// Process character data between src byte offsets [start, end).
    fn character_data(&mut self, start: usize, end: usize) {
        if self.in_skip() {
            return;
        }
        let mut i = start;
        let bytes = self.src.as_bytes();

        while i < end {
            if bytes[i] == b'&' {
                if let Some((ch, len)) = decode_entity(&self.src[i..end]) {
                    self.push_char(ch, i, i + len);
                    i += len;
                    continue;
                }
            }
            let ch = self.src[i..].chars().next().unwrap();
            let l = ch.len_utf8();
            self.push_char(ch, i, i + l);
            i += l;
        }
    }

    fn ensure_chunk(&mut self) -> u32 {
        // Guaranteed non-empty stack
        let top = self.stack.last_mut().unwrap();
        if let Some(c) = top.open_chunk {
            return c;
        }
        // Steps of every element strictly below the root element (stack[1]),
        // then the odd text index within the current element.
        let mut path: Vec<usize> = self.stack.iter().skip(2).map(|e| e.step).collect();
        let top = self.stack.last_mut().unwrap();
        path.push(top.child_elems * 2 + 1);

        let id = self.chunks.len() as u32;
        self.chunks.push(Chunk { path, chars: 0 });
        top.open_chunk = Some(id);
        id
    }

    fn push_char(&mut self, ch: char, src_start: usize, src_end: usize) {
        let chunk = self.ensure_chunk();
        let char_pos = self.chunks[chunk as usize].chars;
        self.chunks[chunk as usize].chars += 1;

        if ch.is_whitespace() {
            match &mut self.pending_ws {
                Some(ws) => {
                    if !ws.frozen {
                        ws.src_end = src_end as u32;
                    }
                    if ws.chunk == chunk {
                        ws.char_len += 1;
                    }
                }
                None => {
                    self.pending_ws = Some(PendingWs {
                        chunk,
                        char_start: char_pos,
                        char_len: 1,
                        src_start: src_start as u32,
                        src_end: src_end as u32,
                        frozen: false,
                    });
                }
            }
            return;
        }

        self.emit_pending_spaces();

        let norm_start = self.text.len();
        self.text.push(ch);
        let norm_len = ch.len_utf8();
        let verbatim = norm_len == src_end - src_start;

        // Extend the previous run when both sides are contiguous
        if verbatim {
            if let Some(last) = self.entries.last_mut() {
                if last.kind == Kind::Run
                    && last.chunk == chunk
                    && last.src_end as usize == src_start
                    && (last.norm_start + last.norm_len) as usize == norm_start
                    && last.char_start + last.char_len == char_pos
                {
                    last.norm_len += norm_len as u32;
                    last.src_end = src_end as u32;
                    last.char_len += 1;
                    return;
                }
            }
        }

        self.entries.push(Entry {
            // An entity decodes to different bytes than its source; treat a
            // non-verbatim char like an atomic entity span.
            kind: if verbatim { Kind::Run } else { Kind::Ws },
            norm_start: norm_start as u32,
            norm_len: norm_len as u32,
            src_start: src_start as u32,
            src_end: src_end as u32,
            chunk,
            char_start: char_pos,
            char_len: 1,
        });
    }

    /// Emit at most one space for pending source whitespace and/or a block
    /// boundary, but never at the very start of the text and never doubled.
    fn emit_pending_spaces(&mut self) {
        let ws = self.pending_ws.take();
        let block = std::mem::take(&mut self.pending_block);

        if self.text.is_empty() || (ws.is_none() && !block) {
            return;
        }
        if self.text.ends_with(' ') {
            return;
        }

        let norm_start = self.text.len() as u32;
        self.text.push(' ');

        match ws {
            Some(ws) => self.entries.push(Entry {
                kind: Kind::Ws,
                norm_start,
                norm_len: 1,
                src_start: ws.src_start,
                src_end: ws.src_end,
                chunk: ws.chunk,
                char_start: ws.char_start,
                char_len: ws.char_len,
            }),
            None => {
                // Block boundary only: no source characters back this space
                let pos = self
                    .entries
                    .last()
                    .map(|e| e.src_end)
                    .unwrap_or(0);
                self.entries.push(Entry {
                    kind: Kind::Synthetic,
                    norm_start,
                    norm_len: 1,
                    src_start: pos,
                    src_end: pos,
                    chunk: u32::MAX,
                    char_start: 0,
                    char_len: 0,
                });
            }
        }
    }
}

/// Scan a start tag at the beginning of `s` (starting with `<`), respecting
/// quoted attribute values. Returns `(lowercased name, self_closing, len)`.
fn scan_tag(s: &str) -> Option<(String, bool, usize)> {
    let b = s.as_bytes();
    let mut i = 1;
    let name_start = i;
    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = s[name_start..i].to_ascii_lowercase();
    // strip namespace prefix
    let name = name.rsplit(':').next().unwrap_or(&name).to_string();

    let mut self_closing = false;
    while i < b.len() {
        match b[i] {
            b'>' => return Some((name, self_closing, i + 1)),
            b'/' => {
                self_closing = true;
                i += 1;
            }
            b'"' | b'\'' => {
                let q = b[i];
                i += 1;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                if i >= b.len() {
                    return None;
                }
                i += 1;
                self_closing = false;
            }
            _ => {
                if b[i] != b'/' && !b[i].is_ascii_whitespace() {
                    self_closing = false;
                }
                i += 1;
            }
        }
    }
    None
}

fn tag_name(s: &str) -> String {
    let name = s
        .trim()
        .split(|c: char| c.is_ascii_whitespace())
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    name.rsplit(':').next().unwrap_or(&name).to_string()
}

/// Decode one entity at the start of `s` (which begins with `&`).
/// Returns `(char, source length)`.
fn decode_entity(s: &str) -> Option<(char, usize)> {
    let end = s[..s.len().min(12)].find(';')?;
    let body = &s[1..end];
    let ch = match body {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "hellip" => '\u{2026}',
        "rsquo" => '\u{2019}',
        "lsquo" => '\u{2018}',
        "rdquo" => '\u{201d}',
        "ldquo" => '\u{201c}',
        "shy" => '\u{ad}',
        _ => {
            let code = if let Some(hex) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X"))
            {
                u32::from_str_radix(hex, 16).ok()?
            } else if let Some(dec) = body.strip_prefix('#') {
                dec.parse().ok()?
            } else {
                return None;
            };
            char::from_u32(code)?
        }
    };
    Some((ch, end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "<html><head><title>t</title></head><body><p>Hello <b>world</b> again</p><p>x</p></body></html>";

    fn range_str(html: &str, needle: &str, spine: usize) -> String {
        let map = TextMap::parse(html);
        let start = map.text().find(needle).unwrap();
        map.cfi_range(spine, start, start + needle.len())
            .unwrap()
            .to_string()
    }

    #[test]
    fn normalized_text_uses_block_boundaries() {
        let map = TextMap::parse(DOC);
        assert_eq!(map.text(), "Hello world again x");

        // Inline tags do NOT introduce spaces
        let map = TextMap::parse("<p>He<b>ll</b>o</p>");
        assert_eq!(map.text(), "Hello");

        // Blocks do
        let map = TextMap::parse("<div>one</div><div>two</div>");
        assert_eq!(map.text(), "one two");

        // Head/script/style content excluded, whitespace collapsed
        let map = TextMap::parse(
            "<html><head><style>p{}</style></head><body><p>a\n\n   b</p><script>var x=1</script></body></html>",
        );
        assert_eq!(map.text(), "a b");
    }

    #[test]
    fn entities_decode_with_source_spans() {
        let map = TextMap::parse("<p>A &amp; B &#65; &#x42; &unknown; C</p>");
        assert_eq!(map.text(), "A & B A B &unknown; C");
    }

    #[test]
    fn cfi_numbering_matches_hand_computed_vectors() {
        // Display uses the epub.js-style range form: the parent part stops at
        // the common element, locals carry the text step + offset.
        // "world" is the text child (1) of <b> (element 2 of p1 (2) of body (4))
        assert_eq!(range_str(DOC, "world", 1), "epubcfi(/6/4!/4/2/2,/1:0,/1:5)");
        // "again" is in chunk 3 of p1 (after 1 element child), char 1 of " again"
        assert_eq!(range_str(DOC, "again", 1), "epubcfi(/6/4!/4/2,/3:1,/3:6)");
        // "x" in the second <p> (4) of body
        assert_eq!(range_str(DOC, "x", 0), "epubcfi(/6/2!/4/4,/1:0,/1:1)");
    }

    #[test]
    fn entity_and_uncollapsed_offsets_count_chunk_chars() {
        // Without a <head>, body is the FIRST element child of html: step 2.
        // Chunk data is "A & B" decoded: B at char 4.
        let s = range_str("<html><body><p>A &amp; B</p></body></html>", "B", 0);
        assert_eq!(s, "epubcfi(/6/2!/2/2,/1:4,/1:5)");

        // Source has three spaces; normalized has one, but chunk chars count
        // all of them: "bar" starts at char 6 of "foo   bar".
        // Root-level <p>: the path addresses the root element's children.
        let s = range_str("<p>foo   bar</p>", "bar", 0);
        assert_eq!(s, "epubcfi(/6/2!,/1:6,/1:9)");
    }

    #[test]
    fn offsets_count_code_points_not_utf16_units() {
        // 𝄞 is one code point (two UTF-16 units); x is at char 1
        let s = range_str("<p>\u{1d11e}x</p>", "x", 0);
        assert_eq!(s, "epubcfi(/6/2!,/1:1,/1:2)");
    }

    #[test]
    fn range_spanning_elements_gets_split_parent() {
        // "Hello world" spans chunk 1 and b's chunk
        let map = TextMap::parse(DOC);
        let start = map.text().find("Hello").unwrap();
        let end = map.text().find("world").unwrap() + "world".len();
        let r = map.cfi_range(1, start, end).unwrap();
        assert_eq!(r.to_string(), "epubcfi(/6/4!/4/2,/1:0,/2/1:5)");

        // Round-trips through the parser
        let parsed = CfiRange::parse(&r.to_string()).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn source_segments_split_at_markup() {
        let map = TextMap::parse(DOC);
        let start = map.text().find("Hello").unwrap();
        let end = map.text().find("world").unwrap() + "world".len();

        let segs = map.source_segments(start, end);
        assert_eq!(segs.len(), 2);
        assert_eq!(&DOC[segs[0].0..segs[0].1], "Hello ");
        assert_eq!(&DOC[segs[1].0..segs[1].1], "world");

        // Single-run range: one segment, exact span
        let s = map.text().find("again").unwrap();
        let segs = map.source_segments(s, s + 5);
        assert_eq!(segs.len(), 1);
        assert_eq!(&DOC[segs[0].0..segs[0].1], "again");
    }

    #[test]
    fn source_range_never_splits_an_entity() {
        let html = "<p>A &amp; B</p>";
        let map = TextMap::parse(html);
        // "& B" includes the entity: span must start at '&' of &amp;
        let start = map.text().find('&').unwrap();
        let (s, e) = map.source_range(start, map.text().len()).unwrap();
        assert_eq!(&html[s..e], "&amp; B");
    }

    #[test]
    fn comments_and_pis_do_not_split_chunks() {
        let s = range_str("<p>fo<!-- c -->o</p>", "foo", 0);
        // Still one chunk: offsets 0..3
        assert_eq!(s, "epubcfi(/6/2!,/1:0,/1:3)");
    }

    #[test]
    fn source_segments_never_contain_markup() {
        // Splicing tags around segments must keep the document well-formed,
        // so no segment may span a tag — including via whitespace runs that
        // sit against a tag boundary.
        for html in [
            "<p>a <b>b</b></p>",
            "<p>a</b> <b>b</p>",
            DOC,
            "<div>one</div> <div>two</div>",
        ] {
            let map = TextMap::parse(html);
            let segs = map.source_segments(0, map.text().len());
            for (s, e) in segs {
                assert!(
                    !html[s..e].contains('<') && !html[s..e].contains('>'),
                    "segment {:?} of {:?} contains markup",
                    &html[s..e],
                    html
                );
            }
        }
    }

    #[test]
    fn malformed_input_does_not_panic() {
        for s in [
            "",
            "<",
            "<p",
            "<p>unclosed",
            "</b>text",
            "&amp",
            "<![CDATA[x",
            "<p>a<b>b</p>",
            "&#xZZ; ok",
        ] {
            let _ = TextMap::parse(s);
        }
        assert_eq!(TextMap::parse("plain text, no markup").text(), "plain text, no markup");
    }
}
