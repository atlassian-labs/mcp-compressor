# CLI parity report

This report compares the current Rust migration CLIs with the published legacy Python CLI.

Compared surfaces:

| Surface | Command used |
|---|---|
| Legacy Python CLI | `uvx mcp-compressor` |
| Rust CLI binary | `target/debug/mcp-compressor` |
| Python package CLI wrapper | `python/mcp-compressor` → `uv run mcp-compressor-rust` |
| TypeScript package CLI wrapper | `typescript` → `bun dist/cli.js` |

The current Python and TypeScript CLIs are thin wrappers around the Rust binary, so their behavior should match the Rust CLI apart from wrapper-level process handling.

---

## Executive summary

The current Rust migration CLI is now close to the legacy Python CLI for the core flows:

- standard compressed MCP proxy over stdio,
- `low` / `medium` / `high` / `max` compression levels,
- remote streamable HTTP backends with native OAuth or explicit backend headers,
- CLI Mode,
- Code Mode for Python and TypeScript generated clients,
- Just Bash metadata mode,
- generated CLI help.

The most important parity fix is now implemented: the `get_tool_schema` wrapper description includes a compression-level-specific list of backend tools. This is critical because it gives the LLM compressed tool-selection context before it asks for a full schema.

Remaining non-P0 parity gaps are mostly polish:

- structured MCP `structuredContent` parity for some wrapper responses,
- shell completion generation,
- optional async Python generated clients,
- deeper generated CLI argument help for complex schemas such as nested objects and arrays.

---

## Top-level help comparison

### Legacy Python CLI

```text
Usage: mcp-compressor [OPTIONS] -- <command> [args]...

Options include compression level, include/exclude filters, CLI mode, Python mode,
TypeScript mode, Just Bash mode, cwd/env/header/timeout backend controls, logging,
and shell completion helpers.
```

### Current Rust / Python wrapper / TypeScript wrapper CLI

```text
Usage: mcp-compressor [OPTIONS] [COMMAND] -- <COMMAND>...

Options:
  -c, --compression <COMPRESSION>
  -n, --server-name <SERVER_NAME>
      --transform-mode <TRANSFORM_MODE>
      --cli-mode
      --code-mode <LANGUAGE>
      --just-bash-mode
      --include-tools <INCLUDE_TOOLS>
      --exclude-tools <EXCLUDE_TOOLS>
      --toonify
      --output-dir <OUTPUT_DIR>
      --transport <TRANSPORT>
      --port <PORT>
  -V, --version
  -h, --help
```

The Rust CLI intentionally separates frontend compressor options from backend server options. Backend options belong after `--`.

```bash
mcp-compressor [frontend options] -- <backend command or URL> [backend args]
```

Examples:

```bash
# Remote backend header after --
mcp-compressor -c medium -- https://mcp.example.com/v1/mcp -H "Authorization=Bearer ${TOKEN}"

# Local stdio backend cwd/env/timeout after --
mcp-compressor -c medium -- python server.py --cwd ./repo -e FOO=bar -t 30
```

This matches the separation-of-concerns model already used for `-H` and avoids mixing frontend compression controls with backend process/transport controls.

---

## Backend argument placement

| Backend concern | Legacy Python CLI | Current Rust CLI |
|---|---|---|
| HTTP headers | after `--` via `-H KEY=VALUE` | after `--` via `-H KEY=VALUE` |
| working directory | after `--` via `--cwd PATH` | after `--` via `--cwd PATH` |
| environment | after `--` via `-e KEY=VALUE` / `--env KEY=VALUE` | after `--` via `-e KEY=VALUE` / `--env KEY=VALUE` |
| timeout | after `--` via `-t SECONDS` / `--timeout SECONDS` | after `--` via `-t SECONDS` / `--timeout SECONDS` |

Top-level backend flags such as `mcp-compressor --cwd ./repo -- python server.py` are rejected in the Rust CLI. Use `mcp-compressor -- python server.py --cwd ./repo`.

---

## Compressed wrapper tool descriptions

The wrapper descriptions are the most important compression surface. The model sees these before it decides whether to call `get_tool_schema`.

### Low compression

```text
alpha_get_tool_schema

Get the input schema for a specific tool from the alpha toolset.

Available tools are:
<tool>echo(message): Echo a message from alpha.</tool>
<tool>add(a,b): Add two integers on alpha.</tool>
<tool>structured_data: Return structured alpha data.</tool>
```

### Medium compression

```text
alpha_get_tool_schema

Get the input schema for a specific tool from the alpha toolset.

Available tools are:
<tool>echo(message): Echo a message from alpha</tool>
<tool>add(a,b): Add two integers on alpha</tool>
<tool>structured_data: Return structured alpha data</tool>
```

### High compression

```text
alpha_get_tool_schema

Get the input schema for a specific tool from the alpha toolset.

Available tools are:
<tool>echo(message)</tool>
<tool>add(a,b)</tool>
<tool>structured_data</tool>
```

### Max compression

```text
alpha_get_tool_schema

Get the input schema for a specific tool from the alpha toolset.

Available tools are:
<tool>echo</tool>
<tool>add</tool>
<tool>structured_data</tool>
```

The level-specific listing is generated by the same compression engine that formats backend listings elsewhere, with a name-only fallback for `max`.

---

## Multi-server configuration parity

Legacy Python and the Rust CLI both support MCP JSON configuration containing multiple `mcpServers` entries. The current preferred Rust shape is explicit:

```bash
mcp-compressor --compression max --server-name suite --config ./mcp.json
```

with:

```json
{
  "mcpServers": {
    "alpha": {"command": "python", "args": ["alpha_server.py"]},
    "beta": {"command": "python", "args": ["beta_server.py"]}
  }
}
```

The expected compressed tool surface is:

```text
suite_alpha_get_tool_schema
suite_alpha_invoke_tool
suite_alpha_list_tools
suite_beta_get_tool_schema
suite_beta_invoke_tool
suite_beta_list_tools
```

The legacy CLI's option names differ (`--compression-level` instead of `--compression`) and its MCP config handling accepts a positional config value in some versions rather than the Rust CLI's explicit `--config`. The migration docs and tests lock the Rust public behavior while this report records the legacy naming difference.

## MCP tool surface examples

Using the alpha fixture backend with server name `alpha`:

### Medium

```text
alpha_get_tool_schema
alpha_invoke_tool
```

### Max

```text
alpha_get_tool_schema
alpha_invoke_tool
alpha_list_tools
```

This matches the intended architecture: lower compression levels expose schema and invocation wrappers; `max` additionally exposes `list_tools` because the wrapper descriptions are name-only.

---

## Schema response comparison

Calling:

```json
{
  "tool_name": "echo"
}
```

through `alpha_get_tool_schema` returns the full backend schema.

### Current Rust output shape

```text
<tool>echo(message): Echo a message from alpha.</tool>

{
  "type": "object",
  "properties": {
    "message": {
      "type": "string"
    }
  },
  "required": ["message"]
}
```

### Legacy Python output shape

```text
<tool>echo(message): Echo a message from alpha.</tool>

{
  "type": "object",
  "properties": {
    "message": {
      "type": "string"
    }
  },
  "required": ["message"]
}
```

The text response is now materially aligned for tool-selection and schema-fetch behavior. Legacy may still include structured MCP metadata in some responses; that remains a P1 parity item.

---

## Generated CLI help comparison

Generated CLI top-level help now follows the legacy style.

```text
alpha - the alpha toolset

When relevant, outputs from this CLI will prefer using the TOON format for more efficient representation of data.

USAGE:
  alpha <subcommand> [options]

SUBCOMMANDS:
  add                                 Add two integers on alpha
  echo                                Echo a message from alpha
  structured-data                     Return structured alpha data

Run 'alpha <subcommand> --help' for subcommand usage.
```

Generated subcommand help includes required/optional status, type, description, defaults, and enum values when present.

```text
alpha fetch

Fetch a URL.

USAGE:
  alpha fetch --url <value> --timeout <value> --method <value>

OPTIONS:
  --url                        <string>   required — URL to fetch.
  --timeout                    <integer>  optional — Timeout in seconds.; default: 30
  --method                     <string>   optional — HTTP method to use.; values: GET, POST; default: GET
```

Remaining generated CLI polish:

- richer display for nested object properties,
- array item details,
- min/max/pattern constraints,
- examples if schemas provide them.

---

## Code Mode comparison

The Rust CLI now uses the public umbrella term **Code Mode**:

```bash
mcp-compressor --code-mode python --server-name atlassian --output-dir ./generated-py -- https://mcp.atlassian.com/v1/mcp
mcp-compressor --code-mode typescript --server-name atlassian --output-dir ./generated-ts -- https://mcp.atlassian.com/v1/mcp
```

Deprecated aliases still work:

```bash
--python-mode
--typescript-mode
```

Generated clients call the local Rust proxy and expose backend tools directly, for example:

=== "Python"

    ```python
    import atlassian

    resources = atlassian.getAccessibleAtlassianResources()
    page = atlassian.getConfluencePage(page_id="123")
    ```

=== "TypeScript"

    ```ts
    import { getAccessibleAtlassianResources, getConfluencePage } from "./atlassian";

    const resources = await getAccessibleAtlassianResources();
    const page = await getConfluencePage("123");
    ```

Real-world Atlassian Code Mode tests import and execute generated Python and TypeScript clients against `https://mcp.atlassian.com/v1/mcp`.

---

## CLI Mode comparison

CLI Mode generates shell commands that hit the local proxy.

```bash
mcp-compressor --cli-mode --server-name atlassian -- https://mcp.atlassian.com/v1/mcp
```

Generated CLI usage:

```bash
atlassian --help
atlassian get-accessible-atlassian-resources
atlassian get-confluence-page --page-id 123
```

The current Rust generated CLI help is close to the legacy Python output and now includes more schema-derived argument detail.

---

## Just Bash comparison

The Rust core does not execute shell commands itself. Instead, it exposes Just Bash provider metadata to Python and TypeScript host libraries. Language hosts convert that metadata into Just Bash commands.

This is intentional: Rust owns compression/proxy routing; language hosts own Just Bash runtime integration.

Parity status:

- TypeScript helper: `createJustBashCommands(proxy)` implemented and tested.
- Python helper: `create_just_bash_commands(proxy)` implemented and tested.
- Rust CLI `--just-bash-mode` exposes the expected proxy/help surface.

---

## Atlassian MCP real-world parity

The real-world test suite covers:

- standard CLI compression levels against Atlassian MCP,
- custom server name,
- include filters,
- TOON output,
- CLI Mode,
- Code Mode Python and TypeScript,
- Just Bash mode surface,
- multi-server JSON config,
- port configuration,
- high-level Python SDK,
- high-level TypeScript SDK,
- generated Python/TypeScript clients.

Atlassian examples should prefer native OAuth:

```bash
mcp-compressor -c medium --server-name atlassian -- https://mcp.atlassian.com/v1/mcp
```

Explicit headers remain supported for CI/service-account cases and belong after `--`:

```bash
mcp-compressor -c medium --server-name atlassian -- https://mcp.atlassian.com/v1/mcp -H "Authorization=Basic ${ATLASSIAN_MCP_BASIC_TOKEN}"
```

---

## Remaining parity gaps

### P1

1. Add structured MCP `structuredContent` parity where legacy emits it.
2. Add shell completion support if still valuable.
3. Add richer generated CLI help for nested schemas and constraints.
4. Consider opt-in async Python Code Mode.
5. Expand Just Bash host parity tests to more real-world schemas.

### P2

1. Further compare exact logging output and log-level behavior.
2. Compare edge-case schema formatting for unions, `oneOf`, `anyOf`, and arrays.
3. Compare timeout semantics once request timeout enforcement is fully wired through remote and stdio paths.
