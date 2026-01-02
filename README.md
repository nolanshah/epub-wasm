# epub-reader

A Rust-based EPUB reader library with WASM support, designed as a replacement for epub.js.

## Architecture

```
epub-reader/
├── core/           # Pure Rust EPUB parsing library
├── renderer/       # WASM rendering layer
├── test-app/       # CLI test server
└── client/         # Client-side only SPA
```

### Core Library (`epub-reader-core`)

Pure Rust EPUB parsing with no WASM dependencies. Can be used server-side or compiled to WASM.

**Features:**
- ZIP archive handling
- Container.xml parsing
- OPF parsing (metadata, manifest, spine)
- NAV (EPUB3) and NCX (EPUB2) TOC parsing
- CFI (Canonical Fragment Identifier) parsing and generation
- Full-text search with CFI results
- Section content loading

**Usage (Rust):**
```rust
use epub_reader_core::Book;

let book = Book::from_path("book.epub")?;
println!("Title: {}", book.metadata.title);
println!("Sections: {}", book.section_count());

// Load section content
let content = book.section_content(0)?;
```

### Renderer (`epub-reader-renderer`)

WASM-compatible browser rendering layer.

**Features:**
- iframe-based content isolation
- CSS column pagination
- Blob URL resource management
- Progress/location tracking
- JavaScript bindings for CDN usage

**Usage (JavaScript):**
```javascript
import init, { JsBook } from './pkg/epub_reader_renderer.js';

await init();
const response = await fetch('book.epub');
const data = new Uint8Array(await response.arrayBuffer());

const book = new JsBook(data);
console.log(book.metadata);       // { title, creators, ... }
console.log(book.toc);            // Table of contents
console.log(book.section_count);  // Number of sections

const content = book.get_section_content(0);  // HTML content
```

## Running

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install wasm-pack (for building WASM)
cargo install wasm-pack
```

### Build Everything

```bash
cd ebm/epub-reader

# Build core library
cargo build --release --package epub-reader-core

# Build WASM renderer
wasm-pack build --target web renderer

# Build test server
cargo build --release --package epub-reader-test

# Copy WASM to client directory
cp -r renderer/pkg client/
```

### Run Tests

```bash
cargo test --package epub-reader-core
```

### Option 1: CLI Test Server

A Rust-based server that loads an EPUB from disk and serves it with a web UI.

```bash
./target/release/epub-test /path/to/book.epub
```

Options:
- `-p, --port <PORT>` - Port to run on (default: 3000)

Then open http://localhost:3000

### Option 2: Client-Side SPA

A static single-page app where you upload an EPUB in the browser. No server needed for EPUB processing.

```bash
cd client
./serve.sh
# or: python3 -m http.server 8080
```

Then open http://localhost:8080

**Static Hosting:** Upload the `client/` directory to any static host (GitHub Pages, Netlify, S3, etc.):
```
client/
├── index.html
└── pkg/
    ├── epub_reader_renderer.js
    ├── epub_reader_renderer.d.ts
    └── epub_reader_renderer_bg.wasm
```

## Viewing Modes

Both the test server and client SPA support two viewing modes:

- **📖 Chapters** - Navigate section-by-section with Prev/Next buttons or arrow keys
- **📜 Single Page** - All content loaded in one scrollable view with fragment-based navigation

## API Reference

### JsBook (WASM)

| Property/Method | Description |
|----------------|-------------|
| `new JsBook(data: Uint8Array)` | Create from EPUB bytes |
| `metadata` | Book metadata (title, creators, language, etc.) |
| `toc` | Table of contents as nested array |
| `section_count` | Number of spine sections |
| `get_section(index)` | Get section metadata |
| `get_section_content(index)` | Get section HTML content |
| `get_resource(href)` | Get embedded resource (images, CSS, fonts) |
| `get_cover()` | Get cover image data |
| `search(query, options?)` | Full-text search |

### JsCfi (WASM)

| Property/Method | Description |
|----------------|-------------|
| `new JsCfi(cfiString)` | Parse a CFI string |
| `from_spine_index(index)` | Create CFI for spine position |
| `spine_index` | Get spine index |
| `character_offset` | Get character offset |
| `toString()` | Serialize to CFI string |
| `compare(other)` | Compare two CFIs (-1, 0, 1) |

## Dependencies

### Core
- `zip` - EPUB archive handling (deflate only for WASM compat)
- `quick-xml` - XML parsing
- `scraper` - HTML parsing
- `thiserror` - Error handling
- `serde` - Serialization

### Renderer
- `wasm-bindgen` - Rust/JS interop
- `web-sys` - Web API bindings
- `js-sys` - JavaScript API bindings

## License

MIT
