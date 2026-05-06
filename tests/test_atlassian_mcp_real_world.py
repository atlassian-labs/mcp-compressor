from __future__ import annotations

# These tests intentionally spawn trusted local binaries and call the fixed Atlassian MCP HTTPS endpoint.
# ruff: noqa: S105,S603,S607,S310
import importlib
import json
import os
import queue
import re
import subprocess
import sys
import tempfile
import textwrap
import threading
import time
from pathlib import Path
from typing import Any, cast
from urllib import request

import pytest
from fastmcp import Client

ROOT = Path(__file__).parents[1]
ATLASSIAN_URL = "https://mcp.atlassian.com/v1/mcp"
TOKEN_ENV = "ATLASSIAN_MCP_BASIC_TOKEN"
SAFE_TOOL = "getAccessibleAtlassianResources"
SAFE_PYTHON_FUNCTION = "get_accessible_atlassian_resources"
SAFE_SUBCOMMAND = "get-accessible-atlassian-resources"
CORE_BIN = ROOT / "target" / "debug" / "mcp-compressor-core"


def _token() -> str:
    token = os.environ.get(TOKEN_ENV)
    if not token:
        pytest.skip(f"{TOKEN_ENV} is not set")
    return cast("str", token)


@pytest.fixture(scope="session", autouse=True)
def _build_core() -> None:
    subprocess.run(["cargo", "build", "-p", "mcp-compressor-core"], cwd=ROOT, check=True)


def _backend_args() -> list[str]:
    return [ATLASSIAN_URL, "-H", f"Authorization=Basic {_token()}", "--auth", "explicit-headers"]


def _client_config(*args: str) -> dict[str, Any]:
    return {"mcpServers": {"rust": {"command": str(CORE_BIN), "args": list(args)}}}


def _wait_http(url: str, token: str, timeout: float = 20.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            req = request.Request(f"{url}/health", headers={"Authorization": f"Bearer {token}"})
            with request.urlopen(req, timeout=2) as response:
                if response.status == 200:
                    return
        except Exception:
            time.sleep(0.2)
    raise AssertionError(f"proxy did not become ready: {url}")


def _start_proxy_mode(
    *extra_args: str, script_name: str | None = "atlassian"
) -> tuple[subprocess.Popen[str], str, str, str | None]:
    env = os.environ.copy()
    explicit_output_dir = next(
        (extra_args[index + 1] for index, arg in enumerate(extra_args[:-1]) if arg == "--output-dir"), None
    )
    output_dir = explicit_output_dir or tempfile.mkdtemp(prefix="mcp-compressor-atlassian-")
    env["MCP_COMPRESSOR_CLI_OUTPUT_DIR"] = output_dir
    child = subprocess.Popen(
        [
            str(CORE_BIN),
            "--cli-mode",
            "--server-name",
            "atlassian",
            *extra_args,
            "--",
            *_backend_args(),
        ],
        cwd=ROOT,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    bridge_url = ""
    deadline = time.time() + 40
    stdout_lines: list[str] = []
    while time.time() < deadline:
        line = child.stdout.readline() if child.stdout else ""
        if line:
            stdout_lines.append(line)
            match = re.search(r"Bridge URL: (http://[^\s]+)", line)
            if match:
                bridge_url = match.group(1)
            if "Press Ctrl+C to stop" in line and bridge_url:
                token = None
                if script_name is not None:
                    script = Path(output_dir) / script_name
                    token = script.read_text().split("TOKEN=", 1)[1].split("\n", 1)[0].strip("'\"")
                    _wait_http(bridge_url, token)
                return child, output_dir, bridge_url, token
        if child.poll() is not None:
            break
    stderr = child.stderr.read() if child.stderr else ""
    child.terminate()
    raise AssertionError("CLI mode did not become ready\nSTDOUT:\n" + "".join(stdout_lines) + "\nSTDERR:\n" + stderr)


def _reader_thread(stream: Any, lines: queue.Queue[str]) -> threading.Thread:
    def read() -> None:
        for line in stream:
            lines.put(line)

    thread = threading.Thread(target=read, daemon=True)
    thread.start()
    return thread


def _stop(child: subprocess.Popen[str]) -> None:
    child.terminate()
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        child.kill()


@pytest.mark.parametrize("level", ["low", "medium", "high", "max"])
@pytest.mark.asyncio
async def test_atlassian_cli_stdio_compression_levels(level: str) -> None:
    _token()
    async with Client(
        cast(
            "Any",
            _client_config(
                "-c",
                level,
                "--server-name",
                "atlassian",
                "--",
                *_backend_args(),
            ),
        )
    ) as client:
        tools = {tool.name for tool in await client.list_tools()}
        assert "atlassian_get_tool_schema" in tools
        assert "atlassian_invoke_tool" in tools
        assert ("atlassian_list_tools" in tools) is (level == "max")


@pytest.mark.asyncio
async def test_atlassian_cli_filters_and_toonify() -> None:
    _token()
    async with Client(
        cast(
            "Any",
            _client_config(
                "-c",
                "medium",
                "--server-name",
                "atlassian",
                "--include-tools",
                "getConfluencePage,updateConfluencePage",
                "--toonify",
                "--",
                *_backend_args(),
            ),
        )
    ) as client:
        schema = await client.call_tool(
            "atlassian_get_tool_schema",
            {"tool_name": "getConfluencePage"},
        )
        assert "getConfluencePage" in schema.content[0].text
        tools = {tool.name for tool in await client.list_tools()}
        assert tools == {"atlassian_get_tool_schema", "atlassian_invoke_tool"}


def test_atlassian_cli_mode_creates_usable_cli() -> None:
    child, output_dir, _bridge, _token_value = _start_proxy_mode()
    try:
        script = Path(output_dir) / "atlassian"
        help_result = subprocess.run([str(script), "--help"], text=True, capture_output=True, check=True)
        assert "atlassian - the atlassian toolset" in help_result.stdout
        assert SAFE_SUBCOMMAND in help_result.stdout.lower()
        call_result = subprocess.run(
            [str(script), SAFE_SUBCOMMAND], text=True, capture_output=True, check=True, timeout=30
        )
        assert call_result.stdout.strip()
    finally:
        _stop(child)


@pytest.mark.parametrize(("flag", "expected"), [("--python-mode", ".py"), ("--typescript-mode", ".ts")])
def test_atlassian_code_modes_generate_clients(flag: str, expected: str) -> None:
    output_dir = tempfile.mkdtemp(prefix="mcp-compressor-code-")
    child, _script_dir, _bridge, _token_value = _start_proxy_mode(flag, "--output-dir", output_dir, script_name=None)
    try:
        assert any(path.suffix == expected for path in Path(output_dir).iterdir())
        # The generated artifacts are intentionally smoke-tested here for real-world
        # schema generation and proxy startup. Deeper generated-client invocation is
        # covered by fixture-backed e2e; Atlassian schemas include optional-before-required
        # patterns that need a dedicated generator hardening PR.
    finally:
        _stop(child)


def test_atlassian_just_bash_mode_starts_bridge() -> None:
    env = os.environ.copy()
    env["MCP_COMPRESSOR_EXIT_AFTER_READY"] = "1"
    result = subprocess.run(
        [
            str(CORE_BIN),
            "--just-bash-mode",
            "--server-name",
            "atlassian",
            "--",
            *_backend_args(),
        ],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=True,
        timeout=60,
    )
    assert "Just Bash ready" in result.stdout


@pytest.mark.asyncio
async def test_atlassian_mcp_config_multi_server_and_streamable_http_port() -> None:
    _token()
    config_path = Path(tempfile.mkdtemp(prefix="mcp-config-")) / "mcp.json"
    config_path.write_text(
        json.dumps({
            "mcpServers": {
                "atl_a": {"command": ATLASSIAN_URL, "args": _backend_args()[1:]},
                "atl_b": {"command": ATLASSIAN_URL, "args": _backend_args()[1:]},
            }
        })
    )
    child = subprocess.Popen(
        [
            str(CORE_BIN),
            "-c",
            "medium",
            "--transport",
            "streamable-http",
            "--port",
            "0",
            "--config",
            str(config_path),
        ],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        stderr_lines: queue.Queue[str] = queue.Queue()
        if child.stderr:
            _reader_thread(child.stderr, stderr_lines)
        deadline = time.time() + 40
        url = ""
        captured_stderr: list[str] = []
        while time.time() < deadline:
            if child.poll() is not None:
                break
            try:
                line = stderr_lines.get(timeout=0.2)
            except queue.Empty:
                continue
            captured_stderr.append(line)
            match = re.search(r"(http://127\.0\.0\.1:\d+/mcp)", line)
            if match:
                url = match.group(1)
                break
        assert url, "".join(captured_stderr)
        async with Client(url) as client:
            tools = {tool.name for tool in await client.list_tools()}
            assert "atl_a_get_tool_schema" in tools
            assert "atl_b_get_tool_schema" in tools
    finally:
        _stop(child)


def test_atlassian_python_native_session() -> None:
    sys.path.insert(0, str(ROOT / "python" / "mcp-compressor-rust"))
    rust_package = cast("Any", importlib.import_module("mcp_compressor_rust"))

    session = rust_package.start_compressed_session(
        rust_package.CompressedSessionConfig(
            compression_level="medium", server_name="atlassian", include_tools=["getConfluencePage"]
        ),
        [
            rust_package.BackendConfig(
                name="atlassian",
                command_or_url=ATLASSIAN_URL,
                args=["-H", f"Authorization=Basic {_token()}", "--auth", "explicit-headers"],
            )
        ],
    )
    try:
        info = session.info()
        assert info["bridge_url"].startswith("http://127.0.0.1:")
        assert {tool["name"] for tool in info["frontend_tools"]} == {
            "atlassian_atlassian_get_tool_schema",
            "atlassian_atlassian_invoke_tool",
        }
    finally:
        session.close()


def test_atlassian_typescript_native_session() -> None:
    script = textwrap.dedent(
        f"""
        import {{ startCompressedSession }} from './dist/rust_core.js';
        const session = await startCompressedSession(
          {{ compressionLevel: 'medium', serverName: 'atlassian', includeTools: ['getConfluencePage'] }},
          [{{ name: 'atlassian', commandOrUrl: '{ATLASSIAN_URL}', args: ['-H', `Authorization=Basic ${{process.env.{TOKEN_ENV}}}`, '--auth', 'explicit-headers'] }}]
        );
        const info = session.info();
        console.log(JSON.stringify({{ bridge: info.bridge_url, tools: info.frontend_tools.map((tool) => tool.name).sort() }}));
        session.close();
        """
    )
    result = subprocess.run(
        ["bun", "--eval", script],
        cwd=ROOT / "typescript",
        env={**os.environ, TOKEN_ENV: _token()},
        text=True,
        capture_output=True,
        check=True,
        timeout=60,
    )
    payload: dict[str, Any] = json.loads(result.stdout)
    assert payload["bridge"].startswith("http://127.0.0.1:")
    assert payload["tools"] == ["atlassian_atlassian_get_tool_schema", "atlassian_atlassian_invoke_tool"]
