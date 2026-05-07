# mcp-compressor-rust

Experimental Rust-backed Python package for `mcp-compressor`.

The package name is temporary while the Rust-core migration is validated. The public API should be treated as the future Python SDK shape: users create a `CompressorClient` around one or more MCP server configs and use the resulting proxy/tools. The fact that the implementation is Rust-backed is not part of the user-facing programming model.

## Quick start

The primary SDK object is `CompressorClient`. It starts a Rust-backed local proxy in-process and returns a `CompressorProxy`; no `mcp-compressor` stdio subprocess is required.


```python
from mcp_compressor_rust import CompressorClient

servers = {
    "alpha": {
        "command": "python",
        "args": ["alpha_server.py"],
    },
    "atlassian": {
        "url": "https://mcp.atlassian.com/v1/mcp",
        "headers": {
            "Authorization": f"Basic {token}",
        },
    },
}

with CompressorClient(
    servers=servers,
    mode="compressed",
    compression_level="medium",
    include_tools=["getConfluencePage", "updateConfluencePage"],
    toonify=True,
) as proxy:
    print([tool.name for tool in proxy.tools])
    result = proxy.invoke(
        "getAccessibleAtlassianResources",
        {},
        server="atlassian",
    )
    print(result)
```

## Modes

`CompressorClient` accepts these modes:

- `compressed` — expose compressed wrapper tools such as `<server>_get_tool_schema` and `<server>_invoke_tool`.
- `cli` — expose CLI/help transform metadata through the Rust core session.
- `bash` — expose Just Bash provider metadata through the Rust core session.
Generated Python and TypeScript clients are produced with `proxy.write_client(...)` rather than by selecting a long-lived session mode.

## Generated clients

A connected proxy can write shell, Python, or TypeScript clients that call the live Rust proxy:

```python
with CompressorClient(servers=servers, compression_level="max") as proxy:
    proxy.write_client("cli", "./bin", name="atlassian")
    proxy.write_client("python", "./generated-py", name="atlassian")
    proxy.write_client("typescript", "./generated-ts", name="atlassian")
```

## Advanced helpers

Low-level helpers such as `compress_tool_listing`, `parse_tool_argv`, and `ToolSpec` remain available for tests and advanced integrations, but the primary SDK entry point is `CompressorClient`.
