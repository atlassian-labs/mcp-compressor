# Atlassian MCP example

This example uses the Atlassian remote MCP server:

```text
https://mcp.atlassian.com/v1/mcp
```

Atlassian MCP supports OAuth. The examples below intentionally use OAuth-first configuration and do not include explicit `Authorization` headers.

!!! note "Automated tests"
    CI may use explicit Basic headers for non-interactive real-world tests, but end-user documentation should prefer OAuth.

## CLI compression with OAuth

The first run opens a browser to authorize the backend. Subsequent runs reuse stored OAuth credentials.

```bash
mcp-compressor -c medium --server-name atlassian -- https://mcp.atlassian.com/v1/mcp
```

## Tool filters

```bash
mcp-compressor -c medium \
  --server-name atlassian \
  --include-tools getAccessibleAtlassianResources,getConfluencePage \
  -- https://mcp.atlassian.com/v1/mcp
```

## SDK usage

=== "Python"

    ```python
    from mcp_compressor import CompressorClient

    with CompressorClient(
        servers={
            "atlassian": {
                "url": "https://mcp.atlassian.com/v1/mcp",
            }
        },
        compression_level="medium",
        server_name="atlassian",
        include_tools=["getAccessibleAtlassianResources"],
    ) as proxy:
        print(proxy.invoke("getAccessibleAtlassianResources"))
    ```

=== "TypeScript"

    ```ts
    import { CompressorClient } from "@atlassian/mcp-compressor";

    const proxy = await new CompressorClient({
      servers: {
        atlassian: {
          url: "https://mcp.atlassian.com/v1/mcp",
        },
      },
      compressionLevel: "medium",
      serverName: "atlassian",
      includeTools: ["getAccessibleAtlassianResources"],
    }).connect();

    try {
      console.log(await proxy.invoke("getAccessibleAtlassianResources"));
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
        .server("atlassian", ServerConfig::url("https://mcp.atlassian.com/v1/mcp"))
        .server_name("atlassian")
        .compression_level(CompressionLevel::Medium)
        .include_tools(["getAccessibleAtlassianResources"])
        .build()
        .connect()
        .await?;

    let output = proxy
        .invoke("getAccessibleAtlassianResources", json!({}))
        .await?;
    ```

## Generated clients

=== "Python"

    ```python
    with CompressorClient(
        servers={"atlassian": {"url": "https://mcp.atlassian.com/v1/mcp"}},
        server_name="atlassian",
    ) as proxy:
        proxy.write_client("cli", "./bin", name="atlassian")
        proxy.write_client("python", "./generated-py", name="atlassian")
        proxy.write_client("typescript", "./generated-ts", name="atlassian")
    ```

=== "TypeScript"

    ```ts
    const proxy = await new CompressorClient({
      servers: { atlassian: { url: "https://mcp.atlassian.com/v1/mcp" } },
      serverName: "atlassian",
    }).connect();

    try {
      proxy.writeClient("cli", "./bin", { name: "atlassian" });
      proxy.writeClient("python", "./generated-py", { name: "atlassian" });
      proxy.writeClient("typescript", "./generated-ts", { name: "atlassian" });
    } finally {
      proxy.close();
    }
    ```

Generated clients call the live Rust proxy, so keep the proxy session alive while they are used.

## Clear OAuth credentials

```bash
mcp-compressor clear-oauth atlassian
mcp-compressor clear-oauth https://mcp.atlassian.com/v1/mcp
```
