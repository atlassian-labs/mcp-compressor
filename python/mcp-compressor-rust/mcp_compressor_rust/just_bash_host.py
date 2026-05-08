from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from mcp_compressor_rust.client import CompressorProxy, JustBashCommand
from mcp_compressor_rust.core import ToolSpec, parse_tool_argv


@dataclass(frozen=True)
class JustBashCallableCommand:
    """Python-hosted callable equivalent of a Just Bash custom command."""

    provider_name: str
    command_name: str
    backend_tool_name: str
    help_tool_name: str
    command: JustBashCommand
    proxy: CompressorProxy

    def parse(self, args: list[str]) -> dict[str, Any]:
        return parse_tool_argv(
            ToolSpec(
                name=self.backend_tool_name,
                description=self.command.description,
                input_schema=self.command.input_schema,
            ),
            args,
        )

    def __call__(self, args: list[str] | None = None) -> str:
        return self.proxy.invoke(
            self.backend_tool_name,
            self.parse(args or []),
            server=self.provider_name,
        )


def create_just_bash_commands(proxy: CompressorProxy) -> list[JustBashCallableCommand]:
    """Create Python-hosted Just Bash command callables from a compressor proxy.

    This mirrors the TypeScript `createJustBashCommands` helper: Rust owns the
    compressed proxy and provider metadata, while the language host decides how
    to register and execute commands.
    """
    raw_names = [command.command_name for provider in proxy.just_bash_providers for command in provider.tools]
    duplicate_names = {name for name in raw_names if raw_names.count(name) > 1}
    commands: list[JustBashCallableCommand] = []
    for provider in proxy.just_bash_providers:
        for command in provider.tools:
            command_name = (
                f"{provider.provider_name}_{command.command_name}"
                if command.command_name in duplicate_names
                else command.command_name
            )
            commands.append(
                JustBashCallableCommand(
                    provider_name=provider.provider_name,
                    command_name=command_name,
                    backend_tool_name=command.backend_tool_name,
                    help_tool_name=provider.help_tool_name,
                    command=command,
                    proxy=proxy,
                )
            )
    return commands
