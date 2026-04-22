use anyhow::Result;

/// Result of rendering a web page.
pub struct RenderedPage {
    pub html: String,
    pub plain_text: String,
    pub title: Option<String>,
    pub screenshot: Option<Vec<u8>>,
}

/// Renderer backend trait. Implementations can be local (headless Chrome)
/// or remote (API call to isolated rendering service).
pub trait Renderer {
    fn render(&self, url: &str) -> Result<RenderedPage>;
}

/// Local renderer using headless Chrome via chromiumoxide.
pub struct LocalRenderer {
    runtime: tokio::runtime::Runtime,
}

impl LocalRenderer {
    pub fn new() -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        Ok(Self { runtime })
    }

    async fn render_async(url: &str) -> Result<RenderedPage> {
        use chromiumoxide::browser::{Browser, BrowserConfig};
        use futures_util::StreamExt;

        let browser_path = std::env::var("LORE_BROWSER").ok();

        let mut config = BrowserConfig::builder()
            .no_sandbox()
            .window_size(1280, 1024)
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage");

        if let Some(ref path) = browser_path {
            config = config.chrome_executable(path);
        }

        let config = config.build().map_err(|e| anyhow::anyhow!("{}", e))?;

        let (browser, mut handler) = Browser::launch(config).await?;

        // Spawn handler in background
        let handle = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                let _ = event;
            }
        });

        let page = browser.new_page(url).await?;
        page.wait_for_navigation().await?;

        // Wait a bit for JS to settle
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let html = page.content().await?;

        // Extract title
        let title = page
            .evaluate("document.title")
            .await
            .ok()
            .and_then(|v| v.into_value::<String>().ok())
            .filter(|t| !t.is_empty());

        // Extract plain text from body
        let plain_text = page
            .evaluate("document.body?.innerText || ''")
            .await
            .ok()
            .and_then(|v| v.into_value::<String>().ok())
            .unwrap_or_default();

        // Screenshot
        let screenshot = page
            .screenshot(
                chromiumoxide::page::ScreenshotParams::builder()
                    .full_page(true)
                    .build(),
            )
            .await
            .ok();

        // Clean up
        drop(page);
        drop(browser);
        handle.abort();

        Ok(RenderedPage {
            html,
            plain_text,
            title,
            screenshot,
        })
    }
}

impl Renderer for LocalRenderer {
    fn render(&self, url: &str) -> Result<RenderedPage> {
        let url = url.to_string();
        self.runtime.block_on(Self::render_async(&url))
    }
}

/// Simple HTTP fallback renderer (no JS rendering, no screenshots).
/// Used when headless Chrome is not available.
pub struct HttpRenderer;

impl Renderer for HttpRenderer {
    fn render(&self, url: &str) -> Result<RenderedPage> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("lore/0.1 (personal knowledge archive)")
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()?;

        let response = client.get(url).send()?;
        if !response.status().is_success() {
            anyhow::bail!("HTTP {}", response.status());
        }

        let html = response.text()?;
        let (title, plain_text) = extract_from_html(&html);

        Ok(RenderedPage {
            html,
            plain_text,
            title,
            screenshot: None,
        })
    }
}

fn extract_from_html(html: &str) -> (Option<String>, String) {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);

    let title = Selector::parse("title")
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|t| !t.is_empty());

    // Try semantic elements first
    let main_selectors = [
        "article",
        "main",
        "[role=main]",
        ".post-content",
        ".entry-content",
    ];
    for sel_str in &main_selectors {
        if let Ok(sel) = Selector::parse(sel_str) {
            let elements: Vec<_> = document.select(&sel).collect();
            if !elements.is_empty() {
                let text: String = elements
                    .iter()
                    .map(|el| collapse_whitespace(&el.text().collect::<Vec<_>>().join(" ")))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if text.len() > 100 {
                    return (title, text);
                }
            }
        }
    }

    // Fallback: body text
    if let Ok(body_sel) = Selector::parse("body")
        && let Some(body) = document.select(&body_sel).next()
    {
        let text = collapse_whitespace(&body.text().collect::<Vec<_>>().join(" "));
        return (title, text);
    }

    let text = collapse_whitespace(&document.root_element().text().collect::<Vec<_>>().join(" "));
    (title, text)
}

fn collapse_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut last_was_space = true;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(ch);
            last_was_space = false;
        }
    }
    result.trim().to_string()
}

/// Renderer that tries headless Chrome first, falls back to HTTP.
pub struct FallbackRenderer {
    local: Option<LocalRenderer>,
    http: HttpRenderer,
    chrome_failed: std::cell::Cell<bool>,
}

impl Default for FallbackRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl FallbackRenderer {
    pub fn new() -> Self {
        let local = LocalRenderer::new().ok();
        if local.is_none() {
            eprintln!("Note: headless Chrome not available, using HTTP fetch");
        }
        Self {
            local,
            http: HttpRenderer,
            chrome_failed: std::cell::Cell::new(false),
        }
    }
}

impl Renderer for FallbackRenderer {
    fn render(&self, url: &str) -> Result<RenderedPage> {
        // Try Chrome if available and hasn't permanently failed
        if !self.chrome_failed.get()
            && let Some(ref local) = self.local
        {
            match local.render(url) {
                Ok(page) => return Ok(page),
                Err(e) => {
                    eprintln!(
                        "Chrome render failed ({}), falling back to HTTP for all remaining",
                        e
                    );
                    self.chrome_failed.set(true);
                }
            }
        }
        self.http.render(url)
    }
}

pub fn create_renderer() -> Box<dyn Renderer> {
    Box::new(FallbackRenderer::new())
}
