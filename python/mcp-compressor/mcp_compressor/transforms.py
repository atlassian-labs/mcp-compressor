from __future__ import annotations

from collections.abc import Callable, MutableMapping
from dataclasses import dataclass
from typing import Any

from mcp_compressor.client import ExecutableTool
from mcp_compressor.core import ToolSpec, parse_tool_argv


@dataclass(frozen=True)
class JustBashLocalCommand:
    """Callable command backed directly by a Python executable tool."""

    provider_name: str
    command_name: str
    backend_tool_name: str
    input_schema: dict[str, Any]
    execute: Callable[[dict[str, Any] | None], str]

    def parse(self, args: list[str]) -> dict[str, Any]:
        return parse_tool_argv(
            ToolSpec(name=self.backend_tool_name, description=None, input_schema=self.input_schema),
            args,
        )

    def __call__(self, args: list[str] | None = None) -> str:
        return self.execute(self.parse(args or []))


@dataclass(frozen=True)
class JustBashTransformResult:
    tools: dict[str, ExecutableTool]
    registrations: list[JustBashLocalCommand]


def transform_tools_for_just_bash(
    tools: dict[str, ExecutableTool],
    *,
    bash: Any,
    server_name: str = "tools",
) -> JustBashTransformResult:
    """Install executable tools as direct Just Bash commands and return help tools.

    The tools execute in-process; no mcp-compressor proxy bridge is created.
    """
    normalized = _normalize_server_name(server_name)
    registrations = [
        JustBashLocalCommand(
            provider_name=normalized,
            command_name=f"{normalized}_{name}",
            backend_tool_name=name,
            input_schema=tool.input_schema,
            execute=tool.execute,
        )
        for name, tool in tools.items()
    ]
    _install_commands(bash, registrations)
    return JustBashTransformResult(
        tools={
            f"{normalized}_help": ExecutableTool(
                name=f"{normalized}_help",
                description=f"Show help for Just Bash commands generated from {normalized}.",
                input_schema={"type": "object", "properties": {}},
                execute=lambda _input=None: "\n".join(
                    [
                        f"Backend tools have been installed as Just Bash commands for {normalized}.",
                        "",
                        *[f"- {command.command_name}" for command in registrations],
                    ]
                ),
            )
        },
        registrations=registrations,
    )


def _install_commands(bash: Any, commands: list[JustBashLocalCommand]) -> None:
    for attribute in ("custom_commands", "commands"):
        target = getattr(bash, attribute, None)
        if isinstance(target, MutableMapping):
            target.update({command.command_name: command for command in commands})
            return
        if isinstance(target, list):
            target.extend(commands)
            return
    bash.custom_commands = {command.command_name: command for command in commands}


def _normalize_server_name(name: str) -> str:
    normalized = "_".join(
        part
        for part in "".join(char if char.isalnum() or char == "_" else "_" for char in name).lower().split("_")
        if part
    )
    if not normalized:
        return "tools"
    if normalized[0].isdigit():
        return f"server_{normalized}"
    return normalized
