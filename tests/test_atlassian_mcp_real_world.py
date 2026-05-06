from __future__ import annotations

# These tests intentionally spawn trusted local binaries and call the fixed Atlassian MCP HTTPS endpoint.
# ruff: noqa: S105,S603,S607,S310
import json
import os
import re
import subprocess
import sys
import tempfile
import textwrap
import time
from pathlib import Path
from typing import Any
from urllib import request

import pytest
from fastmcp import Client

ROOT = Path(__file__).parents[1]
ATLASSIAN_URL = "https://mcp.atlassian.com/v1/mcp"
TOKEN_ENV = "ATLASSIAN_MCP_BASIC_TOKEN"
SAFE_TOOL = "getAccessibleAtlassianResources"
SAFE_SUBCOMMAND = "get-accessible-atlassian-resources"
CORE_BIN = ROOT / "target" / "debug" / "mcp-compressor-core"


def _token() -> str:
    token = os.environ.get(TOKEN_ENV)
    if not token:
        pytest.skip(f"{TOKEN_ENV} is not set")
    return token


@pytest.fixture(scope="session", autouse=True)
def _build_core() -> None:
    subprocess.run(["cargo", "build", "-p", "mcp-compressor-core"], cwd=ROOT, check=True)


def _backend_args() -> list[str]:
    return [ATLASSIAN_URL, "-H", f"Authorization=Basic {_token()}", "--auth", "explicit-headers"]


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


def _start_cli_mode(*extra_args: str) -> tuple[subprocess.Popen[str], str, str, str]:
    env = os.environ.copy()
    output_dir = tempfile.mkdtemp(prefix="mcp-compressor-atlassian-")
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
                script = Path(output_dir) / "atlassian"
                token = script.read_text().split("TOKEN=", 1)[1].split("\n", 1)[0].strip("'\"")
                _wait_http(bridge_url, token)
                return child, output_dir, bridge_url, token
        if child.poll() is not None:
            break
    stderr = child.stderr.read() if child.stderr else ""
    child.terminate()
    raise AssertionError("CLI mode did not become ready\nSTDOUT:\n" + "".join(stdout_lines) + "\nSTDERR:\n" + stderr)


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
    async with Client([
        str(CORE_BIN),
        "-c",
        level,
        "--server-name",
        "atlassian",
        "--",
        *_backend_args(),
    ]) as client:
        tools = {tool.name for tool in await client.list_tools()}
        assert "atlassian_get_tool_schema" in tools
        assert "atlassian_invoke_tool" in tools
        assert ("atlassian_list_tools" in tools) is (level == "max")


@pytest.mark.asyncio
async def test_atlassian_cli_filters_and_toonify() -> None:
    _token()
    async with Client([
        str(CORE_BIN),
        "-c",
        "medium",
        "--server-name",
        "atlassian",
        "--include-tools",
        "getConfluencePage,updateConfluencePage",
        "--toonify",
        "--",
        *_backend_args(),
    ]) as client:
        schema = await client.call_tool(
            "atlassian_get_tool_schema",
            {"tool_name": "getConfluencePage"},
        )
        assert "getConfluencePage" in schema.content[0].text
        tools = {tool.name for tool in await client.list_tools()}
        assert tools == {"atlassian_get_tool_schema", "atlassian_invoke_tool"}


def test_atlassian_cli_mode_creates_usable_cli() -> None:
    child, output_dir, _bridge, _token_value = _start_cli_mode()
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
    child, _script_dir, _bridge, _token_value = _start_cli_mode(flag, "--output-dir", output_dir)
    try:
        assert any(path.suffix == expected for path in Path(output_dir).iterdir())
        if flag == "--python-mode":
            result = subprocess.run(
                [
                    sys.executable,
                    "-c",
                    f"import sys; sys.path.insert(0, {output_dir!r}); import atlassian; print(atlassian.{SAFE_TOOL}())",
                ],
                text=True,
                capture_output=True,
                check=True,
                timeout=30,
            )
        else:
            result = subprocess.run(
                [
                    "bun",
                    "--eval",
                    f"import {{ {SAFE_TOOL} }} from {json.dumps(str(Path(output_dir) / 'atlassian.ts'))}; console.log(await {SAFE_TOOL}());",
                ],
                text=True,
                capture_output=True,
                check=True,
                timeout=30,
            )
        assert result.stdout.strip()
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
        stderr = ""
        deadline = time.time() + 40
        url = ""
        while time.time() < deadline:
            line = child.stderr.readline() if child.stderr else ""
            stderr += line
            match = re.search(r"(http://127\\.0\\.0\\.1:\\d+/mcp)", line)
            if match:
                url = match.group(1)
                break
            if child.poll() is not None:
                break
        assert url, stderr
        async with Client(url) as client:
            tools = {tool.name for tool in await client.list_tools()}
            assert "atl_a_get_tool_schema" in tools
            assert "atl_b_get_tool_schema" in tools
    finally:
        _stop(child)


def test_atlassian_python_native_session() -> None:
    sys.path.insert(0, str(ROOT / "python" / "mcp-compressor-rust"))
    from mcp_compressor_rust import BackendConfig, CompressedSessionConfig, start_compressed_session

    session = start_compressed_session(
        CompressedSessionConfig(
            compression_level="medium", server_name="atlassian", include_tools=["getConfluencePage"]
        ),
        [
            BackendConfig(
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
            "atlassian_get_tool_schema",
            "atlassian_invoke_tool",
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
    assert payload["tools"] == ["atlassian_get_tool_schema", "atlassian_invoke_tool"]
