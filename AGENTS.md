# AGENTS.md

Guidance for coding agents working in this repository.

## Project purpose

`mcp-compressor` helps agents use large MCP servers without sending every backend tool description and schema to the model up front. It provides:

- a Rust CLI MCP proxy (`mcp-compressor`),
- Rust, Python, and TypeScript SDKs,
- generated CLI/Python/TypeScript clients,
- Just Bash integration,
- remote streamable HTTP backend support,
- OAuth and dynamic auth-provider support,
- local TypeScript tool compression for AI SDK-style tools.

The implementation is Rust-first. Python and TypeScript bindings should stay thin and delegate core behavior to Rust whenever practical.

## Repository layout

- `crates/mcp-compressor/` — public Rust crate and installable `mcp-compressor` binary.
- `crates/mcp-compressor-core/` — core Rust implementation: compression, config parsing, MCP runtime, proxy server, OAuth, generated clients, SDK helpers, and FFI DTOs.
- `crates/mcp-compressor-python/` — PyO3 extension crate backing the Python package.
- `crates/mcp-compressor-node/` — napi-rs native addon backing the TypeScript package.
- `python/mcp-compressor/` — public Python package. Public import is `mcp_compressor`.
- `typescript/` — public TypeScript package `@atlassian/mcp-compressor`.
- `docs/` — public documentation site.
- `tests/` — repository-level Python integration and real-world tests.

## Public API names

Do not leak implementation/migration names into public docs or examples.

Use:

- Python package/import: `mcp-compressor` / `mcp_compressor`
- TypeScript package: `@atlassian/mcp-compressor`
- Rust crate: `mcp-compressor` / `mcp_compressor`
- CLI: `mcp-compressor`

Avoid public references to old or internal names such as `mcp-compressor-rust`, `mcp_compressor_rust`, or `mcp-compressor-core` unless you are editing explicitly internal/development documentation.

Run this guard after public API/doc changes:

```bash
uv run python scripts/check_public_api_names.py
```

## Core architecture principles

1. **Keep the core in Rust.** Compression semantics, config normalization, proxy routing, OAuth persistence, generated-client behavior, and SDK session logic should live in Rust when feasible.
2. **Keep bindings thin.** Python and TypeScript should provide idiomatic wrappers, type shapes, and package ergonomics, but avoid duplicating core behavior.
3. **Keep framework adapters separate.** TypeScript framework helpers such as AI SDK/Mastra adapters belong in TS-specific adapter modules. Do not add TS-framework concepts to Python or Rust APIs.
4. **Preserve MCP semantics.** The proxy should pass through backend tool/resource/prompt behavior unless intentionally transforming the tool surface or output format.
5. **Be explicit about compression.** Changes to tool listings, wrapper descriptions, help text, schema lookup, or invocation can affect token savings. Test all compression levels when changing compression behavior.
6. **Do not reintroduce stdio subprocess requirements for SDK use.** SDK clients should start local Rust sessions/proxies in-process rather than invoking the `mcp-compressor` CLI as a subprocess.

## Important Rust modules

- `compression/` — compression levels and tool/schema formatting.
- `config/` — MCP config parsing and topology.
- `server/` — backend connections, compressed server state, MCP frontend registration, and tool cache.
- `proxy/` — local authenticated HTTP proxy used by SDKs and generated clients.
- `client_gen/` — generated shell CLI, Python, and TypeScript clients.
- `cli/` — generated CLI argument mapping/parsing.
- `app/` — top-level CLI options, paths, runtime modes, and startup banner.
- `oauth.rs` — OAuth callback listener and credential/state stores.
- `sdk.rs` — public Rust SDK (`CompressorClient`, `CompressorProxy`, `ServerConfig`, etc.).
- `ffi/` — DTOs and helpers consumed by PyO3/napi crates.

## CLI behavior to preserve

- Standard MCP proxy mode:

  ```bash
  mcp-compressor -c medium -- python server.py
  ```

- Backend server arguments belong after `--`:

  ```bash
  mcp-compressor -- https://example.com/mcp -H "Authorization=Bearer ${TOKEN}" --timeout 30
  ```

- `--server-name` is for direct backend commands/URLs only. It must not be combined with MCP JSON config, because config keys define server names.
- CLI Mode installs generated shell commands to a PATH-style location unless `--output-dir` is provided.
- Code Mode uses `--code-mode python|typescript` and defaults generated code to `./dist` when `--output-dir` is omitted.
- Deprecated aliases `--python-mode` and `--typescript-mode` may exist for compatibility, but new docs/examples should prefer `--code-mode`.

## SDK behavior to preserve

All three language SDKs should expose the same mental model:

```text
CompressorClient -> CompressorProxy -> tools/schema/invoke/generated clients
```

Python:

```python
from mcp_compressor import CompressorClient
```

TypeScript:

```ts
import { CompressorClient } from "@atlassian/mcp-compressor";
```

Rust:

```rust
use mcp_compressor::sdk::{CompressorClient, ServerConfig};
```

SDKs should support:

- direct backend command/URL configs,
- MCP JSON config where applicable,
- compression levels,
- include/exclude filters,
- TOON output,
- CLI/Just Bash transform modes,
- generated CLI/Python/TypeScript clients,
- dynamic per-request auth providers for remote HTTP backends.

## TypeScript local tool compression

The TS package also supports in-process compression of local tool functions via `compressTools(...)`. This is for AI SDK-style tools and should remain TypeScript-specific unless a real equivalent use case appears in another language.

## OAuth/auth guidance

- Remote streamable HTTP backends can use explicit `-H KEY=VALUE` backend headers after `--`.
- SDK auth providers should refresh per request for remote HTTP backends.
- OAuth callback UI should remain self-contained and should not load remote assets/scripts.
- OAuth credential/state writes should remain atomic.
- Do not log secrets. If debugging auth headers, log only presence/redacted lengths.

## Testing and validation

Use the narrowest relevant checks first, then broader checks before PRs.

General:

```bash
make check
make docs-test
```

Rust:

```bash
cargo check -p mcp-compressor-core
cargo check -p mcp-compressor
PYTHON="$PWD/.venv/bin/python" cargo test -p mcp-compressor-core --lib -- --nocapture
PYTHON="$PWD/.venv/bin/python" cargo test -p mcp-compressor-core --tests --no-run
```

Python package:

```bash
cd python/mcp-compressor
uv run maturin develop
PYTHON="$PWD/../../.venv/bin/python" uv run pytest -q tests
uv run ruff check mcp_compressor tests
uv run ty check --project . --python .venv mcp_compressor tests
```

TypeScript package:

```bash
cd typescript
bun run build:native
PYTHON="$PWD/../.venv/bin/python" bun run check
```

Repository integration tests:

```bash
uv run pytest -q tests/test_public_cli_workflows.py
uv run pytest -q tests/test_rust_core_normal_mode.py
```

Real-world Atlassian MCP tests require:

```bash
ATLASSIAN_MCP_BASIC_TOKEN=...
```

and normally run via the dedicated GitHub environment/workflow.

## CI/release workflows

Important workflows:

- `.github/workflows/main.yml` — primary checks.
- `.github/workflows/atlassian-mcp-integration.yml` — real-world Atlassian MCP tests.
- `.github/workflows/release-artifacts.yml` — cross-platform artifact smoke tests.
- `.github/workflows/on-release-main.yml` — unified release entrypoint for Python, Rust crates, TypeScript dispatch, and docs.
- `.github/workflows/publish-typescript-package.yml` — TypeScript npm-public publish workflow dispatched on the `release` branch so Artifactory receives branch-based OIDC claims.

Release versions are derived from tags where possible. Avoid hardcoding release versions in source unless a package manager requires a placeholder. See `docs/development-release.md` before changing release workflows.

## Documentation guidance

Docs are public-facing. Prefer clear user concepts over implementation history.

- Use tabs for equivalent Python/TypeScript/Rust/CLI examples.
- Use `Code Mode` as the umbrella term for generated Python/TypeScript clients.
- Keep Atlassian examples OAuth-first unless demonstrating explicit headers.
- Keep the README and docs index aligned; `docs/index.md` includes `README.md`.
- Do not add migration/parity reports to public docs.

Run:

```bash
make docs-test
```

## Dependency and upstream inspection

If you need to inspect dependency behavior, prefer official docs/source:

- `rmcp` for Rust MCP SDK behavior.
- `napi-rs` for Node native bindings.
- `PyO3`/`maturin` for Python native bindings.
- `FastMCP` for Python fixture server behavior.
- `just-bash` for Just Bash command integration.

Use web or cloned dependency sources when behavior is version-sensitive.

## PR hygiene

- Keep PRs focused.
- Include validation commands in PR descriptions.
- Avoid broad refactors unless they simplify current architecture.
- Do not commit generated native artifacts such as `.node` files or built wheels.
- Avoid shell deletion in commands; use appropriate file tools or safe temp directories.
- If CodeQL flags a true issue, harden the code. Suppress only when it is clearly a generated-binding false positive and document why.
