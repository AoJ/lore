#!/usr/bin/env bash
# Resync the pinned wasm-bindgen-cli in flake.nix to the `wasm-bindgen` crate
# version in ../Cargo.lock. `dx` requires the CLI version to match the crate
# exactly, and the hashes are fixed-output (so version-specific) — this script
# is the "don't hand-edit three values in lockstep" button.
#
# What it does:
#   1. read the wasm-bindgen version from Cargo.lock
#   2. write it into flake.nix (wasmBindgenVersion)
#   3. src hash via `nix-prefetch-url --unpack` (matches fetchCrate; no build)
#   4. vendor hash by realizing the cargo-deps FOD alone and scraping the
#      "got: sha256-..." mismatch (no prefetch URL exists for vendored deps,
#      and building just the FOD avoids compiling the whole CLI)
#
# Builds via `path:` so it sees the working-tree flake.nix without `git add`.
# Linux/macOS only (no wasm-bindgen-cli build target on other hosts).
set -euo pipefail

cd "$(dirname "$0")"
FLAKE="flake.nix"
LOCK="../Cargo.lock"
FAKE="sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
NIX="nix --extra-experimental-features nix-command --extra-experimental-features flakes"

[ -f "$LOCK" ] || { echo "error: $LOCK not found (run from repo)"; exit 1; }

# 1. version of the `wasm-bindgen` crate (exact name match within its block).
ver=$(awk '
  /^name = / { name=$3; gsub(/"/,"",name) }
  /^version = / { v=$3; gsub(/"/,"",v); if (name=="wasm-bindgen") { print v; exit } }
' "$LOCK")
[ -n "$ver" ] || { echo "error: wasm-bindgen not found in $LOCK"; exit 1; }
echo "Cargo.lock wants wasm-bindgen $ver"

cur=$(sed -nE 's/^\s*wasmBindgenVersion = "([^"]*)";/\1/p' "$FLAKE")
echo "flake currently pins $cur"

patch_field() { # $1 = binding name, $2 = value
  sed -i -E "s|^(\s*$1 = \")[^\"]*(\";)|\1$2\2|" "$FLAKE"
}

# 2. version.
patch_field wasmBindgenVersion "$ver"

# 3. src hash — prefetch the crates.io tarball (unpacked NAR hash == fetchCrate).
url="https://crates.io/api/v1/crates/wasm-bindgen-cli/$ver/download"
b32=$(nix-prefetch-url --type sha256 --unpack "$url" 2>/dev/null | tail -1)
[ -n "$b32" ] || { echo "error: prefetch of $url failed" >&2; exit 1; }
src_sri=$($NIX hash convert --hash-algo sha256 --to sri "$b32")
echo "  crate src hash    -> $src_sri"
patch_field wasmBindgenSrcHash "$src_sri"

# 4. vendor hash — realize the cargo-deps FOD alone; read the mismatch.
patch_field wasmBindgenCargoHash "$FAKE"
out=$($NIX build "path:$PWD#wasm-bindgen-cargo-deps" --no-link 2>&1) || true
got=$(grep -oE 'got:[[:space:]]+sha256-[A-Za-z0-9+/=]+' <<<"$out" | head -1 | grep -oE 'sha256-[A-Za-z0-9+/=]+' || true)
if [ -z "$got" ]; then
  # No mismatch means the fake somehow matched (impossible) or the build
  # broke for another reason — surface it.
  if $NIX build "path:$PWD#wasm-bindgen-cargo-deps" --no-link >/dev/null 2>&1; then
    got=$(sed -nE 's/^\s*wasmBindgenCargoHash = "([^"]*)";/\1/p' "$FLAKE")
  else
    echo "error: cargo-deps build failed without a hash mismatch:" >&2
    echo "$out" | tail -20 >&2
    exit 1
  fi
fi
echo "  cargo vendor hash -> $got"
patch_field wasmBindgenCargoHash "$got"

# Verify the FOD now builds clean (cheap — no CLI compile).
$NIX build "path:$PWD#wasm-bindgen-cargo-deps" --no-link >/dev/null 2>&1 \
  && echo "✓ hashes verified" \
  || { echo "error: vendor hash still wrong after patch" >&2; exit 1; }

echo
echo "flake.nix updated to wasm-bindgen $ver. Review & commit:"
echo "  git -C .. diff -- dev-env/flake.nix"
