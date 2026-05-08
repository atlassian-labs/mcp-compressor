# Remote servers and authentication

`mcp-compressor` supports remote streamable HTTP MCP backends.

Most hosted MCP servers require authentication. `mcp-compressor` supports two patterns:

1. **OAuth** — preferred for end users and interactive development.
2. **Explicit headers** — useful for CI, service accounts, or providers that issue static tokens.

## Native OAuth

When a remote backend requires OAuth and you do not provide an explicit `Authorization` header, `mcp-compressor` starts the native OAuth flow:

1. discovers provider metadata,
2. opens a browser to authorize,
3. listens on a local loopback callback URL,
4. exchanges the code for tokens,
5. stores credentials for future runs.

=== "CLI"

    ```bash
    mcp-compressor -c medium --server-name remote -- https://mcp.example.com/v1/mcp
    ```

=== "Python"

    ```python
    from mcp_compressor import CompressorClient

    with CompressorClient(
        servers={"remote": {"url": "https://mcp.example.com/v1/mcp"}},
        server_name="remote",
    ) as proxy:
        print(proxy.tools)
    ```

=== "TypeScript"

    ```ts
    import { CompressorClient } from "@atlassian/mcp-compressor";

    const proxy = await new CompressorClient({
      servers: { remote: { url: "https://mcp.example.com/v1/mcp" } },
      serverName: "remote",
    }).connect();
    ```

=== "Rust"

    ```rust
    let proxy = CompressorClient::builder()
        .server("remote", ServerConfig::url("https://mcp.example.com/v1/mcp"))
        .server_name("remote")
        .build()
        .connect()
        .await?;
    ```

## Clear OAuth credentials

```bash
mcp-compressor clear-oauth
mcp-compressor clear-oauth https://mcp.example.com/v1/mcp
mcp-compressor clear-oauth remote
```

Python and TypeScript also expose OAuth store helper APIs for applications that need to list or clear stored credentials.

## Explicit headers

Use explicit headers when you already have a token or need non-interactive CI.

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

!!! note
    OAuth is the recommended public-user flow for providers such as Atlassian MCP. Explicit headers remain useful for automated tests and service-account style usage.
