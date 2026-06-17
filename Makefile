DB ?= $(CURDIR)/db.sqlite

build:
	cargo build --workspace

release:
	cargo build --release --workspace

# Cross builds for the headless crates (cli/server/worker). Each target wraps
# itself in the `cross` nix dev shell (which carries the cross gcc toolchains
# + per-target linker env), so `make cross*` works from anywhere — no need to
# be in a shell first. lore-ui (GTK/WebView) is macOS-native and not crossed;
# lore-worker stays Linux-only (drives CloakBrowser), so it has no Windows
# target. The first run builds mingw/gnu cross gcc from source (slow, then
# cached); plain `nix develop ./dev-env` stays lean and avoids that.
CROSS_LINUX_TARGET := x86_64-unknown-linux-gnu
CROSS_WIN_TARGET   := x86_64-pc-windows-gnu
# NB: the '#' in the flake ref must be escaped — bare '#' starts a make comment.
NIX_CROSS := nix --extra-experimental-features "nix-command flakes" develop ./dev-env\#cross --command

cross-linux:
	$(NIX_CROSS) cargo build --release --target $(CROSS_LINUX_TARGET) \
		-p lore-cli -p lore-server -p lore-worker

cross-windows:
	$(NIX_CROSS) cargo build --release --target $(CROSS_WIN_TARGET) \
		-p lore-cli -p lore-server

cross: cross-linux cross-windows


# Dependency updates. `cargo update` can bump the `wasm-bindgen` crate, which
# must stay version-matched to the pinned wasm-bindgen-cli in dev-env/flake.nix
# (dx refuses a mismatch). `update-wasm-bindgen` reads the new version from
# Cargo.lock and rewrites the flake's version + both fixed-output hashes
# automatically, so the two never drift by hand.
update-wasm-bindgen:
	./dev-env/update-wasm-bindgen.sh

update-deps:
	cargo update
	nix --extra-experimental-features "nix-command flakes" flake update --flake ./dev-env
	$(MAKE) update-wasm-bindgen

# JS editor bundle (Milkdown-based, output: crates/lore-ui/assets/milkdown.js)
JS_DIR := crates/lore-ui/js
JS_OUT := crates/lore-ui/assets/milkdown.js

js-install:
	cd $(JS_DIR) && npm install

js-build:
	cd $(JS_DIR) && npm run build

js-watch:
	cd $(JS_DIR) && npm run watch

js-clean:
	rm -rf $(JS_DIR)/node_modules $(JS_DIR)/package-lock.json


desktop:
	LORE_DB=$(DB) cargo run -p lore-ui

desktop-release:
	LORE_DB=$(DB) cargo run --release -p lore-ui

serve:
	LORE_DB=$(DB) cargo run -p lore-server

# Self-contained release server: bakes the `make web` bundle into the binary
# (`embed-web` feature) so there's a single file to deploy — no `static/` dir
# to ship alongside and keep in sync. Depends on `web` for a fresh bundle.
server-bundle: web
	cargo build --release -p lore-server --features embed-web
	@echo "Self-contained server → target/release/lore-serve (web bundle embedded)"


# Web frontend bundle. Builds `lore-ui` for `wasm32-unknown-unknown` via
# the Dioxus CLI (`dx`) and stages the output where `lore-server`'s
# `ServeDir` fallback expects it. Resulting in: open `http://localhost:3000`
# after `make serve` to talk to the web UI.
WEB_BUILD_OUT := target/dx/lore-desktop/release/web/public
SERVER_STATIC := crates/lore-server/static

web:
	@command -v dx >/dev/null || { \
		echo "dx (Dioxus CLI) not installed. Install: cargo install --locked dioxus-cli"; \
		exit 1; \
	}
	cd crates/lore-ui && dx build --release --platform web
	@mkdir -p $(SERVER_STATIC)
	@rm -rf $(SERVER_STATIC)/*
	cp -r $(WEB_BUILD_OUT)/. $(SERVER_STATIC)/
	@echo "Web bundle ready in $(SERVER_STATIC)/. Run \`make serve\` and open http://localhost:3000/"

web-clean:
	rm -rf $(WEB_BUILD_OUT) $(SERVER_STATIC)/*


# End-to-end integration tests via chromiumoxide. Spawns a fresh
# `lore-serve` subprocess per `TestApp` (random port, tmp DB) and drives
# the WASM frontend through headless Chromium. Depends on:
#   1. `make web` — bundle staged in lore-server/static/
#   2. lore-serve binary at target/debug/lore-serve
# `make e2e` chains both so a fresh checkout works.
#
# Tests live in crates/lore-e2e/tests/. Not part of `make check` because
# they need a built web bundle + Chromium, which `make check` doesn't.
#
# `TestApp::spawn` recovers from chromiumoxide's flaky `new_page` hang (a missed
# `"load"` lifecycle event — see `open_browser_page`), so the suite is robust on
# a loaded single-core box. `--no-fail-fast` still runs every file if one test
# trips, and the short settle keeps the first launches off the build's tail.
e2e: web
	cargo build -p lore-server
	cargo test -p lore-e2e --tests --no-run
	@sync; sleep 10
	cargo test -p lore-e2e --tests --no-fail-fast -- --nocapture --test-threads 1

worker:
	LORE_DB=$(DB) cargo run -p lore-worker -- --db $(DB)

test:
	cargo test --workspace


# DB schema management
db-version:
	cargo run -q -p lore-cli -- --db $(DB) db-version

migrate:
	cargo run -q -p lore-cli -- --db $(DB) migrate


lint:
	cargo clippy --workspace
	cargo fmt --all -- --check


fmt:
	cargo fmt --all


# Architecture gate — fails CI if rules in .sentrux/rules.toml are violated.
check-arch:
	@command -v sentrux >/dev/null || { \
		echo "sentrux CLI not installed. Install: brew install sentrux"; \
		exit 1; \
	}
	sentrux check


# Dependency audit — licenses, advisories, duplicates (deny.toml).
audit:
	@command -v cargo-deny >/dev/null || { \
		echo "cargo-deny not installed. Install: cargo install --locked cargo-deny"; \
		exit 1; \
	}
	cargo deny check


# Mutation testing on lore-core. Scope/timeouts in .cargo/mutants.toml.
# Slow (minutes); not part of `make check`. Run on demand when adding tests.
mutants:
	@command -v cargo-mutants >/dev/null || { \
		echo "cargo-mutants not installed. Install: cargo install --locked cargo-mutants"; \
		exit 1; \
	}
	cargo mutants --no-shuffle


# Formal verification of pure functions in lore-core via Kani (bounded model
# checking). Slow (minutes per harness); not part of `make check`. Harnesses
# live in `#[cfg(kani)] mod proofs` blocks next to the functions they verify.
verify:
	@command -v cargo-kani >/dev/null || { \
		echo "Kani not installed. Install: cargo install --locked kani-verifier && cargo kani setup"; \
		exit 1; \
	}
	cargo kani -p lore-core


# Combined pre-PR check. `lore-e2e` is excluded because it depends on a
# built web bundle + `lore-serve` binary + Chromium — that's `make e2e`'s
# job, not the fast pre-PR sanity gate.
check: lint check-arch audit
	cargo test --workspace --exclude lore-e2e


# Code coverage via cargo-llvm-cov (requires nix shell with cargo-llvm-cov).
# Baseline measurement; no threshold gate yet.
coverage:
	cargo llvm-cov --workspace --exclude lore-e2e --html
	@echo "Report: target/llvm-cov/html/index.html"

coverage-lcov:
	cargo llvm-cov --workspace --exclude lore-e2e --lcov --output-path lcov.info


clean:
	cargo clean

.PHONY: build release desktop desktop-release serve server-bundle worker test \
        lint fmt check check-arch audit mutants verify coverage coverage-lcov \
        clean js-install js-build js-watch js-clean db-version migrate web \
        web-clean e2e cross cross-linux cross-windows update-wasm-bindgen \
        update-deps
