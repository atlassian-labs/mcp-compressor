# Quickstart

This quickstart uses a local MCP server command. Replace it with any stdio MCP server command or a remote streamable HTTP MCP URL.

## Start a compressed MCP proxy

=== "CLI"

    ```bash
    mcp-compressor -c medium -- python server.py
    ```

    MCP clients connected to this process see compressed tools instead of the full backend tool list.

=== "Python"

    ```python
    from mcp_compressor import CompressorClient

    with CompressorClient(
        servers={"local": {"command": "python", "args": ["server.py"]}},
        compression_level="medium",
    ) as proxy:
        print([tool.name for tool in proxy.tools])
        result = proxy.invoke("myTool", {"arg": "value"})
        print(result)
    ```

=== "TypeScript"

    ```ts
    import { CompressorClient } from "@atlassian/mcp-compressor";

    const client = new CompressorClient({
      servers: { local: { command: "python", args: ["server.py"] } },
      compressionLevel: "medium",
    });

    const proxy = await client.connect();
    try {
      console.log(proxy.tools.map((tool) => tool.name));
      console.log(await proxy.invoke("myTool", { arg: "value" }));
    } finally {
      proxy.close();
    }
    ```

=== "Rust"

    ```rust
    use mcp_compressor::compression::CompressionLevel;
    use mcp_compressor::sdk::{CompressorClient, ServerConfig};
    use serde_json::json;

    let proxy = CompressorClient::builder()
        .server("local", ServerConfig::command("python").arg("server.py"))
        .compression_level(CompressionLevel::Medium)
        .build()
        .connect()
        .await?;

    let result = proxy.invoke("myTool", json!({ "arg": "value" })).await?;
    ```

## Remote MCP server

For a remote streamable HTTP backend, pass a URL. If the server requires OAuth and no explicit `Authorization` header is provided, `mcp-compressor` starts the OAuth flow.

=== "CLI"

    ```bash
    mcp-compressor -c medium -- https://mcp.example.com/v1/mcp
    ```

=== "Python"

    ```python
    with CompressorClient(
        servers={"remote": {"url": "https://mcp.example.com/v1/mcp"}},
        compression_level="medium",
    ) as proxy:
        print(proxy.tools)
    ```

=== "TypeScript"

    ```ts
    const proxy = await new CompressorClient({
      servers: { remote: { url: "https://mcp.example.com/v1/mcp" } },
      compressionLevel: "medium",
    }).connect();
    ```

=== "Rust"

    ```rust
    let proxy = CompressorClient::builder()
        .server("remote", ServerConfig::url("https://mcp.example.com/v1/mcp"))
        .compression_level(CompressionLevel::Medium)
        .build()
        .connect()
        .await?;
    ```
