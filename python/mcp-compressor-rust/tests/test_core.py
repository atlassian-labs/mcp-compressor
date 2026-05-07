from __future__ import annotations

import json
import os
from pathlib import Path
from urllib import error, request

from mcp_compressor_rust import (
    BackendConfig,
    CompressedSessionConfig,
    RustTool,
    compress_tool_listing,
    format_tool_schema_response,
    parse_mcp_config,
    parse_tool_argv,
    start_compressed_session,
    start_compressed_session_from_mcp_config,
)

ROOT = Path(__file__).resolve().parents[2].parent
FIXTURES = ROOT / "crates" / "mcp-compressor-core" / "tests" / "fixtures"
PYTHON = os.environ.get("PYTHON") or str(ROOT / ".venv" / "bin" / "python")


def invoke_proxy(bridge_url: str, token: str, tool: str, tool_name: str, tool_input: dict[str, object]) -> str:
    body = json.dumps({"tool": tool, "input": {"tool_name": tool_name, "tool_input": tool_input}}).encode()
    req = request.Request(  # noqa: S310 - local Rust test proxy
        f"{bridge_url}/exec",
        data=body,
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        method="POST",
    )
    with request.urlopen(req, timeout=10) as response:  # noqa: S310 - local Rust test proxy
        return response.read().decode()


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


def test_native_extension_starts_session_and_invokes_backend() -> None:
    session = start_compressed_session(
        CompressedSessionConfig(compression_level="max", server_name="alpha"),
        [BackendConfig(name="alpha", command_or_url=PYTHON, args=[str(FIXTURES / "alpha_server.py")])],
    )
    info = session.info()
    assert str(info["bridge_url"]).startswith("http://127.0.0.1:")
    invoke_tool = next(tool for tool in info["frontend_tools"] if tool["name"].endswith("invoke_tool"))
    assert (
        invoke_proxy(str(info["bridge_url"]), str(info["token"]), invoke_tool["name"], "echo", {"message": "py"})
        == "alpha:py"
    )


def test_python_agent_can_start_compressed_multi_server_proxy_without_compressor_subprocess(monkeypatch) -> None:
    monkeypatch.setenv("MCP_COMPRESSOR_CORE_BINARY", os.devnull + "-missing")
    monkeypatch.setenv("PATH", "")
    session = start_compressed_session_from_mcp_config(
        CompressedSessionConfig(compression_level="max"),
        json.dumps(
            {
                "mcpServers": {
                    "alpha": {"command": PYTHON, "args": [str(FIXTURES / "alpha_server.py")]},
                    "beta": {"command": PYTHON, "args": [str(FIXTURES / "beta_server.py")]},
                }
            }
        ),
    )
    try:
        info = session.info()
        tool_names = {tool["name"] for tool in info["frontend_tools"]}
        assert {"alpha_get_tool_schema", "alpha_invoke_tool", "beta_get_tool_schema", "beta_invoke_tool"}.issubset(
            tool_names
        )
        assert (
            invoke_proxy(str(info["bridge_url"]), str(info["token"]), "alpha_invoke_tool", "echo", {"message": "agent"})
            == "alpha:agent"
        )
        assert (
            invoke_proxy(str(info["bridge_url"]), str(info["token"]), "beta_invoke_tool", "multiply", {"a": 6, "b": 7})
            == "42"
        )
    finally:
        session.close()


def test_native_extension_starts_session_from_mcp_config_and_routes() -> None:
    session = start_compressed_session_from_mcp_config(
        CompressedSessionConfig(compression_level="max"),
        json.dumps(
            {
                "mcpServers": {
                    "alpha": {"command": PYTHON, "args": [str(FIXTURES / "alpha_server.py")]},
                    "beta": {"command": PYTHON, "args": [str(FIXTURES / "beta_server.py")]},
                }
            }
        ),
    )
    info = session.info()
    tool_names = {tool["name"] for tool in info["frontend_tools"]}
    assert "alpha_invoke_tool" in tool_names
    assert "beta_invoke_tool" in tool_names
    assert (
        invoke_proxy(str(info["bridge_url"]), str(info["token"]), "alpha_invoke_tool", "add", {"a": 2, "b": 3}) == "5"
    )
    assert (
        invoke_proxy(str(info["bridge_url"]), str(info["token"]), "beta_invoke_tool", "multiply", {"a": 4, "b": 5})
        == "20"
    )


def test_native_extension_toonifies_json_outputs() -> None:
    session = start_compressed_session(
        CompressedSessionConfig(compression_level="max", server_name="alpha", toonify=True),
        [BackendConfig(name="alpha", command_or_url=PYTHON, args=[str(FIXTURES / "alpha_server.py")])],
    )
    info = session.info()
    invoke_tool = next(tool for tool in info["frontend_tools"] if tool["name"].endswith("invoke_tool"))
    output = invoke_proxy(str(info["bridge_url"]), str(info["token"]), invoke_tool["name"], "structured_data", {})
    assert "server: alpha" in output
    assert "values" in output
    assert not output.strip().startswith("{")


def test_native_extension_applies_include_exclude_filters() -> None:
    session = start_compressed_session(
        CompressedSessionConfig(
            compression_level="max",
            server_name="alpha",
            include_tools=["echo", "add"],
            exclude_tools=["add"],
        ),
        [BackendConfig(name="alpha", command_or_url=PYTHON, args=[str(FIXTURES / "alpha_server.py")])],
    )
    info = session.info()
    invoke_tool = next(tool for tool in info["frontend_tools"] if tool["name"].endswith("invoke_tool"))
    assert (
        invoke_proxy(str(info["bridge_url"]), str(info["token"]), invoke_tool["name"], "echo", {"message": "filtered"})
        == "alpha:filtered"
    )
    try:
        invoke_proxy(str(info["bridge_url"]), str(info["token"]), invoke_tool["name"], "add", {"a": 1, "b": 2})
    except error.HTTPError as exc:
        assert exc.code == 400
        assert "not found" in exc.read().decode().lower()
    else:  # pragma: no cover - defensive assertion for filter enforcement
        raise AssertionError("excluded add tool unexpectedly invoked")


def test_native_extension_supports_cli_transform_mode() -> None:
    session = start_compressed_session(
        CompressedSessionConfig(compression_level="max", server_name="alpha", transform_mode="cli"),
        [BackendConfig(name="alpha", command_or_url=PYTHON, args=[str(FIXTURES / "alpha_server.py")])],
    )
    info = session.info()
    assert [tool["name"] for tool in info["frontend_tools"]] == ["alpha_alpha_help"]


def test_native_extension_supports_just_bash_transform_mode() -> None:
    session = start_compressed_session(
        CompressedSessionConfig(compression_level="max", transform_mode="just-bash"),
        [
            BackendConfig(name="alpha", command_or_url=PYTHON, args=[str(FIXTURES / "alpha_server.py")]),
            BackendConfig(name="beta", command_or_url=PYTHON, args=[str(FIXTURES / "beta_server.py")]),
        ],
    )
    info = session.info()
    tool_names = {tool["name"] for tool in info["frontend_tools"]}
    assert {"bash_tool", "alpha_help", "beta_help"}.issubset(tool_names)
    providers = {provider["provider_name"]: provider for provider in info["just_bash_providers"]}
    assert set(providers) == {"alpha", "beta"}
    assert providers["alpha"]["help_tool_name"] == "alpha_help"
    assert any(command["command_name"] == "echo" for command in providers["alpha"]["tools"])


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
