#!/usr/bin/env bash
# Resync the pinned wasm-bindgen-cli in flake.nix to the `wasm-bindgen` crate
# version in ../Cargo.lock. `dx` requires the CLI version to match the crate
# exactly, and the hashes are fixed-output (so version-specific) — this script
# is the "don't hand-edit three values in lockstep" button.
#
# What it does:
#   1. read the wasm-bindgen version from Cargo.lock
#   2. write it into flake.nix (wasmBindgenVersion)
#   3. blank both hashes to the fake-hash sentinel
#   4. build .#wasm-bindgen-cli repeatedly, scraping each "got: sha256-..."
#      mismatch and writing the real hash back, until the build succeeds
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

# 2 + 3. set version, reset both hashes to the sentinel.
sed -i -E "s|^(\s*wasmBindgenVersion = \")[^\"]*(\";)|\1$ver\2|" "$FLAKE"
sed -i -E "s|^(\s*wasmBindgenSrcHash = \")[^\"]*(\";)|\1$FAKE\2|" "$FLAKE"
sed -i -E "s|^(\s*wasmBindgenCargoHash = \")[^\"]*(\";)|\1$FAKE\2|" "$FLAKE"

patch_hash() { # $1 = binding name, $2 = sha256-... value
  sed -i -E "s|^(\s*$1 = \")[^\"]*(\";)|\1$2\2|" "$FLAKE"
}

# 4. build, scrape the mismatch, write it back. The src tarball fails first,
#    then the cargo vendor — at most two fixes, plus a final success pass.
for attempt in 1 2 3; do
  out=$($NIX build "path:$PWD#wasm-bindgen-cli" --no-link 2>&1) && { echo "✓ build OK"; break; }
  mismatch=$(grep "hash mismatch in fixed-output derivation" <<<"$out" | head -1 || true)
  got=$(grep -oE 'got:[[:space:]]+sha256-[A-Za-z0-9+/=]+' <<<"$out" | head -1 | grep -oE 'sha256-[A-Za-z0-9+/=]+' || true)
  if [ -z "$got" ]; then
    echo "error: build failed without a hash mismatch:" >&2
    echo "$out" | tail -20 >&2
    exit 1
  fi
  if grep -qi 'vendor' <<<"$mismatch"; then
    echo "  cargo vendor hash -> $got"
    patch_hash wasmBindgenCargoHash "$got"
  else
    echo "  crate src hash    -> $got"
    patch_hash wasmBindgenSrcHash "$got"
  fi
  [ "$attempt" = 3 ] && { echo "error: still failing after 3 attempts" >&2; exit 1; }
done

echo
echo "flake.nix updated to wasm-bindgen $ver. Review & commit:"
echo "  git -C .. diff -- dev-env/flake.nix"
