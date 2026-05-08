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
mcp-compressor -c medium --server-name atlassian -- https://mcp.atlassian.com/v1/mcp \
  -H "Authorization=Basic ${ATLASSIAN_MCP_BASIC_TOKEN}"
```

## Streamable HTTP frontend

```bash
mcp-compressor -c medium --transport streamable-http --port 9000 -- python server.py
```

## CLI mode

CLI mode writes a shell script that calls a local Rust proxy.

```bash
mcp-compressor --cli-mode --server-name atlassian -- https://mcp.atlassian.com/v1/mcp \
  -H "Authorization=Basic ${ATLASSIAN_MCP_BASIC_TOKEN}"
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

## Generated Python and TypeScript clients

```bash
mcp-compressor --python-mode --server-name atlassian --output-dir ./generated-py -- https://mcp.atlassian.com/v1/mcp \
  -H "Authorization=Basic ${ATLASSIAN_MCP_BASIC_TOKEN}"

mcp-compressor --typescript-mode --server-name atlassian --output-dir ./generated-ts -- https://mcp.atlassian.com/v1/mcp \
  -H "Authorization=Basic ${ATLASSIAN_MCP_BASIC_TOKEN}"
```

## Just Bash mode

```bash
mcp-compressor --just-bash-mode --server-name atlassian -- https://mcp.atlassian.com/v1/mcp \
  -H "Authorization=Basic ${ATLASSIAN_MCP_BASIC_TOKEN}"
```

Just Bash mode exposes provider metadata for language hosts to register commands. See [Just Bash](just-bash.md).
