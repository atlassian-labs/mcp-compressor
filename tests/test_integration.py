import json

import pytest
from fastmcp import Client
from fastmcp.exceptions import ToolError
from mcp.types import TextContent

from mcp_compressor.tools import QUIET_MODE_THRESHOLD
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


async def test_passthrough_list_prompts(proxy_mcp_client: Client, backend_mcp_client: Client) -> None:
    """Test that list_prompts passes through correctly."""
    proxy_result = await proxy_mcp_client.list_prompts()
    backend_result = await backend_mcp_client.list_prompts()
    assert proxy_result == backend_result


async def test_passthrough_list_resources(proxy_mcp_client: Client, backend_mcp_client: Client) -> None:
    """Test that backend resources are passed through and the compressor resource is added."""
    proxy_result = await proxy_mcp_client.list_resources()
    backend_result = await backend_mcp_client.list_resources()

    # All backend resources should be present in the proxy
    backend_uris = {str(r.uri) for r in backend_result}
    proxy_uris = {str(r.uri) for r in proxy_result}
    assert backend_uris.issubset(proxy_uris)

    # The compressor resource should also be present
    assert "compressor://uncompressed-tools" in proxy_uris


async def test_get_tool_schema_returns_backend_schemas(proxy_mcp_client: Client, backend_mcp_client: Client) -> None:
    """Test that get_tool_schema returns the same schemas as the backend MCP server."""
    backend_tools = await backend_mcp_client.list_tools()
    for backend_tool in backend_tools:
        result = await proxy_mcp_client.call_tool("test_server_get_tool_schema", {"tool_name": backend_tool.name})
        assert result.content
        assert isinstance(result.content[0], TextContent)
        assert json.dumps(backend_tool.inputSchema, indent=2) in result.content[0].text


async def test_uncompressed_tools_not_listed_as_tool(proxy_mcp_client: Client) -> None:
    """Test that list_uncompressed_tools is not exposed as a tool."""
    tools = await proxy_mcp_client.list_tools()
    tool_names = {tool.name for tool in tools}
    assert "invoke_tool" not in tool_names
    assert all(not tool.name.endswith("list_uncompressed_tools") for tool in tools)


async def test_uncompressed_tools_resource_is_listed(proxy_mcp_client: Client) -> None:
    """Test that the uncompressed tools resource is advertised in list_resources."""
    resources = await proxy_mcp_client.list_resources()
    resource_names = {r.name for r in resources}
    assert "list_uncompressed_tools" in resource_names


async def test_uncompressed_tools_resource_returns_upstream_list_tools_payload(
    proxy_mcp_client: Client, backend_mcp_client: Client
) -> None:
    """Test that the uncompressed tools resource returns the same payload as the upstream list_tools endpoint."""
    backend_tools = await backend_mcp_client.list_tools()
    result = await proxy_mcp_client.read_resource("compressor://uncompressed-tools")

    assert result
    payload = json.loads(result[0].text)
    assert payload == [tool.model_dump(mode="json") for tool in backend_tools]
    annotated_tool = next(tool for tool in payload if tool["name"] == "annotated_tool")
    assert annotated_tool["annotations"]["destructiveHint"] is False
    assert annotated_tool["annotations"]["readOnlyHint"] is True


async def test_hidden_invoke_tool_alias_can_invoke_backend_tools(proxy_mcp_client: Client) -> None:
    """Test that the hidden bare invoke_tool alias remains callable."""
    result = await proxy_mcp_client.call_tool("invoke_tool", {"tool_name": "add", "tool_input": {"a": 5, "b": 7}})

    assert result.content
    assert isinstance(result.content[0], TextContent)
    assert result.content[0].text == "12"


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


@pytest.mark.parametrize(
    "output_length,should_truncate",
    [
        (QUIET_MODE_THRESHOLD - 1, False),  # Under threshold - no truncation
        (QUIET_MODE_THRESHOLD, True),  # At threshold - truncated (uses < comparison)
        (QUIET_MODE_THRESHOLD * 2, True),  # Well over threshold - truncated
    ],
)
async def test_invoke_tool_quiet_mode(proxy_mcp_client: Client, output_length: int, should_truncate: bool) -> None:
    """Test that quiet mode truncates tool outputs at or above threshold."""
    result = await proxy_mcp_client.call_tool(
        "test_server_invoke_tool",
        {"tool_name": "generate_long_output", "tool_input": {"length": output_length}, "quiet": True},
    )

    assert result.content
    assert isinstance(result.content[0], TextContent)
    result_text = result.content[0].text

    if should_truncate:
        # Should be truncated with the marker
        assert "(truncated due to quiet mode)" in result_text
        # Should contain beginning and end preview (half of threshold each)
        preview_length = QUIET_MODE_THRESHOLD // 2
        assert result_text.startswith("X" * preview_length)
        assert result_text.endswith("X" * preview_length)
    else:
        # Should return full output without truncation
        assert "(truncated due to quiet mode)" not in result_text
        assert result_text == "X" * output_length


async def test_invoke_tool_validation_errors_include_schema(proxy_mcp_client: Client) -> None:
    """Test that validation failures include the tool schema in the error message."""
    with pytest.raises(ToolError) as exc_info:
        await proxy_mcp_client.call_tool(
            "test_server_invoke_tool",
            {"tool_name": "add", "tool_input": {"a": "wrong-type"}},
        )

    error_message = str(exc_info.value)
    assert "Tool 'add' input validation failed:" in error_message
    assert "Here is the result of get_tool_schema('add'):" in error_message
    assert "<tool>add(a, b)" in error_message
    assert '"required": [' in error_message
    assert '"a"' in error_message
    assert '"b"' in error_message
    assert '"type": "object"' in error_message


@pytest.mark.parametrize(
    ("wrapper_tool_name", "wrapper_args"),
    [
        ("test_server_get_tool_schema", {"tool_name": "missing_tool"}),
        ("test_server_invoke_tool", {"tool_name": "missing_tool", "tool_input": {}}),
    ],
)
async def test_missing_tool_errors_include_available_tools(
    proxy_mcp_client: Client, wrapper_tool_name: str, wrapper_args: dict
) -> None:
    """Test that missing-tool errors include a list of available backend tools."""
    with pytest.raises(ToolError) as exc_info:
        await proxy_mcp_client.call_tool(wrapper_tool_name, wrapper_args)

    error_message = str(exc_info.value)
    assert "Tool 'missing_tool' not found in backend MCP server." in error_message
    assert "Available tools:" in error_message
    for tool_name in [
        "add",
        "do_nothing",
        "empty_tool",
        "generate_long_output",
        "return_json_string",
        "return_object",
        "return_plain_text",
        "throw_error",
    ]:
        assert tool_name in error_message


async def test_toonify_converts_json_outputs(proxy_mcp_client_toonify: Client) -> None:
    """Test that toonify converts JSON object responses into TOON text."""
    result = await proxy_mcp_client_toonify.call_tool("return_object", {})
    assert result.content
    assert isinstance(result.content[0], TextContent)
    assert result.content[0].text == "name: Alice\nage: 30\ntags[2]: admin,user"

    json_string_result = await proxy_mcp_client_toonify.call_tool("return_json_string", {})
    assert json_string_result.content
    assert isinstance(json_string_result.content[0], TextContent)
    assert json_string_result.content[0].text == "project: mcp-compressor\nstars: 5"


async def test_toonify_leaves_plain_text_unchanged(proxy_mcp_client_toonify: Client) -> None:
    """Test that toonify does not alter non-JSON text outputs."""
    result = await proxy_mcp_client_toonify.call_tool("return_plain_text", {})
    assert result.content
    assert isinstance(result.content[0], TextContent)
    assert result.content[0].text == "plain text"


async def test_toonify_applies_to_invoke_tool_success_response(proxy_mcp_client_toonify: Client) -> None:
    """Test that toonify is applied to successful backend results returned via invoke_tool."""
    result = await proxy_mcp_client_toonify.call_tool(
        "test_server_invoke_tool", {"tool_name": "return_object", "tool_input": {}}
    )
    assert result.content
    assert isinstance(result.content[0], TextContent)
    assert result.content[0].text == "name: Alice\nage: 30\ntags[2]: admin,user"


async def test_toonify_does_not_modify_invoke_tool_error_response(proxy_mcp_client_toonify: Client) -> None:
    """Test that wrapper-generated invoke_tool errors are not toonified."""
    with pytest.raises(ToolError) as exc_info:
        await proxy_mcp_client_toonify.call_tool(
            "test_server_invoke_tool", {"tool_name": "add", "tool_input": {"a": "wrong-type"}}
        )

    error_message = str(exc_info.value)
    assert "Tool 'add' input validation failed:" in error_message
    assert "Here is the result of get_tool_schema('add'):" in error_message
    assert '"required": [' in error_message
