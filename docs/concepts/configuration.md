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

## Multi-server via the CLI

To configure multiple backend servers from the CLI without a config file, use `--multi-server` (repeatable):

```bash
mcp-compressor -c medium \
  --multi-server "alpha=python alpha.py" \
  --multi-server "beta=python beta.py"
```

The format is `name=command [args...]`. This adds one backend per flag value.

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

Pass the config file path with `--config`:

```bash
mcp-compressor -c medium --config mcp.json
```

!!! note
    `--server-name` cannot be combined with `--config`. When using a config file, server names come from the `mcpServers` keys.

For providers that require OAuth, the URL-only form triggers native OAuth. For non-interactive CI or static-token providers, add explicit headers:

```json
{
  "mcpServers": {
    "remote": {
      "url": "https://mcp.example.com/v1/mcp",
      "headers": {
        "Authorization": "******"
      }
    }
  }
}
```

Environment variables in header values (`${TOKEN}`) are interpolated by the backend argument parser at runtime.

## Filters

Use include/exclude filters to reduce the backend tool set before compression.

=== "CLI"

    ```bash
    # Include only specific tools
    mcp-compressor -c medium --include-tools getPage,updatePage -- python server.py

    # Exclude specific tools
    mcp-compressor -c medium --exclude-tools dangerousDelete -- python server.py
    ```

=== "Python"

    ```python
    CompressorClient(
        servers=servers,
        include_tools=["getPage", "updatePage"],
        exclude_tools=["dangerousDelete"],
    )
    ```

=== "TypeScript"

    ```ts
    new CompressorClient({
      servers,
      includeTools: ["getPage", "updatePage"],
      excludeTools: ["dangerousDelete"],
    });
    ```

=== "Rust"

    ```rust
    CompressorClient::builder()
        .include_tools(["getPage", "updatePage"])
        .exclude_tools(["dangerousDelete"])
        .build();
    ```

Filters are applied before compression: the compressed frontend only sees the filtered tool set.

## TOON output

TOON (Token-Oriented Object Notation) is a token-efficient alternative representation for JSON-structured data. When enabled, the proxy converts JSON text in tool outputs to TOON format before returning results to the client. TOON encodes the same information as JSON using fewer tokens, which reduces the context consumed by large tool responses.

=== "CLI"

    ```bash
    mcp-compressor -c medium --toonify -- python server.py
    ```

=== "Python"

    ```python
    CompressorClient(
        servers=servers,
        toonify=True,
    )
    ```

=== "TypeScript"

    ```ts
    new CompressorClient({
      servers,
      toonify: true,
    });
    ```

=== "Rust"

    ```rust
    CompressorClient::builder()
        .toonify(true)
        .build();
    ```

!!! note
    TOON output is most effective for tool responses that return deeply nested JSON objects. Plain text responses pass through unchanged.
