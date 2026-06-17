//! Integration test harness for the WASM frontend.
//!
//! Each [`TestApp`] spins up:
//! - a fresh SQLite DB under a per-test [`TempDir`]
//! - the `lore-serve` binary as a subprocess bound to a random local port
//! - a headless Chromium via `chromiumoxide`
//! - a page navigated to the server root, with the WASM bundle booted
//!
//! Cleanup on `Drop`: kill the server, the `TempDir` removes the DB, and the
//! browser's Chromium tree is SIGKILLed by its unique profile dir (chromiumoxide
//! only relies on tokio `kill_on_drop`, which reaps too lazily for a busy
//! single-core host). Tests can run in parallel because each gets its own
//! port + DB.
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
    /// This browser's unique `--user-data-dir`; used by `Drop` to SIGKILL the
    /// Chromium tree synchronously (see [`kill_chrome_for_profile`]).
    browser_profile_dir: PathBuf,
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

        let mut server = Command::new(&bin)
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

        let (browser, handler_task, page, browser_profile_dir) =
            match open_browser_page(&base_url).await {
                Ok(v) => v,
                Err(e) => {
                    // `server` hasn't moved into a `TestApp` yet, so its `Drop`
                    // won't run — a bare `std::process::Child` doesn't kill on
                    // drop, so kill it here or the lore-serve process leaks.
                    let _ = server.kill();
                    let _ = server.wait();
                    return Err(e);
                }
            };

        let app = TestApp {
            base_url,
            server_port: port,
            db_path: db_path.clone(),
            page,
            _browser: browser,
            _browser_handler: handler_task,
            browser_profile_dir,
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
    ///
    /// Polls up to the default selector timeout: the target often appears a
    /// render after whatever click opened its panel (e.g. the Export button
    /// landing after the page detail mounts), so a single query races it.
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
        let start = Instant::now();
        loop {
            let found: bool = self
                .page
                .evaluate(js.as_str())
                .await
                .context("evaluate click_text")?
                .into_value()
                .context("parse click_text result")?;
            if found {
                return Ok(());
            }
            if start.elapsed() > SELECTOR_TIMEOUT {
                return Err(anyhow!(
                    "no `{}` element with textContent `{}` within {:?}",
                    selector,
                    text,
                    SELECTOR_TIMEOUT
                ));
            }
            tokio::time::sleep(SELECTOR_POLL_INTERVAL).await;
        }
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
        // Synchronously kill this test's Chromium tree. `Browser::drop` only
        // sets tokio `kill_on_drop`, which reaps lazily — on a single core the
        // dying browser starves the next test's into hangs/timeouts.
        kill_chrome_for_profile(&self.browser_profile_dir);
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

/// Launch a browser and open the app page.
///
/// The flaky hang we kept hitting is a `chromiumoxide` race, not host luck:
/// `browser.new_page(url)` resolves only once the target's main frame reports a
/// `"load"` lifecycle event (`handler/target.rs` → `Frame::is_loaded`). But the
/// lifecycle stream is enabled as part of the target's *init command chain*, so
/// if the page finishes loading before that chain is processed, the `"load"`
/// event is never observed and `new_page` waits forever. Whether load wins the
/// race is pure timing — a warm WASM bundle in the page cache, or a busy single
/// core delaying the init commands, both make it lose — which is exactly the
/// "passes cold, hangs after a build / on repeat" pattern we measured.
///
/// The page itself *is* created and *does* load; only the signal is missed. So
/// instead of betting on retries, we bound `new_page` and, on timeout, recover
/// the already-loaded page out of `browser.pages()` (which doesn't gate on
/// `is_loaded`). A couple of fresh-browser retries remain as a backstop for the
/// rarer case where the whole launch is wedged. Returns the profile dir so the
/// caller tears the browser down the same way.
async fn open_browser_page(
    base_url: &str,
) -> Result<(
    Browser,
    tokio::task::JoinHandle<()>,
    chromiumoxide::Page,
    PathBuf,
)> {
    const ATTEMPTS: usize = 3;
    const NEW_PAGE_TIMEOUT: Duration = Duration::from_secs(10);
    const RECOVER_TIMEOUT: Duration = Duration::from_secs(5);

    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=ATTEMPTS {
        let (browser, handler, profile_dir) = launch_browser().await?;
        let handler_task = tokio::spawn(async move {
            let mut h = handler;
            while h.next().await.is_some() {}
        });

        let url = format!("{}/", base_url);
        let page = match tokio::time::timeout(NEW_PAGE_TIMEOUT, browser.new_page(url)).await {
            Ok(Ok(page)) => Some(page),
            // Timed out waiting for the missed `"load"` event — pull the page
            // that was nonetheless created+loaded back out of the browser.
            Err(_) => {
                tokio::time::timeout(RECOVER_TIMEOUT, recover_loaded_page(&browser, base_url))
                    .await
                    .ok()
                    .flatten()
            }
            Ok(Err(e)) => {
                last_err = Some(anyhow::Error::new(e).context("open new page"));
                None
            }
        };

        if let Some(page) = page {
            // `wait_for_navigation` waits for the *next* lifecycle navigation;
            // the initial load may already be done, so bound it — the real
            // readiness gate is the `.app-layout` poll in the caller.
            let _ = tokio::time::timeout(WASM_BOOT_TIMEOUT, page.wait_for_navigation()).await;
            return Ok((browser, handler_task, page, profile_dir));
        }

        if last_err.is_none() {
            last_err = Some(anyhow!(
                "new_page hung and page recovery failed (attempt {attempt})"
            ));
        }
        // Tear this browser down (the CDP channel may be wedged, so kill by
        // profile dir) before retrying with a fresh one.
        handler_task.abort();
        drop(browser);
        kill_chrome_for_profile(&profile_dir);
    }
    Err(last_err.unwrap_or_else(|| anyhow!("browser setup failed")))
}

/// Recover the app page after `new_page` timed out on the missed-`"load"` race.
/// `browser.pages()` returns pages straight from their targets without gating on
/// the `is_loaded` flag, so the page that `new_page` is still (pointlessly)
/// waiting on shows up here. Match it by URL so we don't grab the launch's
/// initial `about:blank` target.
async fn recover_loaded_page(browser: &Browser, base_url: &str) -> Option<chromiumoxide::Page> {
    let pages = browser.pages().await.ok()?;
    for page in pages {
        if let Ok(Some(url)) = page.url().await {
            if url.starts_with(base_url) {
                return Some(page);
            }
        }
    }
    None
}

async fn launch_browser() -> Result<(Browser, chromiumoxide::handler::Handler, PathBuf)> {
    let browser_path = std::env::var("LORE_BROWSER").ok();
    // Per-instance profile dir. chromiumoxide's default points every
    // launched browser at `$TMPDIR/chromiumoxide-runner`, so two tests
    // (or a test + a leftover Chromium from a previous crash) race on
    // `SingletonLock` and the loser fails with
    // `Failed to create … SingletonLock: File exists`. Pinning each test
    // to its own dir avoids the lock entirely.
    let profile_dir = std::env::temp_dir().join(format!(
        "lore-e2e-chromium-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    // Headless Chromium aggressively throttles `setTimeout` in non-focused
    // contexts. `gloo-timers::future::sleep` (the web build's `platform::
    // sleep`) is just `setTimeout` under the hood, so the 2 s poll loop
    // ends up running once a minute — long enough to fail any
    // sync/refresh test inside its timeout window. These flags pin the
    // renderer at full speed for the duration of the run.
    let mut config = BrowserConfig::builder()
        .no_sandbox()
        .window_size(1280, 800)
        .user_data_dir(&profile_dir)
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
    let (browser, handler) = Browser::launch(config).await.context("launch chromium")?;
    Ok((browser, handler, profile_dir))
}

/// SIGKILL every Chromium process whose command line references this profile
/// dir (each browser gets a unique `--user-data-dir`, so this targets exactly
/// one test's browser tree). chromiumoxide's `Browser::drop` only relies on
/// tokio `kill_on_drop`, which reaps "in the background" with no timing
/// guarantee — on a single-core host the previous test's Chromium is often
/// still alive when the next test launches, starving it into `new_page`
/// hangs and WASM-mount timeouts. This makes teardown synchronous + immediate.
fn kill_chrome_for_profile(profile_dir: &std::path::Path) {
    let _ = std::process::Command::new("pkill")
        .arg("-9")
        .arg("-f")
        .arg(profile_dir.as_os_str())
        .status();
    let _ = std::fs::remove_dir_all(profile_dir);
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
            db::insert_snapshot(
                &conn,
                page_id,
                "<html></html>",
                body,
                None,
                None,
                db::ReadabilityBundle::default(),
            )?;
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
        lore_core::db::insert_snapshot(
            &conn,
            page_id,
            "<html></html>",
            plain_text,
            None,
            None,
            lore_core::db::ReadabilityBundle::default(),
        )
    }

    /// Same as `add_snapshot` but with explicit screenshot bytes. Used by
    /// the thumb/full-load lazy-fetch test where we need a real screenshot
    /// + thumb in the DB.
    pub fn add_snapshot_with_screenshots(
        &self,
        page_id: i64,
        plain_text: &str,
        full: Option<&[u8]>,
        thumb: Option<&[u8]>,
    ) -> anyhow::Result<i64> {
        let conn = self.conn()?;
        lore_core::db::insert_snapshot(
            &conn,
            page_id,
            "<html></html>",
            plain_text,
            full,
            thumb,
            lore_core::db::ReadabilityBundle::default(),
        )
    }

    /// Seed a snapshot with readability fields populated — used by tests
    /// that want to assert the Article-view path without running a real
    /// worker (no Chrome, no dom_smoothie roundtrip).
    pub fn add_snapshot_with_readability(
        &self,
        page_id: i64,
        plain_text: &str,
        readability_html: &str,
        readability_text: &str,
    ) -> anyhow::Result<i64> {
        let conn = self.conn()?;
        lore_core::db::insert_snapshot(
            &conn,
            page_id,
            "<html></html>",
            plain_text,
            None,
            None,
            lore_core::db::ReadabilityBundle {
                html: Some(readability_html),
                text: Some(readability_text),
            },
        )
    }

    /// Run the `lore-worker` binary against the test DB and wait for it
    /// to exit. Returns the exit code so tests can assert on it
    /// (0 ok, 1 failed, 2 degraded — see worker `main`).
    ///
    /// `--limit 100` is enough for any reasonable test fixture. The worker
    /// shells out to Chrome; if Chrome is unavailable in CI it falls back
    /// to HTTP automatically, which is fine for assertions on workflow
    /// shape (status transitions, snapshot creation) since we don't
    /// validate visual fidelity here.
    pub fn run_worker(&self) -> anyhow::Result<std::process::ExitStatus> {
        let bin = std::env::var("LORE_WORKER_BIN").unwrap_or_else(|_| {
            let manifest = env!("CARGO_MANIFEST_DIR");
            std::path::PathBuf::from(manifest)
                .join("../../target/debug/lore-worker")
                .canonicalize()
                .unwrap_or_else(|_| {
                    std::path::PathBuf::from(manifest).join("../../target/debug/lore-worker")
                })
                .display()
                .to_string()
        });
        Command::new(&bin)
            .args([
                "--db",
                &self.db_path.display().to_string(),
                "--limit",
                "100",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("spawn lore-worker at {}", bin))
    }
}
