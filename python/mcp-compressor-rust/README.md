# mcp-compressor-rust

Experimental Rust-backed Python package for `mcp-compressor`.

The package name is temporary while the Rust-core migration is validated. The public API should be treated as the future Python SDK shape: users create a `CompressorClient` around one or more MCP server configs and use the resulting proxy/tools. The fact that the implementation is Rust-backed is not part of the user-facing programming model.

## Quick start

The primary SDK object is `CompressorClient`. It starts a Rust-backed local proxy in-process and returns a `CompressorProxy`; no `mcp-compressor` stdio subprocess is required.


```python
from mcp_compressor_rust import CompressorClient

servers = {
    "alpha": {
        "command": "python",
        "args": ["alpha_server.py"],
    },
    "atlassian": {
        "url": "https://mcp.atlassian.com/v1/mcp",
        "headers": {
            "Authorization": f"Basic {token}",
        },
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
- `cli` — expose CLI/help transform metadata through the Rust core session.
- `bash` — expose Just Bash provider metadata through the Rust core session.
Generated Python and TypeScript clients are produced with `proxy.write_client(...)` rather than by selecting a long-lived session mode.

## Just Bash metadata

Just Bash mode exposes the compressed proxy bridge plus typed provider metadata. Language hosts can use this metadata to register backend MCP tools as Just Bash commands without the Rust core executing shell commands itself:

```python
with CompressorClient(servers=servers, mode="bash") as proxy:
    for provider in proxy.just_bash_providers:
        print(provider.provider_name, provider.help_tool_name)
        for command in provider.tools:
            print(command.command_name, command.backend_tool_name, command.invoke_tool_name)

    # Python hosts can also create callable command objects from the metadata.
    commands = {cmd.command_name: cmd for cmd in create_just_bash_commands(proxy)}
    print(commands["atlassian_get-accessible-atlassian-resources"]([]))
```

Duplicate backend command names are prefixed with the provider name, for example `alpha_echo` and `beta_echo`.

## Generated clients

A connected proxy can write shell, Python, or TypeScript clients that call the live Rust proxy:

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
python -m venv /tmp/mcp-compressor-rust-wheel-test
/tmp/mcp-compressor-rust-wheel-test/bin/python -m pip install "$PWD"/dist/*.whl
cd /tmp
/tmp/mcp-compressor-rust-wheel-test/bin/python -c "from mcp_compressor_rust import CompressorClient, ToolSpec"
```

CI runs the same kind of wheel smoke test before uploading the built wheel as an artifact.

## Advanced helpers

Low-level helpers such as `compress_tool_listing`, `parse_tool_argv`, and `ToolSpec` remain available for tests and advanced integrations, but the primary SDK entry point is `CompressorClient`.
