from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Literal

from mcp_compressor.client import ExecutableTool
from mcp_compressor.core import ToolSpec, build_host_transform_plan
from mcp_compressor.just_bash_host import JustBashServerCommand, JustBashSubcommand, install_commands


@dataclass(frozen=True)
class JustBashTransformResult:
    tools: dict[str, ExecutableTool]
    registrations: list[JustBashServerCommand]


def transform_tools_for_just_bash(
    tools: dict[str, ExecutableTool],
    *,
    bash: Any,
    server_name: str = "tools",
    help_mode: Literal["full", "summary"] = "full",
    help_summary: str | None = None,
) -> JustBashTransformResult:
    """Install executable tools as one Just Bash command and return its help tool.

    `server_name` becomes the command; each tool is a subcommand, exactly like
    the TypeScript transform and generated CLIs (`alpha echo --message hi`).
    The tools execute in-process; no mcp-compressor proxy bridge is created.
    """
    normalized = _normalize_server_name(server_name)
    specs = [
        ToolSpec(name=name, description=tool.description, input_schema=tool.input_schema)
        for name, tool in tools.items()
    ]
    plan = build_host_transform_plan("just-bash", normalized, specs, help_mode=help_mode, help_summary=help_summary)
    just_bash = plan["justBash"]

    def invoke(tool_name: str, tool_input: dict[str, Any]) -> str:
        return tools[tool_name].execute(tool_input)

    command = JustBashServerCommand(
        provider_name=normalized,
        command_name=str(just_bash["commandName"]),
        help_tool_name=str(plan["helpToolName"]),
        subcommands=[
            JustBashSubcommand(
                name=str(entry["commandName"]),
                backend_tool_name=str(entry["backendToolName"]),
                tool=ToolSpec(
                    name=str(entry["backendToolName"]),
                    description=entry.get("description"),
                    input_schema=entry["inputSchema"],
                ),
            )
            for entry in just_bash["commands"]
        ],
        invoke=invoke,
    )
    install_commands(bash, [command])
    help_text = str(plan["helpText"])
    return JustBashTransformResult(
        tools={
            command.help_tool_name: ExecutableTool(
                name=command.help_tool_name,
                description=str(plan["helpDescription"]),
                input_schema={"type": "object", "properties": {}},
                execute=lambda _input=None: help_text,
            )
        },
        registrations=[command],
    )


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
