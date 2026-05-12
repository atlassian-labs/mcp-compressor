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
        servers={
            "remote": {
                "url": "https://mcp.example.com/v1/mcp",
                # Optional display name shown by OAuth providers.
                "oauth_app_name": "My Agent",
            }
        },
        server_name="remote",
    ) as proxy:
        print(proxy.tools)
    ```

=== "TypeScript"

    ```ts
    import { CompressorClient } from "@atlassian/mcp-compressor";

    const proxy = await new CompressorClient({
      servers: {
        remote: {
          url: "https://mcp.example.com/v1/mcp",
          // Optional display name shown by OAuth providers.
          oauthAppName: "My Agent",
        },
      },
      serverName: "remote",
    }).connect();
    ```

=== "Rust"

    ```rust
    let proxy = CompressorClient::builder()
        .server(
            "remote",
            ServerConfig::url("https://mcp.example.com/v1/mcp")
                .oauth_app_name("My Agent"),
        )
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

## SDK auth providers

When your application already owns token refresh, prefer SDK auth providers over embedding static headers in config. SDK auth providers are evaluated for each remote backend request in Rust, Python, and TypeScript.

=== "Python"

    ```python
    from mcp_compressor import CompressorClient

    client = CompressorClient(
        servers={
            "remote": {
                "url": "https://mcp.example.com/v1/mcp",
                "auth_provider": lambda: {"Authorization": f"Bearer {token_store.current()}"},
            }
        }
    )

    proxy = client.connect()

    # Later, after refreshing the token:
    proxy.close()
    proxy = client.connect()
    ```

=== "TypeScript"

    ```ts
    import { CompressorClient } from "@atlassian/mcp-compressor";

    const client = new CompressorClient({
      servers: {
        remote: {
          url: "https://mcp.example.com/v1/mcp",
          authProvider: async () => ({
            Authorization: `Bearer ${await tokenStore.current()}`,
          }),
        },
      },
    });

    let proxy = await client.connect();

    // Later, after refreshing the token:
    proxy.close();
    proxy = await client.connect();
    ```

=== "Rust"

    ```rust
    use std::collections::BTreeMap;
    use mcp_compressor::sdk::{CompressorClient, ServerConfig};

    let client = CompressorClient::builder()
        .server(
            "remote",
            ServerConfig::url("https://mcp.example.com/v1/mcp")
                .auth_provider(|| {
                    Ok(BTreeMap::from([(
                        "Authorization".to_string(),
                        format!("Bearer {}", token_store_current()?),
                    )]))
                }),
        )
        .build();
    ```

Long-lived SDK sessions can refresh per request by returning the latest headers from the provider callback.

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
