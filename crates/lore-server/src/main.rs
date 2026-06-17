//! `lore-server` — HTTP API in front of the SQLite store. Endpoints mirror
//! the `Backend` trait surface 1:1 so the future Dioxus/WASM client
//! (`HttpBackend`) can swap in transparently. All endpoints are
//! `POST /api/<method>` with JSON in/out.
//!
//! Bootstrap (migrations + seed) runs once on startup; per-request
//! handlers open fresh `Connection`s via `db::open_existing` against the
//! shared `db_path` — mirrors the desktop `LocalBackend` model.

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use tower_http::cors::CorsLayer;
#[cfg(not(feature = "embed-web"))]
use tower_http::services::ServeDir;

mod handlers;

use handlers::AppState;

/// Serve the `make web` bundle straight out of the binary. Enabled by the
/// `embed-web` feature so a release server ships as a single self-contained
/// file (no `static/` dir to deploy alongside and keep in sync). The default
/// build serves the same files from disk via `ServeDir` instead.
#[cfg(feature = "embed-web")]
mod web_embed {
    use axum::body::Body;
    use axum::http::{StatusCode, Uri, header};
    use axum::response::Response;

    #[derive(rust_embed::RustEmbed)]
    #[folder = "static/"]
    struct WebAssets;

    /// `index.html` with `Cache-Control: no-store` (mirrors `handlers::serve_index`).
    pub async fn index() -> Response {
        serve("index.html", true)
    }

    /// Any other bundle path (`/assets/…wasm`, `/assets/…js`). Hashed names →
    /// cacheable, so no `no-store`.
    pub async fn asset(uri: Uri) -> Response {
        let path = uri.path().trim_start_matches('/');
        serve(if path.is_empty() { "index.html" } else { path }, false)
    }

    fn serve(path: &str, no_store: bool) -> Response {
        match WebAssets::get(path) {
            Some(file) => {
                let mime = mime_guess::from_path(path).first_or_octet_stream();
                let mut builder = Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, mime.as_ref());
                if no_store {
                    builder = builder.header(header::CACHE_CONTROL, "no-store");
                }
                builder.body(Body::from(file.data.into_owned())).unwrap()
            }
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("not found"))
                .unwrap(),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_path = db_path();
    eprintln!("Database: {}", db_path.display());

    // Bootstrap once: opens DB, applies pending migrations, seeds defaults.
    // Drops the connection immediately — every handler opens its own.
    let _ = lore_core::db::open(&db_path)?;

    let port: u16 = std::env::var("LORE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    let static_dir = std::env::var("LORE_STATIC").unwrap_or_else(|_| {
        let manifest = env!("CARGO_MANIFEST_DIR");
        format!("{}/static", manifest)
    });

    let state = Arc::new(AppState {
        db_path: db_path.clone(),
        static_dir: std::path::PathBuf::from(&static_dir),
    });

    // API router: every endpoint is `POST /api/<method>` with JSON in/out.
    // Routes that don't match here fall through to the api-level fallback,
    // which returns a structured `route_not_found` JSON error so the
    // client can distinguish "no such endpoint" from "no such entity".
    let api = Router::new()
        // Bootstrap
        .route("/get_revision", post(handlers::get_revision))
        .route("/db_schema_version", post(handlers::db_schema_version))
        // Spaces
        .route("/list_spaces", post(handlers::list_spaces))
        .route("/list_all_spaces", post(handlers::list_all_spaces))
        .route("/get_active_space", post(handlers::get_active_space))
        .route("/space_stats", post(handlers::space_stats))
        .route("/touch_space", post(handlers::touch_space))
        .route("/create_space", post(handlers::create_space))
        .route("/rename_space", post(handlers::rename_space))
        .route("/trash_space", post(handlers::trash_space))
        .route("/restore_space", post(handlers::restore_space))
        .route(
            "/delete_space_permanent",
            post(handlers::delete_space_permanent),
        )
        // Folders
        .route("/list_folders", post(handlers::list_folders))
        .route("/folder_note_counts", post(handlers::folder_note_counts))
        .route("/create_folder", post(handlers::create_folder))
        .route("/rename_folder", post(handlers::rename_folder))
        .route("/delete_folder", post(handlers::delete_folder))
        // Notes
        .route("/list_notes", post(handlers::list_notes))
        .route(
            "/list_note_ids_ordered",
            post(handlers::list_note_ids_ordered),
        )
        .route("/get_note", post(handlers::get_note))
        .route("/create_note", post(handlers::create_note))
        .route("/update_note", post(handlers::update_note))
        .route("/move_note", post(handlers::move_note))
        .route("/trash_note", post(handlers::trash_note))
        .route("/restore_note", post(handlers::restore_note))
        .route(
            "/delete_note_permanent",
            post(handlers::delete_note_permanent),
        )
        .route(
            "/find_notes_referencing_url",
            post(handlers::find_notes_referencing_url),
        )
        // Pages
        .route("/list_pages", post(handlers::list_pages))
        .route(
            "/list_page_ids_ordered",
            post(handlers::list_page_ids_ordered),
        )
        .route("/get_page", post(handlers::get_page))
        .route("/archive_url", post(handlers::archive_url))
        .route(
            "/auto_archive_from_text",
            post(handlers::auto_archive_from_text),
        )
        .route("/check_urls_status", post(handlers::check_urls_status))
        .route("/trash_page", post(handlers::trash_page))
        .route("/restore_page", post(handlers::restore_page))
        .route(
            "/delete_page_permanent",
            post(handlers::delete_page_permanent),
        )
        .route("/update_page_status", post(handlers::update_page_status))
        .route("/list_page_versions", post(handlers::list_page_versions))
        .route("/get_page_version", post(handlers::get_page_version))
        .route("/delete_page_version", post(handlers::delete_page_version))
        .route(
            "/get_snapshot_full_screenshot",
            post(handlers::get_snapshot_full_screenshot),
        )
        .route("/export_snapshot", post(handlers::export_snapshot))
        .route(
            "/snapshots/{snapshot_id}/export",
            get(handlers::export_snapshot_raw),
        )
        .route("/request_reachive", post(handlers::request_reachive))
        // Files
        .route("/list_files", post(handlers::list_files))
        .route("/get_file", post(handlers::get_file))
        .route("/get_file_data", post(handlers::get_file_data))
        .route("/insert_file", post(handlers::insert_file))
        .route("/trash_file", post(handlers::trash_file))
        .route("/restore_file", post(handlers::restore_file))
        .route(
            "/delete_file_permanent",
            post(handlers::delete_file_permanent),
        )
        // Attachments
        .route("/list_attachments", post(handlers::list_attachments))
        .route(
            "/list_removed_attachments",
            post(handlers::list_removed_attachments),
        )
        .route("/get_attachment", post(handlers::get_attachment))
        .route("/get_attachment_data", post(handlers::get_attachment_data))
        .route("/insert_attachment", post(handlers::insert_attachment))
        .route(
            "/cleanup_orphaned_attachments",
            post(handlers::cleanup_orphaned_attachments),
        )
        .route("/restore_attachment", post(handlers::restore_attachment))
        // Trash
        .route("/list_trash", post(handlers::list_trash))
        .route("/trash_count", post(handlers::trash_count))
        // Activity
        .route("/activity_by_day", post(handlers::activity_by_day))
        .route("/activity_for_day", post(handlers::activity_for_day))
        // Classification
        .route("/load_rules", post(handlers::load_rules))
        // FTS5 search
        .route("/search_pages_brief", post(handlers::search_pages_brief))
        .route("/search_notes", post(handlers::search_notes))
        // Raw blob downloads (browser-native: anchor + Content-Disposition).
        // GET, not POST — the response is the bytes, not JSON.
        .route("/files/{id}/raw", get(handlers::file_raw))
        .route("/attachments/{id}/raw", get(handlers::attachment_raw))
        .fallback(handlers::route_not_found);

    let app = Router::new().nest("/api", api);

    // Static web bundle. index.html is served with no-store so stale bundle
    // hashes never stick; JS/WASM assets carry content hashes in their
    // filenames (dx build), so default caching is fine. Either baked into the
    // binary (`embed-web`) or served from disk — both only catch non-`/api/*`
    // paths since the `nest` above owns everything under `/api`.
    #[cfg(feature = "embed-web")]
    let app = app
        .route("/", get(web_embed::index))
        .route("/index.html", get(web_embed::index))
        .fallback(web_embed::asset);

    #[cfg(not(feature = "embed-web"))]
    let app = app
        .route("/", get(handlers::serve_index))
        .route("/index.html", get(handlers::serve_index))
        .fallback_service(ServeDir::new(&static_dir).append_index_html_on_directories(true));

    let app = app
        // Permissive CORS — server is expected to bind to localhost only in
        // dev. Production deployment behind a reverse proxy must restrict
        // origin lists (deferred together with auth).
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    eprintln!("Listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn db_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("LORE_DB") {
        return std::path::PathBuf::from(p);
    }
    if let Some(dir) = dirs::data_local_dir() {
        let p = dir.join("lore");
        std::fs::create_dir_all(&p).ok();
        return p.join("lore.db");
    }
    std::path::PathBuf::from("lore.db")
}
