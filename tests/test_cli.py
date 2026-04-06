"""Tests for CLI mode: cli_tools, cli_bridge, cli_script."""

from __future__ import annotations

import inspect
import os
import stat
import sys
from pathlib import Path

import pytest
from fastmcp.tools import Tool
from fastmcp.tools.tool import ToolResult
from mcp.types import TextContent
from starlette.testclient import TestClient

from mcp_compressor.cli_bridge import CliBridge
from mcp_compressor.cli_script import find_script_dir, generate_cli_script, remove_cli_script_entry
from mcp_compressor.cli_tools import (
    build_help_tool_description,
    format_tool_help,
    format_top_level_help,
    parse_argv_to_tool_input,
    sanitize_cli_name,
    subcommand_to_tool_name,
    tool_name_to_subcommand,
)

# ---------------------------------------------------------------------------
# cli_tools helpers
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    "tool_name,expected",
    [
        ("get_confluence_page", "get-confluence-page"),
        ("add", "add"),
        ("create_jira_issue", "create-jira-issue"),
        ("UPPER_CASE", "upper-case"),
    ],
)
def test_tool_name_to_subcommand(tool_name: str, expected: str) -> None:
    assert tool_name_to_subcommand(tool_name) == expected


@pytest.mark.parametrize(
    "subcommand,expected",
    [
        ("get-confluence-page", "get_confluence_page"),
        ("add", "add"),
    ],
)
def test_subcommand_to_tool_name(subcommand: str, expected: str) -> None:
    assert subcommand_to_tool_name(subcommand) == expected


@pytest.mark.parametrize(
    "name,expected",
    [
        ("atlassian", "atlassian"),
        ("Atlassian MCP!", "atlassian-mcp"),
        ("My Server", "my-server"),
        ("123tools", "mcp-123tools"),
        ("  --bad--  ", "bad"),
        ("a" * 100, "a" * 100),
    ],
)
def test_sanitize_cli_name(name: str, expected: str) -> None:
    assert sanitize_cli_name(name) == expected


def _make_tool(name: str, description: str, properties: dict, required: list) -> Tool:
    """Create a minimal Tool instance without going through from_function."""

    # Build a real function with explicit parameters matching the schema
    params = []
    for prop_name, prop_schema in properties.items():
        annotation: type = str  # default
        if prop_schema.get("type") == "integer":
            annotation = int
        elif prop_schema.get("type") == "number":
            annotation = float
        elif prop_schema.get("type") == "boolean":
            annotation = bool
        elif prop_schema.get("type") == "array":
            annotation = list
        param = inspect.Parameter(prop_name, inspect.Parameter.POSITIONAL_OR_KEYWORD, annotation=annotation)
        params.append(param)

    def _fn() -> str:
        return ""

    _fn.__name__ = name
    _fn.__doc__ = description
    _fn.__signature__ = inspect.Signature(params, return_annotation=str)  # type: ignore[attr-defined]
    _fn.__annotations__ = {p.name: p.annotation for p in params}
    _fn.__annotations__["return"] = str

    t = Tool.from_function(_fn)
    t.name = name
    t.description = description
    t.parameters = {"type": "object", "properties": properties, "required": required}
    return t


@pytest.fixture
def add_tool() -> Tool:
    return _make_tool(
        "add",
        "Add two numbers together.",
        {
            "a": {"type": "integer", "description": "First number"},
            "b": {"type": "integer", "description": "Second number"},
        },
        ["a", "b"],
    )


@pytest.fixture
def do_nothing_tool() -> Tool:
    return _make_tool(
        "do_nothing",
        "Does nothing. Second sentence.",
        {"arg": {"type": "string", "description": "A string argument"}},
        ["arg"],
    )


def test_format_top_level_help_includes_subcommands(add_tool: Tool, do_nothing_tool: Tool) -> None:
    text = format_top_level_help("mycli", "the test server", [add_tool, do_nothing_tool])
    assert "mycli" in text
    assert "add" in text
    assert "do-nothing" in text
    assert "SUBCOMMANDS" in text
    assert "mycli <subcommand> --help" in text


def test_format_tool_help_includes_flags_and_description(add_tool: Tool) -> None:
    text = format_tool_help("mycli", add_tool)
    assert "mycli add" in text
    assert "--a" in text
    assert "--b" in text
    assert "required" in text
    assert "Add two numbers" in text


def test_build_help_tool_description_reuses_format_top_level_help(add_tool: Tool, do_nothing_tool: Tool) -> None:
    """build_help_tool_description should contain the same subcommand table as format_top_level_help."""
    desc = build_help_tool_description("mycli", "test server", [add_tool, do_nothing_tool])
    top_help = format_top_level_help("mycli", "test server", [add_tool, do_nothing_tool])
    assert top_help in desc


def test_build_help_tool_description_lists_subcommands(add_tool: Tool, do_nothing_tool: Tool) -> None:
    text = build_help_tool_description("mycli", "the test server", [add_tool, do_nothing_tool])
    assert "add" in text
    assert "do-nothing" in text
    assert "mycli --help" in text


# ---------------------------------------------------------------------------
# parse_argv_to_tool_input
# ---------------------------------------------------------------------------


def test_parse_argv_strings(add_tool: Tool) -> None:
    # integers
    result = parse_argv_to_tool_input(["--a", "5", "--b", "3"], add_tool)
    assert result == {"a": 5, "b": 3}


def test_parse_argv_missing_required_raises(add_tool: Tool) -> None:
    with pytest.raises(ValueError, match="Missing required"):
        parse_argv_to_tool_input(["--a", "5"], add_tool)


def test_parse_argv_unknown_flag_raises(add_tool: Tool) -> None:
    with pytest.raises(ValueError, match="Unknown option"):
        parse_argv_to_tool_input(["--a", "5", "--b", "3", "--unknown", "x"], add_tool)


def test_parse_argv_boolean_flag() -> None:
    tool = _make_tool(
        "t",
        "desc",
        {"flag": {"type": "boolean"}, "name": {"type": "string"}},
        ["name"],
    )
    result = parse_argv_to_tool_input(["--name", "foo", "--flag"], tool)
    assert result == {"name": "foo", "flag": True}


def test_parse_argv_array_repeated() -> None:
    tool = _make_tool(
        "t",
        "desc",
        {"labels": {"type": "array", "items": {"type": "string"}}},
        [],
    )
    result = parse_argv_to_tool_input(["--labels", "bug", "--labels", "urgent"], tool)
    assert result == {"labels": ["bug", "urgent"]}


def test_parse_argv_object_type() -> None:
    """Object-typed props should be JSON-parsed from the CLI string."""
    tool = _make_tool(
        "create_issue",
        "Create an issue",
        {"fields": {"type": "object", "description": "Issue fields"}},
        [],
    )
    result = parse_argv_to_tool_input(["--fields", '{"priority":{"name":"High"}}'], tool)
    assert result == {"fields": {"priority": {"name": "High"}}}


def test_parse_argv_object_type_fallback_to_string() -> None:
    """Object-typed props fall back to raw string if value is not valid JSON."""
    tool = _make_tool(
        "create_issue",
        "Create an issue",
        {"fields": {"type": "object", "description": "Issue fields"}},
        [],
    )
    result = parse_argv_to_tool_input(["--fields", "plain-string"], tool)
    assert result == {"fields": "plain-string"}


def test_parse_argv_array_of_objects() -> None:
    """Array items typed as object should each be JSON-parsed."""
    tool = _make_tool(
        "batch_op",
        "Batch operation",
        {"items": {"type": "array", "items": {"type": "object"}}},
        [],
    )
    result = parse_argv_to_tool_input(
        ["--items", '{"id":1}', "--items", '{"id":2}'],
        tool,
    )
    assert result == {"items": [{"id": 1}, {"id": 2}]}


def test_parse_argv_unknown_type_json_parsed() -> None:
    """Unknown/complex schema types (e.g. missing 'type') should attempt JSON parsing."""
    tool = _make_tool(
        "complex_tool",
        "Complex tool",
        {"data": {"description": "Some data"}},  # no 'type' key
        [],
    )
    result = parse_argv_to_tool_input(["--data", '{"key":"val"}'], tool)
    assert result == {"data": {"key": "val"}}


def test_parse_argv_json_escape_hatch(add_tool: Tool) -> None:
    result = parse_argv_to_tool_input(["--json", '{"a": 10, "b": 20}'], add_tool)
    assert result == {"a": 10, "b": 20}


def test_parse_argv_kebab_to_snake() -> None:
    tool = _make_tool(
        "t",
        "desc",
        {"page_url": {"type": "string"}},
        ["page_url"],
    )
    result = parse_argv_to_tool_input(["--page-url", "https://example.com"], tool)
    assert result == {"page_url": "https://example.com"}


# ---------------------------------------------------------------------------
# cli_script
# ---------------------------------------------------------------------------


def test_generate_cli_script_is_executable(tmp_path: Path) -> None:
    script_path, _ = generate_cli_script("atlassian", bridge_port=12345, session_pid=os.getpid(), script_dir=tmp_path)
    assert script_path.exists()
    assert script_path.name == "atlassian"
    assert script_path.stat().st_mode & stat.S_IXUSR


def test_generate_cli_script_contains_bridge_url(tmp_path: Path) -> None:
    script_path, _ = generate_cli_script("mycli", bridge_port=54321, session_pid=os.getpid(), script_dir=tmp_path)
    content = script_path.read_text()
    assert "http://127.0.0.1:54321" in content
    assert "mycli" in content


def test_generate_cli_script_uses_current_interpreter_and_modern_typing(tmp_path: Path) -> None:
    script_path, _ = generate_cli_script("mycli", bridge_port=54321, session_pid=os.getpid(), script_dir=tmp_path)
    content = script_path.read_text()
    assert content.startswith(f"#!{sys.executable}\n")
    assert "def _find_bridge() -> str | None:" in content
    assert "def _pick_bridge() -> str | None:" in content
    assert "from typing import Optional" not in content


def test_find_script_dir_returns_path_and_bool() -> None:

    script_dir, on_path = find_script_dir()
    assert isinstance(script_dir, Path)
    assert isinstance(on_path, bool)


def test_generate_cli_script_cwd_fallback(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """When no candidate dir is on PATH, falls back to CWD (supplied via script_dir=None + mock)."""
    from mcp_compressor import cli_script

    monkeypatch.setattr(cli_script, "_UNIX_CANDIDATE_SCRIPT_DIRS", [])
    monkeypatch.setattr(cli_script, "_WINDOWS_CANDIDATE_SCRIPT_DIRS", [])
    monkeypatch.chdir(tmp_path)
    script_path, on_path = generate_cli_script("mycli", bridge_port=1234, session_pid=os.getpid())
    assert script_path.parent == tmp_path
    assert on_path is False


def test_generate_cli_script_multi_instance(tmp_path: Path) -> None:
    """Two instances with different session PIDs should both appear in BRIDGES."""
    from mcp_compressor import cli_script as cs

    original = cs._live_bridges

    def no_prune(bridges: dict) -> dict:
        return bridges

    cs._live_bridges = no_prune  # type: ignore[assignment]
    try:
        generate_cli_script("mycli", bridge_port=1111, session_pid=100, script_dir=tmp_path)
        generate_cli_script("mycli", bridge_port=2222, session_pid=200, script_dir=tmp_path)
    finally:
        cs._live_bridges = original  # type: ignore[assignment]

    content = (tmp_path / "mycli").read_text()
    assert "127.0.0.1:1111" in content
    assert "127.0.0.1:2222" in content
    assert "100" in content
    assert "200" in content


def test_remove_cli_script_entry_last_entry_deletes_file(tmp_path: Path) -> None:
    """Removing the only entry deletes the script file."""
    generate_cli_script("mycli", bridge_port=1111, session_pid=42, script_dir=tmp_path)
    assert (tmp_path / "mycli").exists()
    remove_cli_script_entry("mycli", session_pid=42, script_dir=tmp_path)
    assert not (tmp_path / "mycli").exists()


def test_remove_cli_script_entry_other_entries_preserved(tmp_path: Path) -> None:
    """Removing one entry keeps the other in the script."""
    generate_cli_script("mycli", bridge_port=1111, session_pid=100, script_dir=tmp_path)
    generate_cli_script("mycli", bridge_port=2222, session_pid=200, script_dir=tmp_path)
    # Port 2222 is not alive, so it will be pruned by _live_bridges on next write.
    # We just check that after removing 100's entry, 200's port stays.
    # Patch _live_bridges to not prune so test is deterministic.
    from mcp_compressor import cli_script as cs

    original = cs._live_bridges

    def no_prune(bridges: dict) -> dict:
        return bridges

    cs._live_bridges = no_prune  # type: ignore[assignment]
    try:
        remove_cli_script_entry("mycli", session_pid=100, script_dir=tmp_path)
    finally:
        cs._live_bridges = original  # type: ignore[assignment]

    content = (tmp_path / "mycli").read_text()
    assert "127.0.0.1:1111" not in content
    assert "127.0.0.1:2222" in content
    assert (tmp_path / "mycli").exists()


def test_generate_windows_cmd_script(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """On Windows, generates a .cmd batch script."""
    from mcp_compressor import cli_script

    monkeypatch.setattr(cli_script, "_IS_WINDOWS", True)
    script_path, _ = generate_cli_script("atlassian", bridge_port=9999, session_pid=os.getpid(), script_dir=tmp_path)
    assert script_path.suffix == ".cmd"
    assert script_path.name == "atlassian.cmd"
    content = script_path.read_text()
    assert "@echo off" in content
    assert "http://127.0.0.1:9999" in content
    assert "powershell" in content
    assert "Invoke-WebRequest" in content
    assert "ConvertTo-Json" in content


# ---------------------------------------------------------------------------
# cli_bridge
# ---------------------------------------------------------------------------


@pytest.fixture
def bridge_tools(add_tool: Tool, do_nothing_tool: Tool) -> dict:
    return {"add": add_tool, "do_nothing": do_nothing_tool}


@pytest.fixture
def mock_invoke_fn(add_tool: Tool):
    from fastmcp.tools.tool import ToolResult

    async def invoke(tool_name: str, tool_input: dict | None, quiet: bool = False) -> ToolResult:
        if tool_name == "add":
            val = (tool_input or {}).get("a", 0) + (tool_input or {}).get("b", 0)
            text = str(val)
            if quiet:
                text = f"[quiet] {text}"
            return ToolResult(content=[TextContent(type="text", text=text)])
        return ToolResult(content=[TextContent(type="text", text="ok")])

    return invoke


@pytest.fixture
def bridge(bridge_tools, mock_invoke_fn) -> CliBridge:
    async def get_tools_fn() -> dict:
        return bridge_tools

    return CliBridge(
        cli_name="mycli",
        server_description="the test server",
        get_tools_fn=get_tools_fn,
        invoke_fn=mock_invoke_fn,
        port=0,
    )


async def test_bridge_health(bridge: CliBridge) -> None:

    client = TestClient(bridge.app)
    response = client.get("/health")
    assert response.status_code == 200
    assert response.text == "ok"


async def test_bridge_top_level_help(bridge: CliBridge) -> None:

    client = TestClient(bridge.app)
    response = client.post("/exec", json={"argv": ["--help"]})
    assert response.status_code == 200
    assert "SUBCOMMANDS" in response.text
    assert "add" in response.text
    assert "do-nothing" in response.text


async def test_bridge_per_tool_help(bridge: CliBridge) -> None:

    client = TestClient(bridge.app)
    response = client.post("/exec", json={"argv": ["add", "--help"]})
    assert response.status_code == 200
    assert "--a" in response.text
    assert "--b" in response.text


async def test_bridge_invokes_tool(bridge: CliBridge) -> None:

    client = TestClient(bridge.app)
    response = client.post("/exec", json={"argv": ["add", "--a", "5", "--b", "3"]})
    assert response.status_code == 200
    assert "8" in response.text


async def test_bridge_unknown_subcommand(bridge: CliBridge) -> None:

    client = TestClient(bridge.app)
    response = client.post("/exec", json={"argv": ["nonexistent", "--foo", "bar"]})
    assert response.status_code == 400
    assert "unknown subcommand" in response.text


async def test_bridge_missing_required_arg(bridge: CliBridge) -> None:

    client = TestClient(bridge.app)
    response = client.post("/exec", json={"argv": ["add", "--a", "5"]})
    assert response.status_code == 400
    assert "Missing required" in response.text


async def test_bridge_quiet_flag_is_passed_to_invoke_fn(bridge: CliBridge) -> None:
    """Test that --quiet is extracted from argv and passed to the invoke function."""

    client = TestClient(bridge.app)
    response = client.post("/exec", json={"argv": ["add", "--a", "5", "--b", "3", "--quiet"]})
    assert response.status_code == 200
    # The mock_invoke_fn prefixes output with "[quiet]" when quiet=True
    assert "[quiet] 8" in response.text


async def test_bridge_without_quiet_flag_passes_false_to_invoke_fn(bridge: CliBridge) -> None:
    """Test that quiet=False is passed when --quiet is not in argv."""

    client = TestClient(bridge.app)
    response = client.post("/exec", json={"argv": ["add", "--a", "5", "--b", "3"]})
    assert response.status_code == 200
    assert response.text.strip() == "8"
    assert "[quiet]" not in response.text


async def test_bridge_quiet_flag_not_passed_to_tool_input(bridge: CliBridge) -> None:
    """Test that --quiet is stripped from argv before parse_argv_to_tool_input, so it doesn't
    cause 'Unknown option' errors for tools that don't declare a 'quiet' parameter."""

    client = TestClient(bridge.app)
    # If --quiet leaked into tool input parsing it would raise "Unknown option: --quiet"
    response = client.post("/exec", json={"argv": ["add", "--quiet", "--a", "5", "--b", "3"]})
    assert response.status_code == 200
    assert "[quiet] 8" in response.text


async def test_format_tool_help_includes_quiet_flag(add_tool: Tool) -> None:
    """Test that --quiet appears in per-subcommand help output."""
    from mcp_compressor.cli_tools import format_tool_help

    help_text = format_tool_help("mycli", add_tool)
    assert "--quiet" in help_text
    assert "Truncate large output" in help_text
