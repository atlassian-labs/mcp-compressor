from __future__ import annotations

import asyncio
import json
import sys
from contextlib import asynccontextmanager
from pathlib import Path

import pytest
from mcp import StdioServerParameters

# The normal workspace retains SDK v1; this suite runs in isolation.
Client = pytest.importorskip("mcp.client.client", reason="Requires the isolated SDK 2.2.0 environment").Client
MCPError = pytest.importorskip("mcp.shared.exceptions").MCPError


def compressor_parameters(binary: Path, level: str = "max", *, toon: bool = False) -> StdioServerParameters:
    backend = Path(__file__).parent / "fixtures" / "protocol_legacy_server.py"
    return StdioServerParameters(
        command=str(binary),
        args=[
            "--compression",
            level,
            "--server-name",
            "alpha",
            *(["--toonify"] if toon else []),
            "--",
            sys.executable,
            str(backend),
        ],
    )


@asynccontextmanager
async def http_compressor(binary: Path):
    parameters = compressor_parameters(binary)
    process = await asyncio.create_subprocess_exec(
        parameters.command,
        "--transport",
        "streamable-http",
        "--port",
        "0",
        *parameters.args,
        stdout=asyncio.subprocess.DEVNULL,
        stderr=asyncio.subprocess.PIPE,
    )
    try:
        assert process.stderr is not None
        async with asyncio.timeout(15):
            while True:
                line = await process.stderr.readline()
                assert line, "compressor exited before opening its HTTP endpoint"
                if b"Streamable HTTP MCP server listening on " in line:
                    url = line.decode().split("listening on ", 1)[1].strip()
                    break
        yield url
    finally:
        if process.returncode is None:
            process.terminate()
        async with asyncio.timeout(10):
            await process.communicate()


@pytest.mark.parametrize("mode", ["auto", "legacy", "2026-07-28"])
async def test_sdk_v2_connects_to_compressor(rust_core_binary: Path, mode: str) -> None:
    options = {} if mode == "auto" else {"mode": mode}
    async with Client(compressor_parameters(rust_core_binary), read_timeout_seconds=10, **options) as client:
        assert client.protocol_version == ("2025-11-25" if mode == "legacy" else "2026-07-28")
        tools = await client.list_tools()
        assert {tool.name for tool in tools.tools} == {"alpha_get_tool_schema", "alpha_invoke_tool", "alpha_list_tools"}
        schema = await client.call_tool("alpha_get_tool_schema", {"tool_name": "echo"})
        assert "message" in schema.content[0].text
        result = await client.call_tool("alpha_invoke_tool", {"tool_name": "echo", "tool_input": {"message": "auto"}})
        assert result.content[0].text == "echo:auto"


@pytest.mark.parametrize("mode", ["auto", "legacy"])
@pytest.mark.parametrize("level", ["low", "medium", "high", "max"])
async def test_sdk_v2_preserves_compression_and_errors(rust_core_binary: Path, mode: str, level: str) -> None:
    options = {} if mode == "auto" else {"mode": "legacy"}
    async with Client(compressor_parameters(rust_core_binary, level), **options) as client:
        tools = await client.list_tools()
        assert "alpha_invoke_tool" in {tool.name for tool in tools.tools}
        failed = await client.call_tool("alpha_invoke_tool", {"tool_name": "fail"})
        assert failed.is_error
        assert failed.content[0].text == "fixture tool failure"
        with pytest.raises(MCPError, match="fixture protocol failure"):
            await client.call_tool("alpha_invoke_tool", {"tool_name": "rpc_fail"})
        with pytest.raises(MCPError):
            await client.call_tool("alpha_invoke_tool", {})
        metadata = await client.call_tool(
            "alpha_invoke_tool", {"tool_name": "metadata"}, meta={"test/trace": "retained"}
        )
        forwarded = json.loads(metadata.content[0].text)
        assert forwarded["test/trace"] == "retained"
        assert set(forwarded) <= {"test/trace", "progressToken"}
        result = await client.call_tool("alpha_invoke_tool", {"tool_name": "records"})
        assert json.loads(result.content[0].text)["items"][1]["name"] == "two"


@pytest.mark.parametrize("mode", ["auto", "legacy"])
async def test_sdk_v2_preserves_toon_output(rust_core_binary: Path, mode: str) -> None:
    options = {} if mode == "auto" else {"mode": "legacy"}
    async with Client(compressor_parameters(rust_core_binary, toon=True), **options) as client:
        result = await client.call_tool("alpha_invoke_tool", {"tool_name": "records"})
        assert result.content[0].text == "items[2]{id,name}:\n  1,one\n  2,two"


@pytest.mark.parametrize("mode", ["auto", "legacy", "2026-07-28"])
async def test_sdk_v2_http_frontend(rust_core_binary: Path, mode: str) -> None:
    options = {} if mode == "auto" else {"mode": mode}
    async with http_compressor(rust_core_binary) as url, Client(url, read_timeout_seconds=10, **options) as client:
        assert client.protocol_version == ("2025-11-25" if mode == "legacy" else "2026-07-28")
        await client.list_tools()
        result = await client.call_tool("alpha_invoke_tool", {"tool_name": "echo", "tool_input": {"message": "http"}})
        assert result.content[0].text == "echo:http"


@pytest.mark.parametrize("mode", ["auto", "legacy"])
async def test_sdk_v2_and_strict_legacy_backends_can_be_mixed(
    rust_core_binary: Path, tmp_path: Path, mode: str
) -> None:
    fixtures = Path(__file__).parent / "fixtures"
    config = tmp_path / "mixed.json"
    config.write_text(
        json.dumps({
            "mcpServers": {
                "old": {"command": sys.executable, "args": [str(fixtures / "protocol_legacy_server.py")]},
                "new": {"command": sys.executable, "args": [str(fixtures / "protocol_sdk_v2_server.py")]},
            }
        }),
        encoding="utf-8",
    )
    parameters = StdioServerParameters(
        command=str(rust_core_binary), args=["--compression", "max", "--config", str(config)]
    )
    options = {} if mode == "auto" else {"mode": "legacy"}
    async with Client(parameters, read_timeout_seconds=10, **options) as client:
        tools = await client.list_tools()
        assert {"old_invoke_tool", "new_invoke_tool"} <= {tool.name for tool in tools.tools}
        for name, expected in [("old", "echo:mixed"), ("new", "sdk2:mixed")]:
            schema = await client.call_tool(f"{name}_get_tool_schema", {"tool_name": "echo"})
            assert "message" in schema.content[0].text
            result = await client.call_tool(
                f"{name}_invoke_tool", {"tool_name": "echo", "tool_input": {"message": "mixed"}}
            )
            assert result.content[0].text == expected


@asynccontextmanager
async def raw_compressor(binary: Path, *, toon: bool = False):
    parameters = compressor_parameters(binary, toon=toon)
    process = await asyncio.create_subprocess_exec(
        parameters.command,
        *parameters.args,
        stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.DEVNULL,
    )
    try:
        yield process
    finally:
        if process.stdin is not None:
            process.stdin.close()
        try:
            async with asyncio.timeout(10):
                await process.wait()
        except TimeoutError:
            process.kill()
            await process.wait()
            raise


async def exchange(process: asyncio.subprocess.Process, request: dict) -> dict:
    assert process.stdin is not None and process.stdout is not None
    process.stdin.write(json.dumps({"jsonrpc": "2.0", **request}).encode() + b"\n")
    await process.stdin.drain()
    async with asyncio.timeout(10):
        response = await process.stdout.readline()
    assert response, "compressor closed the protocol stream"
    decoded = json.loads(response)
    assert decoded["id"] == request["id"]
    return decoded


async def test_modern_wire_rejects_invalid_metadata_without_closing(rust_core_binary: Path) -> None:
    async with raw_compressor(rust_core_binary) as process:
        meta = {
            "io.modelcontextprotocol/protocolVersion": "2099-01-01",
            "io.modelcontextprotocol/clientCapabilities": {},
        }
        unsupported = await exchange(process, {"id": "version", "method": "server/discover", "params": {"_meta": meta}})
        assert unsupported["error"]["code"] == -32022
        assert "2026-07-28" in unsupported["error"]["data"]["supported"]
        meta["io.modelcontextprotocol/protocolVersion"] = "2026-07-28"
        discovered = await exchange(
            process, {"id": "discovery", "method": "server/discover", "params": {"_meta": meta}}
        )
        assert discovered["result"]["resultType"] == "complete"
        assert "2026-07-28" in discovered["result"]["supportedVersions"]
        missing = await exchange(process, {"id": 3, "method": "tools/list"})
        assert missing["error"]["code"] == -32602
        listed = await exchange(process, {"id": 4, "method": "tools/list", "params": {"_meta": meta}})
        assert listed["result"]["resultType"] == "complete"
        assert listed["result"]["ttlMs"] == 0
        assert listed["result"]["cacheScope"] == "private"
        assert len(listed["result"]["tools"]) == 3
        unknown = await exchange(process, {"id": 5, "method": "fixture/unknown", "params": {"_meta": meta}})
        assert unknown["error"]["code"] == -32601


@pytest.mark.parametrize("mode", ["auto", "legacy"])
async def test_sdk_v2_resources_and_prompts_preserve_baseline_content(rust_core_binary: Path, mode: str) -> None:
    options = {} if mode == "auto" else {"mode": "legacy"}
    async with Client(compressor_parameters(rust_core_binary), read_timeout_seconds=10, **options) as client:
        resources = await client.list_resources()
        assert "fixture://resource" in {str(resource.uri) for resource in resources.resources}
        content = await client.read_resource("fixture://resource")
        assert content.contents[0].text == "fixture resource"
        prompts = await client.list_prompts()
        assert prompts.prompts[0].name == "fixture_prompt"
        prompt = await client.get_prompt("fixture_prompt")
        assert prompt.messages[0].content.text == "fixture prompt"


async def negotiate_wire(process: asyncio.subprocess.Process, mode: str) -> tuple[dict, dict]:
    if mode == "modern":
        meta = {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
        }
        response = await exchange(process, {"id": "start", "method": "server/discover", "params": {"_meta": meta}})
        return meta, response["result"]["capabilities"]
    response = await exchange(
        process,
        {
            "id": "start",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "clientInfo": {"name": "wire-fixture", "version": "1"},
                "capabilities": {},
            },
        },
    )
    assert process.stdin is not None
    process.stdin.write(b'{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
    await process.stdin.drain()
    return {}, response["result"]["capabilities"]


@pytest.mark.parametrize("mode", ["modern", "legacy"])
async def test_wire_catalogs_are_static_and_version_specific(rust_core_binary: Path, mode: str) -> None:
    async with raw_compressor(rust_core_binary) as process:
        meta, capabilities = await negotiate_wire(process, mode)
        for category in ("tools", "resources", "prompts"):
            assert capabilities[category].get("listChanged") is not True
        assert capabilities["resources"].get("subscribe") is not True
        requests = [
            ("tools/list", {}),
            ("resources/list", {}),
            ("prompts/list", {}),
            ("resources/read", {"uri": "fixture://resource"}),
            ("prompts/get", {"name": "fixture_prompt"}),
            ("tools/call", {"name": "alpha_get_tool_schema", "arguments": {"tool_name": "echo"}}),
        ]
        for method, params in requests:
            response = await exchange(process, {"id": method, "method": method, "params": {**params, "_meta": meta}})
            result = response["result"]
            if mode == "modern":
                assert result["resultType"] == "complete"
                if method.endswith("/list") or method == "resources/read":
                    assert result["ttlMs"] == 0
                    assert result["cacheScope"] == "private"
            else:
                assert not {"resultType", "ttlMs", "cacheScope"}.intersection(result)
        method = "subscriptions/listen" if mode == "modern" else "resources/subscribe"
        params = {"notifications": {"toolsListChanged": True}} if mode == "modern" else {"uri": "fixture://resource"}
        rejected = await exchange(process, {"id": "subscribe", "method": method, "params": {**params, "_meta": meta}})
        assert rejected["error"]["code"] == -32601
        after = await exchange(process, {"id": "after", "method": "tools/list", "params": {"_meta": meta}})
        assert len(after["result"]["tools"]) == 3


@pytest.mark.parametrize("mode", ["modern", "legacy"])
@pytest.mark.parametrize("toon", [False, True])
async def test_wire_preserves_tool_result_envelope(rust_core_binary: Path, mode: str, toon: bool) -> None:
    async with raw_compressor(rust_core_binary, toon=toon) as process:
        meta, _ = await negotiate_wire(process, mode)
        response = await exchange(
            process,
            {
                "id": "envelope",
                "method": "tools/call",
                "params": {
                    "name": "alpha_invoke_tool",
                    "arguments": {"tool_name": "envelope"},
                    "_meta": meta,
                },
            },
        )
        expected = {
            "content": [
                {
                    "type": "text",
                    "text": "value: 42" if toon else '{"value":42}',
                    "annotations": {"audience": ["user"]},
                    "_meta": {"fixture/block": True},
                }
            ],
            "structuredContent": {"value": 42},
            "isError": False,
            "_meta": {"fixture/result": True},
        }
        if mode == "modern":
            expected["resultType"] = "complete"
        assert response["result"] == expected
