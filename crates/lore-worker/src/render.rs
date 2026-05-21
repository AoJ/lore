use anyhow::Result;

/// Cap on the stored full-screenshot height. Long-scroll pages (forums,
/// landing pages) can render at 20 000+ px tall — storing the whole strip
/// is mostly wasted bytes since the thumbnail and on-screen view rarely
/// show below the fold. 3000 px ≈ 2.5 viewport heights at 1280×1024,
/// which is enough context without ballooning the DB.
const MAX_FULL_HEIGHT: u32 = 3000;

/// Target thumbnail height. The thumb keeps the page's full aspect ratio
/// so the user gets a real preview of the layout (top to bottom), not just
/// the first 200 px of header / sticky ad. A typical 1280-wide page comes
/// out as roughly 85×200 — narrow, but recognisable. Will live in the
/// detail view's left column once we redo the layout side-by-side.
const THUMB_HEIGHT: u32 = 200;

/// Result of rendering a web page.
pub struct RenderedPage {
    pub html: String,
    pub plain_text: String,
    pub title: Option<String>,
    pub screenshot: Option<Vec<u8>>,
    /// Down-scaled preview of `screenshot` for cheap rendering in the
    /// detail view. `None` when no full screenshot was captured (HTTP
    /// fallback) or when thumbnail generation failed.
    pub screenshot_thumb: Option<Vec<u8>>,
    /// True when the page was fetched via the HTTP fallback after Chrome
    /// failed. The snapshot is still usable (worker stores it the same way),
    /// but the summary counts it as a warning and the user knows the
    /// rendering is degraded (no JS execution, no screenshot, often
    /// truncated content).
    pub via_fallback: bool,
}

/// Crop the captured screenshot to `MAX_FULL_HEIGHT` (keeping the top of
/// the page) and produce a height-scaled thumbnail. Returns
/// `(cropped_full_png, thumb_png)`; the thumb is `None` only if the page
/// was already shorter than `THUMB_HEIGHT` or encoding failed (the full
/// path then stands alone).
///
/// Both decode and encode steps are best-effort: any failure falls back
/// to the original `raw_png`, so a single weird page can't fail the whole
/// archive — the worker still stores the unmodified screenshot.
fn process_screenshot(raw_png: &[u8]) -> (Vec<u8>, Option<Vec<u8>>) {
    use image::ImageReader;
    use std::io::Cursor;

    let Ok(img) = ImageReader::with_format(Cursor::new(raw_png), image::ImageFormat::Png).decode()
    else {
        return (raw_png.to_vec(), None);
    };

    // Crop the bottom off if the page is taller than the cap. Image-level
    // crop (not just CSS) so the DB doesn't carry a 20 000-px strip.
    let cropped = if img.height() > MAX_FULL_HEIGHT {
        img.crop_imm(0, 0, img.width(), MAX_FULL_HEIGHT)
    } else {
        img
    };

    let full_bytes = encode_png(&cropped).unwrap_or_else(|| raw_png.to_vec());

    // Skip thumb when the source is already at-or-below the target height —
    // a no-op resize would still re-encode and grow the byte count.
    let thumb = if cropped.height() > THUMB_HEIGHT {
        let width = ((cropped.width() as u64 * THUMB_HEIGHT as u64)
            / cropped.height() as u64)
            .max(1) as u32;
        let resized = cropped.resize_exact(
            width,
            THUMB_HEIGHT,
            image::imageops::FilterType::Triangle,
        );
        encode_png(&resized)
    } else {
        None
    };

    (full_bytes, thumb)
}

fn encode_png(img: &image::DynamicImage) -> Option<Vec<u8>> {
    use std::io::Cursor;
    let mut buf = Vec::with_capacity(64 * 1024);
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .ok()?;
    Some(buf)
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
            .arg("--disable-dev-shm-usage")
            .arg("--disable-notifications")
            .arg("--disable-popup-blocking");

        if let Some(ref path) = browser_path {
            config = config.chrome_executable(path);
        }

        let config = config.build().map_err(|e| anyhow::anyhow!("{}", e))?;

        let (browser, mut handler) = Browser::launch(config).await?;

        let handle = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                let _ = event;
            }
        });

        let page = browser.new_page(url).await?;
        page.wait_for_navigation().await?;

        // Wait for page to settle (network idle approximation)
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // --- Phase 1: CSS cosmetic hiding of cookie/consent banners ---
        page.evaluate(COOKIE_BANNER_CSS_INJECT).await.ok();

        // --- Phase 2: JS auto-dismiss cookie/consent dialogs ---
        page.evaluate(COOKIE_BANNER_JS_DISMISS).await.ok();

        // Wait for dismiss animations to complete
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // --- Phase 3: Second pass dismiss (some banners appear after delay) ---
        page.evaluate(COOKIE_BANNER_JS_DISMISS).await.ok();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Scroll to top before screenshot
        page.evaluate("window.scrollTo(0, 0)").await.ok();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let html = page.content().await?;

        let title = page
            .evaluate("document.title")
            .await
            .ok()
            .and_then(|v| v.into_value::<String>().ok())
            .filter(|t| !t.is_empty());

        let plain_text = page
            .evaluate("document.body?.innerText || ''")
            .await
            .ok()
            .and_then(|v| v.into_value::<String>().ok())
            .unwrap_or_default();

        let screenshot = page
            .screenshot(
                chromiumoxide::page::ScreenshotParams::builder()
                    .full_page(true)
                    .build(),
            )
            .await
            .ok();

        drop(page);
        drop(browser);
        handle.abort();

        // Crop the full to a sane height and derive the height-scaled
        // thumbnail in one decode pass. If there's no screenshot at all
        // (rare, page.screenshot failed), both stay None.
        let (screenshot, screenshot_thumb) = match screenshot {
            Some(raw) => {
                let (cropped, thumb) = process_screenshot(&raw);
                (Some(cropped), thumb)
            }
            None => (None, None),
        };

        Ok(RenderedPage {
            html,
            plain_text,
            title,
            screenshot,
            screenshot_thumb,
            via_fallback: false,
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
            screenshot_thumb: None,
            // HTTP renderer alone doesn't mark via_fallback — only the
            // FallbackRenderer flips it when it falls back after a Chrome
            // failure, so we can distinguish "intentional HTTP-only" from
            // "degraded HTTP after Chrome blew up".
            via_fallback: false,
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
        let chrome_was_supposed_to_run = !self.chrome_failed.get() && self.local.is_some();
        if let Some(ref local) = self.local
            && !self.chrome_failed.get()
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
        // HTTP path. Mark `via_fallback` only if Chrome was supposed to run
        // and we got here because it broke — pure HTTP-only setups (no
        // Chrome at all) aren't a warning, that's just the configured mode.
        let mut page = self.http.render(url)?;
        if chrome_was_supposed_to_run {
            page.via_fallback = true;
        }
        Ok(page)
    }
}

pub fn create_renderer() -> Box<dyn Renderer> {
    Box::new(FallbackRenderer::new())
}

// ---- Cookie/Consent banner removal ----

/// CSS rules injected to cosmetically hide common cookie/consent banners.
/// Covers major consent frameworks and common class/id patterns.
const COOKIE_BANNER_CSS_INJECT: &str = r#"
(function() {
    var style = document.createElement('style');
    style.textContent = `
        /* Common cookie banner IDs */
        #cookie-banner, #cookie-consent, #cookie-notice, #cookie-bar,
        #cookie-popup, #cookie-modal, #cookie-law, #cookie-policy,
        #cookies-banner, #cookies-consent, #cookies-notice,
        #gdpr-banner, #gdpr-consent, #gdpr-notice, #gdpr-popup,
        #consent-banner, #consent-popup, #consent-modal, #consent-notice,
        #privacy-banner, #privacy-notice, #privacy-popup,
        #CybotCookiebotDialog, #CybotCookiebotDialogBodyUnderlay,
        #onetrust-consent-sdk, #onetrust-banner-sdk,
        #truste-consent-track, #truste-consent-content,
        #cookiescript_injected, #cookiescript_injected_wrapper,
        #hs-eu-cookie-confirmation, #iubenda-cs-banner,
        #cc-main, #cc_div, #cc-window,

        /* Common cookie banner classes */
        .cookie-banner, .cookie-consent, .cookie-notice, .cookie-bar,
        .cookie-popup, .cookie-modal, .cookie-overlay,
        .cookies-banner, .cookies-consent, .cookies-notice,
        .gdpr-banner, .gdpr-consent, .gdpr-notice, .gdpr-popup,
        .consent-banner, .consent-popup, .consent-modal, .consent-overlay,
        .privacy-banner, .privacy-notice, .privacy-popup,
        .cc-banner, .cc-window, .cc-dialog, .cc-overlay,
        .cookieconsent, .cookie-consent-banner,
        .js-cookie-consent, .js-cookie-banner,
        .eupopup, .eu-cookie,
        .osano-cm-window, .osano-cm-dialog,
        .qc-cmp-showing, .qc-cmp2-container,
        .fc-consent-root, .fc-dialog-container,
        .cmp-container, .cmp-modal,
        .sp_message_container,

        /* Common overlay/backdrop for consent */
        .cookie-overlay, .consent-overlay, .gdpr-overlay,
        .modal-backdrop.cookie, .cookie-backdrop,
        [class*="cookie-banner"], [class*="cookie-consent"],
        [class*="CookieConsent"], [class*="CookieBanner"],
        [id*="cookie-banner"], [id*="cookie-consent"],
        [data-testid="cookie-banner"], [data-testid="cookie-consent"],
        [aria-label*="cookie" i], [aria-label*="consent" i]
        {
            display: none !important;
            visibility: hidden !important;
            opacity: 0 !important;
            pointer-events: none !important;
            height: 0 !important;
            max-height: 0 !important;
            overflow: hidden !important;
        }

        /* Reset body overflow that consent banners often set */
        html.cookie-modal-open, body.cookie-modal-open,
        html.modal-open, body.modal-open,
        html.no-scroll, body.no-scroll,
        html.overflow-hidden, body.overflow-hidden {
            overflow: auto !important;
            position: static !important;
        }
    `;
    document.head.appendChild(style);
})();
"#;

/// JavaScript that actively tries to dismiss cookie/consent dialogs by
/// clicking common "accept", "reject", "close" buttons.
const COOKIE_BANNER_JS_DISMISS: &str = r#"
(function() {
    // Patterns for buttons that dismiss consent banners
    // Priority: reject/necessary-only > close > accept (prefer not to accept tracking)
    var rejectPatterns = [
        // English
        'reject all', 'reject', 'decline', 'deny', 'refuse',
        'only necessary', 'necessary only', 'necessary cookies only',
        'essential only', 'required only', 'required cookies only',
        'manage preferences', 'customize',
        // German
        'alle ablehnen', 'ablehnen', 'nur notwendige',
        // French
        'tout refuser', 'refuser', 'continuer sans accepter',
        // Czech
        'odmítnout vše', 'odmítnout', 'pouze nezbytné',
        // Spanish
        'rechazar todo', 'rechazar',
    ];

    var acceptPatterns = [
        // English
        'accept all', 'accept cookies', 'accept', 'agree',
        'allow all', 'allow cookies', 'i agree', 'i understand',
        'got it', 'ok', 'okay', 'continue', 'close',
        'dismiss', 'confirm',
        // German
        'alle akzeptieren', 'akzeptieren', 'zustimmen', 'verstanden',
        // French
        'tout accepter', 'accepter', "j'accepte", 'compris',
        // Czech
        'přijmout vše', 'přijmout', 'souhlasím', 'rozumím',
        // Spanish
        'aceptar todo', 'aceptar',
        // Generic
        'proceed', 'submit',
    ];

    var closePatterns = ['×', '✕', '✖', '✗', 'x', 'close'];

    function normalizeText(s) {
        return (s || '').toLowerCase().trim().replace(/\s+/g, ' ');
    }

    function tryClick(el) {
        if (!el) return false;
        var rect = el.getBoundingClientRect();
        if (rect.width === 0 && rect.height === 0) return false;
        try {
            el.click();
            return true;
        } catch(e) {
            return false;
        }
    }

    function findAndClick(patterns) {
        // Check buttons, links, and divs with role=button
        var candidates = document.querySelectorAll(
            'button, a, [role="button"], input[type="submit"], input[type="button"]'
        );
        for (var i = 0; i < candidates.length; i++) {
            var el = candidates[i];
            var text = normalizeText(el.textContent);
            var ariaLabel = normalizeText(el.getAttribute('aria-label'));
            var value = normalizeText(el.getAttribute('value'));
            var title = normalizeText(el.getAttribute('title'));

            for (var j = 0; j < patterns.length; j++) {
                var pat = patterns[j];
                if (text === pat || ariaLabel === pat || value === pat || title === pat ||
                    text.indexOf(pat) !== -1) {
                    // Verify button is inside or related to a consent context
                    var parent = el.closest(
                        '[class*="cookie"], [class*="Cookie"], [class*="consent"], [class*="Consent"], ' +
                        '[class*="gdpr"], [class*="GDPR"], [class*="privacy"], [class*="Privacy"], ' +
                        '[id*="cookie"], [id*="consent"], [id*="gdpr"], [id*="privacy"], ' +
                        '[class*="banner"], [class*="modal"], [class*="popup"], [class*="overlay"], ' +
                        '[class*="cmp"], [class*="cc-"], [class*="osano"], [class*="onetrust"], ' +
                        '[class*="sp_message"], [class*="fc-"], [class*="qc-"]'
                    );
                    if (parent && tryClick(el)) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    // Try reject/necessary-only first (privacy-preserving)
    if (findAndClick(rejectPatterns)) return;
    // Then try accept (if no reject option available)
    if (findAndClick(acceptPatterns)) return;
    // Last resort: try close buttons
    findAndClick(closePatterns);
})();
"#;
