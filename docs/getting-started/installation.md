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
    pip install mcp-compressor-rust
    ```

    Then import:

    ```python
    from mcp_compressor import CompressorClient
    ```

    The temporary distribution name is `mcp-compressor-rust`, but users import `mcp_compressor`.

=== "TypeScript"

    ```bash
    npm install @atlassian/mcp-compressor
    ```

    Then import:

    ```ts
    import { CompressorClient } from "@atlassian/mcp-compressor";
    ```

=== "Rust"

    Add the public Rust crate to your project:

    ```toml
    mcp-compressor = "0.1"
    ```

    Then import:

    ```rust
    use mcp_compressor::sdk::{CompressorClient, ServerConfig};
    ```

## Install the CLI

=== "Rust binary"

    Download a release artifact named for your platform, place it on `PATH`, and verify:

    ```bash
    mcp-compressor --help
    ```

    From source:

    ```bash
    cargo build -p mcp-compressor-core --release
    ./target/release/mcp-compressor --help
    ```

=== "Python wrapper"

    ```bash
    pip install mcp-compressor-rust
    mcp-compressor-rust --help
    ```

    The wrapper delegates to the Rust binary. Set `MCP_COMPRESSOR_BINARY` if the binary is not on `PATH`.

=== "TypeScript wrapper"

    ```bash
    npm install -g @atlassian/mcp-compressor
    mcp-compressor --help
    ```

    The wrapper also delegates to the Rust binary. Set `MCP_COMPRESSOR_BINARY` to override binary discovery.

## Development from source

Use these commands when working on this repository.

=== "Python"

    ```bash
    cd python/mcp-compressor
    uv sync --group dev
    uv run maturin develop
    uv run pytest -q tests
    ```

    The `dev` group is only needed for contributor tooling such as `maturin`, `pytest`, `ruff`, and `ty`.

=== "TypeScript"

    ```bash
    cd typescript
    bun install
    bun run build
    bun run build:native
    bun run check
    ```

=== "Rust"

    ```bash
    cargo check -p mcp-compressor
    cargo test -p mcp-compressor-core --tests --no-run
    ```

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
