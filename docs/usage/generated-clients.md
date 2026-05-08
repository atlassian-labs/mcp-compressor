# Generated clients

Generated clients let agents call MCP tools through shell, Python, or TypeScript code while the Rust proxy owns MCP routing and authorization.

They are useful when your agent is better at command or code execution than raw MCP tool invocation.

## What gets generated?

- **CLI**: an executable shell script with one subcommand per backend tool.
- **Python**: a Python module with one function per backend tool.
- **TypeScript**: an ESM module and `.d.ts` declarations with one function per backend tool.

All generated clients call a live local Rust proxy using a session token. Keep the proxy session alive while generated clients are used.

## Generate from the CLI

=== "Shell CLI"

    ```bash
    mcp-compressor --cli-mode \
      --server-name atlassian \
      --output-dir ./bin \
      -- https://mcp.atlassian.com/v1/mcp
    ```

=== "Python client"

    ```bash
    mcp-compressor --python-mode \
      --server-name atlassian \
      --output-dir ./generated-py \
      -- https://mcp.atlassian.com/v1/mcp
    ```

=== "TypeScript client"

    ```bash
    mcp-compressor --typescript-mode \
      --server-name atlassian \
      --output-dir ./generated-ts \
      -- https://mcp.atlassian.com/v1/mcp
    ```

The Atlassian examples use OAuth. The first run opens a browser if no stored credentials exist.

## Generate from SDKs

=== "Python"

    ```python
    from mcp_compressor import CompressorClient

    with CompressorClient(servers=servers, compression_level="max") as proxy:
        proxy.write_client("cli", "./bin", name="atlassian")
        proxy.write_client("python", "./generated-py", name="atlassian")
        proxy.write_client("typescript", "./generated-ts", name="atlassian")
    ```

=== "TypeScript"

    ```ts
    import { CompressorClient } from "@atlassian/mcp-compressor";

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
    use mcp_compressor::sdk::GeneratedClientKind;

    proxy.write_client(GeneratedClientKind::Cli, "./bin", Some("atlassian"))?;
    proxy.write_client(GeneratedClientKind::Python, "./generated-py", Some("atlassian"))?;
    proxy.write_client(GeneratedClientKind::TypeScript, "./generated-ts", Some("atlassian"))?;
    ```

## Example generated CLI usage

```bash
./bin/atlassian --help
./bin/atlassian get-accessible-atlassian-resources
```

## Example generated Python usage

```python
import sys
sys.path.insert(0, "./generated-py")

import atlassian
print(atlassian.getAccessibleAtlassianResources())
```

## Example generated TypeScript usage

```ts
import { getAccessibleAtlassianResources } from "./generated-ts/atlassian.ts";

console.log(await getAccessibleAtlassianResources());
```
