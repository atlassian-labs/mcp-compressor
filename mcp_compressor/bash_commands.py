"""just-bash integration — converts MCP backend tools to just-bash custom commands.

Each backend server becomes a single parent command (e.g. ``alpha``) whose subcommands
map to the server's MCP tools.  Argument parsing and tool invocation reuse the existing
CLI bridge infrastructure.
"""

from __future__ import annotations

from collections.abc import Callable, Coroutine
from typing import Any

from just_bash import CommandContext, ExecResult

from mcp_compressor.cli_tools import (
    ToolLike,
    format_tool_help,
    format_top_level_help,
    parse_argv_to_tool_input,
    tool_name_to_subcommand,
)

# Type alias matching the invoke signature in CompressedTools.invoke_tool
InvokeFn = Callable[[str, dict[str, Any] | None, bool], Coroutine[Any, Any, Any]]


class McpServerCommand:
    """A just-bash command that dispatches subcommands to MCP backend tools.

    Acts as a parent command (e.g. ``alpha``) with subcommands corresponding to
    individual MCP tools (e.g. ``alpha alpha-add --a 1 --b 2``).
    """

    def __init__(
        self,
        cli_name: str,
        server_description: str,
        tools: list[ToolLike],
        invoke_fn: InvokeFn,
    ) -> None:
        self.name = cli_name
        self._server_description = server_description
        self._tools = {tool.name: tool for tool in tools}
        self._subcommand_map = {tool_name_to_subcommand(name): name for name in self._tools}
        self._invoke_fn = invoke_fn

    async def execute(self, args: list[str], ctx: CommandContext) -> ExecResult:
        """Dispatch subcommands or return help."""
        if not args or args[0] in ("--help", "-h"):
            help_text = format_top_level_help(self.name, self._server_description, list(self._tools.values()))
            return ExecResult(stdout=help_text, stderr="", exit_code=0)

        subcommand = args[0]
        rest = args[1:]

        tool_name = self._subcommand_map.get(subcommand)
        if tool_name is None:
            help_text = format_top_level_help(self.name, self._server_description, list(self._tools.values()))
            return ExecResult(
                stdout="",
                stderr=f"{self.name}: unknown subcommand '{subcommand}'\n\n{help_text}",
                exit_code=1,
            )

        tool = self._tools[tool_name]

        if rest and rest[0] in ("--help", "-h"):
            return ExecResult(
                stdout=format_tool_help(self.name, tool),
                stderr="",
                exit_code=0,
            )

        try:
            tool_input = parse_argv_to_tool_input(rest, tool) if rest else {}
        except ValueError as exc:
            return ExecResult(
                stdout="",
                stderr=f"error: {exc}\n\n{format_tool_help(self.name, tool)}",
                exit_code=1,
            )

        try:
            result = await self._invoke_fn(tool_name, tool_input, False)
            # Extract text from ToolResult content blocks
            from mcp.types import TextContent

            output_parts: list[str] = []
            for content_block in result.content:
                if isinstance(content_block, TextContent):
                    output_parts.append(content_block.text)
                else:
                    output_parts.append(f"[{type(content_block).__name__} content]")
            return ExecResult(stdout="\n".join(output_parts), stderr="", exit_code=0)
        except Exception as exc:
            return ExecResult(stdout="", stderr=str(exc), exit_code=1)


def create_bash_command(
    cli_name: str,
    server_description: str,
    tools: list[ToolLike],
    invoke_fn: InvokeFn,
) -> McpServerCommand:
    """Create a just-bash parent command for a set of MCP backend tools.

    The command is named ``cli_name`` and dispatches subcommands that correspond to individual MCP tools.
    """
    return McpServerCommand(
        cli_name=tool_name_to_subcommand(cli_name),
        server_description=server_description,
        tools=tools,
        invoke_fn=invoke_fn,
    )


BASH_TOOL_DESCRIPTION = """\
Execute bash commands in a sandboxed environment (just-bash). \
Supports standard Unix utilities (grep, cat, jq, sed, awk, sort, find, and many more) \
as well as custom commands from connected MCP servers. \
See the help tools for available server commands and usage."""


def build_bash_tool_description(
    server_commands: list[dict[str, Any]],
) -> str:
    """Build the simple tool description for the bash tool.

    This returns a short, fixed description. Per-server help is provided
    via separate ``<server_name>_help`` tools rather than being embedded
    in the bash tool description.
    """
    return BASH_TOOL_DESCRIPTION
