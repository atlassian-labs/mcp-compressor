from __future__ import annotations

from mcp_compressor_rust import (
    RustTool,
    compress_tool_listing,
    format_tool_schema_response,
    parse_mcp_config,
    parse_tool_argv,
)


def sample_tool() -> RustTool:
    return RustTool(
        name="echo",
        description="Echo a value.",
        input_schema={
            "type": "object",
            "properties": {"message": {"type": "string"}},
            "required": ["message"],
        },
    )


def test_native_extension_compresses_tool_listing() -> None:
    assert compress_tool_listing("high", [sample_tool()]) == "<tool>echo(message)</tool>"


def test_native_extension_formats_schema_response() -> None:
    response = format_tool_schema_response(sample_tool())
    assert "Echo a value." in response
    assert '"message"' in response


def test_native_extension_parses_tool_argv() -> None:
    assert parse_tool_argv(sample_tool(), ["--message", "hello"]) == {"message": "hello"}


def test_native_extension_parses_mcp_config() -> None:
    parsed = parse_mcp_config('{"mcpServers":{"my-server":{"command":"python","args":["server.py"]}}}')
    assert parsed == [
        {
            "name": "my-server",
            "command": "python",
            "args": ["server.py"],
            "env": [],
            "cli_prefix": "my-server",
        }
    ]
