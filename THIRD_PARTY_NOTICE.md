# Third-party notices

mcp-compressor is a multi-language project made up of:

- a Python package published as `mcp-compressor`
- a TypeScript package published as `@atlassian/mcp-compressor`
- a shared Rust core crate in `crates/mcp-compressor-core`

This notice summarizes the direct third-party dependencies used by those package surfaces. Each dependency may include additional transitive dependencies with their own license terms; consult the relevant lockfiles (`uv.lock`, `typescript/bun.lock`, and `Cargo.lock`) for the complete resolved dependency graph used for builds.

The mcp-compressor source code is licensed under the repository `LICENSE` file.

## Python package direct dependencies

Declared in `pyproject.toml`:

- `anyio`
- `cryptography`
- `keyring`
- `py-key-value-aio`
- `fastmcp`
- `loguru`
- `loguru-logging-intercept`
- `mcp`
- `pydantic`
- `psutil`
- `starlette`
- `toons`
- `typer`
- `uvicorn`
- `click`
- `just-bash`
- `httpx`

## TypeScript package direct dependencies

Declared in `typescript/package.json`:

- `@modelcontextprotocol/sdk`
- `@toon-format/toon`
- `commander`
- `fastmcp`
- `zod`
- `just-bash` (optional peer dependency)

## Rust core direct dependencies

Declared in `crates/mcp-compressor-core/Cargo.toml`:

- `serde`
- `serde_json`
- `thiserror`
- `rand`
- `tokio`
- `rmcp`
- `axum`
- `async-trait`
- `dirs`
- `reqwest`
- `clap`
- `open`
- `toon-format`
- `pyo3` (optional Python binding feature)
- `napi` (optional Node.js binding feature)
- `napi-derive` (optional Node.js binding feature)

## Development and documentation tooling

The repository also uses third-party development tools for tests, linting, formatting, documentation, and packaging. These are declared in the package manager manifests and lockfiles for each ecosystem.
