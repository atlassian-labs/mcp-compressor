# CLI usage

The CLI can serve a compressed MCP server or start helper modes that create local command/code clients.

## Standard MCP proxy

```bash
mcp-compressor -c medium -- python server.py
```

Compression level shorthand:

```bash
mcp-compressor -c low -- python server.py
mcp-compressor -c medium -- python server.py
mcp-compressor -c high -- python server.py
mcp-compressor -c max -- python server.py
```

## Custom server name

```bash
mcp-compressor -c medium --server-name atlassian -- https://mcp.atlassian.com/v1/mcp
```

The first run opens a browser for OAuth if no stored credentials exist.

## Streamable HTTP frontend

```bash
mcp-compressor -c medium --transport streamable-http --port 9000 -- python server.py
```

## CLI mode

CLI mode writes a shell script that calls the live local session created by `mcp-compressor`.

```bash
mcp-compressor --cli-mode --server-name atlassian -- https://mcp.atlassian.com/v1/mcp
```

Then run commands through the generated script:

```bash
atlassian --help
atlassian get-accessible-atlassian-resources
```

Use an explicit output directory:

```bash
mcp-compressor --cli-mode --server-name alpha --output-dir ./bin -- python server.py
```

## Code Mode

Code Mode generates Python or TypeScript functions for backend MCP tools while keeping the local `mcp-compressor` session alive. By default, generated Code Mode files are written under `./dist` in the current working directory.

```bash
mcp-compressor --code-mode python --server-name atlassian -- https://mcp.atlassian.com/v1/mcp

mcp-compressor --code-mode typescript --server-name atlassian -- https://mcp.atlassian.com/v1/mcp
```

See [Code Mode and generated clients](generated-clients.md) for examples of generated functions and agent usage.

## Just Bash mode

```bash
mcp-compressor --just-bash-mode --server-name atlassian -- https://mcp.atlassian.com/v1/mcp
```

Just Bash mode lets language hosts register MCP tools as shell-style commands. See [Just Bash](just-bash.md).

## LLM management

`mcp-compressor` can optionally use a small local LLM for proxy-layer assistance. The `llm` subcommand manages the required runtime and model assets.

```bash
# Check installation status
mcp-compressor llm status

# Download the managed llama-server runtime and default model
mcp-compressor llm pull

# Run a quick inference test
mcp-compressor llm test

# Remove all managed runtime and model assets
mcp-compressor llm remove
```

All assets are stored locally. Nothing is downloaded unless you explicitly run `mcp-compressor llm pull` or `mcp-compressor llm test`. The standard proxy mode (`mcp-compressor -c medium -- ...`) is unaffected when no LLM assets are installed.

See [CLI reference](../reference/cli.md#llm-management) for all options.
