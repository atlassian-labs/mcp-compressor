from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest
from fastmcp import Client
from mcp.types import TextContent

from mcp_compressor.main import _server
from mcp_compressor.types import CompressionLevel


@pytest.fixture
def alpha_server_path() -> Path:
    return Path(__file__).parent / "e2e_server_alpha.py"


@pytest.fixture
def beta_server_path() -> Path:
    return Path(__file__).parent / "e2e_server_beta.py"


@pytest.fixture
def single_server_config_json(alpha_server_path: Path) -> str:
    return json.dumps({"mcpServers": {"alpha": {"command": sys.executable, "args": [str(alpha_server_path)]}}})


@pytest.fixture
def multi_server_config_json(alpha_server_path: Path, beta_server_path: Path) -> str:
    return json.dumps({
        "mcpServers": {
            "alpha": {"command": sys.executable, "args": [str(alpha_server_path)]},
            "beta": {"command": sys.executable, "args": [str(beta_server_path)]},
        }
    })


async def test_single_server_command_proxy_happy_paths(alpha_server_path: Path) -> None:
    async with (
        _server(
            command_or_url_list=[sys.executable, str(alpha_server_path)],
            cwd=None,
            env_list=None,
            header_list=None,
            timeout=10.0,
            compression_level=CompressionLevel.MAX,
            server_name="alpha",
        ) as mcp,
        Client(mcp) as client,
    ):
        tool_names = {tool.name for tool in await client.list_tools()}
        assert tool_names == {"alpha_get_tool_schema", "alpha_invoke_tool", "alpha_list_tools"}

        schema = await client.call_tool("alpha_get_tool_schema", {"tool_name": "alpha_echo"})
        assert "alpha_echo" in schema.content[0].text

        listed = await client.call_tool("alpha_list_tools", {})
        assert "alpha_add" in listed.content[0].text

        result = await client.call_tool(
            "alpha_invoke_tool", {"tool_name": "alpha_echo", "tool_input": {"message": "hello"}}
        )
        assert isinstance(result.content[0], TextContent)
        assert result.content[0].text == "alpha:hello"

        resources = {str(resource.uri) for resource in await client.list_resources()}
        assert "e2e://alpha-resource" in resources
        assert "compressor://alpha/uncompressed-tools" in resources
        assert (await client.read_resource("e2e://alpha-resource"))[0].text == "alpha resource"
        assert "alpha_echo" in (await client.read_resource("compressor://alpha/uncompressed-tools"))[0].text

        prompts = {prompt.name for prompt in await client.list_prompts()}
        assert "alpha_prompt" in prompts


async def test_single_server_mcp_config_proxy_supports_filters_and_toonify(single_server_config_json: str) -> None:
    async with (
        _server(
            command_or_url_list=[single_server_config_json],
            cwd=None,
            env_list=None,
            header_list=None,
            timeout=10.0,
            compression_level=CompressionLevel.LOW,
            server_name=None,
            toonify=True,
            include_tools=["alpha_object", "alpha_echo"],
            exclude_tools=["alpha_echo"],
        ) as mcp,
        Client(mcp) as client,
    ):
        tool_names = {tool.name for tool in await client.list_tools()}
        assert tool_names == {"get_tool_schema", "invoke_tool"}

        object_result = await client.call_tool("invoke_tool", {"tool_name": "alpha_object", "tool_input": {}})
        assert object_result.content[0].text == "server: alpha\nvalues[2]: 1,2"

        with pytest.raises(Exception, match="Available tools: alpha_object"):
            await client.call_tool("get_tool_schema", {"tool_name": "alpha_echo"})


async def test_multi_server_config_proxy_happy_paths(multi_server_config_json: str) -> None:
    async with (
        _server(
            command_or_url_list=[multi_server_config_json],
            cwd=None,
            env_list=None,
            header_list=None,
            timeout=10.0,
            compression_level=CompressionLevel.MAX,
            server_name="suite",
        ) as mcp,
        Client(mcp) as client,
    ):
        tool_names = {tool.name for tool in await client.list_tools()}
        assert {
            "suite_alpha_get_tool_schema",
            "suite_alpha_invoke_tool",
            "suite_alpha_list_tools",
            "suite_beta_get_tool_schema",
            "suite_beta_invoke_tool",
            "suite_beta_list_tools",
        }.issubset(tool_names)

        alpha_tools = await client.call_tool("suite_alpha_list_tools", {})
        beta_tools = await client.call_tool("suite_beta_list_tools", {})
        assert "alpha_echo" in alpha_tools.content[0].text
        assert "beta_echo" in beta_tools.content[0].text

        alpha_result = await client.call_tool(
            "suite_alpha_invoke_tool", {"tool_name": "alpha_add", "tool_input": {"a": 2, "b": 5}}
        )
        beta_result = await client.call_tool(
            "suite_beta_invoke_tool", {"tool_name": "beta_multiply", "tool_input": {"a": 3, "b": 4}}
        )
        assert alpha_result.content[0].text == "7"
        assert beta_result.content[0].text == "12"

        resources = {str(resource.uri) for resource in await client.list_resources()}
        assert "compressor://suite_alpha/uncompressed-tools" in resources
        assert "compressor://suite_beta/uncompressed-tools" in resources
        assert "alpha_echo" in (await client.read_resource("compressor://suite_alpha/uncompressed-tools"))[0].text
        assert "beta_echo" in (await client.read_resource("compressor://suite_beta/uncompressed-tools"))[0].text


async def test_single_server_cli_mode_generated_script_prints_help_for_invalid_args(
    single_server_config_json: str, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    import mcp_compressor.cli_script as cli_script_module

    monkeypatch.setattr(cli_script_module, "find_script_dir", lambda: (tmp_path, True))
    async with _server(
        command_or_url_list=[single_server_config_json],
        cwd=None,
        env_list=None,
        header_list=None,
        timeout=10.0,
        compression_level=CompressionLevel.LOW,
        server_name="alpha_cli",
        toonify=True,
        cli_mode=True,
        cli_port=0,
    ):
        script = tmp_path / "alpha_cli"
        assert script.exists(), f"Expected generated script at {script}"

        # Use asyncio subprocess so the event loop stays alive for the bridge
        import asyncio
        import re

        script_text = script.read_text()
        bridge_urls = re.findall(r"http://127\.0\.0\.1:\d+", script_text)
        assert bridge_urls, "Expected at least one bridge URL in generated script"

        # Invalid positional arg (should be --message hello)
        proc = await asyncio.create_subprocess_exec(
            sys.executable,
            str(script),
            "alpha-echo",
            "hello",
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=10)
        output = (stdout or b"").decode() + (stderr or b"").decode()
        assert proc.returncode != 0
        assert "error:" in output
        assert "alpha_cli alpha-echo" in output
        assert "--message" in output


async def test_single_server_cli_mode_exposes_help_tool(single_server_config_json: str) -> None:
    async with (
        _server(
            command_or_url_list=[single_server_config_json],
            cwd=None,
            env_list=None,
            header_list=None,
            timeout=10.0,
            compression_level=CompressionLevel.LOW,
            server_name="alpha_cli",
            toonify=True,
            cli_mode=True,
            cli_port=0,
        ) as mcp,
        Client(mcp) as client,
    ):
        tool_names = {tool.name for tool in await client.list_tools()}
        assert tool_names == {"alpha_cli_help"}
        help_result = await client.call_tool("alpha_cli_help", {})
        assert "Functionality associated with the alpha_cli toolset" in help_result.content[0].text


async def test_multi_server_config_cli_mode_exposes_prefixed_help_tools(multi_server_config_json: str) -> None:
    async with (
        _server(
            command_or_url_list=[multi_server_config_json],
            cwd=None,
            env_list=None,
            header_list=None,
            timeout=10.0,
            compression_level=CompressionLevel.LOW,
            server_name=None,
            toonify=True,
            cli_mode=True,
            cli_port=0,
        ) as mcp,
        Client(mcp) as client,
    ):
        tool_names = {tool.name for tool in await client.list_tools()}
        assert {"alpha_help", "beta_help"}.issubset(tool_names)

        alpha_help = await client.call_tool("alpha_help", {})
        beta_help = await client.call_tool("beta_help", {})
        assert "Functionality associated with the alpha toolset" in alpha_help.content[0].text
        assert "Functionality associated with the beta toolset" in beta_help.content[0].text
