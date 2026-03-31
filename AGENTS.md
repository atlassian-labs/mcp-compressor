# AGENTS.md

This file provides guidance for coding agents working in this repository.

## Repo purpose

`mcp-compressor` is a Python CLI and MCP proxy server that wraps an upstream MCP server and reduces the token footprint exposed to LLMs.

At a high level it:
- connects to an upstream MCP server over stdio, streamable HTTP, or SSE
- proxies that server through FastMCP
- replaces a large tool surface with a compressed wrapper interface
- optionally supports CLI mode and TOON output formatting
- persists OAuth state for remote servers using encrypted local storage

## Important source files

- `mcp_compressor/main.py`
  - CLI entrypoint (`main`, `entrypoint`)
  - transport selection and creation
  - proxy server startup (`_server`, `_cli_mode_server`, `_proxy_client`)
  - OAuth storage helpers (`_build_token_storage`, `_get_or_create_encryption_key`)
  - `clear-oauth` subcommand
- `mcp_compressor/tools.py`
  - `CompressedTools` middleware class
  - compressed tool listing/schema lookup/invocation
  - include/exclude tool filtering
  - validation error formatting
  - TOON output conversion
- `mcp_compressor/logging.py`
  - `configure_logging(log_level)` — loguru setup and stdlib logging intercept
  - `suppress_recoverable_oauth_traceback_logging(transport)` — narrow log filter context manager for recoverable OAuth retries
  - `_RecoverableOAuthTracebackFilter` — the log filter implementation
- `mcp_compressor/cli_tools.py`
  - CLI-facing tool help and argument handling
- `mcp_compressor/cli_bridge.py`
  - local HTTP bridge used in CLI mode
- `mcp_compressor/cli_script.py`
  - generated CLI script management
- `mcp_compressor/types.py`
  - enums and shared types (`CompressionLevel`, `LogLevel`, `TransportType`)
- `tests/`
  - unit/integration coverage for transports, middleware, CLI mode, and proxy behavior

## How the proxy works

### Connection setup

`main()` → `_async_main()` → `_server()` or `_cli_mode_server()`

1. Infers transport type from the command/URL argument
2. Creates a FastMCP transport (stdio, streamable HTTP, or SSE)
3. Opens a `ProxyClient` via `_proxy_client(transport)`, which:
   - wraps the connection with log suppression for recoverable OAuth errors
   - on failure matching the stale-OAuth-500 signature, clears cached OAuth state and retries once
4. Creates a proxy `FastMCP` server via `FastMCP.as_proxy(backend=client)`
5. Installs `CompressedTools` middleware on the proxy server

### Compression middleware

`CompressedTools` (in `tools.py`) is the core behavior of this repo.

It:
- hides all upstream backend tools from clients
- registers a small fixed set of wrapper tools instead:
  - `get_tool_schema(tool_name)` — returns the full upstream schema on demand
  - `invoke_tool(tool_name, tool_input)` — calls the upstream tool
  - `list_tools()` — returned only at `max` compression level
- formats compressed tool listings at four levels: `low`, `medium`, `high`, `max`
- applies optional include/exclude filters on the backend tool set
- enriches validation failures with the real upstream schema
- optionally converts JSON text outputs to TOON format
- exposes a hidden resource `compressor://uncompressed-tools` with the raw upstream tool list

### CLI mode

CLI mode exposes a single `<server>_help` tool to the LLM, and generates a local shell script that maps subcommands to upstream tool invocations via a local HTTP bridge.

Key files:
- `mcp_compressor/cli_tools.py` — help text and arg parsing
- `mcp_compressor/cli_bridge.py` — local HTTP bridge
- `mcp_compressor/cli_script.py` — shell script generation and lifecycle management

### OAuth and token persistence

For remote (HTTP/SSE) servers, `mcp-compressor`:
- delegates the OAuth flow entirely to FastMCP's `OAuth` client
- adds encrypted persistent token storage via:
  - OS keyring (preferred) or file-based fallback for the encryption key
  - `py-key-value-aio` + `cryptography.fernet` for the encrypted token store
- provides a `clear-oauth` CLI command to reset cached state
- suppresses noisy upstream traceback logs when a stale-cache OAuth error is automatically recovered

## Core architectural patterns

### 1. Prefer thin integration code over protocol reimplementation
This project relies heavily on FastMCP and the MCP Python SDK. Prefer using their built-in types and flows rather than reimplementing protocol behavior locally.

Examples:
- use FastMCP transports and `ProxyClient`
- use FastMCP `OAuth` for the OAuth flow; local code only adds persistent storage and clear-oauth UX
- keep local logic focused on compressed tool exposure, encrypted token persistence, and CLI UX

### 2. Keep changes narrow and composable
The repo is relatively small and organized around a few key flows. Prefer adding small helpers over broad refactors unless a broader change is clearly justified.

Good examples already in the codebase:
- small transport helper functions in `main.py`
- encapsulated middleware logic in `CompressedTools`
- targeted helpers for OAuth cache clearing and retry behavior in `main.py`
- logging setup and suppression isolated in `logging.py`

### 3. Preserve pass-through semantics
This wrapper should generally preserve upstream behavior unless it is intentionally transforming output.

When changing behavior, be careful not to break:
- tool invocation pass-through
- prompt/resource pass-through
- validation error reporting
- upstream schema fidelity

### 4. Keep compression behavior explicit
Compression levels are a core product behavior. Changes to tool descriptions, hidden tools, CLI help output, or invocation flows should be evaluated in the context of:
- `low`
- `medium`
- `high`
- `max`

### 5. Prefer focused tests over broad end-to-end testing
The existing test suite favors targeted unit and integration tests. Follow that pattern.

## Development workflow

### Environment setup
This repo uses `uv`.

Common commands:

```bash
make install
make test
make check
make docs-test
```

Direct commands are also common:

```bash
uv sync
uv run pytest -q
uv run ruff check .
uv run ty check
```

### Testing guidance
When making changes, run the smallest relevant test subset first.

Examples:

```bash
uv run pytest -q tests/test_main.py
uv run pytest -q tests/test_tools.py
uv run pytest -q tests/test_cli.py
uv run pytest -q tests/test_integration.py
```

Only run broader checks when the change justifies it.

### Linting/type checking
The repo uses:
- Ruff
- ty
- deptry
- pre-commit

If you touch Python code, at minimum run Ruff on the changed files and the most relevant tests.

## Code style and best practices

- Keep functions small and single-purpose where practical.
- Match existing naming and file layout before introducing new modules.
- Preserve typed signatures; this repo uses modern typing heavily.
- Prefer simple helper functions over deeply nested inline logic.
- Keep user-facing error messages actionable.
- For tests, prefer monkeypatching/fakes for narrow behavior instead of spinning up unnecessary infrastructure.
- Avoid changing unrelated formatting or refactoring unrelated code while fixing a targeted issue.

## OAuth-specific guidance

OAuth support should stay mostly delegated to FastMCP.

Local code in this repo is primarily responsible for:
- encrypted persistent token storage
- clearing cached OAuth state (`clear-oauth`)
- suppressing recoverable stale-cache traceback logs (see `mcp_compressor/logging.py`)

Avoid reimplementing OAuth protocol logic locally unless absolutely necessary.

## Working with local dependency clones

This repo is set up so coding agents can inspect important dependency source code locally.

### Use `dependencies/` when needed
If the `dependencies/` directory exists, agents should use the cloned repos there when they need to inspect upstream implementation details, docs, or tests.

This is especially helpful for:
- FastMCP transport and proxy behavior
- FastMCP OAuth/client behavior
- MCP Python SDK auth/protocol behavior

### If `dependencies/` does not exist
Agents should create it and clone the relevant repos into it.

Important: `dependencies/` is intentionally gitignored at the top level and should remain uncommitted.

Recommended initial clones:

```bash
git clone https://github.com/jlowin/fastmcp.git dependencies/fastmcp
git clone https://github.com/modelcontextprotocol/python-sdk.git dependencies/python-sdk
```

Use these repos as read-only local context unless the task explicitly involves editing them.

### Relevant upstream repos
- FastMCP: `https://github.com/jlowin/fastmcp`
- MCP Python SDK: `https://github.com/modelcontextprotocol/python-sdk`

### Getting FastMCP documentation

FastMCP docs are written in MDX and live in `dependencies/fastmcp/docs/`.

**Always check out the tag matching the installed version before reading docs:**

```bash
# Check which version is installed
uv run python -c "import fastmcp; print(fastmcp.__version__)"

# Checkout the corresponding tag in the clone
cd dependencies/fastmcp && git fetch --tags && git checkout v<VERSION>
```

For example, if the installed version is `3.1.1`:

```bash
cd dependencies/fastmcp && git fetch --tags && git checkout v3.1.1
```

**Key doc directories in `dependencies/fastmcp/docs/`:**

| Directory | What's in it |
|---|---|
| `getting-started/` | Installation, quickstart |
| `clients/` | Client API, transports, OAuth auth |
| `clients/auth/` | OAuth, Bearer, CIMD auth flows |
| `servers/` | Server API, tools, resources, prompts, middleware, auth |
| `servers/providers/` | Proxy, local, filesystem, OpenAPI providers |
| `servers/auth/` | OAuth server, OIDC proxy, token verification |
| `integrations/` | Claude, Cursor, Gemini, GitHub, and other integrations |
| `python-sdk/` | Auto-generated API reference for all FastMCP modules |
| `patterns/` | Testing, CLI, contrib patterns |

**Most relevant for this repo:**

- `dependencies/fastmcp/docs/clients/auth/oauth.mdx` — OAuth client flow and token storage
- `dependencies/fastmcp/docs/clients/transports.mdx` — transport types and configuration
- `dependencies/fastmcp/docs/servers/providers/proxy.mdx` — ProxyClient and proxy server behavior
- `dependencies/fastmcp/docs/servers/middleware.mdx` — Middleware API that `CompressedTools` uses
- `dependencies/fastmcp/docs/python-sdk/fastmcp-client-auth-oauth.mdx` — full OAuth class reference
- `dependencies/fastmcp/docs/python-sdk/fastmcp-server-providers-proxy.mdx` — full ProxyClient reference

## Practical change guidelines for agents

Before changing code:
1. identify the smallest affected path
2. inspect relevant tests first
3. inspect upstream FastMCP / python-sdk behavior if the issue touches transports, OAuth, or MCP protocol semantics
4. check out the matching FastMCP tag in `dependencies/fastmcp` and read the relevant docs

When changing code:
1. keep the implementation small
2. avoid duplicating upstream behavior locally
3. preserve backward-compatible CLI behavior where possible
4. add focused tests for the changed behavior

Before finishing:
1. run targeted tests
2. run `make check` to verify lint, types, and deps are clean
3. summarize the behavioral change and any remaining risks

## When to consult upstream dependency code

Check `dependencies/fastmcp` (docs and source) for questions about:
- `ProxyClient` behavior
- transport creation and options
- FastMCP OAuth flow, token storage, stale-client retry behavior
- `Middleware` base class API
- `FastMCP.as_proxy(...)` behavior
- `Tool`, `Resource`, `Prompt` model types

Check `dependencies/python-sdk` for questions about:
- MCP auth state machine behavior
- refresh-token handling
- authorization flow details (the `OAuthClientProvider.async_auth_flow` generator)
- lower-level protocol/auth semantics

## Summary

The best changes in this repo are usually:
- small
- well-tested
- aligned with FastMCP/MCP SDK behavior
- focused on wrapper-specific behavior rather than protocol reinvention
