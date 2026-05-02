from __future__ import annotations

import json
import subprocess
import time
from pathlib import Path

from fastmcp import Client
from fastmcp.client.transports import StdioTransport, StreamableHttpTransport


def rust_core_command(*args: str) -> list[str]:
    return [
        "cargo",
        "run",
        "-q",
        "-p",
        "mcp-compressor-core",
        "--",
        *args,
    ]


async def test_rust_core_normal_stdio_mode_with_fixture_server() -> None:
    root = Path(__file__).parents[1]
    alpha = root / "crates" / "mcp-compressor-core" / "tests" / "fixtures" / "alpha_server.py"
    command = rust_core_command(
        "--compression",
        "max",
        "--server-name",
        "alpha",
        "--",
        "python3",
        str(alpha),
    )

    async with Client(StdioTransport(command=command[0], args=command[1:])) as client:
        tools = {tool.name for tool in await client.list_tools()}
        assert tools == {
            "alpha_get_tool_schema",
            "alpha_invoke_tool",
            "alpha_list_tools",
        }

        result = await client.call_tool(
            "alpha_invoke_tool",
            {
                "tool_name": "echo",
                "tool_input": {"message": "hello"},
            },
        )
        assert result.content[0].text == "alpha:hello"

        schema = await client.call_tool(
            "alpha_get_tool_schema",
            {"tool_name": "echo"},
        )
        assert "echo" in schema.content[0].text
        assert "message" in schema.content[0].text

        listed = await client.call_tool("alpha_list_tools", {})
        assert "echo" in listed.content[0].text
        assert "add" in listed.content[0].text

        resources = {str(resource.uri) for resource in await client.list_resources()}
        assert "fixture://alpha-resource" in resources
        assert "compressor://alpha/uncompressed-tools" in resources

        alpha_resource = await client.read_resource("fixture://alpha-resource")
        assert alpha_resource[0].text == "alpha resource"

        compressor_resource = await client.read_resource("compressor://alpha/uncompressed-tools")
        assert "echo" in compressor_resource[0].text
        assert "add" in compressor_resource[0].text

        prompts = {prompt.name for prompt in await client.list_prompts()}
        assert "alpha_prompt" in prompts

        prompt = await client.get_prompt("alpha_prompt")
        assert prompt.messages[0].content.text == "alpha prompt"


async def test_rust_core_normal_stdio_mode_with_multi_server_direct_config() -> None:
    root = Path(__file__).parents[1]
    fixture_dir = root / "crates" / "mcp-compressor-core" / "tests" / "fixtures"
    alpha = fixture_dir / "alpha_server.py"
    beta = fixture_dir / "beta_server.py"
    command = rust_core_command(
        "--compression",
        "max",
        "--server-name",
        "suite",
        "--multi-server",
        "alpha=python3",
        str(alpha),
        "--multi-server",
        "beta=python3",
        str(beta),
    )

    async with Client(StdioTransport(command=command[0], args=command[1:])) as client:
        tools = {tool.name for tool in await client.list_tools()}
        assert tools == {
            "suite_alpha_get_tool_schema",
            "suite_alpha_invoke_tool",
            "suite_alpha_list_tools",
            "suite_beta_get_tool_schema",
            "suite_beta_invoke_tool",
            "suite_beta_list_tools",
        }

        alpha_result = await client.call_tool(
            "suite_alpha_invoke_tool",
            {"tool_name": "add", "tool_input": {"a": 3, "b": 7}},
        )
        assert alpha_result.content[0].text == "10"

        beta_result = await client.call_tool(
            "suite_beta_invoke_tool",
            {"tool_name": "multiply", "tool_input": {"a": 4, "b": 5}},
        )
        assert beta_result.content[0].text == "20"

        resources = {str(resource.uri) for resource in await client.list_resources()}
        assert "fixture://alpha-resource" in resources
        assert "fixture://beta-resource" in resources
        assert "compressor://suite_alpha/uncompressed-tools" in resources
        assert "compressor://suite_beta/uncompressed-tools" in resources

        prompts = {prompt.name for prompt in await client.list_prompts()}
        assert "alpha_prompt" in prompts
        assert "beta_prompt" in prompts


async def test_rust_core_normal_streamable_http_mode_with_fixture_server() -> None:
    root = Path(__file__).parents[1]
    alpha = root / "crates" / "mcp-compressor-core" / "tests" / "fixtures" / "alpha_server.py"
    command = rust_core_command(
        "--compression",
        "max",
        "--server-name",
        "alpha",
        "--transport",
        "streamable-http",
        "--port",
        "0",
        "--",
        "python3",
        str(alpha),
    )
    process = subprocess.Popen(  # noqa: S603
        command,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        assert process.stderr is not None
        deadline = time.monotonic() + 30
        url = None
        while time.monotonic() < deadline:
            line = process.stderr.readline()
            if "Streamable HTTP MCP server listening on " in line:
                url = line.rsplit(" ", 1)[1].strip()
                break
            if process.poll() is not None:
                raise AssertionError(f"rust HTTP server exited early with {process.returncode}")
        assert url is not None

        async with Client(StreamableHttpTransport(url)) as client:
            tools = {tool.name for tool in await client.list_tools()}
            assert tools == {
                "alpha_get_tool_schema",
                "alpha_invoke_tool",
                "alpha_list_tools",
            }
            result = await client.call_tool(
                "alpha_invoke_tool",
                {"tool_name": "echo", "tool_input": {"message": "http"}},
            )
            assert result.content[0].text == "alpha:http"
    finally:
        process.terminate()
        process.wait(timeout=10)


async def test_rust_core_normal_stdio_mode_with_remote_streamable_http_backend() -> None:
    root = Path(__file__).parents[1]
    alpha = root / "crates" / "mcp-compressor-core" / "tests" / "fixtures" / "alpha_server.py"
    upstream = rust_core_command(
        "--compression",
        "max",
        "--server-name",
        "upstream",
        "--transport",
        "streamable-http",
        "--port",
        "0",
        "--",
        "python3",
        str(alpha),
    )
    process = subprocess.Popen(  # noqa: S603
        upstream,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        assert process.stderr is not None
        deadline = time.monotonic() + 30
        url = None
        while time.monotonic() < deadline:
            line = process.stderr.readline()
            if "Streamable HTTP MCP server listening on " in line:
                url = line.rsplit(" ", 1)[1].strip()
                break
            if process.poll() is not None:
                raise AssertionError(f"upstream Rust server exited early with {process.returncode}")
        assert url is not None

        command = rust_core_command(
            "--compression",
            "max",
            "--server-name",
            "remote",
            "--",
            url,
            "--auth",
            "explicit-headers",
        )
        async with Client(StdioTransport(command=command[0], args=command[1:])) as client:
            tools = {tool.name for tool in await client.list_tools()}
            assert tools == {
                "remote_get_tool_schema",
                "remote_invoke_tool",
                "remote_list_tools",
            }
            result = await client.call_tool(
                "remote_invoke_tool",
                {
                    "tool_name": "upstream_invoke_tool",
                    "tool_input": {"tool_name": "echo", "tool_input": {"message": "remote"}},
                },
            )
            assert result.content[0].text == "alpha:remote"
    finally:
        process.terminate()
        process.wait(timeout=10)


async def test_rust_core_normal_stdio_mode_with_json_config(tmp_path: Path) -> None:
    root = Path(__file__).parents[1]
    fixture_dir = root / "crates" / "mcp-compressor-core" / "tests" / "fixtures"
    alpha = fixture_dir / "alpha_server.py"
    beta = fixture_dir / "beta_server.py"
    config = tmp_path / "mcp.json"
    config.write_text(
        json.dumps({
            "mcpServers": {
                "alpha": {"command": "python3", "args": [str(alpha)]},
                "beta": {"command": "python3", "args": [str(beta)]},
            }
        }),
        encoding="utf-8",
    )
    command = rust_core_command(
        "--compression",
        "max",
        "--server-name",
        "suite",
        "--config",
        str(config),
    )

    async with Client(StdioTransport(command=command[0], args=command[1:])) as client:
        tools = {tool.name for tool in await client.list_tools()}
        assert "suite_alpha_invoke_tool" in tools
        assert "suite_beta_invoke_tool" in tools

        alpha_result = await client.call_tool(
            "suite_alpha_invoke_tool",
            {"tool_name": "echo", "tool_input": {"message": "json"}},
        )
        assert alpha_result.content[0].text == "alpha:json"

        beta_result = await client.call_tool(
            "suite_beta_invoke_tool",
            {"tool_name": "echo", "tool_input": {"message": "json"}},
        )
        assert beta_result.content[0].text == "beta:json"
