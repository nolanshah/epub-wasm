//! EPUB Reader Test Server
//!
//! Loads an EPUB from disk and serves the client-test reader UI for it.
//! The UI, the wasm package and the book are all embedded/served from this
//! one binary — the browser does the parsing and rendering via WASM.
//!
//! Usage: epub-server-test <path-to-epub> [-p PORT]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{RawQuery, State},
    http::header,
    response::{Html, IntoResponse, Redirect},
    routing::get,
    Router,
};
use clap::Parser;
use epub_reader_core::Book;
use tower_http::cors::CorsLayer;

// The reader UI and wasm package, embedded at compile time.
// Build the wasm first: `wasm-pack build --release --target web renderer`
const INDEX_HTML: &str = include_str!("../../client-test/index.html");
const RENDITION_HTML: &str = include_str!("../../client-test/rendition.html");
const WASM_JS: &str = include_str!("../../renderer/pkg/epub_reader_renderer.js");
const WASM_BIN: &[u8] = include_bytes!("../../renderer/pkg/epub_reader_renderer_bg.wasm");

#[derive(Parser)]
#[command(name = "epub-server-test")]
#[command(about = "Test server for the epub-wasm reader")]
struct Args {
    /// Path to the EPUB file to serve
    epub_path: PathBuf,

    /// Port to run the server on
    #[arg(short, long, default_value = "3000")]
    port: u16,
}

struct AppState {
    epub_data: Vec<u8>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let epub_data = std::fs::read(&args.epub_path)?;

    // Validate it parses before serving it
    let book = Book::from_bytes(epub_data.clone())?;
    println!("Loaded: {}", book.metadata.title);
    println!("Author: {}", book.metadata.creators.join(", "));
    println!("Sections: {}", book.section_count());

    let state = Arc::new(AppState { epub_data });

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/index.html", get(index_handler))
        .route("/rendition.html", get(|| async { Html(RENDITION_HTML) }))
        .route("/book.epub", get(epub_handler))
        .route(
            "/pkg/epub_reader_renderer.js",
            get(|| async { ([(header::CONTENT_TYPE, "application/javascript")], WASM_JS) }),
        )
        .route(
            "/pkg/epub_reader_renderer_bg.wasm",
            get(|| async { ([(header::CONTENT_TYPE, "application/wasm")], WASM_BIN) }),
        )
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    println!("\n🚀 Server running at http://{}", addr);
    println!("   Reader UI:  http://{}/", addr);
    println!("   Rendition:  http://{}/rendition.html\n", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Serve the reader; without a `load` param, redirect so the UI opens the
/// book straight away instead of showing the upload screen.
async fn index_handler(RawQuery(query): RawQuery) -> impl IntoResponse {
    match query {
        Some(q) if q.contains("load=") => Html(INDEX_HTML).into_response(),
        _ => Redirect::temporary("/?load=%2Fbook.epub").into_response(),
    }
}

async fn epub_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/epub+zip")],
        state.epub_data.clone(),
    )
}
