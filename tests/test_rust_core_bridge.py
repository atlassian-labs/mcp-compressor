from __future__ import annotations

import json
import sys
from types import ModuleType

import pytest

from mcp_compressor import rust_core
from mcp_compressor.types import CompressionLevel


class FakeRustCore(ModuleType):
    def __init__(self) -> None:
        super().__init__("_mcp_compressor_core")
        self.calls: list[tuple[str, tuple[object, ...]]] = []

    def compress_tool_listing_json(self, level: str, tools_json: str) -> str:
        self.calls.append(("compress", (level, tools_json)))
        tools = json.loads(tools_json)
        return f"{level}:{tools[0]['name']}"

    def format_tool_schema_response_json(self, tool_json: str) -> str:
        self.calls.append(("schema", (tool_json,)))
        return str(json.loads(tool_json)["name"])

    def parse_tool_argv_json(self, tool_json: str, argv_json: str) -> str:
        self.calls.append(("argv", (tool_json, argv_json)))
        argv = json.loads(argv_json)
        return json.dumps({"message": argv[1]})

    def parse_mcp_config_json(self, config_json: str) -> str:
        self.calls.append(("config", (config_json,)))
        return json.dumps([{"name": "alpha", "command": "python", "args": [], "env": [], "cli_prefix": "alpha"}])

    def list_oauth_credentials_json(self) -> str:
        self.calls.append(("list_oauth", ()))
        return json.dumps([
            {"backend_name": "alpha", "backend_uri": "https://example.test/mcp", "store_dir": "/example/store"}
        ])

    def clear_oauth_credentials_json(self, target: str | None) -> str:
        self.calls.append(("clear_oauth", (target,)))
        return json.dumps(["/example/store"])


@pytest.fixture
def fake_extension(monkeypatch: pytest.MonkeyPatch) -> FakeRustCore:
    fake = FakeRustCore()
    monkeypatch.setitem(sys.modules, "_mcp_compressor_core", fake)
    return fake


@pytest.fixture
def sample_tool() -> rust_core.RustTool:
    return rust_core.RustTool(
        name="echo",
        description="Echo a value.",
        input_schema={
            "type": "object",
            "properties": {"message": {"type": "string"}},
            "required": ["message"],
        },
    )


def test_compress_tool_listing_uses_extension(fake_extension: FakeRustCore, sample_tool: rust_core.RustTool) -> None:
    assert rust_core.compress_tool_listing(CompressionLevel.HIGH, [sample_tool]) == "high:echo"
    name, (level, tools_json) = fake_extension.calls[-1]
    assert name == "compress"
    assert level == "high"
    assert json.loads(str(tools_json))[0]["name"] == "echo"


def test_format_tool_schema_response_uses_extension(
    fake_extension: FakeRustCore, sample_tool: rust_core.RustTool
) -> None:
    _ = fake_extension
    assert rust_core.format_tool_schema_response(sample_tool) == "echo"


def test_parse_tool_argv_decodes_json_result(fake_extension: FakeRustCore, sample_tool: rust_core.RustTool) -> None:
    _ = fake_extension
    assert rust_core.parse_tool_argv(sample_tool, ["--message", "hello"]) == {"message": "hello"}


def test_parse_mcp_config_decodes_json_result(fake_extension: FakeRustCore) -> None:
    _ = fake_extension
    parsed = rust_core.parse_mcp_config('{"mcpServers":{"alpha":{"command":"python"}}}')
    assert parsed[0]["name"] == "alpha"


def test_oauth_helpers_decode_json_results(fake_extension: FakeRustCore) -> None:
    _ = fake_extension
    assert rust_core.list_oauth_credentials()[0]["backend_name"] == "alpha"
    assert rust_core.clear_oauth_credentials("alpha") == ["/example/store"]


def test_missing_extension_raises_helpful_error(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delitem(sys.modules, "_mcp_compressor_core", raising=False)
    with pytest.raises(rust_core.RustCoreUnavailableError, match="Rust core extension is not installed"):
        rust_core.compress_tool_listing("high", [])
