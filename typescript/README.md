# @atlassian/mcp-compressor (TypeScript)

A TypeScript MCP proxy that wraps one or more MCP servers and reduces the token footprint exposed to LLMs.

## How it works

mcp-compressor connects to upstream MCP servers (via stdio, HTTP, or SSE) and replaces their full tool catalogs with a compressed interface. Instead of exposing every tool individually, it provides:

- **`get_tool_schema(tool_name)`** — returns the full schema for a specific tool on demand
- **`invoke_tool(tool_name, tool_input)`** — calls an upstream tool
- **`list_tools()`** — lists available tool names (only at `max` compression)

This dramatically reduces the token cost of tool descriptions in LLM context windows while preserving full tool functionality.

### Compression levels

| Level | What the LLM sees |
|---|---|
| `low` | Tool name, parameters, and full description |
| `medium` (default) | Tool name, parameters, and first sentence of description |
| `high` | Tool name and parameters only |
| `max` | Tool names only (requires `list_tools` call to discover) |

### Three modes

| Mode | Tools exposed | How the LLM invokes tools |
|---|---|---|
| **Compressed** (default) | `get_tool_schema` + `invoke_tool` | Via MCP tool calls |
| **CLI** | Per-server `_help` tools | Via bash CLI commands (bridge + generated scripts) |
| **Bash** | Per-server `_help` tools + `bash` tool | Via a sandboxed [just-bash](https://www.npmjs.com/package/just-bash) shell |

## Installation

```bash
npm install @atlassian/mcp-compressor

# Optional: for bash mode
npm install just-bash
```

## STDIO MCP proxy

The simplest way to use mcp-compressor is as a CLI that wraps another MCP server and exposes a compressed proxy over stdio.

### Compressed mode (default)

```bash
# Wrap a remote MCP server
bun cli -- https://mcp.atlassian.com/v1/mcp

# Wrap a local stdio server
bun cli --server-name filesystem -- npx -y @modelcontextprotocol/server-filesystem .

# With options
bun cli -c high --server-name atlassian --toonify -- https://mcp.atlassian.com/v1/mcp

# Multi-server via MCP config JSON
bun cli -- '{"mcpServers":{"jira":{"url":"https://jira-mcp.example.com"},"confluence":{"command":"node","args":["confluence-server.js"]}}}'
```

### CLI mode

CLI mode generates shell scripts for each backend server so the LLM (or user) can invoke tools directly from bash. The MCP proxy exposes per-server help tools that describe the available commands.

```bash
# Start CLI mode
bun cli --cli-mode --server-name atlassian -- https://mcp.atlassian.com/v1/mcp

# In another terminal, use the generated CLI:
atlassian --help
atlassian search-confluence --query oauth
atlassian get-jira-issue --issue-url https://jira.example.com/browse/PROJ-123
```

TOON output formatting is automatically enabled in CLI mode.

### Bash mode

Bash mode registers all backend tools as custom commands in a sandboxed just-bash shell, then exposes a single `bash` MCP tool plus per-server help tools. The LLM can run MCP tools alongside standard Unix utilities, including pipes and composition.

```bash
bun cli --just-bash --server-name atlassian -- https://mcp.atlassian.com/v1/mcp
```

The LLM then sees:
- `bash` — execute commands in the sandboxed shell
- `atlassian_help` — lists available atlassian subcommands

Example commands the LLM can run via the bash tool:
```bash
atlassian search-issues --jql "project=PROJ" | jq '.issues[].key'
atlassian get-page --page-id 12345 | grep "summary"
echo "hello" | grep hello
```

### OAuth

For remote OAuth backends, the CLI handles the authorization flow automatically — it opens the browser, completes the code exchange, and persists tokens for future sessions.

```bash
# Clear cached OAuth state
bun cli clear-oauth https://mcp.atlassian.com/v1/mcp
```

## In-process usage (CompressorClient)

For TypeScript applications and agent frameworks, `CompressorClient` provides a single unified interface for all modes — no subprocess needed.

### Compressed mode

```typescript
import { CompressorClient } from '@atlassian/mcp-compressor';

const client = new CompressorClient({
  servers: {
    jira: { url: 'https://jira-mcp.example.com' },
    confluence: { command: 'node', args: ['confluence-server.js'] },
  },
  compressionLevel: 'medium',
});

await client.connect();
const tools = await client.getTools();
// → { jira_get_tool_schema, jira_invoke_tool, confluence_get_tool_schema, confluence_invoke_tool }

// Use with any AI SDK-compatible framework
const schema = await tools.jira_get_tool_schema.execute({ tool_name: 'search_issues' });
const result = await tools.jira_invoke_tool.execute({
  tool_name: 'search_issues',
  tool_input: { query: 'oauth' },
});

await client.close();
```

### CLI mode

```typescript
const client = new CompressorClient({
  servers: { atlassian: { url: 'https://mcp.atlassian.com/v1/mcp' } },
  mode: 'cli',
});

await client.connect();
const tools = await client.getTools();
// → { atlassian_help }
// Side effect: HTTP bridge started, shell script generated

// client.scripts has info about generated scripts
for (const script of client.scripts) {
  console.log(`Run '${script.cliName} --help' for usage`);
}

await client.close();
```

### Bash mode

```typescript
const client = new CompressorClient({
  servers: {
    jira: { url: 'https://jira-mcp.example.com' },
    confluence: { command: 'node', args: ['confluence-server.js'] },
  },
  mode: 'bash',
});

await client.connect();
const tools = await client.getTools();
// → { bash, jira_help, confluence_help }

// Execute commands via the bash tool
const result = await tools.bash.execute({ command: 'jira search-issues --query oauth' });

// Access the Bash instance directly
const execResult = await client.bash!.exec('jira search-issues --query test | jq .issues');

await client.close();
```

### Bash mode with a pre-existing Bash instance

If your application already has a `Bash` instance (e.g. with its own custom commands), you can inject it:

```typescript
import { Bash } from 'just-bash';

const existingBash = new Bash({ customCommands: [myCustomCommand] });

const client = new CompressorClient({
  servers: { atlassian: { url: 'https://mcp.atlassian.com/v1/mcp' } },
  mode: 'bash',
  bash: { bash: existingBash },
});

await client.connect();
const tools = await client.getTools();
// → { bash, atlassian_help }
// MCP commands are registered into existingBash via registerCommand()
```

### Server configuration formats

`CompressorClient` accepts several formats for the `servers` option:

```typescript
// Named servers map (recommended for multi-server)
new CompressorClient({
  servers: {
    jira: { url: 'https://jira-mcp.example.com' },
    filesystem: { command: 'npx', args: ['-y', '@modelcontextprotocol/server-filesystem', '.'] },
  },
});

// Single BackendConfig
new CompressorClient({
  servers: { type: 'http', url: 'https://mcp.example.com' },
});

// URL string
new CompressorClient({
  servers: 'https://mcp.example.com',
});

// MCP config JSON string
new CompressorClient({
  servers: '{"mcpServers":{"jira":{"url":"https://jira-mcp.example.com"}}}',
});
```

### Escape hatches

```typescript
// Access individual runtimes
const jiraRuntime = client.getRuntime('jira');
await jiraRuntime.invokeTool('search_issues', { query: 'bug' });

// List all server names
console.log(client.serverNames); // ['jira', 'confluence']

// Access all runtimes
for (const runtime of client.runtimes) {
  console.log(runtime.serverName, await runtime.listToolNames());
}
```

## Development

### Setup

```bash
cd typescript
bun install
```

### Commands

```bash
bun run test          # run tests with vitest
bun run check         # lint + format + typecheck + test
bun run lint          # lint with oxlint
bun run format        # format with oxfmt
bun run format:check  # check formatting
bun run build         # compile to dist/
bun cli               # run the CLI directly from source
```

### Toolchain

| Tool | Purpose |
|---|---|
| [Bun](https://bun.sh/) | Package manager and runtime |
| [TypeScript](https://www.typescriptlang.org/) | Type checking |
| [Vitest](https://vitest.dev/) | Test runner |
| [oxlint](https://oxc.rs/docs/guide/usage/linter) | Linter |
| [oxfmt](https://oxc.rs/docs/guide/usage/formatter) | Formatter |
| [Commander](https://github.com/tj/commander.js) | CLI argument parsing |

## Publishing

Published as `@atlassian/mcp-compressor` via Atlassian Artifactory → npmJS.

```bash
npm install @atlassian/mcp-compressor
```
