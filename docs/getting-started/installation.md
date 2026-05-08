# Installation

`mcp-compressor` ships three public surfaces:

- a CLI named `mcp-compressor`,
- SDKs for Python, TypeScript, and Rust,
- generated clients for shell, Python, and TypeScript.

!!! note "Migration branch package names"
    On the Rust migration branch, the Python distribution is still published separately as `mcp-compressor-rust` until final cutover. The public Python import is already `mcp_compressor`.

## Install the SDK

=== "Python"

    ```bash
    # Local development from this repository
    cd python/mcp-compressor
    uv sync --group dev
    uv run maturin develop
    ```

    Then import:

    ```python
    from mcp_compressor import CompressorClient
    ```

    The temporary distribution/script name is `mcp-compressor-rust`, but users import `mcp_compressor`.

=== "TypeScript"

    ```bash
    cd typescript
    bun install
    bun run build
    bun run build:native
    ```

    Then import:

    ```ts
    import { CompressorClient } from "@atlassian/mcp-compressor";
    ```

=== "Rust"

    Add the public Rust crate in your workspace:

    ```toml
    mcp-compressor = { path = "crates/mcp-compressor" }
    ```

    Then import:

    ```rust
    use mcp_compressor::sdk::{CompressorClient, ServerConfig};
    ```

## Install the CLI

=== "Rust binary from source"

    ```bash
    cargo build -p mcp-compressor-core --release
    ./target/release/mcp-compressor --help
    ```

=== "Python wrapper"

    ```bash
    cd python/mcp-compressor
    uv sync --group dev
    uv run maturin develop
    uv run mcp-compressor-rust --help
    ```

    The wrapper delegates to the Rust binary. Set `MCP_COMPRESSOR_BINARY` if the binary is not on `PATH`.

=== "TypeScript wrapper"

    ```bash
    cd typescript
    bun install
    bun run build
    bun run build:native
    bun run mcp-compressor -- --help
    ```

    The wrapper also delegates to the Rust binary. Set `MCP_COMPRESSOR_BINARY` to override binary discovery.

## Verify installation

=== "CLI"

    ```bash
    mcp-compressor --help
    ```

=== "Python"

    ```python
    from mcp_compressor import CompressorClient

    assert CompressorClient is not None
    ```

=== "TypeScript"

    ```ts
    import { CompressorClient } from "@atlassian/mcp-compressor";

    console.log(typeof CompressorClient);
    ```

=== "Rust"

    ```bash
    cargo check -p mcp-compressor
    ```
