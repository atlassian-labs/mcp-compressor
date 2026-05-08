# CLI parity report: legacy Python vs Rust migration CLIs

Date: 2026-05-08

This report compares:

1. the current published legacy Python CLI, run with `uvx mcp-compressor`,
2. the Rust binary, `target/debug/mcp-compressor`,
3. the Rust-backed Python wrapper, `uv run mcp-compressor-rust`,
4. the Rust-backed TypeScript wrapper, `bun dist/cli.js`.

The goal is to identify user-visible parity gaps before the Rust migration branch becomes the main implementation.

## Test setup

### Deterministic local backend

Most comparisons used the checked-in FastMCP fixture server:

```text
crates/mcp-compressor-core/tests/fixtures/alpha_server.py
```

It exposes:

- `echo(message)`
- `add(a, b)`
- `structured_data()`
- one resource
- one prompt

### Real-world remote backend

The Atlassian MCP server is already covered by the real-world test suite:

```text
https://mcp.atlassian.com/v1/mcp
```

The parity report did not paste credentials or secret-derived output. Current user-facing examples should prefer OAuth; CI may use explicit headers for non-interactive testing.

### Captured artifacts

Temporary captures were written to:

```text
/tmp/mcp-compressor-parity
```

They included top-level `--help`, generated CLI `--help`, generated subcommand help, MCP tool lists, and `get_tool_schema` / `invoke_tool` results.

## Executive summary

| Area | Parity status | Notes |
|---|---|---|
| Rust/Python/TypeScript migration CLIs | Good | All three current CLIs delegate to the same Rust binary behavior. |
| Standard compressed MCP schema output | Good | Rust and legacy produce the same tool schema text for the local fixture. |
| Wrapper tool names by compression level | Mostly good | `medium` exposes `get_tool_schema` / `invoke_tool`; `max` adds `list_tools`, matching intended Rust design. |
| Generated CLI help UX | Good and likely better than legacy | Rust-generated CLI has clear top-level and subcommand help. |
| Top-level CLI options | Partial parity | Rust is missing several legacy convenience options: `--cwd`, `--env`, `--timeout`, `--log-level`, shell completion flags. Rust adds newer `--code-mode`, `--transport`, `--config`, `clear-oauth`. |
| Wrapper tool descriptions | Parity gap | Legacy `get_tool_schema` description embeds the available compressed backend tool list; Rust description is generic. |
| MCP structured content | Parity gap | Legacy `get_tool_schema` result includes `structured_content`; Rust currently returns text content only. |
| Code Mode naming | Improved in Rust | Rust has `--code-mode python/typescript`; legacy has no equivalent unified term. |
| OAuth | Rust better aligned with target UX | Rust supports native OAuth; legacy public docs historically emphasized explicit headers. |

## Top-level CLI help comparison

### Legacy Python CLI

Command:

```bash
uvx mcp-compressor --help
```

Key characteristics:

- Typer/Rich formatted help.
- Required positional `COMMAND_OR_URL`.
- Options include:
  - `--cwd`
  - `--env` / `-e`
  - `--header` / `-H`
  - `--timeout` / `-t`
  - `--compression-level` / `-c`
  - `--server-name` / `-n`
  - `--log-level` / `-l`
  - `--toonify`
  - `--cli-mode`
  - `--just-bash`
  - `--cli-port`
  - `--include-tools`
  - `--exclude-tools`
  - shell completion flags
  - `--version`

Strengths:

- Very descriptive help strings.
- Explicitly documents stdio, HTTP, SSE, and MCP config JSON input.
- Documents backend headers/env/cwd/timeouts in the top-level CLI.

Weaknesses relative to Rust migration:

- No unified `Code Mode` terminology.
- No native OAuth command surface.
- No streamable HTTP frontend option.

### Rust binary

Command:

```bash
mcp-compressor --help
```

Key characteristics:

- Clap formatted help.
- Backend arguments are explicitly placed after `--`.
- Options include:
  - `--compression` / `-c`
  - `--config`
  - `--server-name`
  - `--transform-mode`
  - `--cli-mode`
  - `--just-bash`
  - `--just-bash-mode`
  - `--code-mode <python|typescript>`
  - `--include-tools`
  - `--exclude-tools`
  - `--toonify`
  - `--output-dir`
  - `--multi-server`
  - `--transport`
  - `--port`
  - `clear-oauth`

Strengths:

- Public binary name is now correct: `mcp-compressor`.
- Code Mode has a unified option.
- Supports frontend `stdio` and `streamable-http` transports.
- Supports OAuth clearing.
- Backend arguments after `--` avoid ambiguity between compressor options and backend options.

Gaps versus legacy:

- Missing `--cwd` for stdio backend working directory.
- Missing `--env/-e` for stdio backend env injection.
- Missing `--timeout/-t` for backend connect/request timeout.
- Missing `--log-level/-l`.
- Missing `--version`.
- Missing completion generation/install flags.
- No short aliases for some legacy options:
  - `--server-name` lacks `-n`.
  - `--header` is only parsed after backend URL, not top-level.

### Python and TypeScript wrappers

The current Python and TypeScript CLIs delegate to the Rust binary. Their help output is intentionally identical to the Rust binary after wrapper startup noise is removed.

Python wrapper command:

```bash
mcp-compressor-rust --help
```

TypeScript wrapper command:

```bash
mcp-compressor --help
```

Observations:

- Good: wrappers do not maintain separate CLI behavior.
- Good: parity across current Rust/Python/TypeScript surfaces is strong.
- Watch item: the Python wrapper still has temporary distribution/script name `mcp-compressor-rust` until final Python package cutover.

## Compressed MCP tool surface comparison

### Medium compression, local fixture

Legacy Python and Rust both expose:

```text
alpha_get_tool_schema
alpha_invoke_tool
```

Rust `medium` output:

```text
alpha_get_tool_schema
alpha_invoke_tool
```

Legacy `medium` output:

```text
alpha_get_tool_schema
alpha_invoke_tool
```

### Max compression, local fixture

Rust exposes:

```text
alpha_get_tool_schema
alpha_invoke_tool
alpha_list_tools
```

This matches the current Rust design, where `max` includes a separate `list_tools` wrapper.

## Wrapper tool descriptions

### Legacy behavior

Legacy `alpha_get_tool_schema` description includes the available compressed backend tool list inline, e.g.:

```text
Get the input schema for a specific tool from the alpha toolset.

Available tools are:
<tool>echo(message): Echo a message from alpha.</tool>
...
```

This is useful because the model can see candidate backend tool names inside the wrapper description.

### Rust behavior

Rust currently uses generic descriptions:

```text
Return the full schema for a backend tool.
Invoke a backend tool by name.
```

### Recommendation

Bring Rust wrapper descriptions closer to legacy:

- include toolset/server name,
- include compressed backend tool listing in `get_tool_schema` description,
- mention expected `tool_name` values,
- keep descriptions concise by compression level.

This is likely the most important user/model-behavior parity gap found in the report.

## Schema response comparison

For `echo(message)`, Rust and legacy returned the same human-visible schema text:

```text
<tool>echo(message): Echo a message from alpha.</tool>

{
  "additionalProperties": false,
  "properties": {
    "message": {
      "type": "string"
    }
  },
  "required": [
    "message"
  ],
  "type": "object"
}
```

Difference:

- Legacy includes `structured_content` containing the same result.
- Rust currently returns text content only.

Recommendation:

- Consider setting structured content for wrapper tool calls where supported by `rmcp`.
- This is lower priority than wrapper descriptions, but useful for clients that prefer structured responses.

## Generated CLI help comparison

Rust generated CLI top-level help for the local fixture:

```text
alpha - the alpha toolset

When relevant, outputs from this CLI will prefer using the TOON format for more efficient representation of data.

USAGE:
  alpha <subcommand> [options]

SUBCOMMANDS:
  echo                                Echo a message from alpha.
  add                                 Add two integers on alpha.
  structured-data                     Return structured alpha data.

Run 'alpha <subcommand> --help' for subcommand usage.
```

Rust generated subcommand help:

```text
alpha echo

Echo a message from alpha.

USAGE:
  alpha echo --message <value>

OPTIONS:
  --message                    <string>  required
```

Observations:

- This is strong and close to the desired legacy UX.
- The TOON note is present.
- Kebab-case subcommand names are correct.
- Required argument status is visible.

Potential improvements:

- Include schema descriptions for individual arguments where available.
- Show defaults/enums for JSON Schema fields when available.
- Consider supporting `-h` as an alias for generated subcommand help if not already supported.

## Generated Code Mode clients

The current Rust migration branch supports:

```bash
mcp-compressor --code-mode python ...
mcp-compressor --code-mode typescript ...
```

The legacy Python CLI does not have the same unified Code Mode terminology.

Parity status:

- Rust behavior is the intended future-facing API.
- Legacy parity is not required here; this is an improvement.

Important current behavior:

- Generated Python functions are synchronous.
- Generated TypeScript functions are async.

Open design question:

- Should generated Python Code Mode also support async functions?

Recommendation:

- Keep sync Python generation as the default for simple agent/script use.
- Add async Python generation as an explicit option later, rather than silently changing the default.

## Just Bash comparison

Legacy Python supports `--just-bash` directly.

Rust migration supports:

- `--just-bash`
- `--just-bash-mode`
- SDK provider metadata for Python and TypeScript hosts.

Parity status:

- CLI startup and provider metadata are implemented.
- Language-host helpers exist for Python and TypeScript.
- Rust intentionally does not execute shell commands itself; hosts consume provider metadata and register commands.

Recommendation:

- Add a dedicated real-world Just Bash parity report/test once the host integration contract is final.

## Remote Atlassian MCP notes

Current user-facing Rust docs prefer OAuth:

```bash
mcp-compressor -c medium --server-name atlassian -- https://mcp.atlassian.com/v1/mcp
```

CI real-world tests may use explicit Basic auth headers for non-interactive execution.

Parity status:

- Rust has stronger native OAuth direction than legacy.
- Legacy explicit-header behavior remains available through backend args.

## Current CLI parity table

| Capability | Legacy Python CLI | Rust CLI | Python wrapper | TypeScript wrapper | Notes |
|---|---:|---:|---:|---:|---|
| Standard stdio compression | ✅ | ✅ | ✅ | ✅ | Rust/Python/TS wrappers share Rust behavior. |
| Remote HTTP backend | ✅ | ✅ | ✅ | ✅ | Rust supports streamable HTTP and native TLS. |
| OAuth remote backend | ❌ / limited | ✅ | ✅ | ✅ | Rust has native OAuth flow. |
| Explicit headers | ✅ top-level `-H` | ✅ backend arg `-H` after `--` | ✅ | ✅ | Different placement by design. |
| `--cwd` | ✅ | ❌ | ❌ | ❌ | Missing parity item. |
| `--env/-e` | ✅ | ❌ | ❌ | ❌ | Missing parity item. |
| `--timeout/-t` | ✅ | ❌ | ❌ | ❌ | Missing parity item. |
| `--log-level/-l` | ✅ | ❌ | ❌ | ❌ | Missing parity item. |
| `--version` | ✅ | ❌ | ❌ | ❌ | Should add before release. |
| shell completion flags | ✅ | ❌ | ❌ | ❌ | Nice-to-have. |
| CLI Mode generated shell client | ✅ | ✅ | ✅ | ✅ | Rust generated help is good. |
| Code Mode Python client | ❌ / not same concept | ✅ | ✅ | ✅ | Rust migration improvement. |
| Code Mode TypeScript client | ❌ / not same concept | ✅ | ✅ | ✅ | Rust migration improvement. |
| Just Bash mode | ✅ | ✅ partial | ✅ partial | ✅ partial | Host integration exists; final parity should be tested. |
| Tool filters | ✅ | ✅ | ✅ | ✅ | Covered in tests. |
| TOON output | ✅ | ✅ | ✅ | ✅ | Covered in tests. |
| Streamable HTTP frontend | ❌ | ✅ | ✅ | ✅ | Rust migration improvement. |
| Public SDK imports | N/A | ✅ `mcp_compressor` crate | ✅ `mcp_compressor` | ✅ `@atlassian/mcp-compressor` | Guarded by tests. |

## Recommended follow-up work

### P0 before final cutover

1. **Improve Rust wrapper tool descriptions**

   Match legacy behavior by including the compressed backend tool list in `get_tool_schema` descriptions.

2. **Add missing high-value legacy CLI options**

   Add:

   - `--cwd`
   - `--env/-e`
   - `--timeout/-t`
   - `--version`

   These are practical user-facing options.

3. **Decide top-level header placement**

   Rust currently accepts headers after backend URL:

   ```bash
   mcp-compressor -- https://server/mcp -H "Authorization=Bearer ..."
   ```

   Legacy accepts top-level `-H`. Keeping backend args after `--` is cleaner, but docs should call this out clearly.

4. **Add generated CLI argument descriptions**

   Generated CLI already shows required/type. Add JSON Schema field descriptions/defaults/enums where available.

### P1 polish

1. Add `--log-level/-l`.
2. Add shell completion support if Clap makes it straightforward.
3. Add structured content to Rust wrapper responses.
4. Add async Python Code Mode as an opt-in generator variant.
5. Add a dedicated Just Bash host parity test against a real agent host.

## Bottom line

The Rust migration CLIs are already behaviorally aligned across Rust, Python, and TypeScript because both language wrappers delegate to the same Rust binary.

Compared with the production legacy Python CLI, the Rust migration has stronger SDK, Code Mode, OAuth, and release-artifact direction. The main parity gaps are now concentrated in:

- descriptive wrapper tool text,
- a handful of legacy top-level CLI convenience flags,
- generated CLI argument detail,
- structured MCP result metadata.
