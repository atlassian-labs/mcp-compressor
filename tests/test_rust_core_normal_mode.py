from __future__ import annotations

from pathlib import Path

from fastmcp import Client
from fastmcp.client.transports import StdioTransport


async def test_rust_core_normal_stdio_mode_with_fixture_server() -> None:
    root = Path(__file__).parents[1]
    alpha = root / "crates" / "mcp-compressor-core" / "tests" / "fixtures" / "alpha_server.py"
    command = [
        "cargo",
        "run",
        "-q",
        "-p",
        "mcp-compressor-core",
        "--",
        "--compression",
        "max",
        "--server-name",
        "alpha",
        "--",
        "python3",
        str(alpha),
    ]

    async with Client(StdioTransport(command=command[0], args=command[1:])) as client:
        tools = {tool.name for tool in await client.list_tools()}
        assert tools == {
            "alpha_get_tool_schema",
            "alpha_invoke_tool",
            "alpha_list_tools",
        }

        result = await client.call_tool(
            "alpha_invoke_tool",
            {
                "tool_name": "echo",
                "tool_input": {"message": "hello"},
            },
        )
        assert result.content[0].text == "alpha:hello"

        schema = await client.call_tool(
            "alpha_get_tool_schema",
            {"tool_name": "echo"},
        )
        assert "echo" in schema.content[0].text
        assert "message" in schema.content[0].text

        listed = await client.call_tool("alpha_list_tools", {})
        assert "echo" in listed.content[0].text
        assert "add" in listed.content[0].text
