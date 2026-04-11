import contextlib

from fastmcp import FastMCP

mcp = FastMCP("Alpha E2E Server")


@mcp.tool
def alpha_echo(message: str) -> str:
    """Echo a message from alpha."""
    return f"alpha:{message}"


@mcp.tool
def alpha_add(a: int, b: int) -> int:
    """Add two integers on alpha."""
    return a + b


@mcp.tool
def alpha_object() -> dict[str, object]:
    """Return structured alpha data."""
    return {"server": "alpha", "values": [1, 2]}


@mcp.resource("e2e://alpha-resource")
def alpha_resource() -> str:
    return "alpha resource"


@mcp.prompt
def alpha_prompt() -> str:
    return "alpha prompt"


if __name__ == "__main__":
    with contextlib.suppress(KeyboardInterrupt):
        mcp.run(show_banner=False)
