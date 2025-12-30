# mcp-compressor

[![Release](https://img.shields.io/github/v/release/atlassian/mcp-compressor)](https://img.shields.io/github/v/release/atlassian/mcp-compressor)
[![Build status](https://img.shields.io/github/actions/workflow/status/atlassian/mcp-compressor/main.yml?branch=main)](https://github.com/atlassian/mcp-compressor/actions/workflows/main.yml?query=branch%3Amain)
[![codecov](https://codecov.io/gh/atlassian/mcp-compressor/branch/main/graph/badge.svg)](https://codecov.io/gh/atlassian/mcp-compressor)
[![Commit activity](https://img.shields.io/github/commit-activity/m/atlassian/mcp-compressor)](https://img.shields.io/github/commit-activity/m/atlassian/mcp-compressor)
[![License](https://img.shields.io/github/license/atlassian/mcp-compressor)](https://img.shields.io/github/license/atlassian/mcp-compressor)

An MCP server wrapper for reducing tokens consumed by MCP tools.

- **Github repository**: <https://github.com/atlassian/mcp-compressor/>
- **Documentation** <https://atlassian.github.io/mcp-compressor/>

## Overview

MCP Compressor is a proxy server that wraps existing [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) servers and compresses their tool descriptions to significantly reduce token consumption. Instead of exposing all tools with full schemas directly to language models, it provides a two-step interface:

1. **`get_tool_schema(tool_name)`** - Retrieve the full schema for a specific tool when needed
2. **`invoke_tool(tool_name, tool_input)`** - Execute a tool with the provided arguments

This approach dramatically reduces the number of tokens sent in the initial context while maintaining full functionality.

## Features

- **Token Reduction**: Compress tool descriptions by up to 90% depending on compression level
- **Multiple Compression Levels**: Choose between `low`, `medium`, `high`, or `max`
- **Universal Compatibility**: Works with any MCP server (stdio, HTTP, SSE)
- **Zero Functionality Loss**: All tools remain fully accessible through the wrapper interface
- **Easy Integration**: Drop-in replacement for existing MCP servers

## Installation

Install using pip or uv:

```bash
pip install mcp-compressor
# or
uv pip install mcp-compressor
```

## Quick Start

### Basic Usage

Wrap any MCP server by providing its command or URL:

```bash
# Wrap a stdio MCP server
uvx mcp-compressor uvx mcp-server-fetch

# Wrap a remote HTTP MCP server
uvx mcp-compressor https://example.com/server/mcp

# Wrap a remote SSE MCP server
uvx mcp-compressor https://example.com/server/sse
```

See `uvx mcp-compressor --help` for detailed documentation on available arguments.

### Compression Levels

Control how much compression to apply with the `--compression-level` or `-c` flag:

```bash
# Low
mcp-compressor uvx mcp-server-fetch -c low

# Medium (default)
mcp-compressor uvx mcp-server-fetch -c medium

# High
mcp-compressor uvx mcp-server-fetch -c high

# Max
mcp-compressor uvx mcp-server-fetch -c max
```

### Advanced Options

#### Stdio Servers

```bash
# Set working directory
mcp-compressor uvx mcp-server-fetch --cwd /path/to/dir

# Pass environment variables (supports environment variable expansion)
mcp-compressor uvx mcp-server-fetch \
  -e API_KEY=${MY_API_KEY} \
  -e DEBUG=true
```

#### Remote Servers (HTTP/SSE)

```bash
# Add custom headers
mcp-compressor https://api.example.com/mcp \
  -H "Authorization=Bearer ${TOKEN}" \
  -H "X-Custom-Header=value"

# Set timeout (default: 10 seconds)
mcp-compressor https://api.example.com/mcp \
  --timeout 30
```

#### Custom Server Names

When running multiple MCP servers through mcp-compressor, you can add custom prefixes to the wrapper tool names to avoid conflicts:

```bash
# Without server name - tools will be: get_tool_schema, invoke_tool
mcp-compressor uvx mcp-server-fetch

# With server name - tools will be: github_get_tool_schema, github_invoke_tool
mcp-compressor https://api.githubcopilot.com/mcp/ --server-name github

# Special characters are automatically sanitized
mcp-compressor uvx mcp-server-fetch --server-name "My Server!"
  # Results in: My_Server__get_tool_schema, My_Server__invoke_tool
```

#### Logging

```bash
# Set log level
mcp-compressor uvx mcp-server-fetch --log-level debug
mcp-compressor uvx mcp-server-fetch -l warning
```

## How It Works

The MCP Compressor acts as a transparent proxy between your LLM client and the underlying MCP server:

```
┌─────────┐      ┌──────────────────┐      ┌────────────┐
│   MCP   │ <──> │  MCP Compressor  │ <──> │ MCP Server │
│  Client │      │  (This Package)  │      │            │
└─────────┘      └──────────────────┘      └────────────┘
```

Instead of seeing all tools with full schemas (which are often thousands of tokens), the LLM sees just:

```
Available tools:
<tool>search_web(query, max_results): Search the web for information</tool>
<tool>get_weather(location, units): Get current weather for a location</tool>
<tool>send_email(to, subject, body): Send an email message</tool>
```

When the LLM needs to use a tool, it first calls `get_tool_schema(tool_name)` to retrieve the full schema, then `invoke_tool(tool_name, tool_input)` to execute it.

## Compression Level Details

| Level | Description | Use Case |
|-------|-------------|----------|
| `max` | Maximum compression - exposes `list_tools()` function | Maximum token savings. Good for (1) MCP servers you want to provide to your agent but expect tools to be used rarely and (2) for servers with a very large number of tools |
| `high` | Only tool name and parameter names | Maximum token savings, best for large toolsets |
| `medium` (default) | First sentence of each description | Balanced approach, good for most cases. |
| `low` | Complete tool descriptions | For tools that are unusual and not intuitive for the agent to understand and use. Using a lower level of compression in these cases provides more context to the LLM on the purpose of the tools and how they relate to each other. |

The best choice of compression level will depend on a number of factors, including:
1. The number of tools in the MCP server - more tools, use more compression.
1. How frequently the tools are expected to be used - if tools from a compressed server are rarely used, compress them more to prevent eating up tokens for nothing.
1. How unusual or complex the tools are - simpler tools can be compressed more heavily with little downsize. Consider a simple `bash` tool with a single input argument `command`. Any modern LLM will understand exactly how to use it after seeing just the tool name and the name of the argument, so unless there is unexpected internal logic within the tool, aggressive compression can be used with little downside.

## Configuration with MCP JSON file

To configure mcp-compressor in an MCP JSON configuration file, use the following pattern:

```json
{
  "mcpServers": {
    "compressed-github": {
      "command": "mcp-compressor",
      "args": [
        "https://api.githubcopilot.com/mcp/",
        "--header",
        "Authorization=Bearer ${GH_PAT}",
        "--server-name",
        "github"
      ],
    },
    "compressed-fetch": {
      "command": "mcp-compressor",
      "args": [
        "uvx",
        "mcp-server-fetch",
        "--server-name",
        "fetch"
      ],
    }
  }
}
```

This configuration will create tools named `github_get_tool_schema`, `github_invoke_tool`, `fetch_get_tool_schema`, and `fetch_invoke_tool`, preventing naming conflicts when multiple compressed servers are used together.

With compression level:

```json
{
  "mcpServers": {
    "compressed-fetch": {
      "command": "mcp-compressor",
      "args": [
        "uvx",
        "mcp-server-fetch",
        "--compression-level", "high"
      ],
    }
  }
}
```

## Use Cases

- **Large Toolsets**: When your MCP server exposes dozens or hundreds of tools
- **Token-Limited Models**: Maximize available context window for actual conversation
- **Cost Optimization**: Reduce token costs for pay-per-token API usage
- **Performance**: Faster initial responses with smaller context
- **Multi-Server Setups**: Use with multiple MCP servers without overwhelming the context

## Command-Line Reference

```
Usage: mcp-compressor [OPTIONS] COMMAND_OR_URL...

Arguments:
  COMMAND_OR_URL...  Command and args for stdio servers, or URL for remote
                     servers [required]

Options:
  --cwd DIRECTORY              Working directory for stdio servers
  -e, --env TEXT               Environment variables (VAR=VALUE format)
  -H, --header TEXT            HTTP headers for remote servers
  -t, --timeout FLOAT          Timeout in seconds [default: 10.0]
  -c, --compression-level      Compression level [default: medium]
                               [low|medium|high|max]
  -n, --server-name TEXT       Custom name prefix for wrapper tools
  -l, --log-level              Logging level [default: info]
                               [debug|info|warning|error|critical]
  --help                       Show this message and exit
```
