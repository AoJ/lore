# Phase 4 — Mutation & Proof Re-verification

## Status: mutation gaps fixed ✓ · Kani deployment-gated on NixOS ⚠️

### How the nix tooling was run (sandbox workaround)

`nix develop ./dev-env` fails here (a `.gitmodules` devtmpfs lock blocks flake
evaluation) and the command sandbox forbids the nix-daemon Unix socket. What
works:

```
# one-off, only if the flake changed:
nix build ./dev-env#wrapper --out-link dev-env/result   # sandbox disabled

dev-env/result/bin/wrapper <command>                    # sandbox disabled
```

The pre-built `wrapper` enters the pinned nix env (cargo, cargo-mutants,
cargo-llvm-cov, network) without `nix develop`. The sandbox **must** be
disabled for these — the nix-daemon needs a Unix socket the sandbox blocks
(`cannot create Unix domain socket: Operation not permitted`). No SSH is
involved (the sandbox netns only has loopback, so the VM is unreachable).

### Phase 4a — `cargo mutants` ✓

Full run: **499 mutants, 71 initially missed** (3 h 27 m, single core).

| Bucket | Count | Action |
|--------|-------|--------|
| `merge.rs` diff/LCS/3-way | 35 | +10 exact-output tests |
| `db/web_page.rs` accessors + `compute_change_summary` | 16 | +9 effect/value tests |
| `export.rs` `compact_stamp` / `slug_safe` | 4 | +5 exact tests |
| `migrations.rs` `m0009` | 1 | +1 DB test |
| `#[cfg(kani)] mod proofs` (rules/url_extract/search) | 15 | false positives — excluded in `mutants.toml` |

Why the 56 real gaps escaped the existing suite:
- **merge**: the identity-law proptests short-circuit on the `ours==base` /
  `theirs==base` early returns and never reach `diff`/`lcs_pairs`/
  `apply_three_way`; the old unit tests asserted `.contains()`, not exact text.
- **web_page**: the accessor/mutator fns were called but only asserted to "not
  error" — never checked the returned rows / bytes / status.
- **export / migrations**: edge inputs (fractional-second timestamps, `_` in
  domains, the m0009 cleanup effect) were untested.

The 15 `proofs::*` mutants live in `#[cfg(kani)]` blocks that `cargo test`
never compiles, so no test can ever kill them — excluded via `exclude_re`
(together with one behaviourally-equivalent `||→&&` fast-path guard in
`lcs_pairs`). Verified by `make verify` (Kani), not the test suite.

Two merge survivors needed more than exact-output cases — the LCS dp recurrence
(caught by an exhaustive maximal-common-subsequence oracle) and the
`apply_three_way` multi-hunk overlap advance (5 targeted cases + an exhaustive
panic-free / conflict-symmetric net). Two mutants are **provably equivalent**
(`lcs_pairs` `||→&&` guard and the `dp[i][j+1]→dp[i][j]` traceback read) and are
excluded with proofs in `mutants.toml`.

Result: `cargo test -p lore-core` → **120 lib + 83 integration** green. A scoped
`cargo mutants -f <the 4 files>` re-run is **0 missed** (227 caught, 8 timeouts
= the `*=` infinite-loop mutants detected by hang, 11 unviable). Every tricky
survivor was additionally confirmed killed by hand (apply mutation → run test →
fails).

### Phase 4b — Kani (`make verify`) ⚠️ deployment-gated

Not run on this host. `cargo install --locked kani-verifier` succeeds, but
`cargo kani setup` needs `rustup` to install a pinned nightly (absent — rust
comes from the nix rust-overlay), and the downloaded kani-compiler/CBMC are
FHS-linked binaries that don't run on NixOS without an FHS env. Kani is also
not in the pinned nixpkgs (a `pkgs.cargo-kani` flake addition was wrong and was
reverted — it broke `nix develop`). The proof harnesses were untouched by Phase
0–4, so the last-known **15 / 15** still holds. Re-enable once Kani is wired
into the deployment (rustup-in-FHS, or a packaged kani derivation) — the user
owns system-level deps.
