# CLI reference

Run:

```bash
mcp-compressor --help
```

## Common options

| Option | Description |
|---|---|
| `-c`, `--compression <level>` | Compression level: `low`, `medium`, `high`, `max`. |
| `-n`, `--server-name <name>` | Public name for a single backend server. |
| `--cwd <path>` | Working directory for stdio backend commands. |
| `-e`, `--env KEY=VALUE` | Environment variable for stdio backend commands. Repeatable. |
| `-t`, `--timeout <seconds>` | Backend connect/request timeout in seconds. |
| `-V`, `--version` | Print version information. |
| `--config <path>` | MCP config JSON file. |
| `--multi-server <name=command ...>` | Direct multi-server CLI configuration. |
| `--include-tools <a,b>` | Include only selected backend tools. |
| `--exclude-tools <a,b>` | Exclude selected backend tools. |
| `--toonify` | Convert JSON text outputs to TOON where applicable. |
| `--transport <stdio|streamable-http>` | Frontend MCP transport. |
| `--port <port>` | Port for streamable HTTP frontend. Use `0` for OS-selected. |
| `--cli-mode` | Generate a shell CLI and run a local proxy. |
| `--code-mode python` | Start Python Code Mode: generate a Python client and run a local proxy. |
| `--code-mode typescript` | Start TypeScript Code Mode: generate a TypeScript client and run a local proxy. |
| `--python-mode` | Deprecated alias for `--code-mode python`. |
| `--typescript-mode` | Deprecated alias for `--code-mode typescript`. |
| `--just-bash-mode` | Expose Just Bash command metadata and run a local proxy. |
| `--output-dir <path>` | Output directory for generated clients/scripts. |

Backend command or URL arguments come after `--`.

```bash
mcp-compressor [options] -- <backend command or URL> [backend args]
```

## Header args for remote URLs

```bash
mcp-compressor -- https://mcp.example.com/v1/mcp -H "Authorization=Bearer ${TOKEN}"
```

## OAuth cleanup

```bash
mcp-compressor clear-oauth
mcp-compressor clear-oauth <name-or-uri>
```
