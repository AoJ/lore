# Rust quality, performance & correctness — playbook

A portable playbook for keeping quality, correctness, and performance under
control in a serious Rust project. Distilled from real projects (the running
concrete example is the `lore` workspace this file ships in) and written to be
copied into a new repo as a starting template.

**How to read it.** Every section is *baseline → how to tighten*. Not all of it
applies to every project — a CLI tool, a multi-crate desktop app, and a
numeric hot-path library want different subsets. Sections are tagged:

- **[core]** — applies to essentially every Rust project. Adopt by default.
- **[workspace]** — matters once you have more than one crate.
- **[stateful]** — for anything with a persistent store (DB, on-disk format).
- **[hot-path]** — numeric / throughput-critical code only. Skip otherwise.

The fastest way to use this is the checklist in §13, then come back for the
rationale on anything unfamiliar.

---

## 1. Project structure & enforced boundaries — [workspace]

Architecture decays unless something *fails the build* when it's violated.
Three layers of enforcement, cheapest first:

**1. Workspace shape.** One pure library crate that everyone shares; binaries
are peers that never depend on each other. In `lore`:

```
crates/
├── lore-core/    # pure: DB, rules, search. No network, no UI framework. WASM-ready.
├── lore-cli/     # binary
├── lore-ui/      # binary (Dioxus desktop)
├── lore-server/  # binary (axum)
└── lore-worker/  # binary (headless browser)
```

The binaries communicate only through a shared store, never by importing each
other. Keeping the shared crate *pure* (no network, no GUI framework, no
platform deps) is what lets it compile for WASM and lets it be the only crate
under heavy unit test / mutation / formal verification.

**2. Shared metadata via the workspace table.** Bump the version once:

```toml
# /Cargo.toml
[workspace]
resolver = "3"
members = ["crates/lore-core", "crates/lore-cli", "..."]

[workspace.package]
version = "0.2.0"
edition = "2024"
license = "MIT"
```

Each crate then writes `version.workspace = true`, `edition.workspace = true`.
A release bump is a one-line change instead of N.

**3. A boundary linter that fails CI.** `cargo-deny`'s `bans` table catches
*dependency-graph* violations; a dedicated architecture linter catches
*intent* the graph can't express. `lore` uses `sentrux` (`.sentrux/rules.toml`,
run via `make check-arch`): it declares layers (`core` order 0, `binary`
order 1) and explicit `[[boundaries]]` forbidding e.g. `ui → server`. It also
gates per-function complexity (`max_cc = 25`) and length (`max_fn_lines = 200`).

> If you can't add a bespoke linter, you still get 80% from `cargo-deny`
> `bans.deny = [{ name = "..." }]` (forbid a crate appearing anywhere) and from
> splitting the pure logic into its own crate so the compiler enforces that it
> can't reach the binaries' deps.

---

## 2. Reproducible toolchain — [core]

The single biggest source of "works on my machine" in Rust is not the compiler
— it's the *system* deps (OpenSSL, GTK/WebKit, a matching `wasm-bindgen` CLI, a
browser for e2e) and tool versions (`dx`, `cargo-deny`). Pin them.

**Minimum (every project):** commit a `rust-toolchain.toml` so `rustc`/`clippy`
are identical everywhere:

```toml
[toolchain]
channel = "1.86.0"          # or "stable" if you track latest
components = ["rustfmt", "clippy", "rust-src"]
targets = ["wasm32-unknown-unknown"]   # only the extra targets you build
```

**Stronger (recommended once system deps appear): a Nix flake dev shell.**
`lore` pins the *entire* environment — rustc + extra targets, `dx`,
`wasm-bindgen-cli` (version-matched to the crate), node, GTK/WebKit, a patched
Chromium for e2e, cross toolchains — in `dev-env/flake.nix` + `flake.lock`.
**CI then runs inside the same shell**, so "passes in CI" and "passes locally"
mean the same toolchain:

```yaml
- run: nix develop ./dev-env --command cargo clippy --workspace -- -D warnings
```

Two patterns from `lore` worth copying:

- **Split heavy shells.** Keep the default shell lean; put rarely-needed,
  build-from-source toolchains (cross-gcc) in a second shell
  (`nix develop ./dev-env#cross`). Entering the common shell stays fast.
- **Automate version lockstep.** Some tools demand an exact version match with
  a crate (`dx` ↔ the `wasm-bindgen` crate). Pinning that by hand in the flake
  on every `cargo update` is error-prone. `lore` has a script
  (`make update-deps`) that reads the version from `Cargo.lock` and rewrites
  the flake's pinned version + both fixed-output hashes automatically. **If a
  manual cross-file sync exists, script it** — it will drift otherwise.

---

## 3. Formatting — rustfmt — [core]

**Config (`rustfmt.toml`):**
```toml
edition = "2024"
max_width = 100
use_small_heuristics = "Default"
```

**Makefile:**
```makefile
fmt:
	cargo fmt --all
fmt-check:
	cargo fmt --all -- --check
```

`fmt` applies in place; `fmt-check` is read-only and fails CI. (`lore`'s CI
runs `fmt` on push and auto-commits the result — see §11 — which is an
alternative to failing the build.)

**How to tighten:**
- `imports_granularity = "Crate"` — collapse imports from the same crate
- `group_imports = "StdExternalCrate"` — std / external / local sections
- `wrap_comments = true`, `comment_width = 80` — wrap doc comments
- `format_code_in_doc_comments = true` — run rustfmt inside `///` blocks
- `error_on_unformatted = true` — fail on bug-prone constructs
- `unstable_features = true` + nightly — unlocks the four above

---

## 4. Linting — clippy — [core]

**Invocation:**
```bash
cargo clippy --workspace -- -D warnings
cargo clippy --workspace --all-features -- -D warnings
cargo clippy --no-default-features -- -D warnings   # feature-gated code
```

Run across feature combinations — `#[cfg(feature = "x")]` code is otherwise
never linted. In `lore`, `lore-core` builds WASM with `--no-default-features`
(drops the `sqlite` feature), so the no-features lint pass is not optional.

**Centralize lint policy in the workspace table (edition 2021+).** Prefer this
over scattering `#![deny(...)]` across every crate root — one source of truth,
inherited via `lints.workspace = true`:

```toml
# /Cargo.toml
[workspace.lints.rust]
unsafe_op_in_unsafe_fn = "deny"
rust_2018_idioms = { level = "deny", priority = -1 }
missing_debug_implementations = "warn"

[workspace.lints.clippy]
unwrap_used = "warn"           # .unwrap() is a smell
expect_used = "warn"
panic_in_result_fn = "warn"
indexing_slicing = "warn"      # prefer .get(i) over a[i]
dbg_macro = "warn"
todo = "warn"
print_stderr = "warn"          # forces a logger instead of eprintln
```

```toml
# each crate's Cargo.toml
[lints]
workspace = true
```

(`lore` currently only uses `[lints.rust]` to silence the `cfg(kani)` lint; the
block above is the recommended target state for the template.)

**`clippy.toml` thresholds** (raise only when justified):
```toml
cognitive-complexity-threshold = 40
too-many-arguments-threshold = 8
```

**Tightening gradient:**

| Level | Add | What you buy |
|-------|-----|--------------|
| Default + warnings | `-D warnings` | safe baseline |
| + pedantic | `-W clippy::pedantic` | API sanity, `must_use`, naming |
| + nursery | `-W clippy::nursery` | WIP lints, often useful |
| + cargo | `-W clippy::cargo` | `Cargo.toml` lints (multiple-versions, wildcards) |
| Selective deny | the `[workspace.lints]` block above | enforce key properties |

---

## 5. Dependencies — cargo-deny — [core]

**`deny.toml`:**
```toml
[advisories]
version = 2
yanked = "warn"
# Ignore advisories you've consciously accepted (transitive, can't upgrade
# alone). ALWAYS leave a comment naming why and the upgrade trigger.
ignore = [
  # "RUSTSEC-2024-0429", # glib unsoundness via GTK3 — revisit when off GTK3
]

[licenses]
version = 2
allow = [
  "MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause", "BSD-3-Clause", "ISC", "MPL-2.0",
  "Unicode-3.0", "CC0-1.0", "Zlib", "BSL-1.0",
]
confidence-threshold = 0.8

[bans]
multiple-versions = "warn"
wildcards = "warn"
allow-wildcard-paths = true     # workspace-local `path = ` deps look like wildcards

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

The license allow-list is the high-value part: it keeps GPL/AGPL/LGPL/SSPL out
of the tree mechanically. Scope `[graph].targets` to the platforms you actually
ship so you don't audit deps for targets you never build.

**The `ignore` list is a ledger, not a dumping ground.** Every entry needs a
comment with the reason *and* the condition under which it goes away (`lore`
groups its GTK3-stack advisories with "revisit when Dioxus moves off GTK3").
When you accept a new advisory, that's a one-line decision you should be able
to defend.

**How to tighten:**
- `bans.multiple-versions = "deny"` + explicit `skip = [...]` — keeps duplicate
  inventory honest (only practical once the tree is small enough)
- `bans.deny = [{ name = "openssl" }]` — block a crate (e.g. force `rustls`)
- `confidence-threshold = 0.93` — stricter SPDX detection

**Complementary:** `cargo-audit` (advisories only, faster), `cargo-machete` /
`cargo-udeps` (unused deps), `cargo-bloat`, `cargo-msrv`.

---

## 6. Tests — the layered pyramid — [core]

| Layer | Tool / mechanism | What you buy | Tag |
|-------|------------------|--------------|-----|
| Unit + integration | `cargo test` | API correctness | core |
| Boundary pairs | hand-written at-edge tests | kills off-by-one mutants | core |
| Property-based | `proptest` / `quickcheck` | invariants over random inputs | core |
| Snapshot | `insta` | structured output drift | core |
| Mutation | `cargo-mutants` | tests that actually assert | core |
| Formal | `kani` (BMC) | panic-freedom, UB, OOB on every path | core |
| UB in unsafe | `miri` | dangling/misaligned/OOB in `unsafe` | core |
| End-to-end | real binary + driver | the whole stack, as shipped | workspace |
| Web/HTTP | `tower::ServiceExt::oneshot` | routers without a TCP socket | workspace |
| Hot-path anchors | counters / custom allocator / `perf-event` | exact alloc & RNG counts | hot-path |
| Benchmarks | `criterion`, `iai-callgrind` | regression gates | hot-path |

Put the bulk of unit/property/mutation/formal effort on the **pure** crate
(`lore-core`) — it's deterministic, dependency-light, and fast to compile, so
the heavy tools stay cheap. `lore` scopes both `cargo-mutants` and Kani to
`lore-core` for exactly this reason.

### 6.1 Boundary tests + mutation testing

For every comparison operator (`>`, `>=`, `==`) write a *pair* — at-edge accept
and at-edge-plus-one reject. This is what kills `>=` → `>` mutants that survive
even at 100% line coverage:

```rust
#[test]
fn at_capacity_is_accepted() {
    assert!(validate(i16::MAX as i64).is_ok());
}

#[test]
fn one_over_capacity_is_rejected() {
    let err = validate(i16::MAX as i64 + 1).unwrap_err();
    assert!(err.to_string().contains("exceeds i16"));
}
```

`cargo-mutants` then *measures* whether your tests are real. It mutates the
source (flip comparisons/booleans, swap arithmetic, replace return values,
delete `!`) and reports any mutant the suite failed to catch. Run it scoped and
configured (`.cargo/mutants.toml`):

```toml
examine_globs = ["crates/lore-core/src/**/*.rs"]
exclude_globs = ["**/migrations/*.sql", "crates/lore-core/build.rs"]
test_package  = ["lore-core"]   # don't compile the UI for every mutant
test_workspace = false
timeout_multiplier = 5.0        # kill hangs at 5× baseline
```

It's slow (minutes to tens of minutes) — keep it out of the per-PR gate and run
it before a release or on a nightly cron. Target state is **0 missed**
(`lore-core`: 0 missed / 234 viable). Log survivors so the next wave starts
with them.

### 6.2 Property-based tests — `proptest`

Best for invariants that should hold over *any* input: round-trips
(`parse(render(x)) == x`), determinism (same seed ⇒ same output), or "never
panics on arbitrary bytes". Pure parser/classifier functions (URL extraction,
query preparation, classification rules) are the ideal target.

### 6.3 Formal verification — `kani` — [core]

For pure functions, bounded model checking proves properties `cargo test` can
only sample: **panic-freedom, integer overflow/UB, slice OOB, and pointer
soundness on every reachable path**. Harnesses live behind `#[cfg(kani)]` so
they're invisible to `cargo build`/`cargo test`:

```rust
#[cfg(kani)]
mod proofs {
    #[kani::proof]
    fn extract_urls_never_panics() {
        let input = "...";           // see note on input shape below
        let _ = super::extract_urls(input);
    }
}
```

**Scope lesson (from `lore`):** feeding a fully symbolic `&str` stalls CBMC on
the char-boundary loops inside `str::trim`/`to_lowercase`/`find`/`parse`
(`run_utf8_validation` blows up). The workable pattern is **fixed/concrete
inputs, full-body symbolic execution** — Kani still discharges hundreds to
thousands of checks per harness across every branch reachable from that input.
`lore` verifies `rules`, `search::prepare_query`, and `url_extract`
(15/15 proofs, ≈25 s). Silence the `unexpected_cfgs` lint for `cfg(kani)` in
`Cargo.toml`. Needs `cargo install --locked kani-verifier && cargo kani setup`;
keep it on-demand, not in the PR gate.

### 6.4 Snapshot tests — `insta`

For structured output (JSON, schemas, rendered markup, plans). `insta` stores
the expected value and shows a reviewable diff on change — far better than
hand-rolled string asserts that rot. `cargo insta review` to accept drift.

### 6.5 End-to-end tests — the real binary — [workspace]

For an app with a server + frontend, unit tests can't catch wiring bugs.
`lore`'s `lore-e2e` crate spawns the **real** `lore-serve` binary as a
subprocess (random port, temp DB) and drives the WASM frontend in headless
Chromium via `chromiumoxide`. Each `TestApp::spawn()` is fully isolated (own
port + DB, killed on `Drop`), so tests run in parallel with no shared state.
Keep e2e **out** of the fast pre-PR gate (it needs a built bundle + a browser);
give it its own `make e2e`.

> Lighter alternative for HTTP-only crates: `tower::ServiceExt::oneshot` drives
> an axum/tower router in-process with no socket — fast enough for the normal
> test run.

### 6.6 Hot-path anchor tests — [hot-path]

*Skip this section unless you have throughput-critical numeric code.* Benchmarks
tell you something got slower; **anchor tests fail CI when it does**, by
asserting an *exact* resource count.

**RNG / call-count anchor** (feature-gated, zero overhead when off):
```rust
#[cfg(feature = "counters")]
thread_local! { static RNG_CALLS: std::cell::Cell<u64> = std::cell::Cell::new(0); }

#[inline(always)]
pub fn bump_rng() {
    #[cfg(feature = "counters")]
    RNG_CALLS.with(|c| c.set(c.get() + 1));
}

#[test]
#[cfg(feature = "counters")]
fn one_rng_call_per_row() {
    reset_rng_calls();
    let _ = generate_array(10_000);
    assert_eq!(rng_calls(), 10_000);
}
```

**Allocation anchor** — a custom `#[global_allocator]` in a dedicated
integration-test binary (each integration test file is its own binary, so the
counter doesn't leak into other tests):
```rust
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 { ALLOCS.fetch_add(1, Relaxed); System.alloc(l) }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { System.dealloc(p, l) }
}
#[global_allocator] static GLOBAL: Counting = Counting;

#[test]
fn batch_is_O1_allocs() {
    ALLOCS.store(0, Relaxed);
    let _ = generate_array(10_000);
    assert!(ALLOCS.load(Relaxed) < 200, "expected O(1) allocs/batch");
}
```

**Hot-path rules:** (1) zero per-row heap alloc; (2) exact RNG count per row;
(3) no dynamic dispatch — `enum` over `Box<dyn Trait>`; (4) pre-allocate and
reuse via `.clear()`. **`miri`** (`cargo +nightly miri test`) catches the UB
these patterns risk in `unsafe` blocks.

---

## 7. Persistent state: migrations + a version gate — [stateful]

Any app with an on-disk store needs a forward-only migration runner *and* a
guard against opening a store written by a newer build. `lore` keeps a
`PRAGMA user_version` and an `EXPECTED_DB_SCHEMA_VERSION` constant at the crate
root; on startup it runs pending migrations and **refuses to start on a
newer-than-known DB** (surfacing a "DB is from a newer build" banner instead of
crashing on a missing column).

Patterns worth copying:
- **One bootstrap entrypoint** that runs migrations + seeds, called once at
  startup; a separate cheap `open_existing` for per-request/per-poll opens that
  does *not* re-run the migration check. (Routing every open through the
  migration runner is both slow and hides real bootstrap failures.)
- The version constant lives where **every** target can read it (including WASM
  clients that learn it over the API), not buried in the SQL module.
- A CLI subcommand to inspect/apply (`db-version`, `migrate`) without booting
  the GUI.

---

## 8. Benchmarking — [hot-path]

| Tool | Measures | Stability | When |
|------|----------|-----------|------|
| `iai-callgrind` | instructions, cache, branches | very high (instrumentation) | CI regression gate |
| `criterion` | wall-clock, throughput | medium | local comparisons |
| `perf stat` | global HW counters | low without isolation | ad-hoc |
| `perf record` + `samply` | flame graph | low | bottleneck hunting |
| `valgrind --tool=callgrind` | call graph + cycles | high but slow | deep dives |

**Guard external-tool targets in the Makefile** so they fail fast with a
concrete message instead of a cryptic underlying error:

```makefile
bench-iai:
	@[ "$$(uname -s)" = "Linux" ] || { echo "requires Linux (valgrind)"; exit 1; }
	@command -v valgrind >/dev/null || { echo "missing 'valgrind'"; exit 1; }
	@command -v iai-callgrind-runner >/dev/null || { echo "cargo install iai-callgrind-runner"; exit 1; }
	cargo bench --bench iai_callgrind_bench
```

For instruction-stable CI gating use `iai-callgrind`; for human-facing
wall-clock comparison use `criterion`. They're complementary — the first misses
cache/IO effects, the second is noisy.

---

## 9. CI — nix where it works, native matrix where it doesn't — [core]

Two principles: **run the common checks inside the pinned environment** (so CI
and local agree), and **build OS-specific artifacts natively** (cross-compiling
a GTK/WebKit desktop app isn't worth it).

`lore`'s split:
- **`ci.yml`** (push/PR): a `fmt` job (rustup, auto-commits `cargo fmt`), a
  `checks` job running clippy + test + `cargo deny` *inside the nix shell*, and
  a `web` job building the WASM bundle with the pinned `dx`/`wasm-bindgen`.
- **`release.yml`** (on `v*` tags): web bundle via nix; Linux (x86_64 +
  aarch64), Windows, and macOS binaries on native matrix runners.
- **`deps-update.yml`** (weekly cron): `cargo update` + `nix flake update` +
  the wasm-bindgen resync, opened as a **PR** that `ci.yml` tests, auto-merging
  when green.

A minimal single-crate version of the quality gate:

```yaml
name: CI
on: { push: { branches: [main] }, pull_request: {} }
env: { CARGO_TERM_COLOR: always, RUST_BACKTRACE: 1 }
jobs:
  quality:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with: { components: rustfmt, clippy }
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo clippy --workspace --all-features -- -D warnings
      - uses: EmbarkStudios/cargo-deny-action@v2
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --workspace
```

**Gotchas learned the hard way:**
- **A scheduled-job PR opened by the default `GITHUB_TOKEN` does not trigger
  other workflows** (anti-recursion). The auto-update PR's checks will never
  run. Use a fine-grained PAT (scoped to the one repo, contents + pull-requests
  read/write) stored as a secret, and enable "Allow auto-merge" + a
  branch-protection required-check rule.
- **Keep action versions current** (`actions/checkout@v5`) — old majors pin a
  deprecated Node runtime and CI nags.
- Pin `Swatinem/rust-cache@v2` for a shared target cache; set `RUST_BACKTRACE: 1`.
- An architecture linter that isn't packaged for your CI environment (e.g.
  `sentrux` isn't in nixpkgs) stays a **local** pre-PR gate — document it.

**How to tighten:** matrix across feature flags; a `--release` build job
(release can hide optimization regressions); `cargo-mutants` on a nightly cron;
coverage via `cargo-llvm-cov` + a Codecov gate; a
`cargo doc --no-deps -D rustdoc::broken-intra-doc-links` job.

---

## 10. Design rules worth porting

Tooling catches regressions; design choices prevent whole bug classes.

1. **Feature flags for build targets and hard limits, not runtime config.** A
   compile-time feature (`sqlite`, `desktop`/`web`, `demo`) can't be flipped at
   runtime. `lore` drops all SQL from WASM builds via
   `--no-default-features`; the type surface stays, the implementation is
   `#[cfg]`-gated out. Hard caps belong in code under `#[cfg]`, not in a config
   file an operator can edit.
2. **A stable error envelope across process boundaries.** `lore` serializes one
   `BackendError { code, message }` for *every* server response — handler
   error, route fallback, JSON-rejection — with a small fixed set of
   `snake_case` codes the frontend branches on (`not_found` vs `invalid_input`
   vs `internal`). The message is for humans; the code is for control flow.
3. **Newtypes for units.** `struct Bytes(u64)`, `struct Rows(u64)`. Mixing them
   becomes a compile error, not a 1024× bug.
4. **`#[non_exhaustive]` on public enums/structs.** Add variants/fields later
   without breaking downstream `match`/struct-literal code.
5. **Strict config deserialization.** `#[serde(deny_unknown_fields)]` on every
   config struct — a typo in YAML/TOML is a hard error, not silent neglect.
   ```rust
   #[derive(serde::Deserialize)]
   #[serde(deny_unknown_fields)]
   struct Config { name: String, count: u64 }
   ```
6. **Determinism as a test.** Same input ⇒ same bytes out. Hash-checked golden
   snapshots catch drift the moment it appears.
7. **Plan-time validation + a runtime backstop.** Validate before doing the
   work *and* keep a runtime `assert!` as a net for silent integer
   wraparound / capacity overflow.
8. **Single source of truth for shared data.** In `lore`'s UI, views never
   touch the DB directly — one store owns the cache and every mutation. The
   same instinct applies to config, schema versions, and user-visible strings:
   one place each.

---

## 11. The single pre-PR gate

Wire everything fast into one target so "ready for PR" is one command. Keep the
slow tools (e2e, mutants, kani, benches) *out* of it, with their own targets:

```makefile
check: lint check-arch audit            # fmt-check + clippy, boundaries, cargo-deny
	cargo test --workspace --exclude lore-e2e

e2e:      ## browser + built bundle — slow
mutants:  ## mutation waves — minutes
verify:   ## kani proofs — minutes
```

`lore`'s `make check` = clippy + fmt + `sentrux check` + `cargo deny` + the
workspace test run (excluding the browser-driven e2e crate). If it's green, the
PR will be too.

---

## 12. Quick checklist for a new Rust project

```text
STRUCTURE
[ ] pure core crate; binaries are peers, communicate via store not imports   [ws]
[ ] [workspace.package] for shared version/edition/license                    [ws]
[ ] boundary linter (sentrux) or at least cargo-deny bans + crate split       [ws]

TOOLCHAIN
[ ] rust-toolchain.toml pinned (channel + components + extra targets)
[ ] Nix flake dev shell once system deps appear; CI runs inside it
[ ] any cross-file version lockstep is scripted, not hand-maintained

LINT & FORMAT
[ ] rustfmt.toml + fmt-check (or auto-commit) in CI
[ ] clippy -D warnings across every feature combination (incl. --no-default-features)
[ ] lint policy centralized in [workspace.lints]
[ ] cargo-deny: license allow-list + advisory ignore-ledger (commented)

TESTS
[ ] unit + integration on the pure core
[ ] boundary pairs for every comparison operator
[ ] proptest for parsers/classifiers/round-trips
[ ] insta snapshots for structured output
[ ] cargo-mutants config, scoped to the pure crate; target 0 missed
[ ] kani proofs for pure functions (fixed input, full-body symbolic)
[ ] miri locally for every unsafe block
[ ] e2e against the real binary, isolated per test, own make target

STATEFUL APPS
[ ] forward-only migrations + version gate (refuse newer-than-known store)
[ ] one bootstrap open (migrate+seed); cheap open_existing for hot opens

HOT-PATH ONLY
[ ] alloc + RNG anchor tests (exact counts, feature-gated)
[ ] iai-callgrind regression gate; criterion for local comparison

CI
[ ] checks inside the pinned env; OS artifacts native; Swatinem cache
[ ] scheduled deps-update PR via a real PAT (not GITHUB_TOKEN) + auto-merge
[ ] current action versions

DESIGN
[ ] feature-gated build targets & hard limits, not runtime config
[ ] stable error envelope (code + message) across process boundaries
[ ] newtypes for units; #[non_exhaustive] on public types
[ ] #[serde(deny_unknown_fields)] on every config struct

GATE
[ ] one fast `make check`; slow tools (e2e/mutants/kani/bench) on their own targets
```
