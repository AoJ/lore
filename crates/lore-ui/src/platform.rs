//! Platform abstractions — same call surface across desktop and web,
//! different implementations chosen at compile time via cargo features.
//!
//! Today this hosts an async `sleep` (used by polling loops + the
//! pre-dialog delay hack). When W3b adds the web save-dialog equivalent
//! (`<a href download>`), it belongs here too.

use std::time::Duration;

#[cfg(feature = "desktop")]
pub async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}

#[cfg(feature = "web")]
pub async fn sleep(duration: Duration) {
    gloo_timers::future::sleep(duration).await;
}
