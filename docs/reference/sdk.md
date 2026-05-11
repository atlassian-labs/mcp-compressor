# SDK reference overview

This page is a human-oriented map of the main SDK objects. Generated API references can be added once the migration package layout is finalized.

## Auth provider semantics

Python, TypeScript, and Rust SDK clients support dynamic auth providers for remote HTTP backend servers. Providers are evaluated by the remote HTTP transport for each backend request, so long-lived sessions can attach freshly rotated bearer tokens without reconnecting.

## Shared concepts

| Concept | Python | TypeScript | Rust |
|---|---|---|---|
| High-level client | `CompressorClient` | `CompressorClient` | `CompressorClient` |
| Connected proxy/session | `CompressorProxy` | `CompressorProxy` | `CompressorProxy` |
| Tool metadata | `ToolSpec`, `ProxyTool` | `ToolSpec`, `ProxyTool` | `Tool` |
| Just Bash provider | `JustBashProvider` | `JustBashProvider` | `JustBashProviderSpec` |
| Generated client kind | string: `cli`, `python`, `typescript` | string union | `GeneratedClientKind` |

## Python imports

```python
from mcp_compressor import (
    CompressorClient,
    ToolSpec,
    create_just_bash_commands,
)
```

## TypeScript imports

```ts
import {
  CompressorClient,
  type ToolSpec,
  createJustBashCommands,
} from "@atlassian/mcp-compressor";
```

## Rust imports

```rust
use mcp_compressor::compression::CompressionLevel;
use mcp_compressor::sdk::{
    CompressorClient,
    GeneratedClientKind,
    ServerConfig,
};
```
