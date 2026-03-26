"""Tool compression middleware for MCP servers.

This module provides the CompressedTools middleware that wraps MCP server tools and compresses their descriptions to
reduce token consumption while maintaining full functionality through a two-step tool invocation pattern.
"""

import json
import re
from collections.abc import Sequence
from typing import Any, cast

import toons
from fastmcp import FastMCP
from fastmcp.exceptions import ToolError
from fastmcp.server.middleware import CallNext, Middleware, MiddlewareContext
from fastmcp.tools import Tool
from fastmcp.tools.tool import ToolResult
from loguru import logger
from mcp.types import CallToolRequestParams, ContentBlock, ListToolsRequest, TextContent
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


class CompressedTools(Middleware):
    """Middleware that compresses MCP tool descriptions to reduce token consumption.

    This middleware wraps an MCP client and exposes its tools through a compressed interface.
    In normal mode it provides two or three public wrapper tools:
    - get_tool_schema: Retrieves the full schema for a specific tool
    - invoke_tool: Executes a tool with the provided arguments
    - list_tools: (optional) Lists all available tools with brief descriptions (only if compression level is MAX)

    It also registers a hidden helper tool for MCP-aware clients that need the upstream server's original
    list_tools payload in machine-readable form.

    In CLI mode it provides a single help tool (<server_name>_help) that lists all CLI subcommands.
    The compression level determines how much information is included in the get_tool_schema tool description.
    """

    def __init__(
        self,
        proxy_server: FastMCP,
        compression_level: CompressionLevel,
        server_name: str | None = None,
        toonify: bool = False,
        cli_mode: bool = False,
        cli_name: str | None = None,
    ) -> None:
        """Initialize the CompressedTools middleware.

        Args:
            proxy_server: The MCP proxy server instance.
            compression_level: The level of compression to apply to tool descriptions.
            server_name: Optional custom name prefix for tool names (will be sanitized and used as prefix).
            toonify: Whether to convert JSON text tool outputs to TOON format.
            cli_mode: Whether to run in CLI mode (exposes a single help tool instead of wrapper tools).
            cli_name: The CLI script name (used in CLI mode for help text).
        """
        self._proxy_server = proxy_server
        self._compression_level = compression_level
        self._tool_name_prefix = f"{server_name}_" if server_name else ""
        self._server_description = f"the {server_name} toolset" if server_name else "this toolset"
        self._toonify = toonify
        self._cli_mode = cli_mode
        self._cli_name = cli_name or (server_name or "mcp")
        self._help_tool_name = sanitize_tool_name(f"{server_name}_help" if server_name else "help")
        self._get_schema_tool_name = sanitize_tool_name(f"{self._tool_name_prefix}get_tool_schema")
        self._invoke_tool_name = sanitize_tool_name(f"{self._tool_name_prefix}invoke_tool")
        self._invoke_tool_alias_name = sanitize_tool_name("invoke_tool")
        self._list_tools_name = sanitize_tool_name(f"{self._tool_name_prefix}list_tools")
        self._hidden_schema_tool_name = sanitize_tool_name("list_uncompressed_tools")
        self._hidden_tool_names = {self._hidden_schema_tool_name}
        self._built_in_tool_names = {self._get_schema_tool_name, self._invoke_tool_name, self._hidden_schema_tool_name}
        if self._invoke_tool_alias_name != self._invoke_tool_name:
            self._built_in_tool_names.add(self._invoke_tool_alias_name)
            self._hidden_tool_names.add(self._invoke_tool_alias_name)
        if self._compression_level == CompressionLevel.MAX:
            self._built_in_tool_names.add(self._list_tools_name)
        self._all_tools: dict[str, Tool] | None = None

    async def list_tools(self) -> str:
        """List all available tools in {server_description}.

        Returns:
            A formatted string listing tool names and brief descriptions.
        """
        return await self._get_tool_descriptions(CompressionLevel.MEDIUM)

    async def get_tool_schema(self, tool_name: str) -> str:
        """Get the input schema for a specific tool from {server_description}.

        Available tools are:
        {tool_descriptions}

        Args:
            tool_name: The name of the tool to get the schema for.

        Returns:
            The input schema for the specified tool.

        Raises:
            ValueError: If the tool name is not found in the server.
        """
        tool = await self._get_backend_tool(tool_name)
        tool_description = self._format_tool_description(tool, CompressionLevel.LOW)
        return tool_description + "\n\n" + json.dumps(tool.parameters, indent=2)

    async def invoke_tool(self, tool_name: str, tool_input: dict[str, Any] | None = None, quiet: bool = False) -> Any:
        """Invoke a tool from {server_description}.

        Args:
            tool_name: The name of the tool to invoke.
            tool_input: The input to the tool. Schemas can be retrieved using the appropriate `get_tool_schema`
                function.
            quiet: If true, truncates large tool outputs for successful invocations. This is useful for reducing token
                consumption when the output is not needed. Full responses will always be returned for tool errors.

        Returns:
            The output from the tool.
        """
        tool = await self._get_backend_tool(tool_name)
        try:
            tool_result = await tool.run(tool_input or {})
        except ValidationError as exc:
            raise ToolError(await self._format_validation_error(tool_name, str(exc))) from exc
        except ToolError as exc:
            if self._is_validation_error_message(str(exc)):
                raise ToolError(await self._format_validation_error(tool_name, str(exc))) from exc
            raise
        if self._toonify:
            tool_result = self._toonify_tool_result(tool_result)
        if quiet:
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
        return tool_result

    async def list_uncompressed_tools(self) -> str:
        """Return the upstream server's original list_tools payload as JSON.

        This hidden helper is intended for MCP-aware clients that need the backend server's uncompressed tool
        inventory, including the same descriptions and schemas exposed by the upstream list_tools endpoint.

        Returns:
            A JSON array matching the upstream server's list_tools response.
        """
        tools = []
        for tool in (await self._get_backend_tools()).values():
            tools.append({
                "name": tool.name,
                "title": tool.title,
                "description": tool.description,
                "inputSchema": tool.parameters,
                "outputSchema": tool.output_schema,
                "icons": tool.icons,
                "annotations": tool.annotations,
                "meta": tool.meta,
                "execution": tool.execution,
            })
        return json.dumps(tools, indent=2)

    async def on_call_tool(
        self,
        context: MiddlewareContext[CallToolRequestParams],
        call_next: CallNext[CallToolRequestParams, ToolResult],
    ) -> ToolResult:
        """Middleware to clean up tool invocation arguments to invoke_tool and route to the underlying tool.

        Args:
            context: The middleware context containing the call request.
            call_next: The next middleware or handler in the chain.

        Returns:
            The result from calling the underlying tool.
        """
        tool_args = context.message.arguments
        if not context.message.name.endswith("invoke_tool") or not tool_args:
            result = await call_next(context)
            if self._toonify and not self._is_built_in_tool_name(context.message.name):
                return self._toonify_tool_result(result)
            return result

        if "tool_input" not in tool_args or tool_args["tool_input"] is None:
            tool_input = {k: v for k, v in tool_args.items() if k not in ["tool_name", "quiet"]}
        else:
            tool_input = tool_args["tool_input"]
        return await self.invoke_tool(
            tool_name=tool_args["tool_name"],
            tool_input=tool_input,
            quiet=tool_args.get("quiet", False),
        )

    async def on_list_tools(
        self, context: MiddlewareContext[ListToolsRequest], call_next: CallNext[ListToolsRequest, Sequence[Tool]]
    ) -> Sequence[Tool]:
        """Middleware to inject compressed tool descriptions and suppress backend tools.

        In normal mode, updates get_tool_schema's description with the tool list.
        In CLI mode, updates the help tool's description with the full CLI help text.

        Returns:
            The sequence of built-in wrapper tools with updated descriptions.
        """
        built_in_tools = [
            tool for tool in (await self._get_built_in_tools()).values() if not self._is_hidden_tool_name(tool.name)
        ]
        if self._cli_mode:
            description = await self._build_cli_description()
            for tool in built_in_tools:
                tool.description = description
            return built_in_tools
        prepared_tools = []
        for tool in built_in_tools:
            logger.info(f"Preparing tool: {tool.name}")
            prepared_tools.append(tool)
            tool.description = cast(str, tool.description).format(
                tool_descriptions=await self._get_tool_descriptions(self._compression_level),
                server_description=self._server_description,
            )
        return prepared_tools

    async def configure_server(self) -> None:
        """Configure an MCP server with compressed tool wrappers.

        In normal mode, registers get_tool_schema, invoke_tool, and optionally list_tools.
        In CLI mode, registers a single <server_name>_help tool.
        """
        if self._cli_mode:

            async def help_tool() -> str:
                return await self._build_cli_description()

            help_tool.__doc__ = f"Get help for the '{self._cli_name}' CLI. Lists all available subcommands."
            self._proxy_server.tool(name=self._help_tool_name)(help_tool)
        else:
            # Create tool names with optional server name prefix
            self._proxy_server.tool(name=self._get_schema_tool_name)(self.get_tool_schema)
            self._proxy_server.tool(name=self._invoke_tool_name)(self.invoke_tool)
            if self._invoke_tool_alias_name != self._invoke_tool_name:
                self._proxy_server.tool(name=self._invoke_tool_alias_name)(self.invoke_tool)
            self._proxy_server.tool(name=self._hidden_schema_tool_name)(self.list_uncompressed_tools)
            if self._compression_level == CompressionLevel.MAX:
                self._proxy_server.tool(name=self._list_tools_name)(self.list_tools)
        self._proxy_server.add_middleware(self)
        self._all_tools = None  # Reset cached tools, if any

    async def get_compression_stats(self) -> dict[str, Any]:
        """Get statistics about the compression of tool descriptions.

        Computes the original backend schema size vs the compressed proxy tool size
        for all compression levels. Works identically in both normal and CLI mode —
        the only difference is which tools _get_built_in_tools() returns.

        Returns:
            A dictionary containing statistics about the original and compressed tool description sizes.
        """
        backend_tools = await self._get_backend_tools()
        original_tool_count = len(backend_tools)
        original_schema_size = sum(
            len(json.dumps(tool.parameters)) + len(json.dumps(tool.output_schema)) + len(tool.description or "")
            for tool in backend_tools.values()
        )
        # Low/Medium/High/Max: always measured from the backend tools compressed at each level.
        # This is what a non-CLI agent would see in the get_tool_schema description,
        # and it gives meaningful differentiation between levels in both modes.
        compressed_tool_count = len(backend_tools)
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

        # "cli" key: the help tool description — what the agent sees in CLI mode.
        compressed_schema_sizes["cli"] = len(await self._build_cli_description())
        return {
            "original_tool_count": original_tool_count,
            "compressed_tool_count": compressed_tool_count,
            "original_schema_size": original_schema_size,
            "compressed_schema_sizes": compressed_schema_sizes,
        }

    async def _build_cli_description(self) -> str:
        """Build the full help description for CLI mode — same content as the CLI --help output."""
        _, on_path = find_script_dir()
        tools = list((await self._get_backend_tools()).values())
        return build_help_tool_description(self._cli_name, self._server_description, tools, on_path=on_path)

    async def _get_tool_descriptions(self, compression_level: CompressionLevel) -> str:
        """Generate a formatted string of tool descriptions at the specified compression level.

        Args:
            compression_level: The compression level to use for formatting.

        Returns:
            A newline-separated string of formatted tool descriptions.
        """
        if compression_level == CompressionLevel.MAX:
            return ""
        tool_descriptions = []
        for tool in (await self._get_backend_tools()).values():
            tool_descriptions.append(self._format_tool_description(tool, compression_level))
        return "\n".join(tool_descriptions)

    def _format_tool_description(self, tool: Tool, compression_level: CompressionLevel) -> str:
        """Format a single tool's description based on the compression level.

        Args:
            tool: The tool to format.
            compression_level: The compression level determining how much detail to include.

        Returns:
            A formatted string representation of the tool in the format:
            <tool>tool_name(param1, param2): description</tool>
        """
        tool_name = tool.name
        if compression_level == CompressionLevel.MAX:
            # Maximum compression: tool name only, no args, no description
            return f"<tool>{tool_name}</tool>"
        tool_arg_names = list(tool.parameters.get("properties", {}))
        tool_description = (tool.description or "").strip()
        if compression_level == CompressionLevel.HIGH:
            tool_description = ""
        elif tool_description and compression_level == CompressionLevel.MEDIUM:
            tool_description = tool_description.splitlines()[0].split(".")[0]
        tool_description = ": " + tool_description if tool_description else ""
        return f"<tool>{tool_name}({', '.join(tool_arg_names)}){tool_description}</tool>"

    def _is_built_in_tool(self, tool: Tool) -> bool:
        """Check if a tool is one of the built-in wrapper tools.

        Args:
            tool: The tool to check.

        Returns:
            True if the tool is a built-in wrapper tool, False otherwise.
        """
        return self._is_built_in_tool_name(tool.name)

    def _is_built_in_tool_name(self, tool_name: str) -> bool:
        """Check if a tool name refers to one of the built-in wrapper tools."""
        if self._cli_mode:
            return tool_name == self._help_tool_name
        return tool_name in self._built_in_tool_names

    def _is_hidden_tool_name(self, tool_name: str) -> bool:
        """Check if a built-in tool should be omitted from list_tools responses."""
        return tool_name in self._hidden_tool_names

    async def _get_backend_tools(self) -> dict[str, Tool]:
        """Retrieve backend tools from the proxy server, caching the result."""
        if self._all_tools is None:
            self._all_tools = await self._proxy_server.get_tools()
        return {name: tool for name, tool in self._all_tools.items() if not self._is_built_in_tool(tool)}

    async def _get_built_in_tools(self) -> dict[str, Tool]:
        """Retrieve built-in wrapper tools from the proxy server, caching the result."""
        if self._all_tools is None:
            self._all_tools = await self._proxy_server.get_tools()
        return {name: tool for name, tool in self._all_tools.items() if self._is_built_in_tool(tool)}

    async def _get_backend_tool(self, tool_name: str) -> Tool:
        """Retrieve a specific backend tool from the proxy server."""
        backend_tools = await self._get_backend_tools()
        tool = backend_tools.get(tool_name)
        if tool is None:
            available_tools = tuple(sorted(backend_tools))
            logger.error(f"Tool '{tool_name}' not found in backend tools. Available tools: {available_tools}")
            raise ToolNotFoundError(tool_name, available_tools)
        return tool

    async def _format_validation_error(self, tool_name: str, error_message: str) -> str:
        """Format a validation failure with the tool schema for client guidance."""
        tool_schema = await self.get_tool_schema(tool_name)
        return (
            f"Tool '{tool_name}' input validation failed: {error_message}\n\n"
            f"Here is the result of get_tool_schema('{tool_name}'):\n{tool_schema}"
        )

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
    """Sanitize a tool name to conform to MCP tool name specifications.

    Tool names must:
    - Be between 1 and 128 characters (inclusive)
    - Only contain: A-Z, a-z, 0-9, underscore (_), hyphen (-), and dot (.)
    - Not contain spaces, commas, or other special characters

    Args:
        name: The raw tool name to sanitize.

    Returns:
        A sanitized tool name conforming to MCP specifications.
    """
    # Replace spaces and invalid characters with underscores
    sanitized = re.sub(r"[^A-Za-z0-9_\-.]", "_", name).lower()

    # Ensure the name is not empty after sanitization
    if not sanitized:
        raise ValueError("Tool name must contain at least one valid character after sanitization.")  # noqa: TRY003

    # Truncate to 128 characters if needed
    return sanitized[:128]
