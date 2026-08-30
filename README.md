# epub-wasm

A Rust EPUB parsing and rendering library that compiles to WebAssembly — an
[epub.js](https://github.com/futurepress/epub.js) replacement where all the
heavy lifting (ZIP, OPF, TOC, path resolution, resource rewriting, search)
happens in Rust.

```
epub-wasm/
├── core/           # Pure Rust EPUB parsing (no WASM deps, usable server-side)
├── renderer/       # WASM bindings: JsBook, Rendition, JsCfi
├── server-test/    # Axum test server (serves the client UI + a book from disk)
└── client-test/    # Static SPA demo (upload or ?load= an EPUB in the browser)
```

## Quickstart (browser, ~15 lines)

```html
<div id="viewer" style="height: 100vh"></div>
<script type="module">
  import init, { JsBook } from './pkg/epub_reader_renderer.js';
  await init();

  const bytes = new Uint8Array(await (await fetch('book.epub')).arrayBuffer());
  const book = new JsBook(bytes);

  console.log(book.metadata.title, book.toc);

  const iframe = document.createElement('iframe');
  iframe.style.cssText = 'width:100%;height:100%;border:none';
  document.getElementById('viewer').appendChild(iframe);

  // Display-ready HTML: images/CSS/fonts become blob URLs, internal links
  // carry data-epub-section/-fragment attributes, scripts are stripped.
  iframe.srcdoc = book.render_section(0, { styles: 'body { max-width: 40em; margin: auto }' });
</script>
```

Or let `Rendition` manage the iframe, section navigation and internal links
for you:

```js
import init, { Rendition } from './pkg/epub_reader_renderer.js';
await init();

const r = new Rendition(bytes, document.getElementById('viewer'));
r.on_relocated(({ index, href }) => console.log('now at', index, href));
r.display();
r.next(); r.prev(); r.display_href(r.toc[0].href);
```

(Working example: `client-test/rendition.html`.)

## Building

```bash
# Prerequisites: Rust + wasm-pack (cargo install wasm-pack)

make          # = make wasm build test
make wasm     # WASM package + sync client-test/pkg (required first; pkg/ is not checked in)
make build    # native release build (test server)
make test     # cargo test --workspace
make e2e      # Playwright click-path tests (uses your installed Chrome)
```

CI (GitHub Actions) runs the same targets plus the browser tests on every
push and PR.

## Running the demos

**Test server** (loads an EPUB from disk, serves the reader UI for it):

```bash
./target/release/epub-server-test /path/to/book.epub   # -p PORT (default 3000)
```

It embeds `client-test/*.html` and `renderer/pkg` at compile time, so build
the WASM before building the server.

**Client-side SPA** (all parsing happens in the browser):

```bash
cd client-test && ./serve.sh    # then open http://localhost:8080
```

Upload an EPUB, or deep-link one: `http://localhost:8080/?load=<epub-url>&section=3`
(the URL must be same-origin or CORS-enabled). The `client-test/` directory is
fully static — host it anywhere.

## API

### `JsBook`

| Member | Description |
|---|---|
| `new JsBook(bytes)` | Parse an EPUB from a `Uint8Array` |
| `metadata` | `{ title, creators, language, identifier, description, publisher, date, subjects, rights, cover_id }` |
| `toc` | Nested `{ id, href, label, children }`. Hrefs are **normalized archive paths** (+ `#fragment`) — no path math needed |
| `section_count` / `sections` | Spine length / all section metadata in one call |
| `direction` | `"rtl"` / `"ltr"` from the spine, if declared |
| `layout` / `section_viewport(i)` | `"pre-paginated"` for fixed-layout books / a section's `{width, height}` design size from its viewport meta |
| `get_section(i)` | `{ index, id, href, media_type, linear, properties }` |
| `get_section_content(i)` | Raw XHTML exactly as stored |
| `get_section_text(i)` | Plain text, whitespace-collapsed |
| `render_section(i, opts?)` | **Display-ready HTML** — resources → blob URLs (CSS files have their inner `url()`s rewritten too), internal links → `data-epub-section` / `data-epub-fragment`, external links → `target="_blank"`, scripts stripped. `opts`: `{ styles?, baseStyles?, stripScripts?, resolveLinks?, highlights? }`; `highlights: [{start, end}]` (plain-text byte ranges, e.g. from search results) wraps matches in `<mark class="epub-highlight" data-epub-offset="start">`, correctly split across element boundaries |
| `resolve_href(fromSection, href)` | `{ index, fragment }` or `null` — resolves any link/TOC href |
| `section_index_for_href(href)` | Archive path, OPF-relative path or bare filename → spine index |
| `get_resource(href)` / `get_resource_url(href)` | Resource bytes / cached blob URL |
| `media_type(href)` | MIME type (manifest first, extension fallback) |
| `get_cover()` / `get_cover_url()` | Cover image bytes / blob URL |
| `generate_locations(n)` | Build a stable position index (one location per `n` chars of text); returns the count. Enables `locations` and `percentage_at` |
| `locations` / `percentage_at(i, fraction)` | The index as `[{ cfi, section_index, offset, percentage }]` / progress (0–100) at a fraction through section `i` |
| `search(query, opts?)` | `[{ section_index, matched_text, excerpt, offset, len, cfi }]` — `cfi` is a **range CFI string** (e.g. `epubcfi(/6/4!/4/2,/1:5,/1:11)`) targeting the matched text in the raw document; `{start: offset, end: offset + len}` feeds straight into `render_section` highlights. `opts`: `{ caseInsensitive?, maxResults?, contextChars? }` (plain object, reusable) |
| `revoke_resources()` | Release all blob URLs |

### `Rendition`

Reader controller: one iframe, scrolled **or paginated** (CSS multi-column)
flow, internal-link and TOC navigation. `new Rendition(bytes, containerElement)`,
`display()`, `display_section(i)`, `display_href(href)`, `next()`, `prev()`
(page-aware in paginated flow), `set_flow("paginated"|"scrolled")`, `flow`,
`current_page()`, `page_count()`, `current_section_index()`, `metadata`, `toc`,
`search()`, `generate_locations(n)`, `set_styles(css)`, `on_relocated(cb)`
(`{ index, href, page, page_count, percentage }` — `percentage` is non-null
once locations are generated), `on_error(cb)` (errors from event handlers;
`console.error` otherwise), `destroy()`. Pagination re-measures on window
resize and when embedded fonts finish loading.

RTL: `next()`/`prev()` always mean reading order; content computing to
`direction: rtl` flips the internal paging math automatically, and the demos
flip arrow keys per the spine's `page-progression-direction`. Fixed-layout
(`rendition:layout` pre-paginated, incl. per-itemref overrides): sections are
scaled to fit the viewport from their `<meta name="viewport">` design size —
column CSS is never applied to FXL content.

### `JsCfi`

`new JsCfi(str)`, `JsCfi.from_spine_index(i)`, `.spine_index`,
`.character_offset`, `.toString()`, `.compare(other)`.

### Rust (`epub-reader-core`)

```rust
use epub_reader_core::{Book, SearchOptions};

let mut book = Book::from_path("book.epub")?;
println!("{} — {} sections", book.metadata.title, book.section_count());
let html = book.section_content(0)?;
let text = book.section_text(0)?;
let hits = book.search("rabbit", &SearchOptions::new())?;
```

Handles the messy real world: percent-encoded hrefs, `../` paths,
namespace-prefixed OPF elements, non-self-closing tags, `<span>` TOC headings,
NAV→NCX fallback, `<spine toc=…>`, EPUB2 `<meta name="cover">` and EPUB3
`cover-image`, Unicode-safe search (no panics on multibyte text).

CFI support (`Cfi`, `CfiRange`, `TextMap`): search matches carry range CFIs
with element paths and character offsets (code points, per spec), generated
by a single scan that also maps text offsets back to source byte spans.
CFIs address the **raw stored document** — the rendered DOM may differ once
scripts are stripped or highlights injected, so persistent references target
the file, not the live view.

## Roadmap

Not implemented yet, in rough priority order:

- **Annotations** — persistent user highlights + notes (the `<mark>`
  injection mechanism from search highlighting is the building block)
- **Font deobfuscation** (IDPF/Adobe schemes)
- **Streaming ZIP** — the archive is currently fully decompressed into memory
- **npm packaging** — publish `renderer/pkg` with hand-checked `.d.ts`

## License

MIT
