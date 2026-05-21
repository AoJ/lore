//! Integration test harness for the WASM frontend.
//!
//! Each [`TestApp`] spins up:
//! - a fresh SQLite DB under a per-test [`TempDir`]
//! - the `lore-serve` binary as a subprocess bound to a random local port
//! - a headless Chromium via `chromiumoxide`
//! - a page navigated to the server root, with the WASM bundle booted
//!
//! Cleanup on `Drop`: SIGTERM the server, the `TempDir` removes the DB,
//! the browser handle drops its handler task. Tests can run in parallel
//! because each gets its own port + DB.
//!
//! The harness assumes:
//! - `lore-serve` has been built (e.g. via `make e2e` which depends on
//!   `make web` + `cargo build -p lore-server`)
//! - the web bundle exists in `crates/lore-server/static/`
//! - `Chromium.app` (or `chromium` on PATH) is installed
//!
//! `LORE_SERVER_BIN` env var overrides the binary path. `LORE_BROWSER`
//! overrides the Chromium path (matches `lore-worker`'s convention).

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures_util::StreamExt;
use serde_json::Value;
use tempfile::TempDir;

const WASM_BOOT_TIMEOUT: Duration = Duration::from_secs(15);
const SELECTOR_POLL_INTERVAL: Duration = Duration::from_millis(50);
const SELECTOR_TIMEOUT: Duration = Duration::from_secs(5);

/// One isolated test fixture: server + DB + headless browser + a single
/// open page. Drop kills the server and frees the temp DB.
pub struct TestApp {
    /// Base URL of the spawned server, e.g. `http://127.0.0.1:54321`.
    pub base_url: String,
    /// Port the server is bound to. Exposed so `restart_server` can rebind
    /// on the same port after `stop_server` kills the process.
    pub server_port: u16,
    /// Path to the SQLite DB used by the server subprocess.
    pub db_path: std::path::PathBuf,
    /// Currently-open Chromium page, already past `wait_for_navigation`.
    /// Tests usually call `wait_for(...)` first to ensure WASM is mounted.
    pub page: chromiumoxide::Page,
    /// Kept alive so its task keeps draining CDP events.
    _browser: Browser,
    _browser_handler: tokio::task::JoinHandle<()>,
    server: Child,
    _db_dir: TempDir,
}

impl TestApp {
    /// Boot a server on a random port with an empty DB, launch headless
    /// Chromium, navigate to `/`, and wait for the WASM bundle to mount.
    pub async fn spawn() -> Result<Self> {
        let db_dir = tempfile::tempdir().context("create temp dir for test DB")?;
        let db_path = db_dir.path().join("lore-e2e.sqlite");

        let port = pick_port()?;

        let bin = std::env::var("LORE_SERVER_BIN")
            .unwrap_or_else(|_| default_server_bin().display().to_string());

        let server = Command::new(&bin)
            .env("LORE_DB", &db_path)
            .env("LORE_PORT", port.to_string())
            // The server boots a tokio runtime that writes a banner to
            // stderr; we don't need to capture it but mustn't block on it.
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn lore-serve binary at {}", bin))?;

        let base_url = format!("http://127.0.0.1:{}", port);
        wait_for_http_ready(&base_url, Duration::from_secs(10)).await?;

        let (browser, handler) = launch_browser().await?;
        let handler_task = tokio::spawn(async move {
            let mut h = handler;
            while let Some(_) = h.next().await {}
        });

        let page = browser
            .new_page(format!("{}/", base_url))
            .await
            .context("open new page")?;
        page.wait_for_navigation()
            .await
            .context("wait for initial navigation")?;

        let app = TestApp {
            base_url,
            server_port: port,
            db_path: db_path.clone(),
            page,
            _browser: browser,
            _browser_handler: handler_task,
            server,
            _db_dir: db_dir,
        };

        // Block until the WASM bundle has mounted the app layout — every
        // test needs this and forgetting leads to flaky "selector not
        // found" errors on the first poll.
        app.wait_for(".app-layout", WASM_BOOT_TIMEOUT).await?;
        Ok(app)
    }

    /// Direct HTTP API call (`POST /api/<method>`). Useful for seeding state
    /// before driving UI assertions ("create 3 notes, then check sidebar
    /// renders 3 list-items").
    pub async fn api_post(&self, method: &str, body: Value) -> Result<Value> {
        let url = format!("{}/api/{}", self.base_url, method);
        let body_str = body.to_string();
        let js = format!(
            r#"
            (async () => {{
                const r = await fetch({url}, {{
                    method: 'POST',
                    headers: {{ 'Content-Type': 'application/json' }},
                    body: {body},
                }});
                return {{ status: r.status, body: await r.text() }};
            }})()
            "#,
            url = serde_json::to_string(&url)?,
            body = serde_json::to_string(&body_str)?,
        );
        let result: Value = self
            .page
            .evaluate(js.as_str())
            .await
            .context("evaluate fetch")?
            .into_value()
            .context("parse fetch result")?;

        let status = result.get("status").and_then(Value::as_u64).unwrap_or(0);
        let body_text = result
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        if !(200..300).contains(&status) {
            return Err(anyhow!(
                "POST /api/{} → HTTP {}: {}",
                method,
                status,
                body_text
            ));
        }
        // Server returns JSON; unit returns are literal `null`.
        Ok(serde_json::from_str(&body_text).unwrap_or(Value::Null))
    }

    /// Raw HTTP call via `fetch` from inside the page. Returns the status
    /// code and the response body verbatim — no 2xx check, no JSON parse.
    /// Use this for assertions on the error wire format (`code: ...`),
    /// for GET endpoints that aren't on the RPC trait surface
    /// (`/api/files/:id/raw`), and anywhere [`api_post`] would swallow
    /// the info you care about.
    ///
    /// `body` is the request body string (typically JSON); pass `None`
    /// for GET. Content-Type is set to `application/json` whenever a
    /// body is provided.
    pub async fn fetch_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, String)> {
        let url = format!("{}{}", self.base_url, path);
        let opts_body = match body {
            Some(b) => format!(
                "body: {body}, headers: {{ 'Content-Type': 'application/json' }},",
                body = serde_json::to_string(b)?,
            ),
            None => String::new(),
        };
        let js = format!(
            r#"
            (async () => {{
                const r = await fetch({url}, {{ method: {method}, {opts_body} }});
                return {{ status: r.status, body: await r.text() }};
            }})()
            "#,
            url = serde_json::to_string(&url)?,
            method = serde_json::to_string(method)?,
        );
        let result: Value = self
            .page
            .evaluate(js.as_str())
            .await
            .context("evaluate fetch_raw")?
            .into_value()
            .context("parse fetch_raw result")?;
        let status = result.get("status").and_then(Value::as_u64).unwrap_or(0) as u16;
        let body = result
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Ok((status, body))
    }

    /// Click the first element matching `selector` whose `textContent`
    /// (trimmed) equals `text`. Useful for sidebar nav items where the
    /// CSS class is shared across siblings — no need to count
    /// `nth-child` indices.
    pub async fn click_text(&self, selector: &str, text: &str) -> Result<()> {
        let js = format!(
            r#"
            (() => {{
                const items = Array.from(document.querySelectorAll({sel}));
                const target = items.find(el => el.textContent.trim() === {text});
                if (target) {{ target.click(); return true; }}
                return false;
            }})()
            "#,
            sel = serde_json::to_string(selector)?,
            text = serde_json::to_string(text)?,
        );
        let found: bool = self
            .page
            .evaluate(js.as_str())
            .await
            .context("evaluate click_text")?
            .into_value()
            .context("parse click_text result")?;
        if !found {
            return Err(anyhow!(
                "no `{}` element with textContent `{}`",
                selector,
                text
            ));
        }
        Ok(())
    }

    /// Wait until `selector` appears in the DOM. Polls every 50 ms up to
    /// `timeout`; returns the first matching element handle.
    pub async fn wait_for(
        &self,
        selector: &str,
        timeout: Duration,
    ) -> Result<chromiumoxide::Element> {
        let start = Instant::now();
        loop {
            if let Ok(el) = self.page.find_element(selector).await {
                return Ok(el);
            }
            if start.elapsed() > timeout {
                return Err(anyhow!(
                    "selector `{}` did not appear within {:?}",
                    selector,
                    timeout
                ));
            }
            tokio::time::sleep(SELECTOR_POLL_INTERVAL).await;
        }
    }

    /// `wait_for` with the default 5 s timeout.
    pub async fn wait_for_default(&self, selector: &str) -> Result<chromiumoxide::Element> {
        self.wait_for(selector, SELECTOR_TIMEOUT).await
    }

    /// Click an element, retrying selector lookup until it shows up.
    pub async fn click(&self, selector: &str) -> Result<()> {
        let el = self.wait_for_default(selector).await?;
        el.click().await.context("click")?;
        Ok(())
    }

    /// Read `innerText` from the first matching element.
    pub async fn text(&self, selector: &str) -> Result<String> {
        let el = self.wait_for_default(selector).await?;
        let text = el
            .inner_text()
            .await
            .context("read inner_text")?
            .unwrap_or_default();
        Ok(text)
    }

    /// Block until a polled predicate returns Ok with a non-empty value or
    /// the timeout expires. Useful for polling-driven UI: revision bumps,
    /// list-item count changes, etc.
    pub async fn wait_until<F, T, Fut>(&self, mut predicate: F, timeout: Duration) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<Option<T>>>,
    {
        let start = Instant::now();
        loop {
            match predicate().await {
                Ok(Some(v)) => return Ok(v),
                Ok(None) => {}
                Err(e) => return Err(e),
            }
            if start.elapsed() > timeout {
                return Err(anyhow!("predicate did not satisfy within {:?}", timeout));
            }
            tokio::time::sleep(SELECTOR_POLL_INTERVAL).await;
        }
    }

    /// PNG screenshot to `path`. Mostly for ad-hoc debugging of failing
    /// tests; functional tests should assert on DOM, not pixels.
    pub async fn screenshot(&self, path: impl Into<PathBuf>) -> Result<()> {
        let path = path.into();
        let bytes = self
            .page
            .screenshot(chromiumoxide::page::ScreenshotParams::default())
            .await
            .context("take screenshot")?;
        std::fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    /// Kill the server process. The DB and the open browser page are kept
    /// intact. Use together with `restart_server` to test offline/recovery.
    pub fn stop_server(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }

    /// Spawn a fresh `lore-serve` process on the same port against the same
    /// DB. Waits until the server accepts HTTP connections before returning.
    /// Call only after `stop_server`.
    pub async fn restart_server(&mut self) -> Result<()> {
        let bin = std::env::var("LORE_SERVER_BIN")
            .unwrap_or_else(|_| default_server_bin().display().to_string());
        self.server = Command::new(&bin)
            .env("LORE_DB", &self.db_path)
            .env("LORE_PORT", self.server_port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("restart lore-serve at {}", bin))?;
        wait_for_http_ready(&self.base_url, Duration::from_secs(10)).await?;
        Ok(())
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        // Best-effort SIGTERM. The OS reaps the child; the temp dir's
        // own Drop wipes the DB file.
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

// ---- Internals ----

fn default_server_bin() -> PathBuf {
    // Cargo runs tests with CWD = the test crate's manifest dir, so step
    // up to the workspace root to find `target/debug/lore-serve`.
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
        .join("../../target/debug/lore-serve")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(manifest).join("../../target/debug/lore-serve"))
}

fn pick_port() -> Result<u16> {
    // Bind to :0 to let the kernel choose, then drop the listener — the
    // port might briefly be free for the server to grab. Race risk is
    // tiny (no other process is competing on this host during tests).
    let listener = std::net::TcpListener::bind("127.0.0.1:0").context("bind random port")?;
    Ok(listener.local_addr()?.port())
}

async fn wait_for_http_ready(base_url: &str, timeout: Duration) -> Result<()> {
    use std::io::{Read, Write};
    let start = Instant::now();
    let host_port = base_url.trim_start_matches("http://");
    loop {
        if let Ok(mut s) = std::net::TcpStream::connect(host_port) {
            // Connection succeeded — issue a HEAD-ish request to confirm
            // axum is serving (not just port bound). Empty response body
            // is fine; we only care that the connection talks HTTP.
            let _ = s.write_all(b"GET / HTTP/1.0\r\nHost: x\r\n\r\n");
            let mut buf = [0u8; 16];
            let _ = s.read(&mut buf);
            if buf.starts_with(b"HTTP/1.") {
                return Ok(());
            }
        }
        if start.elapsed() > timeout {
            return Err(anyhow!("server not ready within {:?}", timeout));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn launch_browser() -> Result<(Browser, chromiumoxide::handler::Handler)> {
    let browser_path = std::env::var("LORE_BROWSER").ok();
    // Headless Chromium aggressively throttles `setTimeout` in non-focused
    // contexts. `gloo-timers::future::sleep` (the web build's `platform::
    // sleep`) is just `setTimeout` under the hood, so the 2 s poll loop
    // ends up running once a minute — long enough to fail any
    // sync/refresh test inside its timeout window. These flags pin the
    // renderer at full speed for the duration of the run.
    let mut config = BrowserConfig::builder()
        .no_sandbox()
        .window_size(1280, 800)
        .arg("--disable-gpu")
        .arg("--disable-background-timer-throttling")
        .arg("--disable-renderer-backgrounding")
        .arg("--disable-backgrounding-occluded-windows");
    if let Some(path) = browser_path {
        config = config.chrome_executable(path);
    } else if std::path::Path::new("/Applications/Chromium.app/Contents/MacOS/Chromium").exists() {
        config = config.chrome_executable("/Applications/Chromium.app/Contents/MacOS/Chromium");
    }
    let config = config.build().map_err(|e| anyhow!("{}", e))?;
    Browser::launch(config).await.context("launch chromium")
}

// ---- DB seeding helpers ----
//
// The test process opens its own SQLite connection to the same DB the server
// uses (WAL mode tolerates concurrent readers + one writer). Helpers below
// are thin convenience wrappers — tests reach for them whenever HTTP seeding
// would mean wiring up a fixture for state the API doesn't expose directly
// (e.g. pre-existing snapshot versions, which would otherwise require
// running the headless-Chrome worker).

impl TestApp {
    /// Open a short-lived rusqlite connection to the test DB. Uses
    /// `open_existing` so it doesn't re-run migrations — the server's
    /// initial `db::open` already did that on startup.
    pub fn conn(&self) -> anyhow::Result<rusqlite::Connection> {
        lore_core::db::open_existing(&self.db_path)
    }

    /// Insert a page (status `archived`) plus N snapshots, returning the
    /// page id. Each snapshot gets a distinct plain_text so `content_hash`
    /// values differ and `change_summary` reflects the diff.
    pub fn seed_page_with_snapshots(
        &self,
        url: &str,
        title: &str,
        snapshot_bodies: &[&str],
    ) -> anyhow::Result<i64> {
        use lore_core::db::{self, NewWebPage};
        let conn = self.conn()?;
        let page_id = db::insert_web_page(
            &conn,
            &NewWebPage {
                url,
                url_normalized: url,
                title: Some(title),
                domain: "example.test",
                category: "archive",
                status: "archived",
                source: None,
                space_id: Some(1),
            },
        )?;
        for body in snapshot_bodies {
            db::insert_snapshot(&conn, page_id, "<html></html>", body, None)?;
        }
        Ok(page_id)
    }

    /// Update a page's title — used between snapshot inserts to verify
    /// `title_changed` ends up true in the next snapshot's `change_summary`.
    pub fn set_page_title(&self, page_id: i64, title: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE web_page SET title = ?1 WHERE id = ?2",
            rusqlite::params![title, page_id],
        )?;
        Ok(())
    }

    /// Add one more snapshot to an existing page (in addition to whatever
    /// `seed_page_with_snapshots` produced). Returns the new snapshot id.
    pub fn add_snapshot(&self, page_id: i64, plain_text: &str) -> anyhow::Result<i64> {
        let conn = self.conn()?;
        lore_core::db::insert_snapshot(&conn, page_id, "<html></html>", plain_text, None)
    }
}
