# Internal implementation notes

The current public architecture is documented in [Architecture](architecture.md).

This page is intentionally short. Earlier migration-planning notes were removed from the public docs because the library now presents a stable public package structure rather than a migration plan.

## Implementation crates

- `crates/mcp-compressor` is the public Rust crate and public `mcp-compressor` binary target.
- `crates/mcp-compressor-core` contains the shared implementation used by every public surface.
- `crates/mcp-compressor-python` is the PyO3 extension crate used by the Python package.
- `crates/mcp-compressor-node` is the napi-rs extension crate used by the TypeScript package.

## Design principles

- Keep compression, routing, OAuth, proxy, and generated-client behavior in shared Rust where practical.
- Keep language packages thin and idiomatic.
- Do not expose implementation crate names in public examples.
- Prefer framework-neutral primitives in the public SDKs, with optional adapters for specific ecosystems.
