pub mod db;
pub mod error;
pub mod merge;
pub mod rules;
pub mod serde_b64;
pub mod url_extract;
pub mod version;

/// On-disk schema version this build of `lore-core` knows how to operate on.
/// The desktop client compares this against `PRAGMA user_version` on every
/// poll tick so it can surface a "DB is from a newer build" banner instead
/// of crashing on a missing column. Lives at the crate root (not inside
/// `migrations`) so it's reachable from WASM builds too — the web client
/// reads the value from the server via `db_schema_version` and compares.
pub const EXPECTED_DB_SCHEMA_VERSION: u32 = 8;

// Native-only modules (rusqlite-bound from top to bottom). The `sqlite`
// feature is on by default for desktop/server/worker/cli builds; WASM
// consumers (`HttpBackend` in `lore-ui::backend`) compile with
// `--no-default-features` and only get the types from `db::*`.
#[cfg(feature = "sqlite")]
pub mod migrations;
#[cfg(feature = "sqlite")]
pub mod search;
