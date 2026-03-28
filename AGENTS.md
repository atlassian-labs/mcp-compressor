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
  - CLI entrypoint
  - transport selection and creation
  - proxy server startup
  - CLI mode startup
  - OAuth storage and clear-oauth support
- `mcp_compressor/tools.py`
  - `CompressedTools` middleware
  - compressed tool listing/schema lookup/invocation
  - validation error formatting
  - TOON output conversion
- `mcp_compressor/cli_tools.py`
  - CLI-facing tool help and argument handling
- `mcp_compressor/cli_bridge.py`
  - local HTTP bridge used in CLI mode
- `mcp_compressor/cli_script.py`
  - generated CLI script management
- `mcp_compressor/types.py`
  - enums and shared types
- `tests/`
  - unit/integration coverage for transports, middleware, CLI mode, and proxy behavior

## Core architectural patterns

### 1. Prefer thin integration code over protocol reimplementation
This project relies heavily on FastMCP and the MCP Python SDK. Prefer using their built-in types and flows rather than reimplementing protocol behavior locally.

Examples:
- use FastMCP transports and `ProxyClient`
- use FastMCP OAuth support instead of custom OAuth flow logic
- keep local logic focused on repo-specific behavior such as compressed tool exposure, encrypted token persistence, and CLI UX

### 2. Keep changes narrow and composable
The repo is relatively small and organized around a few key flows. Prefer adding small helpers over broad refactors unless a broader change is clearly justified.

Good examples already in the codebase:
- small transport helper functions in `main.py`
- encapsulated middleware logic in `CompressedTools`
- targeted helper functions for OAuth cache clearing and retry behavior

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
- small UX improvements around stale cached credentials

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

## Practical change guidelines for agents

Before changing code:
1. identify the smallest affected path
2. inspect relevant tests first
3. inspect upstream FastMCP / python-sdk behavior if the issue touches transports, OAuth, or MCP protocol semantics

When changing code:
1. keep the implementation small
2. avoid duplicating upstream behavior locally
3. preserve backward-compatible CLI behavior where possible
4. add focused tests for the changed behavior

Before finishing:
1. run targeted tests
2. run targeted linting
3. summarize the behavioral change and any remaining risks

## When to consult upstream dependency code

Check `dependencies/fastmcp` for questions about:
- `ProxyClient`
- transport behavior
- FastMCP OAuth
- auth retry behavior
- server/client expectations

Check `dependencies/python-sdk` for questions about:
- MCP auth state machine behavior
- refresh-token handling
- authorization flow details
- lower-level protocol/auth semantics

## Summary

The best changes in this repo are usually:
- small
- well-tested
- aligned with FastMCP/MCP SDK behavior
- focused on wrapper-specific behavior rather than protocol reinvention
