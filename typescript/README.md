# mcp-compressor (TypeScript)

This directory contains the TypeScript implementation of `mcp-compressor`.

It is designed to live beside the Python implementation so that:
- Python users can depend on the Python package
- TypeScript and Node.js users can depend on the TypeScript package
- both implementations can evolve toward the same compression model and product behavior

## Current scope

The TypeScript package currently focuses on the core compressed proxy behavior:

- create a FastMCP server that exposes compressed wrapper tools
- connect to a backend MCP server over:
  - stdio
  - streamable HTTP
  - SSE
- support single-server MCP config JSON input
- expose:
  - `get_tool_schema`
  - `invoke_tool`
  - `list_tools` at `max` compression
  - `compressor://uncompressed-tools`
- support include/exclude tool filtering
- optionally convert JSON text output to TOON format
- support persistent OAuth for remote backends, including:
  - encrypted local storage
  - discovery-state persistence
  - stale-auth retry after clearing cached credentials
  - `clear-oauth`

## Not yet at Python parity

The TypeScript package is intentionally narrower than the Python implementation for now.

Notable differences today:
- no CLI mode equivalent yet
- no prompt passthrough yet
- no resource passthrough beyond the hidden uncompressed-tools resource
- no OS keyring-backed encryption-key storage yet
- release and packaging flow are newer than the Python side

## Package structure

- `src/index.ts` — public exports and top-level server creation helpers
- `src/server.ts` — compressed FastMCP server implementation
- `src/backend-client.ts` — upstream MCP client lifecycle and retry behavior
- `src/oauth.ts` — persistent OAuth provider and state handling
- `src/config.ts` — single-server MCP config JSON parsing
- `src/formatting.ts` — compressed tool descriptions and TOON conversion
- `src/cli.ts` — minimal CLI entrypoint
- `tests/` — focused Node test coverage

## Install dependencies

```bash
cd typescript
npm install
```

## Run checks

```bash
npm test
npm run check
```

## Build

```bash
npm run build
```

## Library usage

### Start an MCP server

```ts
import { startCompressorServer } from 'mcp-compressor';

await startCompressorServer({
  backend: {
    type: 'http',
    url: 'https://mcp.atlassian.com/v1/mcp',
  },
  compressionLevel: 'medium',
  serverName: 'atlassian',
  toonify: true,
  start: {
    transportType: 'stdio',
  },
});
```

### Use it in-process without starting a subprocess

For agent frameworks like Mastra, you can use the compression/runtime layer directly and skip the FastMCP server transport entirely:

```ts
import { initializeCompressedFunctionToolset } from 'mcp-compressor';

const { runtime, toolset } = await initializeCompressedFunctionToolset({
  backend: {
    type: 'http',
    url: 'https://mcp.atlassian.com/v1/mcp',
  },
  compressionLevel: 'medium',
  serverName: 'atlassian',
});

const schema = await toolset.atlassian_get_tool_schema({ tool_name: 'search_confluence' });
const result = await toolset.atlassian_invoke_tool({
  tool_name: 'search_confluence',
  tool_input: { query: 'oauth' },
});

await runtime.close();
```

If you need more direct control, you can also use `initializeCompressorRuntime(...)` and call:
- `runtime.getToolSchema(...)`
- `runtime.invokeTool(...)`
- `runtime.listToolNames()`
- `runtime.listUncompressedTools()`
- `runtime.buildCompressedDescription()`

You can also pass a single-server MCP config JSON string as the backend:

```ts
import { startCompressorServer } from 'mcp-compressor';

await startCompressorServer({
  backend: '{"mcpServers":{"atlassian":{"url":"https://mcp.atlassian.com/v1/mcp"}}}',
  compressionLevel: 'high',
});
```

## CLI usage

The TypeScript package includes a CLI entrypoint for normal MCP server mode:

```bash
node dist/cli.js https://mcp.atlassian.com/v1/mcp
```

For local development, the easiest way to run it is:

```bash
npm run dev -- https://mcp.atlassian.com/v1/mcp
```

For remote OAuth backends, the CLI chooses an available `localhost` loopback callback port automatically, opens the browser, and completes the authorization-code exchange before connecting.
If the browser does not open, the CLI also prints the authorization URL so you can open it manually.

### CLI mode

CLI mode starts a local bridge plus a generated shell command for the backend tools.

Remote backend example:

```bash
npm run dev -- --cli-mode --server-name atlassian -- https://mcp.atlassian.com/v1/mcp
```

Single-server JSON config example:

```bash
npm run dev -- --cli-mode -- '{"mcpServers":{"atlassian":{"url":"https://mcp.atlassian.com/v1/mcp"}}}'
```

Stdio backend example:

```bash
npm run dev -- --cli-mode --server-name filesystem -- npx -y @modelcontextprotocol/server-filesystem .
```

When CLI mode starts, it prints the generated script path. Keep the TS process running, then invoke the generated command, for example:

```bash
atlassian --help
atlassian search-confluence --query oauth
```

Clear cached OAuth state for a remote backend:

```bash
node dist/cli.js clear-oauth https://mcp.atlassian.com/v1/mcp
```

## Publishing

This package is configured to publish as unscoped `mcp-compressor` through Atlassian Artifactory forwarding to npm.

Relevant files:
- `package.json` — package metadata and `publishConfig`
- `.github/workflows/publish-typescript.yml` — publish workflow
- `.github/workflows/main.yml` — CI checks for the TypeScript package

## Relationship to the Python implementation

The Python package remains the more mature implementation today, especially around:
- CLI mode
- broader pass-through behavior
- packaging polish

The TypeScript package is intended to converge on the same user-facing product model over time while remaining idiomatic for Node.js consumers.
