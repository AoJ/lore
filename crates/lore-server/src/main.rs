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
use tower_http::services::ServeDir;

mod handlers;

use handlers::AppState;

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

    let app = Router::new()
        .nest("/api", api)
        // index.html served with no-store so stale bundle hashes never stick.
        // The JS/WASM assets already have content hashes in their filenames
        // (dx build), so ServeDir's default headers are fine for them.
        .route("/", get(handlers::serve_index))
        .route("/index.html", get(handlers::serve_index))
        // Static files (W3: WASM bundle); only catches non-`/api/*` paths
        // because the `nest` above owns everything under `/api`.
        .fallback_service(ServeDir::new(&static_dir).append_index_html_on_directories(true))
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
