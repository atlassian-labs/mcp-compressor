# Just Bash

Just Bash mode exposes MCP tools as command metadata that a language host can register with `just-bash`.

Rust owns:

- MCP backend connections,
- compression,
- schema-driven argument parsing,
- proxy routing,
- provider metadata.

The language host owns:

- Just Bash environment setup,
- command registration,
- command execution UX.

## TypeScript host helper

```ts
import { Bash } from "just-bash";
import { CompressorClient, createJustBashCommands } from "@atlassian/mcp-compressor";

const proxy = await new CompressorClient({ servers, mode: "bash" }).connect();
try {
  const registrations = createJustBashCommands(proxy);
  const bash = new Bash({ customCommands: registrations.map((item) => item.command) });

  const result = await bash.exec("alpha_echo --message hello");
  console.log(result.stdout);
} finally {
  proxy.close();
}
```

## Python host helper

```python
from mcp_compressor_rust import CompressorClient, create_just_bash_commands

with CompressorClient(servers=servers, mode="bash") as proxy:
    commands = {command.command_name: command for command in create_just_bash_commands(proxy)}
    print(commands["alpha_echo"](["--message", "hello"]))
```

## Command names and collisions

If two servers expose the same backend command name, helpers prefix the command with the provider/server name:

```text
alpha_echo
beta_echo
```

This avoids one server shadowing another.

## Metadata shape

Provider metadata includes:

- provider name,
- help tool name,
- command name,
- backend tool name,
- input schema,
- invoke wrapper name.
