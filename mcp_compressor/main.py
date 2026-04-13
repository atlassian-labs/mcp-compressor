"""Main entry point for the MCP Compressor CLI.

This module provides the CLI interface for running the MCP Compressor proxy server, which wraps existing MCP servers and
compresses their tool descriptions to reduce token consumption.
"""

import asyncio
import contextlib
import importlib.metadata
import os
import re
import shutil
import signal
import socket
import sys
import threading
import warnings
from collections.abc import AsyncGenerator
from contextlib import asynccontextmanager
from pathlib import Path
from typing import TYPE_CHECKING, Annotated, Any, Literal, cast, overload

import anyio
import keyring
import keyring.errors
import psutil
import typer
from click.core import ParameterSource
from cryptography.fernet import Fernet
from fastmcp import FastMCP
from fastmcp.client.auth import OAuth
from fastmcp.client.transports import SSETransport, StdioTransport, StreamableHttpTransport
from fastmcp.mcp_config import MCPConfig, RemoteMCPServer, StdioMCPServer
from fastmcp.server import create_proxy
from fastmcp.server.providers.proxy import ProxyClient
from key_value.aio.protocols import AsyncKeyValue
from key_value.aio.stores.filetree import (
    FileTreeStore,
    FileTreeV1CollectionSanitizationStrategy,
    FileTreeV1KeySanitizationStrategy,
)
from key_value.aio.wrappers.encryption import FernetEncryptionWrapper
from loguru import logger
from pydantic import ValidationError

from .banner import print_banner
from .cli_bridge import CliBridge
from .cli_script import generate_cli_script, remove_cli_script_entry
from .cli_tools import sanitize_cli_name
from .logging import configure_logging, suppress_recoverable_oauth_traceback_logging
from .tools import CompressedTools
from .types import CompressionLevel, LogLevel, TransportType

if TYPE_CHECKING:
    from just_bash.types import IFileSystem

# Suppress known third-party deprecation warnings that are not actionable from this project.
# uvicorn's websockets implementation uses WebSocketServerProtocol which was deprecated in websockets 14.0.
warnings.filterwarnings("ignore", category=DeprecationWarning, module="uvicorn")
warnings.filterwarnings("ignore", category=DeprecationWarning, module="websockets")

app = typer.Typer(name="MCP Compressor", help="An MCP server wrapper for reducing tokens consumed by MCP tools.")


def _version_callback(value: bool) -> None:
    if value:
        try:
            version = importlib.metadata.version("mcp-compressor")
        except importlib.metadata.PackageNotFoundError:
            version = "unknown"
        typer.echo(f"mcp-compressor {version}")
        raise typer.Exit()


def _check_conflicting_config_options(ctx: typer.Context) -> None:
    """Raise BadParameter if any transport options that conflict with JSON config were provided."""
    conflicting = [
        name
        for name in ("cwd", "env_list", "header_list", "timeout")
        if ctx.get_parameter_source(name) == ParameterSource.COMMANDLINE
    ]
    if conflicting:
        joined = ", ".join(f"--{n.removesuffix('_list').replace('_', '-')}" for n in conflicting)
        raise typer.BadParameter(
            f"JSON MCP config input cannot be combined with {joined}; configure those values inside the JSON instead.",
            param_hint="'COMMAND_OR_URL'",
        )


def _resolve_config_server_name(config: MCPConfig, server_name: str | None, cli_mode: bool) -> str | None:
    """Return the effective server name from a parsed MCP config.

    For single-server configs the JSON key is used as a fallback when no explicit ``--server-name``
    was provided.  For multi-server configs the caller's ``server_name`` is returned unchanged and
    used as a common prefix for per-server names.  In multi-server CLI mode, ``None`` is allowed and
    falls back to the generated ``mcp`` CLI script name while individual servers use their JSON keys.
    """
    if len(config.mcpServers) == 1:
        config_server_name = next(iter(config.mcpServers))
        return server_name or config_server_name

    return server_name


@app.command()
def main(
    ctx: typer.Context,
    command_or_url_list: Annotated[
        list[str],
        typer.Argument(
            ...,
            metavar="COMMAND_OR_URL",
            help=(
                "The backend to wrap: either a remote MCP URL, a stdio command plus arguments, or an MCP config "
                "JSON string with one or more servers. Example stdio usage: uvx mcp-server-fetch"
            ),
        ),
    ],
    cwd: Annotated[
        str | None,
        typer.Option(
            ...,
            "--cwd",
            help="The working directory to use when running stdio MCP servers.",
        ),
    ] = None,
    env_list: Annotated[
        list[str] | None,
        typer.Option(
            ...,
            "--env",
            "-e",
            help=(
                "Environment variables to set when running stdio MCP servers, in the form VAR_NAME=VALUE. Can be used "
                "multiple times. Supports environment variable expansion with ${VAR_NAME} syntax."
            ),
        ),
    ] = None,
    header_list: Annotated[
        list[str] | None,
        typer.Option(
            ...,
            "--header",
            "-H",
            help=(
                "Headers to use for remote (HTTP/SSE) MCP server connections, in the form Header-Name=Header-Value. "
                "Can be use multiple times. Supports environment variable expansion with ${VAR_NAME} syntax."
            ),
        ),
    ] = None,
    timeout: Annotated[
        float,
        typer.Option(
            ...,
            "--timeout",
            "-t",
            help="The timeout in seconds for connecting to the MCP server and making requests.",
        ),
    ] = 10.0,
    compression_level: Annotated[
        CompressionLevel,
        typer.Option(
            ...,
            "--compression-level",
            "-c",
            help=("The level of compression to apply to tool the tools descriptions of the wrapped MCP server."),
            case_sensitive=False,
        ),
    ] = CompressionLevel.MEDIUM,
    server_name: Annotated[
        str | None,
        typer.Option(
            ...,
            "--server-name",
            "-n",
            help=(
                "Optional custom name to prefix the wrapper tool names (get_tool_schema, invoke_tool, list_tools). "
                "The name will be sanitized to conform to MCP tool name specifications (only A-Z, a-z, 0-9, _, -, .)."
            ),
        ),
    ] = None,
    log_level: Annotated[
        LogLevel,
        typer.Option(
            ...,
            "--log-level",
            "-l",
            help=(
                "The logging level. Used for both the MCP Compressor server and the underlying MCP server if it is a "
                "stdio server."
            ),
            case_sensitive=False,
        ),
    ] = LogLevel.ERROR,
    toonify: Annotated[
        bool,
        typer.Option(..., "--toonify", help="Convert JSON tool responses to TOON format automatically."),
    ] = False,
    cli_mode: Annotated[
        bool,
        typer.Option(
            ...,
            "--cli-mode",
            help=(
                "Start in CLI mode: expose a single help MCP tool, start a local HTTP bridge, "
                "and generate a shell script for interacting with the wrapped server via CLI. "
                "--toonify is automatically enabled in this mode."
            ),
        ),
    ] = False,
    just_bash: Annotated[
        bool,
        typer.Option(
            ...,
            "--just-bash",
            help=(
                "Start in just-bash mode: expose a single 'bash' MCP tool powered by "
                "just-bash, with all backend server tools available as custom commands. "
                "--toonify is automatically enabled in this mode."
            ),
        ),
    ] = False,
    cli_port: Annotated[
        int | None,
        typer.Option(
            ...,
            "--cli-port",
            help="Port for the local CLI bridge HTTP server (default: random free port).",
        ),
    ] = None,
    include_tools: Annotated[
        str | None,
        typer.Option(
            ...,
            "--include-tools",
            help=("Comma-separated list of wrapped server tool names to expose. If omitted, all tools are included."),
        ),
    ] = None,
    exclude_tools: Annotated[
        str | None,
        typer.Option(
            ...,
            "--exclude-tools",
            help="Comma-separated list of wrapped server tool names to hide.",
        ),
    ] = None,
    version: Annotated[
        bool,
        typer.Option(
            "--version",
            "-V",
            help="Show the version and exit.",
            callback=_version_callback,
            is_eager=True,
            expose_value=False,
        ),
    ] = False,
):
    """Run the MCP Compressor proxy server.

    This is the main entry point for the CLI application. It connects to an MCP server
    (via stdio, HTTP, or SSE) and wraps it with a compressed tool interface.
    """
    configure_logging(log_level)

    resolved_server_name = server_name
    try:
        parsed_config = _parse_mcp_config_json(command_or_url_list)
    except ValueError as exc:
        raise typer.BadParameter(str(exc), param_hint="'COMMAND_OR_URL'") from exc

    if parsed_config is not None:
        _check_conflicting_config_options(ctx)
        resolved_server_name = _resolve_config_server_name(parsed_config, server_name, cli_mode)

    if (
        (cli_mode or just_bash)
        and resolved_server_name is None
        and (parsed_config is None or len(parsed_config.mcpServers) == 1)
    ):
        raise typer.BadParameter(
            "--server-name is required when using --cli-mode or --just-bash.", param_hint="'--server-name'"
        )
    if compression_level == CompressionLevel.MAX and resolved_server_name is None:
        raise typer.BadParameter(
            "--server-name is required when using --compression-level=max.", param_hint="'--server-name'"
        )

    if threading.current_thread() is threading.main_thread():
        shutting_down = False

        def _handle_interrupt(signum: int, frame: object) -> None:
            nonlocal shutting_down
            if shutting_down:
                logger.debug("Ignoring additional interrupt signal during shutdown")
                return
            shutting_down = True
            logger.info("Server stopped")
            # Terminate child processes (stdio backend server) to avoid zombies
            with contextlib.suppress(Exception):
                current = psutil.Process()
                for child in current.children(recursive=True):
                    with contextlib.suppress(Exception):
                        child.terminate()
            # os._exit(0) bypasses daemon thread join hangs (both stdio stdin-read
            # threads and HTTP transport threads can block interpreter shutdown)
            os._exit(0)

        signal.signal(signal.SIGINT, _handle_interrupt)
        signal.signal(signal.SIGTERM, _handle_interrupt)

    asyncio.run(
        _async_main(
            command_or_url_list=command_or_url_list,
            cwd=cwd,
            env_list=env_list,
            header_list=header_list,
            timeout=timeout,
            compression_level=compression_level,
            server_name=resolved_server_name,
            log_level=log_level,
            toonify=toonify or cli_mode or just_bash,
            cli_mode=cli_mode,
            just_bash=just_bash,
            cli_port=cli_port,
            include_tools=_parse_tool_name_list(include_tools),
            exclude_tools=_parse_tool_name_list(exclude_tools),
        )
    )


clear_oauth_app = typer.Typer(name="clear-oauth", help="Clear stored OAuth tokens.")


@clear_oauth_app.callback(invoke_without_command=True)
def clear_oauth(
    all_tokens: Annotated[
        bool,
        typer.Option("--all", help="Also delete the encryption key, forcing full re-authentication next run."),
    ] = False,
) -> None:
    """Clear stored OAuth tokens for all servers.

    Removes cached OAuth tokens so the next connection will re-authenticate.
    By default the encryption key is preserved so new tokens are stored under
    the same key.  Pass --all to also remove the key itself.
    """
    token_dir = _OAUTH_CONFIG_DIR / "oauth-tokens"
    key_file = _OAUTH_CONFIG_DIR / ".key"
    removed: list[str] = []

    if token_dir.exists():
        shutil.rmtree(token_dir)
        removed.append(str(token_dir))

    if all_tokens and key_file.exists():
        key_file.unlink()
        removed.append(str(key_file))

    if removed:
        print("Removed:")
        for path in removed:
            print(f"  {path}")
        # Also clear from keyring if present
        if all_tokens:
            with contextlib.suppress(Exception):
                keyring.delete_password(_KEYRING_SERVICE, _KEYRING_USERNAME)
                print(f"  keyring entry: {_KEYRING_SERVICE} / {_KEYRING_USERNAME}")
        print("OAuth credentials cleared. You will be prompted to authenticate on next connection.")
    else:
        print("No stored OAuth credentials found.")


def _should_retry_stale_oauth_connect_error(exception: Exception, transport: TransportType) -> bool:
    """Return whether a connection error looks like a stale cached OAuth state issue."""
    if not isinstance(transport, StreamableHttpTransport | SSETransport):
        return False

    auth = getattr(transport, "auth", None)
    if not isinstance(auth, OAuth):
        return False

    exc_str = str(exception)
    return "Unexpected authorization response: 500" in exc_str or "OAuth client not found" in exc_str


async def _clear_transport_oauth_cache(transport: TransportType) -> None:
    """Clear cached OAuth state associated with a transport, if available."""
    auth = getattr(transport, "auth", None)
    if not isinstance(auth, OAuth) or not hasattr(auth, "token_storage_adapter"):
        return

    auth._initialized = False
    await auth.token_storage_adapter.clear()


@asynccontextmanager
async def _proxy_client(transport: TransportType) -> AsyncGenerator[ProxyClient, None]:
    """Connect a proxy client, retrying once after clearing stale cached OAuth state."""
    try:
        with suppress_recoverable_oauth_traceback_logging(transport):
            async with ProxyClient(transport=transport, init_timeout=None) as client:
                yield client
                return
    except Exception as exc:
        if not _should_retry_stale_oauth_connect_error(exc, transport):
            raise

        logger.warning(
            "OAuth connection failed due to stale cached credentials; clearing cached OAuth state and retrying once"
        )
        await _clear_transport_oauth_cache(transport)

    try:
        async with ProxyClient(transport=transport, init_timeout=None) as client:
            yield client
    except Exception as exc:
        raise RuntimeError(
            f"{exc}\n\nCached OAuth credentials may be stale. "
            "mcp-compressor cleared cached OAuth state and retried once. "
            "If the problem persists, run 'mcp-compressor clear-oauth' and try again."
        ) from exc


async def _async_main(
    command_or_url_list: list[str],
    cwd: str | None,
    env_list: list[str] | None,
    header_list: list[str] | None,
    timeout: float,
    compression_level: CompressionLevel,
    server_name: str | None,
    log_level: LogLevel,
    toonify: bool,
    cli_mode: bool = False,
    just_bash: bool = False,
    cli_port: int | None = None,
    include_tools: list[str] | None = None,
    exclude_tools: list[str] | None = None,
) -> None:
    """Run the MCP Compressor proxy server asynchronously."""
    logger.info(f"Starting MCP Compressor with log level: {log_level.value}")

    async with _server(
        command_or_url_list=command_or_url_list,
        cwd=cwd,
        env_list=env_list,
        header_list=header_list,
        timeout=timeout,
        compression_level=compression_level,
        server_name=server_name,
        toonify=toonify,
        cli_mode=cli_mode,
        just_bash=just_bash,
        cli_port=cli_port,
        include_tools=include_tools,
        exclude_tools=exclude_tools,
    ) as mcp:
        logger.info("Starting MCP Compressor server")
        await mcp.run_async(show_banner=False, log_level=log_level.value)


@asynccontextmanager
async def _server(
    command_or_url_list: list[str],
    cwd: str | None,
    env_list: list[str] | None,
    header_list: list[str] | None,
    timeout: float,
    compression_level: CompressionLevel,
    server_name: str | None,
    toonify: bool = False,
    cli_mode: bool = False,
    just_bash: bool = False,
    cli_port: int | None = None,
    include_tools: list[str] | None = None,
    exclude_tools: list[str] | None = None,
) -> AsyncGenerator[FastMCP, None]:
    command_or_url = " ".join(command_or_url_list)
    parsed_config = _parse_mcp_config_json(command_or_url_list)
    if parsed_config is not None and len(parsed_config.mcpServers) > 1:
        logger.info(f"Loaded multi-server MCP config JSON with {len(parsed_config.mcpServers)} servers")
        async with _multi_server(
            config=parsed_config,
            compression_level=compression_level,
            server_name=server_name,
            toonify=toonify,
            cli_mode=cli_mode,
            just_bash=just_bash,
            cli_port=cli_port,
            include_tools=include_tools,
            exclude_tools=exclude_tools,
        ) as mcp:
            yield mcp
        return

    if parsed_config is not None:
        transport, transport_type = _get_single_server_transport_from_mcp_config(config=parsed_config)
        logger.info("Loaded single-server MCP config JSON")
    else:
        transport_type = _infer_transport_type(command_or_url)
        logger.info(f"Inferred transport type: {transport_type}")

        # Handle different transport types
        if transport_type == "stdio":
            transport = _get_stdio_transport(
                command=command_or_url_list[0], args=command_or_url_list[1:], cwd=cwd, env_list=env_list
            )
        elif transport_type == "http":
            transport = _get_streamable_http_transport(url=command_or_url, header_list=header_list, timeout=timeout)
        elif transport_type == "sse":
            transport = _get_sse_transport(url=command_or_url, header_list=header_list, timeout=timeout)

    if just_bash:
        async with _just_bash_server(
            transport=transport,
            compression_level=compression_level,
            server_name=server_name,
            toonify=toonify,
            include_tools=include_tools,
            exclude_tools=exclude_tools,
        ) as mcp:
            yield mcp
        return

    if cli_mode:
        cli_name = sanitize_cli_name(server_name or "mcp")
        async with _cli_mode_server(
            transport=transport,
            transport_type=transport_type,
            cli_name=cli_name,
            compression_level=compression_level,
            server_name=server_name,
            toonify=toonify,
            cli_port=cli_port,
            include_tools=include_tools,
            exclude_tools=exclude_tools,
        ) as mcp:
            yield mcp
        return

    logger.info("Initializing proxy server")
    async with _proxy_client(transport) as client:
        mcp = create_proxy(client, name="MCP Compressor Proxy")

        # Shared compressed tools for backend access
        compressed_tools = CompressedTools(
            mcp,
            compression_level=compression_level,
            server_name=server_name,
            toonify=toonify,
            include_tools=include_tools,
            exclude_tools=exclude_tools,
        )

        logger.info("Configuring compressed tools middleware")
        await compressed_tools.configure_server()

        stats = await compressed_tools.get_compression_stats()
        print_banner(server_name, transport_type, stats, compression_level)

        yield mcp


@asynccontextmanager
async def _cli_mode_server(
    transport: TransportType,
    transport_type: str,
    cli_name: str,
    compression_level: CompressionLevel,
    server_name: str | None,
    toonify: bool,
    cli_port: int | None,
    include_tools: list[str] | None,
    exclude_tools: list[str] | None,
) -> AsyncGenerator[FastMCP, None]:
    """Set up and run the CLI mode server.

    Architecture is identical to normal mode — ProxyClient + CompressedTools —
    with cli_mode=True so CompressedTools registers the single help tool instead
    of the wrapper tools, and the bridge calls invoke_tool directly.
    """
    async with _proxy_client(transport) as client:
        logger.info("Initializing proxy server for CLI mode")
        mcp = create_proxy(client, name="MCP Compressor Proxy", version="0.1.0")

        compressed_tools = CompressedTools(
            mcp,
            compression_level=compression_level,
            server_name=server_name,
            toonify=toonify,
            cli_mode=True,
            cli_name=cli_name,
            include_tools=include_tools,
            exclude_tools=exclude_tools,
        )
        await compressed_tools.configure_server()

        stats = await compressed_tools.get_compression_stats()

        port = cli_port or _find_free_port()
        session_pid = os.getppid()

        bridge = CliBridge(
            cli_name=cli_name,
            server_description=compressed_tools._server_description,
            get_tools_fn=compressed_tools.get_backend_tools,
            invoke_fn=compressed_tools.invoke_tool,
            port=port,
            fastmcp=mcp,
        )
        bridge_server = bridge.make_server()

        async with anyio.create_task_group() as tg:
            tg.start_soon(bridge_server.serve)
            while not bridge_server.started:
                await anyio.sleep(0.01)

            script_path, on_path = generate_cli_script(cli_name=cli_name, bridge_port=port, session_pid=session_pid)
            invoke_prefix = cli_name if on_path else f"./{cli_name}"
            print_banner(
                server_name,
                transport_type,
                stats,
                compression_level,
                cli_mode=True,
                cli_script_path=str(script_path),
                cli_bridge_url=f"http://127.0.0.1:{port}",
                cli_invoke_prefix=invoke_prefix,
            )

            logger.info("Starting MCP Compressor server in CLI mode")
            try:
                yield mcp
            finally:
                bridge_server.should_exit = True
                tg.cancel_scope.cancel()
                remove_cli_script_entry(cli_name=cli_name, session_pid=session_pid)
                logger.debug("Removed CLI script entry for session_pid={}", session_pid)


@asynccontextmanager
async def _just_bash_server(
    transport: TransportType,
    compression_level: CompressionLevel,
    server_name: str | None,
    toonify: bool,
    include_tools: list[str] | None,
    exclude_tools: list[str] | None,
) -> AsyncGenerator[FastMCP, None]:
    """Set up a just-bash server that exposes a single 'bash' MCP tool.

    All backend MCP tools are registered as custom commands in a just-bash sandboxed
    shell with ReadWriteFs rooted at the current working directory.
    """
    from just_bash import Bash
    from just_bash.fs import ReadWriteFs
    from just_bash.fs.read_write_fs import ReadWriteFsOptions

    from mcp_compressor.bash_commands import build_bash_tool_description, create_bash_command

    async with _proxy_client(transport) as client:
        logger.info("Initializing proxy server for just-bash mode")
        # Use a proxy to connect to the backend and fetch tools, but serve via a fresh FastMCP
        proxy = create_proxy(client, name="MCP Compressor Backend")

        compressed_tools = CompressedTools(
            proxy,
            compression_level=compression_level,
            server_name=server_name,
            toonify=toonify,
            include_tools=include_tools,
            exclude_tools=exclude_tools,
        )
        await compressed_tools.configure_server()

        cli_name = sanitize_cli_name(server_name or "mcp")
        backend_tools = await compressed_tools.get_backend_tools()
        tools_list = list(backend_tools.values())

        command = create_bash_command(
            cli_name, compressed_tools._server_description, cast(list[Any], tools_list), compressed_tools.invoke_tool
        )
        bash = Bash(
            commands={command.name: command},
            fs=cast("IFileSystem", ReadWriteFs(ReadWriteFsOptions(root=os.getcwd()))),
            cwd="/",
        )
        description = build_bash_tool_description([
            {
                "server_name": cli_name,
                "server_description": compressed_tools._server_description,
                "command": command,
                "tools": cast(list[Any], tools_list),
            }
        ])

        # Serve only the bash tool via a fresh FastMCP (not the proxy with backend tools)
        mcp = FastMCP(name="MCP Compressor Proxy")

        @mcp.tool(description=description)
        async def bash_tool(command: str) -> str:
            """Execute a bash command."""
            result = await bash.exec(command)
            if result.exit_code != 0:
                return f"Exit code: {result.exit_code}\n{result.stdout}\n{f'STDERR: {result.stderr}' if result.stderr else ''}"
            return result.stdout or "(no output)"

        logger.info("Starting MCP Compressor server in just-bash mode")
        yield mcp


@asynccontextmanager
async def _multi_server(
    config: MCPConfig,
    compression_level: CompressionLevel,
    server_name: str | None,
    toonify: bool,
    cli_mode: bool,
    just_bash: bool = False,
    cli_port: int | None = None,
    include_tools: list[str] | None = None,
    exclude_tools: list[str] | None = None,
) -> AsyncGenerator[FastMCP, None]:
    """Set up a proxy server that aggregates multiple backend servers from an MCP config."""
    outer_mcp = FastMCP(name="MCP Compressor Proxy")
    logger.info("Initializing multi-server proxy")

    async with contextlib.AsyncExitStack() as exit_stack:
        server_commands: list[dict] = []  # For just-bash aggregation

        for config_server_name, server_config in config.mcpServers.items():
            effective_name = f"{server_name}_{config_server_name}" if server_name else config_server_name
            single_config = MCPConfig(mcpServers={config_server_name: server_config})
            transport, transport_type = _get_single_server_transport_from_mcp_config(config=single_config)

            if cli_mode and not just_bash:
                proxy = await exit_stack.enter_async_context(
                    _cli_mode_server(
                        transport=transport,
                        transport_type=transport_type,
                        cli_name=sanitize_cli_name(effective_name),
                        compression_level=compression_level,
                        server_name=effective_name,
                        toonify=toonify,
                        cli_port=cli_port,
                        include_tools=include_tools,
                        exclude_tools=exclude_tools,
                    )
                )
                outer_mcp.mount(proxy)
                continue

            client = await exit_stack.enter_async_context(_proxy_client(transport))
            proxy = create_proxy(client, name=f"MCP Compressor Proxy - {effective_name}")
            compressed_tools = CompressedTools(
                proxy,
                compression_level=compression_level,
                server_name=effective_name,
                toonify=toonify,
                include_tools=include_tools,
                exclude_tools=exclude_tools,
            )
            logger.info(f"Configuring compressed tools for server '{effective_name}'")
            await compressed_tools.configure_server()

            if just_bash:
                from mcp_compressor.bash_commands import create_bash_command

                cli_name = sanitize_cli_name(effective_name)
                backend_tools = await compressed_tools.get_backend_tools()
                tools_list = list(backend_tools.values())
                command = create_bash_command(
                    cli_name,
                    compressed_tools._server_description,
                    cast(list[Any], tools_list),
                    compressed_tools.invoke_tool,
                )
                server_commands.append({
                    "server_name": cli_name,
                    "server_description": compressed_tools._server_description,
                    "command": command,
                    "tools": cast(list[Any], tools_list),
                })
            else:
                stats = await compressed_tools.get_compression_stats()
                print_banner(effective_name, transport_type, stats, compression_level)
                outer_mcp.mount(proxy)

        if just_bash and server_commands:
            from just_bash import Bash
            from just_bash.fs import ReadWriteFs
            from just_bash.fs.read_write_fs import ReadWriteFsOptions

            from mcp_compressor.bash_commands import build_bash_tool_description

            all_commands = {sc["command"].name: sc["command"] for sc in server_commands}
            bash = Bash(
                commands=all_commands,
                fs=cast("IFileSystem", ReadWriteFs(ReadWriteFsOptions(root=os.getcwd()))),
                cwd="/",
            )
            description = build_bash_tool_description(server_commands)

            @outer_mcp.tool(description=description)
            async def bash_tool(command: str) -> str:
                """Execute a bash command."""
                result = await bash.exec(command)
                if result.exit_code != 0:
                    return (
                        f"Exit code: {result.exit_code}\n{result.stdout}\n"
                        f"{f'STDERR: {result.stderr}' if result.stderr else ''}"
                    )
                return result.stdout or "(no output)"

        yield outer_mcp


_OAUTH_CONFIG_DIR = Path.home() / ".config" / "mcp-compressor"
_OAUTH_TOKEN_DIR = _OAUTH_CONFIG_DIR / "oauth-tokens"
_OAUTH_KEY_FILE = _OAUTH_CONFIG_DIR / ".key"
_KEYRING_SERVICE = "mcp-compressor"
_KEYRING_USERNAME = "oauth-encryption-key"


def _get_or_create_encryption_key() -> bytes:
    """Return a persistent Fernet encryption key for OAuth token storage.

    Tries the OS keychain first (macOS Keychain, Windows Credential Manager,
    GNOME Keyring).  Falls back to a file at
    ``~/.config/mcp-compressor/.key`` with 0o600 permissions if the keychain
    is unavailable (e.g. headless/server environments).
    """
    # 1. Try OS keychain
    try:
        stored = keyring.get_password(_KEYRING_SERVICE, _KEYRING_USERNAME)
        if stored:
            logger.debug("OAuth encryption key loaded from OS keychain")
            return stored.encode()
        # Generate and store a new key
        new_key = Fernet.generate_key()
        keyring.set_password(_KEYRING_SERVICE, _KEYRING_USERNAME, new_key.decode())
        logger.debug("OAuth encryption key generated and stored in OS keychain")
    except keyring.errors.NoKeyringError:
        logger.debug("No OS keychain available; falling back to file-based encryption key")
    else:
        return new_key

    # 2. File-based fallback
    _OAUTH_CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    if _OAUTH_KEY_FILE.exists():
        key = _OAUTH_KEY_FILE.read_bytes().strip()
        logger.debug("OAuth encryption key loaded from {}", _OAUTH_KEY_FILE)
        return key
    new_key = Fernet.generate_key()
    _OAUTH_KEY_FILE.write_bytes(new_key)
    _OAUTH_KEY_FILE.chmod(0o600)
    logger.debug("OAuth encryption key generated and stored at {}", _OAUTH_KEY_FILE)
    return new_key


def _build_token_storage() -> AsyncKeyValue:
    """Build an encrypted persistent OAuth token storage backend.

    Tokens are stored in ``~/.config/mcp-compressor/oauth-tokens`` using a FileTreeStore with stable sanitization
    strategies, then encrypted with a Fernet key obtained from :func:`_get_or_create_encryption_key`.
    """
    _OAUTH_TOKEN_DIR.mkdir(parents=True, exist_ok=True)
    store: AsyncKeyValue = FileTreeStore(
        data_directory=_OAUTH_TOKEN_DIR,
        key_sanitization_strategy=FileTreeV1KeySanitizationStrategy(_OAUTH_TOKEN_DIR),
        collection_sanitization_strategy=FileTreeV1CollectionSanitizationStrategy(_OAUTH_TOKEN_DIR),
    )
    fernet_key = _get_or_create_encryption_key()
    encrypted_store = FernetEncryptionWrapper(key_value=store, fernet=Fernet(fernet_key))
    logger.debug("OAuth token storage: encrypted file-tree store at {}", _OAUTH_TOKEN_DIR)
    return encrypted_store


def _find_free_port() -> int:
    """Find a free port on the loopback interface."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _parse_tool_name_list(tool_name_group: str | None) -> list[str] | None:
    """Parse a comma-separated tool name list into a normalized list."""
    if not tool_name_group:
        return None

    tool_names = [tool_name.strip() for tool_name in tool_name_group.split(",")]
    return [tool_name for tool_name in tool_names if tool_name] or None


def _parse_single_server_mcp_config(command_or_url_list: list[str]) -> tuple[MCPConfig, str] | None:
    """Parse a single JSON MCP config argument and return the config plus its only server name."""
    if len(command_or_url_list) != 1:
        return None

    command_or_url = command_or_url_list[0].strip()
    if not command_or_url.startswith("{"):
        return None

    try:
        config = MCPConfig.model_validate_json(command_or_url)
    except ValidationError as exc:
        raise ValueError("Invalid MCP config JSON provided for COMMAND_OR_URL.") from exc

    if len(config.mcpServers) != 1:
        raise ValueError("MCP config JSON must contain exactly one server in the mcpServers map.")

    server_name = next(iter(config.mcpServers))
    return config, server_name


def _parse_mcp_config_json(command_or_url_list: list[str]) -> MCPConfig | None:
    """Parse a JSON MCP config argument, supporting one or more servers.

    Returns the parsed MCPConfig (with any number of servers), or None if the input is not a JSON
    config string. Raises ValueError for invalid JSON or an empty mcpServers map.
    """
    if len(command_or_url_list) != 1:
        return None

    command_or_url = command_or_url_list[0].strip()
    if not command_or_url.startswith("{"):
        return None

    try:
        config = MCPConfig.model_validate_json(command_or_url)
    except ValidationError as exc:
        raise ValueError("Invalid MCP config JSON provided for COMMAND_OR_URL.") from exc

    if len(config.mcpServers) == 0:
        raise ValueError("MCP config JSON must contain at least one server in the mcpServers map.")

    return config


def _get_single_server_transport_from_mcp_config(
    config: MCPConfig,
) -> tuple[TransportType, Literal["stdio", "http", "sse"]]:
    """Create a transport for a validated single-server MCP config.

    Single-server config JSON is self-contained: transport settings come only from the config.
    Remote configs default to the same OAuth flow used by direct URL inputs unless explicit auth is provided.
    """
    server_config = next(iter(config.mcpServers.values()))

    if isinstance(server_config, StdioMCPServer):
        return (
            _get_stdio_transport(
                command=server_config.command,
                args=server_config.args,
                cwd=server_config.cwd,
                env_list=[f"{key}={value}" for key, value in server_config.env.items()] or None,
            ),
            "stdio",
        )

    if isinstance(server_config, RemoteMCPServer):
        transport_type = "sse" if (server_config.transport == "sse") else _infer_transport_type(server_config.url)
        auth = server_config.auth
        if auth in (None, "oauth"):
            auth = OAuth(mcp_url=server_config.url, token_storage=_build_token_storage())

        headers = {key: _interpolate_string(value) for key, value in server_config.headers.items()}
        if transport_type == "http":
            return (
                StreamableHttpTransport(
                    url=server_config.url,
                    headers=headers,
                    auth=auth,
                    sse_read_timeout=server_config.sse_read_timeout,
                ),
                "http",
            )
        return (
            SSETransport(
                url=server_config.url,
                headers=headers,
                auth=auth,
                sse_read_timeout=server_config.sse_read_timeout,
            ),
            "sse",
        )

    raise ValueError("Unsupported single-server MCP config type.")


def _interpolate_string(value: str) -> str:
    """Interpolate environment variables in a single string.

    Args:
        value: A string that may contain environment variable references like ${VAR_NAME}.

    Returns:
        The string with interpolated environment variables. If a variable cannot be interpolated, it is left as-is
            without interpolation.
    """
    try:
        if not value or "${" not in value:
            return value
        # Replace ${VAR_NAME} with {VAR_NAME} and use format() with environment variables
        return value.replace("${", "{").format(**os.environ)
    except Exception as e:
        logger.warning(f"Failed to interpolate environment variable {value}: {e}, using uninterpolated value")
        return value


def _infer_transport_type(command_or_url: str) -> Literal["stdio", "http", "sse"]:
    """Infer a transport type from a command or URL string."""
    if not command_or_url.startswith(("http://", "https://")):
        return "stdio"
    return "sse" if re.search(r"/sse(/|\?|&|$)", command_or_url) else "http"


@overload
def _get_remote_transport(
    url: str, header_list: list[str] | None, timeout: float, transport_type: Literal["http"]
) -> StreamableHttpTransport: ...


@overload
def _get_remote_transport(
    url: str, header_list: list[str] | None, timeout: float, transport_type: Literal["sse"]
) -> SSETransport: ...


def _get_remote_transport(
    url: str, header_list: list[str] | None, timeout: float, transport_type: Literal["http", "sse"]
) -> StreamableHttpTransport | SSETransport:
    """Create a remote transport (HTTP or SSE) with the specified configuration.

    Args:
        url: The URL of the remote MCP server.
        header_list: Optional list of headers in Header-Name=Value format.
        timeout: Timeout for SSE read operations.
        transport_type: Either "http" for streamable HTTP or "sse" for server-sent events.

    Returns:
        Configured transport instance for the specified type.
    """
    header_dict: dict[str, str] = {}
    if header_list:
        for header in header_list:
            key, val = header.split("=", 1)
            header_dict[key] = _interpolate_string(val)

    oauth = OAuth(mcp_url=url, token_storage=_build_token_storage())

    if transport_type == "http":
        return StreamableHttpTransport(url=url, headers=header_dict, auth=oauth)
    return SSETransport(url=url, headers=header_dict, auth=oauth, sse_read_timeout=timeout)


def _get_streamable_http_transport(url: str, header_list: list[str] | None, timeout: float) -> StreamableHttpTransport:
    """Create a streamable HTTP transport for connecting to an MCP server.

    Args:
        url: The HTTP URL of the MCP server.
        header_list: Optional list of headers in Header-Name=Value format.
        timeout: Timeout for read operations.

    Returns:
        Configured StreamableHttpTransport instance.
    """
    return _get_remote_transport(url, header_list, timeout, transport_type="http")


def _get_sse_transport(url: str, header_list: list[str] | None, timeout: float) -> SSETransport:
    """Create an SSE (Server-Sent Events) transport for connecting to an MCP server.

    Args:
        url: The SSE URL of the MCP server.
        header_list: Optional list of headers in Header-Name=Value format.
        timeout: Timeout for SSE read operations.

    Returns:
        Configured SSETransport instance.
    """
    return _get_remote_transport(url, header_list, timeout, transport_type="sse")


def _get_stdio_transport(command: str, args: list[str], cwd: str | None, env_list: list[str] | None) -> StdioTransport:
    """Create a stdio transport for running a local MCP server as a subprocess.

    Args:
        command: The command to execute (e.g., "uvx", "python").
        args: Arguments to pass to the command.
        cwd: Optional working directory for the subprocess.
        env_list: Optional list of environment variables in VAR=VALUE format.

    Returns:
        Configured StdioTransport instance.
    """
    # Start with the entire environment from the current process - this is appropriate because mcp-compressor is already
    # a stdio MCP proxy itself, so clients have applied any necessary environment filtering already
    env = os.environ.copy()
    # Update with any explicitly provided environment variables
    if env_list:
        for var in env_list:
            key, val = var.split("=", 1)
            env[key] = _interpolate_string(val)
    return StdioTransport(command=command, args=args, env=env, cwd=cwd)


def entrypoint() -> None:
    """Main entrypoint for the mcp-compressor CLI.

    Handles the 'clear-oauth' subcommand manually before delegating to the
    main Typer app, so that 'mcp-compressor <url>' works without a subcommand.
    """
    if len(sys.argv) > 1 and sys.argv[1] == "clear-oauth":
        sys.argv = [sys.argv[0], *sys.argv[2:]]
        clear_oauth_app()
    else:
        app()


if __name__ == "__main__":
    entrypoint()
