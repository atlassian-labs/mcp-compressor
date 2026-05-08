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

CLI mode writes a shell script that calls a local Rust proxy.

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

Code Mode generates Python or TypeScript functions for backend MCP tools while keeping the local Rust proxy alive.

```bash
mcp-compressor --code-mode python --server-name atlassian --output-dir ./generated-py -- https://mcp.atlassian.com/v1/mcp

mcp-compressor --code-mode typescript --server-name atlassian --output-dir ./generated-ts -- https://mcp.atlassian.com/v1/mcp
```

See [Code Mode and generated clients](generated-clients.md) for examples of generated functions and agent usage.

## Just Bash mode

```bash
mcp-compressor --just-bash-mode --server-name atlassian -- https://mcp.atlassian.com/v1/mcp
```

Just Bash mode exposes provider metadata for language hosts to register commands. See [Just Bash](just-bash.md).
