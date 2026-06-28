# SDK reference overview

This page maps the main SDK objects across Python, TypeScript, and Rust.

## Shared concepts

| Concept | Python | TypeScript | Rust |
|---|---|---|---|
| High-level client | `CompressorClient` | `CompressorClient` | `CompressorClient` |
| Connected proxy/session | `CompressorProxy` | `CompressorProxy` | `CompressorProxy` |
| Tool metadata | `ProxyTool` | `ProxyTool` | `Tool` |
| Tool specification | `ToolSpec` | `ToolSpec` | `Tool` |
| Callable tool | `ExecutableTool` | `ExecutableTool` | — |
| Generated code client | `GeneratedCodeClient` | `GeneratedCodeClient` | — |
| Just Bash provider | `JustBashProvider` | `JustBashProvider` | `JustBashProviderSpec` |
| Generated client kind | `str`: `"cli"`, `"python"`, `"typescript"` | `GeneratedClientKind` | `GeneratedClientKind` |

## Auth provider semantics

Python, TypeScript, and Rust SDK clients support dynamic auth providers for remote HTTP backend servers. Providers are evaluated by the remote HTTP transport for each backend request, so long-lived sessions can attach freshly rotated bearer tokens without reconnecting.

Static `headers` in server config and dynamic provider headers can be combined. Provider headers override static headers with the same name.

## CompressorClient constructor options

All three SDKs accept the same core options, with idiomatic naming per language:

| Option | Python | TypeScript | Rust |
|---|---|---|---|
| Backend servers | `servers` | `servers` | `.server(name, config)` |
| Compression level | `compression_level` | `compressionLevel` | `.compression_level(level)` |
| Server name prefix | `server_name` | `serverName` | `.server_name(name)` |
| Include tools filter | `include_tools` | `includeTools` | `.include_tools([...])` |
| Exclude tools filter | `exclude_tools` | `excludeTools` | `.exclude_tools([...])` |
| TOON output | `toonify` | `toonify` | `.toonify(bool)` |
| Proxy mode | `mode` | `mode` | `.mode(CompressorMode::...)` |

## CompressorProxy methods

| Method | Python | TypeScript | Rust |
|---|---|---|---|
| Frontend tool list | `proxy.tools` | `proxy.tools` | `proxy.tools()` |
| Invoke a backend tool | `proxy.invoke(tool, input)` | `proxy.invoke(tool, input)` | `proxy.invoke(tool, input).await` |
| Invoke on specific server | `proxy.invoke(..., server="name")` | `proxy.invoke(..., { server: "name" })` | `proxy.invoke_on(Some("name"), ...)` |
| Raw wrapper invocation | `proxy.invoke_wrapper(wrapper, input)` | `proxy.invokeWrapper(wrapper, input)` | — |
| Get backend tool schema | `proxy.schema(tool)` | — | — |
| Executable tool map | `proxy.to_executable_tools()` | `proxy.toExecutableTools()` | `proxy.executable_tools()` |
| Write CLI client | `proxy.write_client("cli", dir)` | `proxy.writeClient("cli", dir)` | `proxy.write_cli_client(dir, name)` |
| Write code client | `proxy.write_code_client(lang, dir)` | `proxy.writeCodeClient({language, outputDir})` | `proxy.write_code_client(lang, dir, name)` |
| Close session | `proxy.close()` | `proxy.close()` | dropped automatically |

## Python imports

```python
from mcp_compressor import (
    CompressorClient,
    CompressorProxy,
    ExecutableTool,
    GeneratedCodeClient,
    JustBashProvider,
    JustBashCommand,
    JustBashCallableCommand,
    ProxyResponse,
    ProxyTool,
    ToolSpec,
    BackendConfig,
    compress_tool_listing,
    format_tool_schema_response,
    parse_tool_argv,
    parse_mcp_config,
    clear_oauth_credentials,
    list_oauth_credentials,
    create_just_bash_commands,
    install_just_bash_commands,
    transform_tools_for_just_bash,
    normalize_servers,
)
```

## TypeScript imports

```ts
import {
  CompressorClient,
  CompressorProxy,
  type ProxyTool,
  type ProxyResponse,
  type ExecutableTool,
  type GeneratedCodeClient,
  type JustBashProvider,
  type JustBashCommand,
  type ToolSpec,
  type CompressorMode,
  type ServersInput,
  compressTools,
  compressToolListing,
  formatToolSchemaResponse,
  parseToolArgv,
  parseMCPConfig,
  normalizeServers,
  createJustBashCommands,
  installJustBashCommands,
  transformToolsForJustBash,
  toAISDKTools,
  toMastraTools,
} from "@atlassian/mcp-compressor";
```

## Rust imports

```rust
use mcp_compressor::compression::CompressionLevel;
use mcp_compressor::sdk::{
    CompressorClient,
    CompressorMode,
    GeneratedClientKind,
    ServerConfig,
};
```
