"""Tool compression helpers built on FastMCP v3 transforms.

This module provides a transform-first implementation that replaces the visible tool
catalog with a compressed wrapper interface while keeping backend tools available for
passthrough access.
"""

from __future__ import annotations

import asyncio
import json
import re
from collections.abc import Sequence
from typing import Any

import toons
from fastmcp import FastMCP
from fastmcp.exceptions import ToolError
from fastmcp.resources import Resource
from fastmcp.server.context import Context
from fastmcp.server.middleware import CallNext, Middleware, MiddlewareContext
from fastmcp.server.providers.proxy import ProxyTool
from fastmcp.server.transforms import GetResourceNext, GetToolNext
from fastmcp.server.transforms.catalog import CatalogTransform
from fastmcp.tools import Tool, ToolResult
from loguru import logger
from mcp.types import CallToolRequestParams, ContentBlock, TextContent
from pydantic import ValidationError

from .cli_script import find_script_dir
from .cli_tools import build_help_tool_description
from .types import CompressionLevel

# Minimum output length before quiet mode truncation applies
QUIET_MODE_THRESHOLD = 1000


class ToolNotFoundError(ValueError):
    """Exception raised when a requested tool is not found in the backend MCP server."""

    def __init__(self, tool_name: str, available_tools: Sequence[str]) -> None:
        self.tool_name = tool_name
        self.available_tools = tuple(available_tools)
        available_tools_text = ", ".join(self.available_tools) if self.available_tools else "(none)"
        super().__init__(f"Tool '{tool_name}' not found in backend MCP server. Available tools: {available_tools_text}")


class InvokeToolCompatibilityMiddleware(Middleware):
    """Small compatibility shim for flattened invoke_tool arguments and direct toonify."""

    def __init__(self, compressed_tools: CompressedTools) -> None:
        self._compressed_tools = compressed_tools

    async def on_call_tool(
        self,
        context: MiddlewareContext[CallToolRequestParams],
        call_next: CallNext[CallToolRequestParams, ToolResult],
    ) -> ToolResult:
        tool_name = context.message.name
        tool_args = context.message.arguments or {}
        if tool_name in self._compressed_tools.invoke_tool_names and "tool_name" in tool_args:
            tool_input_raw = tool_args.get("tool_input")
            if isinstance(tool_input_raw, dict):
                # Structured call: {tool_name: "foo", tool_input: {...}}
                # tool_input may be an empty dict for zero-argument tools — that is valid.
                tool_input = tool_input_raw
            else:
                # tool_input is None or absent: check for flattened args
                # e.g. {tool_name: "add", "a": 5, "b": 3} (no tool_input wrapper)
                # Exclude meta-keys so they don't leak into the backend tool args.
                flat_input = {k: v for k, v in tool_args.items() if k not in {"tool_name", "quiet", "tool_input"}}
                tool_input = flat_input  # may be empty dict for zero-argument tools

            return await self._compressed_tools.invoke_tool(
                tool_name=tool_args["tool_name"],
                tool_input=tool_input,
                quiet=tool_args.get("quiet", False),
                ctx=context.fastmcp_context,
            )

        result = await call_next(context)
        if self._compressed_tools.should_toonify_tool(tool_name):
            return self._compressed_tools._toonify_tool_result(result)
        return result


class CompressedTools(CatalogTransform):
    """Transform that replaces the tool catalog with compressed wrapper tools.

    In normal mode it exposes two or three public wrapper tools:
    - get_tool_schema: Retrieves the full schema for a specific tool
    - invoke_tool: Executes a tool with the provided arguments
    - list_tools: (optional) Lists all available tools with brief descriptions (only if compression level is MAX)

    It also exposes a resource (``compressor://uncompressed-tools``) that returns the upstream server's original
    list_tools payload in machine-readable JSON form.

    In CLI mode it exposes a single help tool (<server_name>_help) instead of the wrapper tool catalog.
    """

    def __init__(
        self,
        proxy_server: FastMCP,
        compression_level: CompressionLevel,
        server_name: str | None = None,
        toonify: bool = False,
        cli_mode: bool = False,
        cli_name: str | None = None,
        include_tools: Sequence[str] | None = None,
        exclude_tools: Sequence[str] | None = None,
    ) -> None:
        super().__init__()
        self._proxy_server = proxy_server
        self._compression_level = compression_level
        self._tool_name_prefix = f"{server_name}_" if server_name else ""
        self._server_description = f"the {server_name} toolset" if server_name else "this toolset"
        self._toonify = toonify
        self._cli_mode = cli_mode
        self._cli_name = cli_name or (server_name or "mcp")
        self._include_tools = set(include_tools or [])
        self._exclude_tools = set(exclude_tools or [])
        self._cached_backend_tools: dict[str, Tool] | None = None
        self._tool_cache_lock: asyncio.Lock = asyncio.Lock()
        self._help_tool_name = sanitize_tool_name(f"{server_name}_help" if server_name else "help")
        self._get_schema_tool_name = sanitize_tool_name(f"{self._tool_name_prefix}get_tool_schema")
        self._invoke_tool_name = sanitize_tool_name(f"{self._tool_name_prefix}invoke_tool")
        self._invoke_tool_alias_name = sanitize_tool_name("invoke_tool")
        self._list_tools_name = sanitize_tool_name(f"{self._tool_name_prefix}list_tools")
        self._uncompressed_tools_resource_uri = (
            f"compressor://{server_name}/uncompressed-tools" if server_name else "compressor://uncompressed-tools"
        )

    @property
    def invoke_tool_names(self) -> set[str]:
        """All invoke_tool wrapper names, including the hidden alias."""
        return {self._invoke_tool_name, self._invoke_tool_alias_name}

    def should_toonify_tool(self, tool_name: str) -> bool:
        """Return whether direct calls to a tool should be toonified."""
        if not self._toonify:
            return False
        return tool_name not in self._wrapper_tool_names()

    def _wrapper_tool_names(self) -> set[str]:
        if self._cli_mode:
            return {self._help_tool_name}
        tool_names = {self._get_schema_tool_name, self._invoke_tool_name, self._invoke_tool_alias_name}
        if self._compression_level == CompressionLevel.MAX:
            tool_names.add(self._list_tools_name)
        return tool_names

    async def configure_server(self) -> None:
        """Attach the transform and any small compatibility middleware to the server."""
        await self._configure_backend_tool_visibility()
        self._proxy_server.add_transform(self)
        if not self._cli_mode:
            self._proxy_server.add_middleware(InvokeToolCompatibilityMiddleware(self))

    async def _configure_backend_tool_visibility(self) -> None:
        """Apply FastMCP visibility rules for backend tool allow/deny filtering."""
        all_tools = await self._proxy_server.list_tools(run_middleware=False)
        filters_applied = False
        if self._include_tools:
            all_tool_names = {tool.name for tool in all_tools}
            names_to_disable = all_tool_names - self._include_tools
            if names_to_disable:
                self._proxy_server.disable(names=names_to_disable, components={"tool"})
                filters_applied = True
        if self._exclude_tools:
            self._proxy_server.disable(names=self._exclude_tools, components={"tool"})
            filters_applied = True
        # Warm the tool cache after visibility rules are applied so the cache
        # reflects the filtered tool set that clients will actually see.
        # Re-fetch only when filters changed the visible set; otherwise reuse the
        # list we already have (avoids a redundant backend round-trip).
        if filters_applied:
            visible_tools = await self._proxy_server.list_tools(run_middleware=False)
        else:
            visible_tools = all_tools
        self._cached_backend_tools = {tool.name: tool for tool in visible_tools}

    async def transform_tools(self, tools: Sequence[Tool]) -> Sequence[Tool]:
        """Replace the visible tool catalog with compressed wrapper tools."""
        effective_tools: Sequence[Tool] = (
            list(self._cached_backend_tools.values()) if self._cached_backend_tools is not None else tools
        )
        if self._cli_mode:
            return [self._make_help_tool(await self._build_cli_description_from(effective_tools))]

        visible_tools = [
            self._make_get_schema_tool(
                await self._get_tool_descriptions_from(effective_tools, self._compression_level)
            ),
            self._make_invoke_tool(self._invoke_tool_name),
        ]
        if self._compression_level == CompressionLevel.MAX:
            visible_tools.append(self._make_list_tools_tool())
        return visible_tools

    async def get_tool(self, name: str, call_next: GetToolNext, *, version: Any | None = None) -> Tool | None:
        """Return synthetic wrapper tools and delegate backend tool lookups unchanged."""
        if self._cli_mode and name == self._help_tool_name:
            return self._make_help_tool()
        if name == self._get_schema_tool_name:
            return self._make_get_schema_tool()
        if name in self.invoke_tool_names:
            return self._make_invoke_tool(name)
        if name == self._list_tools_name and self._compression_level == CompressionLevel.MAX:
            return self._make_list_tools_tool()
        return await call_next(name, version=version)

    async def transform_resources(self, resources: Sequence[Resource]) -> Sequence[Resource]:
        """Append the synthetic uncompressed-tools resource in normal mode."""
        if self._cli_mode:
            return resources
        return [*resources, self._make_uncompressed_tools_resource()]

    async def get_resource(
        self, uri: str, call_next: GetResourceNext, *, version: Any | None = None
    ) -> Resource | None:
        """Return the synthetic resource when requested, else delegate."""
        if not self._cli_mode and uri == self._uncompressed_tools_resource_uri:
            return self._make_uncompressed_tools_resource()
        return await call_next(uri, version=version)

    async def list_tools_tool(self, ctx: Context | None = None) -> str:
        """List all available tools in {server_description}."""
        if ctx is None:
            async with Context(fastmcp=self._proxy_server) as active_ctx:
                return await self.list_tools_tool(active_ctx)
        backend_tools = await self._get_backend_tools(ctx)
        return await self._get_tool_descriptions_from(list(backend_tools.values()), CompressionLevel.MEDIUM)

    async def get_tool_schema(self, tool_name: str, ctx: Context | None = None) -> str:
        """Get the input schema for a specific tool from {server_description}."""
        if ctx is None:
            async with Context(fastmcp=self._proxy_server) as active_ctx:
                return await self.get_tool_schema(tool_name, active_ctx)
        tool = await self._get_backend_tool(ctx, tool_name)
        tool_description = self._format_tool_description(tool, CompressionLevel.LOW)
        return tool_description + "\n\n" + json.dumps(tool.parameters, indent=2)

    async def invoke_tool(
        self,
        tool_name: str,
        tool_input: dict[str, Any] | None = None,
        quiet: bool = False,
        ctx: Context | None = None,
        toonify: bool | None = None,
    ) -> ToolResult:
        """Invoke a backend tool from the compressed catalog.

        ``toonify`` overrides the instance default for this call (``None`` =
        use the instance setting).
        """
        if ctx is None:
            async with Context(fastmcp=self._proxy_server) as active_ctx:
                return await self.invoke_tool(tool_name, tool_input, quiet, active_ctx, toonify)
        tool = await self._get_backend_tool(ctx, tool_name)
        try:
            if isinstance(tool, ProxyTool):
                tool_result = await tool.run(tool_input or {}, context=ctx)
            else:
                tool_result = await tool.run(tool_input or {})
        except ValidationError as exc:
            raise ToolError(await self._format_validation_error(ctx, tool_name, str(exc))) from exc
        except ToolError as exc:
            if self._is_validation_error_message(str(exc)):
                raise ToolError(await self._format_validation_error(ctx, tool_name, str(exc))) from exc
            raise
        effective_toonify = self._toonify if toonify is None else toonify
        if effective_toonify:
            tool_result = self._toonify_tool_result(tool_result)
        if not quiet:
            return tool_result
        if len(tool_result.content) == 1 and isinstance(tool_result.content[0], TextContent):
            return_text = tool_result.content[0].text
            if len(return_text) < QUIET_MODE_THRESHOLD:
                return tool_result
            preview_length = QUIET_MODE_THRESHOLD // 2
            return_text = (
                return_text[:preview_length]
                + "\n...\n(truncated due to quiet mode)\n...\n"
                + return_text[-preview_length:]
            )
        else:
            return_text = f"Successfully executed tool '{tool.name}' without output."
        return ToolResult(content=[TextContent(type="text", text=return_text)])

    async def list_uncompressed_tools(self, ctx: Context | None = None) -> str:
        """Return the upstream server's original list_tools payload as JSON."""
        if ctx is None:
            async with Context(fastmcp=self._proxy_server) as active_ctx:
                return await self.list_uncompressed_tools(active_ctx)
        backend_tools = await self._get_backend_tools(ctx)
        return json.dumps([tool.to_mcp_tool().model_dump(mode="json") for tool in backend_tools.values()], indent=2)

    async def get_backend_tools(self) -> dict[str, Tool]:
        """Return the current backend tool catalog keyed by name."""
        async with Context(fastmcp=self._proxy_server) as ctx:
            return await self._get_backend_tools(ctx)

    async def get_compression_stats(self) -> dict[str, Any]:
        """Get statistics about the compression of tool descriptions."""
        backend_tools = await self.get_backend_tools()
        original_tool_count = len(backend_tools)
        original_schema_size = sum(
            len(json.dumps(tool.parameters)) + len(json.dumps(tool.output_schema)) + len(tool.description or "")
            for tool in backend_tools.values()
        )
        compressed_schema_sizes: dict[CompressionLevel | str, int] = {}
        for compression_level in [
            CompressionLevel.LOW,
            CompressionLevel.MEDIUM,
            CompressionLevel.HIGH,
            CompressionLevel.MAX,
        ]:
            compressed_schema_sizes[compression_level] = sum(
                len(self._format_tool_description(tool, compression_level)) for tool in backend_tools.values()
            )
        compressed_schema_sizes["cli"] = len(await self._build_cli_description())
        return {
            "original_tool_count": original_tool_count,
            "compressed_tool_count": original_tool_count,
            "original_schema_size": original_schema_size,
            "compressed_schema_sizes": compressed_schema_sizes,
        }

    async def _build_cli_description(self) -> str:
        """Build the full help description for CLI mode."""
        backend_tools = await self.get_backend_tools()
        return await self._build_cli_description_from(list(backend_tools.values()))

    async def _build_cli_description_from(self, tools: Sequence[Tool]) -> str:
        _, on_path = find_script_dir()
        return build_help_tool_description(self._cli_name, self._server_description, list(tools), on_path=on_path)

    async def _get_tool_descriptions_from(self, tools: Sequence[Tool], compression_level: CompressionLevel) -> str:
        """Generate formatted tool descriptions for a set of tools."""
        if compression_level == CompressionLevel.MAX:
            return ""
        return "\n".join(self._format_tool_description(tool, compression_level) for tool in tools)

    async def _get_backend_tools(self, ctx: Context) -> dict[str, Tool]:
        """Retrieve backend tools from cache, fetching from backend on first call.

        The tool catalog is cached on first access (normally at startup via ``configure_server()``) so subsequent
        operations — invoke_tool, get_tool_schema, list_uncompressed_tools, etc. — do not make a live backend call every
        time.  Use ``invalidate_tool_cache()`` to force a refresh if the backend tool catalog changes at runtime.
        """
        if self._cached_backend_tools is not None:
            return self._cached_backend_tools
        async with self._tool_cache_lock:
            # Double-checked locking: another coroutine may have filled the cache
            # while we waited for the lock.
            if self._cached_backend_tools is not None:
                return self._cached_backend_tools
            logger.debug("Tool cache is empty; fetching backend tool catalog.")
            self._cached_backend_tools = {
                tool.name: tool for tool in await self.get_tool_catalog(ctx, run_middleware=False)
            }
        return self._cached_backend_tools

    def invalidate_tool_cache(self) -> None:
        """Invalidate the cached backend tool catalog.

        The next call to any method that needs the backend tool list will
        re-fetch it from the backend server.
        """
        self._cached_backend_tools = None

    async def _get_backend_tool(self, ctx: Context, tool_name: str) -> Tool:
        """Retrieve a specific backend tool from the proxy server."""
        backend_tools = await self._get_backend_tools(ctx)
        tool = backend_tools.get(tool_name)
        if tool is None:
            available_tools = tuple(sorted(backend_tools))
            logger.error(f"Tool '{tool_name}' not found in backend tools. Available tools: {available_tools}")
            raise ToolNotFoundError(tool_name, available_tools)
        return tool

    async def _format_validation_error(self, ctx: Context, tool_name: str, error_message: str) -> str:
        """Format a validation failure with the tool schema for client guidance."""
        tool_schema = await self.get_tool_schema(tool_name, ctx)
        return (
            f"Tool '{tool_name}' input validation failed: {error_message}\n\n"
            f"Here is the result of get_tool_schema('{tool_name}'):\n{tool_schema}"
        )

    def _make_help_tool(self, description: str | None = None) -> Tool:
        async def help_tool() -> str:
            return await self._build_cli_description()

        return Tool.from_function(
            help_tool,
            name=self._help_tool_name,
            description=description or f"Get help for the '{self._cli_name}' CLI. Lists all available subcommands.",
        )

    def _make_get_schema_tool(self, tool_descriptions: str | None = None) -> Tool:
        description = (
            f"Get the input schema for a specific tool from {self._server_description}.\n\n"
            f"Available tools are:\n{tool_descriptions or '{tool_descriptions}'}"
        )
        return Tool.from_function(self.get_tool_schema, name=self._get_schema_tool_name, description=description)

    def _make_invoke_tool(self, tool_name: str) -> Tool:
        description = f"Invoke a tool from {self._server_description}."
        return Tool.from_function(self.invoke_tool, name=tool_name, description=description)

    def _make_list_tools_tool(self) -> Tool:
        description = f"List all available tools in {self._server_description}."
        return Tool.from_function(self.list_tools_tool, name=self._list_tools_name, description=description)

    def _make_uncompressed_tools_resource(self) -> Resource:
        return Resource.from_function(
            self.list_uncompressed_tools,
            uri=self._uncompressed_tools_resource_uri,
            description="The upstream server's original uncompressed tool list as JSON.",
            mime_type="application/json",
        )

    def _format_tool_description(self, tool: Tool, compression_level: CompressionLevel) -> str:
        """Format a single tool's description based on the compression level."""
        tool_name = tool.name
        if compression_level == CompressionLevel.MAX:
            return f"<tool>{tool_name}</tool>"
        tool_arg_names = list(tool.parameters.get("properties", {}))
        tool_description = (tool.description or "").strip()
        if compression_level == CompressionLevel.HIGH:
            tool_description = ""
        elif tool_description and compression_level == CompressionLevel.MEDIUM:
            tool_description = tool_description.splitlines()[0].split(".")[0]
        tool_description = ": " + tool_description if tool_description else ""
        return f"<tool>{tool_name}({', '.join(tool_arg_names)}){tool_description}</tool>"

    def _is_validation_error_message(self, error_message: str) -> bool:
        """Return whether a tool error message appears to be an input validation failure."""
        lowered_message = error_message.lower()
        return "validation error" in lowered_message or "missing required argument" in lowered_message

    def _toonify_tool_result(self, tool_result: ToolResult) -> ToolResult:
        """Convert JSON text content blocks in a tool result to TOON format."""
        converted_content: list[ContentBlock] = []
        content_changed = False
        for content_block in tool_result.content:
            if isinstance(content_block, TextContent):
                converted_text = self._toonify_json_text(content_block.text)
                if converted_text != content_block.text:
                    content_changed = True
                    converted_content.append(TextContent(type="text", text=converted_text))
                    continue
            converted_content.append(content_block)
        if not content_changed:
            return tool_result
        return ToolResult(
            content=converted_content,
            structured_content=tool_result.structured_content,
            meta=tool_result.meta,
        )

    def _toonify_json_text(self, text: str) -> str:
        """Convert a JSON object/array string to TOON; pass through other text unchanged."""
        try:
            parsed = json.loads(text)
        except json.JSONDecodeError:
            return text
        if not isinstance(parsed, dict | list):
            return text
        return toons.dumps(parsed)


def sanitize_tool_name(name: str) -> str:
    """Sanitize a tool name to conform to MCP tool name specifications."""
    sanitized = re.sub(r"[^A-Za-z0-9_\-.]", "_", name).lower()
    if not sanitized:
        raise ValueError("Tool name must contain at least one valid character after sanitization.")
    return sanitized[:128]
