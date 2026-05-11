from __future__ import annotations

import os
import subprocess
import sys
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from mcp_compressor import CompressorClient

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
