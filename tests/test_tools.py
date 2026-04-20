"""Tests for mcp_compressor/tools.py."""

from unittest.mock import AsyncMock

import pytest
import toons
from fastmcp import FastMCP
from fastmcp.server import create_proxy
from fastmcp.server.context import Context
from fastmcp.server.middleware import MiddlewareContext
from fastmcp.server.providers.proxy import ProxyTool
from fastmcp.tools import Tool, ToolResult
from mcp.types import CallToolRequestParams, TextContent

from mcp_compressor.tools import (
    CompressedTools,
    InvokeToolCompatibilityMiddleware,
    ToolNotFoundError,
    sanitize_tool_name,
)
from mcp_compressor.types import CompressionLevel


@pytest.mark.parametrize(
    "input_name,expected",
    [
        # Valid characters pass through unchanged
        ("my_tool", "my_tool"),
        ("my-tool", "my-tool"),
        ("my.tool", "my.tool"),
        ("MyTool123", "mytool123"),
        # Invalid characters are replaced with underscores
        ("my tool", "my_tool"),
        ("my!tool", "my_tool"),
        ("my@tool#name", "my_tool_name"),
        ("tool with spaces!", "tool_with_spaces_"),
        # Mixed valid and invalid
        ("github_get-schema.v1!", "github_get-schema.v1_"),
    ],
)
def test_sanitize_tool_name(input_name: str, expected: str) -> None:
    """Test that invalid characters are replaced with underscores."""
    assert sanitize_tool_name(input_name) == expected


def test_sanitize_tool_name_truncates_long_names() -> None:
    """Test that names longer than 128 characters are truncated."""
    long_name = "a" * 150
    result = sanitize_tool_name(long_name)
    assert len(result) == 128
    assert result == "a" * 128


def test_sanitize_tool_name_all_invalid_chars_become_underscores() -> None:
    """Test that all-invalid input becomes underscores."""
    assert sanitize_tool_name("!!!") == "___"


class TestCompressedTools:
    """Tests for the CompressedTools class."""

    @pytest.fixture
    def compressed_tools(self) -> CompressedTools:
        """Create a CompressedTools instance for testing."""
        # We don't need a real proxy server for these tests
        return CompressedTools(None, CompressionLevel.LOW, server_name=None)  # type: ignore[arg-type]

    @pytest.fixture
    def sample_tool(self) -> Tool:
        """Create a sample tool for testing."""

        def dummy_fn(param1: str, param2: int) -> str:
            """First sentence of description. Second sentence here.

            More details on another line.
            """
            return ""

        return Tool.from_function(dummy_fn)

    @pytest.mark.parametrize(
        "compression_level,expected_in_result",
        [
            # LOW keeps full first line of description
            (CompressionLevel.LOW, ": First sentence of description. Second sentence here."),
            # MEDIUM takes only up to first period
            (CompressionLevel.MEDIUM, ": First sentence of description"),
            # HIGH removes description entirely
            (CompressionLevel.HIGH, "dummy_fn(param1, param2)</tool>"),
        ],
    )
    def test_compression_levels(
        self,
        compressed_tools: CompressedTools,
        sample_tool: Tool,
        compression_level: CompressionLevel,
        expected_in_result: str,
    ) -> None:
        """Test that different compression levels produce appropriate output."""
        result = compressed_tools._format_tool_description(sample_tool, compression_level)
        assert expected_in_result in result
        assert result.startswith("<tool>dummy_fn(param1, param2)")
        assert result.endswith("</tool>")

    def test_tool_with_no_description(self, compressed_tools: CompressedTools) -> None:
        """Test formatting a tool with no description."""

        def no_desc_tool(arg: str) -> str:
            return arg

        tool = Tool.from_function(no_desc_tool)
        tool.description = None
        result = compressed_tools._format_tool_description(tool, CompressionLevel.LOW)
        assert result == "<tool>no_desc_tool(arg)</tool>"

    def test_tool_with_no_parameters(self, compressed_tools: CompressedTools) -> None:
        """Test formatting a tool with no parameters."""

        def empty_tool() -> None:
            """A tool with no params."""
            pass

        tool = Tool.from_function(empty_tool)
        result = compressed_tools._format_tool_description(tool, CompressionLevel.LOW)
        assert result == "<tool>empty_tool(): A tool with no params.</tool>"

    def test_toonify_json_text_converts_objects_and_arrays(self, compressed_tools: CompressedTools) -> None:
        """Test that toonify converts JSON object/array strings to TOON."""
        assert compressed_tools._toonify_json_text('{"name":"Alice","age":30}') == toons.dumps({
            "name": "Alice",
            "age": 30,
        })
        assert compressed_tools._toonify_json_text('[{"id":1},{"id":2}]') == toons.dumps([{"id": 1}, {"id": 2}])

    def test_toonify_json_text_leaves_non_json_text_unchanged(self, compressed_tools: CompressedTools) -> None:
        """Test that toonify leaves non-JSON text unchanged."""
        assert compressed_tools._toonify_json_text("plain text") == "plain text"
        assert compressed_tools._toonify_json_text("123") == "123"


async def test_configure_server_applies_visibility_filters_for_backend_tools() -> None:
    """Test that include/exclude filters are translated into FastMCP visibility rules."""

    class FakeProxyServer:
        def __init__(self) -> None:
            self.enabled_calls: list[dict] = []
            self.disabled_calls: list[dict] = []
            self.middleware: list[object] = []
            self.transforms: list[object] = []
            self.tools = [
                Tool.from_function(lambda a, b: a + b, name="add"),
                Tool.from_function(lambda arg: arg, name="do_nothing"),
                Tool.from_function(lambda: None, name="empty_tool"),
            ]

        def enable(self, **kwargs):
            self.enabled_calls.append(kwargs)
            return self

        def disable(self, **kwargs):
            self.disabled_calls.append(kwargs)
            return self

        def add_middleware(self, middleware) -> None:
            self.middleware.append(middleware)

        def add_transform(self, transform) -> None:
            self.transforms.append(transform)

        async def list_tools(self, *, run_middleware: bool = True):
            return self.tools

    proxy_server = FakeProxyServer()
    compressed_tools = CompressedTools(
        proxy_server,  # type: ignore[arg-type]
        CompressionLevel.LOW,
        server_name="test_server",
        include_tools=["add", "do_nothing"],
        exclude_tools=["do_nothing"],
    )

    await compressed_tools.configure_server()

    assert proxy_server.enabled_calls == []
    assert proxy_server.disabled_calls == [
        {
            "names": {"empty_tool"},
            "components": {"tool"},
        },
        {
            "names": {"do_nothing"},
            "components": {"tool"},
        },
    ]
    assert proxy_server.transforms == [compressed_tools]
    assert len(proxy_server.middleware) == 1


class FakeProxyServer:
    """Minimal proxy server fake for caching tests."""

    def __init__(self, tools: list[Tool]) -> None:
        self.tools = tools
        self.list_tools_call_count = 0
        self.disabled_calls: list[dict] = []
        self.middleware: list[object] = []
        self.transforms: list[object] = []

    def disable(self, **kwargs):
        self.disabled_calls.append(kwargs)
        return self

    def add_middleware(self, middleware) -> None:
        self.middleware.append(middleware)

    def add_transform(self, transform) -> None:
        self.transforms.append(transform)

    async def list_tools(self, *, run_middleware: bool = True):
        self.list_tools_call_count += 1
        return self.tools


async def test_tool_cache_warmed_at_configure_server() -> None:
    """Tool catalog should be cached after configure_server() so no extra backend calls are made."""
    tools = [
        Tool.from_function(lambda a, b: a + b, name="add"),
        Tool.from_function(lambda arg: arg, name="echo"),
    ]
    proxy_server = FakeProxyServer(tools)
    compressed_tools = CompressedTools(
        proxy_server,  # type: ignore[arg-type]
        CompressionLevel.LOW,
        server_name="test",
    )

    await compressed_tools.configure_server()

    # Cache should be populated after configure_server
    assert compressed_tools._cached_backend_tools is not None
    assert set(compressed_tools._cached_backend_tools.keys()) == {"add", "echo"}
    # No include/exclude filters, so list_tools is called exactly once and the
    # result is reused directly for the cache (no redundant second fetch).
    assert proxy_server.list_tools_call_count == 1


async def test_get_backend_tools_uses_cache_after_configure_server() -> None:
    """_get_backend_tools() should not call the backend after the cache is warmed."""

    backend = FastMCP(name="backend")

    @backend.tool()
    def my_tool() -> str:
        """A test tool."""
        return "result"

    proxy_server = create_proxy(backend, name="proxy")
    compressed_tools = CompressedTools(
        proxy_server,
        CompressionLevel.LOW,
        server_name="test",
    )

    await compressed_tools.configure_server()

    # Cache should be warm — record what's in it
    assert compressed_tools._cached_backend_tools is not None
    assert "my_tool" in compressed_tools._cached_backend_tools

    # Patch out the backend to confirm no further fetches happen
    original_cache = compressed_tools._cached_backend_tools

    async with Context(fastmcp=proxy_server) as ctx:
        result1 = await compressed_tools._get_backend_tools(ctx)
        result2 = await compressed_tools._get_backend_tools(ctx)
        result3 = await compressed_tools._get_backend_tools(ctx)

    # All calls should return the same cached dict object (identity check)
    assert result1 is original_cache
    assert result2 is original_cache
    assert result3 is original_cache


async def test_invalidate_tool_cache_forces_refetch() -> None:
    """invalidate_tool_cache() should clear the cache so the next call re-fetches from backend."""

    backend = FastMCP(name="backend")

    @backend.tool()
    def my_tool() -> str:
        """A test tool."""
        return "result"

    proxy_server = create_proxy(backend, name="proxy")
    compressed_tools = CompressedTools(
        proxy_server,
        CompressionLevel.LOW,
        server_name="test",
    )

    await compressed_tools.configure_server()
    original_cache = compressed_tools._cached_backend_tools

    # Invalidate the cache
    compressed_tools.invalidate_tool_cache()
    assert compressed_tools._cached_backend_tools is None

    # Next call should re-fetch and produce a new cache object
    async with Context(fastmcp=proxy_server) as ctx:
        result = await compressed_tools._get_backend_tools(ctx)

    assert set(result.keys()) == {"my_tool"}
    assert compressed_tools._cached_backend_tools is not None
    # A fresh dict was created (different object from original)
    assert compressed_tools._cached_backend_tools is not original_cache


async def test_get_backend_tools_lazy_fetch_when_cache_cold() -> None:
    """_get_backend_tools() should fetch from backend if cache is cold (configure_server not called)."""

    backend = FastMCP(name="backend")

    @backend.tool()
    def lazy_tool() -> str:
        """A lazy test tool."""
        return "result"

    proxy_server = create_proxy(backend, name="proxy")
    compressed_tools = CompressedTools(
        proxy_server,
        CompressionLevel.LOW,
        server_name="test",
    )

    # Cache is cold — configure_server was not called
    assert compressed_tools._cached_backend_tools is None

    async with Context(fastmcp=proxy_server) as ctx:
        result = await compressed_tools._get_backend_tools(ctx)

    assert set(result.keys()) == {"lazy_tool"}
    # Cache should now be populated
    assert compressed_tools._cached_backend_tools is not None


class TestToolNotFoundError:
    """Tests for ToolNotFoundError."""

    def test_error_message_contains_tool_name_and_available_tools(self) -> None:
        """Test that the error message includes the tool name and available tools."""
        error = ToolNotFoundError("missing_tool", ["add", "do_nothing"])
        assert "missing_tool" in str(error)
        assert "Available tools: add, do_nothing" in str(error)
        assert error.tool_name == "missing_tool"
        assert error.available_tools == ("add", "do_nothing")


async def test_invoke_tool_passes_ctx_to_proxy_tool() -> None:
    """invoke_tool should pass ctx to ProxyTool.run() so that request meta is forwarded."""

    backend = FastMCP(name="backend")

    @backend.tool()
    def my_tool() -> str:
        """A test tool."""
        return "hello"

    proxy_server = create_proxy(backend, name="proxy")
    compressed_tools = CompressedTools(proxy_server, CompressionLevel.LOW, server_name="test")
    await compressed_tools.configure_server()

    # Verify the cached tool is a ProxyTool (the proxy wraps backend tools as ProxyTools)
    assert compressed_tools._cached_backend_tools is not None
    cached_tool = compressed_tools._cached_backend_tools["my_tool"]
    assert isinstance(cached_tool, ProxyTool), "Expected a ProxyTool in proxy server cache"

    captured: dict = {}

    # Subclass ProxyTool to intercept run() and capture what context is passed
    class CapturingProxyTool(ProxyTool):
        async def run(self, arguments: dict, context: Context | None = None) -> ToolResult:
            captured["context"] = context
            return ToolResult(content=[TextContent(type="text", text="hello")])

    # Swap the real ProxyTool for our capturing subclass in the cache
    capturing_tool = cached_tool.model_copy(update={})
    capturing_tool.__class__ = CapturingProxyTool
    original_cache = compressed_tools._cached_backend_tools
    compressed_tools._cached_backend_tools = {"my_tool": capturing_tool}

    try:
        async with Context(fastmcp=proxy_server) as ctx:
            await compressed_tools.invoke_tool("my_tool", {}, ctx=ctx)
    finally:
        compressed_tools._cached_backend_tools = original_cache

    assert "context" in captured, "context was not passed to ProxyTool.run()"
    assert captured["context"] is ctx, "invoke_tool should forward ctx to ProxyTool.run()"


async def test_middleware_passes_fastmcp_context_to_invoke_tool() -> None:
    """InvokeToolCompatibilityMiddleware should pass fastmcp_context to invoke_tool."""

    from fastmcp import FastMCP

    backend = FastMCP(name="backend")

    @backend.tool()
    def echo(msg: str) -> str:
        """Echo a message."""
        return msg

    proxy_server = create_proxy(backend, name="proxy")
    compressed_tools = CompressedTools(proxy_server, CompressionLevel.LOW, server_name="test")
    await compressed_tools.configure_server()

    middleware = InvokeToolCompatibilityMiddleware(compressed_tools)

    captured: dict = {}
    original_invoke = compressed_tools.invoke_tool

    async def capturing_invoke_tool(tool_name, tool_input=None, quiet=False, ctx: Context | None = None):
        captured["ctx"] = ctx
        return await original_invoke(tool_name, tool_input, quiet, ctx)

    compressed_tools.invoke_tool = capturing_invoke_tool  # type: ignore[method-assign]

    async with Context(fastmcp=proxy_server) as ctx:
        mw_context = MiddlewareContext(
            message=CallToolRequestParams(
                name="test_invoke_tool", arguments={"tool_name": "echo", "tool_input": {"msg": "hi"}}
            ),
            method="tools/call",
            fastmcp_context=ctx,
        )
        call_next = AsyncMock(return_value=ToolResult(content=[TextContent(type="text", text="hi")]))
        await middleware.on_call_tool(mw_context, call_next)

    assert "ctx" in captured, "ctx was not passed through middleware to invoke_tool"
    assert captured["ctx"] is ctx, "middleware should forward fastmcp_context as ctx to invoke_tool"


async def test_on_call_tool_extracts_flat_args_as_tool_input(proxy_mcp_client) -> None:
    """Test that invoke_tool creates tool_input from flat args when tool_input is not provided."""
    # Call invoke_tool with flat args (no tool_input wrapper)
    # This simulates how some LLMs call tools with args flattened
    result = await proxy_mcp_client.call_tool(
        "test_server_invoke_tool",
        {"tool_name": "add", "a": 5, "b": 3},
    )
    assert result.content
    assert result.content[0].text == "8"


@pytest.mark.parametrize(
    "tool_args",
    [
        {"tool_name": "empty_tool", "tool_input": {}},
        {"tool_name": "empty_tool", "tool_input": None},
        {"tool_name": "empty_tool"},
    ],
    ids=["empty_dict_tool_input", "null_tool_input", "no_tool_input_key"],
)
async def test_on_call_tool_handles_zero_arg_tool(proxy_mcp_client, tool_args: dict) -> None:
    """Test that invoke_tool correctly handles zero-argument tools.

    This is a regression test for a bug where zero-arg tools caused a Pydantic
    'Unexpected keyword argument' validation error. The root cause was:
    - tool_input={} is falsy, so it fell through to the flat-args path
    - The flat-args filter didn't exclude 'tool_input' itself, so it leaked
      into the backend call as an unexpected kwarg (additionalProperties=false)
    """
    result = await proxy_mcp_client.call_tool("test_server_invoke_tool", tool_args)
    assert result.content is not None
