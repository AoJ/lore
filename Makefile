DB ?= $(CURDIR)/db.sqlite

build:
	cargo build --workspace

release:
	cargo build --release --workspace


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


lint:
	cargo clippy --workspace
	cargo fmt --all -- --check


fmt:
	cargo fmt --all


clean:
	cargo clean

.PHONY: build release desktop desktop-release serve worker test lint fmt clean
