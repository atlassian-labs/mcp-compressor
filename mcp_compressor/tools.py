"""Tool compression middleware for MCP servers.

This module provides the CompressedTools middleware that wraps MCP server tools and compresses their descriptions to
reduce token consumption while maintaining full functionality through a two-step tool invocation pattern.
"""

import json
import re
from collections.abc import Sequence
from typing import Any, cast

from fastmcp import Client, FastMCP
from fastmcp.server.middleware import CallNext, Middleware, MiddlewareContext
from fastmcp.tools.tool import Tool as _Tool
from fastmcp.tools.tool import ToolResult
from mcp.types import CallToolRequestParams, ListToolsRequest, Tool

from .types import CompressionLevel, ToolResultBlock


class CompressedTools(Middleware):
    """Middleware that compresses MCP tool descriptions to reduce token consumption.

    This middleware wraps an MCP client and exposes its tools through a compressed interface.
    Instead of exposing tools directly with full schemas, it provides two or three wrapper tools:
    - get_tool_schema: Retrieves the full schema for a specific tool
    - invoke_tool: Executes a tool with the provided arguments
    - list_tools: (optional) Lists all available tools with brief descriptions (only if compression level is MAX)

    The compression level determines how much information is included in the get_tool_schema tool description.
    """

    def __init__(self, client: Client, compression_level: CompressionLevel, server_name: str | None = None) -> None:
        """Initialize the CompressedTools middleware.

        Args:
            client: The MCP client connected to the underlying server.
            compression_level: The level of compression to apply to tool descriptions.
            server_name: Optional custom name prefix for tool names (will be sanitized and used as prefix).
        """
        self._client = client
        self._compression_level = compression_level
        self._tool_name_prefix = f"{server_name}_" if server_name else ""
        self._server_description = f"the {server_name} toolset" if server_name else "this toolset"
        self._tools: list[Tool] | None = None

    @property
    def tools(self) -> list[Tool]:
        """Get the list of tools from the underlying MCP server.

        Returns:
            List of tools available in the wrapped server.

        Raises:
            ValueError: If tools have not been initialized yet.
        """
        if self._tools is None:
            raise ValueError("Tools have not been initialized. Call 'initialize' first.")  # noqa: TRY003
        return self._tools

    async def initialize(self) -> None:
        """Initialize the tools by fetching them from the client."""
        self._tools = await self._client.list_tools()

    async def list_tools(self) -> str:
        """List all available tools in {server_description}.

        Returns:
            A formatted string listing tool names and brief descriptions.
        """
        return self._get_tool_descriptions(CompressionLevel.MEDIUM)

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
        for tool in self.tools:
            if tool.name == tool_name:
                break
        else:
            raise ValueError(f"{tool_name} not found in MCP server.")  # noqa: TRY003

        tool_description = self._format_tool_description(tool, CompressionLevel.LOW)
        return tool_description + "\n\n" + json.dumps(tool.inputSchema, indent=2)

    async def invoke_tool(self, tool_name: str, tool_input: dict[str, Any] | None = None) -> list[ToolResultBlock]:
        """Invoke a tool from {server_description}.

        Args:
            tool_name: The name of the tool to invoke.
            tool_input: The input to the tool. Schemas can be retrieved using the appropriate `get_tool_schema`
                function.

        Returns:
            The output from the tool.
        """
        return []

    async def on_call_tool(
        self,
        context: MiddlewareContext[CallToolRequestParams],
        call_next: CallNext[CallToolRequestParams, ToolResult],
    ) -> ToolResult:
        """Middleware to route tool calls to the underlying MCP server.

        This intercepts calls to the invoke_tool wrapper and forwards them to the actual tool in the wrapped MCP server.

        Args:
            context: The middleware context containing the call request.
            call_next: The next middleware or handler in the chain.

        Returns:
            The result from calling the underlying tool.
        """
        wrapper_tool_args = context.message.arguments
        if not context.message.name.endswith("invoke_tool") or not wrapper_tool_args:
            return await call_next(context)

        if "tool_input" not in wrapper_tool_args or not wrapper_tool_args["tool_input"]:
            tool_args = {k: v for k, v in wrapper_tool_args.items() if k != "tool_name"}
        else:
            tool_args = wrapper_tool_args["tool_input"]
        tool_name = wrapper_tool_args["tool_name"]

        tool_result = await self._client.call_tool(tool_name, tool_args)
        return ToolResult(
            content=tool_result.content,
            structured_content=tool_result.structured_content,
            meta=tool_result.meta,
        )

    async def on_list_tools(
        self, context: MiddlewareContext[ListToolsRequest], call_next: CallNext[ListToolsRequest, Sequence[_Tool]]
    ) -> Sequence[_Tool]:
        """Middleware to inject compressed tool descriptions into the get_tool_schema tool.

        This updates the get_tool_schema tool's description to include the list of available tools at the appropriate
        compression level.

        Args:
            context: The middleware context for the list tools request.
            call_next: The next middleware or handler in the chain.

        Returns:
            The sequence of tools with updated descriptions.
        """
        tools = await call_next(context)
        for tool in tools:
            tool.description = cast(str, tool.description)
            tool.description = tool.description.format(
                tool_descriptions=self._get_tool_descriptions(self._compression_level),
                server_description=self._server_description,
            )
        return tools

    async def configure_server(self, server: FastMCP) -> None:
        """Configure an MCP server with compressed tool wrappers.

        This initializes the tools from the underlying server and registers the wrapper
        tools (get_tool_schema, invoke_tool, and optionally list_tools) on the provided server.

        Args:
            server: The MCP server to configure with compressed tools.
        """
        await self.initialize()

        # Create tool names with optional server name prefix
        get_schema_name = sanitize_tool_name(f"{self._tool_name_prefix}get_tool_schema")
        invoke_tool_name = sanitize_tool_name(f"{self._tool_name_prefix}invoke_tool")
        list_tools_name = sanitize_tool_name(f"{self._tool_name_prefix}list_tools")

        server.tool(name=get_schema_name)(self.get_tool_schema)
        server.tool(name=invoke_tool_name)(self.invoke_tool)
        if self._compression_level == CompressionLevel.MAX:
            server.tool(name=list_tools_name)(self.list_tools)
        server.add_middleware(self)

    def _get_tool_descriptions(self, compression_level: CompressionLevel) -> str:
        """Generate a formatted string of tool descriptions at the specified compression level.

        Args:
            compression_level: The compression level to use for formatting.

        Returns:
            A newline-separated string of formatted tool descriptions.
        """
        if compression_level == CompressionLevel.MAX:
            return ""
        tool_descriptions = []
        for tool in self.tools:
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
        tool_arg_names = list(tool.inputSchema.get("properties", {}))
        tool_description = (tool.description or "").strip()
        if compression_level == CompressionLevel.HIGH:
            tool_description = ""
        elif tool_description and compression_level == CompressionLevel.MEDIUM:
            tool_description = tool_description.splitlines()[0].split(".")[0]
        tool_description = ": " + tool_description if tool_description else ""
        return f"<tool>{tool_name}({', '.join(tool_arg_names)}){tool_description}</tool>"


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
    sanitized = re.sub(r"[^A-Za-z0-9_\-.]", "_", name)

    # Ensure the name is not empty after sanitization
    if not sanitized:
        raise ValueError("Tool name must contain at least one valid character after sanitization.")  # noqa: TRY003

    # Truncate to 128 characters if needed
    return sanitized[:128]
