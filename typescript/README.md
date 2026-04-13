# @atlassian/mcp-compressor (TypeScript)

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
- support single-server and multi-server MCP config JSON input
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
- CLI mode with generated shell scripts for direct tool invocation
  - single-server and multi-server CLI mode
  - per-server generated scripts for multi-server configs

## Not yet at Python parity

The TypeScript package is intentionally narrower than the Python implementation for now.

Notable differences today:
- no prompt passthrough yet
- no resource passthrough beyond the hidden uncompressed-tools resource
- no OS keyring-backed encryption-key storage yet
- release and packaging flow are newer than the Python side

## Package structure

- `src/index.ts` — public exports and top-level server creation helpers
- `src/server.ts` — compressed FastMCP server implementation
- `src/backend-client.ts` — upstream MCP client lifecycle and retry behavior
- `src/oauth.ts` — persistent OAuth provider and state handling
- `src/config.ts` — single and multi-server MCP config JSON parsing
- `src/formatting.ts` — compressed tool descriptions and TOON conversion
- `src/cli.ts` — CLI entrypoint (uses [commander](https://github.com/tj/commander.js))
- `src/cli_mode.ts` — CLI mode session management
- `src/cli_bridge.ts` — local HTTP bridge for CLI mode
- `src/cli_script.ts` — generated shell script management
- `src/cli_tools.ts` — CLI argument parsing and help formatting
- `tests/` — test coverage using [vitest](https://vitest.dev/)

## Install dependencies

```bash
cd typescript
bun install
```

## Run checks

```bash
bun run test          # run tests with vitest
bun run check         # type-check with tsc
bun run lint          # lint with oxlint
bun run format:check  # check formatting with oxfmt
```

## Format code

```bash
bun run format
```

## Build

```bash
bun run build
```

## Library usage

### Start an MCP server

```ts
import { startCompressorServer } from '@atlassian/mcp-compressor';

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
import { initializeCompressedFunctionToolset } from '@atlassian/mcp-compressor';

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
import { startCompressorServer } from '@atlassian/mcp-compressor';

await startCompressorServer({
  backend: '{"mcpServers":{"atlassian":{"url":"https://mcp.atlassian.com/v1/mcp"}}}',
  compressionLevel: 'high',
});
```

## CLI usage

The TypeScript package includes a CLI entrypoint for normal MCP server mode:

```bash
bun run dist/cli.js https://mcp.atlassian.com/v1/mcp
```

For local development, the easiest way to run it is:

```bash
bun run dev -- https://mcp.atlassian.com/v1/mcp
```

For remote OAuth backends, the CLI chooses an available `localhost` loopback callback port automatically, opens the browser, and completes the authorization-code exchange before connecting.
If the browser does not open, the CLI also prints the authorization URL so you can open it manually.

### CLI mode

CLI mode starts a local bridge plus a generated shell command for the backend tools.

Remote backend example:

```bash
bun run dev -- --cli-mode --server-name atlassian -- https://mcp.atlassian.com/v1/mcp
```

Single-server JSON config example:

```bash
bun run dev -- --cli-mode -- '{"mcpServers":{"atlassian":{"url":"https://mcp.atlassian.com/v1/mcp"}}}'
```

Stdio backend example:

```bash
bun run dev -- --cli-mode --server-name filesystem -- npx -y @modelcontextprotocol/server-filesystem .
```

Multi-server JSON config example:

```bash
bun run dev -- --cli-mode -- '{"mcpServers":{"weather":{"command":"uvx","args":["mcp-weather"]},"calendar":{"command":"uvx","args":["mcp-calendar"]}}}'
```

When CLI mode starts, it prints the generated script path(s). Keep the process running, then invoke the generated command(s), for example:

```bash
# Single-server CLI mode
atlassian --help
atlassian search-confluence --query oauth

# Multi-server CLI mode generates one script per server
weather --help
calendar --help
```

Clear cached OAuth state for a remote backend:

```bash
bun run dist/cli.js clear-oauth https://mcp.atlassian.com/v1/mcp
```

### just-bash mode

just-bash mode registers all backend MCP tools as custom commands in a [just-bash](https://www.npmjs.com/package/just-bash) sandboxed shell environment, then exposes a single `bash` MCP tool. The agent can run standard Unix utilities and MCP tools in the same shell, including pipes and composition.

Requires the `just-bash` package to be installed (optional peer dependency).

#### CLI usage

```bash
# Install just-bash first
npm install just-bash

# Single-server
bun run dev -- --just-bash --server-name atlassian -- https://mcp.atlassian.com/v1/mcp

# Multi-server JSON config
bun run dev -- --just-bash -- '{"mcpServers":{"weather":{"command":"uvx","args":["mcp-weather"]},"calendar":{"command":"uvx","args":["mcp-calendar"]}}}'
```

The agent then sees a single `bash` tool. MCP tools are available as parent commands with subcommands:

```bash
# List available subcommands for a server
atlassian --help

# Invoke a tool
atlassian search-confluence --query oauth

# Pipe MCP output through standard Unix tools
atlassian search-issues --jql "project=PROJ" | jq '.issues[].key'

# Standard bash commands work alongside MCP commands
echo "hello" | grep hello
```

#### In-process (library) usage

For agent frameworks, use the `@atlassian/mcp-compressor/bash` entrypoint to create just-bash commands from a `CompressorRuntime`:

```ts
import { createCompressorRuntime } from '@atlassian/mcp-compressor';
import { createBashCommand, buildBashToolDescription } from '@atlassian/mcp-compressor/bash';
import { Bash } from 'just-bash';

// Connect to the backend
const runtime = createCompressorRuntime({
  backend: { type: 'http', url: 'https://mcp.atlassian.com/v1/mcp' },
  serverName: 'atlassian',
  toonify: true,
});
await runtime.connect();

// Create a parent command with subcommands for each MCP tool
const tools = await runtime.listUncompressedTools();
const command = createBashCommand(runtime, tools);

// Create a Bash instance with the command
const bash = new Bash({ customCommands: [command] });

// Build the tool description for the LLM
const description = buildBashToolDescription([
  { serverName: 'atlassian', command, tools },
]);

// Use as a single AI SDK tool
const bashTool = {
  description,
  parameters: z.object({ command: z.string() }),
  execute: async (args) => {
    const result = await bash.exec(args.command);
    return result.stdout || `Exit ${result.exitCode}: ${result.stderr}`;
  },
};
```

Multiple servers can be combined into a single `Bash` instance — each becomes a separate parent command:

```ts
const allCommands = runtimes.map(({ runtime, tools }) => createBashCommand(runtime, tools));
const bash = new Bash({ customCommands: allCommands });
// Agent can now run: `atlassian search-issues --jql "..."` and `github list-repos --org acme`
```

## Development toolchain

| Tool | Purpose |
|---|---|
| [bun](https://bun.sh/) | Package manager and runtime |
| [TypeScript](https://www.typescriptlang.org/) | Type checking (`tsc`) |
| [vitest](https://vitest.dev/) | Test runner |
| [oxlint](https://oxc.rs/docs/guide/usage/linter) | Linter |
| [oxfmt](https://oxc.rs/docs/guide/usage/formatter) | Formatter |
| [commander](https://github.com/tj/commander.js) | CLI argument parsing |

## Publishing

This package is published as `@atlassian/mcp-compressor` through Atlassian Artifactory forwarding to npmJS.

```bash
npm install @atlassian/mcp-compressor
```

Relevant files:
- `package.json` — package metadata and `publishConfig`
- `.github/workflows/publish-typescript.yml` — publish workflow
- `.github/workflows/main.yml` — CI checks for the TypeScript package

The GitHub Actions publish workflow uses the `atlassian-labs/artifact-publish-token` action with OIDC to authenticate against Atlassian Artifactory (`npm-public`), which then forwards the package to npmJS.

## Relationship to the Python implementation

The Python package remains the more mature implementation today, especially around:
- broader pass-through behavior
- packaging polish

The TypeScript package is intended to converge on the same user-facing product model over time while remaining idiomatic for Node.js consumers.
