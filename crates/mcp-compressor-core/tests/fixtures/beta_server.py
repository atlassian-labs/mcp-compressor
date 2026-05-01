from __future__ import annotations

import contextlib
from typing import Any

from fastmcp import FastMCP

mcp = FastMCP("Rust Core Beta Fixture")


@mcp.tool
def echo(message: str) -> str:
    """Echo a message from beta."""
    return f"beta:{message}"


@mcp.tool
def multiply(a: int, b: int) -> int:
    """Multiply two integers on beta."""
    return a * b


@mcp.tool
def structured_data() -> dict[str, Any]:
    """Return structured beta data."""
    return {"server": "beta", "values": [3, 4], "nested": {"ok": True}}


@mcp.resource("fixture://beta-resource")
def beta_resource() -> str:
    """Return a static beta resource."""
    return "beta resource"


@mcp.prompt
def beta_prompt() -> str:
    """Return a static beta prompt."""
    return "beta prompt"


if __name__ == "__main__":
    with contextlib.suppress(KeyboardInterrupt):
        mcp.run(show_banner=False)
