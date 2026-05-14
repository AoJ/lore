pub mod db;
pub mod error;
pub mod rules;
pub mod serde_b64;
pub mod url_extract;
pub mod version;

// Native-only modules (rusqlite-bound from top to bottom). The `sqlite`
// feature is on by default for desktop/server/worker/cli builds; WASM
// consumers (`HttpBackend` in `lore-ui::backend`) compile with
// `--no-default-features` and only get the types from `db::*`.
#[cfg(feature = "sqlite")]
pub mod migrations;
#[cfg(feature = "sqlite")]
pub mod search;
