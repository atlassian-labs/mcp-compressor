"""Main entry point for the MCP Compressor CLI.

This module provides the CLI interface for running the MCP Compressor proxy server, which wraps existing MCP servers and
compresses their tool descriptions to reduce token consumption.
"""

import asyncio
import os
import socket
import sys
from collections.abc import AsyncGenerator
from contextlib import asynccontextmanager
from typing import Annotated, Any, Literal, overload

import anyio
import typer
from fastmcp import FastMCP
from fastmcp.client.transports import (
    SSETransport,
    StdioTransport,
    StreamableHttpTransport,
    infer_transport_type_from_url,
)
from fastmcp.exceptions import ToolError
from fastmcp.server.proxy import ProxyClient, ProxyTool
from fastmcp.tools.tool import ToolResult
from loguru import logger
from loguru_logging_intercept import setup_loguru_logging_intercept
from mcp.types import TextContent

from .banner import print_banner
from .cli_bridge import CliBridge
from .cli_script import generate_cli_script
from .cli_tools import sanitize_cli_name
from .tools import CliModeTools, CompressedTools
from .types import CompressionLevel, LogLevel, TransportType

app = typer.Typer(name="MCP Compressor", help="An MCP server wrapper for reducing tokens consumed by MCP tools.")


@app.command()
def main(
    command_or_url_list: Annotated[
        list[str],
        typer.Argument(
            ...,
            metavar="COMMAND_OR_URL",
            help=(
                "The URL of the MCP server to connect to for streamable HTTP or SSE servers, or the command and "
                "arguments to run for stdio servers. Example: uvx mcp-server-fetch"
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
    ] = LogLevel.WARNING,
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
    cli_port: Annotated[
        int | None,
        typer.Option(
            ...,
            "--cli-port",
            help="Port for the local CLI bridge HTTP server (default: random free port).",
        ),
    ] = None,
):
    """Run the MCP Compressor proxy server.

    This is the main entry point for the CLI application. It connects to an MCP server
    (via stdio, HTTP, or SSE) and wraps it with a compressed tool interface.
    """
    logger.remove()
    logger.add(sys.stderr, level=log_level.value.upper())
    setup_loguru_logging_intercept(modules=("fastmcp",))

    asyncio.run(
        _async_main(
            command_or_url_list=command_or_url_list,
            cwd=cwd,
            env_list=env_list,
            header_list=header_list,
            timeout=timeout,
            compression_level=compression_level,
            server_name=server_name,
            log_level=log_level,
            toonify=toonify or cli_mode,
            cli_mode=cli_mode,
            cli_port=cli_port,
        )
    )


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
    cli_port: int | None = None,
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
        cli_port=cli_port,
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
    cli_port: int | None = None,
) -> AsyncGenerator[FastMCP, None]:
    if compression_level == CompressionLevel.MAX and server_name is None and not cli_mode:
        raise ValueError("server_name must be provided when using MAX compression level")  # noqa: TRY003

    command_or_url = " ".join(command_or_url_list)
    transport_type = infer_transport_type_from_url(command_or_url) if command_or_url.startswith("http") else "stdio"
    logger.info(f"Inferred transport type: {transport_type}")

    # Handle different transport types
    transport: TransportType
    if transport_type == "stdio":
        transport = _get_stdio_transport(
            command=command_or_url_list[0], args=command_or_url_list[1:], cwd=cwd, env_list=env_list
        )
    elif transport_type == "http":
        transport = _get_streamable_http_transport(url=command_or_url, header_list=header_list, timeout=timeout)
    elif transport_type == "sse":
        transport = _get_sse_transport(url=command_or_url, header_list=header_list, timeout=timeout)

    if cli_mode:
        cli_name = sanitize_cli_name(server_name or "mcp")
        async with _cli_mode_server(
            transport=transport,
            cli_name=cli_name,
            compression_level=compression_level,
            server_name=server_name,
            toonify=toonify,
            cli_port=cli_port,
        ) as mcp:
            yield mcp
        return

    logger.info("Initializing proxy server")
    mcp = FastMCP.as_proxy(backend=transport, name="MCP Compressor Proxy", version="0.1.0")

    # Shared compressed tools for backend access
    compressed_tools = CompressedTools(
        mcp, compression_level=compression_level, server_name=server_name, toonify=toonify
    )

    logger.info("Configuring compressed tools middleware")
    await compressed_tools.configure_server()

    if transport_type == "stdio":
        stats = await compressed_tools.get_compression_stats()
        print_banner(server_name, transport_type, stats, compression_level)
    else:
        logger.info(
            "Skipping startup-time backend inspection for remote %s transport; tool discovery/auth will happen lazily",
            transport_type,
        )

    yield mcp


@asynccontextmanager
async def _cli_mode_server(
    transport: TransportType,
    cli_name: str,
    compression_level: CompressionLevel,
    server_name: str | None,
    toonify: bool,
    cli_port: int | None,
) -> AsyncGenerator[FastMCP, None]:
    """Set up and run the CLI mode server with a persistent authenticated backend session.

    Connects once to the backend, starts a local HTTP bridge, generates a CLI
    script, and yields the MCP server for the caller to run.
    """
    async with ProxyClient(transport=transport, init_timeout=None) as persistent_client:
        logger.info("Initializing proxy server with persistent client for CLI mode")
        mcp = FastMCP.as_proxy(backend=persistent_client, name="MCP Compressor Proxy", version="0.1.0")

        compressed_tools = CompressedTools(
            mcp, compression_level=compression_level, server_name=server_name, toonify=toonify
        )
        cli_mode_tools = CliModeTools(
            proxy_server=mcp,
            cli_name=cli_name,
            server_name=server_name,
            compressed_tools=compressed_tools,
        )
        await cli_mode_tools.configure_server()

        port = cli_port or _find_free_port()

        async def _get_tools() -> dict[str, Any]:
            mcp_tools = await persistent_client.list_tools()
            return {t.name: ProxyTool.from_mcp_tool(persistent_client, t) for t in mcp_tools}

        async def _invoke(tool_name: str, tool_input: dict[str, Any] | None) -> ToolResult:
            result = await persistent_client.call_tool_mcp(name=tool_name, arguments=tool_input or {})
            if result.isError:
                error_text = (
                    result.content[0].text
                    if result.content and isinstance(result.content[0], TextContent)
                    else str(result.content)
                )
                raise ToolError(error_text)
            tool_result = ToolResult(
                content=result.content,
                structured_content=result.structuredContent,
                meta=result.meta,
            )
            if toonify:
                tool_result = compressed_tools._toonify_tool_result(tool_result)
            return tool_result

        bridge = CliBridge(
            cli_name=cli_name,
            server_description=compressed_tools._server_description,
            get_tools_fn=_get_tools,
            invoke_fn=_invoke,
            port=port,
        )
        bridge_server = bridge.make_server()

        async with anyio.create_task_group() as tg:
            tg.start_soon(bridge_server.serve)
            while not bridge_server.started:
                await anyio.sleep(0.01)

            script_path, on_path = generate_cli_script(cli_name=cli_name, bridge_port=port)
            invoke_prefix = cli_name if on_path else f"./{cli_name}"
            print("CLI mode active.")
            print(f"  Script:  {script_path}")
            print(f"  Bridge:  http://127.0.0.1:{port}")
            print(f"  Run:     {invoke_prefix} --help")

            logger.info("Starting MCP Compressor server in CLI mode")
            yield mcp
            tg.cancel_scope.cancel()


def _find_free_port() -> int:
    """Find a free port on the loopback interface."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


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
    if transport_type == "http":
        return StreamableHttpTransport(url=url, headers=header_dict, auth="oauth")
    return SSETransport(url=url, headers=header_dict, auth="oauth", sse_read_timeout=timeout)


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
    env = {}
    if env_list:
        for var in env_list:
            key, val = var.split("=", 1)
            env[key] = _interpolate_string(val)
    return StdioTransport(command=command, args=args, env=env, cwd=cwd)


if __name__ == "__main__":
    app()
