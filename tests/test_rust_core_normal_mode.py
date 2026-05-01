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

        resources = {str(resource.uri) for resource in await client.list_resources()}
        assert "fixture://alpha-resource" in resources
        assert "compressor://alpha/uncompressed-tools" in resources

        alpha_resource = await client.read_resource("fixture://alpha-resource")
        assert alpha_resource[0].text == "alpha resource"

        compressor_resource = await client.read_resource("compressor://alpha/uncompressed-tools")
        assert "echo" in compressor_resource[0].text
        assert "add" in compressor_resource[0].text

        prompts = {prompt.name for prompt in await client.list_prompts()}
        assert "alpha_prompt" in prompts
