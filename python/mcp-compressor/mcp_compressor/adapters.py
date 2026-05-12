from __future__ import annotations

from collections.abc import Callable, Mapping
from typing import Any, TypeVar

from mcp_compressor.client import ExecutableTool

TTool = TypeVar("TTool")
ToolFactory = Callable[[dict[str, Any]], TTool]


def to_ai_sdk_tools(
    tools: Mapping[str, ExecutableTool],
    *,
    tool: ToolFactory | None = None,
) -> dict[str, Any]:
    """Convert executable tools into AI-SDK-style tool definitions.

    The optional ``tool`` factory mirrors the TypeScript adapter: pass a framework helper
    if you want framework-specific wrappers, or omit it to get plain structural objects.
    """
    result: dict[str, Any] = {}
    for name, executable in tools.items():
        definition = {
            "description": executable.description,
            "input_schema": executable.input_schema,
            "execute": executable.execute,
        }
        result[name] = tool(definition) if tool else definition
    return result


def to_mastra_tools(tools: Mapping[str, ExecutableTool]) -> dict[str, dict[str, Any]]:
    """Convert executable tools into a Mastra-like structural tool map."""
    return {
        name: {
            "description": executable.description,
            "input_schema": executable.input_schema,
            "execute": executable.execute,
        }
        for name, executable in tools.items()
    }
