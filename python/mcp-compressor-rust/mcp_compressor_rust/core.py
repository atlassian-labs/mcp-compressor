"""Thin Python wrappers around the Rust core native extension."""

from __future__ import annotations

import importlib
import json
from dataclasses import dataclass
from typing import Any

_mcp_compressor_core = importlib.import_module("mcp_compressor_rust._mcp_compressor_core")


@dataclass(frozen=True)
class RustTool:
    """JSON-serializable tool DTO accepted by the Rust core extension."""

    name: str
    description: str | None
    input_schema: dict[str, Any]

    def to_json_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "description": self.description,
            "input_schema": self.input_schema,
        }


def _json_dumps(value: Any) -> str:
    return json.dumps(value, separators=(",", ":"))


def _tool_payload(tools: list[RustTool]) -> str:
    return _json_dumps([tool.to_json_dict() for tool in tools])


def compress_tool_listing(level: str, tools: list[RustTool]) -> str:
    """Format a Rust-core compressed tool listing."""
    return str(_mcp_compressor_core.compress_tool_listing_json(level, _tool_payload(tools)))


def format_tool_schema_response(tool: RustTool) -> str:
    """Format a Rust-core schema response for one tool."""
    return str(_mcp_compressor_core.format_tool_schema_response_json(_json_dumps(tool.to_json_dict())))


def parse_tool_argv(tool: RustTool, argv: list[str]) -> dict[str, Any]:
    """Parse generated CLI argv for one tool through the Rust core parser."""
    parsed = _mcp_compressor_core.parse_tool_argv_json(
        _json_dumps(tool.to_json_dict()),
        _json_dumps(argv),
    )
    value = json.loads(parsed)
    if not isinstance(value, dict):
        msg = "Rust core parse_tool_argv_json returned non-object JSON"
        raise TypeError(msg)
    return value


def parse_mcp_config(config_json: str) -> list[dict[str, Any]]:
    """Parse MCP config JSON through the Rust core topology parser."""
    value = json.loads(_mcp_compressor_core.parse_mcp_config_json(config_json))
    if not isinstance(value, list):
        msg = "Rust core parse_mcp_config_json returned non-list JSON"
        raise TypeError(msg)
    return value


def list_oauth_credentials() -> list[dict[str, Any]]:
    """List remembered Rust OAuth credential stores."""
    value = json.loads(_mcp_compressor_core.list_oauth_credentials_json())
    if not isinstance(value, list):
        msg = "Rust core list_oauth_credentials_json returned non-list JSON"
        raise TypeError(msg)
    return value


def clear_oauth_credentials(target: str | None = None) -> list[str]:
    """Clear remembered Rust OAuth credential stores."""
    value = json.loads(_mcp_compressor_core.clear_oauth_credentials_json(target))
    if not isinstance(value, list):
        msg = "Rust core clear_oauth_credentials_json returned non-list JSON"
        raise TypeError(msg)
    return value
