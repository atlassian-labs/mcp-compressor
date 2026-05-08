# Configuration

You can configure backends directly or with MCP config JSON.

## Direct server config

=== "CLI"

    ```bash
    mcp-compressor --server-name alpha -- python alpha_server.py
    ```

=== "Python"

    ```python
    servers = {
        "alpha": {"command": "python", "args": ["alpha_server.py"]},
        "atlassian": {"url": "https://mcp.atlassian.com/v1/mcp"},
    }
    ```

=== "TypeScript"

    ```ts
    const servers = {
      alpha: { command: "python", args: ["alpha_server.py"] },
      atlassian: { url: "https://mcp.atlassian.com/v1/mcp" },
    };
    ```

=== "Rust"

    ```rust
    let client = CompressorClient::builder()
        .server("alpha", ServerConfig::command("python").arg("alpha_server.py"))
        .server("atlassian", ServerConfig::url("https://mcp.atlassian.com/v1/mcp"))
        .build();
    ```

## MCP config JSON

MCP config JSON is the easiest way to describe multiple backends.

```json
{
  "mcpServers": {
    "alpha": {
      "command": "python",
      "args": ["alpha_server.py"]
    },
    "remote": {
      "url": "https://mcp.example.com/v1/mcp"
    }
  }
}
```

For providers that require OAuth, the URL-only form triggers native OAuth. For non-interactive CI or static-token providers, add explicit headers:

```json
{
  "mcpServers": {
    "remote": {
      "url": "https://mcp.example.com/v1/mcp",
      "headers": {
        "Authorization": "Bearer ${TOKEN}"
      }
    }
  }
}
```

Environment variables inside header values can be interpolated by the Rust backend argument/header parser.

## Filters

Use include/exclude filters to reduce the backend tool set before compression.

=== "CLI"

    ```bash
    mcp-compressor -c medium --include-tools getPage,updatePage -- python server.py
    ```

=== "Python"

    ```python
    CompressorClient(
        servers=servers,
        include_tools=["getPage", "updatePage"],
    )
    ```

=== "TypeScript"

    ```ts
    new CompressorClient({
      servers,
      includeTools: ["getPage", "updatePage"],
    });
    ```

=== "Rust"

    ```rust
    CompressorClient::builder()
        .include_tools(["getPage", "updatePage"])
        .build();
    ```
