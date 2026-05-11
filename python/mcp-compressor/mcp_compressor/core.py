"""Thin Python wrappers around the Rust core native extension."""

from __future__ import annotations

import importlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

_native = importlib.import_module("mcp_compressor._native")


@dataclass(frozen=True)
class BackendConfig:
    """Runtime backend configuration accepted by the Rust core extension."""

    name: str
    command_or_url: str
    args: list[str] | None = None

    def to_json_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "command_or_url": self.command_or_url,
            "args": self.args or [],
        }


@dataclass(frozen=True)
class CompressedSessionConfig:
    """Runtime session configuration accepted by the Rust core extension."""

    compression_level: str = "max"
    server_name: str | None = None
    include_tools: list[str] | None = None
    exclude_tools: list[str] | None = None
    toonify: bool = False
    transform_mode: str | None = None

    def to_json_dict(self) -> dict[str, Any]:
        return {
            "compression_level": self.compression_level,
            "server_name": self.server_name,
            "include_tools": self.include_tools or [],
            "exclude_tools": self.exclude_tools or [],
            "toonify": self.toonify,
            "transform_mode": self.transform_mode,
        }


class CompressedSession:
    """Python wrapper around a Rust-backed compressed session handle."""

    def __init__(self, native_session: Any) -> None:
        self._native_session = native_session

    def info(self) -> dict[str, Any]:
        value = json.loads(self._native_session.info_json())
        if not isinstance(value, dict):
            msg = "Rust core session info_json returned non-object JSON"
            raise TypeError(msg)
        return value

    def close(self) -> None:
        self._native_session.close()


@dataclass(frozen=True)
class ToolSpec:
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


def _tool_payload(tools: list[ToolSpec]) -> str:
    return _json_dumps([tool.to_json_dict() for tool in tools])


def compress_tool_listing(level: str, tools: list[ToolSpec]) -> str:
    """Format a Rust-core compressed tool listing."""
    return str(_native.compress_tool_listing_json(level, _tool_payload(tools)))


def format_tool_schema_response(tool: ToolSpec) -> str:
    """Format a Rust-core schema response for one tool."""
    return str(_native.format_tool_schema_response_json(_json_dumps(tool.to_json_dict())))


def parse_tool_argv(tool: ToolSpec, argv: list[str]) -> dict[str, Any]:
    """Parse generated CLI argv for one tool through the Rust core parser."""
    parsed = _native.parse_tool_argv_json(
        _json_dumps(tool.to_json_dict()),
        _json_dumps(argv),
    )
    value = json.loads(parsed)
    if not isinstance(value, dict):
        msg = "Rust core parse_tool_argv_json returned non-object JSON"
        raise TypeError(msg)
    return value


def start_compressed_session(
    config: CompressedSessionConfig,
    backends: list[BackendConfig],
) -> CompressedSession:
    """Start a Rust-backed compressed session and local proxy."""
    native_session = _native.start_compressed_session_json(
        _json_dumps(config.to_json_dict()),
        _json_dumps([backend.to_json_dict() for backend in backends]),
    )
    return CompressedSession(native_session)


def start_compressed_session_with_auth_providers(
    config: CompressedSessionConfig,
    backends: list[dict[str, Any]],
    providers: list[Any],
) -> CompressedSession:
    """Start a Rust-backed compressed session with per-request auth providers."""
    native_session = _native.start_compressed_session_with_provider_backends_json(
        _json_dumps(config.to_json_dict()),
        _json_dumps(backends),
        providers,
    )
    return CompressedSession(native_session)


def start_compressed_session_from_mcp_config(
    config: CompressedSessionConfig,
    mcp_config_json: str,
) -> CompressedSession:
    """Start a Rust-backed compressed session from MCP config JSON."""
    native_session = _native.start_compressed_session_from_mcp_config_json(
        _json_dumps(config.to_json_dict()),
        mcp_config_json,
    )
    return CompressedSession(native_session)


def generate_client_artifacts(
    kind: str,
    *,
    cli_name: str,
    bridge_url: str,
    token: str,
    tools: list[dict[str, Any]],
    output_dir: str | Path,
    session_pid: int = 0,
) -> list[Path]:
    config = {
        "cli_name": cli_name,
        "bridge_url": bridge_url,
        "token": token,
        "tools": tools,
        "output_dir": str(output_dir),
        "session_pid": session_pid,
    }
    raw = json.loads(_native.generate_client_artifacts_json(kind, _json_dumps(config)))
    if not isinstance(raw, list):
        msg = "Rust core generate_client_artifacts_json returned non-list JSON"
        raise TypeError(msg)
    return [Path(str(path)) for path in raw]


def normalize_sdk_servers(servers: dict[str, Any]) -> list[BackendConfig]:
    raw = json.loads(_native.normalize_servers_json(_json_dumps(servers)))
    if not isinstance(raw, list):
        msg = "Rust core normalize_servers_json returned non-list JSON"
        raise TypeError(msg)
    return [
        BackendConfig(
            name=str(item["name"]), command_or_url=str(item["command_or_url"]), args=list(item.get("args", []))
        )
        for item in raw
    ]


def parse_mcp_config(config_json: str) -> list[dict[str, Any]]:
    """Parse MCP config JSON through the Rust core topology parser."""
    value = json.loads(_native.parse_mcp_config_json(config_json))
    if not isinstance(value, list):
        msg = "Rust core parse_mcp_config_json returned non-list JSON"
        raise TypeError(msg)
    return value


def list_oauth_credentials() -> list[dict[str, Any]]:
    """List remembered Rust OAuth credential stores."""
    value = json.loads(_native.list_oauth_credentials_json())
    if not isinstance(value, list):
        msg = "Rust core list_oauth_credentials_json returned non-list JSON"
        raise TypeError(msg)
    return value


def clear_oauth_credentials(target: str | None = None) -> list[str]:
    """Clear remembered Rust OAuth credential stores."""
    value = json.loads(_native.clear_oauth_credentials_json(target))
    if not isinstance(value, list):
        msg = "Rust core clear_oauth_credentials_json returned non-list JSON"
        raise TypeError(msg)
    return value
