# mcp-compressor

`mcp-compressor` lets agents use large MCP servers without paying the full token cost of every tool description on every request.

Instead of exposing every backend tool directly, it exposes a small compressed interface:

- `get_tool_schema` — fetch the full schema only when needed.
- `invoke_tool` — invoke the backend tool after selecting it.
- `list_tools` — optional discovery helper at `max` compression.

It works as:

- a **CLI** that runs as an MCP proxy,
- a **Rust SDK**,
- a **Python SDK**,
- a **TypeScript SDK**,
- generated shell/Python/TypeScript clients,
- a Just Bash integration surface.

## Why use it?

MCP servers can expose dozens or hundreds of tools. Tool descriptions and JSON schemas can quickly consume thousands of tokens before the model has done any useful work.

`mcp-compressor` keeps tool discovery cheap and lets the model ask for full schemas only when it has selected a tool.

## Common use cases

- Add several MCP servers to a coding agent without overwhelming context.
- Put a compressed MCP proxy in front of remote servers such as Atlassian MCP.
- Give agents shell-style or code-style access to MCP tools.
- Embed compression directly in Rust, Python, or TypeScript applications without spawning a compressor subprocess.

## Choose your path

=== "CLI"

    ```bash
    mcp-compressor -c medium -- python my_mcp_server.py
    ```

=== "Python"

    ```python
    from mcp_compressor_rust import CompressorClient

    with CompressorClient(
        servers={"alpha": {"command": "python", "args": ["server.py"]}},
        compression_level="medium",
    ) as proxy:
        print([tool.name for tool in proxy.tools])
    ```

=== "TypeScript"

    ```ts
    import { CompressorClient } from "@atlassian/mcp-compressor";

    const proxy = await new CompressorClient({
      servers: { alpha: { command: "python", args: ["server.py"] } },
      compressionLevel: "medium",
    }).connect();

    try {
      console.log(proxy.tools.map((tool) => tool.name));
    } finally {
      proxy.close();
    }
    ```

=== "Rust"

    ```rust
    use mcp_compressor_core::compression::CompressionLevel;
    use mcp_compressor_core::sdk::{CompressorClient, ServerConfig};

    let proxy = CompressorClient::builder()
        .server("alpha", ServerConfig::command("python").arg("server.py"))
        .compression_level(CompressionLevel::Medium)
        .build()
        .connect()
        .await?;
    ```

## Next

Start with [Installation](getting-started/installation.md), then run the [Quickstart](getting-started/quickstart.md).
