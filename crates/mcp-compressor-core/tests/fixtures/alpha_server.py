from __future__ import annotations

import contextlib

from fastmcp import FastMCP

mcp = FastMCP("Rust Core Alpha Fixture")


@mcp.tool
def echo(message: str) -> str:
    """Echo a message from alpha."""
    return f"alpha:{message}"


@mcp.tool
def add(a: int, b: int) -> int:
    """Add two integers on alpha."""
    return a + b


@mcp.tool
def object() -> dict[str, object]:  # noqa: A001
    """Return structured alpha data."""
    return {"server": "alpha", "values": [1, 2], "nested": {"ok": True}}


@mcp.resource("fixture://alpha-resource")
def alpha_resource() -> str:
    """Return a static alpha resource."""
    return "alpha resource"


@mcp.prompt
def alpha_prompt() -> str:
    """Return a static alpha prompt."""
    return "alpha prompt"


if __name__ == "__main__":
    with contextlib.suppress(KeyboardInterrupt):
        mcp.run(show_banner=False)
