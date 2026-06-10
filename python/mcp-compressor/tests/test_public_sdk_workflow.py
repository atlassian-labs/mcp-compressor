from __future__ import annotations

import os
import subprocess
import sys
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from mcp_compressor import CompressorClient, ExecutableTool, transform_tools_for_just_bash
from mcp_compressor.core import generate_client_artifact_files

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "crates" / "mcp-compressor-core" / "tests" / "fixtures" / "alpha_server.py"
PYTHON = os.environ.get("PYTHON", sys.executable)


@contextmanager
def _streamable_http_upstream() -> Iterator[str]:
    command = [
        "cargo",
        "run",
        "-q",
        "-p",
        "mcp-compressor-core",
        "--bin",
        "mcp-compressor",
        "--",
        "--compression",
        "max",
        "--server-name",
        "upstream",
        "--transport",
        "streamable-http",
        "--port",
        "0",
        "--",
        PYTHON,
        str(FIXTURE),
    ]
    process = subprocess.Popen(command, stderr=subprocess.PIPE, text=True)  # noqa: S603 - trusted test command
    try:
        assert process.stderr is not None
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            line = process.stderr.readline()
            if "Streamable HTTP MCP server listening on " in line:
                yield line.rsplit(" ", 1)[1].strip()
                return
            if process.poll() is not None:
                raise AssertionError(f"upstream exited early with {process.returncode}")
        raise AssertionError("timed out waiting for upstream URL")
    finally:
        process.terminate()
        process.wait(timeout=10)


@contextmanager
def _rotating_auth_proxy(target_url: str, *, expected_start: int = 1) -> Iterator[str]:
    proxy = ROOT / "crates" / "mcp-compressor-core" / "tests" / "fixtures" / "rotating_auth_proxy.py"
    env = {
        **os.environ,
        "MCP_COMPRESSOR_AUTH_PROXY_TARGET": target_url,
        "MCP_COMPRESSOR_AUTH_PROXY_EXPECTED_START": str(expected_start),
    }
    process = subprocess.Popen(  # noqa: S603 - trusted test command
        [PYTHON, str(proxy)], stderr=subprocess.PIPE, text=True, env=env
    )
    try:
        assert process.stderr is not None
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            line = process.stderr.readline()
            if line.startswith("AUTH_PROXY_URL="):
                yield line.split("=", 1)[1].strip()
                return
            if process.poll() is not None:
                raise AssertionError(f"auth proxy exited early with {process.returncode}")
        raise AssertionError("timed out waiting for auth proxy URL")
    finally:
        process.terminate()
        process.wait(timeout=10)


def test_public_python_sdk_quickstart_flow() -> None:
    with CompressorClient(
        servers={"alpha": {"command": PYTHON, "args": [str(FIXTURE)]}},
        compression_level="max",
    ) as proxy:
        tool_names = {tool.name for tool in proxy.tools}
        assert "alpha_get_tool_schema" in tool_names
        assert "alpha_invoke_tool" in tool_names

        schema = proxy.schema("echo")
        assert "message" in schema["properties"]

        response = proxy.invoke("echo", {"message": "public-python"})
        assert response == "alpha:public-python"

        executable_tools = proxy.to_executable_tools()
        assert (
            executable_tools["alpha_invoke_tool"].execute(
                {"tool_name": "echo", "tool_input": {"message": "executable-python"}}
            )
            == "alpha:executable-python"
        )


def test_public_python_sdk_in_process_session_without_bridge() -> None:
    from mcp_compressor.core import (
        BackendConfig,
        CompressedSessionConfig,
        start_compressed_session,
    )

    session = start_compressed_session(
        CompressedSessionConfig(compression_level="max", server_name="alpha", bridge=False),
        [BackendConfig(name="alpha", command_or_url=PYTHON, args=[str(FIXTURE)])],
    )
    try:
        info = session.info()
        # In-process mode starts no HTTP bridge.
        assert info["bridge_url"] == ""
        assert info["token"] == ""

        tool_names = {tool["name"] for tool in session.list_tools()}
        invoke_tool = next(name for name in tool_names if name.endswith("invoke_tool"))
        schema_tool = next(name for name in tool_names if name.endswith("get_tool_schema"))

        schema = session.get_schema(schema_tool, "echo")
        assert "echo" in schema

        # In-process invoke returns the same payload the bridge /exec would.
        result = session.invoke(
            invoke_tool,
            {"tool_name": "echo", "tool_input": {"message": "in-process"}},
        )
        assert result == "alpha:in-process"
    finally:
        session.close()


def test_public_python_sdk_preserves_oauth_app_name_in_config() -> None:
    from mcp_compressor.core import normalize_sdk_servers

    backends = normalize_sdk_servers(
        {"atlassian": {"url": "https://mcp.example.test/mcp", "oauth_app_name": "Rovo Dev"}}
    )

    assert backends[0].oauth_app_name == "Rovo Dev"


def test_public_python_sdk_auth_provider_refreshes_per_remote_request() -> None:
    calls = 0

    def auth_provider() -> dict[str, str]:
        nonlocal calls
        calls += 1
        return {"Authorization": f"Bearer token-{calls}"}

    with (
        _streamable_http_upstream() as upstream_url,
        _rotating_auth_proxy(upstream_url, expected_start=2) as proxy_url,
        CompressorClient(
            servers={"remote": {"url": proxy_url, "auth_provider": auth_provider}},
            compression_level="max",
        ) as proxy,
    ):
        first = proxy.invoke_wrapper(
            "remote_invoke_tool",
            {
                "tool_name": "upstream_invoke_tool",
                "tool_input": {"tool_name": "echo", "tool_input": {"message": "one"}},
            },
        ).text
        second = proxy.invoke_wrapper(
            "remote_invoke_tool",
            {
                "tool_name": "upstream_invoke_tool",
                "tool_input": {"tool_name": "echo", "tool_input": {"message": "two"}},
            },
        ).text

    assert first == "alpha:one"
    assert second == "alpha:two"
    assert calls >= 2


def test_public_python_sdk_write_code_client_returns_environment(tmp_path: Path) -> None:
    with CompressorClient(
        servers={"alpha": {"command": PYTHON, "args": [str(FIXTURE)]}},
        compression_level="max",
    ) as proxy:
        generated = proxy.write_code_client("python", tmp_path / "py", name="alpha")
        assert generated.language == "python"
        assert generated.environment == {"PYTHONPATH": str(tmp_path / "py")}
        assert any(path.name == "alpha.py" for path in generated.files)

        ts_generated = proxy.write_code_client("typescript", tmp_path / "ts", name="alpha")
        assert ts_generated.language == "typescript"
        assert ts_generated.environment == {}
        assert any(path.name == "alpha.ts" for path in ts_generated.files)


def test_public_python_sdk_direct_just_bash_transform() -> None:
    class BashHost:
        custom_commands: dict[str, object]

        def __init__(self) -> None:
            self.custom_commands = {}

    tools = {
        "echo": ExecutableTool(
            name="echo",
            description="Echo a message.",
            input_schema={
                "type": "object",
                "properties": {"message": {"type": "string"}},
                "required": ["message"],
            },
            execute=lambda input=None: f"direct:{(input or {})['message']}",
        )
    }
    bash = BashHost()
    result = transform_tools_for_just_bash(tools, bash=bash, server_name="alpha")
    assert list(result.tools) == ["alpha_help"]
    assert "alpha_echo" in bash.custom_commands
    assert bash.custom_commands["alpha_echo"](["--message", "python-bash"]) == "direct:python-bash"
    assert "alpha_echo" in result.tools["alpha_help"].execute()


def test_public_python_sdk_generated_file_map(tmp_path: Path) -> None:
    files = generate_client_artifact_files(
        "python",
        cli_name="alpha",
        bridge_url="http://127.0.0.1:12345",
        token="test-session-token",  # noqa: S106 - synthetic token for generated file test
        tools=[
            {
                "name": "echo",
                "description": "Echo a message.",
                "input_schema": {
                    "type": "object",
                    "properties": {"message": {"type": "string"}},
                    "required": ["message"],
                },
            }
        ],
        output_dir=tmp_path,
    )
    assert "alpha.py" in files
    assert "def echo" in files["alpha.py"]


def test_public_python_sdk_schema_lookup_supports_multi_server() -> None:
    with CompressorClient(
        servers={
            "alpha": {"command": PYTHON, "args": [str(FIXTURE)]},
            "beta": {
                "command": PYTHON,
                "args": [str(ROOT / "crates" / "mcp-compressor-core" / "tests" / "fixtures" / "beta_server.py")],
            },
        },
        compression_level="max",
    ) as proxy:
        schema = proxy.schema("echo", server="alpha")
        assert "message" in schema["properties"]

        try:
            proxy.schema("echo")
        except RuntimeError as error:
            assert "Multiple backend tools" in str(error)
        else:  # pragma: no cover - defensive assertion
            raise AssertionError("expected ambiguous schema lookup to fail")
