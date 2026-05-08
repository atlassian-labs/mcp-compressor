# Atlassian MCP example

This example uses the Atlassian remote MCP server:

```text
https://mcp.atlassian.com/v1/mcp
```

The examples below assume you have a token available as:

```bash
export ATLASSIAN_MCP_BASIC_TOKEN="..."
```

## CLI compression

```bash
mcp-compressor -c medium --server-name atlassian -- https://mcp.atlassian.com/v1/mcp \
  -H "Authorization=Basic ${ATLASSIAN_MCP_BASIC_TOKEN}"
```

## Tool filters

```bash
mcp-compressor -c medium \
  --server-name atlassian \
  --include-tools getAccessibleAtlassianResources,getConfluencePage \
  -- https://mcp.atlassian.com/v1/mcp \
  -H "Authorization=Basic ${ATLASSIAN_MCP_BASIC_TOKEN}"
```

## Python SDK

```python
from mcp_compressor_rust import CompressorClient

with CompressorClient(
    servers={
        "atlassian": {
            "url": "https://mcp.atlassian.com/v1/mcp",
            "headers": {"Authorization": f"Basic {token}"},
        }
    },
    compression_level="medium",
    server_name="atlassian",
    include_tools=["getAccessibleAtlassianResources"],
) as proxy:
    print(proxy.invoke("getAccessibleAtlassianResources"))
```

## TypeScript SDK

```ts
import { CompressorClient } from "@atlassian/mcp-compressor";

const proxy = await new CompressorClient({
  servers: {
    atlassian: {
      url: "https://mcp.atlassian.com/v1/mcp",
      headers: { Authorization: `Basic ${process.env.ATLASSIAN_MCP_BASIC_TOKEN}` },
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

## Generated clients

```python
with CompressorClient(servers=servers, server_name="atlassian") as proxy:
    proxy.write_client("python", "./generated-py", name="atlassian")
    proxy.write_client("typescript", "./generated-ts", name="atlassian")
```

The generated clients call the live Rust proxy, so the proxy session must remain alive while they are used.
