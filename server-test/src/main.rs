//! EPUB Reader Test Server
//!
//! A simple web server for testing the epub-reader library.
//! Usage: epub-test <path-to-epub>

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::header,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use clap::Parser;
use epub_reader_core::Book;
use tower_http::cors::CorsLayer;

#[derive(Parser)]
#[command(name = "epub-test")]
#[command(about = "Test server for epub-reader library")]
struct Args {
    /// Path to the EPUB file to serve
    epub_path: PathBuf,

    /// Port to run the server on
    #[arg(short, long, default_value = "3000")]
    port: u16,
}

struct AppState {
    epub_data: Vec<u8>,
    book: Book,
    wasm_js: &'static str,
    wasm_bin: &'static [u8],
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Read the EPUB file
    let epub_data = std::fs::read(&args.epub_path)?;

    // Validate it's a valid EPUB
    let book = Book::from_bytes(epub_data.clone())?;
    println!("Loaded: {}", book.metadata.title);
    println!("Author: {}", book.metadata.creators.join(", "));
    println!("Sections: {}", book.section_count());

    // Load WASM files (embedded at compile time)
    let wasm_js = include_str!("../../renderer/pkg/epub_reader_renderer.js");
    let wasm_bin = include_bytes!("../../renderer/pkg/epub_reader_renderer_bg.wasm");

    let state = Arc::new(AppState {
        epub_data,
        book,
        wasm_js,
        wasm_bin,
    });

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/book.epub", get(epub_handler))
        .route("/epub_reader_renderer.js", get(wasm_js_handler))
        .route("/epub_reader_renderer_bg.wasm", get(wasm_bin_handler))
        .route("/resource/{*path}", get(resource_handler))
        .route("/api/sections", get(sections_handler))
        .route("/api/section/{index}", get(section_content_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    println!("\n🚀 Server running at http://{}", addr);
    println!("   Open in your browser to view the EPUB\n");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index_handler(State(state): State<Arc<AppState>>) -> Html<String> {
    let metadata = &state.book.metadata;
    let title = &metadata.title;
    let authors = metadata.creators.join(", ");

    // Build TOC JSON
    let toc_json = serde_json::to_string(&state.book.toc).unwrap_or_else(|_| "[]".to_string());

    // Build sections info
    let sections: Vec<_> = state.book.sections().map(|s| {
        serde_json::json!({
            "index": s.index,
            "href": s.href,
            "id": s.id,
        })
    }).collect();
    let sections_json = serde_json::to_string(&sections).unwrap_or_else(|_| "[]".to_string());

    let html = format!(
        r##"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title} - EPUB Reader Test</title>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #1a1a2e;
            color: #eee;
            min-height: 100vh;
        }}
        .header {{
            background: #16213e;
            padding: 1rem 2rem;
            border-bottom: 1px solid #0f3460;
            display: flex;
            justify-content: space-between;
            align-items: center;
            flex-wrap: wrap;
            gap: 1rem;
        }}
        .header h1 {{
            font-size: 1.2rem;
            font-weight: 500;
        }}
        .controls {{
            display: flex;
            gap: 1rem;
            align-items: center;
        }}
        .controls button {{
            background: #0f3460;
            color: #eee;
            border: none;
            padding: 0.5rem 1rem;
            border-radius: 4px;
            cursor: pointer;
            font-size: 0.9rem;
        }}
        .controls button:hover {{
            background: #e94560;
        }}
        .controls button:disabled {{
            opacity: 0.5;
            cursor: not-allowed;
        }}
        .controls button.active {{
            background: #e94560;
        }}
        .mode-toggle {{
            display: flex;
            gap: 0.5rem;
            background: #0f3460;
            padding: 0.25rem;
            border-radius: 4px;
        }}
        .mode-toggle button {{
            background: transparent;
            padding: 0.4rem 0.8rem;
        }}
        .mode-toggle button.active {{
            background: #e94560;
        }}
        .main {{
            display: flex;
            height: calc(100vh - 70px);
        }}
        .sidebar {{
            width: 300px;
            background: #16213e;
            border-right: 1px solid #0f3460;
            overflow-y: auto;
            padding: 1rem;
            flex-shrink: 0;
        }}
        .sidebar h2 {{
            font-size: 0.85rem;
            text-transform: uppercase;
            color: #888;
            margin-bottom: 0.75rem;
            margin-top: 1rem;
        }}
        .sidebar h2:first-child {{
            margin-top: 0;
        }}
        .toc-item {{
            padding: 0.4rem 0.5rem;
            cursor: pointer;
            border-radius: 4px;
            margin-bottom: 0.2rem;
            font-size: 0.9rem;
        }}
        .toc-item:hover {{
            background: #0f3460;
        }}
        .toc-item.active {{
            background: #e94560;
        }}
        .toc-children {{
            margin-left: 1rem;
        }}
        .viewer {{
            flex: 1;
            display: flex;
            flex-direction: column;
            overflow: hidden;
        }}
        #epub-container {{
            flex: 1;
            background: #fff;
            overflow: hidden;
        }}
        #epub-container.single-page {{
            overflow-y: auto;
        }}
        #epub-container iframe {{
            width: 100%;
            height: 100%;
            border: none;
        }}
        .status {{
            padding: 0.5rem 1rem;
            background: #16213e;
            text-align: center;
            font-size: 0.85rem;
            color: #888;
        }}
        .loading {{
            display: flex;
            align-items: center;
            justify-content: center;
            height: 100%;
            color: #333;
            background: #fff;
        }}
        .metadata {{
            padding-bottom: 1rem;
            border-bottom: 1px solid #0f3460;
        }}
        .metadata h3 {{
            font-size: 1rem;
            margin-bottom: 0.25rem;
        }}
        .metadata p {{
            font-size: 0.85rem;
            color: #888;
        }}
    </style>
</head>
<body>
    <div class="header">
        <h1>{title}</h1>
        <div class="controls">
            <div class="mode-toggle">
                <button id="mode-chapter" class="active" title="Chapter by chapter">📖 Chapters</button>
                <button id="mode-single" title="Single scrollable page">📜 Single Page</button>
            </div>
            <button id="prev-btn" disabled>← Prev</button>
            <span id="progress">0%</span>
            <button id="next-btn" disabled>Next →</button>
        </div>
    </div>
    <div class="main">
        <div class="sidebar">
            <div class="metadata">
                <h3>{title}</h3>
                <p>{authors}</p>
            </div>
            <h2>Table of Contents</h2>
            <div id="toc"></div>
        </div>
        <div class="viewer">
            <div id="epub-container">
                <div class="loading">Loading...</div>
            </div>
            <div class="status" id="status-bar">
                Section <span id="section-num">0</span> of <span id="section-total">0</span>
            </div>
        </div>
    </div>

    <script>
        const TOC = {toc_json};
        const SECTIONS = {sections_json};

        let currentSection = 0;
        let viewMode = 'chapter'; // 'chapter' or 'single'

        // Render TOC
        function renderToc(items, depth = 0) {{
            if (!items || items.length === 0) return '';
            let html = depth > 0 ? '<div class="toc-children">' : '';
            for (const item of items) {{
                const href = item.href || '';
                html += `<div class="toc-item" data-href="${{href}}">${{item.label}}</div>`;
                if (item.children && item.children.length > 0) {{
                    html += renderToc(item.children, depth + 1);
                }}
            }}
            html += depth > 0 ? '</div>' : '';
            return html;
        }}

        // Find section index by href
        function findSectionByHref(href) {{
            if (!href) return null;

            const fragment = href.split('#')[1] || null;
            const basePath = href.split('#')[0];
            const filename = basePath.split('/').pop();

            // Try multiple matching strategies
            for (let i = 0; i < SECTIONS.length; i++) {{
                const sectionHref = SECTIONS[i].href;
                const sectionFilename = sectionHref.split('/').pop();

                // Exact match
                if (sectionHref === basePath) {{
                    return {{ index: i, fragment }};
                }}

                // Section href ends with the target path
                if (sectionHref.endsWith(basePath)) {{
                    return {{ index: i, fragment }};
                }}

                // Target path ends with section href
                if (basePath.endsWith(sectionHref)) {{
                    return {{ index: i, fragment }};
                }}

                // Filename match
                if (filename && sectionFilename === filename) {{
                    return {{ index: i, fragment }};
                }}

                // Filename match (URL decoded)
                if (filename && decodeURIComponent(sectionFilename) === decodeURIComponent(filename)) {{
                    return {{ index: i, fragment }};
                }}

                // Partial match - section contains the path or vice versa
                if (sectionHref.includes(basePath) || basePath.includes(sectionFilename)) {{
                    return {{ index: i, fragment }};
                }}
            }}

            console.warn('Could not find section for href:', href);
            return null;
        }}

        // Rewrite URLs in content to use our resource endpoint
        function rewriteUrls(html, sectionHref) {{
            // Get base path from section href
            const basePath = sectionHref.substring(0, sectionHref.lastIndexOf('/') + 1);

            // Rewrite src attributes
            html = html.replace(/src=["']([^"']+)["']/g, (match, url) => {{
                if (url.startsWith('http') || url.startsWith('data:') || url.startsWith('/')) {{
                    return match;
                }}
                const fullPath = resolveUrl(basePath, url);
                return `src="/resource/${{fullPath}}"`;
            }});

            // Rewrite xlink:href for SVG images
            html = html.replace(/xlink:href=["']([^"']+)["']/g, (match, url) => {{
                if (url.startsWith('http') || url.startsWith('data:') || url.startsWith('/') || url.startsWith('#')) {{
                    return match;
                }}
                const fullPath = resolveUrl(basePath, url);
                return `xlink:href="/resource/${{fullPath}}"`;
            }});

            // Rewrite href for stylesheets, images, and links
            html = html.replace(/href=["']([^"']+)["']/g, (match, url) => {{
                if (url.startsWith('http') || url.startsWith('#') || url.startsWith('mailto:')) {{
                    return match;
                }}
                // Check if it's a stylesheet
                if (url.endsWith('.css')) {{
                    const fullPath = resolveUrl(basePath, url);
                    return `href="/resource/${{fullPath}}"`;
                }}
                // Check if it's an image (for SVG image elements, etc.)
                if (url.match(/\.(jpg|jpeg|png|gif|svg|webp)$/i)) {{
                    const fullPath = resolveUrl(basePath, url);
                    return `href="/resource/${{fullPath}}"`;
                }}
                // Internal link - mark for JS handling
                return `href="#" data-epub-href="${{url}}"`;
            }});

            // Rewrite url() in style attributes
            html = html.replace(/url\(["']?([^"')]+)["']?\)/g, (match, url) => {{
                if (url.startsWith('http') || url.startsWith('data:') || url.startsWith('/')) {{
                    return match;
                }}
                const fullPath = resolveUrl(basePath, url);
                return `url(/resource/${{fullPath}})`;
            }});

            return html;
        }}

        // Resolve relative URL
        function resolveUrl(base, relative) {{
            if (relative.startsWith('/')) return relative.substring(1);

            const baseParts = base.split('/').filter(p => p);
            const relParts = relative.split('/');

            for (const part of relParts) {{
                if (part === '..') {{
                    baseParts.pop();
                }} else if (part !== '.') {{
                    baseParts.push(part);
                }}
            }}

            return baseParts.join('/');
        }}

        // Load and display a section
        async function loadSection(index, fragment = null) {{
            if (index < 0 || index >= SECTIONS.length) return;

            currentSection = index;

            const response = await fetch(`/api/section/${{index}}`);
            const data = await response.json();

            let content = rewriteUrls(data.content, data.href);

            displayContent(content, fragment);
            updateStatus();
            highlightTocItem(data.href);
        }}

        // Load all sections for single-page mode
        async function loadAllSections() {{
            const container = document.getElementById('epub-container');
            container.innerHTML = '<div class="loading">Loading all sections...</div>';
            container.classList.add('single-page');

            let allContent = '';

            for (let i = 0; i < SECTIONS.length; i++) {{
                const response = await fetch(`/api/section/${{i}}`);
                const data = await response.json();
                let content = rewriteUrls(data.content, data.href);

                // Extract body content and wrap with section id
                const bodyMatch = content.match(/<body[^>]*>([\s\S]*)<\/body>/i);
                const bodyContent = bodyMatch ? bodyMatch[1] : content;

                // Create anchor for this section
                const sectionId = SECTIONS[i].href.split('/').pop().replace('.xhtml', '').replace('.html', '');
                allContent += `<div id="section-${{i}}" data-section="${{i}}" class="epub-section"><a id="${{sectionId}}"></a>${{bodyContent}}</div><hr style="margin: 2rem 0; border: none; border-top: 1px solid #ddd;">`;
            }}

            displayContent(wrapInHtml(allContent), null);
            document.getElementById('status-bar').style.display = 'none';
            document.getElementById('prev-btn').disabled = true;
            document.getElementById('next-btn').disabled = true;
        }}

        function wrapInHtml(bodyContent) {{
            return `<!DOCTYPE html><html><head><style>
                body {{
                    font-family: Georgia, 'Times New Roman', serif;
                    font-size: 18px;
                    line-height: 1.8;
                    max-width: 800px;
                    margin: 0 auto;
                    padding: 2rem;
                    color: #333;
                }}
                img {{ max-width: 100%; height: auto; }}
                h1, h2, h3, h4, h5, h6 {{ font-family: -apple-system, BlinkMacSystemFont, sans-serif; margin-top: 1.5em; }}
                a {{ color: #0066cc; }}
                .epub-section {{ scroll-margin-top: 20px; }}
                /* Cover image styling */
                svg {{ max-width: 100%; max-height: 90vh; height: auto; display: block; margin: 0 auto; }}
                svg[viewBox] {{ width: auto !important; height: auto !important; }}
                svg image {{ object-fit: contain; }}
            </style></head><body>${{bodyContent}}</body></html>`;
        }}

        // Display content in iframe
        function displayContent(html, fragment) {{
            const container = document.getElementById('epub-container');

            // Add base styles if not present
            if (!html.includes('<style>') && !html.includes('</head>')) {{
                html = wrapInHtml(html.replace(/<body[^>]*>([\s\S]*)<\/body>/i, '$1'));
            }}

            // Create iframe
            const iframe = document.createElement('iframe');
            iframe.style.cssText = 'width: 100%; height: 100%; border: none;';
            container.innerHTML = '';
            container.appendChild(iframe);

            const doc = iframe.contentDocument;
            doc.open();
            doc.write(html);
            doc.close();

            // Handle clicks on internal links
            doc.addEventListener('click', handleLinkClick);

            // Scroll to fragment if specified
            if (fragment) {{
                setTimeout(() => {{
                    const el = doc.getElementById(fragment) || doc.querySelector(`[id="${{fragment}}"]`) || doc.querySelector(`a[name="${{fragment}}"]`);
                    if (el) el.scrollIntoView({{ behavior: 'smooth' }});
                }}, 100);
            }}
        }}

        // Handle internal link clicks
        function handleLinkClick(e) {{
            const link = e.target.closest('a[data-epub-href]');
            if (!link) return;

            e.preventDefault();
            const href = link.dataset.epubHref;

            if (href.startsWith('#')) {{
                // Same-page fragment
                const fragment = href.substring(1);
                const el = e.target.ownerDocument.getElementById(fragment);
                if (el) el.scrollIntoView({{ behavior: 'smooth' }});
            }} else {{
                // Navigate to section
                const result = findSectionByHref(href);
                if (result) {{
                    if (viewMode === 'chapter') {{
                        loadSection(result.index, result.fragment);
                    }} else {{
                        // In single-page mode, scroll to the section
                        const iframe = document.querySelector('#epub-container iframe');
                        if (iframe) {{
                            const doc = iframe.contentDocument;
                            const sectionEl = doc.querySelector(`#section-${{result.index}}`);
                            if (sectionEl) {{
                                sectionEl.scrollIntoView({{ behavior: 'smooth' }});
                                if (result.fragment) {{
                                    setTimeout(() => {{
                                        const fragEl = doc.getElementById(result.fragment);
                                        if (fragEl) fragEl.scrollIntoView({{ behavior: 'smooth' }});
                                    }}, 300);
                                }}
                            }}
                        }}
                    }}
                }}
            }}
        }}

        // Update status bar
        function updateStatus() {{
            document.getElementById('section-num').textContent = currentSection + 1;
            document.getElementById('section-total').textContent = SECTIONS.length;
            const progress = Math.round(((currentSection + 1) / SECTIONS.length) * 100);
            document.getElementById('progress').textContent = progress + '%';
        }}

        // Highlight current TOC item
        function highlightTocItem(href) {{
            document.querySelectorAll('.toc-item').forEach(el => el.classList.remove('active'));
            const basePath = href.split('/').pop().split('#')[0];
            document.querySelectorAll('.toc-item').forEach(el => {{
                const itemHref = el.dataset.href || '';
                if (itemHref.includes(basePath) || basePath.includes(itemHref.split('/').pop().split('#')[0])) {{
                    el.classList.add('active');
                }}
            }});
        }}

        // Switch view mode
        function setViewMode(mode) {{
            viewMode = mode;
            document.getElementById('mode-chapter').classList.toggle('active', mode === 'chapter');
            document.getElementById('mode-single').classList.toggle('active', mode === 'single');

            const container = document.getElementById('epub-container');
            container.classList.toggle('single-page', mode === 'single');

            if (mode === 'chapter') {{
                document.getElementById('status-bar').style.display = 'block';
                document.getElementById('prev-btn').disabled = currentSection === 0;
                document.getElementById('next-btn').disabled = currentSection >= SECTIONS.length - 1;
                loadSection(currentSection);
            }} else {{
                loadAllSections();
            }}
        }}

        // Initialize
        document.addEventListener('DOMContentLoaded', () => {{
            // Render TOC
            document.getElementById('toc').innerHTML = renderToc(TOC);
            document.getElementById('section-total').textContent = SECTIONS.length;

            // TOC click handler
            document.getElementById('toc').addEventListener('click', (e) => {{
                const item = e.target.closest('.toc-item');
                if (!item) return;

                const href = item.dataset.href;
                const result = findSectionByHref(href);

                if (result) {{
                    if (viewMode === 'chapter') {{
                        loadSection(result.index, result.fragment);
                    }} else {{
                        const iframe = document.querySelector('#epub-container iframe');
                        if (iframe) {{
                            const doc = iframe.contentDocument;
                            // Try to find by fragment first
                            if (result.fragment) {{
                                const fragEl = doc.getElementById(result.fragment);
                                if (fragEl) {{
                                    fragEl.scrollIntoView({{ behavior: 'smooth' }});
                                    return;
                                }}
                            }}
                            // Fall back to section
                            const sectionEl = doc.querySelector(`#section-${{result.index}}`);
                            if (sectionEl) sectionEl.scrollIntoView({{ behavior: 'smooth' }});
                        }}
                    }}
                }}
            }});

            // Navigation buttons
            document.getElementById('prev-btn').addEventListener('click', () => {{
                if (viewMode === 'chapter' && currentSection > 0) {{
                    loadSection(currentSection - 1);
                }}
            }});

            document.getElementById('next-btn').addEventListener('click', () => {{
                if (viewMode === 'chapter' && currentSection < SECTIONS.length - 1) {{
                    loadSection(currentSection + 1);
                }}
            }});

            // Mode toggle
            document.getElementById('mode-chapter').addEventListener('click', () => setViewMode('chapter'));
            document.getElementById('mode-single').addEventListener('click', () => setViewMode('single'));

            // Keyboard navigation
            document.addEventListener('keydown', (e) => {{
                if (viewMode !== 'chapter') return;
                if (e.key === 'ArrowLeft') document.getElementById('prev-btn').click();
                else if (e.key === 'ArrowRight') document.getElementById('next-btn').click();
            }});

            // Load first section
            loadSection(0);
            document.getElementById('prev-btn').disabled = false;
            document.getElementById('next-btn').disabled = false;
        }});
    </script>
</body>
</html>"##,
        title = title,
        authors = authors,
        toc_json = toc_json,
        sections_json = sections_json,
    );
    Html(html)
}

async fn epub_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/epub+zip")],
        state.epub_data.clone(),
    )
}

async fn wasm_js_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        state.wasm_js,
    )
}

async fn wasm_bin_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/wasm")],
        state.wasm_bin,
    )
}

async fn resource_handler(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    // Get resource from EPUB
    if let Some(data) = state.book.get_resource(&path) {
        let mime = mime_guess::from_path(&path)
            .first_or_octet_stream()
            .to_string();

        (
            [(header::CONTENT_TYPE, mime)],
            data.to_vec(),
        ).into_response()
    } else {
        // Try without leading path components
        let filename = path.split('/').last().unwrap_or(&path);

        // Search through manifest for matching filename
        for item in state.book.manifest.values() {
            if item.href.ends_with(filename) || item.href == path {
                if let Some(data) = state.book.get_resource(&item.href) {
                    let mime = mime_guess::from_path(&item.href)
                        .first_or_octet_stream()
                        .to_string();

                    return (
                        [(header::CONTENT_TYPE, mime)],
                        data.to_vec(),
                    ).into_response();
                }
            }
        }

        (
            [(header::CONTENT_TYPE, "text/plain".to_string())],
            format!("Resource not found: {}", path).into_bytes(),
        ).into_response()
    }
}

async fn sections_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sections: Vec<_> = state.book.sections().map(|s| {
        serde_json::json!({
            "index": s.index,
            "href": s.href,
            "id": s.id,
        })
    }).collect();

    (
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&sections).unwrap_or_else(|_| "[]".to_string()),
    )
}

async fn section_content_handler(
    State(state): State<Arc<AppState>>,
    Path(index): Path<usize>,
) -> impl IntoResponse {
    // We need mutable access to load section content
    // For simplicity, reload the book for each request
    let mut book = Book::from_bytes(state.epub_data.clone()).unwrap();

    // Get section href first
    let href = match book.section(index) {
        Some(s) => s.href.clone(),
        None => {
            return (
                [(header::CONTENT_TYPE, "application/json")],
                r#"{"error": "Section not found"}"#.to_string(),
            );
        }
    };

    // Now load content
    if let Ok(content) = book.section_content(index) {
        let response = serde_json::json!({
            "index": index,
            "href": href,
            "content": content,
        });

        (
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_string(&response).unwrap(),
        )
    } else {
        (
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error": "Failed to load section content"}"#.to_string(),
        )
    }
}
