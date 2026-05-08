# Generated clients

Generated clients let agents call MCP tools through shell, Python, or TypeScript code while the Rust proxy owns MCP routing and authorization.

## Generate from the CLI

```bash
mcp-compressor --cli-mode --server-name atlassian --output-dir ./bin -- https://mcp.atlassian.com/v1/mcp \
  -H "Authorization=Basic ${ATLASSIAN_MCP_BASIC_TOKEN}"

mcp-compressor --python-mode --server-name atlassian --output-dir ./generated-py -- https://mcp.atlassian.com/v1/mcp \
  -H "Authorization=Basic ${ATLASSIAN_MCP_BASIC_TOKEN}"

mcp-compressor --typescript-mode --server-name atlassian --output-dir ./generated-ts -- https://mcp.atlassian.com/v1/mcp \
  -H "Authorization=Basic ${ATLASSIAN_MCP_BASIC_TOKEN}"
```

## Generate from SDKs

=== "Python"

    ```python
    with CompressorClient(servers=servers, compression_level="max") as proxy:
        proxy.write_client("cli", "./bin", name="atlassian")
        proxy.write_client("python", "./generated-py", name="atlassian")
        proxy.write_client("typescript", "./generated-ts", name="atlassian")
    ```

=== "TypeScript"

    ```ts
    const proxy = await new CompressorClient({ servers, compressionLevel: "max" }).connect();
    try {
      proxy.writeClient("cli", "./bin", { name: "atlassian" });
      proxy.writeClient("python", "./generated-py", { name: "atlassian" });
      proxy.writeClient("typescript", "./generated-ts", { name: "atlassian" });
    } finally {
      proxy.close();
    }
    ```

=== "Rust"

    ```rust
    use mcp_compressor_core::sdk::GeneratedClientKind;

    proxy.write_client(GeneratedClientKind::Python, "./generated-py", Some("atlassian"))?;
    proxy.write_client(GeneratedClientKind::TypeScript, "./generated-ts", Some("atlassian"))?;
    proxy.write_client(GeneratedClientKind::Cli, "./bin", Some("atlassian"))?;
    ```

## What gets generated?

- **CLI**: an executable shell script with one subcommand per backend tool.
- **Python**: a Python module with one function per backend tool.
- **TypeScript**: an ESM module and `.d.ts` declarations with one function per backend tool.

All generated clients call the local Rust proxy using a session token. Keep the proxy process/session alive while generated clients are being used.
