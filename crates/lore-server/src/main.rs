use std::sync::Mutex;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

struct AppState {
    db: Mutex<rusqlite::Connection>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_path = db_path();
    eprintln!("Database: {}", db_path.display());

    let conn = lore_core::db::open(&db_path)?;
    let state = std::sync::Arc::new(AppState {
        db: Mutex::new(conn),
    });

    let port: u16 = std::env::var("LORE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    let static_dir = std::env::var("LORE_STATIC")
        .unwrap_or_else(|_| {
            let manifest = env!("CARGO_MANIFEST_DIR");
            format!("{}/static", manifest)
        });

    let app = Router::new()
        .route("/api/pages", get(list_pages))
        .route("/api/pages", post(add_page))
        .route("/api/pages/{id}", get(get_page))
        .route("/api/pages/{id}/content", get(get_page_content))
        .route("/api/search", get(search))
        .route("/api/rules", get(list_rules))
        .fallback_service(ServeDir::new(&static_dir).append_index_html_on_directories(true))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
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

#[derive(Serialize)]
struct PageRow {
    id: i64,
    url: String,
    title: Option<String>,
    domain: String,
    category: String,
    status: String,
    created_at: String,
}

#[derive(Deserialize)]
struct ListParams {
    category: Option<String>,
    status: Option<String>,
    domain: Option<String>,
    limit: Option<usize>,
}

async fn list_pages(
    State(state): State<std::sync::Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<PageRow>>, StatusCode> {
    let conn = state.db.lock().unwrap();
    let limit = params.limit.unwrap_or(100);

    let mut sql = String::from(
        "SELECT id, url, title, domain, category, status, created_at FROM web_page WHERE 1=1",
    );
    let mut bind_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref cat) = params.category {
        sql.push_str(&format!(" AND category = ?{}", idx));
        bind_params.push(Box::new(cat.clone()));
        idx += 1;
    }
    if let Some(ref st) = params.status {
        sql.push_str(&format!(" AND status = ?{}", idx));
        bind_params.push(Box::new(st.clone()));
        idx += 1;
    }
    if let Some(ref dom) = params.domain {
        sql.push_str(&format!(" AND domain LIKE ?{}", idx));
        bind_params.push(Box::new(format!("%{}%", dom)));
        idx += 1;
    }
    sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ?{}", idx));
    bind_params.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let refs: Vec<&dyn rusqlite::types::ToSql> = bind_params.iter().map(|p| p.as_ref()).collect();

    let rows: Vec<PageRow> = stmt
        .query_map(refs.as_slice(), |row| {
            Ok(PageRow {
                id: row.get(0)?,
                url: row.get(1)?,
                title: row.get(2)?,
                domain: row.get(3)?,
                category: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(rows))
}

#[derive(Serialize)]
struct PageDetail {
    id: i64,
    url: String,
    title: Option<String>,
    domain: String,
    category: String,
    status: String,
    created_at: String,
    content_size: Option<i64>,
    has_snapshot: bool,
    plain_text_preview: Option<String>,
}

async fn get_page(
    State(state): State<std::sync::Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<PageDetail>, StatusCode> {
    let conn = state.db.lock().unwrap();

    let page = conn
        .query_row(
            "SELECT id, url, title, domain, category, status, created_at FROM web_page WHERE id = ?1",
            [id],
            |row| {
                Ok(PageDetail {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    domain: row.get(3)?,
                    category: row.get(4)?,
                    status: row.get(5)?,
                    created_at: row.get(6)?,
                    content_size: None,
                    has_snapshot: false,
                    plain_text_preview: None,
                })
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let snapshot = conn
        .query_row(
            "SELECT LENGTH(html_content), SUBSTR(plain_text, 1, 2000) FROM web_page_snapshot WHERE web_page_id = ?1 ORDER BY version DESC LIMIT 1",
            [id],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .ok();

    let mut page = page;
    if let Some((size, text)) = snapshot {
        page.has_snapshot = true;
        page.content_size = size;
        page.plain_text_preview = text;
    }

    Ok(Json(page))
}

#[derive(Serialize)]
struct PageContent {
    html: Option<String>,
    plain_text: Option<String>,
}

async fn get_page_content(
    State(state): State<std::sync::Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<PageContent>, StatusCode> {
    let conn = state.db.lock().unwrap();

    let content = conn
        .query_row(
            "SELECT html_content, plain_text FROM web_page_snapshot WHERE web_page_id = ?1 ORDER BY version DESC LIMIT 1",
            [id],
            |row| {
                Ok(PageContent {
                    html: row.get(0)?,
                    plain_text: row.get(1)?,
                })
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(content))
}

#[derive(Deserialize)]
struct SearchParams {
    q: String,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct SearchResult {
    id: i64,
    url: String,
    title: Option<String>,
    domain: String,
    category: String,
}

async fn search(
    State(state): State<std::sync::Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<SearchResult>>, StatusCode> {
    let conn = state.db.lock().unwrap();
    let limit = params.limit.unwrap_or(20);

    let query = if params.q.contains('*') || params.q.contains('"') || params.q.contains(" AND ") {
        params.q.clone()
    } else {
        params
            .q
            .split_whitespace()
            .map(|w| format!("{}*", w))
            .collect::<Vec<_>>()
            .join(" ")
    };

    let mut stmt = conn
        .prepare(
            "SELECT wp.id, wp.url, wp.title, wp.domain, wp.category
             FROM web_page_fts fts
             JOIN web_page_snapshot wps ON wps.id = fts.rowid
             JOIN web_page wp ON wp.id = wps.web_page_id
             WHERE web_page_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows: Vec<SearchResult> = stmt
        .query_map(rusqlite::params![query, limit as i64], |row| {
            Ok(SearchResult {
                id: row.get(0)?,
                url: row.get(1)?,
                title: row.get(2)?,
                domain: row.get(3)?,
                category: row.get(4)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(rows))
}

#[derive(Deserialize)]
struct AddPageRequest {
    url: String,
    title: Option<String>,
}

#[derive(Serialize)]
struct AddPageResponse {
    id: i64,
    category: String,
    status: String,
}

async fn add_page(
    State(state): State<std::sync::Arc<AppState>>,
    Json(req): Json<AddPageRequest>,
) -> Result<Json<AddPageResponse>, (StatusCode, String)> {
    let conn = state.db.lock().unwrap();

    let parsed = url::Url::parse(&req.url).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid URL: {}", e),
        )
    })?;

    let rules = lore_core::db::load_rules(&conn).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let normalized = lore_core::rules::normalize_url(&parsed);
    let domain = parsed.host_str().unwrap_or("unknown").to_string();
    let category = lore_core::rules::classify(&parsed, &rules);
    let status = if category == "archive" {
        "queued"
    } else {
        "skipped"
    };

    let id = lore_core::db::insert_web_page(
        &conn,
        &lore_core::db::NewWebPage {
            url: &req.url,
            url_normalized: &normalized,
            title: req.title.as_deref(),
            domain: &domain,
            category: &category,
            status,
            source: Some("web"),
        },
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(AddPageResponse {
        id,
        category,
        status: status.to_string(),
    }))
}

#[derive(Serialize)]
struct RuleRow {
    id: i64,
    pattern: String,
    match_type: String,
    category: String,
    priority: i64,
    note: Option<String>,
}

async fn list_rules(
    State(state): State<std::sync::Arc<AppState>>,
) -> Result<Json<Vec<RuleRow>>, StatusCode> {
    let conn = state.db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id, pattern, match_type, category, priority, note FROM classification_rule ORDER BY priority DESC")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows: Vec<RuleRow> = stmt
        .query_map([], |row| {
            Ok(RuleRow {
                id: row.get(0)?,
                pattern: row.get(1)?,
                match_type: row.get(2)?,
                category: row.get(3)?,
                priority: row.get(4)?,
                note: row.get(5)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(rows))
}
