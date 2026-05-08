# Installation

`mcp-compressor` is available as a Rust CLI/core, a Rust-backed Python package, and a Rust-backed TypeScript package.

!!! note "Migration branch package names"
    On the Rust migration branch, the Python package is published separately as `mcp-compressor-rust` until final cutover. The TypeScript package uses `@atlassian/mcp-compressor`.

## CLI

The CLI is provided by the Rust binary.

=== "From source"

    ```bash
    cargo build -p mcp-compressor-core --release
    ./target/release/mcp-compressor-core --help
    ```

=== "Python wrapper"

    ```bash
    cd python/mcp-compressor-rust
    uv sync --group dev
    uv run maturin develop
    uv run mcp-compressor-rust --help
    ```

=== "TypeScript wrapper"

    ```bash
    cd typescript
    bun install
    bun run build
    bun run build:native
    bun run mcp-compressor -- --help
    ```

## Python SDK

```bash
cd python/mcp-compressor-rust
uv sync --group dev
uv run maturin develop
```

Then:

```python
from mcp_compressor import CompressorClient
```

## TypeScript SDK

```bash
cd typescript
bun install
bun run build
bun run build:native
```

Then:

```ts
import { CompressorClient } from "@atlassian/mcp-compressor";
```

## Rust SDK

Add the core crate in a workspace or path dependency:

```toml
mcp-compressor-core = { path = "crates/mcp-compressor-core" }
```

Use:

```rust
use mcp_compressor::sdk::CompressorClient;
```
