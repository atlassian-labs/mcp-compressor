# SDK usage

The SDKs let applications start compressed proxy sessions directly, without spawning the `mcp-compressor` stdio CLI.

## Create a compressed proxy

=== "Python"

    ```python
    from mcp_compressor_rust import CompressorClient

    with CompressorClient(
        servers={"alpha": {"command": "python", "args": ["server.py"]}},
        compression_level="medium",
    ) as proxy:
        print(proxy.tools)
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
      console.log(proxy.tools);
      console.log(await proxy.invoke("echo", { message: "hello" }));
    } finally {
      proxy.close();
    }
    ```

=== "Rust"

    ```rust
    use mcp_compressor_core::compression::CompressionLevel;
    use mcp_compressor_core::sdk::{CompressorClient, ServerConfig};
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

    await proxy.invoke("echo", { message: "hi" }, { server: "alpha" });
    ```

=== "Rust"

    ```rust
    let output = proxy
        .invoke_on(Some("alpha"), "echo", json!({ "message": "hi" }))
        .await?;
    ```

## Lifecycle

Python context managers close sessions automatically. TypeScript and Rust proxies expose explicit close/drop behavior.

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
