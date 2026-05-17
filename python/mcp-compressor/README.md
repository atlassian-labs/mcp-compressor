# mcp-compressor Python SDK

Python SDK and CLI wrapper for `mcp-compressor`.

The public Python import is `mcp_compressor`:

```python
from mcp_compressor import CompressorClient
```

## Quick start

The primary SDK object is `CompressorClient`. It starts a local session and returns a `CompressorProxy`; no `mcp-compressor` stdio subprocess is required.

```python
from mcp_compressor import CompressorClient

servers = {
    "alpha": {
        "command": "python",
        "args": ["alpha_server.py"],
    },
    "atlassian": {
        "url": "https://mcp.atlassian.com/v1/mcp",
    },
}

with CompressorClient(
    servers=servers,
    mode="compressed",
    compression_level="medium",
    include_tools=["getConfluencePage", "updateConfluencePage"],
    toonify=True,
) as proxy:
    print([tool.name for tool in proxy.tools])
    result = proxy.invoke(
        "getAccessibleAtlassianResources",
        {},
        server="atlassian",
    )
    print(result)
```

## Modes

`CompressorClient` accepts these modes:

- `compressed` — expose compressed wrapper tools such as `<server>_get_tool_schema` and `<server>_invoke_tool`.
- `cli` — expose CLI/help tools for generated shell command usage.
- `bash` — expose Just Bash command integration for command-oriented agents.

Generated Python and TypeScript clients are produced with `proxy.write_client(...)` rather than by selecting a long-lived session mode.

## Just Bash metadata

Just Bash mode lets language hosts register backend MCP tools as shell-style commands:

```python
from mcp_compressor import CompressorClient, create_just_bash_commands

with CompressorClient(servers=servers, mode="bash") as proxy:
    commands = {cmd.command_name: cmd for cmd in create_just_bash_commands(proxy)}
    print(commands["atlassian_get-accessible-atlassian-resources"]([]))
```

Duplicate backend command names are prefixed with the provider name, for example `alpha_echo` and `beta_echo`.

## Generated clients

A connected proxy can write shell, Python, or TypeScript clients that call the live proxy:

```python
with CompressorClient(servers=servers, compression_level="max") as proxy:
    proxy.write_client("cli", "./bin", name="atlassian")
    proxy.write_client("python", "./generated-py", name="atlassian")
    proxy.write_client("typescript", "./generated-ts", name="atlassian")
```

## Packaging smoke test

Build a local wheel and verify it imports from a clean virtualenv:

```bash
uvx maturin build --release --out dist
python -m venv /tmp/mcp-compressor-wheel-test
/tmp/mcp-compressor-wheel-test/bin/python -m pip install "$PWD"/dist/*.whl
cd /tmp
/tmp/mcp-compressor-wheel-test/bin/python -c "from mcp_compressor import CompressorClient, ToolSpec"
```

CI runs the same kind of wheel smoke test before uploading the built wheel as an artifact.

## Advanced helpers

Low-level helpers such as `compress_tool_listing`, `parse_tool_argv`, and `ToolSpec` remain available for tests and advanced integrations, but the primary SDK entry point is `CompressorClient`.
