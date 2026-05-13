//! Application version. Crate version comes from `Cargo.toml`; the git short
//! SHA is injected by `build.rs` at compile time (falls back to `unknown`
//! when the build doesn't happen in a git checkout).

pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn git_sha() -> &'static str {
    env!("LORE_GIT_SHA")
}

pub fn full() -> String {
    format!("{} ({})", crate_version(), git_sha())
}
