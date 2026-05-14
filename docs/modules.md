# API reference

Use the package-level references below to find the public API surface for each language.

## Python

Import from `mcp_compressor`:

```python
from mcp_compressor import CompressorClient, ToolSpec
```

Key public objects:

- `CompressorClient`
- `CompressorProxy`
- `ProxyTool`
- `ProxyResponse`
- `ExecutableTool`
- `GeneratedCodeClient`
- `ToolSpec`
- `compress_tool_listing`
- `format_tool_schema_response`
- `parse_tool_argv`
- `parse_mcp_config`
- `create_just_bash_commands`

## TypeScript

Import from `@atlassian/mcp-compressor`:

```ts
import { CompressorClient, compressTools, toAISDKTools } from "@atlassian/mcp-compressor";
```

Key public exports:

- `CompressorClient`
- `CompressorProxy`
- `ProxyTool`
- `ProxyResponse`
- `ExecutableTool`
- `GeneratedCodeClient`
- `compressTools`
- `compressToolListing`
- `formatToolSchemaResponse`
- `parseToolArgv`
- `parseMCPConfig`
- `createJustBashCommands`
- `toAISDKTools`
- `toMastraTools`

## Rust

Import from `mcp_compressor`:

```rust
use mcp_compressor::sdk::{CompressorClient, ServerConfig};
```

Key public modules:

- `mcp_compressor::sdk`
- `mcp_compressor::compression`
- `mcp_compressor::client_gen`
- `mcp_compressor::cli`
- `mcp_compressor::config`

See [SDK reference overview](reference/sdk.md) for language-by-language examples.
