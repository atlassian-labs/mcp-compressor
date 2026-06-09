"""Thin Python wrappers around the Rust core native extension."""

from __future__ import annotations

import asyncio
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
    oauth_app_name: str | None = None

    def to_json_dict(self) -> dict[str, Any]:
        payload = {
            "name": self.name,
            "command_or_url": self.command_or_url,
            "args": self.args or [],
        }
        if self.oauth_app_name is not None:
            payload["oauth_app_name"] = self.oauth_app_name
        return payload


@dataclass(frozen=True)
class CompressedSessionConfig:
    """Runtime session configuration accepted by the Rust core extension."""

    compression_level: str = "max"
    server_name: str | None = None
    include_tools: list[str] | None = None
    exclude_tools: list[str] | None = None
    toonify: bool = False
    transform_mode: str | None = None
    bridge: bool = True
    """Start the local authenticated HTTP bridge.

    Defaults to ``True`` for out-of-process clients (generated CLI/Python/TS
    clients, Just Bash). Set to ``False`` for in-process use, where tools are
    dispatched directly via :meth:`CompressedSession.invoke` without a local
    HTTP listener, token, or background task.
    """

    def to_json_dict(self) -> dict[str, Any]:
        return {
            "compression_level": self.compression_level,
            "server_name": self.server_name,
            "include_tools": self.include_tools or [],
            "exclude_tools": self.exclude_tools or [],
            "toonify": self.toonify,
            "transform_mode": self.transform_mode,
            "bridge": self.bridge,
        }


class CompressedSession:
    """Python wrapper around a Rust-backed compressed session handle.

    In addition to bridge-based access via :class:`CompressorProxy`, a session
    can be driven entirely in-process: :meth:`list_tools`, :meth:`get_schema`,
    and :meth:`invoke` reuse the session's live upstream connection and OAuth
    without an HTTP bridge. Async variants are provided for use from event
    loops (they run the blocking native call in a worker thread).
    """

    def __init__(self, native_session: Any) -> None:
        self._native_session = native_session

    def info(self) -> dict[str, Any]:
        value = json.loads(self._native_session.info_json())
        if not isinstance(value, dict):
            msg = "Rust core session info_json returned non-object JSON"
            raise TypeError(msg)
        return value

    def list_tools(self) -> list[dict[str, Any]]:
        """Return the compressed frontend tools (in-process; no bridge needed)."""
        value = json.loads(self._native_session.list_frontend_tools_json())
        if not isinstance(value, list):
            msg = "Rust core session list_frontend_tools_json returned non-list JSON"
            raise TypeError(msg)
        return value

    def get_schema(self, wrapper_tool: str, backend_tool: str) -> str:
        """Return the full backend schema response for a tool, in-process.

        ``wrapper_tool`` is the compressed ``*get_tool_schema``/``*invoke_tool``
        wrapper name (used to resolve the backend); ``backend_tool`` is the
        underlying backend tool name.
        """
        return str(self._native_session.get_tool_schema_json(wrapper_tool, backend_tool))

    def invoke(self, tool: str, tool_input: dict[str, Any] | None = None) -> str:
        """Invoke a tool in-process, reusing the session's connection and OAuth.

        ``tool`` is a frontend wrapper tool name (e.g. ``*invoke_tool``) or, in
        single-backend setups, a pass-through backend tool name. For wrapper
        invoke tools, ``tool_input`` should contain ``tool_name`` and
        ``tool_input``. Returns the same payload the HTTP bridge ``/exec``
        endpoint would return.
        """
        return str(self._native_session.invoke_tool_json(tool, _json_dumps(tool_input or {})))

    async def alist_tools(self) -> list[dict[str, Any]]:
        """Async variant of :meth:`list_tools`."""
        return await asyncio.to_thread(self.list_tools)

    async def aget_schema(self, wrapper_tool: str, backend_tool: str) -> str:
        """Async variant of :meth:`get_schema`."""
        return await asyncio.to_thread(self.get_schema, wrapper_tool, backend_tool)

    async def ainvoke(self, tool: str, tool_input: dict[str, Any] | None = None) -> str:
        """Async variant of :meth:`invoke`."""
        return await asyncio.to_thread(self.invoke, tool, tool_input)

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
    """Start a Rust-backed compressed session.

    By default this also starts a local HTTP bridge for out-of-process clients.
    Set ``config.bridge=False`` for in-process use (dispatch via the session's
    :meth:`CompressedSession.invoke` / :meth:`CompressedSession.list_tools`).
    """
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


def generate_client_artifact_files(
    kind: str,
    *,
    cli_name: str,
    bridge_url: str,
    token: str,
    tools: list[dict[str, Any]],
    output_dir: str | Path,
    session_pid: int = 0,
) -> dict[str, str]:
    config = {
        "cli_name": cli_name,
        "bridge_url": bridge_url,
        "token": token,
        "tools": tools,
        "output_dir": str(output_dir),
        "session_pid": session_pid,
    }
    raw = json.loads(_native.generate_client_artifact_files_json(kind, _json_dumps(config)))
    if not isinstance(raw, dict):
        msg = "Rust core generate_client_artifact_files_json returned non-object JSON"
        raise TypeError(msg)
    return {str(name): str(content) for name, content in raw.items()}


def normalize_sdk_servers(servers: dict[str, Any]) -> list[BackendConfig]:
    raw = json.loads(_native.normalize_servers_json(_json_dumps(servers)))
    if not isinstance(raw, list):
        msg = "Rust core normalize_servers_json returned non-list JSON"
        raise TypeError(msg)
    return [
        BackendConfig(
            name=str(item["name"]),
            command_or_url=str(item["command_or_url"]),
            args=list(item.get("args", [])),
            oauth_app_name=(str(item["oauth_app_name"]) if item.get("oauth_app_name") is not None else None),
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
