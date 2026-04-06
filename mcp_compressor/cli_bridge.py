"""Local HTTP bridge server for CLI mode.

Exposes two endpoints:
  GET  /health  - liveness check
  POST /exec    - receive CLI argv and invoke the corresponding backend MCP tool
"""

from __future__ import annotations

from collections.abc import Coroutine
from typing import TYPE_CHECKING, Any, Callable

import uvicorn
from fastmcp.server.context import Context
from fastmcp.tools import Tool
from loguru import logger
from mcp.types import TextContent
from starlette.applications import Starlette
from starlette.requests import Request
from starlette.responses import PlainTextResponse, Response
from starlette.routing import Route

from .cli_tools import format_tool_help, format_top_level_help, parse_argv_to_tool_input, tool_name_to_subcommand

if TYPE_CHECKING:
    from fastmcp import FastMCP

# Type alias for the direct backend call callable — receives tool_name + arguments + quiet flag,
# returns a ToolResult (not a CallToolResult)
InvokeFn = Callable[[str, dict[str, Any] | None, bool], Coroutine[Any, Any, Any]]
# Type alias for the lazy tool fetch callable
GetToolsFn = Callable[[], Coroutine[Any, Any, dict[str, Tool]]]


class CliBridge:
    """Local HTTP bridge that routes CLI argv to backend MCP tools.

    Tools are fetched lazily on the first request so that the bridge works
    correctly with remote backends that require OAuth or other deferred auth.
    """

    def __init__(
        self,
        cli_name: str,
        server_description: str,
        get_tools_fn: GetToolsFn,
        invoke_fn: InvokeFn,
        port: int,
        fastmcp: FastMCP | None = None,
    ) -> None:
        self._cli_name = cli_name
        self._server_description = server_description
        self._get_tools_fn = get_tools_fn
        self._invoke_fn = invoke_fn
        self._port = port
        self._fastmcp = fastmcp

        # Lazily populated on first request
        self._tools: dict[str, Tool] | None = None
        self._subcommand_map: dict[str, str] = {}

        self._app = Starlette(
            routes=[
                Route("/health", self._health, methods=["GET"]),
                Route("/exec", self._exec, methods=["POST"]),
            ]
        )

    async def _ensure_tools(self) -> None:
        """Fetch backend tools on first use, excluding the CLI help tool itself."""
        if self._tools is None:
            all_tools = await self._get_tools_fn()
            # Exclude the <cli_name>_help wrapper tool from the CLI subcommand surface
            help_tool_subcommand = tool_name_to_subcommand(f"{self._cli_name}_help")
            self._tools = {
                name: tool for name, tool in all_tools.items() if tool_name_to_subcommand(name) != help_tool_subcommand
            }
            self._subcommand_map = {tool_name_to_subcommand(name): name for name in self._tools}

    @property
    def app(self) -> Starlette:
        return self._app

    async def _health(self, request: Request) -> Response:
        return PlainTextResponse("ok")

    async def _exec(self, request: Request) -> Response:
        try:
            body = await request.json()
        except Exception:
            return PlainTextResponse("error: invalid JSON body\n", status_code=400)

        argv: list[str] = body.get("argv", [])
        logger.debug("CLI exec argv={}", argv)

        # Ensure tools are loaded (lazy, handles deferred auth)
        try:
            await self._ensure_tools()
        except Exception as exc:
            return PlainTextResponse(f"error: could not load backend tools: {exc}\n", status_code=400)

        # top-level --help or no args
        if not argv or argv in (["--help"], ["-h"]):
            return self._top_level_help()

        subcommand = argv[0]
        rest = argv[1:]

        # subcommand --help
        if rest in (["--help"], ["-h"]):
            return self._subcommand_help(subcommand)

        return await self._invoke_subcommand(subcommand, rest)

    # -- helpers to keep _exec below the complexity threshold --

    def _top_level_help(self) -> Response:
        """Return the top-level --help response."""
        assert self._tools is not None  # noqa: S101
        help_text = format_top_level_help(self._cli_name, self._server_description, list(self._tools.values()))
        return PlainTextResponse(help_text + "\n")

    def _subcommand_help(self, subcommand: str) -> Response:
        """Return per-subcommand --help or an error if the subcommand is unknown."""
        assert self._tools is not None  # noqa: S101
        tool_name = self._subcommand_map.get(subcommand)
        if tool_name is None:
            return PlainTextResponse(f"error: unknown subcommand '{subcommand}'\n", status_code=400)
        return PlainTextResponse(format_tool_help(self._cli_name, self._tools[tool_name]) + "\n")

    async def _invoke_subcommand(self, subcommand: str, rest: list[str]) -> Response:
        """Resolve, parse, invoke, and format a subcommand call."""
        assert self._tools is not None  # noqa: S101
        tool_name = self._subcommand_map.get(subcommand)
        if tool_name is None:
            available = ", ".join(sorted(self._subcommand_map))
            return PlainTextResponse(
                f"error: unknown subcommand '{subcommand}'\n\n"
                f"Available subcommands: {available}\n"
                f"Run './{self._cli_name} --help' for usage.\n",
                status_code=400,
            )

        tool = self._tools[tool_name]

        # Extract --quiet before passing argv to the tool-schema-driven parser,
        # since quiet is a universal flag not present in any individual tool schema.
        quiet = "--quiet" in rest
        rest = [arg for arg in rest if arg != "--quiet"]

        try:
            tool_input = parse_argv_to_tool_input(rest, tool)
        except ValueError as exc:
            return PlainTextResponse(
                f"error: {exc}\n\n" + format_tool_help(self._cli_name, tool) + "\n",
                status_code=400,
            )

        try:
            if self._fastmcp is not None:
                # Establish a FastMCP Context so that proxy tools (which call
                # get_context()) can access the server state.  Without this,
                # stateful backends like chrome-devtools-mcp fail with
                # "No active context found".
                async with Context(fastmcp=self._fastmcp):
                    result = await self._invoke_fn(tool_name, tool_input, quiet)
            else:
                result = await self._invoke_fn(tool_name, tool_input, quiet)
        except Exception as exc:
            return PlainTextResponse(f"error: {exc}\n", status_code=400)

        return self._format_result(result)

    @staticmethod
    def _format_result(result: Any) -> Response:
        """Convert a ToolResult into a plain-text response."""
        output_parts: list[str] = []
        for content_block in result.content:
            if isinstance(content_block, TextContent):
                output_parts.append(content_block.text)
            else:
                output_parts.append(f"[{type(content_block).__name__} content]")
        output = "\n".join(output_parts)
        return PlainTextResponse(output + "\n" if not output.endswith("\n") else output)

    def make_server(self) -> uvicorn.Server:
        """Create a uvicorn Server instance bound to the loopback interface."""
        config = uvicorn.Config(
            app=self._app,
            host="127.0.0.1",
            port=self._port,
            log_level="warning",
            # Disable the ASGI lifespan protocol — the bridge app has no startup/shutdown hooks, and when the enclosing
            # task group is cancelled on exit, Starlette's lifespan `await receive()` would otherwise surface a noisy
            # CancelledError traceback.
            lifespan="off",
        )
        server = uvicorn.Server(config)
        # Disable uvicorn's signal handlers — anyio and the MCP server manage
        # SIGINT/SIGTERM for the process.  Without this, uvicorn's signal thread
        # blocks interpreter shutdown and requires multiple Ctrl+C presses.
        server.install_signal_handlers = lambda: None  # type: ignore[method-assign]
        return server
