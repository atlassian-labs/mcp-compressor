# Installation

`mcp-compressor` ships three public surfaces:

- a CLI named `mcp-compressor`,
- SDKs for Python, TypeScript, and Rust,
- generated clients for shell, Python, and TypeScript.

## Install the SDK

=== "Python"

    ```bash
    pip install mcp-compressor
    ```

    Then import:

    ```python
    from mcp_compressor import CompressorClient
    ```

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

    Install from crates.io:

    ```bash
    cargo install mcp-compressor
    mcp-compressor --help
    ```

    You can also download a release artifact named for your platform, place it on `PATH`, and verify:

    ```bash
    mcp-compressor --help
    ```

    From source:

    ```bash
    cargo build -p mcp-compressor --bin mcp-compressor --release
    ./target/release/mcp-compressor --help
    ```

=== "Python wrapper"

    ```bash
    pip install mcp-compressor
    mcp-compressor --help
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
