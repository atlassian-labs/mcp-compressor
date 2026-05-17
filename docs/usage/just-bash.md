# Just Bash

Just Bash mode lets command-oriented agents call MCP tools as shell-style commands.

Use it when your agent already has a Just Bash environment and you want MCP tools to appear as commands such as:

```text
alpha_echo --message hello
beta_search --query "release notes"
```

## TypeScript host helper

```ts
import { Bash } from "just-bash";
import { CompressorClient, installJustBashCommands } from "@atlassian/mcp-compressor";

const proxy = await new CompressorClient({ servers, mode: "bash" }).connect();
try {
  const bash = new Bash({ customCommands: [] });
  installJustBashCommands(bash, proxy);

  const result = await bash.exec("alpha_echo --message hello");
  console.log(result.stdout);
} finally {
  proxy.close();
}
```

## Python host helper

```python
from mcp_compressor import CompressorClient, install_just_bash_commands

class BashHost:
    def __init__(self) -> None:
        self.custom_commands = {}

with CompressorClient(servers=servers, mode="bash") as proxy:
    bash = BashHost()
    install_just_bash_commands(bash, proxy)
    print(bash.custom_commands["alpha_echo"](["--message", "hello"]))
```

## Local tool functions

If your application already has executable tool functions in memory, you can install them directly into a Bash host without connecting to an MCP server.

=== "TypeScript"

    ```ts
    import { transformToolsForJustBash } from "@atlassian/mcp-compressor";

    const result = transformToolsForJustBash(tools, {
      bash,
      serverName: "alpha",
    });

    await bash.exec("alpha_echo --message hello");
    ```

=== "Python"

    ```python
    from mcp_compressor import transform_tools_for_just_bash

    result = transform_tools_for_just_bash(
        tools,
        bash=bash,
        server_name="alpha",
    )
    ```

## Command names and collisions

Commands are prefixed with the server name so multiple servers can expose tools with the same backend name without shadowing each other:

```text
alpha_echo
beta_echo
```

## Lifecycle

Generated commands call the active `mcp-compressor` session or the local tool functions you provided. Keep that session or host application alive for as long as the agent needs to run the commands.
