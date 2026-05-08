# Code Mode and generated clients

**Code Mode** is the umbrella term for generating Python or TypeScript clients for backend MCP tools.

Instead of asking an agent to choose from a large MCP tool list, Code Mode gives it a small generated module with normal functions. Those functions call a local Rust proxy, which routes the request to the backend MCP server.

CLI Mode is the same idea for shell commands: it generates an executable command-line tool whose subcommands call the same proxy.

## When to use each mode

| Mode | Generates | Best for |
|---|---|---|
| CLI Mode | Shell script with subcommands | Agents that can use bash or terminal tools well. |
| Python Code Mode | Python module with functions | Python agents or notebooks. |
| TypeScript Code Mode | ESM module with functions and `.d.ts` declarations | TypeScript/JavaScript agents. |

All generated clients require the proxy session to stay alive while they are used.

## Generate from the CLI

=== "CLI Mode"

    ```bash
    mcp-compressor --cli-mode \
      --server-name atlassian \
      --output-dir ./bin \
      -- https://mcp.atlassian.com/v1/mcp
    ```

    This writes a script such as:

    ```text
    ./bin/atlassian
    ```

=== "Python Code Mode"

    ```bash
    mcp-compressor --python-mode \
      --server-name atlassian \
      --output-dir ./generated-py \
      -- https://mcp.atlassian.com/v1/mcp
    ```

    This writes a module such as:

    ```text
    ./generated-py/atlassian.py
    ```

=== "TypeScript Code Mode"

    ```bash
    mcp-compressor --typescript-mode \
      --server-name atlassian \
      --output-dir ./generated-ts \
      -- https://mcp.atlassian.com/v1/mcp
    ```

    This writes files such as:

    ```text
    ./generated-ts/atlassian.ts
    ./generated-ts/atlassian.d.ts
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

## What the generated clients look like

Assume the backend MCP server exposes tools named:

- `getAccessibleAtlassianResources`
- `getConfluencePage`

### Generated CLI Mode script

The generated shell script provides help and one subcommand per backend tool:

```bash
./bin/atlassian --help
./bin/atlassian get-accessible-atlassian-resources
./bin/atlassian get-confluence-page --page-id "123456"
```

The top-level help looks like:

```text
atlassian - the atlassian toolset

USAGE:
  atlassian <subcommand> [options]

SUBCOMMANDS:
  get-accessible-atlassian-resources
  get-confluence-page
```

### Generated Python Code Mode module

The generated Python module exposes normal functions:

```python
# generated-py/atlassian.py

def getAccessibleAtlassianResources() -> str: ...

def getConfluencePage(page_id: str) -> str: ...
```

Use it from an agent or application:

```python
import sys
sys.path.insert(0, "./generated-py")

import atlassian

resources = atlassian.getAccessibleAtlassianResources()
page = atlassian.getConfluencePage(page_id="123456")
```

### Generated TypeScript Code Mode module

The generated TypeScript module exposes typed async functions:

```ts
// generated-ts/atlassian.ts
export async function getAccessibleAtlassianResources(): Promise<string>;
export async function getConfluencePage(pageId: string): Promise<string>;
```

Use it from an agent or application:

```ts
import {
  getAccessibleAtlassianResources,
  getConfluencePage,
} from "./generated-ts/atlassian.ts";

const resources = await getAccessibleAtlassianResources();
const page = await getConfluencePage("123456");
```

## How an agent might use CLI Mode

A coding agent with shell access can discover commands once:

```bash
atlassian --help
atlassian get-confluence-page --help
```

Then call only the command it needs:

```bash
atlassian get-confluence-page --page-id "123456"
```

This avoids placing every MCP tool schema in the model context.

## How an agent might use Code Mode

A Python-capable agent can inspect the generated module or use normal autocomplete/static analysis:

```python
import atlassian

# Ask for a resource list only when needed.
print(atlassian.getAccessibleAtlassianResources())
```

A TypeScript-capable agent can do the same with generated declarations:

```ts
import { getAccessibleAtlassianResources } from "./generated-ts/atlassian.ts";

console.log(await getAccessibleAtlassianResources());
```

The generated function signatures replace a large MCP tool list with a small language-native API.
