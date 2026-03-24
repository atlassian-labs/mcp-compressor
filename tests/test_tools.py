"""Tests for mcp_compressor/tools.py."""

import pytest
import toons
from fastmcp.tools import Tool

from mcp_compressor.tools import CompressedTools, ToolNotFoundError, sanitize_tool_name
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


class TestToolNotFoundError:
    """Tests for ToolNotFoundError."""

    def test_error_message_contains_tool_name_and_available_tools(self) -> None:
        """Test that the error message includes the tool name and available tools."""
        error = ToolNotFoundError("missing_tool", ["add", "do_nothing"])
        assert "missing_tool" in str(error)
        assert "Available tools: add, do_nothing" in str(error)
        assert error.tool_name == "missing_tool"
        assert error.available_tools == ("add", "do_nothing")


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
