import json

import pytest
from fastmcp import Client
from fastmcp.exceptions import ToolError
from mcp.types import TextContent

from mcp_compressor.types import CompressionLevel

expected_tools_2 = {"get_tool_schema", "invoke_tool"}
expected_tools_3 = expected_tools_2 | {"list_tools"}


@pytest.mark.parametrize(
    "proxy_mcp_client,expected_tools",
    [
        (CompressionLevel.LOW, expected_tools_2),
        (CompressionLevel.MEDIUM, expected_tools_2),
        (CompressionLevel.HIGH, expected_tools_2),
        (CompressionLevel.MAX, expected_tools_3),
    ],
    indirect=["proxy_mcp_client"],
)
async def test_list_tools(proxy_mcp_client: Client, expected_tools: set[str]) -> None:
    """Test that the list_tools function works correctly."""
    tools = await proxy_mcp_client.list_tools()
    assert len(tools) == len(expected_tools)
    for tool in tools:
        assert any(tool.name.endswith(expected_tool) for expected_tool in expected_tools)


@pytest.mark.parametrize(
    "proxy_mcp_client",
    [CompressionLevel.LOW, CompressionLevel.MEDIUM, CompressionLevel.HIGH],
    indirect=True,
)
async def test_get_tool_schema_description(proxy_mcp_client: Client, backend_mcp_client: Client) -> None:
    """Test that the get_tool_schema function returns descriptions containing all backend tool names."""
    tools = await proxy_mcp_client.list_tools()
    get_tool_schema_tool = None
    for tool in tools:
        if tool.name.endswith("get_tool_schema"):
            get_tool_schema_tool = tool
            break
    assert get_tool_schema_tool is not None
    get_tool_schema_description = str(get_tool_schema_tool.description)

    for backend_tool in await backend_mcp_client.list_tools():
        assert backend_tool.name in get_tool_schema_description


@pytest.mark.parametrize("passthrough_method", ["list_prompts", "list_resources"])
async def test_passthrough_methods(
    proxy_mcp_client: Client, backend_mcp_client: Client, passthrough_method: str
) -> None:
    """Test that passthrough methods work correctly."""
    proxy_method = getattr(proxy_mcp_client, passthrough_method)
    backend_method = getattr(backend_mcp_client, passthrough_method)

    proxy_result = await proxy_method()
    backend_result = await backend_method()

    assert proxy_result == backend_result


async def test_get_tool_schema_returns_backend_schemas(proxy_mcp_client: Client, backend_mcp_client: Client) -> None:
    """Test that get_tool_schema returns the same schemas as the backend MCP server."""
    backend_tools = await backend_mcp_client.list_tools()
    for backend_tool in backend_tools:
        result = await proxy_mcp_client.call_tool("test_server_get_tool_schema", {"tool_name": backend_tool.name})
        assert result.content
        assert isinstance(result.content[0], TextContent)
        assert json.dumps(backend_tool.inputSchema, indent=2) in result.content[0].text


@pytest.mark.parametrize(
    "tool_name,tool_args,expected_result",
    [
        ("do_nothing", {"arg": "Hello, World!"}, "Hello, World!"),
        ("add", {"a": 5, "b": 7}, "12"),
        ("throw_error", {"message": "Test error"}, ToolError()),
    ],
)
async def test_invoke_tool_propagates_results_and_errors(
    proxy_mcp_client: Client, tool_name: str, tool_args: dict, expected_result: str | Exception
) -> None:
    """Test that invoke_tool works correctly through the proxy."""
    if isinstance(expected_result, Exception):
        with pytest.raises(type(expected_result)):
            await proxy_mcp_client.call_tool(tool_name, tool_args)
        return

    result = await proxy_mcp_client.call_tool(tool_name, tool_args)
    assert result.content
    assert isinstance(result.content[0], TextContent)
    assert result.content[0].text == expected_result
