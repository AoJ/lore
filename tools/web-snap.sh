#!/usr/bin/env bash
# Headless Chromium screenshot of a URL. Used by the dev workflow so the
# agent can "see" what the web build is rendering without launching a
# windowed browser.
#
# Usage: tools/web-snap.sh <url> <output.png> [width] [height] [wait_ms]
#
# Defaults: 1280×800 viewport, 1500 ms wait before snapshot (lets the
# WASM bundle boot + initial /api/* requests complete). Bump `wait_ms`
# for slow first-load runs.

set -euo pipefail

URL="${1:-}"
OUT="${2:-}"
W="${3:-1280}"
H="${4:-800}"
WAIT="${5:-1500}"

if [[ -z "$URL" || -z "$OUT" ]]; then
    echo "usage: $0 <url> <output.png> [width=1280] [height=800] [wait_ms=1500]" >&2
    exit 2
fi

CHROMIUM="/Applications/Chromium.app/Contents/MacOS/Chromium"
[[ -x "$CHROMIUM" ]] || CHROMIUM="$(command -v chromium 2>/dev/null || true)"
[[ -x "$CHROMIUM" ]] || { echo "Chromium not found" >&2; exit 1; }

# `--virtual-time-budget` lets headless wait for JS/network without a
# real sleep — Chrome advances a fake clock until budget runs out, but
# only while there's pending JS/network activity. Good for WASM boot.
"$CHROMIUM" \
    --headless=new \
    --disable-gpu \
    --no-sandbox \
    --window-size="${W},${H}" \
    --virtual-time-budget="$WAIT" \
    --screenshot="$OUT" \
    "$URL" \
    >/dev/null 2>&1

[[ -f "$OUT" ]] || { echo "screenshot failed" >&2; exit 1; }
echo "$OUT ($(du -h "$OUT" | cut -f1))"
