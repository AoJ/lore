DB ?= $(CURDIR)/db.sqlite

build:
	cargo build --workspace

release:
	cargo build --release --workspace

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


# Combined pre-PR check.
check: lint check-arch
	cargo test --workspace


clean:
	cargo clean

.PHONY: build release desktop desktop-release serve worker test lint fmt \
        check check-arch clean js-install js-build js-watch js-clean \
        db-version migrate
