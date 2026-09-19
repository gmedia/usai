# Usai — production repository task runner.
# Every target here is documented in CLAUDE.md; keep them in sync.

.PHONY: setup check test build fmt fmt-check lint docs docs-check clean

setup:
	pnpm install --frozen-lockfile
	cargo fetch

check: fmt-check lint docs-check test

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

lint:
	cargo clippy --workspace --all-targets -- -D warnings
	pnpm -r run typecheck

# The SDK reference (docs/sdk) is generated from the sources' doc comments
# and committed; `docs-check` fails when it is stale.
docs:
	pnpm --filter @sakaladev/usai run docs

docs-check: docs
	@if [ -n "$$(git status --porcelain docs/sdk)" ]; then git status --short docs/sdk; echo "docs/sdk is stale: run 'make docs' and commit"; exit 1; fi

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
