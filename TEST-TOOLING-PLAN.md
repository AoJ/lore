# Test tooling rollout plan

Bring `lore` up to the testing baseline in `RUST_BEST_PRACTICE.md`. Designed to
run **unattended in the background**: a sequential foundation phase, then
independent phases that can run in parallel, each ending on a green acceptance
command and its own commit.

---

## Current baseline (already in place — do not redo)

| Tool | State |
|------|-------|
| `cargo test` | 103 inline tests + `lore-core/tests/db_test.rs` + 10 e2e files |
| `cargo-mutants` | configured (`.cargo/mutants.toml`), scoped to `lore-core`, **0 missed** |
| `kani` | 15 proofs across `rules` / `search` / `url_extract` |
| `cargo-deny` | `deny.toml` license allow-list + advisory ledger |
| `sentrux` | `.sentrux/rules.toml`, `make check-arch` |
| Nix shell + CI | pinned toolchain, CI runs inside it |

`lore-core` inline test counts: `rules` 30, `url_extract` 12, `search` 10,
`merge` 9, `export` 7, **`serde_b64` 0, `error` 0**. `lore-core` has **zero
`unsafe`**.

## Gap → what this plan adds

| Gap | Tool | Phase |
|-----|------|-------|
| No `rustfmt.toml` / `clippy.toml` / centralized lint policy | config | 0 |
| No property-based tests | `proptest` | 1 |
| `serde_b64`, `error` untested; no snapshot tests | `insta` | 2 |
| No coverage measurement | `cargo-llvm-cov` | 3 |
| New code not re-checked by mutation | `cargo-mutants` re-run | 4 |
| UB-in-unsafe checking | `miri` | — (N/A: 0 unsafe) |

---

## Execution model (for the background run)

```
Phase 0  (foundation, SEQUENTIAL — everything builds on its new baseline)
   │
   ├─ Phase 1  proptest      ┐
   ├─ Phase 2  insta         ├─ independent files → run in PARALLEL
   └─ Phase 3  coverage      ┘
                  │
               Phase 4  mutation re-verify  (needs Phase 1's new tests present)
```

**Why Phase 0 must go first and alone:** it adds the dev-deps (`proptest`,
`insta`) and `.gitignore` entries that 1–3 rely on, and resets the fmt/lint
baseline. Folding the shared `Cargo.toml` / `.gitignore` edits here means
Phases 1–3 only ever create *new* files (plus Phase 3's flake/Makefile), so
they never write the same file and can run concurrently (separate worktrees or
same tree) without conflicts.

**Per-phase contract:** each phase ends with its acceptance command green and
one commit. `make check` (clippy + fmt-check + sentrux + cargo-deny + tests,
excluding e2e) must stay green throughout — it's the global invariant.

---

## Phase 0 — Foundation: format, lint config, shared deps  [sequential, first]

**Files:** `rustfmt.toml` (new), `clippy.toml` (new), `/Cargo.toml`,
`crates/*/Cargo.toml`, `crates/lore-core/Cargo.toml`, `.gitignore`.

**Steps:**

1. **`rustfmt.toml`** (root):
   ```toml
   edition = "2024"
   max_width = 100
   use_small_heuristics = "Default"
   ```
   Then `cargo fmt --all` and commit any reflow in the same commit.

2. **`clippy.toml`** (root):
   ```toml
   cognitive-complexity-threshold = 40
   too-many-arguments-threshold = 8
   ```

3. **Centralized lint policy** — add to `/Cargo.toml`:
   ```toml
   [workspace.lints.rust]
   rust_2018_idioms = { level = "warn", priority = -1 }
   unexpected_cfgs = { level = "warn", check-cfg = ['cfg(kani)'] }  # move from lore-core

   [workspace.lints.clippy]
   # Start conservative — see RISK below. Restriction lints are added in a
   # SEPARATE follow-up cleanup, NOT in this background batch.
   ```
   Add `[lints]\nworkspace = true` to every crate's `Cargo.toml`, and remove the
   now-duplicated `[lints.rust]` block from `crates/lore-core/Cargo.toml`.

   > **RISK — read before adding any clippy restriction lint.** CI runs
   > `cargo clippy --workspace --exclude lore-e2e -- -D warnings`. `-D warnings`
   > promotes **every** `warn` lint to a hard error. So `unwrap_used`,
   > `expect_used`, `indexing_slicing`, `panic_in_result_fn` at `warn` would
   > break the build everywhere they're used. **Do not add them in this batch.**
   > Their rollout is a dedicated, reviewed cleanup task (fix sites or
   > `#[allow]` with justification), out of scope for the unattended run.
   >
   > For each lint you *do* add: add it, run the acceptance command, and if it
   > fires, either fix trivially or downgrade that one lint to `"allow"` with a
   > `# TODO(lints):` note. The phase must end green.

4. **Shared dev-deps** — add to `crates/lore-core/Cargo.toml` `[dev-dependencies]`
   (so Phases 1–2 only add test files, never touch `Cargo.toml`):
   ```toml
   proptest = "1"
   insta = { version = "1", features = ["json"] }
   ```

5. **`.gitignore`** — append:
   ```
   # insta pending snapshots
   *.pending-snap
   # coverage output
   /target/llvm-cov
   lcov.info
   ```

**Acceptance:** `make check` green (note: this needs `sentrux` + nix locally;
in CI the equivalent is the `checks` job). Minimum offline check:
`cargo fmt --all -- --check && cargo clippy --workspace --exclude lore-e2e -- -D warnings && cargo test --workspace --exclude lore-e2e`.

**Commit:** `chore: rustfmt/clippy config + workspace lints + test dev-deps`

---

## Phase 1 — Property-based tests (`proptest`)  [parallel]

**Files:** new `crates/lore-core/tests/proptests.rs` (one file; keeps `proptest!`
macros out of the unit modules). Pure functions only — no DB.

**Properties to encode** (each is a `proptest!` block):

- **`serde_b64` round-trip** (currently 0 tests — highest value):
  - for any `Vec<u8>`: `from_json(to_json(b)) == b` (the `bytes` codec)
  - for any `Option<Vec<u8>>`: round-trips equal (the `opt_bytes` codec)
  - *(drive via a tiny `#[derive(Serialize, Deserialize)]` test struct using
    `#[serde(with = "serde_b64::bytes")]`)*

- **`merge::three_way_merge`** identity laws + totality:
  - `merge(b, x, x).text == x` and `!had_conflict` (no-op when sides agree)
  - `merge(b, b, t).text == t` (only theirs changed → take theirs)
  - `merge(b, o, b).text == o` (only ours changed → take ours)
  - never panics on arbitrary `(base, ours, theirs)` strings

- **`rules::normalize_url`** idempotence + totality:
  - `normalize(parse(normalize(u))) == normalize(u)` for generated URL-ish
    strings (filter to inputs that `Url::parse` accepts)
  - never panics

- **`rules::classify`** totality: returns a non-empty `String` for any
  parseable URL + arbitrary rule set.

- **`url_extract::extract_urls`**: every returned string `Url::parse`s OK;
  never panics on arbitrary text (incl. non-ASCII, control chars).

- **`search::prepare_query`**: never panics; **covers the `format!("{w}*")`
  auto-prefix path that Kani can't reach** (per CLAUDE.md) — assert the wildcard
  is appended for a single short alphanumeric word with no FTS operators.

**Acceptance:** `cargo test -p lore-core --test proptests` green;
`cargo clippy -p lore-core --tests -- -D warnings` green.

**Commit:** `test(core): proptest round-trip + merge/url/query invariants`

---

## Phase 2 — Snapshot tests (`insta`)  [parallel]

**Files:** new `crates/lore-core/tests/snapshots.rs` + generated
`crates/lore-core/tests/snapshots/*.snap` (committed).

**Targets:**

- **`error.rs`** (currently 0 tests) — snapshot the JSON envelope for each
  constructor so the stable cross-process contract is locked:
  `route_not_found`, `not_found`, `invalid_input`, `internal` →
  `insta::assert_json_snapshot!` of the serialized `{code, message}`. Catches
  any accidental rename of an `ErrorCode` `snake_case` value.

- **`export.rs`** — snapshot exported output per `Format` variant
  (markdown / json / html) for a **fixed in-memory fixture**: open a tempfile
  DB via `db::open`, seed one space + one note + one page deterministically,
  call `export_snapshot(...)`, snapshot the `Exported` bytes as UTF-8 (or its
  hash if binary). Also snapshot `Format::mime()` for each variant (trivial,
  high-signal). *This is the heaviest item — if the fixture proves fiddly,
  ship the `error.rs` + `mime()` snapshots first and leave the full-export
  snapshot as a follow-up rather than blocking the phase.*

**Workflow note:** snapshots are generated with `cargo insta test --accept`
then reviewed; commit the `.snap` files. CI just runs `cargo test` (mismatch =
fail). Document `cargo insta review` in the Makefile/README.

**Acceptance:** `cargo test -p lore-core --test snapshots` green with committed
snapshots; no `*.pending-snap` left over.

**Commit:** `test(core): insta snapshots for error envelope + export formats`

---

## Phase 3 — Coverage (`cargo-llvm-cov`)  [parallel]

**Files:** `dev-env/flake.nix` (add `pkgs.cargo-llvm-cov` to `basePackages`),
`Makefile` (new `coverage` target + `.PHONY`), optionally `.github/workflows/ci.yml`.

**Steps:**

1. Add `cargo-llvm-cov` to the nix dev shell `basePackages`. (Rebuild the shell;
   this is the one phase that changes the environment, hence isolated.)
2. **Makefile** target, excluding the browser-driven e2e crate:
   ```makefile
   coverage:
   	cargo llvm-cov --workspace --exclude lore-e2e --html
   	@echo "Report: target/llvm-cov/html/index.html"

   coverage-lcov:
   	cargo llvm-cov --workspace --exclude lore-e2e --lcov --output-path lcov.info
   ```
   Add both to `.PHONY`.
3. *(Optional, no hard gate yet)* a CI job that runs `coverage-lcov` inside the
   nix shell and uploads `lcov.info` as an artifact. **No threshold gate
   initially** — measure first, gate later once a baseline is known.

**Acceptance:** `nix develop ./dev-env --command cargo llvm-cov --workspace --exclude lore-e2e --summary-only`
runs and prints a coverage summary (any %; we're establishing a baseline, not
gating).

**Commit:** `build: cargo-llvm-cov in dev shell + make coverage targets`

---

## Phase 4 — Mutation & proof re-verification  [after Phase 1]

Not new tooling — confirms the new tests actually killed something and nothing
regressed. On-demand (slow), not in the PR gate.

**Steps:**

1. `make mutants` — confirm still **0 missed** now that `merge.rs` / `serde_b64`
   have more coverage (both already in `examine_globs` via the glob). Log any
   new survivor and add a targeted test.
2. `make verify` — confirm the 15 Kani proofs still pass (no source drift from
   Phase 0's fmt reflow).
3. Update `CLAUDE.md`'s mutation/Kani run-count notes if the totals changed.

**Acceptance:** `make mutants` reports 0 missed; `make verify` 15/15.

**Commit:** `test(core): re-verify mutation + kani after proptest/insta` (only
if any test/source changed; otherwise no commit).

---

## Phase 5 — `miri`  [N/A]

`lore-core` has **zero `unsafe`**, so `miri` would only re-run safe code under a
slower interpreter — no UB to find. **Skip.** Revisit only if `unsafe` is later
introduced (e.g. a custom allocator anchor test or an FFI binding); the
`RUST_BEST_PRACTICE.md` §6.6 note covers how to wire it then.

---

## Risks & gotchas (carry into the background run)

1. **`-D warnings` + restriction lints** — the big one. See Phase 0 RISK. Keep
   `unwrap_used`/`expect_used`/`indexing_slicing` OUT of this batch.
2. **Duplicate dependency versions stay `warn`** — `cargo-deny`'s
   `[bans] multiple-versions = "warn"` reports duplicates (not clippy; the wide
   Dioxus + axum + chromiumoxide tree pulls ~29, expected). They're
   non-blocking and this plan leaves them as-is. Tightening to `"deny"` needs an
   explicit `skip = [...]` list of the unavoidable ones — a reviewed follow-up,
   same category as the restriction-lint rollout, **not** part of this batch.
3. **Parallel `Cargo.toml` writes** — avoided by adding both dev-deps in Phase 0.
   If running phases in isolated worktrees, still rebase Phase 0 first.
4. **fmt reflow churn** — adding `rustfmt.toml` may reformat existing files.
   Keep that diff in the Phase 0 commit alone so later phases stay readable.
5. **insta export fixture** — the only item needing a real DB fixture. Has a
   documented fallback (ship error + mime snapshots first) so it can't block.
6. **Kani after fmt** — proof harnesses are real source; if Phase 0 reflows
   them, Phase 4 must re-run `make verify`. Already covered.
7. **`edition = "2024"` in rustfmt.toml** must match the workspace edition, or
   `cargo fmt` warns. It does (workspace is 2024).

## Definition of done

```text
[ ] Phase 0: rustfmt.toml, clippy.toml, [workspace.lints], dev-deps, .gitignore; make check green
[ ] Phase 1: tests/proptests.rs — serde_b64, merge, normalize_url, classify, extract_urls, prepare_query
[ ] Phase 2: tests/snapshots.rs + committed .snap — error envelope, export formats, mime
[ ] Phase 3: cargo-llvm-cov in flake + make coverage; baseline % printed
[ ] Phase 4: make mutants 0 missed; make verify 15/15
[ ] miri: documented N/A (0 unsafe)
[ ] RUST_BEST_PRACTICE.md §13 checklist items for this repo now satisfied
```
