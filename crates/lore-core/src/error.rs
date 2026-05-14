//! Shared error type for the data-access surface.
//!
//! Lives in `lore-core` so both the in-process consumer (`lore-ui` /
//! `LocalBackend`) and the wire-format consumer (`lore-server` handlers
//! and the future `HttpBackend`) share one definition. The JSON
//! representation is what `lore-server` emits in response bodies and what
//! the future Dioxus/WASM client deserializes back to this type — the
//! `code` discriminator stays stable so the UI can branch on it.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Coarse-grained error category. Stable strings (`snake_case`) so the
/// frontend can pattern-match on them across server versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The HTTP route the client called doesn't exist — typo, version
    /// mismatch, or asking for an endpoint the server doesn't expose. The
    /// frontend should report this as a bug, not "your note disappeared".
    RouteNotFound,
    /// The endpoint exists and the request was well-formed, but the
    /// referenced entity (note, page, file, …) isn't in the DB. This is
    /// the right code for "the user clicked an item that has since been
    /// hard-deleted" — frontend should fall back gracefully (clear
    /// selection, refresh the list).
    NotFound,
    /// Request body / parameters failed validation: malformed JSON,
    /// missing required field, base64 decode failure, etc. Frontend
    /// should treat as a bug (it produced the request).
    InvalidInput,
    /// Anything else: DB error, IO failure, classifier panic. Frontend
    /// should show "something went wrong, please retry" — the `message`
    /// has the detail but the user shouldn't have to read it.
    Internal,
}

/// Structured backend error. Serializable for the HTTP wire format,
/// `Display` + `Error` for in-process use.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackendError {
    pub code: ErrorCode,
    pub message: String,
}

impl BackendError {
    pub fn route_not_found(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::RouteNotFound,
            message: msg.into(),
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::NotFound,
            message: msg.into(),
        }
    }

    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InvalidInput,
            message: msg.into(),
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Internal,
            message: msg.into(),
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Just the message — `Display` is what user-facing toasts pick up,
        // and the code is meaningful only for programmatic branching.
        f.write_str(&self.message)
    }
}

impl std::error::Error for BackendError {}

/// `anyhow::Error` → `BackendError`. Special-cases SQLite "no rows" to
/// `NotFound` so handlers don't have to remember to do that check
/// manually for every lookup. Everything else maps to `Internal`.
impl From<anyhow::Error> for BackendError {
    fn from(e: anyhow::Error) -> Self {
        let msg = format!("{:#}", e);
        for cause in e.chain() {
            if let Some(rs) = cause.downcast_ref::<rusqlite::Error>()
                && matches!(rs, rusqlite::Error::QueryReturnedNoRows)
            {
                return Self::not_found(msg);
            }
        }
        Self::internal(msg)
    }
}

/// Direct `rusqlite::Error` → `BackendError`. Same `QueryReturnedNoRows` →
/// `NotFound` mapping as the `anyhow::Error` path. Lets the few call sites
/// that hit rusqlite directly (e.g. raw `PRAGMA user_version` reads in
/// `db_schema_version`) use `?` without an intermediate `anyhow::Error::from`.
impl From<rusqlite::Error> for BackendError {
    fn from(e: rusqlite::Error) -> Self {
        if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
            Self::not_found(e.to_string())
        } else {
            Self::internal(e.to_string())
        }
    }
}
