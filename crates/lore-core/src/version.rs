//! Application version. Crate version comes from `Cargo.toml`; the git short
//! SHA is injected by `build.rs` at compile time (falls back to `unknown`
//! when the build doesn't happen in a git checkout).
//!
//! Mutation-tested: skipped. The values come from `env!` macros so any
//! "replace with empty string" mutant would need a test pinned to the current
//! crate version / git SHA — brittle to every version bump.

#[cfg_attr(test, mutants::skip)]
pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg_attr(test, mutants::skip)]
pub fn git_sha() -> &'static str {
    env!("LORE_GIT_SHA")
}

#[cfg_attr(test, mutants::skip)]
pub fn full() -> String {
    format!("{} ({})", crate_version(), git_sha())
}
