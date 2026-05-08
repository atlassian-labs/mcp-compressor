# SDK usage

The SDKs are for applications that want to use compressed MCP tools directly without launching `mcp-compressor` as a stdio subprocess.

Each SDK follows the same model:

1. Create a `CompressorClient` with one or more MCP backend servers.
2. Connect to get a `CompressorProxy`.
3. Inspect compressed frontend tools or invoke backend tools through the proxy.
4. Close/drop the proxy when finished.

## Create a compressed proxy

=== "Python"

    ```python
    from mcp_compressor import CompressorClient

    with CompressorClient(
        servers={"alpha": {"command": "python", "args": ["server.py"]}},
        compression_level="medium",
    ) as proxy:
        print([tool.name for tool in proxy.tools])
        print(proxy.invoke("echo", {"message": "hello"}))
    ```

=== "TypeScript"

    ```ts
    import { CompressorClient } from "@atlassian/mcp-compressor";

    const client = new CompressorClient({
      servers: { alpha: { command: "python", args: ["server.py"] } },
      compressionLevel: "medium",
    });

    const proxy = await client.connect();
    try {
      console.log(proxy.tools.map((tool) => tool.name));
      console.log(await proxy.invoke("echo", { message: "hello" }));
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
        .server("alpha", ServerConfig::command("python").arg("server.py"))
        .compression_level(CompressionLevel::Medium)
        .build()
        .connect()
        .await?;

    let output = proxy.invoke("echo", json!({ "message": "hello" })).await?;
    ```

## Multi-server routing

When more than one backend server is configured, specify the server when invoking a backend tool.

=== "Python"

    ```python
    with CompressorClient(servers={
        "alpha": {"command": "python", "args": ["alpha.py"]},
        "beta": {"command": "python", "args": ["beta.py"]},
    }) as proxy:
        print(proxy.invoke("echo", {"message": "hi"}, server="alpha"))
    ```

=== "TypeScript"

    ```ts
    const proxy = await new CompressorClient({
      servers: {
        alpha: { command: "python", args: ["alpha.py"] },
        beta: { command: "python", args: ["beta.py"] },
      },
    }).connect();

    try {
      await proxy.invoke("echo", { message: "hi" }, { server: "alpha" });
    } finally {
      proxy.close();
    }
    ```

=== "Rust"

    ```rust
    let output = proxy
        .invoke_on(Some("alpha"), "echo", json!({ "message": "hi" }))
        .await?;
    ```

## Compression options

The SDKs expose the same main compression options as the CLI.

=== "Python"

    ```python
    CompressorClient(
        servers=servers,
        compression_level="high",
        include_tools=["search", "getPage"],
        exclude_tools=["dangerousDelete"],
        toonify=True,
    )
    ```

=== "TypeScript"

    ```ts
    new CompressorClient({
      servers,
      compressionLevel: "high",
      includeTools: ["search", "getPage"],
      excludeTools: ["dangerousDelete"],
      toonify: true,
    });
    ```

=== "Rust"

    ```rust
    CompressorClient::builder()
        .compression_level(CompressionLevel::High)
        .include_tools(["search", "getPage"])
        .exclude_tools(["dangerousDelete"])
        .toonify(true)
        .build();
    ```

## Modes

| Mode | Purpose |
|---|---|
| `compressed` | Standard `get_tool_schema` / `invoke_tool` compressed surface. |
| `cli` | Help-tool-oriented surface for generated shell command usage. |
| `bash` | Just Bash provider metadata plus proxy routing. |

=== "Python"

    ```python
    CompressorClient(servers=servers, mode="bash")
    ```

=== "TypeScript"

    ```ts
    new CompressorClient({ servers, mode: "bash" });
    ```

=== "Rust"

    ```rust
    use mcp_compressor::sdk::CompressorMode;

    CompressorClient::builder()
        .mode(CompressorMode::JustBash)
        .build();
    ```

## Lifecycle

=== "Python"

    ```python
    with CompressorClient(servers=servers) as proxy:
        ...
    ```

=== "TypeScript"

    ```ts
    const proxy = await client.connect();
    try {
      ...
    } finally {
      proxy.close();
    }
    ```

=== "Rust"

    ```rust
    let proxy = client.connect().await?;
    // proxy closes when dropped
    ```
