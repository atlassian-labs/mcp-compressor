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

All generated clients require the proxy session to stay alive while they are used. CLI Mode installs scripts on `PATH` by default; Code Mode writes Python/TypeScript files to `./dist` in the current working directory by default. Use `--output-dir` to override either behavior.

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
    mcp-compressor --code-mode python \
      --server-name atlassian \
      -- https://mcp.atlassian.com/v1/mcp
    ```

    This writes a module such as:

    ```text
    ./dist/atlassian.py
    ```

=== "TypeScript Code Mode"

    ```bash
    mcp-compressor --code-mode typescript \
      --server-name atlassian \
      -- https://mcp.atlassian.com/v1/mcp
    ```

    This writes files such as:

    ```text
    ./dist/atlassian.ts
    ./dist/atlassian.d.ts
    ```

The Atlassian examples use OAuth. The first run opens a browser if no stored credentials exist.

## Generate from SDKs

=== "Python"

    ```python
    from mcp_compressor import CompressorClient

    with CompressorClient(servers=servers, compression_level="max") as proxy:
        proxy.write_client("cli", "./bin", name="atlassian")
        python_client = proxy.write_code_client("python", "./generated-py", name="atlassian")
        typescript_client = proxy.write_code_client("typescript", "./generated-ts", name="atlassian")
        print(python_client.environment)  # {"PYTHONPATH": "./generated-py"}
    ```

=== "TypeScript"

    ```ts
    import { CompressorClient } from "@atlassian/mcp-compressor";

    const proxy = await new CompressorClient({ servers, compressionLevel: "max" }).connect();
    try {
      proxy.writeClient("cli", "./bin", { name: "atlassian" });

      const pythonClient = proxy.writeCodeClient({
        language: "python",
        outputDir: "./generated-py",
        name: "atlassian",
      });

      const typescriptClient = proxy.writeCodeClient({
        language: "typescript",
        outputDir: "./generated-ts",
        name: "atlassian",
      });

      console.log(pythonClient.environment); // { PYTHONPATH: "./generated-py" }
      console.log(typescriptClient.files);
    } finally {
      proxy.close();
    }
    ```

=== "Rust"

    ```rust
    use mcp_compressor::sdk::CodeLanguage;

    proxy.write_cli_client("./bin", Some("atlassian"))?;
    let python_client = proxy.write_code_client(CodeLanguage::Python, "./generated-py", Some("atlassian"))?;
    let typescript_client = proxy.write_code_client(CodeLanguage::TypeScript, "./generated-ts", Some("atlassian"))?;
    println!("{:?}", python_client.environment); // {"PYTHONPATH": "./generated-py"}
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

The generated Python module exposes normal functions with snake_case names. Parameters get type hints from the tool's input schema, and docstrings document each argument:

```python
# generated-py/atlassian.py

def get_accessible_atlassian_resources() -> str: ...

def get_confluence_page(cloud_id: str, page_id: str, content_format: str | None = None) -> str:
    """Get a Confluence page.

    Args:
        cloud_id: Cloud ID (UUID or site URL)
        content_format: One of: html, markdown, adf.
    """
```

Use it from an agent or application:

```python
import sys
sys.path.insert(0, "./generated-py")

import atlassian

resources = atlassian.get_accessible_atlassian_resources()
page = atlassian.get_confluence_page(cloud_id="my-site.atlassian.net", page_id="123456")
```

### Generated TypeScript Code Mode module

The generated TypeScript module exposes typed async functions. Parameter types come from the tool's input schema: numbers, booleans, enum literals, arrays, and object shapes. Anything the schema doesn't pin down is `unknown`. Parameter descriptions become `@param` JSDoc tags:

```ts
// generated-ts/atlassian.ts
export async function getAccessibleAtlassianResources(): Promise<string>;
export async function getConfluencePage(
  cloudId: string,
  pageId: string,
  contentFormat?: "html" | "markdown" | "adf",
): Promise<string>;
```

Use it from an agent or application:

```ts
import {
  getAccessibleAtlassianResources,
  getConfluencePage,
} from "./generated-ts/atlassian.ts";

const resources = await getAccessibleAtlassianResources();
const page = await getConfluencePage("my-site.atlassian.net", "123456");
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

### Arguments and errors

Flags are converted using the tool's input schema:

- `string` properties are passed verbatim; `--page-id 123` sends `"123"`, never a number.
- `integer`, `number`, and `boolean` properties must parse as that type.
- `object` properties take a JSON object, for example `--filters '{"status":"open"}'`.
- Array properties accept the flag more than once.
- `--json '{...}'` sends a whole JSON object as the tool arguments. It can't be combined with other flags.

Generated CLIs and [Just Bash](just-bash.md) commands report errors the same way:

| Failure | Exit code | stderr |
|---|---|---|
| Unknown subcommand or flag, bad value, missing required flag | `2` | `parse error: ...` or `validation error: ...`, then `Run '<cli> <subcommand> --help' for usage.` |
| The backend tool failed | `1` | The tool's error message |

Error messages name the flag the agent typed, such as `--page-id`, not the schema property. Nothing is written to stdout when a command fails, so pipelines like `atlassian get-confluence-page --page-id 1 | jq .` don't parse error text.

In Code Mode, a tool failure raises `RuntimeError` in Python and rejects with an `Error` in TypeScript. Either way the message is the tool's own error text.

### Timeouts

Generated clients don't set a request timeout of their own. A call waits as long as the proxy does, and the proxy applies the backend `--timeout` you configured. To cap a single client's wait, set `MCP_COMPRESSOR_REQUEST_TIMEOUT` to a number of seconds in its environment. When the cap is hit, the CLI exits `1`, Python raises `TimeoutError`, and TypeScript rejects with an `Error`. The message is `timed out after <n>s waiting for the tool result`.

### Bounded output with recovery

CLI and Code Mode sessions can opt into `--shrinkfit` (default 20,000 characters).
Only oversized supported results change to a JSON preview/recovery envelope.
Original MCP results are persisted on the proxy host. See
[recoverable response shrinking](shrinkfit.md) before consuming the changed shape.

## How an agent might use Code Mode

A Python-capable agent can inspect the generated module or use normal autocomplete/static analysis:

```python
import atlassian

# Ask for a resource list only when needed.
print(atlassian.get_accessible_atlassian_resources())
```

A TypeScript-capable agent can do the same with generated declarations:

```ts
import { getAccessibleAtlassianResources } from "./generated-ts/atlassian.ts";

console.log(await getAccessibleAtlassianResources());
```

The generated function signatures replace a large MCP tool list with a small language-native API.
