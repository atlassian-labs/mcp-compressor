# Remote servers and authentication

`mcp-compressor` supports remote streamable HTTP MCP backends.

## Explicit headers

Use explicit headers when you already have a token.

=== "CLI"

    ```bash
    mcp-compressor -c medium -- https://mcp.example.com/v1/mcp \
      -H "Authorization=Bearer ${TOKEN}"
    ```

=== "Python"

    ```python
    CompressorClient(servers={
        "remote": {
            "url": "https://mcp.example.com/v1/mcp",
            "headers": {"Authorization": f"Bearer {token}"},
        }
    })
    ```

=== "TypeScript"

    ```ts
    new CompressorClient({
      servers: {
        remote: {
          url: "https://mcp.example.com/v1/mcp",
          headers: { Authorization: `Bearer ${token}` },
        },
      },
    });
    ```

=== "Rust"

    ```rust
    ServerConfig::url("https://mcp.example.com/v1/mcp")
        .header("Authorization", format!("Bearer {token}"));
    ```

## Header syntax in CLI backend args

CLI backend arguments use `Header=Value` syntax after `--`:

```bash
mcp-compressor -- https://mcp.example.com/v1/mcp -H "Authorization=Bearer ${TOKEN}"
```

## Native OAuth

Native OAuth support exists for remote MCP servers that require browser authorization. It includes:

- authorization URL generation,
- browser opening,
- loopback callback listener,
- file-backed token/state persistence,
- `clear-oauth`.

Clear stored OAuth state:

```bash
mcp-compressor clear-oauth
mcp-compressor clear-oauth https://mcp.example.com/v1/mcp
```

!!! note
    OAuth behavior should be validated against your target MCP provider. Explicit headers are currently the most predictable option for automated CI.
