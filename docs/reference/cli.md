# CLI reference

Run:

```bash
mcp-compressor --help
```

## Common options

| Option | Description |
|---|---|
| `-c`, `--compression <level>` | Compression level: `low`, `medium`, `high`, `max`. Default: `medium`. |
| `-n`, `--server-name <name>` | Public name for a single backend server. |
| `-V`, `--version` | Print version information. |
| `--config <path>` | MCP config JSON file. Cannot be used with `--server-name`. |
| `--multi-server <name=command ...>` | Direct multi-server CLI configuration (repeatable). Format: `name=command [args...]`. |
| `--include-tools <a,b>` | Include only selected backend tools (comma-separated). |
| `--exclude-tools <a,b>` | Exclude selected backend tools (comma-separated). |
| `--toonify` | Convert JSON text outputs to TOON (Token-Oriented Object Notation) where applicable. |
| `--transport <stdio\|streamable-http>` | Frontend MCP transport. Default: `stdio`. |
| `--port <port>` | Port for streamable HTTP frontend. Use `0` for OS-selected. Default: `8000`. |
| `--cli-mode` | Generate a shell CLI and run a local proxy. |
| `--code-mode python` | Start Python Code Mode: generate a Python client and run a local proxy. |
| `--code-mode typescript` | Start TypeScript Code Mode: generate a TypeScript client and run a local proxy. |
| `--python-mode` | Deprecated alias for `--code-mode python`. |
| `--typescript-mode` | Deprecated alias for `--code-mode typescript`. |
| `--just-bash-mode` | Expose Just Bash command metadata and run a local proxy. |
| `--output-dir <path>` | Output directory for generated clients/scripts. |

Backend command or URL arguments come after `--`.

```bash
mcp-compressor [frontend options] -- <backend command or URL> [backend args]
```

Backend-specific options are intentionally parsed after `--` so frontend compression options stay separate from backend connection details.

| Backend arg | Description |
|---|---|
| `-H`, `--header KEY=VALUE` | HTTP header for remote streamable HTTP backends. Repeatable. |
| `--cwd <path>` | Working directory for stdio backend commands. |
| `-e`, `--env KEY=VALUE` | Environment variable for stdio backend commands. Repeatable. |
| `-t`, `--timeout <seconds>` | Backend connect/request timeout in seconds. |
| `--auth <auto\|oauth\|explicit-headers>` | Remote backend auth mode. `auto` tries OAuth if no explicit headers are set; `oauth` forces OAuth; `explicit-headers` skips OAuth. Default: `auto`. |

```bash
mcp-compressor -- https://mcp.example.com/v1/mcp -H "Authorization=******"
mcp-compressor -- python server.py --cwd ./repo -e FOO=bar -t 30
```

## OAuth cleanup

```bash
mcp-compressor clear-oauth
mcp-compressor clear-oauth <name-or-uri>
```

## LLM management

The `llm` subcommand manages the optional local LLM runtime and model assets used by LLM-assisted proxy features. All LLM assets are local-only and downloaded only on demand.

```bash
mcp-compressor llm <subcommand> [options]
```

### Subcommands

| Subcommand | Description |
|---|---|
| `status` | Show local llama-server and model installation status. |
| `pull` | Download and install llama-server and the configured model. |
| `remove` | Delete managed LLM runtime and model assets from the cache. |
| `test` | Download/install assets (if needed) and run a test prompt through the local model. |

### Options (shared by all LLM subcommands)

| Option | Description |
|---|---|
| `--model <ref>` | Model reference in `<org>/<repo>:<quant>` form. Default: `LiquidAI/LFM2.5-350M-GGUF:Q4_K_M`. |
| `--cache-dir <path>` | Override the mcp-compressor LLM cache directory. |
| `--llama-server <path>` | Explicit path to a llama-server binary. |

The `test` subcommand also accepts:

| Option | Description |
|---|---|
| `--prompt <text>` | Prompt to send to the local model. Default: `Say hello in one short sentence.` |

### Examples

```bash
# Check what's installed
mcp-compressor llm status

# Download the default model and runtime
mcp-compressor llm pull

# Use a specific model
mcp-compressor llm pull --model LiquidAI/LFM2.5-350M-GGUF:Q4_K_M

# Test inference
mcp-compressor llm test --prompt "List three short bullet points about MCP."

# Remove all managed assets
mcp-compressor llm remove
```

### Cache location

LLM assets are stored under:

- `$MCP_COMPRESSOR_CACHE_DIR` if set, otherwise
- `$XDG_CACHE_HOME/mcp-compressor/` on Linux,
- `~/Library/Caches/mcp-compressor/` on macOS,
- `%LOCALAPPDATA%\mcp-compressor\` on Windows.
