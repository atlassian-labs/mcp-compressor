# API reference

Use the package-level references below to find the public API surface for each language.

## Python

Import from `mcp_compressor`:

```python
from mcp_compressor import CompressorClient, CompressorProxy, ToolSpec
```

### High-level client and proxy

| Name | Description |
|---|---|
| `CompressorClient` | High-level client: configure servers, connect to get a proxy. |
| `CompressorProxy` | Active proxy session: list tools, invoke tools, write generated clients. |
| `ProxyTool` | Tool metadata returned by `proxy.tools`. |
| `ProxyResponse` | Raw response from `proxy.invoke_wrapper`. |
| `ExecutableTool` | Callable tool object returned by `proxy.to_executable_tools()`. |
| `GeneratedCodeClient` | Result of `proxy.write_code_client()`: language, files, and environment. |

### Configuration types

| Name | Description |
|---|---|
| `BackendConfig` | Low-level backend configuration object. |
| `CompressedSession` | Active Rust session handle (lower-level than `CompressorProxy`). |
| `CompressedSessionConfig` | Configuration for a `CompressedSession`. |
| `ToolSpec` | Tool specification (name, description, input schema). |

### Just Bash integration

| Name | Description |
|---|---|
| `JustBashProvider` | Provider metadata for a Just Bash host. |
| `JustBashCommand` | Single command spec within a provider. |
| `JustBashCallableCommand` | Callable wrapper returned by `create_just_bash_commands`. |
| `JustBashLocalCommand` | Local in-process tool registered as a Just Bash command. |
| `JustBashTransformResult` | Result of `transform_tools_for_just_bash`. |
| `create_just_bash_commands` | Create callable command objects from a `CompressorProxy`. |
| `install_just_bash_commands` | Install compressed MCP tools into a Just Bash host. |
| `transform_tools_for_just_bash` | Register local in-process tools as Just Bash commands. |

### Compression helpers

| Name | Description |
|---|---|
| `compress_tool_listing` | Format a tool list to a compressed string at a given level. |
| `format_tool_schema_response` | Format a full schema response string for a tool. |
| `parse_tool_argv` | Parse shell-style argv for a tool invocation. |

### OAuth helpers

| Name | Description |
|---|---|
| `clear_oauth_credentials` | Clear stored OAuth credentials (all or for a specific server). |
| `list_oauth_credentials` | List stored OAuth credential entries. |

### MCP config helpers

| Name | Description |
|---|---|
| `parse_mcp_config` | Parse an MCP config JSON string into backend configs. |
| `normalize_servers` | Normalize SDK server config dict into backend configs. |
| `start_compressed_session` | Start a compressed session from backend configs. |
| `start_compressed_session_from_mcp_config` | Start a compressed session from raw MCP config JSON. |

## TypeScript

Import from `@atlassian/mcp-compressor`:

```ts
import {
  CompressorClient,
  compressTools,
  toAISDKTools,
  toMastraTools,
} from "@atlassian/mcp-compressor";
```

### High-level client and proxy

| Name | Description |
|---|---|
| `CompressorClient` | High-level client: configure servers, connect to get a proxy. |
| `CompressorProxy` | Active proxy session: list tools, invoke tools, write generated clients. |
| `ProxyTool` | Tool metadata returned by `proxy.tools`. |
| `ProxyResponse` | Raw response from `proxy.invokeWrapper`. |
| `ExecutableTool` | Callable tool object returned by `proxy.toExecutableTools()`. |
| `GeneratedCodeClient` | Result of `proxy.writeCodeClient()`. |

### Local tool compression

| Name | Description |
|---|---|
| `compressTools` | Compress an in-process AI SDK-style tool map into a `get_tool_schema`/`invoke_tool` surface. |
| `compressToolListing` | Format a tool listing to a compressed string at a given level. |
| `formatToolSchemaResponse` | Format a full schema response string for a tool. |
| `parseToolArgv` | Parse shell-style argv for a tool invocation. |

### Framework adapters

| Name | Description |
|---|---|
| `toAISDKTools` | Convert executable tools to AI SDK tool format. |
| `toMastraTools` | Convert executable tools to Mastra tool format. |

### Just Bash integration

| Name | Description |
|---|---|
| `createJustBashCommands` | Create callable command objects from a `CompressorProxy`. |
| `installJustBashCommands` | Install compressed MCP tools into a Just Bash host. |
| `transformToolsForJustBash` | Register local in-process tools as Just Bash commands. |
| `JustBashProvider` | Provider metadata for a Just Bash host. |
| `JustBashCommand` | Single command spec within a provider. |

### Config and types

| Name | Description |
|---|---|
| `normalizeServers` | Normalize SDK server config into backend configs. |
| `parseMCPConfig` | Parse an MCP config JSON string. |
| `interpolateString` | Interpolate environment variables in a string. |
| `interpolateRecord` | Interpolate environment variables in a record. |
| `interpolateMCPConfig` | Interpolate environment variables in an MCP config object. |
| `parseServerConfigJson` | Parse a server config JSON entry. |
| `BackendConfig` | HTTP or stdio backend config type. |
| `MCPConfigShape` | Shape of an MCP config JSON object. |
| `CompressorMode` | Union of mode strings: `"compressed"`, `"cli"`, `"bash"`. |
| `ServersInput` | Accepted shapes for the `servers` option. |

## Rust

Import from `mcp_compressor`:

```rust
use mcp_compressor::compression::CompressionLevel;
use mcp_compressor::sdk::{
    CompressorClient,
    CompressorMode,
    GeneratedClientKind,
    ServerConfig,
};
```

Key public modules:

| Module | Description |
|---|---|
| `mcp_compressor::sdk` | High-level SDK: `CompressorClient`, `CompressorProxy`, `ServerConfig`. |
| `mcp_compressor::compression` | `CompressionLevel` enum. |
| `mcp_compressor::client_gen` | Generated client kinds and artifacts. |
| `mcp_compressor::cli` | CLI help formatting and argument parsing utilities. |
| `mcp_compressor::config` | MCP config parsing. |

See [SDK reference overview](reference/sdk.md) for language-by-language examples.
