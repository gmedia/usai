# Usai — production repository task runner.
# Every target here is documented in CLAUDE.md; keep them in sync.

.PHONY: setup check test build fmt fmt-check lint clean

setup:
	pnpm install --frozen-lockfile
	cargo fetch

check: fmt-check lint test

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

lint:
	cargo clippy --workspace --all-targets -- -D warnings
	pnpm -r run typecheck

test:
	cargo test --workspace
	USAI_ENGINE=quickjs cargo test -p usai-runtime --test lifecycle --test http --test workloads
	cargo build -p usai-cli
	USAI_BIN=$(CURDIR)/target/debug/usai pnpm -r run test

build:
	cargo build --workspace --release
	pnpm -r run build

clean:
	cargo clean
	rm -rf packages/*/dist examples/*/dist examples/*/.usai
