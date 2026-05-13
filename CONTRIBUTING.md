# Contributing to `mcp-compressor`

Contributions are welcome. Please keep changes focused, add tests for behavior changes, and update docs for user-facing changes.

## Repository layout

- `crates/mcp-compressor-core` — shared Rust implementation and CLI runtime.
- `crates/mcp-compressor` — public Rust crate and public `mcp-compressor` binary target.
- `crates/mcp-compressor-python` — PyO3 extension crate.
- `crates/mcp-compressor-node` — napi-rs extension crate.
- `python/mcp-compressor` — Python package exposing `mcp_compressor`.
- `typescript` — TypeScript package `@atlassian/mcp-compressor`.
- `tests` — repository-level integration tests.
- `docs` — MkDocs documentation.

## Prerequisites

Install:

- Git
- Python 3.11+
- `uv`
- Rust/Cargo
- Bun
- Node.js/npm

## Set up the repository

```bash
git clone git@github.com:YOUR_NAME/mcp-compressor.git
cd mcp-compressor
uv sync
uv run pre-commit install
```

For the Python package native extension:

```bash
cd python/mcp-compressor
uv run maturin develop
```

For the TypeScript package native addon:

```bash
cd typescript
bun install
bun run build:native
```

## Common checks

Run repository-level checks:

```bash
make check
```

Run repository-level tests:

```bash
make test
```

Targeted checks are often faster while developing:

```bash
# Rust
cargo check -p mcp-compressor-core
PYTHON="$PWD/.venv/bin/python" cargo test -p mcp-compressor-core --test generated_clients -- --nocapture

# Python package
cd python/mcp-compressor
PYTHON="$PWD/../../.venv/bin/python" uv run pytest -q tests
uv run ruff check mcp_compressor tests
uv run ty check --project . --python .venv mcp_compressor tests

# TypeScript package
cd typescript
bun run check
```

## Pull request guidelines

Before opening a PR:

1. Add or update tests for behavior changes.
2. Update docs for public CLI/SDK changes.
3. Keep unrelated changes in separate PRs.
4. Run the relevant targeted checks plus `make check` when practical.
5. Avoid leaking implementation names such as `mcp_compressor_rust` or `mcp_compressor_core` into public user docs.
