from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal
from urllib import request

from mcp_compressor_rust.core import (
    BackendConfig,
    CompressedSession,
    CompressedSessionConfig,
    generate_client_artifacts,
    normalize_sdk_servers,
    start_compressed_session,
    start_compressed_session_from_mcp_config,
)

CompressorMode = Literal["compressed", "cli", "bash", "python", "typescript"]
ServerConfig = dict[str, Any] | str | BackendConfig
ServersInput = dict[str, ServerConfig] | ServerConfig | str


@dataclass(frozen=True)
class ProxyTool:
    name: str
    description: str | None
    input_schema: dict[str, Any]


@dataclass(frozen=True)
class ProxyResponse:
    text: str


@dataclass(frozen=True)
class JustBashCommand:
    command_name: str
    backend_tool_name: str
    description: str | None
    input_schema: dict[str, Any]
    invoke_tool_name: str


@dataclass(frozen=True)
class JustBashProvider:
    provider_name: str
    help_tool_name: str
    tools: list[JustBashCommand]


def _backend_from_value(name: str, value: ServerConfig) -> BackendConfig:
    if isinstance(value, BackendConfig):
        return value
    if isinstance(value, str):
        return BackendConfig(name=name, command_or_url=value)
    if "url" in value:
        args: list[str] = []
        headers = value.get("headers")
        if isinstance(headers, dict):
            for key, header_value in headers.items():
                args.extend(["-H", f"{key}={header_value}"])
            if "--auth" not in value.get("args", []):
                args.extend(["--auth", "explicit-headers"])
        args.extend(str(arg) for arg in value.get("args", []))
        return BackendConfig(name=name, command_or_url=str(value["url"]), args=args)
    if "command" in value:
        return BackendConfig(
            name=name,
            command_or_url=str(value["command"]),
            args=[str(arg) for arg in value.get("args", [])],
        )
    msg = f"Unsupported server config for {name!r}"
    raise ValueError(msg)


def _resolve_backends(servers: ServersInput) -> tuple[list[BackendConfig] | None, str | None]:
    if isinstance(servers, str):
        stripped = servers.strip()
        if stripped.startswith("{"):
            return None, servers
        return [BackendConfig(name="default", command_or_url=servers)], None
    if isinstance(servers, BackendConfig):
        return [servers], None
    return normalize_sdk_servers(servers), None


class CompressorProxy:
    def __init__(self, session: CompressedSession, default_server: str | None = None) -> None:
        self._session = session
        self._default_server = default_server
        self._closed = False

    @property
    def bridge_url(self) -> str:
        return str(self._session.info()["bridge_url"])

    @property
    def token(self) -> str:
        return str(self._session.info()["token"])

    @property
    def just_bash_providers(self) -> list[JustBashProvider]:
        providers = self._session.info().get("just_bash_providers", [])
        return [
            JustBashProvider(
                provider_name=str(provider["provider_name"]),
                help_tool_name=str(provider["help_tool_name"]),
                tools=[
                    JustBashCommand(
                        command_name=str(command["command_name"]),
                        backend_tool_name=str(command["backend_tool_name"]),
                        description=command.get("description"),
                        input_schema=dict(command["input_schema"]),
                        invoke_tool_name=str(command["invoke_tool_name"]),
                    )
                    for command in provider["tools"]
                ],
            )
            for provider in providers
        ]

    @property
    def tools(self) -> list[ProxyTool]:
        return [
            ProxyTool(
                name=str(tool["name"]),
                description=tool.get("description"),
                input_schema=dict(tool["input_schema"]),
            )
            for tool in self._session.info()["frontend_tools"]
        ]

    def close(self) -> None:
        self._closed = True
        self._session.close()

    def write_client(self, kind: str, output_dir: str | Path, *, name: str | None = None) -> list[Path]:
        info = self._session.info()
        return generate_client_artifacts(
            kind,
            cli_name=name or self._default_server or "mcp",
            bridge_url=str(info["bridge_url"]),
            token=str(info["token"]),
            tools=list(info.get("backend_tools", info["frontend_tools"])),
            output_dir=output_dir,
        )

    def invoke_wrapper(self, wrapper_tool: str, tool_input: dict[str, Any]) -> ProxyResponse:
        if self._closed:
            msg = "Compressor proxy is closed"
            raise RuntimeError(msg)
        body = json.dumps({"tool": wrapper_tool, "input": tool_input}).encode()
        req = request.Request(  # noqa: S310 - local Rust proxy URL
            f"{self.bridge_url}/exec",
            data=body,
            headers={
                "Authorization": f"Bearer {self.token}",
                "Content-Type": "application/json",
            },
            method="POST",
        )
        with request.urlopen(req, timeout=30) as response:  # noqa: S310 - local Rust proxy
            return ProxyResponse(response.read().decode())

    def schema(self, tool: str, *, server: str | None = None) -> dict[str, Any]:
        prefix = f"{server or self._default_server}_" if server or self._default_server else ""
        invoke_name = f"{prefix}invoke_tool"
        for frontend_tool in self.tools:
            if frontend_tool.name == invoke_name:
                return frontend_tool.input_schema
        msg = f"No compressed invoke wrapper found for {server or self._default_server or 'default'}"
        raise KeyError(msg)

    def invoke(self, tool: str, tool_input: dict[str, Any] | None = None, *, server: str | None = None) -> str:
        wrapper = _wrapper_name(server or self._default_server, "invoke_tool")
        return self.invoke_wrapper(wrapper, {"tool_name": tool, "tool_input": tool_input or {}}).text


def _wrapper_name(server: str | None, suffix: str) -> str:
    return f"{server}_{suffix}" if server else suffix


def normalize_servers(servers: ServersInput) -> list[BackendConfig] | None:
    """Normalize SDK server config into backend configs.

    Returns ``None`` when the input is raw MCP config JSON, which is passed through
    to the Rust core unchanged.
    """
    backends, mcp_config_json = _resolve_backends(servers)
    if mcp_config_json is not None:
        return None
    return backends


class CompressorClient:
    def __init__(
        self,
        *,
        servers: ServersInput,
        mode: CompressorMode = "compressed",
        compression_level: str = "medium",
        server_name: str | None = None,
        include_tools: list[str] | None = None,
        exclude_tools: list[str] | None = None,
        toonify: bool = False,
    ) -> None:
        self._servers = servers
        self._mode = mode
        self._config = CompressedSessionConfig(
            compression_level=compression_level,
            server_name=server_name,
            include_tools=include_tools,
            exclude_tools=exclude_tools,
            toonify=toonify,
            transform_mode=_transform_mode(mode),
        )
        self._session: CompressedSession | None = None

    def connect(self) -> CompressorProxy:
        if self._session is not None:
            return CompressorProxy(self._session, self._default_server())
        backends, mcp_config_json = _resolve_backends(self._servers)
        if mcp_config_json is not None:
            self._session = start_compressed_session_from_mcp_config(self._config, mcp_config_json)
        else:
            self._session = start_compressed_session(self._config, backends or [])
        return CompressorProxy(self._session, self._default_server())

    def _default_server(self) -> str | None:
        backends, mcp_config_json = _resolve_backends(self._servers)
        if mcp_config_json is not None:
            return None
        if backends is not None and len(backends) == 1:
            return backends[0].name
        return None

    def close(self) -> None:
        if self._session is not None:
            self._session.close()
            self._session = None

    def __enter__(self) -> CompressorProxy:
        return self.connect()

    def __exit__(self, exc_type: object, exc: object, tb: object) -> None:
        self.close()


def _transform_mode(mode: CompressorMode) -> str | None:
    if mode == "compressed":
        return None
    if mode == "bash":
        return "just-bash"
    if mode == "python" or mode == "typescript":
        return "cli"
    return mode
