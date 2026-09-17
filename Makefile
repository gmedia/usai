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
	pnpm -r run test

build:
	cargo build --workspace --release
	pnpm -r run build

clean:
	cargo clean
	rm -rf packages/*/dist examples/*/dist examples/*/.usai
