from __future__ import annotations

from fastmcp import FastMCP
from fastmcp.exceptions import ToolError

mcp = FastMCP("Recovery fixture")
calls = 0


@mcp.tool
def records(marker: str = "one", full: bool = False) -> dict:
    """Count actual executions and expose a backend-owned full parameter."""
    global calls
    calls += 1
    return {
        "calls": calls,
        "marker": marker,
        "backend_full": full,
        "data": {"items": [{"id": i, "description": "record details " * 150} for i in range(30)]},
    }


@mcp.tool
def execution_count() -> int:
    return calls


@mcp.tool
def error_response() -> str:
    raise ToolError("backend failure")


if __name__ == "__main__":
    mcp.run()
